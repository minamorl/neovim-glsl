//! Conversions between the position units this host has to meet.
//!
//! The editing core indexes a line as `&[char]`. Protocols around it do not:
//! LSP speaks UTF-16 code units, ripgrep reports byte columns, and the renderer
//! advances in grid cells. Keeping those conversions here prevents each caller
//! from quietly choosing its own idea of a column.

pub fn char_to_utf16(line: &[char], char_idx: usize) -> usize {
    line.iter()
        .take(char_idx.min(line.len()))
        .map(|ch| ch.len_utf16())
        .sum()
}

pub fn utf16_to_char(line: &[char], utf16_idx: usize) -> usize {
    let mut units = 0;
    for (index, ch) in line.iter().enumerate() {
        let next = units + ch.len_utf16();
        if utf16_idx < next {
            return index;
        }
        units = next;
    }
    line.len()
}

pub fn char_to_byte(line: &[char], char_idx: usize) -> usize {
    line.iter()
        .take(char_idx.min(line.len()))
        .map(|ch| ch.len_utf8())
        .sum()
}

pub fn byte_to_char(line: &[char], byte_idx: usize) -> usize {
    let mut bytes = 0;
    for (index, ch) in line.iter().enumerate() {
        let next = bytes + ch.len_utf8();
        if byte_idx < next {
            return index;
        }
        bytes = next;
    }
    line.len()
}

pub fn char_to_cell(line: &[char], char_idx: usize) -> usize {
    line.iter()
        .take(char_idx.min(line.len()))
        .map(|&ch| crate::proto::paint::char_width(ch))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(text: &str) -> Vec<char> {
        text.chars().collect()
    }

    #[test]
    fn japanese_round_trips_between_units() {
        let line = chars("aあb");
        assert_eq!(char_to_utf16(&line, 1), 1);
        assert_eq!(char_to_utf16(&line, 2), 2);
        assert_eq!(utf16_to_char(&line, 2), 2);
        assert_eq!(char_to_byte(&line, 1), 1);
        assert_eq!(char_to_byte(&line, 2), 4);
        assert_eq!(byte_to_char(&line, 4), 2);
        assert_eq!(char_to_cell(&line, 1), 1);
        assert_eq!(char_to_cell(&line, 2), 3);
    }

    #[test]
    fn emoji_round_trips_between_units() {
        let line = chars("a🙂b");
        assert_eq!(char_to_utf16(&line, 1), 1);
        assert_eq!(char_to_utf16(&line, 2), 3);
        assert_eq!(utf16_to_char(&line, 3), 2);
        assert_eq!(utf16_to_char(&line, 2), 1);
        assert_eq!(char_to_byte(&line, 1), 1);
        assert_eq!(char_to_byte(&line, 2), 5);
        assert_eq!(byte_to_char(&line, 5), 2);
        assert_eq!(byte_to_char(&line, 3), 1);
        assert_eq!(char_to_cell(&line, 1), 1);
        assert_eq!(char_to_cell(&line, 2), 3);
    }
}
