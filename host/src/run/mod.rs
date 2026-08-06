//! The task execution boundary.
//!
//! Every task child process goes through [`spawn`]. The only accepted origins
//! are a key the owner pressed (`Origin::OwnerKey`) or a `:` line the owner
//! typed (`Origin::OwnerExCommand`). File contents, modelines and plugin Lua do
//! not call this door. `aish run` and `aish exec` remain closed.
//!
//! This is not a decision for `open_question plugin_effect_boundary`: plugin
//! effects are a separate line. This module only names where owner-requested
//! task processes enter the host.
//!
//! The current implementation uses pipes, measured here with
//! `/bin/sh -c 'test -t 1'` returning status 1. A pipe cannot provide terminal
//! facts and controls that a PTY would: `isatty`, terminal window size and
//! `SIGWINCH`, a process group for terminal Ctrl-C delivery, canonical/raw
//! input modes, or alternate-screen and cursor-addressing semantics.

use std::io::Read;
use std::time::Duration;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    OwnerKey,
    OwnerExCommand,
}

impl Origin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OwnerKey => "key",
            Self::OwnerExCommand => "ex",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    Stdout,
    Stderr,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnsiColor {
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    BrightBlack,
    BrightRed,
    BrightGreen,
    BrightYellow,
    BrightBlue,
    BrightMagenta,
    BrightCyan,
    BrightWhite,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Role {
    pub stream: Stream,
    pub color: Option<AnsiColor>,
    pub bold: bool,
}

impl Role {
    fn new(stream: Stream) -> Self {
        Self {
            stream,
            color: None,
            bold: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment {
    pub text: String,
    pub role: Role,
}

enum Event {
    Output(Vec<Segment>),
    Eof,
}

pub struct Task {
    pub origin: Origin,
    pub argv: Vec<String>,
    pub cwd: PathBuf,
    child: Child,
    rx: Receiver<Event>,
    eof: usize,
    status: Option<ExitStatus>,
    /// Output drained by `status` on its way to the exit code. It is the
    /// program's output like any other and is handed to the next `poll`.
    pending: Vec<Segment>,
}

impl Task {
    pub fn poll(&mut self) -> Vec<Segment> {
        let mut out = std::mem::take(&mut self.pending);
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::Output(mut segments) => out.append(&mut segments),
                Event::Eof => self.eof += 1,
            }
        }
        if self.eof >= 2 && self.status.is_none() {
            if let Ok(Some(status)) = self.child.try_wait() {
                self.status = Some(status);
            }
        }
        out
    }

    pub fn cancel(&mut self) {
        let _ = self.child.kill();
        if let Ok(status) = self.child.wait() {
            self.status = Some(status);
        }
    }

    pub fn status(&mut self) -> Option<ExitStatus> {
        // Draining is what makes the exit status observable — the child is only
        // reaped once both pipes have reported EOF. But the output drained on
        // the way there belongs to the caller. Discarding it lost the entire
        // stdout of any command fast enough to finish inside a single tick,
        // which rendered as an empty panel above a clean `exit status 0`.
        let carried = self.poll();
        self.pending = carried;
        self.status
    }
}

impl Drop for Task {
    fn drop(&mut self) {
        if self.status.is_none() {
            self.cancel();
        }
    }
}

pub fn spawn(origin: Origin, argv: Vec<String>, cwd: PathBuf) -> std::io::Result<Task> {
    let Some(program) = argv.first() else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "empty argv",
        ));
    };
    let mut command = Command::new(program);
    command
        .args(&argv[1..])
        .current_dir(&cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    let mut child = command.spawn()?;
    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");
    let (tx, rx) = mpsc::channel();
    reader(stdout, Stream::Stdout, tx.clone());
    reader(stderr, Stream::Stderr, tx);
    Ok(Task {
        origin,
        argv,
        cwd,
        child,
        rx,
        eof: 0,
        status: None,
        pending: Vec::new(),
    })
}

fn reader(mut input: impl Read + Send + 'static, stream: Stream, tx: mpsc::Sender<Event>) {
    std::thread::Builder::new()
        .name(match stream {
            Stream::Stdout => "nvimglsl-task-stdout".into(),
            Stream::Stderr => "nvimglsl-task-stderr".into(),
        })
        .spawn(move || {
            let mut decoder = AnsiDecoder::new(stream);
            let mut buf = [0u8; 4096];
            loop {
                match input.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let segments = decoder.push(&buf[..n]);
                        if !segments.is_empty() && tx.send(Event::Output(segments)).is_err() {
                            return;
                        }
                    }
                    Err(_) => break,
                }
            }
            let segments = decoder.finish();
            if !segments.is_empty() {
                let _ = tx.send(Event::Output(segments));
            }
            let _ = tx.send(Event::Eof);
        })
        .ok();
}

enum State {
    Ground,
    Esc,
    Csi(String),
    Osc,
    OscEsc,
}

struct AnsiDecoder {
    role: Role,
    state: State,
    text: String,
    segments: Vec<Segment>,
}

impl AnsiDecoder {
    fn new(stream: Stream) -> Self {
        Self {
            role: Role::new(stream),
            state: State::Ground,
            text: String::new(),
            segments: Vec::new(),
        }
    }

    fn push(&mut self, bytes: &[u8]) -> Vec<Segment> {
        let text = String::from_utf8_lossy(bytes);
        for ch in text.chars() {
            match &mut self.state {
                State::Ground => {
                    if ch == '\x1b' {
                        self.flush_text();
                        self.state = State::Esc;
                    } else {
                        self.text.push(ch);
                    }
                }
                State::Esc => match ch {
                    '[' => self.state = State::Csi(String::new()),
                    ']' => self.state = State::Osc,
                    _ => self.state = State::Ground,
                },
                State::Csi(body) => {
                    if ('@'..='~').contains(&ch) {
                        if ch == 'm' {
                            let body = std::mem::take(body);
                            self.apply_sgr(&body);
                        }
                        self.state = State::Ground;
                    } else {
                        body.push(ch);
                    }
                }
                State::Osc => {
                    if ch == '\x07' {
                        self.state = State::Ground;
                    } else if ch == '\x1b' {
                        self.state = State::OscEsc;
                    }
                }
                State::OscEsc => {
                    self.state = State::Ground;
                }
            }
        }
        std::mem::take(&mut self.segments)
    }

    fn finish(&mut self) -> Vec<Segment> {
        self.flush_text();
        std::mem::take(&mut self.segments)
    }

    fn flush_text(&mut self) {
        if self.text.is_empty() {
            return;
        }
        self.segments.push(Segment {
            text: std::mem::take(&mut self.text),
            role: self.role,
        });
    }

    fn apply_sgr(&mut self, body: &str) {
        let codes: Vec<u16> = if body.is_empty() {
            vec![0]
        } else {
            body.split(';')
                .filter_map(|part| part.parse().ok())
                .collect()
        };
        for code in codes {
            match code {
                0 => {
                    self.role.color = None;
                    self.role.bold = false;
                }
                1 => self.role.bold = true,
                22 => self.role.bold = false,
                39 => self.role.color = None,
                30 => self.role.color = Some(AnsiColor::Black),
                31 => self.role.color = Some(AnsiColor::Red),
                32 => self.role.color = Some(AnsiColor::Green),
                33 => self.role.color = Some(AnsiColor::Yellow),
                34 => self.role.color = Some(AnsiColor::Blue),
                35 => self.role.color = Some(AnsiColor::Magenta),
                36 => self.role.color = Some(AnsiColor::Cyan),
                37 => self.role.color = Some(AnsiColor::White),
                90 => self.role.color = Some(AnsiColor::BrightBlack),
                91 => self.role.color = Some(AnsiColor::BrightRed),
                92 => self.role.color = Some(AnsiColor::BrightGreen),
                93 => self.role.color = Some(AnsiColor::BrightYellow),
                94 => self.role.color = Some(AnsiColor::BrightBlue),
                95 => self.role.color = Some(AnsiColor::BrightMagenta),
                96 => self.role.color = Some(AnsiColor::BrightCyan),
                97 => self.role.color = Some(AnsiColor::BrightWhite),
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sgr_decodes_the_allowed_roles_and_drops_other_csi() {
        let mut decoder = AnsiDecoder::new(Stream::Stdout);
        let mut out = decoder.push(b"a\x1b[31mred\x1b[22m still-red\x1b[39m plain\x1b[2Jx");
        out.extend(decoder.finish());
        assert_eq!(out[0].text, "a");
        assert_eq!(out[1].text, "red");
        assert_eq!(out[1].role.color, Some(AnsiColor::Red));
        assert_eq!(out[2].role.color, Some(AnsiColor::Red));
        assert_eq!(out[3].text, " plain");
        assert_eq!(out[3].role.color, None);
        assert_eq!(out[4].text, "x");
    }

    #[test]
    fn pipe_stdout_is_not_a_tty_on_this_boundary() {
        let mut task = spawn(
            Origin::OwnerExCommand,
            vec!["/bin/sh".into(), "-c".into(), "test -t 1".into()],
            std::env::current_dir().unwrap(),
        )
        .unwrap();
        while task.status().is_none() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(task.status().unwrap().code(), Some(1));
    }
}

#[cfg(test)]
mod output_probe {
    use super::*;

    #[test]
    fn a_short_command_keeps_its_stdout() {
        let mut task = spawn(
            Origin::OwnerKey,
            vec!["/bin/echo".into(), "hello".into()],
            std::env::temp_dir(),
        )
        .unwrap();
        let mut seen = Vec::new();
        for _ in 0..200 {
            let mut segments = task.poll();
            let status = task.status();
            segments.extend(task.poll());
            seen.append(&mut segments);
            if status.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let text: String = seen.iter().map(|s| s.text.as_str()).collect();
        assert!(text.contains("hello"), "stdout lost: {seen:?} -> {text:?}");
    }
}
