//! Parsing of Neovim's key notation.
//!
//! `nvim_input` takes a string, not a key event: `"ihello<Esc>"` is five
//! keystrokes and one of them is named. Any host that speaks the protocol has to
//! agree with Neovim about where one key ends and the next begins, including the
//! case where `<` is just a less-than sign.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Named {
    Esc,
    Enter,
    Tab,
    Backspace,
    Delete,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    F(u8),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Code {
    Char(char),
    Named(Named),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Key {
    pub code: Code,
    pub ctrl: bool,
    pub alt: bool,
}

impl Key {
    pub fn char(ch: char) -> Self {
        Self {
            code: Code::Char(ch),
            ctrl: false,
            alt: false,
        }
    }

    pub fn ctrl(ch: char) -> Self {
        Self {
            code: Code::Char(ch),
            ctrl: true,
            alt: false,
        }
    }

    pub fn named(named: Named) -> Self {
        Self {
            code: Code::Named(named),
            ctrl: false,
            alt: false,
        }
    }

    /// The plain character this key stands for, if it stands for one.
    ///
    /// Named keys and anything carrying a modifier do not insert text, and
    /// neither does a raw control character: a literal `\n` arriving inside an
    /// `nvim_input` string is a newline the sender failed to write as `<CR>`,
    /// and inserting it puts a control character inside a line where every
    /// later column count disagrees with what is on screen.
    pub fn as_text(self) -> Option<char> {
        match self.code {
            Code::Char(ch) if !self.ctrl && !self.alt && !ch.is_control() => Some(ch),
            _ => None,
        }
    }
}

fn named_from(name: &str) -> Option<Named> {
    let lower = name.to_ascii_lowercase();
    if let Some(number) = lower.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
        if (1..=12).contains(&number) {
            return Some(Named::F(number));
        }
    }
    Some(match lower.as_str() {
        "esc" => Named::Esc,
        "cr" | "enter" | "return" => Named::Enter,
        "tab" => Named::Tab,
        "bs" => Named::Backspace,
        "del" => Named::Delete,
        "up" => Named::Up,
        "down" => Named::Down,
        "left" => Named::Left,
        "right" => Named::Right,
        "home" => Named::Home,
        "end" => Named::End,
        "pageup" => Named::PageUp,
        "pagedown" => Named::PageDown,
        "insert" => Named::Insert,
        _ => return None,
    })
}

/// Split one `<...>` body into modifiers and a key.
fn parse_bracketed(body: &str) -> Option<Key> {
    match body.to_ascii_lowercase().as_str() {
        "lt" => return Some(Key::char('<')),
        "gt" => return Some(Key::char('>')),
        "bslash" => return Some(Key::char('\\')),
        "bar" => return Some(Key::char('|')),
        "space" => return Some(Key::char(' ')),
        "nul" => return Some(Key::char('\0')),
        _ => {}
    }
    let mut ctrl = false;
    let mut alt = false;
    let mut shift = false;
    let mut rest = body;
    loop {
        let Some((prefix, tail)) = rest.split_once('-') else {
            break;
        };
        match prefix.to_ascii_uppercase().as_str() {
            "C" => ctrl = true,
            "M" | "A" => alt = true,
            "S" => shift = true,
            // Not a modifier — `<C-->` and friends are not worth guessing at.
            _ => break,
        }
        rest = tail;
    }
    let code = if let Some(named) = named_from(rest) {
        Code::Named(named)
    } else if rest.eq_ignore_ascii_case("space") {
        Code::Char(' ')
    } else {
        let mut chars = rest.chars();
        let ch = chars.next()?;
        if chars.next().is_some() {
            return None;
        }
        let ch = if shift {
            ch.to_ascii_uppercase()
        } else if ctrl {
            ch.to_ascii_lowercase()
        } else {
            ch
        };
        Code::Char(ch)
    };
    Some(Key { code, ctrl, alt })
}

/// Parse a `nvim_input` string into keys.
///
/// An unterminated or unrecognised `<` is a literal `<`, which is what Neovim
/// does and what makes typing `a < b` in insert mode work.
pub fn parse(input: &str) -> Vec<Key> {
    let chars: Vec<char> = input.chars().collect();
    let mut keys = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '<' {
            if let Some(close) = (index + 1..chars.len()).find(|&i| chars[i] == '>') {
                let body: String = chars[index + 1..close].iter().collect();
                if let Some(key) = parse_bracketed(&body) {
                    keys.push(key);
                    index = close + 1;
                    continue;
                }
            }
        }
        keys.push(Key::char(chars[index]));
        index += 1;
    }
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_string_is_one_key_per_character() {
        let keys = parse("abc");
        assert_eq!(keys.len(), 3);
        assert_eq!(keys[0], Key::char('a'));
    }

    #[test]
    fn named_keys_are_one_key() {
        let keys = parse("ihello<Esc>");
        assert_eq!(keys.len(), 7);
        assert_eq!(keys[6], Key::named(Named::Esc));
        assert_eq!(parse("<F5>"), vec![Key::named(Named::F(5))]);
        assert_eq!(parse("<F12>"), vec![Key::named(Named::F(12))]);
    }

    #[test]
    fn modifiers_parse() {
        assert_eq!(parse("<C-r>"), vec![Key::ctrl('r')]);
        assert_eq!(
            parse("<C-S-x>"),
            vec![Key {
                code: Code::Char('X'),
                ctrl: true,
                alt: false
            }]
        );
        assert_eq!(parse("<c-S>"), parse("<C-s>"));
        assert_eq!(parse("<C-X>"), parse("<C-x>"));
        assert_eq!(
            parse("<M-x>"),
            vec![Key {
                code: Code::Char('x'),
                ctrl: false,
                alt: true
            }]
        );
    }

    #[test]
    fn an_unrecognised_bracket_is_a_literal_less_than() {
        assert_eq!(
            parse("a<b"),
            vec![Key::char('a'), Key::char('<'), Key::char('b')]
        );
        assert_eq!(parse("<lt>"), vec![Key::char('<')]);
        assert_eq!(parse("<Space>"), vec![Key::char(' ')]);
    }

    #[test]
    fn a_raw_control_character_is_not_insertable_text() {
        assert_eq!(Key::char('\n').as_text(), None);
        assert_eq!(Key::char('\r').as_text(), None);
        assert_eq!(Key::char('\t').as_text(), None);
        assert_eq!(Key::char('a').as_text(), Some('a'));
    }

    #[test]
    fn multibyte_text_survives_parsing() {
        assert_eq!(parse("あ"), vec![Key::char('あ')]);
    }
}
