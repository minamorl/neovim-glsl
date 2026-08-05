//! The server side of the Neovim protocol.
//!
//! This is the half `pin architecture_choice` names: the host sits where Neovim
//! used to sit, and anything that speaks msgpack-RPC to it — including this
//! program's own renderer — is a UI client. The renderer talks to the core
//! through a pipe and real msgpack rather than a function call, so there is one
//! code path and no privileged in-process shortcut that the protocol could
//! silently diverge from.

use std::io::{BufReader, Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};

use rmpv::Value;

use crate::core::editor::{Request, Scope};
use crate::core::{Buffer, Editor, Mode};
use crate::notes::{self, Vault};
use crate::nvim::{self, RedrawEvent, UiOptions};

use super::paint::{Painter, Theme};

/// Notifications the host sends that are not `redraw`. A UI client that does
/// not understand them ignores them, which is the same latitude Neovim gives a
/// client for `vim.rpcnotify`.
pub const NAVIGATE: &str = "nvimglsl_navigate";
pub const QUIT: &str = "nvimglsl_quit";
/// A plugin-registered Ex command reached the editor; the client owns the
/// plugin host, so the name and its argument travel out as an ordinary
/// notification.
pub const PLUGIN: &str = "nvimglsl_plugin";

/// Work the protocol server can receive without blocking on client input.
///
/// Only `Client` is produced today. The other variant is the slot later LSP,
/// git or task output can use to ask the host to flush state pushed from
/// outside the UI client's msgpack stream.
#[derive(Debug)]
pub enum Incoming {
    Client(Value),
    Flush,
}

pub struct Host {
    pub editor: Editor,
    /// The yui note vault. `pin note_substrate` makes it the substrate rather
    /// than something this host owns, so it is opened, never created.
    pub vault: Vault,
    theme: Theme,
    painter: Option<Painter>,
    channel: u64,
    pub quit: bool,
    /// The first paint, held between `nvim_ui_attach` returning its response and
    /// the caller sending traffic. It is kept apart so the response reaches the
    /// client before the redraw that answers it.
    pending_attach: Option<Vec<RedrawEvent>>,
}

impl Host {
    pub fn new(editor: Editor) -> Self {
        Self::with_vault(editor, Vault::default_vault())
    }

    pub fn with_vault(editor: Editor, vault: Vault) -> Self {
        Self::themed(editor, vault, Theme::dark())
    }

    /// A host whose editor behaves the way the owner's `init.lua` says.
    pub fn configured_with_plugins(
        vault: Vault,
        theme: Theme,
        plugin_commands: Vec<String>,
    ) -> Self {
        let mut host = Self::configured(vault, theme);
        host.editor.set_plugin_commands(plugin_commands);
        host
    }

    pub fn configured(vault: Vault, theme: Theme) -> Self {
        let config = crate::luaconf::load_default();
        if let Some(error) = &config.error {
            eprintln!("nvim config: {error}");
        }
        if let Some(path) = &config.path {
            eprintln!(
                "nvim config: {} — {} options, {} mappings",
                path.display(),
                config.options.len(),
                config.mappings.len()
            );
        }
        Self::themed(Editor::with_config(Buffer::empty(), &config), vault, theme)
    }

    pub fn themed(editor: Editor, vault: Vault, theme: Theme) -> Self {
        Self {
            editor,
            vault,
            theme,
            painter: None,
            channel: 1,
            quit: false,
            pending_attach: None,
        }
    }

    fn report(&mut self, text: String, error: bool) {
        self.editor.message = Some(crate::core::Message { text, error });
    }

    fn open_path(&mut self, path: std::path::PathBuf) {
        if let Err(error) = self.editor.open(path.clone()) {
            self.report(
                format!("E484: Can't open file {}: {error}", path.display()),
                true,
            );
        }
    }

    pub fn attached(&self) -> bool {
        self.painter.is_some()
    }

    /// Handle one incoming message, returning everything to send back.
    pub fn handle(&mut self, message: &Value) -> Vec<Value> {
        let Some(parts) = message.as_array() else {
            return Vec::new();
        };
        match parts.first().and_then(Value::as_u64) {
            Some(nvim::REQUEST) if parts.len() == 4 => {
                let msgid = parts[1].as_u64().unwrap_or(0);
                let method = parts[2].as_str().unwrap_or("");
                let args = parts[3].as_array().cloned().unwrap_or_default();
                let (error, result) = self.call(method, &args);
                let mut out = vec![nvim::response(msgid, error, result)];
                out.extend(self.flush_ui());
                out
            }
            Some(nvim::NOTIFY) if parts.len() == 3 => {
                let method = parts[1].as_str().unwrap_or("");
                let args = parts[2].as_array().cloned().unwrap_or_default();
                self.call(method, &args);
                self.flush_ui()
            }
            // A response to something we sent. The host asks the client for
            // nothing, so there is nothing to correlate.
            _ => Vec::new(),
        }
    }

    /// Redraw traffic plus any host notification the last call produced.
    pub fn flush_ui(&mut self) -> Vec<Value> {
        let mut out = Vec::new();
        for request in std::mem::take(&mut self.editor.requests) {
            match request {
                Request::Quit => {
                    self.quit = true;
                    out.push(nvim::notification(QUIT, vec![]));
                }
                Request::OpenNavigation(scope) => out.push(nvim::notification(
                    NAVIGATE,
                    vec![Value::from(match scope {
                        Scope::Notes => "notes",
                        Scope::Files => "files",
                    })],
                )),
                Request::Plugin { name, argument } => out.push(nvim::notification(
                    PLUGIN,
                    vec![Value::from(name), Value::from(argument)],
                )),
                Request::Edit(path) => self.open_path(path),
                Request::NewNote(title) => match self.vault.create(&title) {
                    Ok(path) => {
                        self.open_path(path.clone());
                        self.report(format!("\"{}\" note", path.display()), false);
                    }
                    Err(error) => self.report(format!("E212: {error}"), true),
                },
                Request::FollowLink => {
                    let (line, column) = self.editor.cursor;
                    let text = self.editor.buffer.line_text(line);
                    match notes::link_at(&text, column) {
                        Some(link) => match self.vault.resolve_link(&link) {
                            Some(relative) => {
                                let path = self.vault.path_of(&relative);
                                self.open_path(path);
                            }
                            // A link to a note that does not exist yet is a
                            // normal state in a vault, so it is reported rather
                            // than created behind the owner's back.
                            None => self.report(format!("E447: no note for [[{link}]]"), true),
                        },
                        None => self.report("E446: no link under cursor".into(), true),
                    }
                }
            }
        }
        if let Some(events) = self.render() {
            out.push(nvim::pack_redraw(&events));
        }
        out
    }

    fn render(&mut self) -> Option<Vec<RedrawEvent>> {
        let painter = self.painter.as_mut()?;
        self.editor.set_screen(painter.cols(), painter.text_rows());
        let events = painter.render(&self.editor);
        // A render with nothing but a cursor move and a flush is still traffic
        // worth sending; a render with neither is not.
        if events.len() <= 1 {
            return None;
        }
        Some(events)
    }

    fn call(&mut self, method: &str, args: &[Value]) -> (Option<Value>, Value) {
        match method {
            "nvim_get_api_info" => (
                None,
                Value::Array(vec![
                    Value::from(self.channel),
                    Value::Map(vec![
                        (
                            Value::from("version"),
                            Value::Map(vec![
                                (Value::from("major"), Value::from(0u64)),
                                (Value::from("minor"), Value::from(11u64)),
                                (Value::from("patch"), Value::from(0u64)),
                                (Value::from("api_level"), Value::from(12u64)),
                            ]),
                        ),
                        (
                            Value::from("host"),
                            Value::Map(vec![
                                (Value::from("name"), Value::from("nvimglsl")),
                                (Value::from("protocol_face"), Value::from("ui")),
                            ]),
                        ),
                    ]),
                ]),
            ),
            "nvim_ui_attach" => {
                let cols = args.first().and_then(Value::as_u64).unwrap_or(80) as usize;
                let rows = args.get(1).and_then(Value::as_u64).unwrap_or(24) as usize;
                let options = args
                    .get(2)
                    .and_then(Value::as_map)
                    .map(|map| UiOptions::from_map(map))
                    .unwrap_or_else(UiOptions::none);
                self.attach(cols, rows, options);
                (None, Value::Nil)
            }
            "nvim_ui_detach" => {
                self.painter = None;
                (None, Value::Nil)
            }
            "nvim_ui_try_resize" => {
                let cols = args.first().and_then(Value::as_u64).unwrap_or(80) as usize;
                let rows = args.get(1).and_then(Value::as_u64).unwrap_or(24) as usize;
                if let Some(painter) = &mut self.painter {
                    painter.resize(cols, rows);
                    self.editor.set_screen(painter.cols(), painter.text_rows());
                }
                (None, Value::Nil)
            }
            "nvim_input" => {
                let keys = args.first().and_then(Value::as_str).unwrap_or("");
                self.editor.feed_str(keys);
                (None, Value::from(keys.len() as u64))
            }
            "nvim_command" | "nvim_exec2" => {
                let command = args.first().and_then(Value::as_str).unwrap_or("");
                crate::core::command::execute(&mut self.editor, command);
                (None, Value::Nil)
            }
            "nvim_get_mode" => (
                None,
                Value::Map(vec![
                    (
                        Value::from("mode"),
                        Value::from(self.editor.mode.short_name()),
                    ),
                    (
                        Value::from("blocking"),
                        Value::from(self.editor.mode == Mode::Cmdline),
                    ),
                ]),
            ),
            "nvim_get_current_buf" => (None, Value::from(1u64)),
            "nvim_buf_line_count" => (None, Value::from(self.editor.buffer.line_count() as u64)),
            "nvim_buf_get_name" => (None, Value::from(self.editor.buffer.name())),
            "nvim_buf_get_lines" => {
                let count = self.editor.buffer.line_count();
                let start = index(args.get(1), count, 0);
                let end = index(args.get(2), count, count);
                let lines = self
                    .editor
                    .buffer
                    .lines_text(start, end)
                    .into_iter()
                    .map(Value::from)
                    .collect();
                (None, Value::Array(lines))
            }
            "nvim_buf_set_lines" => {
                let count = self.editor.buffer.line_count();
                let start = index(args.get(1), count, 0);
                let end = index(args.get(2), count, count);
                let replacement = args
                    .get(4)
                    .and_then(Value::as_array)
                    .map(|lines| {
                        lines
                            .iter()
                            .map(|l| l.as_str().unwrap_or("").to_string())
                            .collect()
                    })
                    .unwrap_or_default();
                self.editor.buffer.begin_change(self.editor.cursor);
                self.editor.buffer.splice_lines(start, end, replacement);
                self.editor.buffer.commit_change();
                (None, Value::Nil)
            }
            "nvim_set_client_info" | "nvim_ui_set_option" | "nvim_input_mouse" => {
                (None, Value::Nil)
            }
            // `free lua_runtime_presence` leaves a Lua runtime optional and this
            // host does not embed one. Saying so is not the same as ignoring the
            // call: a client that gets `nil` back would believe its code ran.
            "nvim_exec_lua" => (
                Some(Value::from(
                    "nvimglsl has no Lua runtime; open_question neovim_asset_reuse_scope",
                )),
                Value::Nil,
            ),
            other => (
                Some(Value::from(format!("unknown method: {other}"))),
                Value::Nil,
            ),
        }
    }

    fn attach(&mut self, cols: usize, rows: usize, options: UiOptions) {
        let mut painter = Painter::themed(cols, rows, options, self.theme);
        self.editor.set_screen(painter.cols(), painter.text_rows());
        let mut events = painter.attach_events();
        events.extend(painter.render(&self.editor));
        self.painter = Some(painter);
        self.pending_attach = Some(events);
    }

    /// The events produced by the last `nvim_ui_attach`, if any.
    pub fn take_attach_events(&mut self) -> Option<Vec<RedrawEvent>> {
        self.pending_attach.take()
    }
}

/// A line index as the buffer API states them: zero-based, end-exclusive, and a
/// negative index counted as `length + 1 + index` — so `-1` is the end of the
/// buffer rather than its last line.
fn index(value: Option<&Value>, count: usize, default: usize) -> usize {
    match value.and_then(Value::as_i64) {
        Some(raw) if raw < 0 => (count as i64 + 1 + raw).clamp(0, count as i64) as usize,
        Some(raw) => (raw as usize).min(count),
        None => default,
    }
}

/// Run the protocol over a reader/writer pair until the peer closes it.
///
/// This is the same loop whether the peer is this process's renderer through a
/// pipe or another program through stdio, which is what makes "speaks the
/// Neovim protocol" checkable from outside.
pub fn serve(
    host: &mut Host,
    input: impl Read + Send + 'static,
    output: impl Write,
    initial_path: Option<PathBuf>,
) -> std::io::Result<()> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("nvimglsl-proto-reader".into())
        .spawn(move || {
            let mut reader = BufReader::new(input);
            while let Ok(message) = nvim::read_message(&mut reader) {
                if tx.send(Incoming::Client(message)).is_err() {
                    return;
                }
            }
        })?;

    serve_incoming(host, rx, output, initial_path)
}

pub fn serve_incoming(
    host: &mut Host,
    incoming: Receiver<Incoming>,
    mut output: impl Write,
    initial_path: Option<PathBuf>,
) -> std::io::Result<()> {
    if let Some(path) = initial_path {
        let _ = host.editor.open(path);
    }
    loop {
        let outgoing = match incoming.recv() {
            Ok(Incoming::Client(message)) => host.handle(&message),
            Ok(Incoming::Flush) => host.flush_ui(),
            // A closed channel is how a UI client leaves.
            Err(_) => return Ok(()),
        };
        for message in outgoing {
            nvim::write_message(&mut output, &message)?;
        }
        if let Some(events) = host.take_attach_events() {
            nvim::write_message(&mut output, &nvim::pack_redraw(&events))?;
        }
        if host.quit {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Buffer;

    fn request(id: u64, method: &str, args: Vec<Value>) -> Value {
        Value::Array(vec![
            Value::from(nvim::REQUEST),
            Value::from(id),
            Value::from(method),
            Value::Array(args),
        ])
    }

    fn attached_host(text: &str) -> Host {
        let mut host = Host::new(Editor::new(Buffer::from_text(text)));
        host.handle(&request(
            1,
            "nvim_ui_attach",
            vec![
                Value::from(40u64),
                Value::from(8u64),
                UiOptions::none().to_map(),
            ],
        ));
        host.take_attach_events();
        host
    }

    fn redraw_names(messages: &[Value]) -> Vec<String> {
        messages
            .iter()
            .flat_map(|m| nvim::split_notification(m).0)
            .map(|(name, _)| name)
            .collect()
    }

    #[test]
    fn attaching_produces_a_full_first_paint() {
        let mut host = Host::new(Editor::new(Buffer::from_text("hello\n")));
        host.handle(&request(
            1,
            "nvim_ui_attach",
            vec![
                Value::from(40u64),
                Value::from(8u64),
                UiOptions::none().to_map(),
            ],
        ));
        let events = host.take_attach_events().expect("attach events");
        let names: Vec<&str> = events.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"grid_resize"));
        assert!(names.contains(&"default_colors_set"));
        assert!(names.contains(&"hl_attr_define"));
        assert!(names.contains(&"grid_line"));
        assert_eq!(names.last(), Some(&"flush"));
    }

    #[test]
    fn typing_over_the_protocol_changes_the_buffer_and_redraws() {
        let mut host = attached_host("");
        let out = host.handle(&Value::Array(vec![
            Value::from(nvim::NOTIFY),
            Value::from("nvim_input"),
            Value::Array(vec![Value::from("ihello<Esc>")]),
        ]));
        assert_eq!(host.editor.buffer.line_text(0), "hello");
        assert!(redraw_names(&out).contains(&"grid_line".to_string()));
    }

    #[test]
    fn the_api_info_reports_a_channel_and_a_version() {
        let mut host = Host::new(Editor::default());
        let out = host.handle(&request(7, "nvim_get_api_info", vec![]));
        let parts = out[0].as_array().unwrap();
        assert_eq!(parts[0].as_u64(), Some(nvim::RESPONSE));
        assert_eq!(parts[1].as_u64(), Some(7));
        assert!(parts[2].is_nil());
        assert_eq!(parts[3].as_array().unwrap()[0].as_u64(), Some(1));
    }

    #[test]
    fn buffer_lines_can_be_read_and_written_over_the_api_face() {
        let mut host = attached_host("one\ntwo\n");
        let out = host.handle(&request(
            2,
            "nvim_buf_get_lines",
            vec![
                Value::from(0u64),
                Value::from(0u64),
                Value::from(-1i64),
                Value::from(false),
            ],
        ));
        let lines = out[0].as_array().unwrap()[3].as_array().unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].as_str(), Some("one"));

        host.handle(&request(
            3,
            "nvim_buf_set_lines",
            vec![
                Value::from(0u64),
                Value::from(0u64),
                Value::from(1i64),
                Value::from(false),
                Value::Array(vec![Value::from("ONE")]),
            ],
        ));
        assert_eq!(host.editor.buffer.line_text(0), "ONE");
    }

    #[test]
    fn exec_lua_reports_that_there_is_no_lua_rather_than_succeeding_silently() {
        let mut host = Host::new(Editor::default());
        let out = host.handle(&request(4, "nvim_exec_lua", vec![Value::from("return 1")]));
        let error = &out[0].as_array().unwrap()[2];
        assert!(error.as_str().unwrap().contains("no Lua runtime"));
    }

    fn vault_host(text: &str, dir: &std::path::Path) -> Host {
        let mut host = Host::with_vault(
            Editor::new(Buffer::from_text(text)),
            Vault::open(dir.to_path_buf()),
        );
        host.handle(&request(
            1,
            "nvim_ui_attach",
            vec![
                Value::from(40u64),
                Value::from(8u64),
                UiOptions::none().to_map(),
            ],
        ));
        host.take_attach_events();
        host
    }

    fn input(keys: &str) -> Value {
        Value::Array(vec![
            Value::from(nvim::NOTIFY),
            Value::from("nvim_input"),
            Value::Array(vec![Value::from(keys)]),
        ])
    }

    #[test]
    fn a_new_note_is_created_in_the_vault_and_opened() {
        let dir = std::env::temp_dir().join("nvimglsl-host-note");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut host = vault_host("", &dir);
        host.handle(&input(":Note idea<CR>"));
        assert_eq!(host.editor.buffer.line_text(0), "# idea");
        assert!(dir.join("idea.md").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn gf_follows_a_wiki_link_into_the_vault() {
        let dir = std::env::temp_dir().join("nvimglsl-host-link");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("target.md"), "# target\n").unwrap();
        let mut host = vault_host("see [[target]]\n", &dir);
        // `gf` acts on the link under the cursor, so the cursor is put on it
        // first — starting at column 0 there is no link, which is its own case.
        host.handle(&input("f[gf"));
        assert_eq!(host.editor.buffer.line_text(0), "# target");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn gf_off_a_link_says_there_is_none_rather_than_guessing() {
        let dir = std::env::temp_dir().join("nvimglsl-host-nolink");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut host = vault_host("see [[target]]\n", &dir);
        host.handle(&input("gf"));
        assert!(host
            .editor
            .message
            .as_ref()
            .unwrap()
            .text
            .contains("no link under cursor"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_link_with_no_note_is_reported_rather_than_created() {
        let dir = std::env::temp_dir().join("nvimglsl-host-missing");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut host = vault_host("see [[absent]]\n", &dir);
        host.handle(&input("f[gf"));
        assert!(host.editor.message.as_ref().unwrap().error);
        assert!(!dir.join("absent.md").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_navigation_notification_carries_which_scope_was_asked_for() {
        let mut host = attached_host("");
        let out = host.handle(&input(" f"));
        let scope = out
            .iter()
            .filter_map(|m| m.as_array())
            .find(|p| p.get(1).and_then(Value::as_str) == Some(NAVIGATE))
            .and_then(|p| p[2].as_array())
            .and_then(|params| params.first())
            .and_then(Value::as_str);
        assert_eq!(scope, Some("files"));
    }

    #[test]
    fn the_leader_mapping_reaches_the_client_as_a_notification() {
        let mut host = attached_host("");
        let out = host.handle(&Value::Array(vec![
            Value::from(nvim::NOTIFY),
            Value::from("nvim_input"),
            Value::Array(vec![Value::from(" o")]),
        ]));
        let names: Vec<String> = out
            .iter()
            .filter_map(|m| m.as_array())
            .filter_map(|p| p.get(1).and_then(Value::as_str))
            .map(str::to_string)
            .collect();
        assert!(names.contains(&NAVIGATE.to_string()));
    }

    #[test]
    fn quitting_is_announced_and_ends_the_session() {
        let mut host = attached_host("");
        host.handle(&Value::Array(vec![
            Value::from(nvim::NOTIFY),
            Value::from("nvim_input"),
            Value::Array(vec![Value::from(":q<CR>")]),
        ]));
        assert!(host.quit);
    }

    #[test]
    fn a_resize_changes_what_the_grid_reports() {
        let mut host = attached_host("one\n");
        host.handle(&request(
            5,
            "nvim_ui_try_resize",
            vec![Value::from(20u64), Value::from(5u64)],
        ));
        assert_eq!(host.editor.view_rows, 3);
    }
}
