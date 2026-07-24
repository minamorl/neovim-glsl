//! Msgpack-RPC transport to a `nvim --embed` child process.
//!
//! Neovim is the editing engine; this process is only its UI. Nothing here
//! interprets buffers or keymaps — it forwards input and consumes `redraw`.

use std::io::{BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::thread;

use rmpv::Value;

pub struct Nvim {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<Value>,
    next_msgid: u64,
}

/// A `redraw` batch already split into its individual UI events.
pub type RedrawEvent = (String, Vec<Value>);

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

        Ok(Self { child, stdin, rx, next_msgid: 1 })
    }

    pub fn request(&mut self, method: &str, params: Vec<Value>) -> std::io::Result<()> {
        let msgid = self.next_msgid;
        self.next_msgid += 1;
        let msg = Value::Array(vec![
            Value::from(0u8),
            Value::from(msgid),
            Value::from(method),
            Value::Array(params),
        ]);
        self.send(msg)
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

    /// Drain everything currently queued, returning only the flattened `redraw`
    /// events. Responses to our own requests carry no UI state and are dropped.
    pub fn drain_redraw(&mut self) -> (Vec<RedrawEvent>, bool) {
        let mut out = Vec::new();
        let mut closed = false;
        loop {
            match self.rx.try_recv() {
                Ok(v) => collect_redraw(&v, &mut out),
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
            Ok(v) => collect_redraw(&v, &mut out),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => return (out, false),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return (out, true),
        }
        let (mut rest, closed) = self.drain_redraw();
        out.append(&mut rest);
        (out, closed)
    }
}

impl Drop for Nvim {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A notification is `[2, method, params]`; for `redraw`, params is a list of
/// `[event_name, args…]` where each `args` is itself one invocation.
fn collect_redraw(v: &Value, out: &mut Vec<RedrawEvent>) {
    let Some(arr) = v.as_array() else { return };
    if arr.len() != 3 || arr[0].as_u64() != Some(2) {
        return;
    }
    if arr[1].as_str() != Some("redraw") {
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
