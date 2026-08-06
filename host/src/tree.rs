//! Pure file tree model.
//!
//! The model expands lazily: a directory is read only when it becomes visible
//! through opening or reveal. The view layer decides where these rows are
//! painted.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    Dir,
    Note,
    File,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeRow {
    pub path: PathBuf,
    pub depth: usize,
    pub kind: RowKind,
    pub open: bool,
}

#[derive(Clone, Debug)]
struct Node {
    kind: RowKind,
    open: bool,
    loaded: bool,
    children: Vec<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct FileTree {
    root: PathBuf,
    nodes: BTreeMap<PathBuf, Node>,
    rows: Vec<TreeRow>,
    selected: usize,
}

impl FileTree {
    pub fn new(root: impl Into<PathBuf>, ignore: impl Fn(&Path, bool) -> bool) -> Self {
        let root = normalize(root.into());
        let mut tree = Self {
            root: root.clone(),
            nodes: BTreeMap::new(),
            rows: Vec::new(),
            selected: 0,
        };
        tree.nodes.insert(
            root.clone(),
            Node {
                kind: RowKind::Dir,
                open: true,
                loaded: false,
                children: Vec::new(),
            },
        );
        tree.load_dir(&root, &ignore);
        tree.rebuild_rows();
        tree
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn rows(&self) -> &[TreeRow] {
        &self.rows
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn selected_path(&self) -> Option<&Path> {
        self.rows.get(self.selected).map(|row| row.path.as_path())
    }

    pub fn selected_kind(&self) -> Option<RowKind> {
        self.rows.get(self.selected).map(|row| row.kind)
    }

    pub fn move_selection(&mut self, delta: isize) {
        if self.rows.is_empty() {
            self.selected = 0;
            return;
        }
        let last = self.rows.len() - 1;
        self.selected = if delta < 0 {
            self.selected.saturating_sub(delta.unsigned_abs())
        } else {
            (self.selected + delta as usize).min(last)
        };
    }

    pub fn open_selected(&mut self, ignore: impl Fn(&Path, bool) -> bool) {
        let Some(path) = self.selected_path().map(Path::to_path_buf) else {
            return;
        };
        self.open_dir(&path, ignore);
    }

    pub fn close_selected(&mut self) {
        let Some(path) = self.selected_path().map(Path::to_path_buf) else {
            return;
        };
        if let Some(node) = self.nodes.get_mut(&path) {
            if node.kind == RowKind::Dir && node.open {
                node.open = false;
                self.rebuild_rows();
                self.select_path(&path);
                return;
            }
        }
        if let Some(parent) = path.parent().map(Path::to_path_buf) {
            self.select_path(&parent);
        }
    }

    pub fn toggle_selected(&mut self, ignore: impl Fn(&Path, bool) -> bool) {
        let Some(path) = self.selected_path().map(Path::to_path_buf) else {
            return;
        };
        let Some(node) = self.nodes.get(&path) else {
            return;
        };
        if node.kind != RowKind::Dir {
            return;
        }
        if node.open {
            self.close_selected();
        } else {
            self.open_dir(&path, ignore);
        }
    }

    pub fn reveal(&mut self, path: &Path, ignore: impl Fn(&Path, bool) -> bool) -> bool {
        let target = normalize(path.to_path_buf());
        if !target.starts_with(&self.root) {
            return false;
        }
        let mut ancestors = Vec::new();
        let mut at = target.as_path();
        while let Some(parent) = at.parent() {
            if parent.starts_with(&self.root) {
                ancestors.push(parent.to_path_buf());
            }
            if parent == self.root {
                break;
            }
            at = parent;
        }
        ancestors.reverse();
        for ancestor in ancestors {
            self.open_dir(&ancestor, &ignore);
        }
        self.rebuild_rows();
        self.select_path(&target)
    }

    pub fn reload(&mut self, ignore: impl Fn(&Path, bool) -> bool) {
        let open: Vec<PathBuf> = self
            .nodes
            .iter()
            .filter_map(|(path, node)| node.open.then_some(path.clone()))
            .collect();
        let selected = self.selected_path().map(Path::to_path_buf);
        self.nodes.clear();
        self.nodes.insert(
            self.root.clone(),
            Node {
                kind: RowKind::Dir,
                open: true,
                loaded: false,
                children: Vec::new(),
            },
        );
        for path in open {
            self.open_dir(&path, &ignore);
        }
        self.rebuild_rows();
        if let Some(path) = selected {
            self.select_path(&path);
        }
    }

    fn open_dir(&mut self, path: &Path, ignore: impl Fn(&Path, bool) -> bool) {
        let path = normalize(path.to_path_buf());
        if !self.nodes.contains_key(&path) {
            self.nodes.insert(
                path.clone(),
                Node {
                    kind: RowKind::Dir,
                    open: false,
                    loaded: false,
                    children: Vec::new(),
                },
            );
        }
        if let Some(node) = self.nodes.get_mut(&path) {
            if node.kind != RowKind::Dir {
                return;
            }
            node.open = true;
        }
        self.load_dir(&path, ignore);
        self.rebuild_rows();
        self.select_path(&path);
    }

    fn load_dir(&mut self, path: &Path, ignore: impl Fn(&Path, bool) -> bool) {
        if self.nodes.get(path).is_some_and(|node| node.loaded) {
            return;
        }
        let Ok(entries) = std::fs::read_dir(path) else {
            return;
        };
        let mut children: Vec<((u8, String), PathBuf)> = Vec::new();
        for entry in entries.flatten() {
            let child = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let is_dir = kind.is_dir();
            if ignore(&child, is_dir) {
                continue;
            }
            let row_kind = if is_dir {
                RowKind::Dir
            } else if is_note(&child) {
                RowKind::Note
            } else {
                RowKind::File
            };
            let normalized = normalize(child);
            // The sort key is built once per child, from the kind `read_dir`
            // already reported. Asking the filesystem inside the comparator
            // instead cost one `is_dir` per comparison — about 700,000 stats to
            // order a 23,000-entry directory, which is where 1.12 seconds of
            // opening `/nix/store` went.
            children.push((sort_key_of(row_kind, &normalized), normalized.clone()));
            self.nodes.entry(normalized).or_insert(Node {
                kind: row_kind,
                open: false,
                loaded: !is_dir,
                children: Vec::new(),
            });
        }
        children.sort_by(|a, b| a.0.cmp(&b.0));
        let children: Vec<PathBuf> = children.into_iter().map(|(_, path)| path).collect();
        if let Some(node) = self.nodes.get_mut(path) {
            node.children = children;
            node.loaded = true;
        }
    }

    fn rebuild_rows(&mut self) {
        let mut rows = Vec::new();
        self.push_rows(&self.root, 0, &mut rows);
        self.rows = rows;
        if self.rows.is_empty() {
            self.selected = 0;
        } else {
            self.selected = self.selected.min(self.rows.len() - 1);
        }
    }

    fn push_rows(&self, path: &Path, depth: usize, rows: &mut Vec<TreeRow>) {
        let Some(node) = self.nodes.get(path) else {
            return;
        };
        rows.push(TreeRow {
            path: path.to_path_buf(),
            depth,
            kind: node.kind,
            open: node.open,
        });
        if node.kind == RowKind::Dir && node.open {
            for child in &node.children {
                self.push_rows(child, depth + 1, rows);
            }
        }
    }

    fn select_path(&mut self, path: &Path) -> bool {
        if let Some(index) = self.rows.iter().position(|row| row.path == path) {
            self.selected = index;
            true
        } else {
            false
        }
    }
}

pub fn is_note(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("md" | "markdown")
    )
}

/// How many leading cells of a row's label are indent guides rather than the
/// row itself. The renderer needs this to colour them apart; keeping the count
/// here is what stops the two from disagreeing about where the name starts.
pub fn indent_cells(depth: usize) -> usize {
    depth * 2
}

pub fn row_label(root: &Path, row: &TreeRow) -> String {
    let name = if row.depth == 0 {
        root.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("/")
            .to_string()
    } else {
        row.path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("/")
            .to_string()
    };
    // The twisty gets a column of its own, and files get the same column empty.
    // Putting the marker *in* the name column, as this did, meant a directory's
    // name started two cells right of a file's at the same depth: the one column
    // the eye scans down was the one column that never lined up.
    let twisty = match (row.kind, row.open) {
        (RowKind::Dir, true) => "▾ ",
        (RowKind::Dir, false) => "▸ ",
        (RowKind::Note | RowKind::File, _) => "  ",
    };
    // One guide per level of ancestry, so a name three levels in still says
    // which spine it hangs from once the parent has scrolled off the top.
    format!("{}{}{}", "│ ".repeat(row.depth), twisty, name)
}

/// Directory, then note, then any other file — so a file never sits at the top
/// of the screen where the owner looks first.
fn sort_key_of(kind: RowKind, path: &Path) -> (u8, String) {
    let class = match kind {
        RowKind::Dir => 0,
        RowKind::Note => 1,
        RowKind::File => 2,
    };
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    (class, name)
}

fn normalize(path: PathBuf) -> PathBuf {
    path.canonicalize().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "nvimglsl-tree-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn expands_lazily_and_sorts_dirs_notes_then_files() {
        let root = temp("sort");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("z.rs"), "").unwrap();
        std::fs::write(root.join("a.md"), "").unwrap();
        std::fs::write(root.join("src/main.rs"), "").unwrap();

        let mut tree = FileTree::new(&root, |_, _| false);
        let names: Vec<String> = tree
            .rows()
            .iter()
            .skip(1)
            .map(|row| row.path.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert_eq!(names, vec!["src", "a.md", "z.rs"]);
        assert!(!tree.rows().iter().any(|row| row.path.ends_with("main.rs")));

        tree.move_selection(1);
        tree.open_selected(|_, _| false);
        assert!(tree.rows().iter().any(|row| row.path.ends_with("main.rs")));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn reveal_opens_ancestors_and_selects_the_file() {
        let root = temp("reveal");
        let nested = root.join("a/b");
        std::fs::create_dir_all(&nested).unwrap();
        let note = nested.join("n.md");
        std::fs::write(&note, "").unwrap();

        let mut tree = FileTree::new(&root, |_, _| false);
        assert!(tree.reveal(&note, |_, _| false));
        let note = note.canonicalize().unwrap();
        assert_eq!(tree.selected_path(), Some(note.as_path()));
        assert!(tree.rows().iter().any(|row| row.path == note));
        let _ = std::fs::remove_dir_all(root);
    }
}

#[cfg(test)]
mod expand_cost {
    use super::*;
    use std::time::Instant;

    /// A floor, not a budget. Building the sort key inside the comparator meant
    /// one `is_dir` per comparison — roughly 700,000 stats to order a
    /// 23,000-entry directory — and opening `/nix/store` took 1.12s. Computing
    /// the key once per child brings the same expansion to about 270ms.
    ///
    /// The warm-up is not padding: a cold page cache costs about 550ms on its
    /// own, which would put a fixed threshold on the wrong side of the line
    /// depending only on what the machine happened to have read before.
    #[test]
    fn a_large_directory_expands_without_stat_per_comparison() {
        if !Path::new("/nix/store").is_dir() {
            eprintln!("skip: no /nix/store to measure against");
            return;
        }
        let Some(child) = std::fs::read_dir("/nix/store")
            .ok()
            .and_then(|entries| entries.flatten().next())
            .map(|entry| entry.path())
        else {
            return;
        };
        FileTree::new(Path::new("/"), |_, _| false).reveal(&child, |_, _| false);

        let mut model = FileTree::new(Path::new("/"), |_, _| false);
        let started = Instant::now();
        model.reveal(&child, |_, _| false);
        let elapsed = started.elapsed();
        eprintln!("expand {} rows in {:?}", model.rows().len(), elapsed);
        assert!(
            elapsed.as_millis() < 600,
            "expanding {} rows took {:?}",
            model.rows().len(),
            elapsed
        );
    }

    #[test]
    fn a_directory_comes_before_a_note_which_comes_before_a_file() {
        let dir = std::env::temp_dir().join("nvimglsl-tree-order");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("zzz-dir")).unwrap();
        std::fs::write(dir.join("aaa.txt"), "").unwrap();
        std::fs::write(dir.join("mmm.md"), "").unwrap();

        let model = FileTree::new(&dir, |_, _| false);
        let names: Vec<String> = model
            .rows()
            .iter()
            .skip(1)
            .filter_map(|row| row.path.file_name()?.to_str().map(String::from))
            .collect();
        assert_eq!(names, vec!["zzz-dir", "mmm.md", "aaa.txt"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A directory's name used to start two cells right of a file's at the same
    /// depth, because the marker sat in the name column. The one column the eye
    /// scans down was the one that never lined up.
    #[test]
    fn a_name_starts_at_the_same_column_whatever_the_row_is() {
        let dir = std::env::temp_dir().join("nvimglsl-tree-columns");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("file.rs"), "").unwrap();

        let model = FileTree::new(&dir, |_, _| false);
        let starts: Vec<usize> = model
            .rows()
            .iter()
            .skip(1)
            .map(|row| {
                let label = row_label(model.root(), row);
                label.chars().count()
                    - label
                        .trim_start_matches(['\u{2502}', ' ', '\u{25be}', '\u{25b8}'])
                        .chars()
                        .count()
            })
            .collect();
        assert!(
            starts.windows(2).all(|pair| pair[0] == pair[1]),
            "names start at different columns: {starts:?}"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn indent_cells_matches_the_guides_the_label_actually_carries() {
        let dir = std::env::temp_dir().join("nvimglsl-tree-indent");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("a/b")).unwrap();
        std::fs::write(dir.join("a/b/deep.rs"), "").unwrap();

        let mut model = FileTree::new(&dir, |_, _| false);
        model.reveal(&dir.join("a/b/deep.rs"), |_, _| false);
        for row in model.rows() {
            let label = row_label(model.root(), row);
            let guides: String = label.chars().take(indent_cells(row.depth)).collect();
            assert_eq!(guides, "\u{2502} ".repeat(row.depth), "row {label:?}");
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
