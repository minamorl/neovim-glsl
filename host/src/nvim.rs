//! The Neovim protocol as this host speaks it.
//!
//! `pin host_protocol_dialect` says the dialect is Neovim's; `pin
//! asset_reuse_includes_protocol` says the protocol is the piece of Neovim that
//! is inherited rather than rebuilt. So the message shapes here are not ours to
//! design — they are transcribed.
//!
//! Which *face* of the protocol the host serves is
//! `open_question protocol_surface_scope`, still open. What is implemented is
//! the UI face (`nvim_ui_attach` and `redraw`) plus the few `nvim_buf_*` calls
//! the host itself needs; that is an implementation choosing inside an open
//! axis, not the axis being closed. `nvim_exec_lua` answers with an error rather
//! than a silence, so a client cannot mistake "not implemented" for "did
//! nothing".

use std::io::{BufRead, Write};
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError};
use std::time::Duration;

use rmpv::Value;

/// One UI event out of a `redraw` batch: its name, and one invocation's
/// arguments.
pub type RedrawEvent = (String, Vec<Value>);

/// Any notification that is not `redraw`.
pub type Notification = (String, Vec<Value>);

pub const REQUEST: u64 = 0;
pub const RESPONSE: u64 = 1;
pub const NOTIFY: u64 = 2;

/// Which UI surfaces the client draws itself instead of letting the host paint
/// them into the grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UiOptions {
    pub ext_multigrid: bool,
    pub ext_popupmenu: bool,
    pub ext_cmdline: bool,
    pub ext_messages: bool,
}

impl Default for UiOptions {
    fn default() -> Self {
        Self {
            ext_multigrid: false,
            ext_popupmenu: false,
            ext_cmdline: true,
            ext_messages: true,
        }
    }
}

impl UiOptions {
    pub fn none() -> Self {
        Self {
            ext_multigrid: false,
            ext_popupmenu: false,
            ext_cmdline: false,
            ext_messages: false,
        }
    }

    pub fn from_map(map: &[(Value, Value)]) -> Self {
        let get = |key: &str| {
            map.iter()
                .find(|(k, _)| k.as_str() == Some(key))
                .and_then(|(_, v)| v.as_bool())
                .unwrap_or(false)
        };
        Self {
            ext_multigrid: get("ext_multigrid"),
            ext_popupmenu: get("ext_popupmenu"),
            // ext_messages implies ext_cmdline, the same way it does upstream.
            ext_cmdline: get("ext_cmdline") || get("ext_messages"),
            ext_messages: get("ext_messages"),
        }
    }

    pub fn to_map(self) -> Value {
        let mut map = vec![
            (Value::from("ext_linegrid"), Value::from(true)),
            (Value::from("rgb"), Value::from(true)),
        ];
        for (name, on) in [
            ("ext_multigrid", self.ext_multigrid),
            ("ext_popupmenu", self.ext_popupmenu),
            ("ext_cmdline", self.ext_cmdline || self.ext_messages),
            ("ext_messages", self.ext_messages),
        ] {
            if on {
                map.push((Value::from(name), Value::from(true)));
            }
        }
        Value::Map(map)
    }
}

pub fn read_message(reader: &mut impl BufRead) -> std::io::Result<Value> {
    rmpv::decode::read_value(reader)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

pub fn write_message(writer: &mut impl Write, message: &Value) -> std::io::Result<()> {
    let mut buffer = Vec::new();
    rmpv::encode::write_value(&mut buffer, message)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    writer.write_all(&buffer)?;
    writer.flush()
}

pub fn notification(method: &str, params: Vec<Value>) -> Value {
    Value::Array(vec![
        Value::from(NOTIFY),
        Value::from(method),
        Value::Array(params),
    ])
}

pub fn response(msgid: u64, error: Option<Value>, result: Value) -> Value {
    Value::Array(vec![
        Value::from(RESPONSE),
        Value::from(msgid),
        error.unwrap_or(Value::Nil),
        result,
    ])
}

/// A notification is `[2, method, params]`; for `redraw`, params is a list of
/// `[event_name, args…]` where each `args` is itself one invocation.
pub fn split_notification(v: &Value) -> (Vec<RedrawEvent>, Vec<Notification>) {
    let mut out = Vec::new();
    let mut custom = Vec::new();
    let Some(arr) = v.as_array() else {
        return (out, custom);
    };
    if arr.len() != 3 || arr[0].as_u64() != Some(NOTIFY) {
        return (out, custom);
    }
    if arr[1].as_str() != Some("redraw") {
        if let (Some(name), Some(params)) = (arr[1].as_str(), arr[2].as_array()) {
            custom.push((name.to_string(), params.clone()));
        }
        return (out, custom);
    }
    let Some(events) = arr[2].as_array() else {
        return (out, custom);
    };
    for event in events {
        let Some(parts) = event.as_array() else {
            continue;
        };
        let Some(name) = parts.first().and_then(|n| n.as_str()) else {
            continue;
        };
        for call in &parts[1..] {
            if let Some(args) = call.as_array() {
                out.push((name.to_string(), args.clone()));
            }
        }
    }
    (out, custom)
}

/// Pack flattened events back into one `redraw` notification.
///
/// Consecutive events with the same name share one entry, which is what Neovim
/// does and what keeps a full-screen repaint from becoming one array per line.
pub fn pack_redraw(events: &[RedrawEvent]) -> Value {
    let mut batches: Vec<(String, Vec<Value>)> = Vec::new();
    for (name, args) in events {
        match batches.last_mut() {
            Some((last, calls)) if last == name => calls.push(Value::Array(args.clone())),
            _ => batches.push((name.clone(), vec![Value::Array(args.clone())])),
        }
    }
    let packed: Vec<Value> = batches
        .into_iter()
        .map(|(name, mut calls)| {
            let mut parts = vec![Value::from(name)];
            parts.append(&mut calls);
            Value::Array(parts)
        })
        .collect();
    notification("redraw", packed)
}

/// What ended a blocking wait for traffic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ready {
    Message,
    TimedOut,
    Closed,
}

/// The receive side of the transport.
///
/// Waiting for traffic and decoding it are separate calls so that a caller
/// timing the work can open its span between them and never fold idle wait into
/// the measurement.
pub struct RedrawQueue {
    rx: Receiver<Value>,
    pending: Option<Value>,
    custom: Vec<Notification>,
}

impl RedrawQueue {
    pub fn new(rx: Receiver<Value>) -> Self {
        Self {
            rx,
            pending: None,
            custom: Vec::new(),
        }
    }

    pub fn wait_ready(&mut self, timeout: Duration) -> Ready {
        if self.pending.is_some() {
            return Ready::Message;
        }
        match self.rx.recv_timeout(timeout) {
            Ok(v) => {
                self.pending = Some(v);
                Ready::Message
            }
            Err(RecvTimeoutError::Timeout) => Ready::TimedOut,
            Err(RecvTimeoutError::Disconnected) => Ready::Closed,
        }
    }

    pub fn drain_redraw(&mut self) -> (Vec<RedrawEvent>, bool) {
        let mut out = Vec::new();
        let mut closed = false;
        if let Some(v) = self.pending.take() {
            self.collect(&v, &mut out);
        }
        loop {
            match self.rx.try_recv() {
                Ok(v) => self.collect(&v, &mut out),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    closed = true;
                    break;
                }
            }
        }
        (out, closed)
    }

    fn collect(&mut self, v: &Value, out: &mut Vec<RedrawEvent>) {
        let (mut events, mut notes) = split_notification(v);
        out.append(&mut events);
        self.custom.append(&mut notes);
    }

    pub fn take_notifications(&mut self) -> Vec<Notification> {
        std::mem::take(&mut self.custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packing_and_splitting_are_inverse() {
        let events: Vec<RedrawEvent> = vec![
            (
                "grid_resize".into(),
                vec![Value::from(1), Value::from(80), Value::from(24)],
            ),
            ("grid_line".into(), vec![Value::from(1), Value::from(0)]),
            ("grid_line".into(), vec![Value::from(1), Value::from(1)]),
            ("flush".into(), vec![]),
        ];
        let (round_trip, custom) = split_notification(&pack_redraw(&events));
        assert_eq!(round_trip, events);
        assert!(custom.is_empty());
    }

    #[test]
    fn consecutive_events_of_one_kind_share_a_batch() {
        let events: Vec<RedrawEvent> = vec![
            ("grid_line".into(), vec![Value::from(1)]),
            ("grid_line".into(), vec![Value::from(2)]),
        ];
        let packed = pack_redraw(&events);
        let batches = packed.as_array().unwrap()[2].as_array().unwrap();
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].as_array().unwrap().len(), 3);
    }

    #[test]
    fn a_non_redraw_notification_passes_through_untouched() {
        let message = notification("nvimgl_image", vec![Value::from("/tmp/a.png")]);
        let (events, custom) = split_notification(&message);
        assert!(events.is_empty());
        assert_eq!(custom[0].0, "nvimgl_image");
    }

    #[test]
    fn ui_options_round_trip_and_messages_imply_cmdline() {
        let options = UiOptions {
            ext_messages: true,
            ..UiOptions::none()
        };
        let map = options.to_map();
        let parsed = UiOptions::from_map(map.as_map().unwrap());
        assert!(parsed.ext_cmdline && parsed.ext_messages);
    }
}
