use std::collections::HashMap;
use std::io::{BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};

use super::jsonrpc::{self, Id, Message};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestKind {
    Initialize,
    Shutdown,
    Completion,
    Hover,
    Definition,
}

#[derive(Clone, Debug)]
pub enum LspEvent {
    Request {
        server: String,
        id: Id,
        method: String,
        params: Value,
    },
    Response {
        server: String,
        id: Id,
        kind: Option<RequestKind>,
        result: Option<Value>,
        error: Option<Value>,
    },
    Notification {
        server: String,
        method: String,
        params: Value,
    },
    Closed {
        server: String,
    },
}

pub trait Transport: Send {
    fn split(
        self: Box<Self>,
    ) -> std::io::Result<(Box<dyn TransportReader>, Box<dyn TransportWriter>)>;
}

pub trait TransportReader: Send {
    fn read(&mut self) -> std::io::Result<Message>;
}

pub trait TransportWriter: Send {
    fn write(&mut self, message: &Message) -> std::io::Result<()>;
}

pub struct ChildProcessTransport {
    child: Child,
}

impl ChildProcessTransport {
    pub fn spawn(
        program: &std::path::Path,
        args: &[String],
        root: &std::path::Path,
    ) -> std::io::Result<Self> {
        let child = Command::new(program)
            .args(args)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        Ok(Self { child })
    }
}

impl Transport for ChildProcessTransport {
    fn split(
        mut self: Box<Self>,
    ) -> std::io::Result<(Box<dyn TransportReader>, Box<dyn TransportWriter>)> {
        let stdout = self.child.stdout.take().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::BrokenPipe, "LSP stdout missing")
        })?;
        let stdin = self.child.stdin.take().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::BrokenPipe, "LSP stdin missing")
        })?;
        Ok((
            Box::new(JsonRpcReader {
                reader: BufReader::new(stdout),
            }),
            Box::new(JsonRpcWriter { writer: stdin }),
        ))
    }
}

struct JsonRpcReader<R> {
    reader: BufReader<R>,
}

impl<R: std::io::Read + Send> TransportReader for JsonRpcReader<R> {
    fn read(&mut self) -> std::io::Result<Message> {
        jsonrpc::read_message(&mut self.reader)
    }
}

struct JsonRpcWriter<W> {
    writer: W,
}

impl<W: Write + Send> TransportWriter for JsonRpcWriter<W> {
    fn write(&mut self, message: &Message) -> std::io::Result<()> {
        jsonrpc::write_message(&mut self.writer, message)
    }
}

pub struct InMemoryTransport {
    from_server: mpsc::Receiver<Message>,
    to_server: mpsc::Sender<Message>,
}

pub struct InMemoryServer {
    from_client: mpsc::Receiver<Message>,
    to_client: mpsc::Sender<Message>,
}

impl InMemoryTransport {
    pub fn pair() -> (Self, InMemoryServer) {
        let (to_server, from_client) = mpsc::channel();
        let (to_client, from_server) = mpsc::channel();
        (
            Self {
                from_server,
                to_server,
            },
            InMemoryServer {
                from_client,
                to_client,
            },
        )
    }
}

impl InMemoryServer {
    pub fn recv(&self) -> Message {
        self.from_client.recv().unwrap()
    }

    pub fn send(&self, message: Message) {
        self.to_client.send(message).unwrap();
    }
}

impl Transport for InMemoryTransport {
    fn split(
        self: Box<Self>,
    ) -> std::io::Result<(Box<dyn TransportReader>, Box<dyn TransportWriter>)> {
        Ok((
            Box::new(MemoryReader {
                rx: self.from_server,
            }),
            Box::new(MemoryWriter { tx: self.to_server }),
        ))
    }
}

struct MemoryReader {
    rx: mpsc::Receiver<Message>,
}

impl TransportReader for MemoryReader {
    fn read(&mut self) -> std::io::Result<Message> {
        self.rx.recv().map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "in-memory LSP closed")
        })
    }
}

struct MemoryWriter {
    tx: mpsc::Sender<Message>,
}

impl TransportWriter for MemoryWriter {
    fn write(&mut self, message: &Message) -> std::io::Result<()> {
        self.tx.send(message.clone()).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::BrokenPipe, "in-memory LSP closed")
        })
    }
}

struct Pending {
    kind: RequestKind,
    wait: Option<mpsc::Sender<(Option<Value>, Option<Value>)>>,
}

pub struct LspClient {
    server: String,
    writer: Arc<Mutex<Box<dyn TransportWriter>>>,
    pending: Arc<Mutex<HashMap<Id, Pending>>>,
    next_id: u64,
}

impl LspClient {
    pub fn start(
        server: impl Into<String>,
        transport: Box<dyn Transport>,
        events: mpsc::Sender<LspEvent>,
    ) -> std::io::Result<Self> {
        let server = server.into();
        let (mut reader, writer) = transport.split()?;
        let pending: Arc<Mutex<HashMap<Id, Pending>>> = Arc::new(Mutex::new(HashMap::new()));
        let pending_for_thread = Arc::clone(&pending);
        let server_for_thread = server.clone();
        std::thread::Builder::new()
            .name(format!("lsp-reader-{server}"))
            .spawn(move || loop {
                let message = match reader.read() {
                    Ok(message) => message,
                    Err(_) => {
                        let _ = events.send(LspEvent::Closed {
                            server: server_for_thread.clone(),
                        });
                        return;
                    }
                };
                match message {
                    Message::Response { id, result, error } => {
                        let pending = pending_for_thread.lock().unwrap().remove(&id);
                        let kind = pending.as_ref().map(|p| p.kind);
                        if let Some(wait) = pending.and_then(|p| p.wait) {
                            let _ = wait.send((result.clone(), error.clone()));
                        }
                        let _ = events.send(LspEvent::Response {
                            server: server_for_thread.clone(),
                            id,
                            kind,
                            result,
                            error,
                        });
                    }
                    Message::Notification { method, params } => {
                        let _ = events.send(LspEvent::Notification {
                            server: server_for_thread.clone(),
                            method,
                            params,
                        });
                    }
                    Message::Request { id, method, params } => {
                        let _ = events.send(LspEvent::Request {
                            server: server_for_thread.clone(),
                            id,
                            method,
                            params,
                        });
                    }
                }
            })?;
        Ok(Self {
            server,
            writer: Arc::new(Mutex::new(writer)),
            pending,
            next_id: 1,
        })
    }

    pub fn server(&self) -> &str {
        &self.server
    }

    pub fn request_wait(
        &mut self,
        method: &str,
        params: Value,
        kind: RequestKind,
        timeout: Duration,
    ) -> std::io::Result<(Option<Value>, Option<Value>)> {
        let id = self.next_id();
        let (tx, rx) = mpsc::channel();
        self.pending.lock().unwrap().insert(
            id.clone(),
            Pending {
                kind,
                wait: Some(tx),
            },
        );
        self.send(&Message::Request {
            id,
            method: method.into(),
            params,
        })?;
        rx.recv_timeout(timeout)
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, method))
    }

    pub fn request_async(
        &mut self,
        method: &str,
        params: Value,
        kind: RequestKind,
    ) -> std::io::Result<Id> {
        let id = self.next_id();
        self.pending
            .lock()
            .unwrap()
            .insert(id.clone(), Pending { kind, wait: None });
        self.send(&Message::Request {
            id: id.clone(),
            method: method.into(),
            params,
        })?;
        Ok(id)
    }

    pub fn notify(&mut self, method: &str, params: Value) -> std::io::Result<()> {
        self.send(&Message::Notification {
            method: method.into(),
            params,
        })
    }

    pub fn initialize(
        &mut self,
        root_uri: &str,
        process_id: Option<u32>,
    ) -> std::io::Result<Option<Value>> {
        let (result, error) = self.request_wait(
            "initialize",
            json!({
                "processId": process_id,
                "rootUri": root_uri,
                "capabilities": {
                    "textDocument": {
                        "synchronization": { "didSave": false },
                        "completion": { "completionItem": { "snippetSupport": false } },
                        "hover": {},
                        "definition": {},
                        "publishDiagnostics": {}
                    }
                }
            }),
            RequestKind::Initialize,
            Duration::from_secs(5),
        )?;
        if let Some(error) = error {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("initialize failed: {error}"),
            ));
        }
        Ok(result)
    }

    pub fn initialized(&mut self) -> std::io::Result<()> {
        self.notify("initialized", json!({}))
    }

    pub fn shutdown(&mut self) -> std::io::Result<()> {
        let _ = self.request_wait(
            "shutdown",
            Value::Null,
            RequestKind::Shutdown,
            Duration::from_secs(2),
        );
        self.notify("exit", Value::Null)
    }

    fn next_id(&mut self) -> Id {
        let id = self.next_id;
        self.next_id += 1;
        Id::Number(id)
    }

    fn send(&mut self, message: &Message) -> std::io::Result<()> {
        self.writer.lock().unwrap().write(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_initialized_and_shutdown_correlate_ids_against_a_fake_server() {
        let (transport, server) = InMemoryTransport::pair();
        let (events_tx, events_rx) = mpsc::channel();
        let mut client = LspClient::start("fake", Box::new(transport), events_tx).unwrap();

        let server_thread = std::thread::spawn(move || {
            let init = server.recv();
            let Message::Request { id, method, .. } = init else {
                panic!("expected initialize request");
            };
            assert_eq!(method, "initialize");
            server.send(Message::Response {
                id,
                result: Some(json!({"capabilities":{}})),
                error: None,
            });

            let initialized = server.recv();
            assert!(matches!(
                initialized,
                Message::Notification { ref method, .. } if method == "initialized"
            ));

            let shutdown = server.recv();
            let Message::Request { id, method, .. } = shutdown else {
                panic!("expected shutdown request");
            };
            assert_eq!(method, "shutdown");
            server.send(Message::Response {
                id,
                result: Some(Value::Null),
                error: None,
            });
            let exit = server.recv();
            assert!(matches!(
                exit,
                Message::Notification { ref method, .. } if method == "exit"
            ));
        });

        assert!(client.initialize("file:///tmp", None).unwrap().is_some());
        client.initialized().unwrap();
        client.shutdown().unwrap();
        server_thread.join().unwrap();
        assert!(matches!(
            events_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            LspEvent::Response {
                kind: Some(RequestKind::Initialize),
                ..
            }
        ));
    }

    #[test]
    fn server_notifications_reach_the_event_channel() {
        let (transport, server) = InMemoryTransport::pair();
        let (events_tx, events_rx) = mpsc::channel();
        let _client = LspClient::start("fake", Box::new(transport), events_tx).unwrap();
        server.send(Message::Notification {
            method: "textDocument/publishDiagnostics".into(),
            params: json!({"uri":"file:///tmp/x.rs","diagnostics":[]}),
        });
        assert!(matches!(
            events_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            LspEvent::Notification { method, .. } if method == "textDocument/publishDiagnostics"
        ));
    }
}
