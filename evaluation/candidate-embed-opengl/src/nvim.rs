//! Msgpack-RPC transport to a `nvim --embed` child process.
//!
//! Neovim is the editing engine; this process is only its UI. Nothing here
//! interprets buffers or keymaps — it forwards input and consumes `redraw`.

use std::io::{BufReader, Write};
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
        let mut cmd = Command::new("nvim");
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

    pub fn ui_attach(&mut self, cols: u32, rows: u32) -> std::io::Result<()> {
        let opts = Value::Map(vec![
            (Value::from("ext_linegrid"), Value::from(true)),
            (Value::from("rgb"), Value::from(true)),
        ]);
        self.request("nvim_ui_attach", vec![Value::from(cols), Value::from(rows), opts])
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

/// A notification is `[2, method, params]`; for `redraw`, params is a list of
/// `[event_name, args…]` where each `args` is itself one invocation. Anything
/// else is a message from Lua and is passed through untouched.
fn collect(v: &Value, out: &mut Vec<RedrawEvent>, custom: &mut Vec<Notification>) {
    let Some(arr) = v.as_array() else { return };
    if arr.len() != 3 || arr[0].as_u64() != Some(2) {
        return;
    }
    if arr[1].as_str() != Some("redraw") {
        if let (Some(name), Some(params)) = (arr[1].as_str(), arr[2].as_array()) {
            custom.push((name.to_string(), params.clone()));
        }
        return;
    }
    let Some(events) = arr[2].as_array() else { return };
    for ev in events {
        let Some(parts) = ev.as_array() else { continue };
        let Some(name) = parts.first().and_then(|n| n.as_str()) else { continue };
        for call in &parts[1..] {
            if let Some(args) = call.as_array() {
                out.push((name.to_string(), args.clone()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
