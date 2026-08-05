//! The system clipboard, for `clipboard = unnamedplus`.
//!
//! Shelling out to the platform tool rather than linking a clipboard crate: the
//! yank register is not on any hot path, and every failure here should lose the
//! clipboard rather than the yank. Nothing in this module returns an error for
//! that reason — a missing `pbcopy` is a clipboard that does not work, not an
//! edit that does not happen.

use std::io::Write;
use std::process::{Command, Stdio};

#[cfg(target_os = "macos")]
const COPY: (&str, &[&str]) = ("pbcopy", &[]);
#[cfg(target_os = "macos")]
const PASTE: (&str, &[&str]) = ("pbpaste", &[]);

#[cfg(target_os = "linux")]
const COPY: (&str, &[&str]) = ("wl-copy", &[]);
#[cfg(target_os = "linux")]
const PASTE: (&str, &[&str]) = ("wl-paste", &["--no-newline"]);

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
const COPY: (&str, &[&str]) = ("", &[]);
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
const PASTE: (&str, &[&str]) = ("", &[]);

pub fn write(text: &str) {
    if COPY.0.is_empty() {
        return;
    }
    let Ok(mut child) = Command::new(COPY.0)
        .args(COPY.1)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
    else {
        return;
    };
    if let Some(stdin) = child.stdin.as_mut() {
        let _ = stdin.write_all(text.as_bytes());
    }
    let _ = child.wait();
}

pub fn read() -> Option<String> {
    if PASTE.0.is_empty() {
        return None;
    }
    let output = Command::new(PASTE.0).args(PASTE.1).output().ok()?;
    String::from_utf8(output.stdout).ok()
}

#[cfg(test)]
mod tests {
    /// A round trip through the real clipboard, when there is one.
    ///
    /// Deliberately restores whatever was there: a test that leaves the
    /// owner's clipboard holding its fixture has broken something outside
    /// itself.
    ///
    /// The probe is not ceremony. `write` swallows its errors by design — a
    /// failed `pbcopy` should lose the clipboard, not the yank — so a process
    /// that may read the pasteboard but not write it produces `Some("")` here
    /// rather than an error. That is exactly what a sandboxed agent gets, and a
    /// baseline only the owner's own shell can reproduce is not a baseline. So
    /// the environment is asked whether it can write at all, and the real
    /// assertion runs only where the answer is yes.
    #[test]
    fn text_survives_a_round_trip_and_the_clipboard_is_put_back() {
        const PROBE: &str = "nvimglsl clipboard probe";
        const BODY: &str = "nvimglsl clipboard round trip\nsecond line";
        let Some(before) = super::read() else { return };
        super::write(PROBE);
        if super::read().as_deref() != Some(PROBE) {
            super::write(&before);
            return;
        }
        super::write(BODY);
        let read_back = super::read();
        super::write(&before);
        assert_eq!(read_back.as_deref(), Some(BODY));
    }
}
