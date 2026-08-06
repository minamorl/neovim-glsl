//! nvimglsl — an own host that speaks the Neovim protocol, drawn in GLSL.
//!
//! One process, two halves that only meet over msgpack: the editing core and
//! its protocol server on one side, a UI client and the GLSL renderer on the
//! other. They are joined by a real pipe, so the protocol face is the same one
//! an outside client gets through `--embed`, and cannot quietly drift into a
//! function call that only this program knows how to make.

mod clipboard;
mod core;
mod git;
mod ignore;
mod grep;
mod keymap;
mod lsp;
mod luaconf;
mod notes;
mod nvim;
mod picker;
mod plugin;
mod project;
mod proto;
mod root_ui;
mod run;
mod tategaki;
mod textpos;
mod tree;
mod workspace;

// The grid renderer is the measured candidate's, reached by path rather than
// copied. `pin neovim_asset_not_discarded` forbids throwing the Neovim-derived
// work away, and `open_question embed_candidate_disposition` has not chosen
// between keeping the candidate and discarding it — including its files leaves
// that artefact exactly as it was measured.
#[path = "../../evaluation/candidate-embed-opengl/src/cmap.rs"]
mod cmap;

#[path = "../../evaluation/candidate-embed-opengl/src/ext_ui.rs"]
mod ext_ui;
#[path = "../../evaluation/candidate-embed-opengl/src/gl.rs"]
mod gl;
#[path = "../../evaluation/candidate-embed-opengl/src/grid.rs"]
mod grid;
#[path = "../../evaluation/candidate-embed-opengl/src/panel.rs"]
mod panel;
#[path = "../../evaluation/candidate-embed-opengl/src/screen.rs"]
mod screen;
#[path = "../../evaluation/candidate-embed-opengl/src/text.rs"]
mod text;
// `perf` is this crate's own: the candidate's version carries tests that read
// evidence files relative to its manifest, and those belong to that artefact
// rather than to the product's test run.
mod perf;
#[path = "../../evaluation/candidate-embed-opengl/src/picker_state.rs"]
mod picker_state;

use std::io::PipeWriter;
use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

use glutin::config::ConfigTemplateBuilder;
use glutin::context::{
    ContextApi, ContextAttributesBuilder, GlProfile, PossiblyCurrentContext, Version,
};
use glutin::display::GetGlDisplay;
use glutin::prelude::*;
use glutin::surface::{Surface, SurfaceAttributesBuilder, WindowSurface};
use glutin_winit::{DisplayBuilder, GlWindow};
use raw_window_handle::HasWindowHandle;
use rmpv::Value;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, Ime, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowId};


/// What an IME event asks the window to do, decided away from the window so it
/// can be checked without one.
///
/// A real composition needs a real input method and a real window, and neither
/// fits in a test. What does fit is the shape of the decision, and one detail
/// inside it that is easy to get wrong: a commit is fed into the same string
/// that carries key notation, so a literal `<` arriving from an input method
/// would open a `<C-x>`-shaped token nobody typed.
#[derive(Debug, PartialEq, Eq)]
enum ImeAction {
    /// Replace the composition being shown. An empty string ends it.
    Compose(String),
    /// Send these keys to whatever has focus.
    Commit(String),
    Nothing,
}

fn ime_action(event: &Ime) -> ImeAction {
    match event {
        Ime::Preedit(text, _) => ImeAction::Compose(text.clone()),
        Ime::Commit(text) => ImeAction::Commit(text.replace('<', "<lt>")),
        // A composition that is abandoned leaves nothing on screen.
        Ime::Disabled => ImeAction::Compose(String::new()),
        Ime::Enabled => ImeAction::Nothing,
    }
}

// ---------------------------------------------------------------------------
// Arguments
// ---------------------------------------------------------------------------

struct Args {
    file: Option<PathBuf>,
    cols: usize,
    rows: usize,
    font_size: f32,
    scheme: String,
    /// Whether `--scheme` was actually passed. The grid has a default scheme;
    /// the vertical preview does not inherit it, because a reading view is
    /// paper the way a book is, not dark the way a terminal is.
    scheme_given: bool,
    snapshot: Option<String>,
    input: Option<String>,
    /// Serve the protocol on stdin/stdout instead of opening a window, the way
    /// `nvim --embed` does. This is what makes "speaks the Neovim protocol"
    /// checkable from outside this program.
    embed: bool,
    /// Set the note as a vertical page, write it here, and exit. No window and
    /// no GL, so it runs anywhere — including where the page is then rendered
    /// by something other than a browser on this machine.
    tategaki: Option<String>,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            file: None,
            cols: 100,
            rows: 30,
            font_size: 15.0,
            scheme: "dark".into(),
            scheme_given: false,
            snapshot: None,
            input: None,
            embed: false,
            tategaki: None,
        }
    }
}

fn parse_args_from(argv: Vec<String>) -> Args {
    let mut args = Args::default();
    let mut index = 0;
    while index < argv.len() {
        let take = |index: &mut usize| -> Option<String> {
            *index += 1;
            argv.get(*index).cloned()
        };
        match argv[index].as_str() {
            "--embed" => args.embed = true,
            "--cols" => {
                args.cols = take(&mut index)
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(args.cols)
            }
            "--rows" => {
                args.rows = take(&mut index)
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(args.rows)
            }
            "--font-size" => {
                args.font_size = take(&mut index)
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(args.font_size)
            }
            "--scheme" => {
                args.scheme = take(&mut index).unwrap_or(args.scheme);
                args.scheme_given = true;
            }
            "--snapshot" => args.snapshot = take(&mut index),
            "--tategaki" => args.tategaki = take(&mut index),
            "--input" => args.input = take(&mut index),
            other if !other.starts_with('-') && args.file.is_none() => {
                args.file = Some(PathBuf::from(other))
            }
            other => eprintln!("nvimglsl: ignoring unknown argument {other}"),
        }
        index += 1;
    }
    args
}

// ---------------------------------------------------------------------------
// The client side of the protocol
// ---------------------------------------------------------------------------

/// A UI client attached to the host over a pipe.
///
/// Deliberately the same shape a client attached over stdio would have. The
/// host runs on its own thread and neither half can reach into the other's
/// state; everything crossing between them is an encoded msgpack message.
struct Link {
    to_host: PipeWriter,
    queue: nvim::RedrawQueue,
    next_msgid: u64,
}

impl Link {
    fn spawn(
        initial: Option<PathBuf>,
        theme: proto::paint::Theme,
        plugin_commands: Vec<String>,
        preview_scheme: tategaki::Scheme,
    ) -> std::io::Result<Self> {
        let (host_input, to_host) = std::io::pipe()?;
        let (from_host, host_output) = std::io::pipe()?;

        std::thread::Builder::new()
            .name("nvimglsl-host".into())
            .spawn(move || {
                // The grid and the navigation surface take the same theme, so the
                // two halves of one window cannot disagree about it.
                let mut host = proto::Host::configured_with_plugins(
                    notes::Vault::default_vault(),
                    theme,
                    plugin_commands,
                );
                host.preview.scheme = preview_scheme;
                if let Err(error) = proto::serve(&mut host, host_input, host_output, initial) {
                    eprintln!("host: {error}");
                }
            })?;

        let (tx, rx) = channel();
        std::thread::Builder::new()
            .name("nvimglsl-reader".into())
            .spawn(move || {
                let mut reader = std::io::BufReader::new(from_host);
                while let Ok(value) = nvim::read_message(&mut reader) {
                    if tx.send(value).is_err() {
                        break;
                    }
                }
            })?;

        Ok(Self {
            to_host,
            queue: nvim::RedrawQueue::new(rx),
            next_msgid: 1,
        })
    }

    fn request(&mut self, method: &str, params: Vec<Value>) -> std::io::Result<()> {
        let msgid = self.next_msgid;
        self.next_msgid += 1;
        nvim::write_message(
            &mut self.to_host,
            &Value::Array(vec![
                Value::from(nvim::REQUEST),
                Value::from(msgid),
                Value::from(method),
                Value::Array(params),
            ]),
        )
    }

    fn notify(&mut self, method: &str, params: Vec<Value>) -> std::io::Result<()> {
        nvim::write_message(&mut self.to_host, &nvim::notification(method, params))
    }

    fn ui_attach(
        &mut self,
        cols: usize,
        rows: usize,
        options: nvim::UiOptions,
    ) -> std::io::Result<()> {
        self.request(
            "nvim_ui_attach",
            vec![
                Value::from(cols as u64),
                Value::from(rows as u64),
                options.to_map(),
            ],
        )
    }

    fn try_resize(&mut self, cols: usize, rows: usize) -> std::io::Result<()> {
        self.request(
            "nvim_ui_try_resize",
            vec![Value::from(cols as u64), Value::from(rows as u64)],
        )
    }

    fn input(&mut self, keys: &str) -> std::io::Result<()> {
        self.notify("nvim_input", vec![Value::from(keys)])
    }
}

// ---------------------------------------------------------------------------
// The application
// ---------------------------------------------------------------------------

enum Overlay {
    None,
    Picker {
        picker: picker::Picker,
        kind: PickerKind,
    },
    Grep(picker::GrepPicker),
    Output(OutputPanel),
    Plugin(String),
}

impl Overlay {
    fn is_open(&self) -> bool {
        !matches!(self, Self::None)
    }
}

#[derive(Clone, Copy)]
enum PickerKind {
    Vault,
    Files,
    Workspace,
}

struct OutputPanel {
    task: Option<run::Task>,
    argv: Vec<String>,
    cwd: PathBuf,
    lines: Vec<root_ui::output::LineInput>,
    status: Option<String>,
    scroll: usize,
}

impl OutputPanel {
    fn running(task: run::Task) -> Self {
        let argv = task.argv.clone();
        let cwd = task.cwd.clone();
        Self {
            task: Some(task),
            argv,
            cwd,
            lines: vec![root_ui::output::LineInput::default()],
            status: Some("running".into()),
            scroll: 0,
        }
    }

    fn message(argv: Vec<String>, cwd: PathBuf, message: String) -> Self {
        let mut panel = Self {
            task: None,
            argv,
            cwd,
            lines: vec![root_ui::output::LineInput::default()],
            status: Some(message),
            scroll: 0,
        };
        panel.append(vec![run::Segment {
            text: panel.status.clone().unwrap_or_default(),
            role: run::Role {
                stream: run::Stream::Stderr,
                color: None,
                bold: false,
            },
        }]);
        panel
    }

    fn poll(&mut self) -> bool {
        let Some(task) = self.task.as_mut() else {
            return false;
        };
        let mut segments = task.poll();
        let status = task.status();
        // `status()` drains the pipes on its way to the exit code, and a command
        // fast enough to finish inside one tick has *all* of its output in that
        // batch. Taking it back is what stops `python3 hello.py` from painting
        // an empty panel above a clean `exit status 0`.
        segments.extend(task.poll());
        let changed = !segments.is_empty();
        self.append(segments);
        if let Some(status) = status {
            self.status = Some(match status.code() {
                Some(code) => format!("exit status {code}"),
                None => "terminated by signal".into(),
            });
            self.task = None;
            return true;
        }
        changed
    }

    fn cancel(&mut self) {
        if let Some(task) = self.task.as_mut() {
            task.cancel();
            self.status = Some("cancelled".into());
        }
        self.task = None;
    }

    fn append(&mut self, segments: Vec<run::Segment>) {
        for segment in segments {
            let mut rest = segment.text.as_str();
            loop {
                if let Some(at) = rest.find('\n') {
                    let (head, tail) = rest.split_at(at);
                    if !head.is_empty() {
                        self.current_line()
                            .segments
                            .push(root_ui::output::SegmentInput {
                                text: head.to_string(),
                                role: segment.role,
                            });
                    }
                    self.lines.push(root_ui::output::LineInput::default());
                    rest = &tail[1..];
                } else {
                    if !rest.is_empty() {
                        self.current_line()
                            .segments
                            .push(root_ui::output::SegmentInput {
                                text: rest.to_string(),
                                role: segment.role,
                            });
                    }
                    break;
                }
            }
        }
        if self.lines.len() > 20_000 {
            let drop = self.lines.len() - 20_000;
            self.lines.drain(0..drop);
            self.scroll = self.scroll.saturating_sub(drop);
        }
        self.scroll = self.lines.len().saturating_sub(1);
    }

    fn current_line(&mut self) -> &mut root_ui::output::LineInput {
        if self.lines.is_empty() {
            self.lines.push(root_ui::output::LineInput::default());
        }
        self.lines.last_mut().unwrap()
    }

    fn feed(&mut self, keys: &str) -> bool {
        for key in crate::core::key::parse(keys) {
            use crate::core::key::{Code, Named};
            match key.code {
                Code::Named(Named::Esc) => return false,
                Code::Char('q') if !key.ctrl => return false,
                Code::Char('c') if key.ctrl => self.cancel(),
                Code::Named(Named::Up) | Code::Char('k') => {
                    self.scroll = self.scroll.saturating_sub(1)
                }
                Code::Named(Named::Down) | Code::Char('j') => {
                    self.scroll = (self.scroll + 1).min(self.lines.len().saturating_sub(1))
                }
                Code::Named(Named::PageUp) => self.scroll = self.scroll.saturating_sub(10),
                Code::Named(Named::PageDown) => {
                    self.scroll = (self.scroll + 10).min(self.lines.len().saturating_sub(1))
                }
                _ => {}
            }
        }
        true
    }
}

struct App {
    args: Args,
    started: bool,
    window: Option<Window>,
    context: Option<PossiblyCurrentContext>,
    surface: Option<Surface<WindowSurface>>,
    gl: Option<glow::Context>,
    renderer: Option<gl::Renderer>,
    adapter: Option<root_ui::adapter::Adapter>,
    atlas: Option<text::Atlas>,
    screen: Option<screen::Screen>,
    ext_ui: ext_ui::ExtUi,
    link: Option<Link>,
    overlay: Overlay,
    /// Plugins live on this side because a surface is a UI concern and this is
    /// the UI. The editor only learns their command *names*, so that a name
    /// belonging to no plugin can still report itself as unknown.
    plugins: plugin::Host,
    mods: ModifiersState,
    /// Physical pixels per density-independent pixel, from the display.
    scale: f32,
    preedit: String,
    last_stats: root_ui::adapter::AdapterStats,
}

impl App {
    fn new(args: Args) -> Self {
        Self {
            args,
            started: false,
            window: None,
            context: None,
            surface: None,
            gl: None,
            renderer: None,
            adapter: None,
            atlas: None,
            screen: None,
            ext_ui: ext_ui::ExtUi::new(),
            link: None,
            overlay: Overlay::None,
            plugins: plugin::Host::load_default(),
            mods: ModifiersState::empty(),
            scale: 1.0,
            preedit: String::new(),
            last_stats: root_ui::adapter::AdapterStats::default(),
        }
    }

    /// Drain redraw traffic and the host's own notifications. Returns true when
    /// the host closed the pipe.
    fn pump(&mut self) -> bool {
        let Some(link) = self.link.as_mut() else {
            return false;
        };
        let (events, closed) = link.queue.drain_redraw();
        if !events.is_empty() {
            if let Some(screen) = self.screen.as_mut() {
                screen.apply(&events);
            }
            self.ext_ui.apply(&events);
        }
        let notifications = link.queue.take_notifications();
        for (name, params) in notifications {
            match name.as_str() {
                proto::server::NAVIGATE => {
                    let files = params.first().and_then(rmpv::Value::as_str) == Some("files");
                    self.open_navigation(files);
                }
                proto::server::PROJECT_SEARCH => {
                    let origin = match params.first().and_then(rmpv::Value::as_str) {
                        Some("ex") => run::Origin::OwnerExCommand,
                        _ => run::Origin::OwnerKey,
                    };
                    let path = params
                        .get(1)
                        .and_then(rmpv::Value::as_str)
                        .map(PathBuf::from);
                    self.open_project_search(origin, path);
                }
                proto::server::RUN_TASK => {
                    let origin = match params.first().and_then(rmpv::Value::as_str) {
                        Some("ex") => run::Origin::OwnerExCommand,
                        _ => run::Origin::OwnerKey,
                    };
                    let path = params
                        .get(1)
                        .and_then(rmpv::Value::as_str)
                        .map(PathBuf::from);
                    self.run_task(origin, path);
                }
                proto::server::TATEGAKI => {
                    if let Some(path) = params.first().and_then(rmpv::Value::as_str) {
                        open_page(path);
                    }
                }
                proto::server::QUIT => return true,
                proto::server::PLUGIN => {
                    let name = params
                        .first()
                        .and_then(rmpv::Value::as_str)
                        .unwrap_or_default();
                    let argument = params
                        .get(1)
                        .and_then(rmpv::Value::as_str)
                        .unwrap_or_default();
                    self.run_plugin(name, argument);
                }
                other => eprintln!("note: {other} (no handler)"),
            }
        }
        closed
    }

    /// Open the navigation surface over whichever corpus was asked for.
    ///
    /// Notes are the default because `pin primary_object` makes them the primary
    /// object; the file tree stays one key away because
    /// `pin file_retained_for_programming` keeps files in scope for programming
    /// work. An absent vault falls back to files with a message rather than
    /// opening an empty surface that looks like a vault with no notes.
    fn open_navigation(&mut self, files: bool) {
        let rows = self.screen.as_ref().map(screen::Screen::rows).unwrap_or(24);
        let visible = (rows / 2).max(4);
        let vault = notes::Vault::default_vault();
        if !files && vault.exists() {
            let source = notes::NotesSource::new(&vault);
            self.overlay = Overlay::Picker {
                picker: picker::Picker::open(&source, visible),
                kind: PickerKind::Vault,
            };
        } else {
            if !files {
                eprintln!(
                    "note vault {} is not there; showing files instead",
                    vault.root().display()
                );
            }
            let root = self
                .args
                .file
                .as_ref()
                .and_then(|path| path.parent().map(PathBuf::from))
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
            let root = workspace::workable_root(root);
            let source = picker::TreeSource::new(root);
            self.overlay = Overlay::Picker {
                picker: picker::Picker::open(&source, visible),
                kind: PickerKind::Files,
            };
        }
        self.request_redraw();
    }

    fn open_entry_point(&mut self) {
        match workspace::entry_point_source(workspace::EntryPointOrientation::pinned_default()) {
            workspace::EntryPointSource::Repositories(source)
            | workspace::EntryPointSource::NotesAndRepositories(source) => {
                let rows = self.screen.as_ref().map(screen::Screen::rows).unwrap_or(24);
                let visible = (rows / 2).max(4);
                self.overlay = Overlay::Picker {
                    picker: picker::Picker::open(&source, visible),
                    kind: PickerKind::Workspace,
                };
                self.request_redraw();
            }
            workspace::EntryPointSource::RecentRepository(Some(repo)) => {
                if let Some(link) = self.link.as_mut() {
                    let escaped = repo.path.display().to_string().replace('<', "<lt>");
                    let _ = link.input(&format!(":Tree {escaped}<CR>"));
                }
            }
            workspace::EntryPointSource::RecentRepository(None) => self.open_navigation(false),
        }
    }

    fn open_project_search(&mut self, origin: run::Origin, path: Option<PathBuf>) {
        let rows = self.screen.as_ref().map(screen::Screen::rows).unwrap_or(24);
        let visible = (rows / 2).max(4);
        let root = path
            .as_deref()
            .map(project::root_of)
            .unwrap_or_else(|| {
                workspace::workable_root(
                    std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
                )
            });
        self.overlay = Overlay::Grep(picker::GrepPicker::open(root, origin, visible));
        self.request_redraw();
    }

    /// The `<F5>` table is this host's choice. The owner's `init.lua` maps the
    /// key to Jaq but does not provide Jaq recipes, so unknown filetypes are
    /// reported rather than guessed. Cwd is always the worktree root.
    fn run_task(&mut self, origin: run::Origin, path: Option<PathBuf>) {
        let cwd = path
            .as_deref()
            .map(project::root_of)
            .unwrap_or_else(|| {
                workspace::workable_root(
                    std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
                )
            });
        let Some(path) = path else {
            self.overlay = Overlay::Output(OutputPanel::message(
                vec!["Jaq".into()],
                cwd,
                "Jaq: current buffer has no filetype to run".into(),
            ));
            self.request_redraw();
            return;
        };
        let relative = path.strip_prefix(&cwd).unwrap_or(&path).to_path_buf();
        let Some(argv) = task_argv_for(&path, &relative) else {
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("[none]");
            self.overlay = Overlay::Output(OutputPanel::message(
                vec!["Jaq".into()],
                cwd,
                format!("Jaq: no task recipe for filetype {ext}"),
            ));
            self.request_redraw();
            return;
        };
        match run::spawn(origin, argv, cwd.clone()) {
            Ok(task) => self.overlay = Overlay::Output(OutputPanel::running(task)),
            Err(error) => {
                self.overlay = Overlay::Output(OutputPanel::message(
                    vec!["Jaq".into()],
                    cwd,
                    format!("Jaq: {error}"),
                ));
            }
        }
        self.request_redraw();
    }

    fn request_redraw(&self) {
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

    /// While the navigation surface is open the host owns the keyboard.
    ///
    /// `open_question navigation_input_routing` is open, and this is the
    /// arrangement chosen for the implementation rather than a claim that it is
    /// the required one: the keys never reach the editing core, and the choice
    /// made on the surface comes back as an ordinary `:e` over the protocol.
    fn feed_overlay(&mut self, keys: &str) {
        // A plugin surface has no input model — `open_question plugin_api_scope`
        // has not said whether plugins receive keys — so the only key it takes
        // is the one that closes it. Routing keys into it would be answering
        // that question by accident.
        match std::mem::replace(&mut self.overlay, Overlay::None) {
            Overlay::Plugin(name) => {
                if keys.contains("<Esc>") || keys == "q" {
                    self.request_redraw();
                } else {
                    self.overlay = Overlay::Plugin(name);
                }
            }
            Overlay::Grep(mut picker) => {
                let choice = match picker.feed(keys) {
                    picker::Outcome::Open => {
                        self.overlay = Overlay::Grep(picker);
                        None
                    }
                    picker::Outcome::Cancelled => None,
                    picker::Outcome::Chose(choice) => Some(choice),
                };
                if let Some(choice) = choice {
                    if let Some(link) = self.link.as_mut() {
                        let escaped = choice.replace('<', "<lt>");
                        let _ = link.input(&format!("<Esc>:e {escaped}<CR>"));
                    }
                }
                self.request_redraw();
            }
            Overlay::Output(mut panel) => {
                if panel.feed(keys) {
                    self.overlay = Overlay::Output(panel);
                }
                self.request_redraw();
            }
            Overlay::Picker { mut picker, kind } => {
                let choice = match picker.feed(keys) {
                    picker::Outcome::Open => {
                        self.overlay = Overlay::Picker { picker, kind };
                        None
                    }
                    picker::Outcome::Cancelled => None,
                    picker::Outcome::Chose(choice) => {
                        let path = if matches!(kind, PickerKind::Vault) {
                            notes::Vault::default_vault()
                                .path_of(&choice)
                                .display()
                                .to_string()
                        } else {
                            choice
                        };
                        Some(path)
                    }
                };
                if let Some(path) = choice {
                    if let Some(link) = self.link.as_mut() {
                        let escaped = path.replace('<', "<lt>");
                        let command = if matches!(kind, PickerKind::Workspace) {
                            format!("<Esc>:Tree {escaped}<CR>")
                        } else {
                            format!("<Esc>:e {escaped}<CR>")
                        };
                        let _ = link.input(&command);
                    }
                }
                self.request_redraw();
            }
            Overlay::None => {}
        }
    }

    fn ext_overlay(&self) -> ext_ui::Overlay {
        match self.screen.as_ref() {
            Some(screen) if !self.ext_ui.is_idle() => {
                self.ext_ui.layout(screen.cols(), screen.rows())
            }
            _ => ext_ui::Overlay::default(),
        }
    }

    /// Run what a plugin registered under `name`.
    ///
    /// A surface opens; a command runs. A name can be both, and the surface
    /// wins, because the surface is the thing the owner can see.
    fn run_plugin(&mut self, name: &str, argument: &str) {
        if self
            .plugins
            .surface_names()
            .iter()
            .any(|known| *known == name)
        {
            self.overlay = Overlay::Plugin(name.to_string());
            self.request_redraw();
            return;
        }
        if let Err(error) = self.plugins.run_command(name, argument) {
            eprintln!("plugin: {error}");
        }
        for message in std::mem::take(&mut self.plugins.messages) {
            eprintln!("plugin: {message}");
        }
        self.request_redraw();
    }

    /// Push a plugin's declared scene into the adapter.
    ///
    /// `pin plugin_surface_renderer` puts the drawing here: the plugin said
    /// what it wanted, and this is the only place that turns it into quads.
    fn build_plugin_surface(&mut self, width: f32, height: f32) {
        let Overlay::Plugin(name) = &self.overlay else {
            return;
        };
        let name = name.clone();
        let composed = match self.plugins.surface(&name, (width, height), self.scale) {
            Ok(scene) => scene,
            Err(error) => {
                eprintln!("plugin surface {name}: {error}");
                self.overlay = Overlay::None;
                return;
            }
        };
        for role in &composed.unknown_roles {
            eprintln!("plugin surface {name}: no colour role named {role}");
        }
        let (Some(adapter), Some(atlas)) = (self.adapter.as_mut(), self.atlas.as_mut()) else {
            return;
        };
        let runtime = root_ui::navigation::color_runtime(&self.args.scheme);
        let Ok(prepared) = root_ui::prepare_flat_scene(&composed.scene) else {
            eprintln!("plugin surface {name}: the declared scene did not resolve");
            self.overlay = Overlay::None;
            return;
        };
        let Ok(scene) = root_ui::bind_flat_scene_user_color_scheme(&prepared, &runtime) else {
            eprintln!("plugin surface {name}: the declared scene has no colour");
            self.overlay = Overlay::None;
            return;
        };
        let mut stats = adapter.push_scene(&scene, width, height, atlas.cell_h, self.scale);
        let scheme = runtime
            .schemes
            .iter()
            .find(|scheme| scheme.id == runtime.scheme_id)
            .unwrap_or(&runtime.schemes[0]);
        for run in &composed.texts {
            let color = scheme
                .colors
                .get(run.role)
                .copied()
                .unwrap_or([1.0, 1.0, 1.0, 1.0]);
            stats.glyph_quads +=
                adapter.push_text(atlas, run.x, run.baseline, &run.text, color, run.max_x);
        }
        self.last_stats = stats;
    }

    fn build_output_surface(&mut self, width: f32, height: f32) {
        let (Overlay::Output(panel), Some(adapter), Some(atlas)) =
            (&self.overlay, self.adapter.as_mut(), self.atlas.as_mut())
        else {
            return;
        };
        let cwd = panel.cwd.display().to_string();
        let composed = root_ui::output::build(root_ui::output::Input {
            argv: &panel.argv,
            cwd: &cwd,
            status: panel.status.as_deref(),
            lines: &panel.lines,
            scroll: panel.scroll,
            window_w: width,
            window_h: height,
            cell_w: atlas.cell_w,
            cell_h: atlas.cell_h,
            ascent: atlas.ascent,
            scale: self.scale,
        });
        let runtime = root_ui::output::color_runtime(&self.args.scheme);
        let prepared = match root_ui::prepare_flat_scene(&composed.scene) {
            Ok(prepared) => prepared,
            Err(error) => {
                eprintln!("output surface did not resolve: {error}");
                return;
            }
        };
        let scene = match root_ui::bind_flat_scene_user_color_scheme(&prepared, &runtime) {
            Ok(scene) => scene,
            Err(error) => {
                eprintln!("output surface has no colour: {error}");
                return;
            }
        };
        let mut stats = adapter.push_scene(&scene, width, height, atlas.cell_h, self.scale);
        let scheme = runtime
            .schemes
            .iter()
            .find(|scheme| scheme.id == runtime.scheme_id)
            .unwrap_or(&runtime.schemes[0]);
        for run in &composed.texts {
            let color = scheme
                .colors
                .get(run.role)
                .copied()
                .unwrap_or([1.0, 1.0, 1.0, 1.0]);
            stats.glyph_quads +=
                adapter.push_text(atlas, run.x, run.baseline, &run.text, color, run.max_x);
        }
        self.last_stats = stats;
    }

    /// Push the navigation surface, as a root-ui scene, into the adapter.
    fn build_navigation(&mut self, width: f32, height: f32) {
        let (label, query, matched, total, rows, row_budget, offset) = match &self.overlay {
            Overlay::Picker { picker, .. } => (
                picker.label().to_string(),
                picker.query().to_string(),
                picker.matches(),
                picker.corpus_len(),
                picker.visible(),
                picker.row_budget(),
                picker.offset(),
            ),
            Overlay::Grep(picker) => (
                picker.label(),
                picker.query().to_string(),
                picker.matches(),
                picker.corpus_len(),
                picker.visible(),
                picker.row_budget(),
                picker.offset(),
            ),
            _ => return,
        };
        let (Some(adapter), Some(atlas)) = (self.adapter.as_mut(), self.atlas.as_mut()) else {
            return;
        };
        let composed = root_ui::navigation::build(root_ui::navigation::Input {
            label: &label,
            query: &query,
            matched,
            total,
            rows: &rows,
            row_budget,
            offset,
            window_w: width,
            window_h: height,
            cell_w: atlas.cell_w,
            cell_h: atlas.cell_h,
            ascent: atlas.ascent,
            scale: self.scale,
        });

        let runtime = root_ui::navigation::color_runtime(&self.args.scheme);
        let prepared = match root_ui::prepare_flat_scene(&composed.scene) {
            Ok(prepared) => prepared,
            Err(error) => {
                eprintln!("navigation surface did not resolve: {error}");
                return;
            }
        };
        let scene = match root_ui::bind_flat_scene_user_color_scheme(&prepared, &runtime) {
            Ok(scene) => scene,
            Err(error) => {
                eprintln!("navigation surface has no colour: {error}");
                return;
            }
        };

        let mut stats = adapter.push_scene(&scene, width, height, atlas.cell_h, self.scale);

        let scheme = runtime
            .schemes
            .iter()
            .find(|scheme| scheme.id == runtime.scheme_id)
            .unwrap_or(&runtime.schemes[0]);
        for run in &composed.texts {
            let color = scheme
                .colors
                .get(run.role)
                .copied()
                .unwrap_or([1.0, 1.0, 1.0, 1.0]);
            stats.glyph_quads +=
                adapter.push_text(atlas, run.x, run.baseline, &run.text, color, run.max_x);
        }
        self.last_stats = stats;
    }

    fn build_overlay_surface(&mut self, width: f32, height: f32) {
        if !self.overlay.is_open() {
            return;
        }
        if let Some(adapter) = self.adapter.as_mut() {
            adapter.begin();
        } else {
            return;
        }
        if matches!(&self.overlay, Overlay::Picker { .. } | Overlay::Grep(_)) {
            self.build_navigation(width, height);
        } else if matches!(&self.overlay, Overlay::Output(_)) {
            self.build_output_surface(width, height);
        } else if matches!(&self.overlay, Overlay::Plugin(_)) {
            self.build_plugin_surface(width, height);
        }
    }

    fn poll_overlay_work(&mut self) {
        match &mut self.overlay {
            Overlay::Grep(picker) => {
                let before = picker.matches();
                picker.poll();
                if picker.matches() != before {
                    self.request_redraw();
                }
            }
            Overlay::Output(panel) => {
                if panel.poll() {
                    self.request_redraw();
                }
            }
            _ => {}
        }
    }

    fn render(&mut self) {
        let Some(size) = self.window.as_ref().map(Window::inner_size) else {
            return;
        };
        // The grid first, then the surface over it. Both write into the same
        // window and the same frame; they differ only in which shader is asked.
        {
            let (Some(gl), Some(renderer), Some(atlas), Some(screen)) = (
                self.gl.as_ref(),
                self.renderer.as_mut(),
                self.atlas.as_mut(),
                self.screen.as_ref(),
            ) else {
                return;
            };
            let overlay = if self.ext_ui.is_idle() {
                ext_ui::Overlay::default()
            } else {
                self.ext_ui.layout(screen.cols(), screen.rows())
            };
            renderer.build(screen, atlas, &self.preedit, &self.ext_ui, &overlay);
            renderer.draw(gl, atlas, size.width as i32, size.height as i32, &[]);
        }

        if self.overlay.is_open() {
            self.build_overlay_surface(size.width as f32, size.height as f32);
            if let (Some(adapter), Some(atlas), Some(gl)) =
                (self.adapter.as_mut(), self.atlas.as_mut(), self.gl.as_ref())
            {
                adapter.draw(gl, atlas, size.width as i32, size.height as i32);
            }
        }

        if let (Some(surface), Some(context)) = (self.surface.as_ref(), self.context.as_ref()) {
            let _ = surface.swap_buffers(context);
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.started {
            return;
        }
        self.started = true;
        let snapshot = self.args.snapshot.is_some();

        let attributes = Window::default_attributes()
            .with_title("nvimglsl")
            .with_visible(!snapshot)
            .with_inner_size(winit::dpi::LogicalSize::new(940.0, 600.0));
        let (window, config) = DisplayBuilder::new()
            .with_window_attributes(Some(attributes))
            .build(el, ConfigTemplateBuilder::new(), |c| {
                c.last().expect("no GL config")
            })
            .expect("display build failed");
        let window = window.expect("no window");
        window.set_ime_allowed(true);

        let scale = window.scale_factor() as f32;
        self.scale = scale;
        let atlas = text::Atlas::new(self.args.font_size * scale);
        let width = (atlas.cell_w * self.args.cols as f32).ceil() as u32;
        let height = (atlas.cell_h * self.args.rows as f32).ceil() as u32;
        let _ = window.request_inner_size(winit::dpi::PhysicalSize::new(width, height));

        let raw = window.window_handle().ok().map(|h| h.as_raw());
        let display = config.display();
        let context_attributes = ContextAttributesBuilder::new()
            .with_context_api(ContextApi::OpenGl(Some(Version::new(3, 3))))
            .with_profile(GlProfile::Core)
            .build(raw);
        let not_current = unsafe {
            display
                .create_context(&config, &context_attributes)
                .expect("create_context")
        };
        let surface_attributes: SurfaceAttributesBuilder<WindowSurface> = Default::default();
        let surface_attributes = window
            .build_surface_attributes(surface_attributes)
            .expect("surface attrs");
        let surface = unsafe {
            display
                .create_window_surface(&config, &surface_attributes)
                .expect("window surface")
        };
        let context = not_current.make_current(&surface).expect("make_current");
        let glc =
            unsafe { glow::Context::from_loader_function_cstr(|s| display.get_proc_address(s)) };
        unsafe {
            use glow::HasContext;
            eprintln!(
                "GL {} | {} | GLSL {}",
                glc.get_parameter_string(glow::VERSION),
                glc.get_parameter_string(glow::RENDERER),
                glc.get_parameter_string(glow::SHADING_LANGUAGE_VERSION)
            );
        }

        let mut plugin_names: Vec<String> = self
            .plugins
            .command_names()
            .into_iter()
            .chain(self.plugins.surface_names())
            .map(str::to_string)
            .collect();
        plugin_names.sort();
        plugin_names.dedup();
        if !self.plugins.plugins.is_empty() || !self.plugins.errors.is_empty() {
            eprintln!(
                "plugins: {} loaded, {} command(s), {} surface(s)",
                self.plugins.plugins.len(),
                self.plugins.command_names().len(),
                self.plugins.surface_names().len()
            );
            for error in &self.plugins.errors {
                eprintln!("plugin: {error}");
            }
        }
        let mut link = Link::spawn(
            self.args.file.clone(),
            proto::paint::Theme::named(&self.args.scheme),
            plugin_names,
            preview_scheme(&self.args),
        )
        .expect("host thread");
        link.ui_attach(
            self.args.cols,
            self.args.rows,
            nvim::UiOptions {
                ext_multigrid: true,
                ext_popupmenu: true,
                ..nvim::UiOptions::none()
            },
        )
        .expect("ui_attach");

        self.renderer = Some(gl::Renderer::new(&glc));
        self.adapter = Some(root_ui::adapter::Adapter::new(&glc));
        self.screen = Some(screen::Screen::new(self.args.cols, self.args.rows));
        self.gl = Some(glc);
        self.atlas = Some(atlas);
        self.window = Some(window);
        self.context = Some(context);
        self.surface = Some(surface);
        self.link = Some(link);

        // Launched with nothing to edit — from the Dock, say — the useful first
        // screen is the note list, not an empty buffer. `pin primary_object`
        // makes notes the primary object, and `pin user_file_awareness_not_required`
        // says the user should not have to name a file to get started.
        // Snapshot mode is included deliberately: a first screen that cannot be
        // photographed cannot be checked.
        if self.args.file.is_none() {
            self.open_entry_point();
        }

        if let Some(path) = self.args.snapshot.clone() {
            self.run_snapshot(&path, width, height);
            el.exit();
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::ModifiersChanged(m) => self.mods = m.state(),
            WindowEvent::Focused(true) => {
                if let Some(window) = self.window.as_ref() {
                    window.set_ime_allowed(true);
                }
            }
            WindowEvent::Resized(size) => {
                if let (Some(atlas), Some(surface), Some(context)) = (
                    self.atlas.as_ref(),
                    self.surface.as_ref(),
                    self.context.as_ref(),
                ) {
                    let cols = (size.width as f32 / atlas.cell_w).floor().max(1.0) as usize;
                    let rows = (size.height as f32 / atlas.cell_h).floor().max(1.0) as usize;
                    if let Some(link) = self.link.as_mut() {
                        let _ = link.try_resize(cols, rows);
                    }
                    if let (Some(w), Some(h)) = (
                        std::num::NonZeroU32::new(size.width),
                        std::num::NonZeroU32::new(size.height),
                    ) {
                        surface.resize(context, w, h);
                    }
                }
            }
            WindowEvent::Ime(ime) => {
                match ime_action(&ime) {
                    ImeAction::Compose(text) => self.preedit = text,
                    ImeAction::Commit(keys) => {
                        self.preedit.clear();
                        if self.overlay.is_open() {
                            self.feed_overlay(&keys);
                        } else if let Some(link) = self.link.as_mut() {
                            let _ = link.input(&keys);
                        }
                    }
                    ImeAction::Nothing => {}
                }
                self.request_redraw();
            }
            // While a composition is open the IME owns the keystrokes.
            WindowEvent::KeyboardInput { .. } if !self.preedit.is_empty() => {}
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                if let Some(keys) = encode_key(&event.logical_key, self.mods) {
                    if self.overlay.is_open() {
                        self.feed_overlay(&keys);
                    } else if let Some(link) = self.link.as_mut() {
                        let _ = link.input(&keys);
                    }
                }
            }
            WindowEvent::RedrawRequested => self.render(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if self.args.snapshot.is_some() {
            return;
        }
        if self.pump() {
            el.exit();
            return;
        }
        self.poll_overlay_work();
        let flushed = self.screen.as_mut().is_some_and(|screen| {
            let flushed = screen.flushed;
            screen.flushed = false;
            flushed
        });
        if flushed {
            self.request_redraw();
        }
        el.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_millis(8),
        ));
    }
}

impl App {
    /// Settle the redraw stream, then draw one frame offscreen and write it as
    /// a PNG, so a change can be checked without a human watching a window.
    fn run_snapshot(&mut self, path: &str, width: u32, height: u32) {
        let settle = |app: &mut App, until: Duration| {
            let deadline = Instant::now() + until;
            while Instant::now() < deadline {
                if let Some(link) = app.link.as_mut() {
                    link.queue.wait_ready(Duration::from_millis(40));
                }
                if app.pump() {
                    break;
                }
                // The live loop polls overlay work on every tick; a snapshot
                // that skipped it could never show anything that arrives
                // asynchronously. A grep with results still in flight would
                // photograph as an empty list — a picture of the harness, not
                // of the feature.
                app.poll_overlay_work();
            }
        };

        settle(self, Duration::from_millis(300));
        if let Some(keys) = self.args.input.clone() {
            // A snapshot script may open the navigation surface, in which case
            // the rest of the keys belong to it and not to the editor.
            for key in split_keys(&keys) {
                if self.overlay.is_open() {
                    self.feed_overlay(&key);
                } else if let Some(link) = self.link.as_mut() {
                    let _ = link.input(&key);
                }
                settle(self, Duration::from_millis(60));
            }
        }
        settle(self, Duration::from_millis(200));

        // The surface's vertices are built before the GL block opens: nothing
        // in root-ui's phases needs a context, and building inside the block
        // would hold a borrow of `self.gl` across a call that takes all of self.
        if self.overlay.is_open() {
            self.build_overlay_surface(width as f32, height as f32);
        }

        let gl = self.gl.as_ref().unwrap();
        let pixels = unsafe {
            use glow::HasContext;
            let texture = gl.create_texture().unwrap();
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                width as i32,
                height as i32,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                None,
            );
            let fbo = gl.create_framebuffer().unwrap();
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
            gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(texture),
                0,
            );
            assert_eq!(
                gl.check_framebuffer_status(glow::FRAMEBUFFER),
                glow::FRAMEBUFFER_COMPLETE,
                "offscreen target incomplete"
            );

            let overlay = self.ext_overlay();
            let renderer = self.renderer.as_mut().unwrap();
            renderer.build(
                self.screen.as_ref().unwrap(),
                self.atlas.as_mut().unwrap(),
                &self.preedit,
                &self.ext_ui,
                &overlay,
            );
            renderer.draw(
                gl,
                self.atlas.as_mut().unwrap(),
                width as i32,
                height as i32,
                &[],
            );

            if self.overlay.is_open() {
                let adapter = self.adapter.as_mut().unwrap();
                adapter.draw(
                    gl,
                    self.atlas.as_mut().unwrap(),
                    width as i32,
                    height as i32,
                );
            }
            gl.finish();

            let mut buffer = vec![0u8; (width * height * 4) as usize];
            gl.read_pixels(
                0,
                0,
                width as i32,
                height as i32,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(&mut buffer),
            );
            buffer
        };
        write_png(path, &pixels, width, height);
        eprintln!(
            "WROTE {path} ({width}x{height}) root-ui surfaces={} round={} glyphs={} off_grid={}",
            self.last_stats.surfaces,
            self.last_stats.round_boxes,
            self.last_stats.glyph_quads,
            self.last_stats.origin_off_grid
        );
    }
}

/// Split an input script into single keys, so a script can cross the boundary
/// between the editor and the navigation surface mid-way.
fn split_keys(script: &str) -> Vec<String> {
    let chars: Vec<char> = script.chars().collect();
    let mut keys = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '<' {
            if let Some(close) = (index + 1..chars.len()).find(|&i| chars[i] == '>') {
                keys.push(chars[index..=close].iter().collect());
                index = close + 1;
                continue;
            }
        }
        keys.push(chars[index].to_string());
        index += 1;
    }
    keys
}

fn encode_key(key: &Key, mods: ModifiersState) -> Option<String> {
    let named = |s: &str| Some(wrap(s, mods));
    match key {
        Key::Named(n) => match n {
            NamedKey::Enter => named("CR"),
            NamedKey::Escape => named("Esc"),
            NamedKey::Backspace => named("BS"),
            NamedKey::Tab => named("Tab"),
            NamedKey::Space => Some(if mods.control_key() {
                "<C-Space>".into()
            } else {
                " ".into()
            }),
            NamedKey::ArrowLeft => named("Left"),
            NamedKey::ArrowRight => named("Right"),
            NamedKey::ArrowUp => named("Up"),
            NamedKey::ArrowDown => named("Down"),
            NamedKey::Home => named("Home"),
            NamedKey::End => named("End"),
            NamedKey::PageUp => named("PageUp"),
            NamedKey::PageDown => named("PageDown"),
            NamedKey::Delete => named("Del"),
            NamedKey::F1 => named("F1"),
            NamedKey::F2 => named("F2"),
            NamedKey::F3 => named("F3"),
            NamedKey::F4 => named("F4"),
            NamedKey::F5 => named("F5"),
            NamedKey::F6 => named("F6"),
            NamedKey::F7 => named("F7"),
            NamedKey::F8 => named("F8"),
            NamedKey::F9 => named("F9"),
            NamedKey::F10 => named("F10"),
            NamedKey::F11 => named("F11"),
            NamedKey::F12 => named("F12"),
            _ => None,
        },
        Key::Character(s) => {
            if mods.control_key() || mods.super_key() || mods.alt_key() {
                Some(wrap(s.as_str(), mods))
            } else if s.as_str() == "<" {
                Some("<lt>".into())
            } else {
                Some(s.to_string())
            }
        }
        _ => None,
    }
}

fn wrap(base: &str, mods: ModifiersState) -> String {
    let mut prefix = String::new();
    if mods.control_key() {
        prefix.push_str("C-");
    }
    if mods.alt_key() {
        prefix.push_str("A-");
    }
    if mods.super_key() {
        prefix.push_str("D-");
    }
    if mods.shift_key() && base.len() > 1 {
        prefix.push_str("S-");
    }
    format!("<{prefix}{base}>")
}

fn task_argv_for(path: &std::path::Path, relative: &std::path::Path) -> Option<Vec<String>> {
    let ext = path.extension()?.to_str()?;
    let file = relative.display().to_string();
    Some(match ext {
        "rs" => vec![
            "cargo".into(),
            "run".into(),
            "--manifest-path".into(),
            "host/Cargo.toml".into(),
        ],
        "py" => vec!["python3".into(), file],
        "lua" => vec!["lua".into(), file],
        "sh" | "bash" => vec!["sh".into(), file],
        "js" | "mjs" | "cjs" => vec!["node".into(), file],
        _ => return None,
    })
}

fn write_png(path: &str, rgba_bottom_up: &[u8], width: u32, height: u32) {
    let stride = (width * 4) as usize;
    let mut flipped = vec![0u8; rgba_bottom_up.len()];
    for y in 0..height as usize {
        let source = (height as usize - 1 - y) * stride;
        flipped[y * stride..(y + 1) * stride]
            .copy_from_slice(&rgba_bottom_up[source..source + stride]);
    }
    let file = std::fs::File::create(path).expect("create png");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(&flipped)
        .unwrap();
}

/// The paper the vertical preview is set on.
///
/// Paper unless `--scheme` was given, because the preview imitates a book rather
/// than the editor around it. Passing `--scheme dark` moves it, and `t` inside
/// the page moves it again without touching the editor.
fn preview_scheme(args: &Args) -> tategaki::Scheme {
    if args.scheme_given {
        tategaki::Scheme::parse(&args.scheme)
    } else {
        tategaki::Scheme::Paper
    }
}

/// Hand a written page to whatever the machine opens HTML with.
///
/// `pin first_stage_platform` makes macOS the first stage and
/// `pin multi_target_portability_direction` forbids stopping there, so the other
/// two openers are named rather than left out. A failure is reported and not
/// fatal: the page is already on disk, and saying where it is beats dying.
fn open_page(path: &str) {
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "explorer"
    } else {
        "xdg-open"
    };
    match std::process::Command::new(opener).arg(path).spawn() {
        Ok(_) => {}
        Err(error) => eprintln!("nvimglsl: {opener} {path}: {error} (the page is written)"),
    }
}

fn main() {
    let args = parse_args_from(std::env::args().skip(1).collect());
    if let Some(to) = args.tategaki.as_deref() {
        // Read from the file rather than through the editing core: nothing has
        // been typed yet, so the buffer and the file are the same text, and this
        // path needs neither a window nor a pipe.
        let markdown = match args.file.as_ref() {
            Some(path) => match std::fs::read_to_string(path) {
                Ok(text) => text,
                Err(error) => {
                    eprintln!("nvimglsl: {}: {error}", path.display());
                    std::process::exit(1);
                }
            },
            None => {
                eprintln!("nvimglsl: --tategaki needs a note to set");
                std::process::exit(1);
            }
        };
        let style = tategaki::Style {
            scheme: preview_scheme(&args),
            ..tategaki::Style::default()
        };
        if let Err(error) = tategaki::write(&markdown, &style, std::path::Path::new(to)) {
            eprintln!("nvimglsl: {to}: {error}");
            std::process::exit(1);
        }
        println!("{to}");
        return;
    }
    if args.embed {
        // The protocol face, served the way `nvim --embed` serves it.
        let mut host = if std::env::var_os("NVIMGLSL_CONFIGURED_EMBED").is_some() {
            proto::Host::configured(notes::Vault::default_vault(), proto::paint::Theme::dark())
        } else {
            proto::Host::new(core::Editor::default())
        };
        let stdin = std::io::stdin();
        let stdout = std::io::stdout();
        if let Err(error) = proto::serve(&mut host, stdin, stdout.lock(), args.file.clone()) {
            eprintln!("nvimglsl: {error}");
            std::process::exit(1);
        }
        return;
    }
    let event_loop = EventLoop::new().expect("event loop");
    let mut app = App::new(args);
    event_loop.run_app(&mut app).expect("run_app");
}

#[cfg(test)]
mod ime_tests {
    use super::*;

    #[test]
    fn a_composition_is_shown_and_then_replaced_by_what_is_committed() {
        assert_eq!(
            ime_action(&Ime::Preedit("かん".into(), None)),
            ImeAction::Compose("かん".into())
        );
        assert_eq!(
            ime_action(&Ime::Commit("漢字".into())),
            ImeAction::Commit("漢字".into())
        );
    }

    /// The one that would be silent: `<` reaches the same parser that reads
    /// `<C-x>`, so an input method committing it must not be able to start a
    /// key notation.
    #[test]
    fn a_committed_angle_bracket_cannot_become_key_notation() {
        assert_eq!(
            ime_action(&Ime::Commit("a<b".into())),
            ImeAction::Commit("a<lt>b".into())
        );
    }

    #[test]
    fn abandoning_a_composition_leaves_nothing_on_screen() {
        assert_eq!(ime_action(&Ime::Disabled), ImeAction::Compose(String::new()));
        assert_eq!(ime_action(&Ime::Enabled), ImeAction::Nothing);
    }

    /// While a composition is open the input method owns the keys — the window
    /// arm that drops them keys off exactly this.
    #[test]
    fn keys_are_held_while_a_composition_is_open() {
        let ImeAction::Compose(preedit) = ime_action(&Ime::Preedit("か".into(), None)) else {
            panic!("a preedit composes");
        };
        assert!(!preedit.is_empty());
        let ImeAction::Compose(cleared) = ime_action(&Ime::Disabled) else {
            panic!("disabling composes nothing");
        };
        assert!(cleared.is_empty());
    }
}

#[cfg(test)]
mod output_panel_tests {
    use super::*;

    /// The panel showed `$ python3 hello.py` and `exit status 0` with nothing
    /// between them. Everything below the spawn works, so the loss is here.
    #[test]
    fn a_short_command_reaches_the_panel_lines() {
        let task = run::spawn(
            run::Origin::OwnerKey,
            vec!["/bin/echo".into(), "hello".into()],
            std::env::temp_dir(),
        )
        .unwrap();
        let mut panel = OutputPanel::running(task);
        for _ in 0..200 {
            let done = panel.poll();
            if done && panel.task.is_none() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let text: String = panel
            .lines
            .iter()
            .flat_map(|line| line.segments.iter().map(|s| s.text.as_str()))
            .collect();
        assert!(text.contains("hello"), "panel lost the output: {text:?}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_take_a_file_and_leave_defaults_alone() {
        let args = parse_args_from(vec!["note.md".into(), "--cols".into(), "80".into()]);
        assert_eq!(args.file, Some(PathBuf::from("note.md")));
        assert_eq!(args.cols, 80);
        assert_eq!(args.rows, 30);
    }

    #[test]
    fn a_key_script_splits_into_single_keys() {
        assert_eq!(split_keys("i a<Esc>"), vec!["i", " ", "a", "<Esc>"]);
        assert_eq!(split_keys("<C-n><CR>"), vec!["<C-n>", "<CR>"]);
    }
}
