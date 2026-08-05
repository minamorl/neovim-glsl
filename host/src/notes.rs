//! The note store.
//!
//! `pin primary_object` makes a markdown note the thing being edited,
//! `pin note_substrate` makes the substrate the existing yui notes, and
//! `pin note_substrate_not_new` forbids inventing a second store. Those three
//! together already name the store: yui mirrors `yui_notes` into a local
//! markdown vault and syncs it back, so the vault *is* the local half of
//! `pin storage_model`'s local-repository-and-DB pair.
//!
//! So this file opens that vault. It does not define a note format, a database,
//! or a sync protocol — doing any of those would be the new independent store
//! the pin forbids, wearing a different name.

use std::path::{Path, PathBuf};

/// Where the yui note vault lives.
///
/// The environment variable is yui's own (`OBSIDIAN_VAULT_PATH`), and the
/// fallback is the path its mirror defaults to. Reading yui's variable rather
/// than minting `NVIMGLSL_NOTES` is the difference between sharing a substrate
/// and having a private one that merely looks the same.
pub fn vault_root() -> PathBuf {
    if let Some(configured) = std::env::var_os("OBSIDIAN_VAULT_PATH") {
        if !configured.is_empty() {
            return PathBuf::from(configured);
        }
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    home.join("repos").join("obsidian")
}

pub struct Vault {
    root: PathBuf,
}

/// Directories the mirror keeps for its own bookkeeping rather than for notes.
const SKIP: [&str; 5] = [".git", ".obsidian", "attachments", "node_modules", ".trash"];

impl Vault {
    pub fn open(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn default_vault() -> Self {
        Self::open(vault_root())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn exists(&self) -> bool {
        self.root.is_dir()
    }

    pub fn label(&self) -> String {
        self.root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("notes")
            .to_string()
    }

    /// Every note, as a path relative to the vault.
    pub fn notes(&self) -> Vec<String> {
        let mut out = Vec::new();
        collect(&self.root, &self.root, 0, &mut out);
        out.sort();
        out
    }

    pub fn path_of(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    /// Resolve a `[[wiki link]]` the way the vault addresses notes: by note
    /// name, anywhere in the tree, with or without the extension.
    pub fn resolve_link(&self, link: &str) -> Option<String> {
        // A link may carry a display alias (`[[note|shown]]`) or a heading
        // (`[[note#section]]`); neither is part of the note's identity.
        let target = link.split('|').next().unwrap_or(link);
        let target = target.split('#').next().unwrap_or(target).trim();
        if target.is_empty() {
            return None;
        }
        let wanted = target.trim_end_matches(".md");
        let notes = self.notes();
        notes
            .iter()
            .find(|note| note.trim_end_matches(".md") == wanted)
            .or_else(|| {
                notes.iter().find(|note| {
                    Path::new(note)
                        .file_stem()
                        .and_then(|stem| stem.to_str())
                        .is_some_and(|stem| stem == wanted)
                })
            })
            .cloned()
    }

    /// Create a note and return its path. An existing note of that name is
    /// opened rather than overwritten.
    pub fn create(&self, title: &str) -> std::io::Result<PathBuf> {
        let title = title.trim();
        if title.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "a note needs a title",
            ));
        }
        let relative = if title.ends_with(".md") {
            title.to_string()
        } else {
            format!("{title}.md")
        };
        let path = self.root.join(&relative);
        if path.exists() {
            return Ok(path);
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let stem = Path::new(&relative)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(title);
        std::fs::write(&path, format!("# {stem}\n\n"))?;
        Ok(path)
    }
}

fn collect(root: &Path, dir: &Path, depth: usize, out: &mut Vec<String>) {
    if depth > 8 || out.len() > 20_000 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || SKIP.contains(&name.as_ref()) {
            continue;
        }
        let path = entry.path();
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => collect(root, &path, depth + 1, out),
            Ok(_) if name.ends_with(".md") || name.ends_with(".markdown") => {
                out.push(
                    path.strip_prefix(root)
                        .unwrap_or(&path)
                        .display()
                        .to_string(),
                );
            }
            _ => {}
        }
    }
}

/// The `[[link]]` under a cursor column, if there is one.
pub fn link_at(line: &str, column: usize) -> Option<String> {
    let chars: Vec<char> = line.chars().collect();
    let mut index = 0;
    while index + 1 < chars.len() {
        if chars[index] == '[' && chars[index + 1] == '[' {
            let start = index + 2;
            let mut scan = start;
            while scan + 1 < chars.len() && !(chars[scan] == ']' && chars[scan + 1] == ']') {
                scan += 1;
            }
            if scan + 1 < chars.len() {
                if column >= index && column <= scan + 1 {
                    return Some(chars[start..scan].iter().collect());
                }
                index = scan + 2;
                continue;
            }
            return None;
        }
        index += 1;
    }
    None
}

/// A picker source over the vault.
pub struct NotesSource {
    label: String,
    entries: Vec<String>,
}

impl NotesSource {
    pub fn new(vault: &Vault) -> Self {
        Self {
            label: vault.label(),
            entries: vault.notes(),
        }
    }
}

impl crate::picker::Source for NotesSource {
    fn candidates(&self) -> Vec<String> {
        self.entries.clone()
    }

    fn label(&self) -> &str {
        &self.label
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nvimglsl-notes-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("daily")).unwrap();
        std::fs::create_dir_all(dir.join(".obsidian")).unwrap();
        std::fs::create_dir_all(dir.join("attachments")).unwrap();
        std::fs::write(
            dir.join("index.md"),
            "# index\n\nsee [[daily/2026-08-03]]\n",
        )
        .unwrap();
        std::fs::write(dir.join("daily/2026-08-03.md"), "# today\n").unwrap();
        std::fs::write(dir.join(".obsidian/workspace.json"), "{}").unwrap();
        std::fs::write(dir.join("attachments/a.md"), "not a note").unwrap();
        dir
    }

    #[test]
    fn the_vault_lists_notes_and_skips_its_own_bookkeeping() {
        let dir = scratch("list");
        let vault = Vault::open(dir.clone());
        let notes = vault.notes();
        assert_eq!(
            notes,
            vec!["daily/2026-08-03.md".to_string(), "index.md".to_string()]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_wiki_link_resolves_by_path_or_by_name() {
        let dir = scratch("link");
        let vault = Vault::open(dir.clone());
        assert_eq!(
            vault.resolve_link("daily/2026-08-03"),
            Some("daily/2026-08-03.md".into())
        );
        assert_eq!(
            vault.resolve_link("2026-08-03"),
            Some("daily/2026-08-03.md".into())
        );
        assert_eq!(
            vault.resolve_link("2026-08-03|today"),
            Some("daily/2026-08-03.md".into())
        );
        assert_eq!(
            vault.resolve_link("2026-08-03#heading"),
            Some("daily/2026-08-03.md".into())
        );
        assert_eq!(vault.resolve_link("nothing"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn creating_a_note_twice_opens_the_first_one() {
        let dir = scratch("create");
        let vault = Vault::open(dir.clone());
        let path = vault.create("new idea").unwrap();
        std::fs::write(&path, "# new idea\n\nkept\n").unwrap();
        let again = vault.create("new idea").unwrap();
        assert_eq!(path, again);
        assert!(std::fs::read_to_string(&again).unwrap().contains("kept"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_link_under_the_cursor_is_the_one_the_cursor_is_in() {
        let line = "see [[alpha]] and [[beta]]";
        assert_eq!(link_at(line, 6), Some("alpha".into()));
        assert_eq!(link_at(line, 20), Some("beta".into()));
        assert_eq!(link_at(line, 15), None);
        assert_eq!(link_at("no links here", 3), None);
        assert_eq!(link_at("unclosed [[link", 12), None);
    }

    #[test]
    fn the_vault_root_follows_yuis_own_variable() {
        // Read rather than set, so the test cannot disturb a parallel one.
        let root = vault_root();
        assert!(root.is_absolute() || std::env::var_os("HOME").is_none());
        assert!(root.ends_with("obsidian") || std::env::var_os("OBSIDIAN_VAULT_PATH").is_some());
    }
}
