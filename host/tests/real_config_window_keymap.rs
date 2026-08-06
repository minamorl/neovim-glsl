use std::io::{BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::time::Duration;

use rmpv::Value;

struct Client {
    child: Child,
    stdin: ChildStdin,
    messages: Receiver<Value>,
    next_msgid: u64,
}

impl Client {
    fn start(args: &[&str]) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_nvimglsl"))
            .arg("--embed")
            .args(args)
            .env("NVIMGLSL_CONFIGURED_EMBED", "1")
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
        Self {
            child,
            stdin,
            messages: rx,
            next_msgid: 1,
        }
    }

    fn send(&mut self, message: Value) {
        let mut buffer = Vec::new();
        rmpv::encode::write_value(&mut buffer, &message).expect("encodes");
        self.stdin.write_all(&buffer).expect("writes");
        self.stdin.flush().expect("flushes");
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

    fn input(&mut self, keys: &str) {
        self.notify("nvim_input", vec![Value::from(keys)]);
    }

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

fn real_config_exists() -> bool {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join(".config/nvim/init.lua").exists())
        .unwrap_or(false)
}

fn redraw_events(messages: &[Value]) -> Vec<(String, Vec<Value>)> {
    let mut out = Vec::new();
    for message in messages {
        let Some(parts) = message.as_array() else {
            continue;
        };
        if parts.len() != 3 || parts[0].as_u64() != Some(2) || parts[1].as_str() != Some("redraw") {
            continue;
        }
        let Some(batches) = parts[2].as_array() else {
            continue;
        };
        for batch in batches {
            let Some(batch) = batch.as_array() else {
                continue;
            };
            let Some(name) = batch.first().and_then(Value::as_str) else {
                continue;
            };
            for call in &batch[1..] {
                if let Some(args) = call.as_array() {
                    out.push((name.to_string(), args.clone()));
                }
            }
        }
    }
    out
}

fn attach(client: &mut Client) {
    client.request(
        "nvim_ui_attach",
        vec![
            Value::from(50u64),
            Value::from(10u64),
            Value::Map(vec![
                (Value::from("ext_linegrid"), Value::from(true)),
                (Value::from("ext_multigrid"), Value::from(true)),
                (Value::from("rgb"), Value::from(true)),
            ]),
        ],
    );
    client.drain();
}

fn cursor(events: &[(String, Vec<Value>)]) -> Option<(u64, u64, u64)> {
    events.iter().rev().find_map(|(name, args)| {
        (name == "grid_cursor_goto").then(|| {
            Some((
                args.first()?.as_u64()?,
                args.get(1)?.as_u64()?,
                args.get(2)?.as_u64()?,
            ))
        })?
    })
}

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

fn current_name(client: &mut Client) -> String {
    let msgid = client.request("nvim_buf_get_name", vec![Value::from(0u64)]);
    let messages = client.drain();
    messages
        .iter()
        .filter_map(|m| m.as_array())
        .find(|parts| parts[0].as_u64() == Some(1) && parts[1].as_u64() == Some(msgid))
        .and_then(|parts| parts[3].as_str())
        .unwrap_or_default()
        .to_string()
}

fn temp_file(name: &str, text: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nvimglsl-real-config-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path
}

#[test]
fn the_real_config_tab_hjkl_move_focus_without_moving_columns() {
    if !real_config_exists() {
        return;
    }
    let mut client = Client::start(&[]);
    attach(&mut client);
    client.input("ll<C-w>v<tab>h");
    let left = cursor(&redraw_events(&client.drain())).expect("left cursor");
    client.input("<tab>l");
    let right = cursor(&redraw_events(&client.drain())).expect("right cursor");
    assert_ne!(left.0, right.0);
    assert_eq!(left.2, right.2);

    let mut client = Client::start(&[]);
    attach(&mut client);
    client.input("ll<C-w>s<tab>k");
    let upper = cursor(&redraw_events(&client.drain())).expect("upper cursor");
    client.input("<tab>j");
    let lower = cursor(&redraw_events(&client.drain())).expect("lower cursor");
    assert_ne!(upper.0, lower.0);
    assert_eq!(upper.2, lower.2);
}

#[test]
fn the_real_config_leader_jkl_follow_barbar_and_h_stays_neoclip() {
    if !real_config_exists() {
        return;
    }
    let one = temp_file("one.md", "one\n");
    let two = temp_file("two.md", "two\n");
    let mut client = Client::start(&[one.to_str().unwrap()]);
    attach(&mut client);
    client.input(&format!(":e {}<CR>", two.display()));
    client.drain();

    client.input(" j");
    client.drain();
    assert_eq!(current_name(&mut client), one.display().to_string());
    client.input(" k");
    client.drain();
    assert_eq!(current_name(&mut client), two.display().to_string());
    client.input(" j");
    client.drain();
    assert_eq!(current_name(&mut client), one.display().to_string());
    client.input(" l");
    client.drain();
    assert_eq!(current_name(&mut client), two.display().to_string());

    client.input(" h");
    let events = redraw_events(&client.drain());
    assert_eq!(
        current_name(&mut client),
        two.display().to_string(),
        "<Leader>h must not be treated as BufferPrevious"
    );
    assert!(
        row_text(&events, 9).is_some_and(|text| text.contains("Telescope neoclip")),
        "<Leader>h should be the later neoclip mapping: {events:?}"
    );

    let _ = std::fs::remove_dir_all(one.parent().unwrap());
}
