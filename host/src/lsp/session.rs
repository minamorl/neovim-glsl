use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use serde_json::{json, Value};

use crate::core::buffers::BufferId;
use crate::core::Buffer;

use super::client::{ChildProcessTransport, LspClient, LspEvent, RequestKind};
use super::registry::{self, ServerSpec};
use super::types::{path_to_uri, CompletionItem, Diagnostic, Hover, Location, Position};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletionMenu {
    pub items: Vec<CompletionItem>,
    pub selected: Option<usize>,
    pub row: usize,
    pub col: usize,
    pub grid: u64,
}

impl CompletionMenu {
    pub fn select_next(&mut self) {
        if self.items.is_empty() {
            self.selected = None;
            return;
        }
        self.selected = Some(self.selected.map_or(0, |i| (i + 1) % self.items.len()));
    }

    pub fn select_prev(&mut self) {
        if self.items.is_empty() {
            self.selected = None;
            return;
        }
        self.selected = Some(match self.selected {
            Some(0) | None => self.items.len() - 1,
            Some(i) => i - 1,
        });
    }
}

struct Server {
    spec: ServerSpec,
    root: PathBuf,
    client: LspClient,
}

#[derive(Default)]
pub struct LspSession {
    servers: BTreeMap<String, Server>,
    documents: HashMap<BufferId, DocumentState>,
    diagnostics: HashMap<String, Vec<Diagnostic>>,
    missing: BTreeMap<String, String>,
    pub completion: Option<CompletionMenu>,
    definition: Option<Location>,
}

#[derive(Clone)]
struct DocumentState {
    uri: String,
    path: PathBuf,
    revision: u64,
    version: i32,
    servers: Vec<String>,
}

impl LspSession {
    pub fn sync_current(
        &mut self,
        id: BufferId,
        buffer: &Buffer,
        event_tx: &mpsc::Sender<LspEvent>,
    ) -> Option<String> {
        let path = buffer.path()?.to_path_buf();
        let uri = path_to_uri(&path);
        let specs = registry::servers_for_path(&path);
        if specs.is_empty() {
            return None;
        }

        let mut status = None;
        let mut server_names = Vec::new();
        for spec in specs {
            if !spec.argv[0].exists() {
                if !self.missing.contains_key(spec.name) {
                    let message = spec.missing_message();
                    status = Some(message.clone());
                    self.missing.insert(spec.name.into(), message);
                }
                continue;
            }
            let name = spec.name.to_string();
            if !self.servers.contains_key(&name) {
                match self.start_server(spec.clone(), &path, event_tx.clone()) {
                    Ok(()) => {}
                    Err(error) => {
                        status = Some(format!("LSP server '{}' failed: {error}", spec.name));
                        continue;
                    }
                }
            }
            server_names.push(name);
        }

        let text = buffer.text();
        match self.documents.get_mut(&id) {
            Some(doc) if doc.uri == uri => {
                if doc.revision != buffer.revision() {
                    doc.revision = buffer.revision();
                    doc.version += 1;
                    for server in &doc.servers {
                        if let Some(server) = self.servers.get_mut(server) {
                            let _ = server.client.notify(
                                "textDocument/didChange",
                                json!({
                                    "textDocument": {
                                        "uri": doc.uri,
                                        "version": doc.version
                                    },
                                    "contentChanges": [{ "text": text }]
                                }),
                            );
                        }
                    }
                }
            }
            Some(doc) => {
                for server in &doc.servers {
                    if let Some(server) = self.servers.get_mut(server) {
                        let _ = server.client.notify(
                            "textDocument/didClose",
                            json!({"textDocument": { "uri": doc.uri }}),
                        );
                    }
                }
                self.open_document(id, path, uri, buffer, server_names);
            }
            None => self.open_document(id, path, uri, buffer, server_names),
        }
        status
    }

    pub fn diagnostics_for_current(&self, buffer: &Buffer) -> Vec<Diagnostic> {
        let Some(path) = buffer.path() else {
            return Vec::new();
        };
        let uri = path_to_uri(path);
        self.diagnostics.get(&uri).cloned().unwrap_or_default()
    }

    pub fn handle_event(&mut self, event: LspEvent) -> Option<String> {
        match event {
            LspEvent::Notification { method, params, .. }
                if method == "textDocument/publishDiagnostics" =>
            {
                let Some(uri) = params.get("uri").and_then(Value::as_str) else {
                    return None;
                };
                let diagnostics = params
                    .get("diagnostics")
                    .cloned()
                    .and_then(|value| serde_json::from_value::<Vec<Diagnostic>>(value).ok())
                    .unwrap_or_default();
                self.diagnostics.insert(uri.to_string(), diagnostics);
                None
            }
            LspEvent::Response {
                kind: Some(RequestKind::Completion),
                result,
                ..
            } => {
                let items = completion_items(result.unwrap_or(Value::Null));
                if items.is_empty() {
                    self.completion = None;
                } else if let Some(menu) = self.completion.as_mut() {
                    menu.items = items;
                    menu.selected = Some(0);
                }
                None
            }
            LspEvent::Response {
                kind: Some(RequestKind::Hover),
                result: Some(value),
                ..
            } => serde_json::from_value::<Hover>(value)
                .ok()
                .map(|hover| hover.plain_text())
                .filter(|text| !text.is_empty()),
            LspEvent::Response {
                kind: Some(RequestKind::Definition),
                result: Some(value),
                ..
            } => {
                self.definition = definition_location(value);
                if self.definition.is_none() {
                    Some("LSP definition not found".into())
                } else {
                    None
                }
            }
            LspEvent::Closed { server } => Some(format!("LSP server '{server}' exited")),
            _ => None,
        }
    }

    pub fn request_completion(
        &mut self,
        buffer: &Buffer,
        cursor: (usize, usize),
        row: usize,
        col: usize,
        grid: u64,
    ) {
        self.completion = Some(CompletionMenu {
            items: Vec::new(),
            selected: None,
            row,
            col,
            grid,
        });
        let Some(doc) = self.doc_for_buffer(buffer) else {
            return;
        };
        let position = Position::from_buffer_chars(buffer, cursor.0, cursor.1);
        for server in doc.servers {
            if let Some(server) = self.servers.get_mut(&server) {
                let _ = server.client.request_async(
                    "textDocument/completion",
                    json!({
                        "textDocument": { "uri": doc.uri },
                        "position": position
                    }),
                    RequestKind::Completion,
                );
                return;
            }
        }
    }

    pub fn request_hover(&mut self, buffer: &Buffer, cursor: (usize, usize)) -> Option<String> {
        if let Some(diagnostic) = self
            .diagnostics_for_current(buffer)
            .into_iter()
            .find(|d| d.range.contains_buffer_position(buffer, cursor.0, cursor.1))
        {
            return Some(diagnostic.message);
        }
        let doc = self.doc_for_buffer(buffer)?;
        let position = Position::from_buffer_chars(buffer, cursor.0, cursor.1);
        for server in doc.servers {
            if let Some(server) = self.servers.get_mut(&server) {
                let _ = server.client.request_async(
                    "textDocument/hover",
                    json!({
                        "textDocument": { "uri": doc.uri },
                        "position": position
                    }),
                    RequestKind::Hover,
                );
                return None;
            }
        }
        None
    }

    pub fn request_definition(&mut self, buffer: &Buffer, cursor: (usize, usize)) {
        let Some(doc) = self.doc_for_buffer(buffer) else {
            return;
        };
        let position = Position::from_buffer_chars(buffer, cursor.0, cursor.1);
        for server in doc.servers {
            if let Some(server) = self.servers.get_mut(&server) {
                let _ = server.client.request_async(
                    "textDocument/definition",
                    json!({
                        "textDocument": { "uri": doc.uri },
                        "position": position
                    }),
                    RequestKind::Definition,
                );
                return;
            }
        }
    }

    pub fn take_definition(&mut self) -> Option<Location> {
        self.definition.take()
    }

    pub fn hide_completion(&mut self) {
        self.completion = None;
    }

    pub fn apply_completion_selection(&self) -> Option<String> {
        let menu = self.completion.as_ref()?;
        let index = menu.selected?;
        Some(menu.items.get(index)?.word().to_string())
    }

    fn open_document(
        &mut self,
        id: BufferId,
        path: PathBuf,
        uri: String,
        buffer: &Buffer,
        servers: Vec<String>,
    ) {
        let version = buffer.revision().min(i32::MAX as u64) as i32;
        for server_name in &servers {
            if let Some(server) = self.servers.get_mut(server_name) {
                let _ = server.client.notify(
                    "textDocument/didOpen",
                    json!({
                        "textDocument": {
                            "uri": uri,
                            "languageId": server.spec.language_id,
                            "version": version,
                            "text": buffer.text()
                        }
                    }),
                );
            }
        }
        self.documents.insert(
            id,
            DocumentState {
                uri,
                path,
                revision: buffer.revision(),
                version,
                servers,
            },
        );
    }

    fn start_server(
        &mut self,
        spec: ServerSpec,
        path: &Path,
        event_tx: mpsc::Sender<LspEvent>,
    ) -> std::io::Result<()> {
        let root = registry::root_for(path, spec.root_markers);
        let root_uri = path_to_uri(&root);
        let transport = ChildProcessTransport::spawn(&spec.argv[0], &spec.args, &root)?;
        let mut client = LspClient::start(spec.name, Box::new(transport), event_tx)?;
        client.initialize(&root_uri, Some(std::process::id()))?;
        client.initialized()?;
        self.servers
            .insert(spec.name.into(), Server { spec, root, client });
        Ok(())
    }

    fn doc_for_buffer(&self, buffer: &Buffer) -> Option<DocumentState> {
        let uri = path_to_uri(buffer.path()?);
        self.documents.values().find(|doc| doc.uri == uri).cloned()
    }
}

impl Drop for LspSession {
    fn drop(&mut self) {
        for server in self.servers.values_mut() {
            let _ = server.client.shutdown();
        }
    }
}

fn completion_items(value: Value) -> Vec<CompletionItem> {
    if let Some(items) = value.get("items").cloned() {
        serde_json::from_value(items).unwrap_or_default()
    } else {
        serde_json::from_value(value).unwrap_or_default()
    }
}

fn definition_location(value: Value) -> Option<Location> {
    if value.is_null() {
        return None;
    }
    if let Ok(location) = serde_json::from_value::<Location>(value.clone()) {
        return Some(location);
    }
    value
        .as_array()
        .and_then(|items| items.first().cloned())
        .and_then(|value| serde_json::from_value::<Location>(value).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_menu_wraps_selection() {
        let mut menu = CompletionMenu {
            items: vec![
                CompletionItem {
                    label: "a".into(),
                    ..CompletionItem::default()
                },
                CompletionItem {
                    label: "b".into(),
                    ..CompletionItem::default()
                },
            ],
            selected: None,
            row: 0,
            col: 0,
            grid: 1,
        };
        menu.select_next();
        assert_eq!(menu.selected, Some(0));
        menu.select_prev();
        assert_eq!(menu.selected, Some(1));
    }

    #[test]
    fn publish_diagnostics_updates_uri_bucket() {
        let mut session = LspSession::default();
        session.handle_event(LspEvent::Notification {
            server: "fake".into(),
            method: "textDocument/publishDiagnostics".into(),
            params: json!({
                "uri": "file:///tmp/x.rs",
                "diagnostics": [{
                    "range": {
                        "start": { "line": 0, "character": 1 },
                        "end": { "line": 0, "character": 2 }
                    },
                    "severity": 1,
                    "message": "bad"
                }]
            }),
        });
        assert_eq!(session.diagnostics["file:///tmp/x.rs"][0].message, "bad");
    }

    #[test]
    fn definition_response_keeps_the_first_location() {
        let mut session = LspSession::default();
        session.handle_event(LspEvent::Response {
            server: "fake".into(),
            id: super::super::jsonrpc::Id::Number(1),
            kind: Some(RequestKind::Definition),
            result: Some(json!([{
                "uri": "file:///tmp/main.rs",
                "range": {
                    "start": { "line": 2, "character": 4 },
                    "end": { "line": 2, "character": 8 }
                }
            }])),
            error: None,
        });
        let location = session.take_definition().unwrap();
        assert_eq!(location.uri, "file:///tmp/main.rs");
        assert_eq!(location.range.start.line, 2);
    }
}
