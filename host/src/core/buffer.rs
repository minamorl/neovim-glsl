//! Text storage for the own host.
//!
//! Lines are `Vec<char>` rather than `String` because every editing operation
//! here is addressed by character position, and a `String` would make each of
//! them a byte-index search. The cost of that choice is paid once, on load.
//!
//! A column in this module is a *character* index. What it looks like on the
//! screen is not decided here — a CJK character is one column to the editing
//! core and two cells to the renderer, and folding those two meanings into one
//! number is how a cursor ends up one cell off in Japanese text.

use std::path::{Path, PathBuf};

/// `O_NONBLOCK`. One integer is not worth a `libc` dependency, and the value is
/// frozen by each platform's ABI — it cannot change without breaking every
/// binary already built against it.
#[cfg(any(target_os = "macos", target_os = "ios"))]
const O_NONBLOCK: i32 = 0x0004;
#[cfg(target_os = "linux")]
const O_NONBLOCK: i32 = 0o4000;

/// Read a file's text without ever blocking in `open(2)`.
///
/// A path handed over by the file tree is whatever the filesystem holds, and
/// not all of it is a file. Opening a FIFO that has no writer blocks in the
/// kernel **forever** — there is no timeout to raise and no signal to wait for.
/// The window keeps repainting because only the editor thread is wedged, so it
/// does not look like a hang; it looks like the whole application froze.
///
/// `O_NONBLOCK` makes the open itself return, and the check below asks the
/// *opened descriptor* what it is rather than the path, which would leave a
/// window for the answer to change in between.
fn read_regular_file(path: &Path) -> std::io::Result<String> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;

    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(O_NONBLOCK)
        .open(path)?;
    if !file.metadata()?.file_type().is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{} is not a regular file", path.display()),
        ));
    }
    // `O_NONBLOCK` has no effect on reads from a regular file, so this is a
    // plain blocking read of something that is guaranteed to end.
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    Ok(text)
}

/// One undoable state of the buffer.
///
/// The whole text is stored, not a diff. `free host_editing_core_design` leaves
/// the undo representation open, and a snapshot is the representation that
/// cannot be wrong about what it restores.
#[derive(Clone)]
pub struct Snapshot {
    lines: Vec<Vec<char>>,
    pub cursor: (usize, usize),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LineEnding {
    Lf,
    Crlf,
}

impl LineEnding {
    fn as_str(self) -> &'static str {
        match self {
            LineEnding::Lf => "\n",
            LineEnding::Crlf => "\r\n",
        }
    }
}

#[derive(Clone)]
pub struct Buffer {
    lines: Vec<Vec<char>>,
    path: Option<PathBuf>,
    modified: bool,
    revision: u64,
    ending: LineEnding,
    /// Whether the file ended with a newline. Preserved so that writing a file
    /// back unchanged produces the same bytes.
    trailing_newline: bool,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// Set while an operation is in flight so that a multi-key change (`3dd`)
    /// becomes one undo step rather than three.
    pending: Option<Snapshot>,
}

impl Default for Buffer {
    fn default() -> Self {
        Self::empty()
    }
}

impl Buffer {
    pub fn empty() -> Self {
        Self {
            lines: vec![Vec::new()],
            path: None,
            modified: false,
            revision: 0,
            ending: LineEnding::Lf,
            trailing_newline: true,
            undo: Vec::new(),
            redo: Vec::new(),
            pending: None,
        }
    }

    pub fn from_text(text: &str) -> Self {
        let mut buffer = Self::empty();
        buffer.set_text(text);
        buffer.modified = false;
        buffer
    }

    pub fn open(path: &Path) -> std::io::Result<Self> {
        let mut buffer = match read_regular_file(path) {
            Ok(text) => {
                let mut b = Self::empty();
                b.set_text(&text);
                b
            }
            // Opening a path that does not exist yet is how a new file is
            // started, not an error. Anything else is.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::empty(),
            Err(e) => return Err(e),
        };
        buffer.path = Some(path.to_path_buf());
        buffer.modified = false;
        Ok(buffer)
    }

    fn set_text(&mut self, text: &str) {
        self.ending = if text.contains("\r\n") {
            LineEnding::Crlf
        } else {
            LineEnding::Lf
        };
        self.trailing_newline = text.is_empty() || text.ends_with('\n');
        let body = text.strip_suffix('\n').unwrap_or(text);
        let body = body.strip_suffix('\r').unwrap_or(body);
        self.lines = body
            .split('\n')
            .map(|line| line.strip_suffix('\r').unwrap_or(line).chars().collect())
            .collect();
        if self.lines.is_empty() {
            self.lines.push(Vec::new());
        }
        self.bump_revision();
    }

    fn bump_revision(&mut self) {
        self.revision = self.revision.saturating_add(1);
    }

    pub fn text(&self) -> String {
        let mut out = String::new();
        for (index, line) in self.lines.iter().enumerate() {
            if index > 0 {
                out.push_str(self.ending.as_str());
            }
            out.extend(line.iter());
        }
        if self.trailing_newline {
            out.push_str(self.ending.as_str());
        }
        out
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn set_path(&mut self, path: PathBuf) {
        self.path = Some(path);
    }

    pub fn name(&self) -> String {
        match &self.path {
            Some(path) => path.display().to_string(),
            None => "[No Name]".to_string(),
        }
    }

    pub fn modified(&self) -> bool {
        self.modified
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn write(&mut self, to: Option<&Path>) -> std::io::Result<PathBuf> {
        let target = match to {
            Some(path) => path.to_path_buf(),
            None => self.path.clone().ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "no file name")
            })?,
        };
        if let Some(parent) = target.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        std::fs::write(&target, self.text())?;
        if to.is_none() || self.path.is_none() {
            self.path = Some(target.clone());
        }
        self.modified = false;
        Ok(target)
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn line(&self, index: usize) -> &[char] {
        self.lines.get(index).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn line_text(&self, index: usize) -> String {
        self.line(index).iter().collect()
    }

    pub fn line_len(&self, index: usize) -> usize {
        self.lines.get(index).map(Vec::len).unwrap_or(0)
    }

    pub fn lines_text(&self, start: usize, end: usize) -> Vec<String> {
        let end = end.min(self.lines.len());
        if start >= end {
            return Vec::new();
        }
        self.lines[start..end]
            .iter()
            .map(|l| l.iter().collect())
            .collect()
    }

    // --- undo -------------------------------------------------------------
    //
    // `begin_change` is idempotent within one operation: an operator that
    // touches several lines calls it once per key sequence, not once per line.

    pub fn begin_change(&mut self, cursor: (usize, usize)) {
        if self.pending.is_none() {
            self.pending = Some(Snapshot {
                lines: self.lines.clone(),
                cursor,
            });
        }
    }

    pub fn commit_change(&mut self) {
        if let Some(snapshot) = self.pending.take() {
            if snapshot.lines != self.lines {
                self.undo.push(snapshot);
                self.redo.clear();
                self.modified = true;
                self.bump_revision();
            }
        }
    }

    /// Drop an opened change without recording it. Used when an operation turns
    /// out to be a no-op, so that `u` does not consume a step that changed
    /// nothing.
    pub fn abort_change(&mut self) {
        self.pending = None;
    }

    pub fn undo(&mut self, cursor: (usize, usize)) -> Option<(usize, usize)> {
        let snapshot = self.undo.pop()?;
        self.redo.push(Snapshot {
            lines: std::mem::replace(&mut self.lines, snapshot.lines),
            cursor,
        });
        self.modified = true;
        self.bump_revision();
        Some(snapshot.cursor)
    }

    pub fn redo(&mut self, cursor: (usize, usize)) -> Option<(usize, usize)> {
        let snapshot = self.redo.pop()?;
        self.undo.push(Snapshot {
            lines: std::mem::replace(&mut self.lines, snapshot.lines),
            cursor,
        });
        self.modified = true;
        self.bump_revision();
        Some(snapshot.cursor)
    }

    // --- editing ----------------------------------------------------------

    pub fn insert_char(&mut self, line: usize, col: usize, ch: char) {
        let line = line.min(self.lines.len() - 1);
        let row = &mut self.lines[line];
        let col = col.min(row.len());
        row.insert(col, ch);
    }

    pub fn insert_str(&mut self, line: usize, col: usize, text: &str) {
        let mut col = col;
        for ch in text.chars() {
            self.insert_char(line, col, ch);
            col += 1;
        }
    }

    pub fn delete_range_in_line(&mut self, line: usize, from: usize, to: usize) -> String {
        let Some(row) = self.lines.get_mut(line) else {
            return String::new();
        };
        let from = from.min(row.len());
        let to = to.min(row.len());
        if from >= to {
            return String::new();
        }
        row.drain(from..to).collect()
    }

    pub fn split_line(&mut self, line: usize, col: usize) {
        let row = &mut self.lines[line];
        let col = col.min(row.len());
        let tail: Vec<char> = row.split_off(col);
        self.lines.insert(line + 1, tail);
    }

    /// Append line `line + 1` to `line`, returning the column where the join
    /// happened so the caller can put the cursor there, as vim does.
    pub fn join_lines(&mut self, line: usize, spaced: bool) -> Option<usize> {
        if line + 1 >= self.lines.len() {
            return None;
        }
        let mut next = self.lines.remove(line + 1);
        let row = &mut self.lines[line];
        if spaced {
            // vim drops the indentation of the joined line and inserts a single
            // space, unless one side is already empty.
            while next.first().is_some_and(|c| c.is_whitespace()) {
                next.remove(0);
            }
            while row.last().is_some_and(|c| c.is_whitespace()) {
                row.pop();
            }
            let at = row.len();
            if !row.is_empty() && !next.is_empty() {
                row.push(' ');
            }
            row.extend(next);
            Some(at)
        } else {
            let at = row.len();
            row.extend(next);
            Some(at)
        }
    }

    pub fn insert_line(&mut self, index: usize, text: Vec<char>) {
        let index = index.min(self.lines.len());
        self.lines.insert(index, text);
    }

    pub fn remove_lines(&mut self, from: usize, count: usize) -> Vec<String> {
        if from >= self.lines.len() || count == 0 {
            return Vec::new();
        }
        let to = (from + count).min(self.lines.len());
        let removed: Vec<String> = self
            .lines
            .drain(from..to)
            .map(|l| l.into_iter().collect())
            .collect();
        if self.lines.is_empty() {
            self.lines.push(Vec::new());
        }
        removed
    }

    pub fn replace_line(&mut self, index: usize, text: Vec<char>) {
        if let Some(row) = self.lines.get_mut(index) {
            *row = text;
        }
    }

    pub fn set_lines(&mut self, lines: Vec<String>) {
        self.lines = lines.into_iter().map(|l| l.chars().collect()).collect();
        if self.lines.is_empty() {
            self.lines.push(Vec::new());
        }
        self.modified = true;
        self.bump_revision();
    }

    /// Replace `[start, end)` with `replacement`, the shape `nvim_buf_set_lines`
    /// speaks.
    pub fn splice_lines(&mut self, start: usize, end: usize, replacement: Vec<String>) {
        let start = start.min(self.lines.len());
        let end = end.clamp(start, self.lines.len());
        let replacement: Vec<Vec<char>> = replacement
            .into_iter()
            .map(|l| l.chars().collect())
            .collect();
        self.lines.splice(start..end, replacement);
        if self.lines.is_empty() {
            self.lines.push(Vec::new());
        }
        self.modified = true;
        self.bump_revision();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_round_trips_through_lines() {
        let buffer = Buffer::from_text("one\ntwo\n");
        assert_eq!(buffer.line_count(), 2);
        assert_eq!(buffer.text(), "one\ntwo\n");
    }

    #[test]
    fn a_file_without_a_final_newline_keeps_not_having_one() {
        let buffer = Buffer::from_text("one\ntwo");
        assert_eq!(buffer.text(), "one\ntwo");
    }

    #[test]
    fn columns_count_characters_not_bytes() {
        let mut buffer = Buffer::from_text("あい\n");
        assert_eq!(buffer.line_len(0), 2);
        buffer.insert_char(0, 1, 'x');
        assert_eq!(buffer.line_text(0), "あxい");
    }

    #[test]
    fn undo_restores_the_previous_text_and_cursor() {
        let mut buffer = Buffer::from_text("one\n");
        let revision = buffer.revision();
        buffer.begin_change((0, 0));
        buffer.insert_str(0, 0, "zero ");
        buffer.commit_change();
        assert_eq!(buffer.revision(), revision + 1);
        assert_eq!(buffer.line_text(0), "zero one");
        assert_eq!(buffer.undo((0, 5)), Some((0, 0)));
        assert_eq!(buffer.revision(), revision + 2);
        assert_eq!(buffer.line_text(0), "one");
        assert_eq!(buffer.redo((0, 0)), Some((0, 5)));
        assert_eq!(buffer.revision(), revision + 3);
        assert_eq!(buffer.line_text(0), "zero one");
    }

    #[test]
    fn a_change_that_changed_nothing_is_not_an_undo_step() {
        let mut buffer = Buffer::from_text("one\n");
        let revision = buffer.revision();
        buffer.begin_change((0, 0));
        buffer.commit_change();
        assert_eq!(buffer.revision(), revision);
        assert_eq!(buffer.undo((0, 0)), None);
        assert!(!buffer.modified());
    }

    #[test]
    fn direct_line_replacements_advance_the_revision() {
        let mut buffer = Buffer::from_text("one\n");
        let revision = buffer.revision();
        buffer.set_lines(vec!["two".into()]);
        assert_eq!(buffer.revision(), revision + 1);
        buffer.splice_lines(0, 1, vec!["three".into()]);
        assert_eq!(buffer.revision(), revision + 2);
    }

    #[test]
    fn joining_collapses_indentation_into_one_space() {
        let mut buffer = Buffer::from_text("one\n    two\n");
        assert_eq!(buffer.join_lines(0, true), Some(3));
        assert_eq!(buffer.line_text(0), "one two");
    }

    #[test]
    fn removing_every_line_leaves_one_empty_line() {
        let mut buffer = Buffer::from_text("one\ntwo\n");
        buffer.remove_lines(0, 2);
        assert_eq!(buffer.line_count(), 1);
        assert_eq!(buffer.line_text(0), "");
    }

    /// Before this was fixed, opening a FIFO from the file tree wedged the
    /// editor thread inside `open(2)` with no way back. The test would not
    /// fail — it would never return, which is exactly what the owner saw.
    #[test]
    fn opening_a_fifo_fails_instead_of_blocking() {
        let dir = std::env::temp_dir().join("nvimglsl-fifo-open");
        let _ = std::fs::create_dir_all(&dir);
        let fifo = dir.join("pipe");
        let _ = std::fs::remove_file(&fifo);
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !made {
            eprintln!("skip: mkfifo unavailable");
            return;
        }
        let Err(err) = Buffer::open(&fifo) else {
            panic!("a FIFO is not a file to edit");
        };
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        let _ = std::fs::remove_file(&fifo);
    }

    #[test]
    fn a_directory_is_not_opened_as_text() {
        let Err(err) = Buffer::open(Path::new("/")) else {
            panic!("a directory is not a file to edit");
        };
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }
}
