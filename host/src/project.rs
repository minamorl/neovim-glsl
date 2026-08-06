//! Project-root discovery for IDE features.
//!
//! A linked worktree stores `.git` as a file containing a pointer to the real
//! git directory, while a normal checkout stores it as a directory. The search
//! lane treats both as a project boundary and otherwise falls back to the
//! process cwd.

use std::path::{Path, PathBuf};

pub fn root_of(path: &Path) -> PathBuf {
    // Never the filesystem root: a Dock launch inherits `/`, and a project root
    // of `/` means grepping every file on the machine and running a task from
    // the top of it. `workable_root` answers with the repositories instead.
    let fallback = crate::workspace::workable_root(
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    );
    let mut dir = if path.as_os_str().is_empty() {
        fallback.clone()
    } else if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| fallback.clone())
    };
    if dir.is_relative() {
        dir = fallback.join(dir);
    }
    loop {
        let marker = dir.join(".git");
        if marker.is_dir() || marker.is_file() {
            return dir;
        }
        if !dir.pop() {
            return fallback;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("nvimglsl-project-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn finds_git_directory_while_walking_up() {
        let dir = temp("dir");
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::create_dir_all(dir.join("src/deep")).unwrap();
        assert_eq!(root_of(&dir.join("src/deep/file.rs")), dir);
    }

    #[test]
    fn finds_git_file_for_a_linked_worktree() {
        let dir = temp("file");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join(".git"), "gitdir: elsewhere\n").unwrap();
        assert_eq!(root_of(&dir.join("src/lib.rs")), dir);
    }
}
