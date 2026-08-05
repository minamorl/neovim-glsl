//! The navigation surface's state and behaviour.
//!
//! `pin navigation_locus_choice` puts the surface outside the terminal grid, in
//! the same window and the same process; `pin navigation_surface_renderer` puts
//! the drawing on the host. The drawing itself is root-ui's
//! ([`crate::root_ui::navigation`]); what lives here is the query, the
//! candidate set and the selection.
//!
//! Who owns that state is `open_question navigation_state_owner`, and the human
//! gate answered 「わからない」. It is held host-side here because something has
//! to hold it for the surface to exist at all; that is an implementation
//! standing inside an open axis, not an answer to it. The seam that keeps the
//! other arrangement reachable is [`Source`]: the surface asks a supplier for
//! candidates and never assumes the supplier is local.

use crate::picker_state::PickerState;

/// Where the candidate list comes from.
///
/// A host-side walker implements this today. A supplier that fetched rows from
/// a Neovim-side plugin would implement the same trait, which is the whole
/// reason it is a trait and not a function call.
pub trait Source {
    fn candidates(&self) -> Vec<String>;
    fn label(&self) -> &str;
}

pub struct Picker {
    state: PickerState,
    visible_rows: usize,
    source: String,
}

/// What the surface asks the host to do when it closes.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Outcome {
    /// Still open; keep drawing it.
    Open,
    Cancelled,
    Chose(String),
}

impl Picker {
    pub fn open(source: &dyn Source, visible_rows: usize) -> Self {
        Self {
            state: PickerState::new(source.candidates()),
            visible_rows: visible_rows.max(1),
            source: source.label().to_string(),
        }
    }

    pub fn query(&self) -> &str {
        self.state.query()
    }

    pub fn matches(&self) -> usize {
        self.state.len()
    }

    pub fn corpus_len(&self) -> usize {
        self.state.corpus_len()
    }

    /// Feed one key. While the surface is open the host owns the keyboard;
    /// `open_question navigation_input_routing` is why that is stated here
    /// rather than assumed: the keys do not reach the editing core, and nothing
    /// in this file claims that is the only possible arrangement.
    pub fn feed(&mut self, keys: &str) -> Outcome {
        for key in crate::core::key::parse(keys) {
            use crate::core::key::{Code, Named};
            match key.code {
                Code::Named(Named::Esc) => return Outcome::Cancelled,
                Code::Named(Named::Enter) => {
                    return match self.state.selection() {
                        Some(choice) => Outcome::Chose(choice.to_string()),
                        None => Outcome::Cancelled,
                    }
                }
                Code::Named(Named::Backspace) => {
                    self.state.pop_char();
                }
                Code::Named(Named::Down) => self.state.move_selection(1),
                Code::Named(Named::Up) => self.state.move_selection(-1),
                Code::Char('n') if key.ctrl => self.state.move_selection(1),
                Code::Char('p') if key.ctrl => self.state.move_selection(-1),
                Code::Char('j') if key.ctrl => self.state.move_selection(1),
                Code::Char('k') if key.ctrl => self.state.move_selection(-1),
                Code::Char('c') if key.ctrl => return Outcome::Cancelled,
                _ => {
                    if let Some(ch) = key.as_text() {
                        self.state.push_char(ch);
                    }
                }
            }
        }
        Outcome::Open
    }

    pub fn label(&self) -> &str {
        &self.source
    }

    /// How many rows the surface has room for, which is not how many candidates
    /// survived the query.
    pub fn row_budget(&self) -> usize {
        self.visible_rows
    }

    /// Where the visible window starts in the match list.
    pub fn offset(&self) -> usize {
        self.state
            .selection_row()
            .map(|row| row.saturating_sub(self.visible_rows.saturating_sub(1)))
            .unwrap_or(0)
    }

    /// The visible window onto the candidate list. The match positions travel
    /// with each row, because a surface that cannot show *why* a row matched is
    /// a list rather than a picker.
    pub fn visible(&self) -> Vec<crate::root_ui::navigation::RowInput> {
        self.state
            .rows(self.offset(), self.visible_rows)
            .into_iter()
            .map(|row| crate::root_ui::navigation::RowInput {
                text: row.text,
                positions: row.positions,
                selected: row.selected,
            })
            .collect()
    }
}

/// Candidates walked from a directory: markdown notes first, then everything
/// else. `pin primary_object` makes the note the thing being navigated to, and
/// `pin file_retained_for_programming` is why the files are still in the list.
pub struct TreeSource {
    root: std::path::PathBuf,
    entries: Vec<String>,
}

impl TreeSource {
    pub fn new(root: std::path::PathBuf) -> Self {
        // `..` and `.` have no file name of their own, and a surface labelled
        // "/" tells the reader nothing about where they are.
        let root = root.canonicalize().unwrap_or(root);
        let mut notes = Vec::new();
        let mut files = Vec::new();
        walk(&root, &root, 0, &mut notes, &mut files);
        notes.sort();
        files.sort();
        notes.extend(files);
        Self {
            root,
            entries: notes,
        }
    }
}

impl Source for TreeSource {
    fn candidates(&self) -> Vec<String> {
        self.entries.clone()
    }

    fn label(&self) -> &str {
        self.root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("/")
    }
}

const SKIP: [&str; 6] = [
    ".git",
    "target",
    "node_modules",
    ".venv",
    "dist",
    "__pycache__",
];

fn walk(
    root: &std::path::Path,
    dir: &std::path::Path,
    depth: usize,
    notes: &mut Vec<String>,
    files: &mut Vec<String>,
) {
    if depth > 8 || notes.len() + files.len() > 20_000 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') && name != ".claude" {
            continue;
        }
        if SKIP.contains(&name.as_ref()) {
            continue;
        }
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => walk(root, &path, depth + 1, notes, files),
            Ok(_) => {
                let relative = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .display()
                    .to_string();
                if relative.ends_with(".md") || relative.ends_with(".markdown") {
                    notes.push(relative);
                } else {
                    files.push(relative);
                }
            }
            Err(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixed(Vec<String>);

    impl Source for Fixed {
        fn candidates(&self) -> Vec<String> {
            self.0.clone()
        }
        fn label(&self) -> &str {
            "fixed"
        }
    }

    fn source() -> Fixed {
        Fixed(vec![
            "notes/alpha.md".into(),
            "notes/beta.md".into(),
            "src/main.rs".into(),
            "README.md".into(),
        ])
    }

    #[test]
    fn typing_narrows_and_backspace_widens() {
        let mut picker = Picker::open(&source(), 8);
        assert_eq!(picker.matches(), 4);
        assert_eq!(picker.feed("md"), Outcome::Open);
        assert_eq!(picker.matches(), 3);
        picker.feed("<BS>");
        assert_eq!(picker.matches(), 4);
    }

    #[test]
    fn enter_returns_the_selection_and_escape_returns_nothing() {
        let mut picker = Picker::open(&source(), 8);
        picker.feed("alpha");
        assert_eq!(picker.feed("<CR>"), Outcome::Chose("notes/alpha.md".into()));
        let mut picker = Picker::open(&source(), 8);
        assert_eq!(picker.feed("<Esc>"), Outcome::Cancelled);
    }

    #[test]
    fn control_n_and_p_move_the_selection() {
        let mut picker = Picker::open(&source(), 8);
        let selected = |picker: &Picker| picker.visible().iter().position(|row| row.selected);
        let first = selected(&picker);
        picker.feed("<C-n>");
        assert_ne!(first, selected(&picker));
    }

    #[test]
    fn a_narrowed_row_carries_the_positions_that_matched() {
        let mut picker = Picker::open(&source(), 8);
        picker.feed("alp");
        let rows = picker.visible();
        assert!(
            !rows[0].positions.is_empty(),
            "no match positions reached the surface"
        );
    }

    #[test]
    fn the_visible_window_never_exceeds_the_row_budget() {
        let many: Vec<String> = (0..200).map(|i| format!("file{i}.md")).collect();
        let picker = Picker::open(&Fixed(many), 8);
        assert_eq!(picker.visible().len(), 8);
        assert_eq!(picker.corpus_len(), 200);
    }

    #[test]
    fn narrowing_reports_how_much_of_the_corpus_survived() {
        let mut picker = Picker::open(&source(), 8);
        picker.feed("md");
        assert_eq!((picker.matches(), picker.corpus_len()), (3, 4));
    }

    #[test]
    fn markdown_notes_sort_ahead_of_other_files() {
        let dir = std::env::temp_dir().join("nvimglsl-tree-source");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("zeta.md"), "").unwrap();
        std::fs::write(dir.join("sub/alpha.rs"), "").unwrap();
        let source = TreeSource::new(dir.clone());
        let candidates = source.candidates();
        assert_eq!(candidates.first().map(String::as_str), Some("zeta.md"));
        assert!(candidates.iter().any(|c| c.ends_with("alpha.rs")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
