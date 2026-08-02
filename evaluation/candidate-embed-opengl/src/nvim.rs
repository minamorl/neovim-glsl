//! Msgpack-RPC transport to a `nvim --embed` child process.
//!
//! Neovim is the editing engine; this process is only its UI. Nothing here
//! interprets buffers or keymaps — it forwards input and consumes `redraw`.

use std::ffi::OsStr;
use std::io::{BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::thread;
use std::time::Duration;

use rmpv::Value;

pub struct Nvim {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<Value>,
    next_msgid: u64,
    custom: Vec<Notification>,
}

/// A `redraw` batch already split into its individual UI events.
pub type RedrawEvent = (String, Vec<Value>);

/// Any notification that is not `redraw` — this is how Lua running inside
/// Neovim talks to the UI, via `vim.rpcnotify(1, name, ...)`.
pub type Notification = (String, Vec<Value>);

impl Nvim {
    pub fn spawn(extra_args: &[String]) -> std::io::Result<Self> {
        let mut cmd = Command::new(resolve_nvim()?);
        cmd.arg("--embed");
        for a in extra_args {
            cmd.arg(a);
        }
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;

        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");

        let (tx, rx) = channel();
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            // A decode error means nvim closed the pipe; ending the thread is the
            // signal for that, so the error itself is not actionable here.
            while let Ok(v) = rmpv::decode::read_value(&mut reader) {
                if tx.send(v).is_err() {
                    break;
                }
            }
        });

        Ok(Self { child, stdin, rx, next_msgid: 1, custom: Vec::new() })
    }

    pub fn request(&mut self, method: &str, params: Vec<Value>) -> std::io::Result<()> {
        let msgid = self.next_request_id();
        let msg = Value::Array(vec![
            Value::from(0u8),
            Value::from(msgid),
            Value::from(method),
            Value::Array(params),
        ]);
        self.send(msg)
    }

    fn next_request_id(&mut self) -> u64 {
        let msgid = self.next_msgid;
        self.next_msgid += 1;
        msgid
    }

    /// Return the RPC channel assigned to this embedded client.
    ///
    /// This runs before other requests are placed in flight. Lua executed by
    /// `nvim_exec_lua` does not expose the channel through `vim.v.channel`, so
    /// integrations need it supplied explicitly.
    pub fn api_channel_id(&mut self) -> std::io::Result<u64> {
        let msgid = self.next_request_id();
        self.send(Value::Array(vec![
            Value::from(0u8),
            Value::from(msgid),
            Value::from("nvim_get_api_info"),
            Value::Array(vec![]),
        ]))?;

        let response = self.rx.recv_timeout(Duration::from_secs(2)).map_err(|error| {
            std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("nvim_get_api_info response unavailable: {error}"),
            )
        })?;
        parse_api_channel_id(&response, msgid)
    }

    pub fn notify(&mut self, method: &str, params: Vec<Value>) -> std::io::Result<()> {
        let msg = Value::Array(vec![
            Value::from(2u8),
            Value::from(method),
            Value::Array(params),
        ]);
        self.send(msg)
    }

    fn send(&mut self, msg: Value) -> std::io::Result<()> {
        let mut buf = Vec::new();
        rmpv::encode::write_value(&mut buf, &msg)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        self.stdin.write_all(&buf)?;
        self.stdin.flush()
    }

    pub fn ui_attach(&mut self, cols: u32, rows: u32, multigrid: bool) -> std::io::Result<()> {
        self.request(
            "nvim_ui_attach",
            vec![Value::from(cols), Value::from(rows), attach_options(multigrid)],
        )
    }

    pub fn try_resize(&mut self, cols: u32, rows: u32) -> std::io::Result<()> {
        self.request("nvim_ui_try_resize", vec![Value::from(cols), Value::from(rows)])
    }

    pub fn input(&mut self, keys: &str) -> std::io::Result<()> {
        self.notify("nvim_input", vec![Value::from(keys)])
    }

    pub fn exec_lua(&mut self, code: &str) -> std::io::Result<()> {
        self.request("nvim_exec_lua", vec![Value::from(code), Value::Array(vec![])])
    }

    pub fn exec_lua_with_args(
        &mut self,
        code: &str,
        args: Vec<Value>,
    ) -> std::io::Result<()> {
        self.request("nvim_exec_lua", vec![Value::from(code), Value::Array(args)])
    }

    /// Display structured integration output in an ordinary Neovim scratch
    /// buffer. Neovim still owns the editor state; the host only supplies text.
    pub fn show_json_scratch(&mut self, title: &str, body: &str) -> std::io::Result<()> {
        self.exec_lua_with_args(
            r#"
local title, body = ...
vim.cmd("botright new")
local buffer = vim.api.nvim_get_current_buf()
vim.bo[buffer].buftype = "nofile"
vim.bo[buffer].bufhidden = "wipe"
vim.bo[buffer].swapfile = false
vim.bo[buffer].filetype = "json"
vim.api.nvim_buf_set_name(buffer, title)
vim.api.nvim_buf_set_lines(buffer, 0, -1, false, vim.split(body, "\n", { plain = true }))
vim.bo[buffer].modifiable = false
"#,
            vec![Value::from(title), Value::from(body)],
        )
    }

    /// Notifications Lua sent us since the last call. The UI, not Neovim, decides
    /// what they mean.
    pub fn take_notifications(&mut self) -> Vec<Notification> {
        std::mem::take(&mut self.custom)
    }

    /// Drain everything currently queued, returning only the flattened `redraw`
    /// events. Responses to our own requests carry no UI state and are dropped.
    pub fn drain_redraw(&mut self) -> (Vec<RedrawEvent>, bool) {
        let mut out = Vec::new();
        let mut closed = false;
        loop {
            match self.rx.try_recv() {
                Ok(v) => collect(&v, &mut out, &mut self.custom),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    closed = true;
                    break;
                }
            }
        }
        (out, closed)
    }

    /// Block until at least one event arrives or the timeout elapses.
    pub fn wait_redraw(&mut self, timeout: std::time::Duration) -> (Vec<RedrawEvent>, bool) {
        let mut out = Vec::new();
        match self.rx.recv_timeout(timeout) {
            Ok(v) => collect(&v, &mut out, &mut self.custom),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => return (out, false),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return (out, true),
        }
        let (mut rest, closed) = self.drain_redraw();
        out.append(&mut rest);
        (out, closed)
    }
}

/// UI capabilities we claim at attach time. `ext_multigrid` makes nvim hand out
/// one grid per window plus placement events instead of pre-composing every
/// window into a single screen-sized grid.
fn attach_options(multigrid: bool) -> Value {
    let mut opts = vec![
        (Value::from("ext_linegrid"), Value::from(true)),
        (Value::from("rgb"), Value::from(true)),
    ];
    if multigrid {
        opts.push((Value::from("ext_multigrid"), Value::from(true)));
    }
    Value::Map(opts)
}

fn resolve_nvim() -> std::io::Result<PathBuf> {
    nvim_candidates(
        std::env::var_os("NVIMGL_NVIM").as_deref(),
        std::env::var_os("PATH").as_deref(),
        std::env::var_os("HOME").as_deref(),
    )
    .into_iter()
    .find(|candidate| candidate.is_file())
    .ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Neovim executable not found. Set NVIMGL_NVIM to an absolute nvim path.",
        )
    })
}

fn nvim_candidates(
    configured: Option<&OsStr>,
    path: Option<&OsStr>,
    home: Option<&OsStr>,
) -> Vec<PathBuf> {
    let binary = if cfg!(windows) { "nvim.exe" } else { "nvim" };
    let mut candidates = Vec::new();

    if let Some(configured) = configured.filter(|value| !value.is_empty()) {
        candidates.push(PathBuf::from(configured));
    }
    if let Some(path) = path {
        candidates.extend(std::env::split_paths(path).map(|directory| directory.join(binary)));
    }
    if let Some(home) = home {
        let home = PathBuf::from(home);
        candidates.push(home.join(".local/bin").join(binary));
        candidates.push(home.join(".nix-profile/bin").join(binary));
        #[cfg(target_os = "windows")]
        candidates.push(
            home.join("scoop/apps/neovim/current/bin")
                .join(binary),
        );
    }

    #[cfg(target_os = "macos")]
    candidates.extend([
        PathBuf::from("/opt/homebrew/bin/nvim"),
        PathBuf::from("/usr/local/bin/nvim"),
        PathBuf::from("/opt/local/bin/nvim"),
    ]);

    #[cfg(target_os = "linux")]
    candidates.extend([
        PathBuf::from("/usr/bin/nvim"),
        PathBuf::from("/usr/local/bin/nvim"),
        PathBuf::from("/opt/nvim/bin/nvim"),
        PathBuf::from("/run/current-system/sw/bin/nvim"),
    ]);

    #[cfg(target_os = "windows")]
    {
        if let Some(program_files) = std::env::var_os("ProgramFiles") {
            candidates.push(
                PathBuf::from(program_files)
                    .join("Neovim/bin")
                    .join(binary),
            );
        }
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            candidates.push(
                PathBuf::from(local_app_data)
                    .join("Programs/Neovim/bin")
                    .join(binary),
            );
        }
    }

    candidates
}

fn parse_api_channel_id(response: &Value, expected_msgid: u64) -> std::io::Result<u64> {
    let parts = response.as_array().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "RPC response is not an array")
    })?;
    if parts.len() != 4
        || parts[0].as_u64() != Some(1)
        || parts[1].as_u64() != Some(expected_msgid)
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "unexpected RPC response to nvim_get_api_info",
        ));
    }
    if !parts[2].is_nil() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("nvim_get_api_info failed: {}", parts[2]),
        ));
    }
    parts[3]
        .as_array()
        .and_then(|api_info| api_info.first())
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "nvim_get_api_info omitted its channel id",
            )
        })
}

impl Drop for Nvim {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn collect(v: &Value, out: &mut Vec<RedrawEvent>, custom: &mut Vec<Notification>) {
    let (mut events, mut notes) = split_notification(v);
    out.append(&mut events);
    custom.append(&mut notes);
}

/// A notification is `[2, method, params]`; for `redraw`, params is a list of
/// `[event_name, args…]` where each `args` is itself one invocation. Anything
/// else is a message from Lua and is passed through untouched.
pub fn split_notification(v: &Value) -> (Vec<RedrawEvent>, Vec<Notification>) {
    let mut out = Vec::new();
    let mut custom = Vec::new();
    let Some(arr) = v.as_array() else { return (out, custom) };
    if arr.len() != 3 || arr[0].as_u64() != Some(2) {
        return (out, custom);
    }
    if arr[1].as_str() != Some("redraw") {
        if let (Some(name), Some(params)) = (arr[1].as_str(), arr[2].as_array()) {
            custom.push((name.to_string(), params.clone()));
        }
        return (out, custom);
    }
    let Some(events) = arr[2].as_array() else { return (out, custom) };
    for ev in events {
        let Some(parts) = ev.as_array() else { continue };
        let Some(name) = parts.first().and_then(|n| n.as_str()) else { continue };
        for call in &parts[1..] {
            if let Some(args) = call.as_array() {
                out.push((name.to_string(), args.clone()));
            }
        }
    }
    (out, custom)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finder_style_path_keeps_portable_fallbacks() {
        let binary = if cfg!(windows) { "nvim.exe" } else { "nvim" };
        let configured = PathBuf::from("custom-nvim");
        let first_path = PathBuf::from("first-bin");
        let second_path = PathBuf::from("second-bin");
        let path = std::env::join_paths([&first_path, &second_path]).unwrap();
        let home = PathBuf::from("test-home");
        let candidates = nvim_candidates(
            Some(configured.as_os_str()),
            Some(path.as_os_str()),
            Some(home.as_os_str()),
        );
        assert_eq!(candidates[0], configured);
        assert!(candidates.contains(&first_path.join(binary)));
        assert!(candidates.contains(&home.join(".local/bin").join(binary)));
        #[cfg(target_os = "macos")]
        assert!(candidates.contains(&PathBuf::from("/opt/homebrew/bin/nvim")));
    }

    #[test]
    fn multigrid_is_requested_only_when_asked_for() {
        let has = |opts: &Value, key: &str| {
            opts.as_map()
                .unwrap()
                .iter()
                .any(|(k, v)| k.as_str() == Some(key) && v.as_bool() == Some(true))
        };
        let on = attach_options(true);
        assert!(has(&on, "ext_linegrid") && has(&on, "rgb") && has(&on, "ext_multigrid"));
        let off = attach_options(false);
        assert!(has(&off, "ext_linegrid") && has(&off, "rgb") && !has(&off, "ext_multigrid"));
    }

    #[test]
    fn parses_the_embedded_client_channel_id() {
        let response = Value::Array(vec![
            Value::from(1u8),
            Value::from(7u64),
            Value::Nil,
            Value::Array(vec![Value::from(42u64), Value::Map(vec![])]),
        ]);
        assert_eq!(parse_api_channel_id(&response, 7).unwrap(), 42);
    }
}
