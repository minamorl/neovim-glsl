use std::io::{BufRead, Write};

use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Id {
    Number(u64),
    String(String),
}

impl Id {
    pub fn to_value(&self) -> Value {
        match self {
            Id::Number(id) => Value::from(*id),
            Id::String(id) => Value::from(id.clone()),
        }
    }

    pub fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::Number(number) => number.as_u64().map(Id::Number),
            Value::String(text) => Some(Id::String(text.clone())),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    Request {
        id: Id,
        method: String,
        params: Value,
    },
    Response {
        id: Id,
        result: Option<Value>,
        error: Option<Value>,
    },
    Notification {
        method: String,
        params: Value,
    },
}

impl Message {
    pub fn to_value(&self) -> Value {
        let mut map = serde_json::Map::new();
        map.insert("jsonrpc".into(), Value::from("2.0"));
        match self {
            Message::Request { id, method, params } => {
                map.insert("id".into(), id.to_value());
                map.insert("method".into(), Value::from(method.clone()));
                if !params.is_null() {
                    map.insert("params".into(), params.clone());
                }
            }
            Message::Response { id, result, error } => {
                map.insert("id".into(), id.to_value());
                if let Some(error) = error {
                    map.insert("error".into(), error.clone());
                } else {
                    map.insert("result".into(), result.clone().unwrap_or(Value::Null));
                }
            }
            Message::Notification { method, params } => {
                map.insert("method".into(), Value::from(method.clone()));
                if !params.is_null() {
                    map.insert("params".into(), params.clone());
                }
            }
        }
        Value::Object(map)
    }

    pub fn from_value(value: Value) -> Option<Self> {
        let object = value.as_object()?;
        let method = object.get("method").and_then(Value::as_str);
        let id = object.get("id").and_then(Id::from_value);
        match (id, method) {
            (Some(id), Some(method)) => Some(Message::Request {
                id,
                method: method.to_string(),
                params: object.get("params").cloned().unwrap_or(Value::Null),
            }),
            (Some(id), None) => Some(Message::Response {
                id,
                result: object.get("result").cloned(),
                error: object.get("error").cloned(),
            }),
            (None, Some(method)) => Some(Message::Notification {
                method: method.to_string(),
                params: object.get("params").cloned().unwrap_or(Value::Null),
            }),
            _ => None,
        }
    }
}

pub fn read_value(reader: &mut impl BufRead) -> std::io::Result<Value> {
    let mut content_length = None;
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line)?;
        if read == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "closed before LSP headers",
            ));
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse::<usize>().ok();
            }
        }
    }
    let len = content_length.ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "missing Content-Length")
    })?;
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body)?;
    serde_json::from_slice(&body)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}

pub fn read_message(reader: &mut impl BufRead) -> std::io::Result<Message> {
    read_value(reader).and_then(|value| {
        Message::from_value(value)
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "bad LSP message"))
    })
}

pub fn write_value(writer: &mut impl Write, value: &Value) -> std::io::Result<()> {
    let body = serde_json::to_vec(value)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(&body)?;
    writer.flush()
}

pub fn write_message(writer: &mut impl Write, message: &Message) -> std::io::Result<()> {
    write_value(writer, &message.to_value())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_length_framing_round_trips_a_request() {
        let message = Message::Request {
            id: Id::Number(7),
            method: "textDocument/hover".into(),
            params: serde_json::json!({"textDocument":{"uri":"file:///x"}}),
        };
        let mut bytes = Vec::new();
        write_message(&mut bytes, &message).unwrap();
        assert!(std::str::from_utf8(&bytes)
            .unwrap()
            .starts_with("Content-Length: "));
        let mut reader = std::io::BufReader::new(bytes.as_slice());
        assert_eq!(read_message(&mut reader).unwrap(), message);
    }

    #[test]
    fn notification_has_no_id_and_response_keeps_the_id() {
        let note = Message::from_value(serde_json::json!({
            "jsonrpc": "2.0",
            "method": "initialized",
            "params": {}
        }))
        .unwrap();
        assert!(matches!(note, Message::Notification { .. }));

        let response = Message::from_value(serde_json::json!({
            "jsonrpc": "2.0",
            "id": "abc",
            "result": null
        }))
        .unwrap();
        assert!(matches!(
            response,
            Message::Response {
                id: Id::String(_),
                ..
            }
        ));
    }
}
