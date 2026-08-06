//! Project search.
//!
//! `rg` is preferred because `pin ide_level_includes_project_search` asks for
//! real project grep. The fallback is deliberately labelled literal: it does
//! not pretend to be a regular expression engine, following the same discipline
//! as `command.rs::substitute`.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::Value;

pub const MIN_QUERY_CHARS: usize = 2;
pub const DEBOUNCE: Duration = Duration::from_millis(60);
pub const MAX_HITS: usize = 10_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Engine {
    Ripgrep,
    LiteralFallback,
}

impl Engine {
    pub fn label(&self) -> &'static str {
        match self {
            Engine::Ripgrep => "rg",
            Engine::LiteralFallback => "literal fallback",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    pub path: PathBuf,
    /// 1-based, because this is what `:e +{line}` takes.
    pub line: usize,
    /// Zero-based character column for `Editor.cursor.1`.
    pub column: usize,
    pub text: String,
}

impl Hit {
    pub fn display(&self, root: &Path) -> String {
        let path = self.path.strip_prefix(root).unwrap_or(&self.path);
        format!(
            "{}:{}:{}: {}",
            path.display(),
            self.line,
            self.column + 1,
            self.text.trim_end()
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchResult {
    pub engine: Engine,
    pub hits: Vec<Hit>,
    pub truncated: bool,
}

pub fn rg_path() -> Option<PathBuf> {
    for candidate in [PathBuf::from("/opt/homebrew/bin/rg")] {
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    find_on_path("rg")
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

pub fn search(root: &Path, query: &str) -> SearchResult {
    if query.chars().count() < MIN_QUERY_CHARS {
        return SearchResult {
            engine: Engine::Ripgrep,
            hits: Vec::new(),
            truncated: false,
        };
    }
    match rg_path() {
        Some(rg) => search_rg(root, query, &rg),
        None => search_literal(root, query),
    }
}

fn rg_argv(rg: &Path, query: &str) -> Vec<String> {
    vec![
        rg.display().to_string(),
        "--json".into(),
        "--line-number".into(),
        "--column".into(),
        "--smart-case".into(),
        "--hidden".into(),
        "--glob=!.git".into(),
        "--max-columns".into(),
        "300".into(),
        query.to_string(),
        ".".into(),
    ]
}

fn search_rg(root: &Path, query: &str, rg: &Path) -> SearchResult {
    let mut task = match crate::run::spawn(
        crate::run::Origin::OwnerExCommand,
        rg_argv(rg, query),
        root.to_path_buf(),
    ) {
        Ok(task) => task,
        Err(_) => return search_literal(root, query),
    };
    let mut hits = Vec::new();
    let mut stdout = String::new();
    loop {
        for segment in task.poll() {
            if segment.role.stream != crate::run::Stream::Stdout {
                continue;
            }
            stdout.push_str(&segment.text);
            consume_stdout(root, &mut stdout, &mut hits);
            if hits.len() >= MAX_HITS {
                task.cancel();
                return SearchResult {
                    engine: Engine::Ripgrep,
                    hits,
                    truncated: true,
                };
            }
        }
        if task.status().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    consume_stdout(root, &mut stdout, &mut hits);
    SearchResult {
        engine: Engine::Ripgrep,
        hits,
        truncated: false,
    }
}

fn parse_match(root: &Path, line: &str) -> Option<Hit> {
    let value: Value = serde_json::from_str(line).ok()?;
    if value.get("type")?.as_str()? != "match" {
        return None;
    }
    let data = value.get("data")?;
    let path = root.join(data.get("path")?.get("text")?.as_str()?);
    let line_number = data.get("line_number")?.as_u64()? as usize;
    let text = data.get("lines")?.get("text")?.as_str()?.to_string();
    let submatch = data.get("submatches")?.as_array()?.first()?;
    let byte_column = submatch.get("start")?.as_u64()? as usize;
    let chars: Vec<char> = text.trim_end_matches(['\n', '\r']).chars().collect();
    let column = crate::textpos::byte_to_char(&chars, byte_column);
    Some(Hit {
        path,
        line: line_number,
        column,
        text,
    })
}

fn search_literal(root: &Path, query: &str) -> SearchResult {
    let mut hits = Vec::new();
    let sensitive = query.chars().any(char::is_uppercase);
    let needle = if sensitive {
        query.to_string()
    } else {
        query.to_lowercase()
    };
    walk_literal(root, root, &needle, sensitive, &mut hits);
    let truncated = hits.len() > MAX_HITS;
    hits.truncate(MAX_HITS);
    SearchResult {
        engine: Engine::LiteralFallback,
        hits,
        truncated,
    }
}

fn walk_literal(root: &Path, dir: &Path, needle: &str, sensitive: bool, hits: &mut Vec<Hit>) {
    if hits.len() > MAX_HITS {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if name.to_string_lossy() == ".git" {
            continue;
        }
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => walk_literal(root, &path, needle, sensitive, hits),
            Ok(kind) if kind.is_file() => scan_literal_file(&path, needle, sensitive, hits),
            _ => {}
        }
        if hits.len() > MAX_HITS {
            return;
        }
    }
}

fn scan_literal_file(path: &Path, needle: &str, sensitive: bool, hits: &mut Vec<Hit>) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    for (index, line) in text.lines().enumerate() {
        let haystack = if sensitive {
            line.to_string()
        } else {
            line.to_lowercase()
        };
        if let Some(byte) = haystack.find(needle) {
            let chars: Vec<char> = line.chars().collect();
            hits.push(Hit {
                path: path.to_path_buf(),
                line: index + 1,
                column: crate::textpos::byte_to_char(&chars, byte),
                text: line.to_string(),
            });
            if hits.len() > MAX_HITS {
                return;
            }
        }
    }
}

pub struct LiveSearch {
    root: PathBuf,
    origin: crate::run::Origin,
    query: String,
    changed_at: Instant,
    running: Option<crate::run::Task>,
    hits: Vec<Hit>,
    engine: Engine,
    truncated: bool,
    searched_query: Option<String>,
    stdout: String,
}

impl LiveSearch {
    pub fn new(root: PathBuf, origin: crate::run::Origin) -> Self {
        Self {
            root,
            origin,
            query: String::new(),
            changed_at: Instant::now(),
            running: None,
            hits: Vec::new(),
            engine: Engine::Ripgrep,
            truncated: false,
            searched_query: None,
            stdout: String::new(),
        }
    }

    pub fn set_query(&mut self, query: String) {
        if self.query == query {
            return;
        }
        if let Some(mut running) = self.running.take() {
            running.cancel();
        }
        self.query = query;
        self.changed_at = Instant::now();
        self.hits.clear();
        self.stdout.clear();
        self.truncated = false;
        self.searched_query = None;
    }

    pub fn poll(&mut self) {
        if let Some(task) = self.running.as_mut() {
            for segment in task.poll() {
                if segment.role.stream == crate::run::Stream::Stdout {
                    self.stdout.push_str(&segment.text);
                }
            }
            consume_stdout(&self.root, &mut self.stdout, &mut self.hits);
            if self.hits.len() >= MAX_HITS {
                self.truncated = true;
                task.cancel();
                self.running = None;
                return;
            }
            if task.status().is_some() {
                consume_stdout(&self.root, &mut self.stdout, &mut self.hits);
                self.running = None;
            }
            return;
        }
        if self.query.chars().count() < MIN_QUERY_CHARS || self.changed_at.elapsed() < DEBOUNCE {
            return;
        }
        if self.searched_query.as_deref() != Some(self.query.as_str()) {
            self.start();
        }
    }

    fn start(&mut self) {
        self.searched_query = Some(self.query.clone());
        let Some(rg) = rg_path() else {
            let result = search_literal(&self.root, &self.query);
            self.engine = result.engine;
            self.hits = result.hits;
            self.truncated = result.truncated;
            return;
        };
        let task =
            match crate::run::spawn(self.origin, rg_argv(&rg, &self.query), self.root.clone()) {
                Ok(task) => task,
                Err(_) => {
                    let result = search_literal(&self.root, &self.query);
                    self.engine = result.engine;
                    self.hits = result.hits;
                    self.truncated = result.truncated;
                    return;
                }
            };
        self.engine = Engine::Ripgrep;
        self.running = Some(task);
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn hits(&self) -> &[Hit] {
        &self.hits
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    pub fn truncated(&self) -> bool {
        self.truncated
    }

    pub fn running(&self) -> bool {
        self.running.is_some()
    }

    pub fn running_query(&self) -> Option<&str> {
        if self.running.is_some() {
            self.searched_query.as_deref()
        } else {
            None
        }
    }
}

impl Drop for LiveSearch {
    fn drop(&mut self) {
        if let Some(mut running) = self.running.take() {
            running.cancel();
        }
    }
}

fn consume_stdout(root: &Path, stdout: &mut String, hits: &mut Vec<Hit>) {
    while let Some(newline) = stdout.find('\n') {
        let line: String = stdout.drain(..=newline).collect();
        if let Some(hit) = parse_match(root, line.trim_end()) {
            hits.push(hit);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rg_json_byte_offsets_become_character_columns() {
        let line = r#"{"type":"match","data":{"path":{"text":"sample.txt"},"lines":{"text":"abc あいう\n"},"line_number":7,"absolute_offset":0,"submatches":[{"match":{"text":"い"},"start":7,"end":10}]}}"#;
        let hit = parse_match(Path::new(""), line).unwrap();
        assert_eq!(hit.line, 7);
        assert_eq!(hit.column, 5);
    }

    #[test]
    fn literal_fallback_is_literal_not_regex() {
        let root = std::env::temp_dir().join(format!("nvimglsl-grep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("a.txt"), "a.c\nabc\n").unwrap();
        let result = search_literal(&root, "a.c");
        assert_eq!(result.engine, Engine::LiteralFallback);
        assert_eq!(result.hits.len(), 1);
        assert_eq!(result.hits[0].line, 1);
        let _ = std::fs::remove_dir_all(&root);
    }
}
