//! Repository entry points.
//!
//! This module provides a picker source only. Choosing what the first screen
//! should be remains behind `entry_point_source`.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::picker::Source;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkspaceKind {
    Repository,
    Vault,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Workspace {
    pub path: PathBuf,
    pub name: String,
    pub kind: WorkspaceKind,
    modified: SystemTime,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryPointOrientation {
    Repository,
    RecentRepository,
    NotesAndRepositories,
}

impl EntryPointOrientation {
    pub fn pinned_default() -> Self {
        Self::Repository
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntryPointSource {
    Repositories(WorkspaceSource),
    RecentRepository(Option<Workspace>),
    NotesAndRepositories(WorkspaceSource),
}

pub fn entry_point_source(orientation: EntryPointOrientation) -> EntryPointSource {
    let source = WorkspaceSource::discover();
    match orientation {
        EntryPointOrientation::Repository => EntryPointSource::Repositories(source),
        EntryPointOrientation::RecentRepository => {
            EntryPointSource::RecentRepository(source.workspaces.first().cloned())
        }
        EntryPointOrientation::NotesAndRepositories => {
            EntryPointSource::NotesAndRepositories(source)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceSource {
    roots: Vec<PathBuf>,
    workspaces: Vec<Workspace>,
}

impl WorkspaceSource {
    pub fn discover() -> Self {
        let roots = workspace_roots();
        Self::from_roots(roots)
    }

    pub fn from_roots(roots: Vec<PathBuf>) -> Self {
        let mut workspaces = Vec::new();
        for root in &roots {
            let Ok(entries) = std::fs::read_dir(root) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                    continue;
                }
                let Some(kind) = workspace_kind(&path) else {
                    continue;
                };
                let modified = entry
                    .metadata()
                    .and_then(|metadata| metadata.modified())
                    .unwrap_or(SystemTime::UNIX_EPOCH);
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("/")
                    .to_string();
                workspaces.push(Workspace {
                    path,
                    name,
                    kind,
                    modified,
                });
            }
        }
        workspaces.sort_by(|a, b| {
            b.modified.cmp(&a.modified).then_with(|| {
                a.name
                    .to_ascii_lowercase()
                    .cmp(&b.name.to_ascii_lowercase())
            })
        });
        Self { roots, workspaces }
    }

    pub fn workspaces(&self) -> &[Workspace] {
        &self.workspaces
    }

    pub fn path_for_candidate(&self, candidate: &str) -> Option<PathBuf> {
        self.workspaces
            .iter()
            .find(|workspace| workspace.path.display().to_string() == candidate)
            .map(|workspace| workspace.path.clone())
    }
}

impl Source for WorkspaceSource {
    fn candidates(&self) -> Vec<String> {
        self.workspaces
            .iter()
            .map(|workspace| workspace.path.display().to_string())
            .collect()
    }

    fn label(&self) -> &str {
        "repositories"
    }
}

/// A directory the owner would plausibly be working in.
///
/// A Dock or Finder launch carries no working directory of its own: the process
/// inherits `/`. Rooting the file tree there is not just unhelpful, it is how
/// `/dev` and a 23,000-entry `/nix/store` end up one keystroke from the cursor —
/// and a path under `/dev` is exactly the kind that blocks `open(2)` forever.
///
/// `pin entry_point_orientation = repository` already says where work begins.
/// This applies the same answer to the tree, so a launch with no argument lands
/// on the repositories instead of the filesystem root.
/// Only the filesystem root is rejected. Where else the owner keeps a checkout —
/// an external volume, `/opt`, a mount — is their business, and a rule that
/// second-guessed it would move the tree out from under work that was fine.
pub fn workable_root(candidate: PathBuf) -> PathBuf {
    if candidate.parent().is_some() {
        return candidate;
    }
    workspace_roots()
        .into_iter()
        .find(|root| root.is_dir())
        .unwrap_or(candidate)
}

fn workspace_roots() -> Vec<PathBuf> {
    if let Ok(value) = std::env::var("NVIMGLSL_WORKSPACES") {
        let roots: Vec<PathBuf> = std::env::split_paths(&value).collect();
        if !roots.is_empty() {
            return roots;
        }
    }
    match std::env::var("HOME") {
        Ok(home) => vec![PathBuf::from(home).join("repos")],
        Err(_) => vec![PathBuf::from("repos")],
    }
}

fn workspace_kind(path: &Path) -> Option<WorkspaceKind> {
    if path.file_name().and_then(|name| name.to_str()) == Some("obsidian")
        || path.join(".obsidian").is_dir()
    {
        return Some(WorkspaceKind::Vault);
    }
    path.join(".git")
        .is_dir()
        .then_some(WorkspaceKind::Repository)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "nvimglsl-workspace-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn repositories_under_roots_are_picker_candidates() {
        let root = temp("repos");
        std::fs::create_dir_all(root.join("a/.git")).unwrap();
        std::fs::create_dir_all(root.join("obsidian/.obsidian")).unwrap();
        std::fs::create_dir_all(root.join("plain")).unwrap();

        let source = WorkspaceSource::from_roots(vec![root.clone()]);
        assert_eq!(source.workspaces().len(), 2);
        assert!(source
            .workspaces()
            .iter()
            .any(|workspace| workspace.kind == WorkspaceKind::Vault));
        assert_eq!(source.candidates().len(), 2);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn entry_point_default_is_a_seam_not_a_launch_decision() {
        assert_eq!(
            EntryPointOrientation::pinned_default(),
            EntryPointOrientation::Repository
        );
    }

    /// A Dock launch inherits `/`, and `/` must never become the tree root:
    /// that is what put `/dev` within reach of the cursor.
    #[test]
    fn filesystem_root_is_replaced_by_the_repositories() {
        let root = workable_root(PathBuf::from("/"));
        assert_ne!(root, PathBuf::from("/"));
        assert!(root.ends_with("repos"), "unexpected fallback: {:?}", root);
    }

    #[test]
    fn a_directory_under_home_is_kept_as_is() {
        let Ok(home) = std::env::var("HOME") else {
            return;
        };
        let mine = PathBuf::from(&home).join("repos/neovim-glsl");
        assert_eq!(workable_root(mine.clone()), mine);
    }
}
