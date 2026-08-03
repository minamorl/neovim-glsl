//! The claim `pin architecture_choice` makes is that this host *speaks the
//! Neovim protocol*. A test that called the host's own functions could not
//! check that: it would pass just as well if the protocol had quietly become a
//! private in-process convention.
//!
//! So this test does what an outside UI client does. It runs the built binary
//! as a separate process, attaches over its stdio with msgpack-RPC, types, and
//! reads the grid back out of the `redraw` stream — nothing else about the host
//! is reachable from here.

use std::io::{BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::time::Duration;

use rmpv::Value;

struct Client {
    child: Child,
    /// Held in an `Option` so a test can drop the write end — that is how a UI
    /// client leaves, and the host's exit is part of the contract.
    stdin: Option<ChildStdin>,
    messages: Receiver<Value>,
    next_msgid: u64,
}

impl Client {
    fn start(args: &[&str]) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_nvimglsl"))
            .arg("--embed")
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("the host binary starts");
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            while let Ok(value) = rmpv::decode::read_value(&mut reader) {
                if tx.send(value).is_err() {
                    break;
                }
            }
        });
        Self { child, stdin: Some(stdin), messages: rx, next_msgid: 1 }
    }

    fn send(&mut self, message: Value) {
        let mut buffer = Vec::new();
        rmpv::encode::write_value(&mut buffer, &message).expect("encodes");
        let stdin = self.stdin.as_mut().expect("the client still holds its pipe");
        stdin.write_all(&buffer).expect("writes");
        stdin.flush().expect("flushes");
    }

    fn leave(&mut self) {
        self.stdin = None;
    }

    fn request(&mut self, method: &str, params: Vec<Value>) -> u64 {
        let msgid = self.next_msgid;
        self.next_msgid += 1;
        self.send(Value::Array(vec![
            Value::from(0u8),
            Value::from(msgid),
            Value::from(method),
            Value::Array(params),
        ]));
        msgid
    }

    fn notify(&mut self, method: &str, params: Vec<Value>) {
        self.send(Value::Array(vec![
            Value::from(2u8),
            Value::from(method),
            Value::Array(params),
        ]));
    }

    /// Collect traffic until nothing arrives for a moment.
    ///
    /// The first message gets a long deadline and the rest a short one. A single
    /// short deadline makes this test a race against process startup: eight of
    /// these run at once, and on a loaded machine the first byte can take longer
    /// to appear than the whole quiet period that follows it.
    fn drain(&mut self) -> Vec<Value> {
        let mut out = Vec::new();
        let mut timeout = Duration::from_secs(10);
        loop {
            match self.messages.recv_timeout(timeout) {
                Ok(value) => {
                    out.push(value);
                    timeout = Duration::from_millis(300);
                }
                Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        out
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Flatten `redraw` notifications the way any UI client has to.
fn redraw_events(messages: &[Value]) -> Vec<(String, Vec<Value>)> {
    let mut out = Vec::new();
    for message in messages {
        let Some(parts) = message.as_array() else { continue };
        if parts.len() != 3 || parts[0].as_u64() != Some(2) || parts[1].as_str() != Some("redraw") {
            continue;
        }
        let Some(batches) = parts[2].as_array() else { continue };
        for batch in batches {
            let Some(batch) = batch.as_array() else { continue };
            let Some(name) = batch.first().and_then(Value::as_str) else { continue };
            for call in &batch[1..] {
                if let Some(args) = call.as_array() {
                    out.push((name.to_string(), args.clone()));
                }
            }
        }
    }
    out
}

/// Rebuild one grid row from `grid_line`, expanding the run lengths.
fn row_text(events: &[(String, Vec<Value>)], row: u64) -> Option<String> {
    events.iter().rev().find_map(|(name, args)| {
        if name != "grid_line" || args.get(1)?.as_u64() != Some(row) {
            return None;
        }
        let mut text = String::new();
        for cell in args.get(3)?.as_array()? {
            let parts = cell.as_array()?;
            let chunk = parts.first()?.as_str().unwrap_or("");
            let repeat = parts.get(2).and_then(Value::as_u64).unwrap_or(1);
            for _ in 0..repeat {
                text.push_str(chunk);
            }
        }
        Some(text)
    })
}

fn attach(client: &mut Client, cols: u64, rows: u64) -> Vec<(String, Vec<Value>)> {
    client.request(
        "nvim_ui_attach",
        vec![
            Value::from(cols),
            Value::from(rows),
            Value::Map(vec![
                (Value::from("ext_linegrid"), Value::from(true)),
                (Value::from("rgb"), Value::from(true)),
            ]),
        ],
    );
    redraw_events(&client.drain())
}

#[test]
fn an_outside_client_can_attach_and_receives_a_full_first_paint() {
    let mut client = Client::start(&[]);
    let events = attach(&mut client, 40, 8);
    let names: Vec<&str> = events.iter().map(|(name, _)| name.as_str()).collect();
    for expected in ["grid_resize", "default_colors_set", "hl_attr_define", "grid_line", "flush"] {
        assert!(names.contains(&expected), "{expected} missing from {names:?}");
    }
}

#[test]
fn typing_over_the_protocol_comes_back_as_grid_content() {
    let mut client = Client::start(&[]);
    attach(&mut client, 40, 8);
    client.notify("nvim_input", vec![Value::from("ihello<Esc>")]);
    let events = redraw_events(&client.drain());
    let row = row_text(&events, 0).expect("the first row was repainted");
    assert!(row.contains("hello"), "row was {row:?}");
}

#[test]
fn the_api_face_reports_a_channel_and_a_neovim_version() {
    let mut client = Client::start(&[]);
    let msgid = client.request("nvim_get_api_info", vec![]);
    let messages = client.drain();
    let response = messages
        .iter()
        .filter_map(|m| m.as_array())
        .find(|parts| parts[0].as_u64() == Some(1) && parts[1].as_u64() == Some(msgid))
        .expect("a response to the request we sent");
    assert!(response[2].is_nil(), "error slot was {:?}", response[2]);
    let info = response[3].as_array().expect("api info array");
    assert_eq!(info[0].as_u64(), Some(1), "channel id");
    let map = info[1].as_map().expect("api info map");
    assert!(map.iter().any(|(k, _)| k.as_str() == Some("version")));
}

#[test]
fn buffer_lines_read_back_what_was_typed() {
    let mut client = Client::start(&[]);
    attach(&mut client, 40, 8);
    client.notify("nvim_input", vec![Value::from("ione<CR>two<Esc>")]);
    client.drain();
    let msgid = client.request(
        "nvim_buf_get_lines",
        vec![
            Value::from(0u64),
            Value::from(0u64),
            Value::from(-1i64),
            Value::from(false),
        ],
    );
    let messages = client.drain();
    let response = messages
        .iter()
        .filter_map(|m| m.as_array())
        .find(|parts| parts[0].as_u64() == Some(1) && parts[1].as_u64() == Some(msgid))
        .expect("a response");
    let lines: Vec<&str> =
        response[3].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert_eq!(lines, vec!["one", "two"]);
}

#[test]
fn the_host_announces_its_own_events_as_ordinary_notifications() {
    let mut client = Client::start(&[]);
    attach(&mut client, 40, 8);
    client.notify("nvim_input", vec![Value::from(" o")]);
    let messages = client.drain();
    let names: Vec<&str> = messages
        .iter()
        .filter_map(|m| m.as_array())
        .filter(|parts| parts[0].as_u64() == Some(2))
        .filter_map(|parts| parts[1].as_str())
        .collect();
    assert!(
        names.contains(&"nvimglsl_navigate"),
        "the navigation request did not reach the client: {names:?}"
    );
}

#[test]
fn a_client_that_never_attaches_still_gets_answers_but_no_redraw() {
    let mut client = Client::start(&[]);
    client.notify("nvim_input", vec![Value::from("ihello<Esc>")]);
    let messages = client.drain();
    assert!(
        redraw_events(&messages).is_empty(),
        "redraw traffic was sent to a client that never attached"
    );
}

#[test]
fn closing_the_pipe_ends_the_host() {
    let mut client = Client::start(&[]);
    attach(&mut client, 40, 8);
    // Dropping our write end is how a UI client leaves.
    client.leave();
    let mut waited = 0;
    loop {
        match client.child.try_wait().expect("wait") {
            Some(status) => {
                assert!(status.success(), "the host exited with {status}");
                return;
            }
            None if waited > 40 => panic!("the host did not exit after its client left"),
            None => {
                std::thread::sleep(Duration::from_millis(50));
                waited += 1;
            }
        }
    }
}

/// Not part of the protocol claim, but it guards the same boundary: a UI client
/// must not be able to make the host read a file it was not given.
#[test]
fn an_unknown_method_is_refused_by_name() {
    let mut client = Client::start(&[]);
    let msgid = client.request("nvim_open_the_pod_bay_doors", vec![]);
    let messages = client.drain();
    let response = messages
        .iter()
        .filter_map(|m| m.as_array())
        .find(|parts| parts[0].as_u64() == Some(1) && parts[1].as_u64() == Some(msgid))
        .expect("a response");
    let error = response[2].as_str().expect("an error string");
    assert!(error.contains("nvim_open_the_pod_bay_doors"), "error was {error:?}");
}
