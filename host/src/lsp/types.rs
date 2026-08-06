use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::core::Buffer;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub line: usize,
    pub character: usize,
}

impl Position {
    pub fn from_buffer_chars(buffer: &Buffer, line: usize, char_idx: usize) -> Self {
        Self {
            line,
            character: crate::textpos::char_to_utf16(buffer.line(line), char_idx),
        }
    }

    pub fn to_buffer_chars(self, buffer: &Buffer) -> (usize, usize) {
        let line = self.line.min(buffer.line_count().saturating_sub(1));
        let character = crate::textpos::utf16_to_char(buffer.line(line), self.character);
        (line, character)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

impl Range {
    pub fn from_buffer_chars(buffer: &Buffer, start: (usize, usize), end: (usize, usize)) -> Self {
        Self {
            start: Position::from_buffer_chars(buffer, start.0, start.1),
            end: Position::from_buffer_chars(buffer, end.0, end.1),
        }
    }

    pub fn contains_buffer_position(&self, buffer: &Buffer, line: usize, col: usize) -> bool {
        let start = self.start.to_buffer_chars(buffer);
        let end = self.end.to_buffer_chars(buffer);
        (line, col) >= start && (line, col) < end
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub range: Range,
    #[serde(default)]
    pub severity: Option<u8>,
    #[serde(default)]
    pub code: Option<serde_json::Value>,
    #[serde(default)]
    pub source: Option<String>,
    pub message: String,
}

impl Diagnostic {
    pub fn severity_rank(&self) -> u8 {
        self.severity.unwrap_or(4)
    }

    pub fn line(&self) -> usize {
        self.range.start.line
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextEdit {
    pub range: Range,
    pub new_text: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionItem {
    pub label: String,
    #[serde(default)]
    pub kind: Option<u8>,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub documentation: Option<serde_json::Value>,
    #[serde(default)]
    pub insert_text: Option<String>,
    #[serde(default)]
    pub text_edit: Option<TextEdit>,
}

impl CompletionItem {
    pub fn word(&self) -> &str {
        self.insert_text.as_deref().unwrap_or(&self.label)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hover {
    pub contents: serde_json::Value,
    #[serde(default)]
    pub range: Option<Range>,
}

impl Hover {
    pub fn plain_text(&self) -> String {
        markdown_text(&self.contents)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    pub uri: String,
    pub range: Range,
}

pub fn path_to_uri(path: &Path) -> String {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let mut out = String::from("file://");
    out.push_str(&percent_encode_path(&path));
    out
}

pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    Some(PathBuf::from(percent_decode(rest)))
}

fn percent_encode_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    let mut out = String::new();
    for byte in text.as_bytes() {
        match *byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'.' | b'-' | b'_' | b'~' => {
                out.push(*byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(hex) = u8::from_str_radix(&text[index + 1..index + 3], 16) {
                out.push(hex);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn markdown_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Array(items) => items
            .iter()
            .map(markdown_text)
            .collect::<Vec<_>>()
            .join("\n"),
        serde_json::Value::Object(map) => {
            if let Some(value) = map.get("value").and_then(serde_json::Value::as_str) {
                value.to_string()
            } else if let Some(value) = map.get("contents") {
                markdown_text(value)
            } else {
                String::new()
            }
        }
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lsp_positions_use_utf16_not_bytes_or_cells() {
        let buffer = Buffer::from_text("a🙂b\nあb\n");
        assert_eq!(Position::from_buffer_chars(&buffer, 0, 2).character, 3);
        assert_eq!(
            Position {
                line: 0,
                character: 3
            }
            .to_buffer_chars(&buffer),
            (0, 2)
        );
        assert_eq!(Position::from_buffer_chars(&buffer, 1, 1).character, 1);
    }

    #[test]
    fn file_uris_round_trip_spaces_and_japanese() {
        let path = PathBuf::from("/tmp/a b/日本語.rs");
        let uri = path_to_uri(&path);
        assert!(uri.contains("%20"));
        assert_eq!(uri_to_path(&uri).unwrap(), path);
    }
}
