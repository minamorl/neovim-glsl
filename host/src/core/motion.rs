//! Cursor motions.
//!
//! A motion answers two separate questions, and an operator needs both: *where
//! does the cursor land*, and *does the span it just described include that
//! landing cell*. `dw` and `de` move to different places but both delete
//! forwards; the difference is exclusivity, not distance. Keeping `kind` beside
//! `apply` is what stops `de` from leaving the last character behind.

use super::buffer::Buffer;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Motion {
    Left,
    Right,
    Up,
    Down,
    LineStart,
    FirstNonBlank,
    LineEnd,
    WordForward { big: bool },
    WordBack { big: bool },
    WordEnd { big: bool },
    FileStart,
    FileEnd,
    GotoLine(usize),
    ParagraphForward,
    ParagraphBack,
    /// `f` / `t` / `F` / `T`.
    FindChar { ch: char, forward: bool, till: bool },
    /// `%`
    MatchPair,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// Spans up to but not including the destination.
    Exclusive,
    /// Spans up to and including the destination.
    Inclusive,
    /// Spans whole lines.
    Linewise,
}

impl Motion {
    pub fn kind(self) -> Kind {
        match self {
            Motion::Up
            | Motion::Down
            | Motion::FileStart
            | Motion::FileEnd
            | Motion::GotoLine(_) => Kind::Linewise,
            Motion::LineEnd | Motion::WordEnd { .. } => Kind::Inclusive,
            Motion::FindChar { forward: true, .. } => Kind::Inclusive,
            _ => Kind::Exclusive,
        }
    }

    /// Whether the vertical position should keep the column the user last chose
    /// explicitly. `j` after `$` returns to the end of each line; `l` does not.
    pub fn keeps_desired_column(self) -> bool {
        matches!(self, Motion::Up | Motion::Down)
    }
}

fn char_class(ch: char, big: bool) -> u8 {
    if ch.is_whitespace() {
        0
    } else if big {
        1
    } else if ch.is_alphanumeric() || ch == '_' {
        1
    } else {
        2
    }
}

/// Flatten the buffer position into a single stream index so word motions can
/// cross line boundaries without a special case at every step.
fn forward(buffer: &Buffer, mut line: usize, mut col: usize) -> Option<(usize, usize)> {
    if col < buffer.line_len(line) {
        col += 1;
        if col <= buffer.line_len(line) {
            return Some((line, col));
        }
    }
    if line + 1 < buffer.line_count() {
        line += 1;
        return Some((line, 0));
    }
    None
}

fn backward(buffer: &Buffer, line: usize, col: usize) -> Option<(usize, usize)> {
    if col > 0 {
        return Some((line, col - 1));
    }
    if line > 0 {
        return Some((line - 1, buffer.line_len(line - 1)));
    }
    None
}

fn char_at(buffer: &Buffer, line: usize, col: usize) -> Option<char> {
    buffer.line(line).get(col).copied()
}

fn first_non_blank(buffer: &Buffer, line: usize) -> usize {
    buffer
        .line(line)
        .iter()
        .position(|c| !c.is_whitespace())
        .unwrap_or_else(|| buffer.line_len(line).saturating_sub(1))
}

fn word_forward(buffer: &Buffer, from: (usize, usize), big: bool) -> (usize, usize) {
    let (mut line, mut col) = from;
    let start_class = char_at(buffer, line, col).map(|c| char_class(c, big)).unwrap_or(0);
    // Leave the current run…
    if start_class != 0 {
        while let Some(next) = forward(buffer, line, col) {
            let (l, c) = next;
            // A line break ends a word even mid-run.
            if l != line {
                line = l;
                col = c;
                break;
            }
            line = l;
            col = c;
            match char_at(buffer, line, col) {
                Some(ch) if char_class(ch, big) == start_class => continue,
                _ => break,
            }
        }
    }
    // …then skip the blanks in front of the next one. An empty line is a word
    // in vim, so stop on one rather than skipping through it.
    loop {
        match char_at(buffer, line, col) {
            Some(ch) if ch.is_whitespace() => {}
            Some(_) => break,
            None => {
                if buffer.line_len(line) == 0 {
                    break;
                }
            }
        }
        match forward(buffer, line, col) {
            Some((l, c)) => {
                line = l;
                col = c;
            }
            None => break,
        }
    }
    (line, col.min(buffer.line_len(line)))
}

fn word_back(buffer: &Buffer, from: (usize, usize), big: bool) -> (usize, usize) {
    let (mut line, mut col) = from;
    let Some(prev) = backward(buffer, line, col) else { return (line, col) };
    line = prev.0;
    col = prev.1;
    loop {
        match char_at(buffer, line, col) {
            Some(ch) if ch.is_whitespace() => {}
            Some(_) => break,
            None => {
                if buffer.line_len(line) == 0 {
                    return (line, 0);
                }
            }
        }
        match backward(buffer, line, col) {
            Some((l, c)) => {
                line = l;
                col = c;
            }
            None => return (line, col),
        }
    }
    let class = char_at(buffer, line, col).map(|c| char_class(c, big)).unwrap_or(0);
    while let Some((l, c)) = backward(buffer, line, col) {
        if l != line {
            break;
        }
        match char_at(buffer, l, c) {
            Some(ch) if char_class(ch, big) == class => {
                line = l;
                col = c;
            }
            _ => break,
        }
    }
    (line, col)
}

fn word_end(buffer: &Buffer, from: (usize, usize), big: bool) -> (usize, usize) {
    let (mut line, mut col) = from;
    let Some(next) = forward(buffer, line, col) else { return (line, col) };
    line = next.0;
    col = next.1;
    while char_at(buffer, line, col).map(|c| c.is_whitespace()).unwrap_or(true) {
        match forward(buffer, line, col) {
            Some((l, c)) => {
                line = l;
                col = c;
            }
            None => return (line, col.min(buffer.line_len(line).saturating_sub(1))),
        }
    }
    let class = char_at(buffer, line, col).map(|c| char_class(c, big)).unwrap_or(0);
    while let Some((l, c)) = forward(buffer, line, col) {
        if l != line {
            break;
        }
        match char_at(buffer, l, c) {
            Some(ch) if char_class(ch, big) == class => {
                line = l;
                col = c;
            }
            _ => break,
        }
    }
    (line, col)
}

fn paragraph(buffer: &Buffer, from: usize, forward: bool) -> usize {
    let mut line = from;
    let step = |l: usize| if forward { l + 1 } else { l.wrapping_sub(1) };
    let last = buffer.line_count().saturating_sub(1);
    loop {
        let next = step(line);
        if forward && next > last {
            return last;
        }
        if !forward && (line == 0 || next > last) {
            return 0;
        }
        line = next;
        if buffer.line_len(line) == 0 && buffer.line_len(from) != 0 {
            return line;
        }
        if buffer.line_len(line) == 0 && line != from {
            return line;
        }
    }
}

fn match_pair(buffer: &Buffer, from: (usize, usize)) -> (usize, usize) {
    const PAIRS: [(char, char); 3] = [('(', ')'), ('[', ']'), ('{', '}')];
    let (line, col) = from;
    // vim scans forward on the line for the first pair character, then matches.
    let row = buffer.line(line);
    let start = (col..row.len()).find(|&i| PAIRS.iter().any(|&(o, c)| row[i] == o || row[i] == c));
    let Some(start) = start else { return from };
    let ch = row[start];
    let (open, close, forwards) = match PAIRS.iter().find(|&&(o, c)| ch == o || ch == c) {
        Some(&(o, c)) => (o, c, ch == o),
        None => return from,
    };
    let mut depth = 0i32;
    let mut position = (line, start);
    loop {
        let current = char_at(buffer, position.0, position.1);
        if let Some(current) = current {
            if current == open {
                depth += if forwards { 1 } else { -1 };
            } else if current == close {
                depth += if forwards { -1 } else { 1 };
            }
            if depth == 0 {
                return position;
            }
        }
        let next = if forwards {
            forward(buffer, position.0, position.1)
        } else {
            backward(buffer, position.0, position.1)
        };
        match next {
            Some(next) => position = next,
            None => return from,
        }
    }
}

fn find_char(
    buffer: &Buffer,
    from: (usize, usize),
    ch: char,
    forward_dir: bool,
    till: bool,
    count: usize,
) -> Option<(usize, usize)> {
    let (line, col) = from;
    let row = buffer.line(line);
    // The occurrences are counted from strictly beside the cursor, and only then
    // is `t` backed off by one. Folding the back-off into the search instead
    // makes `2t,` skip an occurrence.
    let mut at = col;
    for _ in 0..count.max(1) {
        at = if forward_dir {
            (at + 1..row.len()).find(|&i| row[i] == ch)?
        } else {
            (0..at).rev().find(|&i| row[i] == ch)?
        };
    }
    Some((line, if till { if forward_dir { at.saturating_sub(1) } else { at + 1 } } else { at }))
}

/// Where a motion lands, given a count.
///
/// The returned column is not clamped to the last character: `$` in insert mode
/// legitimately sits one past the end. Clamping belongs to the mode, which knows
/// whether one-past-the-end is a valid place to be.
pub fn apply(
    buffer: &Buffer,
    cursor: (usize, usize),
    desired_col: usize,
    motion: Motion,
    count: usize,
) -> (usize, usize) {
    let count = count.max(1);
    let (line, col) = cursor;
    let last_line = buffer.line_count().saturating_sub(1);
    match motion {
        Motion::Left => (line, col.saturating_sub(count)),
        Motion::Right => (line, (col + count).min(buffer.line_len(line))),
        Motion::Up => {
            let target = line.saturating_sub(count);
            (target, desired_col.min(buffer.line_len(target)))
        }
        Motion::Down => {
            let target = (line + count).min(last_line);
            (target, desired_col.min(buffer.line_len(target)))
        }
        Motion::LineStart => (line, 0),
        Motion::FirstNonBlank => (line, first_non_blank(buffer, line)),
        Motion::LineEnd => {
            let target = (line + count - 1).min(last_line);
            (target, buffer.line_len(target))
        }
        Motion::WordForward { big } => {
            let mut position = cursor;
            for _ in 0..count {
                position = word_forward(buffer, position, big);
            }
            position
        }
        Motion::WordBack { big } => {
            let mut position = cursor;
            for _ in 0..count {
                position = word_back(buffer, position, big);
            }
            position
        }
        Motion::WordEnd { big } => {
            let mut position = cursor;
            for _ in 0..count {
                position = word_end(buffer, position, big);
            }
            position
        }
        Motion::FileStart => (0, first_non_blank(buffer, 0)),
        Motion::FileEnd => (last_line, first_non_blank(buffer, last_line)),
        Motion::GotoLine(target) => {
            let target = target.min(last_line);
            (target, first_non_blank(buffer, target))
        }
        Motion::ParagraphForward => {
            let mut at = line;
            for _ in 0..count {
                at = paragraph(buffer, at, true);
            }
            (at, 0)
        }
        Motion::ParagraphBack => {
            let mut at = line;
            for _ in 0..count {
                at = paragraph(buffer, at, false);
            }
            (at, 0)
        }
        Motion::FindChar { ch, forward, till } => {
            find_char(buffer, cursor, ch, forward, till, count).unwrap_or(cursor)
        }
        Motion::MatchPair => match_pair(buffer, cursor),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str, cursor: (usize, usize), motion: Motion, count: usize) -> (usize, usize) {
        let buffer = Buffer::from_text(text);
        apply(&buffer, cursor, cursor.1, motion, count)
    }

    #[test]
    fn w_stops_at_the_next_word() {
        assert_eq!(at("one two three\n", (0, 0), Motion::WordForward { big: false }, 1), (0, 4));
        assert_eq!(at("one two three\n", (0, 0), Motion::WordForward { big: false }, 2), (0, 8));
    }

    #[test]
    fn w_treats_punctuation_as_its_own_word_and_W_does_not() {
        assert_eq!(at("a.b c\n", (0, 0), Motion::WordForward { big: false }, 1), (0, 1));
        assert_eq!(at("a.b c\n", (0, 0), Motion::WordForward { big: true }, 1), (0, 4));
    }

    #[test]
    fn w_crosses_the_line_break() {
        assert_eq!(at("one\ntwo\n", (0, 1), Motion::WordForward { big: false }, 1), (1, 0));
    }

    #[test]
    fn e_lands_on_the_last_character_of_the_word() {
        assert_eq!(at("one two\n", (0, 0), Motion::WordEnd { big: false }, 1), (0, 2));
        assert_eq!(at("one two\n", (0, 2), Motion::WordEnd { big: false }, 1), (0, 6));
    }

    #[test]
    fn b_lands_on_the_first_character_of_the_previous_word() {
        assert_eq!(at("one two\n", (0, 5), Motion::WordBack { big: false }, 1), (0, 4));
        assert_eq!(at("one two\n", (0, 4), Motion::WordBack { big: false }, 1), (0, 0));
    }

    #[test]
    fn e_is_inclusive_and_w_is_not() {
        assert_eq!(Motion::WordEnd { big: false }.kind(), Kind::Inclusive);
        assert_eq!(Motion::WordForward { big: false }.kind(), Kind::Exclusive);
    }

    #[test]
    fn j_keeps_the_column_the_user_chose() {
        let buffer = Buffer::from_text("longer line\nab\nlonger line\n");
        let after_short = apply(&buffer, (0, 9), 9, Motion::Down, 1);
        assert_eq!(after_short, (1, 2));
        let back = apply(&buffer, after_short, 9, Motion::Down, 1);
        assert_eq!(back, (2, 9));
    }

    #[test]
    fn f_and_t_differ_by_one_cell() {
        assert_eq!(at("a,b,c\n", (0, 0), Motion::FindChar { ch: ',', forward: true, till: false }, 1), (0, 1));
        assert_eq!(at("a,b,c\n", (0, 0), Motion::FindChar { ch: ',', forward: true, till: true }, 1), (0, 0));
        assert_eq!(at("a,b,c\n", (0, 0), Motion::FindChar { ch: ',', forward: true, till: false }, 2), (0, 3));
    }

    #[test]
    fn percent_matches_the_bracket_ahead_of_the_cursor() {
        assert_eq!(at("if (a) {\n", (0, 0), Motion::MatchPair, 1), (0, 5));
        assert_eq!(at("if (a) {\n", (0, 5), Motion::MatchPair, 1), (0, 3));
    }

    #[test]
    fn paragraph_motion_stops_on_the_blank_line() {
        assert_eq!(at("a\nb\n\nc\n", (0, 0), Motion::ParagraphForward, 1), (2, 0));
    }
}
