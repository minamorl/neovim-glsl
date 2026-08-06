//! The editing core of the own host.
//!
//! `pin neovim_glsl.editor_basis_own_host` puts the editing engine here rather
//! than in a Neovim process, and `pin neovim_glsl.keymap_preservation` says the
//! baseline is the keymap the owner already has. So this file is not a place to
//! design a key language: `hjkl`, operators-times-motions, counts, registers and
//! the `:` line are reproduced, not reconsidered.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::buffer::Buffer;
use super::buffers::{BufferId, BufferStore};
use super::command;
use super::key::{Code, Key, Named};
use super::motion::{self, Kind, Motion};
use super::vcs::{VcsRequest, VcsState};
use super::window::{Direction, Rect, Tabs, WindowId, WindowView};
use crate::keymap::{Keymap, Match, Rhs};
use crate::luaconf::NvimConfig;
use crate::tree::FileTree;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Visual {
    Char,
    Line,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Normal,
    Insert,
    Visual(Visual),
    /// The `:`, `/` and `?` lines. The prompt character is kept beside the mode
    /// because the protocol reports it (`cmdline_show` carries `firstc`).
    Cmdline,
}

impl Mode {
    /// The `mode_info_set` / `mode_change` name this mode reports over the
    /// protocol. These are Neovim's names because a UI client keys its cursor
    /// shape off them.
    pub fn protocol_name(self) -> &'static str {
        match self {
            Mode::Normal => "normal",
            Mode::Insert => "insert",
            Mode::Visual(_) => "visual",
            Mode::Cmdline => "cmdline_normal",
        }
    }

    pub fn short_name(self) -> &'static str {
        match self {
            Mode::Normal => "n",
            Mode::Insert => "i",
            Mode::Visual(Visual::Char) => "v",
            Mode::Visual(Visual::Line) => "V",
            Mode::Cmdline => "c",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Operator {
    Delete,
    Change,
    Yank,
    Indent,
    Dedent,
    /// substitute.nvim, which the owner's config maps onto `s` / `ss` / `S`:
    /// replace the span with the unnamed register instead of deleting it.
    Substitute,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Await {
    /// `g` was pressed and the second half of the command has not arrived.
    G,
    Find {
        forward: bool,
        till: bool,
    },
    Replace,
    Z,
    Leader,
    Register,
    Window,
    Bracket {
        forward: bool,
    },
}

#[derive(Default)]
struct Pending {
    count: Option<usize>,
    operator: Option<Operator>,
    operator_count: Option<usize>,
    awaiting: Option<Await>,
    register: Option<char>,
}

impl Pending {
    fn clear(&mut self) {
        *self = Pending::default();
    }

    /// Counts multiply: `2d3w` deletes six words.
    fn total_count(&self) -> usize {
        self.count.unwrap_or(1) * self.operator_count.unwrap_or(1)
    }

    fn has_count(&self) -> bool {
        self.count.is_some() || self.operator_count.is_some()
    }
}

#[derive(Clone, Default)]
pub struct Register {
    pub lines: Vec<String>,
    pub linewise: bool,
}

/// A span of text an operator acts on.
enum Span {
    Lines {
        from: usize,
        to: usize,
    },
    Chars {
        start: (usize, usize),
        end: (usize, usize),
    },
}

/// What the navigation surface should offer.
///
/// `pin primary_object` puts markdown notes first and
/// `pin file_retained_for_programming` keeps files reachable, so the surface
/// has two scopes rather than one list with both mixed together.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scope {
    Notes,
    Files,
}

/// Something the editing core cannot do by itself and hands to the host.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Request {
    /// `<Space>o` — open the navigation surface. The core does not draw it;
    /// `pin navigation_surface_renderer` puts that on the host.
    OpenNavigation(Scope),
    Quit,
    /// A file the host should make current, from `:e`.
    Edit(PathBuf),
    /// A new note by title. The core has no vault; the host does.
    NewNote(String),
    /// Follow the `[[link]]` under the cursor. The core does not read it
    /// either: which text is a link is the note store's rule, not the editor's.
    FollowLink,
    /// The configured completion key asks the LSP layer for candidates. The
    /// core does not know server state or popup placement.
    Completion,
    /// `vim.diagnostic.open_float()` from the owner's config: show the
    /// diagnostic under the cursor, or ask hover as a fallback.
    DiagnosticFloat,
    /// `gd` asks the LSP layer for the definition under the cursor. The core
    /// has no project or server state, so the host owns the jump.
    Definition,
    /// An Ex command a plugin registered. The core does not run plugins; it
    /// only knows which names belong to one, so that a name belonging to
    /// nothing can still report itself as unknown.
    Plugin {
        name: String,
        argument: String,
    },
    /// `<Space>p` — set the buffer as a vertical page. The core does not
    /// typeset it: `crate::tategaki` owns the page and the host owns somewhere
    /// to put it.
    Preview,
    /// Read-only Git/VCS views. The host owns Git and any spawned process.
    Vcs(VcsRequest),
}

pub struct TreePane {
    pub window: WindowId,
    pub buffer: BufferId,
    pub model: FileTree,
    pub pending_delete: Option<PathBuf>,
}

/// The options this editor can act on, read out of the owner's config.
///
/// Derived at load time rather than written down: the values live in
/// `init.lua`, and a copy here would be a second source that drifts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Options {
    pub shiftwidth: usize,
    pub expandtab: bool,
    pub number: bool,
    pub relativenumber: bool,
    pub cursorline: bool,
    pub ignorecase: bool,
    pub smartcase: bool,
    pub hlsearch: bool,
    /// `list` + `listchars`.
    pub list: bool,
    pub listchar_tab: char,
    pub listchar_trail: char,
    /// `clipboard = unnamedplus`: yanks and deletes reach the system
    /// clipboard, and puts come back from it.
    pub clipboard_unnamed: bool,
    pub splitbelow: bool,
    pub splitright: bool,
}

impl Default for Options {
    fn default() -> Self {
        // Vim's defaults, used only when there is no config to read.
        Self {
            shiftwidth: 8,
            expandtab: false,
            number: false,
            relativenumber: false,
            cursorline: false,
            ignorecase: false,
            smartcase: false,
            hlsearch: true,
            list: false,
            listchar_tab: '>',
            listchar_trail: '-',
            clipboard_unnamed: false,
            splitbelow: false,
            splitright: false,
        }
    }
}

impl Options {
    pub fn from_config(config: &NvimConfig) -> Self {
        let fallback = Options::default();
        Self {
            shiftwidth: config.usize_option("shiftwidth", fallback.shiftwidth).max(1),
            expandtab: config.bool_option("expandtab", fallback.expandtab),
            number: config.bool_option("number", fallback.number),
            relativenumber: config.bool_option("relativenumber", fallback.relativenumber),
            cursorline: config.bool_option("cursorline", fallback.cursorline),
            ignorecase: config.bool_option("ignorecase", fallback.ignorecase),
            smartcase: config.bool_option("smartcase", fallback.smartcase),
            hlsearch: config.bool_option("hlsearch", fallback.hlsearch),
            list: config.bool_option("list", fallback.list),
            listchar_tab: config
                .option("listchars")
                .and_then(|set| set.get("tab"))
                .and_then(|text| text.chars().next())
                .unwrap_or(fallback.listchar_tab),
            listchar_trail: config
                .option("listchars")
                .and_then(|set| set.get("trail"))
                .and_then(|text| text.chars().next())
                .unwrap_or(fallback.listchar_trail),
            clipboard_unnamed: config
                .option("clipboard")
                .map(|value| matches!(value, crate::luaconf::Setting::Text(text) if text.contains("unnamed")))
                .unwrap_or(fallback.clipboard_unnamed),
            splitbelow: config.bool_option("splitbelow", fallback.splitbelow),
            splitright: config.bool_option("splitright", fallback.splitright),
        }
    }

    pub fn indent(&self) -> String {
        if self.expandtab {
            " ".repeat(self.shiftwidth)
        } else {
            "\t".to_string()
        }
    }

    /// `smartcase` applies only on top of `ignorecase`, and it is the pattern
    /// that decides: a capital in it was typed on purpose.
    pub fn case_sensitive(&self, pattern: &str) -> bool {
        if !self.ignorecase {
            return true;
        }
        self.smartcase && pattern.chars().any(char::is_uppercase)
    }

    /// The gutter number for `line`, given where the cursor is.
    pub fn line_number(&self, line: usize, cursor_line: usize) -> Option<usize> {
        match (self.number, self.relativenumber) {
            (false, false) => None,
            (true, false) => Some(line + 1),
            (_, true) if line == cursor_line => Some(line + 1),
            (_, true) => Some(line.abs_diff(cursor_line)),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Message {
    pub text: String,
    pub error: bool,
}

pub struct Editor {
    pub buffer: Buffer,
    pub buffers: BufferStore,
    pub tabs: Tabs,
    pub views: std::collections::BTreeMap<WindowId, WindowView>,
    pub cursor: (usize, usize),
    desired_col: usize,
    pub mode: Mode,
    pending: Pending,
    registers: HashMap<char, Register>,
    pub cmdline: String,
    pub cmdline_prefix: char,
    cmdline_cursor: usize,
    pub message: Option<Message>,
    pub diagnostics: Vec<crate::lsp::types::Diagnostic>,
    pub completion: Option<crate::lsp::session::CompletionMenu>,
    pub last_search: Option<String>,
    last_search_forward: bool,
    visual_anchor: (usize, usize),
    pub top_line: usize,
    pub view_rows: usize,
    pub options: Options,
    /// Ex command names that reach a plugin.
    plugin_commands: std::collections::BTreeSet<String>,
    keymap: Keymap,
    /// Keys typed that are still a prefix of some mapping.
    pending_keys: Vec<Key>,
    pub requests: Vec<Request>,
    pub vcs: VcsState,
    /// Keys typed while the host owns input (the navigation surface is open).
    /// The core stops interpreting them; it does not stop existing.
    pub suspended: bool,
    pub tree: Option<TreePane>,
}

impl Default for Editor {
    fn default() -> Self {
        Self::new(Buffer::empty())
    }
}

impl Editor {
    pub fn new(buffer: Buffer) -> Self {
        let buffers = BufferStore::new(buffer.clone());
        let first_window = WindowId(1);
        let mut views = std::collections::BTreeMap::new();
        views.insert(first_window, WindowView::new(first_window, 2, BufferId(1)));
        Self {
            buffer,
            buffers,
            tabs: Tabs::new(first_window),
            views,
            cursor: (0, 0),
            desired_col: 0,
            mode: Mode::Normal,
            pending: Pending::default(),
            registers: HashMap::new(),
            cmdline: String::new(),
            cmdline_prefix: ':',
            cmdline_cursor: 0,
            message: None,
            diagnostics: Vec::new(),
            completion: None,
            last_search: None,
            last_search_forward: true,
            visual_anchor: (0, 0),
            top_line: 0,
            view_rows: 24,
            options: Options::default(),
            plugin_commands: std::collections::BTreeSet::new(),
            keymap: Keymap::default(),
            pending_keys: Vec::new(),
            requests: Vec::new(),
            vcs: VcsState::default(),
            suspended: false,
            tree: None,
        }
    }

    /// An editor that behaves the way the owner's `init.lua` says.
    pub fn with_config(buffer: Buffer, config: &NvimConfig) -> Self {
        let mut editor = Self::new(buffer);
        editor.options = Options::from_config(config);
        editor.keymap = Keymap::from_config(config);
        editor
    }

    pub fn set_plugin_commands(&mut self, names: impl IntoIterator<Item = String>) {
        self.plugin_commands = names.into_iter().collect();
    }

    pub fn is_plugin_command(&self, name: &str) -> bool {
        self.plugin_commands.contains(name)
    }

    pub fn mapping_count(&self) -> usize {
        self.keymap.len()
    }

    pub fn open(&mut self, path: PathBuf) -> std::io::Result<()> {
        self.save_focused_view();
        let id = self.buffers.open_or_reuse(&path)?;
        self.buffers.set_current(id);
        self.buffer = self.buffers.get(id).unwrap().clone();
        self.cursor = (0, 0);
        self.desired_col = 0;
        self.top_line = 0;
        self.mode = Mode::Normal;
        self.vcs.clear();
        if let Some(view) = self.views.get_mut(&self.focus_window()) {
            view.buffer = id;
        }
        self.save_focused_view();
        self.reveal_current_in_tree();
        Ok(())
    }

    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    pub fn buffer_mut(&mut self) -> &mut Buffer {
        &mut self.buffer
    }

    pub fn cursor(&self) -> (usize, usize) {
        self.cursor
    }

    pub fn set_cursor(&mut self, cursor: (usize, usize)) {
        self.cursor = cursor;
        self.desired_col = cursor.1;
        self.clamp_cursor();
        self.save_focused_view();
    }

    pub fn top_line(&self) -> usize {
        self.top_line
    }

    pub fn focus_window(&self) -> WindowId {
        self.tabs.focus()
    }

    pub fn focused_view(&self) -> &WindowView {
        self.views.get(&self.focus_window()).unwrap()
    }

    pub fn visible_views(&self) -> Vec<WindowView> {
        self.tabs
            .current_layout()
            .leaves()
            .into_iter()
            .filter_map(|id| self.views.get(&id).cloned())
            .collect()
    }

    pub fn window_count(&self) -> usize {
        self.tabs.current_layout().leaves().len()
    }

    pub fn set_screen(&mut self, cols: usize, rows: usize) {
        self.view_rows = rows.max(1);
        if let Some(view) = self.views.get_mut(&self.focus_window()) {
            view.cols = cols.max(1);
            view.rows = self.view_rows;
        }
        self.tabs.remember_geometry(
            Rect::new(0, 0, self.view_rows, cols.max(1)),
            self.options.splitbelow,
            self.options.splitright,
        );
        self.scroll_into_view();
        self.save_focused_view();
    }

    pub fn set_layout_screen(&mut self, cols: usize, rows: usize, row_offset: usize) {
        let area = Rect::new(row_offset, 0, rows.max(1), cols.max(1));
        self.tabs
            .remember_geometry(area, self.options.splitbelow, self.options.splitright);
        let rects = self
            .tabs
            .rects(area, self.options.splitbelow, self.options.splitright);
        for (id, rect) in rects.text {
            if let Some(view) = self.views.get_mut(&id) {
                view.cols = rect.cols.max(1);
                view.rows = rect.rows.max(1);
            }
        }
        self.view_rows = self.focused_view().rows.max(1);
        self.scroll_into_view();
        self.save_focused_view();
    }

    pub fn set_view_rows(&mut self, rows: usize) {
        self.set_screen(self.focused_view().cols, rows);
    }

    pub fn visual_range(&self) -> Option<((usize, usize), (usize, usize))> {
        match self.mode {
            Mode::Visual(_) => {
                let (a, b) = (self.visual_anchor, self.cursor);
                Some(if a <= b { (a, b) } else { (b, a) })
            }
            _ => None,
        }
    }

    pub fn visual_kind(&self) -> Option<Visual> {
        match self.mode {
            Mode::Visual(kind) => Some(kind),
            _ => None,
        }
    }

    pub fn register(&self, name: char) -> Option<&Register> {
        self.registers.get(&name)
    }

    pub fn split_window(&mut self, vertical: bool) {
        self.save_focused_view();
        let old_view = self.focused_view().clone();
        let id = if vertical {
            self.tabs.split_vertical(self.options.splitright)
        } else {
            self.tabs.split_horizontal(self.options.splitbelow)
        };
        let mut view = old_view;
        view.id = id;
        view.grid = self.tabs.grid_for_new_window();
        self.views.insert(id, view);
        self.load_focused_view();
    }

    pub fn scratch_split(&mut self, name: impl Into<String>, lines: Vec<String>, vertical: bool) {
        self.split_window(vertical);
        let id = self.buffers.scratch(name);
        if let Some(buffer) = self.buffers.get_mut(id) {
            buffer.set_lines(lines);
        }
        let focus = self.focus_window();
        if let Some(view) = self.views.get_mut(&focus) {
            view.buffer = id;
            view.cursor = (0, 0);
            view.desired_col = 0;
            view.top_line = self.top_line;
        }
        self.buffer = self.buffers.get(id).unwrap().clone();
        self.cursor = (0, 0);
        self.desired_col = 0;
        self.vcs.clear();
        self.save_focused_view();
    }

    pub fn new_window(&mut self) {
        self.split_window(false);
        let id = self.buffers.empty();
        self.switch_focused_to_buffer(id, true);
    }

    pub fn close_window(&mut self) -> bool {
        self.save_focused_view();
        let old = self.focus_window();
        if self.tabs.close().is_some() {
            self.views.remove(&old);
            if self.tree.as_ref().is_some_and(|tree| tree.window == old) {
                self.tree = None;
            }
            self.load_focused_view();
            return true;
        }
        false
    }

    pub fn only_window(&mut self) {
        self.save_focused_view();
        let focus = self.focus_window();
        self.tabs.only();
        self.views.retain(|id, _| *id == focus);
        if self.tree.as_ref().is_some_and(|tree| tree.window != focus) {
            self.tree = None;
        }
        self.load_focused_view();
    }

    pub fn focus_window_dir(&mut self, dir: Direction) -> bool {
        self.save_focused_view();
        let moved = self.tabs.focus_dir(dir).is_some();
        self.load_focused_view();
        moved
    }

    pub fn cycle_window_focus(&mut self) {
        self.save_focused_view();
        self.tabs.cycle_focus();
        self.load_focused_view();
    }

    pub fn next_tab(&mut self) {
        self.save_focused_view();
        self.tabs.next_tab();
        self.load_focused_view();
    }

    pub fn prev_tab(&mut self) {
        self.save_focused_view();
        self.tabs.prev_tab();
        self.load_focused_view();
    }

    pub fn new_tab(&mut self) {
        self.save_focused_view();
        let (cols, rows) = {
            let view = self.focused_view();
            (view.cols, view.rows)
        };
        let id = self.tabs.new_tab();
        let buffer = self.buffers.empty();
        let mut view = WindowView::new(id, self.tabs.grid_for_new_window(), buffer);
        view.cols = cols;
        view.rows = rows;
        self.views.insert(id, view);
        self.switch_focused_to_buffer(buffer, true);
    }

    pub fn close_tab(&mut self) -> bool {
        self.save_focused_view();
        let Some(removed) = self.tabs.close_tab() else {
            return false;
        };
        for id in removed {
            self.views.remove(&id);
        }
        self.load_focused_view();
        true
    }

    pub fn next_buffer(&mut self) -> bool {
        self.save_focused_view();
        let Some(id) = self.buffers.next() else {
            return false;
        };
        self.switch_focused_to_buffer(id, true)
    }

    pub fn prev_buffer(&mut self) -> bool {
        self.save_focused_view();
        let Some(id) = self.buffers.prev() else {
            return false;
        };
        self.switch_focused_to_buffer(id, true)
    }

    pub fn switch_buffer_index(&mut self, index: usize) -> bool {
        self.save_focused_view();
        let Some(id) = self.buffers.by_index(index) else {
            return false;
        };
        self.switch_focused_to_buffer(id, true)
    }

    pub fn delete_current_buffer(&mut self) -> bool {
        self.save_focused_view();
        let deleting = self.buffers.current_id();
        if !self.buffers.delete(deleting) {
            return false;
        }
        let replacement = self.buffers.current_id();
        for view in self.views.values_mut() {
            if view.buffer == deleting {
                view.buffer = replacement;
                view.cursor = (0, 0);
                view.desired_col = 0;
                view.top_line = 0;
            }
        }
        self.load_focused_view();
        true
    }

    pub fn buffer_list_message(&self) -> String {
        let current = self.buffers.current_id();
        self.buffers
            .list()
            .into_iter()
            .enumerate()
            .filter_map(|(index, id)| {
                let entry = self.buffers.entry(id)?;
                let marker = if id == current { "%" } else { " " };
                Some(format!("{:>3}{marker} {}", index + 1, entry.buffer.name()))
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn open_file_tree_from_file_browser(&mut self, command: &str) {
        let current = self.buffer.path().map(PathBuf::from);
        let root = if command.contains("path=%:p:h") {
            current
                .as_ref()
                .and_then(|path| path.parent().map(PathBuf::from))
                .filter(|path| !path.as_os_str().is_empty())
        } else {
            None
        }
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let root = crate::workspace::workable_root(root);
        let reveal = command
            .contains("select_buffer=true")
            .then_some(current)
            .flatten();
        self.open_file_tree(root, reveal);
    }

    pub fn open_file_tree(&mut self, root: PathBuf, reveal: Option<PathBuf>) {
        self.save_focused_view();
        let root = root.canonicalize().unwrap_or(root);
        let rules = crate::ignore::IgnoreRules::load(&root);
        let mut model = FileTree::new(&root, |path, is_dir| rules.ignored(path, is_dir));
        if let Some(path) = reveal.as_deref() {
            model.reveal(path, |path, is_dir| rules.ignored(path, is_dir));
        }

        let (window, buffer) = match self.tree.as_ref() {
            Some(tree) if self.views.contains_key(&tree.window) => (tree.window, tree.buffer),
            _ => {
                let old = self.focused_view().clone();
                let window = self.tabs.split_vertical_before();
                let buffer = self.buffers.scratch("file-tree");
                let mut view = old;
                view.id = window;
                view.grid = self.tabs.grid_for_new_window();
                view.buffer = buffer;
                view.cursor = (0, 0);
                view.desired_col = 0;
                view.top_line = 0;
                self.views.insert(window, view);
                (window, buffer)
            }
        };
        self.tree = Some(TreePane {
            window,
            buffer,
            model,
            pending_delete: None,
        });
        self.sync_tree_buffer();
        self.tabs.focus_window(window);
        self.load_focused_view();
        self.scroll_tree_into_view();
    }

    pub fn focused_tree(&self) -> Option<&TreePane> {
        self.tree
            .as_ref()
            .filter(|tree| tree.window == self.focus_window())
    }

    fn focused_tree_mut(&mut self) -> Option<&mut TreePane> {
        let focus = self.focus_window();
        self.tree.as_mut().filter(|tree| tree.window == focus)
    }

    pub fn tree_create(&mut self, name: &str) {
        let Some(base) = self.tree_selected_base() else {
            return;
        };
        let name = name.trim();
        if name.is_empty() {
            self.message = Some(Message {
                text: "E471: Argument required: :TreeNew <name>".into(),
                error: true,
            });
            return;
        }
        let path = base.join(name);
        let result = if name.ends_with('/') {
            std::fs::create_dir_all(&path)
        } else {
            std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path)
                .map(|_| ())
        };
        match result {
            Ok(()) => {
                self.refresh_tree(Some(path));
            }
            Err(error) => {
                self.message = Some(Message {
                    text: format!("E212: can't create {}: {error}", path.display()),
                    error: true,
                });
            }
        }
    }

    pub fn tree_rename(&mut self, name: &str) {
        let Some(from) = self
            .focused_tree()
            .and_then(|tree| tree.model.selected_path())
        else {
            return;
        };
        let from = from.to_path_buf();
        let name = name.trim();
        if name.is_empty() {
            self.message = Some(Message {
                text: "E471: Argument required: :TreeRename <name>".into(),
                error: true,
            });
            return;
        }
        let Some(parent) = from.parent() else {
            return;
        };
        let to = parent.join(name);
        match std::fs::rename(&from, &to) {
            Ok(()) => self.refresh_tree(Some(to)),
            Err(error) => {
                self.message = Some(Message {
                    text: format!("E13: can't rename {}: {error}", from.display()),
                    error: true,
                });
            }
        }
    }

    fn feed_tree(&mut self, key: Key) -> bool {
        if key.ctrl {
            return false;
        }
        if let Code::Char(ch) = key.code {
            if ch.is_ascii_digit() && !(ch == '0' && self.pending.count.is_none()) {
                let digit = ch.to_digit(10).unwrap() as usize;
                self.pending.count = Some(self.pending.count.unwrap_or(0) * 10 + digit);
                return true;
            }
        }
        match key.code {
            Code::Named(Named::Down) | Code::Char('j') => self.tree_move(1),
            Code::Named(Named::Up) | Code::Char('k') => self.tree_move(-1),
            Code::Named(Named::Enter) | Code::Char('o') => self.tree_open_selected(),
            Code::Char('l') => {
                let rules = self.tree_ignore_rules();
                if let Some(tree) = self.focused_tree_mut() {
                    tree.model
                        .open_selected(|path, is_dir| rules.ignored(path, is_dir));
                }
                self.sync_tree_buffer();
                self.scroll_tree_into_view();
            }
            Code::Char('h') => {
                if let Some(tree) = self.focused_tree_mut() {
                    tree.model.close_selected();
                }
                self.sync_tree_buffer();
                self.scroll_tree_into_view();
            }
            Code::Char('a') => self.start_tree_command("TreeNew "),
            Code::Char('r') => {
                let name = self
                    .focused_tree()
                    .and_then(|tree| tree.model.selected_path())
                    .and_then(|path| path.file_name())
                    .and_then(|name| name.to_str())
                    .unwrap_or("");
                self.start_tree_command(&format!("TreeRename {name}"));
            }
            Code::Char('d') => self.tree_delete_key(),
            Code::Char('R') => self.refresh_tree(None),
            _ => return false,
        }
        true
    }

    fn tree_move(&mut self, delta: isize) {
        let count = self.pending.count.take().unwrap_or(1).max(1) as isize;
        let delta = delta * count;
        if let Some(tree) = self.focused_tree_mut() {
            tree.pending_delete = None;
            tree.model.move_selection(delta);
        }
        self.scroll_tree_into_view();
    }

    fn tree_open_selected(&mut self) {
        let Some(kind) = self
            .focused_tree()
            .and_then(|tree| tree.model.selected_kind())
        else {
            return;
        };
        if kind == crate::tree::RowKind::Dir {
            let rules = self.tree_ignore_rules();
            if let Some(tree) = self.focused_tree_mut() {
                tree.model
                    .toggle_selected(|path, is_dir| rules.ignored(path, is_dir));
            }
            self.sync_tree_buffer();
            self.scroll_tree_into_view();
            return;
        }
        let Some(path) = self
            .focused_tree()
            .and_then(|tree| tree.model.selected_path())
            .map(PathBuf::from)
        else {
            return;
        };
        let Some(target) = self.first_non_tree_window() else {
            return;
        };
        self.save_focused_view();
        self.tabs.focus_window(target);
        self.load_focused_view();
        if let Err(error) = self.open(path.clone()) {
            self.report_open_failure(&path, &error);
        }
    }

    /// One wording for a failed open, wherever the attempt came from.
    pub fn report_open_failure(&mut self, path: &Path, error: &std::io::Error) {
        self.message = Some(Message {
            text: format!("E484: Can't open file {}: {error}", path.display()),
            error: true,
        });
    }

    fn tree_delete_key(&mut self) {
        let Some(path) = self
            .focused_tree()
            .and_then(|tree| tree.model.selected_path())
            .map(PathBuf::from)
        else {
            return;
        };
        let already_confirmed = self
            .focused_tree()
            .and_then(|tree| tree.pending_delete.as_ref())
            == Some(&path);
        if !already_confirmed {
            if let Some(tree) = self.focused_tree_mut() {
                tree.pending_delete = Some(path.clone());
            }
            self.message = Some(Message {
                text: format!("delete {}? press d again to confirm", path.display()),
                error: false,
            });
            return;
        }

        let is_dir = path.is_dir();
        let result = if is_dir {
            match std::fs::read_dir(&path) {
                Ok(mut entries) => {
                    if entries.next().is_none() {
                        std::fs::remove_dir(&path)
                    } else {
                        Err(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            "recursive delete is refused",
                        ))
                    }
                }
                Err(error) => Err(error),
            }
        } else {
            std::fs::remove_file(&path)
        };
        match result {
            Ok(()) => {
                let parent = path.parent().map(PathBuf::from);
                self.refresh_tree(parent);
            }
            Err(error) => {
                if let Some(tree) = self.focused_tree_mut() {
                    tree.pending_delete = None;
                }
                self.message = Some(Message {
                    text: format!("E13: can't delete {}: {error}", path.display()),
                    error: true,
                });
            }
        }
    }

    fn start_tree_command(&mut self, command: &str) {
        self.cmdline_prefix = ':';
        self.cmdline = command.to_string();
        self.cmdline_cursor = self.cmdline.chars().count();
        self.mode = Mode::Cmdline;
    }

    fn tree_selected_base(&self) -> Option<PathBuf> {
        let path = self.focused_tree()?.model.selected_path()?;
        if path.is_dir() {
            Some(path.to_path_buf())
        } else {
            path.parent().map(PathBuf::from)
        }
    }

    fn refresh_tree(&mut self, reveal: Option<PathBuf>) {
        let rules = self.tree_ignore_rules();
        if let Some(tree) = self.tree.as_mut() {
            tree.pending_delete = None;
            tree.model
                .reload(|path, is_dir| rules.ignored(path, is_dir));
            if let Some(path) = reveal.as_deref() {
                tree.model
                    .reveal(path, |path, is_dir| rules.ignored(path, is_dir));
            }
        }
        self.sync_tree_buffer();
        self.scroll_tree_into_view();
    }

    fn reveal_current_in_tree(&mut self) {
        let Some(path) = self.buffer.path().map(PathBuf::from) else {
            return;
        };
        let rules = self.tree_ignore_rules();
        if let Some(tree) = self.tree.as_mut() {
            if path.starts_with(tree.model.root()) {
                tree.model
                    .reveal(&path, |path, is_dir| rules.ignored(path, is_dir));
            }
        }
        self.sync_tree_buffer();
        self.scroll_tree_into_view();
    }

    fn sync_tree_buffer(&mut self) {
        let Some(tree) = self.tree.as_ref() else {
            return;
        };
        let lines: Vec<String> = tree
            .model
            .rows()
            .iter()
            .map(|row| crate::tree::row_label(tree.model.root(), row))
            .collect();
        if let Some(buffer) = self.buffers.get_mut(tree.buffer) {
            buffer.set_lines(lines);
        }
    }

    fn scroll_tree_into_view(&mut self) {
        let Some(tree) = self.tree.as_ref() else {
            return;
        };
        let selected = tree.model.selected();
        if let Some(view) = self.views.get_mut(&tree.window) {
            if selected < view.top_line {
                view.top_line = selected;
            } else if selected >= view.top_line + view.rows.max(1) {
                view.top_line = selected + 1 - view.rows.max(1);
            }
            view.cursor = (selected, 0);
        }
    }

    fn tree_ignore_rules(&self) -> crate::ignore::IgnoreRules {
        let root = self
            .tree
            .as_ref()
            .map(|tree| tree.model.root().to_path_buf())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        crate::ignore::IgnoreRules::load(root)
    }

    fn first_non_tree_window(&self) -> Option<WindowId> {
        let tree_window = self.tree.as_ref().map(|tree| tree.window);
        self.tabs
            .current_layout()
            .leaves()
            .into_iter()
            .find(|id| Some(*id) != tree_window)
    }

    // --- input ------------------------------------------------------------

    pub fn feed_str(&mut self, input: &str) {
        for key in super::key::parse(input) {
            self.feed(key);
        }
    }

    pub fn feed(&mut self, key: Key) {
        if self.suspended {
            return;
        }
        self.feed_mapped(key, 0);
        self.scroll_into_view();
        self.save_focused_view();
    }

    /// Resolve `key` against the owner's mappings, then act.
    ///
    /// `depth` stops a mapping whose right-hand side reaches its own left-hand
    /// side from recursing forever.
    fn feed_mapped(&mut self, key: Key, depth: usize) {
        // A key that is an *argument* is not a mapping: after `f`, the next key
        // names the character to find, and after `"` it names a register.
        if self.pending.awaiting.is_some() || depth > 16 || self.mode == Mode::Cmdline {
            self.dispatch(key);
            return;
        }
        let mode = self.keymap_mode();
        self.pending_keys.push(key);
        match self.keymap.lookup(mode, &self.pending_keys) {
            Match::Prefix => {}
            Match::Exact(rhs) => {
                let rhs = rhs.clone();
                self.pending_keys.clear();
                self.run(&rhs, depth);
            }
            Match::None => {
                let keys = std::mem::take(&mut self.pending_keys);
                // The longest prefix that *is* a mapping still fires: `s` is
                // mapped and `sx` is not, so `s` runs its mapping and `x`
                // becomes the next command — vim's timeout, resolved by the
                // next key instead of by a clock.
                let split = (1..=keys.len())
                    .rev()
                    .find(|&n| matches!(self.keymap.lookup(mode, &keys[..n]), Match::Exact(_)))
                    .unwrap_or(0);
                if split > 0 {
                    if let Match::Exact(rhs) = self.keymap.lookup(mode, &keys[..split]) {
                        let rhs = rhs.clone();
                        self.run(&rhs, depth);
                    }
                } else {
                    self.dispatch(keys[0]);
                }
                for key in &keys[split.max(1)..] {
                    self.feed_mapped(*key, depth);
                }
            }
        }
    }

    fn keymap_mode(&self) -> char {
        match self.mode {
            Mode::Normal => 'n',
            Mode::Insert => 'i',
            Mode::Visual(_) => 'v',
            Mode::Cmdline => 'c',
        }
    }

    fn dispatch(&mut self, key: Key) {
        if self.mode == Mode::Normal && self.focused_tree().is_some() && self.feed_tree(key) {
            return;
        }
        match self.mode {
            Mode::Insert => self.feed_insert(key),
            Mode::Cmdline => self.feed_cmdline(key),
            Mode::Normal | Mode::Visual(_) => self.feed_normal(key),
        }
    }

    fn run(&mut self, rhs: &Rhs, depth: usize) {
        match rhs {
            Rhs::Nothing => {}
            Rhs::Keys(keys) => {
                for key in keys {
                    self.feed_mapped(*key, depth + 1);
                }
            }
            Rhs::Command(command) => self.run_command(command),
        }
    }

    /// Carry out a mapped command, or say plainly that it cannot be.
    ///
    /// The alternative — ignoring it — makes a key that used to do something
    /// look broken rather than unimplemented.
    fn run_command(&mut self, command: &str) {
        let command = command.trim();
        if let Some(call) = command.strip_prefix("lua ") {
            return self.run_lua_command(call.trim());
        }
        match command {
            "Telescope find_files" | "FzfLua files" | "Telescope git_files" => {
                self.requests.push(Request::OpenNavigation(Scope::Files))
            }
            _ if command.starts_with("Telescope file_browser") => {
                self.open_file_tree_from_file_browser(command)
            }
            "Telescope buffers" | "Telescope oldfiles" => {
                self.requests.push(Request::OpenNavigation(Scope::Notes))
            }
            _ => {
                // Anything else is an Ex command; the ones this editor knows
                // are handled there, and the rest report themselves.
                let before = self.requests.len();
                let had_message = self.message.is_some();
                command::execute(self, command);
                let unhandled = self.requests.len() == before
                    && self
                        .message
                        .as_ref()
                        .is_some_and(|m| m.error && m.text.starts_with("E492"));
                if unhandled {
                    self.message = Some(Message {
                        text: format!("{command}: mapped, but this host has no such command"),
                        error: true,
                    });
                } else if !had_message && self.message.is_none() {
                    // handled quietly
                }
            }
        }
    }

    /// The `lua require("substitute").…()` family the owner maps onto s/ss/S.
    fn run_lua_command(&mut self, call: &str) {
        if !call.contains("substitute") {
            if call.contains("vim.diagnostic.open_float") {
                self.requests.push(Request::DiagnosticFloat);
                return;
            }
            self.message = Some(Message {
                text: format!("{call}: no Lua runtime in the editing path"),
                error: true,
            });
            return;
        }
        let (line, col) = self.cursor;
        if call.contains(".line()") {
            self.apply_operator(
                Operator::Substitute,
                Span::Lines {
                    from: line,
                    to: line,
                },
            );
        } else if call.contains(".eol()") {
            let end = (line, self.buffer.line_len(line));
            self.apply_operator(
                Operator::Substitute,
                Span::Chars {
                    start: (line, col),
                    end,
                },
            );
        } else if call.contains(".visual()") {
            if let Some(kind) = self.visual_kind() {
                let span = self.visual_span(kind);
                self.apply_operator(Operator::Substitute, span);
                self.mode = Mode::Normal;
            }
        } else if call.contains(".operator()") {
            self.pending.operator = Some(Operator::Substitute);
        }
    }

    fn feed_insert(&mut self, key: Key) {
        match key.code {
            Code::Named(Named::Esc) => {
                self.buffer.commit_change();
                self.mode = Mode::Normal;
                self.cursor.1 = self.cursor.1.saturating_sub(1);
                self.clamp_cursor();
            }
            Code::Named(Named::Enter) => {
                self.buffer.split_line(self.cursor.0, self.cursor.1);
                self.cursor = (self.cursor.0 + 1, 0);
            }
            Code::Named(Named::Backspace) => {
                if self.cursor.1 > 0 {
                    self.buffer.delete_range_in_line(
                        self.cursor.0,
                        self.cursor.1 - 1,
                        self.cursor.1,
                    );
                    self.cursor.1 -= 1;
                } else if self.cursor.0 > 0 {
                    let at = self.buffer.line_len(self.cursor.0 - 1);
                    self.buffer.join_lines(self.cursor.0 - 1, false);
                    self.cursor = (self.cursor.0 - 1, at);
                }
            }
            Code::Named(Named::Delete) => {
                self.buffer
                    .delete_range_in_line(self.cursor.0, self.cursor.1, self.cursor.1 + 1);
            }
            Code::Named(Named::Tab) => {
                let indent = self.options.indent();
                self.buffer
                    .insert_str(self.cursor.0, self.cursor.1, &indent);
                self.cursor.1 += indent.chars().count();
            }
            Code::Named(Named::Left) => self.move_by(Motion::Left, 1),
            Code::Named(Named::Right) => self.move_by(Motion::Right, 1),
            Code::Named(Named::Up) => self.move_by(Motion::Up, 1),
            Code::Named(Named::Down) => self.move_by(Motion::Down, 1),
            Code::Named(Named::Home) => self.cursor.1 = 0,
            Code::Named(Named::End) => self.cursor.1 = self.buffer.line_len(self.cursor.0),
            Code::Char('w') if key.ctrl => {
                let target = motion::apply(
                    &self.buffer,
                    self.cursor,
                    self.desired_col,
                    Motion::WordBack { big: false },
                    1,
                );
                if target.0 == self.cursor.0 {
                    self.buffer
                        .delete_range_in_line(self.cursor.0, target.1, self.cursor.1);
                    self.cursor.1 = target.1;
                }
            }
            Code::Char('u') if key.ctrl => {
                self.buffer
                    .delete_range_in_line(self.cursor.0, 0, self.cursor.1);
                self.cursor.1 = 0;
            }
            Code::Char(' ') if key.ctrl => self.requests.push(Request::Completion),
            _ => {
                if let Some(ch) = key.as_text() {
                    self.buffer.insert_char(self.cursor.0, self.cursor.1, ch);
                    self.cursor.1 += 1;
                }
            }
        }
        self.desired_col = self.cursor.1;
    }

    fn feed_cmdline(&mut self, key: Key) {
        match key.code {
            Code::Named(Named::Esc) => {
                self.mode = Mode::Normal;
                self.cmdline.clear();
                self.cmdline_cursor = 0;
            }
            Code::Named(Named::Enter) => {
                let line = std::mem::take(&mut self.cmdline);
                let prefix = self.cmdline_prefix;
                self.cmdline_cursor = 0;
                self.mode = Mode::Normal;
                match prefix {
                    ':' => command::execute(self, &line),
                    '/' | '?' => {
                        self.last_search_forward = prefix == '/';
                        if !line.is_empty() {
                            self.last_search = Some(line);
                        }
                        self.search_next(true);
                    }
                    _ => {}
                }
            }
            Code::Named(Named::Backspace) => {
                if self.cmdline_cursor > 0 {
                    let index = self.cmdline_cursor - 1;
                    let byte = char_boundary(&self.cmdline, index);
                    self.cmdline.remove(byte);
                    self.cmdline_cursor -= 1;
                } else {
                    self.mode = Mode::Normal;
                }
            }
            Code::Named(Named::Left) => self.cmdline_cursor = self.cmdline_cursor.saturating_sub(1),
            Code::Named(Named::Right) => {
                self.cmdline_cursor = (self.cmdline_cursor + 1).min(self.cmdline.chars().count())
            }
            Code::Char('u') if key.ctrl => {
                self.cmdline.clear();
                self.cmdline_cursor = 0;
            }
            _ => {
                if let Some(ch) = key.as_text() {
                    let byte = char_boundary(&self.cmdline, self.cmdline_cursor);
                    self.cmdline.insert(byte, ch);
                    self.cmdline_cursor += 1;
                }
            }
        }
    }

    fn feed_normal(&mut self, key: Key) {
        if let Some(awaiting) = self.pending.awaiting.take() {
            self.resolve_await(awaiting, key);
            return;
        }

        // A leading `0` is a motion; a `0` after another digit is a count.
        if let Code::Char(ch) = key.code {
            if !key.ctrl && ch.is_ascii_digit() && !(ch == '0' && !self.pending.has_count()) {
                let digit = ch.to_digit(10).unwrap() as usize;
                let slot = if self.pending.operator.is_some() {
                    &mut self.pending.operator_count
                } else {
                    &mut self.pending.count
                };
                *slot = Some(slot.unwrap_or(0) * 10 + digit);
                return;
            }
        }

        if let Some(motion) = self.simple_motion(key) {
            self.consume_motion(motion);
            return;
        }

        let count = self.pending.total_count();
        match key.code {
            Code::Named(Named::Esc) => {
                self.pending.clear();
                self.mode = Mode::Normal;
            }
            Code::Named(Named::Enter) => {
                self.pending.clear();
                self.move_by(Motion::Down, count);
                self.cursor.1 = 0;
            }
            Code::Char(ch) if key.ctrl => self.ctrl_key(ch, count),
            Code::Char(ch) => self.normal_char(ch, count),
            _ => self.pending.clear(),
        }
    }

    fn simple_motion(&self, key: Key) -> Option<Motion> {
        if key.ctrl {
            return None;
        }
        Some(match key.code {
            Code::Char('h') => Motion::Left,
            Code::Char('l') => Motion::Right,
            Code::Char('j') => Motion::Down,
            Code::Char('k') => Motion::Up,
            Code::Char('0') => Motion::LineStart,
            Code::Char('^') => Motion::FirstNonBlank,
            Code::Char('$') => Motion::LineEnd,
            Code::Char('w') => Motion::WordForward { big: false },
            Code::Char('W') => Motion::WordForward { big: true },
            Code::Char('b') => Motion::WordBack { big: false },
            Code::Char('B') => Motion::WordBack { big: true },
            Code::Char('e') => Motion::WordEnd { big: false },
            Code::Char('E') => Motion::WordEnd { big: true },
            Code::Char('{') => Motion::ParagraphBack,
            Code::Char('}') => Motion::ParagraphForward,
            Code::Char('%') => Motion::MatchPair,
            Code::Char('G') => {
                return Some(match self.pending.count.or(self.pending.operator_count) {
                    Some(line) => Motion::GotoLine(line.saturating_sub(1)),
                    None => Motion::FileEnd,
                })
            }
            Code::Named(Named::Left) => Motion::Left,
            Code::Named(Named::Right) => Motion::Right,
            Code::Named(Named::Up) => Motion::Up,
            Code::Named(Named::Down) => Motion::Down,
            Code::Named(Named::Home) => Motion::LineStart,
            Code::Named(Named::End) => Motion::LineEnd,
            _ => return None,
        })
    }

    fn resolve_await(&mut self, awaiting: Await, key: Key) {
        let count = self.pending.total_count();
        match awaiting {
            Await::G => match key.code {
                Code::Char('f') => {
                    self.pending.clear();
                    self.requests.push(Request::FollowLink);
                }
                Code::Char('d') => {
                    self.pending.clear();
                    self.requests.push(Request::Definition);
                }
                Code::Char('g') => {
                    let motion = match self.pending.count.or(self.pending.operator_count) {
                        Some(line) => Motion::GotoLine(line.saturating_sub(1)),
                        None => Motion::FileStart,
                    };
                    self.consume_motion(motion);
                }
                Code::Char('t') => {
                    self.save_focused_view();
                    self.tabs.next_tab();
                    self.load_focused_view();
                    self.pending.clear();
                }
                Code::Char('T') => {
                    self.save_focused_view();
                    self.tabs.prev_tab();
                    self.load_focused_view();
                    self.pending.clear();
                }
                _ => self.pending.clear(),
            },
            Await::Find { forward, till } => match key.as_text() {
                Some(ch) => self.consume_motion(Motion::FindChar { ch, forward, till }),
                None => self.pending.clear(),
            },
            Await::Replace => {
                if let Some(ch) = key.as_text() {
                    let (line, col) = self.cursor;
                    if col < self.buffer.line_len(line) {
                        self.buffer.begin_change(self.cursor);
                        self.buffer.delete_range_in_line(
                            line,
                            col,
                            col + count.min(self.buffer.line_len(line) - col),
                        );
                        let text: String = std::iter::repeat(ch).take(count).collect();
                        self.buffer.insert_str(line, col, &text);
                        self.buffer.commit_change();
                        self.cursor.1 = col + count - 1;
                    }
                }
                self.pending.clear();
            }
            Await::Z => {
                match key.code {
                    Code::Char('z') => {
                        self.top_line = self.cursor.0.saturating_sub(self.view_rows / 2)
                    }
                    Code::Char('t') => self.top_line = self.cursor.0,
                    Code::Char('b') => {
                        self.top_line = self
                            .cursor
                            .0
                            .saturating_sub(self.view_rows.saturating_sub(1))
                    }
                    _ => {}
                }
                self.pending.clear();
            }
            Await::Leader => {
                match key.code {
                    // The owner's own mapping for the picker opens the primary
                    // object; `f` is the escape hatch to files.
                    Code::Char('o') | Code::Char('n') => {
                        self.requests.push(Request::OpenNavigation(Scope::Notes))
                    }
                    Code::Char('f') => self.requests.push(Request::OpenNavigation(Scope::Files)),
                    Code::Char('p') => self.requests.push(Request::Preview),
                    _ => {}
                }
                self.pending.clear();
            }
            Await::Register => {
                if let Some(ch) = key.as_text() {
                    self.pending.register = Some(ch);
                } else {
                    self.pending.clear();
                }
            }
            Await::Window => match key.code {
                Code::Char(ch) => self.window_command(ch),
                _ => self.pending.clear(),
            },
            Await::Bracket { forward } => match key.code {
                Code::Char('c') => {
                    if let Some(line) = self.vcs.next_hunk(self.cursor.0, forward) {
                        self.cursor = (line.min(self.buffer.line_count() - 1), 0);
                        self.desired_col = 0;
                        self.scroll_into_view();
                    } else {
                        self.message = Some(Message {
                            text: "E787: No git hunk".into(),
                            error: true,
                        });
                    }
                    self.pending.clear();
                }
                _ => self.pending.clear(),
            },
        }
    }

    fn ctrl_key(&mut self, ch: char, _count: usize) {
        self.pending.clear();
        let half = (self.view_rows / 2).max(1);
        match ch {
            'd' => {
                self.move_by(Motion::Down, half);
                self.top_line = (self.top_line + half).min(self.max_top_line());
            }
            'u' => {
                self.move_by(Motion::Up, half);
                self.top_line = self.top_line.saturating_sub(half);
            }
            'f' => {
                self.move_by(Motion::Down, self.view_rows.saturating_sub(2).max(1));
                self.top_line =
                    (self.top_line + self.view_rows.saturating_sub(2)).min(self.max_top_line());
            }
            'b' => {
                self.move_by(Motion::Up, self.view_rows.saturating_sub(2).max(1));
                self.top_line = self
                    .top_line
                    .saturating_sub(self.view_rows.saturating_sub(2));
            }
            'r' => {
                if let Some(cursor) = self.buffer.redo(self.cursor) {
                    self.cursor = cursor;
                    self.clamp_cursor();
                } else {
                    self.message = Some(Message {
                        text: "Already at newest change".into(),
                        error: false,
                    });
                }
            }
            'e' => self.top_line = (self.top_line + 1).min(self.max_top_line()),
            'y' => self.top_line = self.top_line.saturating_sub(1),
            'w' => self.pending.awaiting = Some(Await::Window),
            _ => {}
        }
    }

    fn normal_char(&mut self, ch: char, count: usize) {
        match ch {
            'g' => {
                self.pending.awaiting = Some(Await::G);
                return;
            }
            'z' => {
                self.pending.awaiting = Some(Await::Z);
                return;
            }
            '"' => {
                self.pending.awaiting = Some(Await::Register);
                return;
            }
            ' ' => {
                self.pending.awaiting = Some(Await::Leader);
                return;
            }
            'f' => {
                self.pending.awaiting = Some(Await::Find {
                    forward: true,
                    till: false,
                });
                return;
            }
            'F' => {
                self.pending.awaiting = Some(Await::Find {
                    forward: false,
                    till: false,
                });
                return;
            }
            't' => {
                self.pending.awaiting = Some(Await::Find {
                    forward: true,
                    till: true,
                });
                return;
            }
            'T' => {
                self.pending.awaiting = Some(Await::Find {
                    forward: false,
                    till: true,
                });
                return;
            }
            'r' => {
                self.pending.awaiting = Some(Await::Replace);
                return;
            }
            ']' => {
                self.pending.awaiting = Some(Await::Bracket { forward: true });
                return;
            }
            '[' => {
                self.pending.awaiting = Some(Await::Bracket { forward: false });
                return;
            }
            _ => {}
        }

        // Operators. A doubled operator (`dd`) is linewise on `count` lines.
        let operator = match ch {
            'd' => Some(Operator::Delete),
            'c' => Some(Operator::Change),
            'y' => Some(Operator::Yank),
            '>' => Some(Operator::Indent),
            '<' => Some(Operator::Dedent),
            _ => None,
        };
        if let Some(operator) = operator {
            if let Some(pending) = self.pending.operator {
                if pending == operator {
                    let from = self.cursor.0;
                    let to = (from + count - 1).min(self.buffer.line_count() - 1);
                    self.apply_operator(operator, Span::Lines { from, to });
                    self.pending.clear();
                    return;
                }
            }
            if let Some(visual) = self.visual_kind() {
                let span = self.visual_span(visual);
                self.apply_operator(operator, span);
                self.mode = Mode::Normal;
                self.pending.clear();
                return;
            }
            self.pending.operator = Some(operator);
            return;
        }

        // From here on the key is an action; a dangling operator is abandoned,
        // exactly as `dx` does nothing in vim.
        let register = self.pending.register;
        self.pending.clear();
        match ch {
            'i' => self.enter_insert(),
            'a' => {
                if self.cursor.1 < self.buffer.line_len(self.cursor.0) {
                    self.cursor.1 += 1;
                }
                self.enter_insert();
            }
            'I' => {
                self.cursor.1 = first_non_blank(&self.buffer, self.cursor.0);
                self.enter_insert();
            }
            'A' => {
                self.cursor.1 = self.buffer.line_len(self.cursor.0);
                self.enter_insert();
            }
            'o' => {
                self.buffer.begin_change(self.cursor);
                let indent = indent_of(&self.buffer, self.cursor.0);
                self.buffer
                    .insert_line(self.cursor.0 + 1, indent.chars().collect());
                self.cursor = (self.cursor.0 + 1, indent.chars().count());
                self.enter_insert();
            }
            'O' => {
                self.buffer.begin_change(self.cursor);
                let indent = indent_of(&self.buffer, self.cursor.0);
                self.buffer
                    .insert_line(self.cursor.0, indent.chars().collect());
                self.cursor = (self.cursor.0, indent.chars().count());
                self.enter_insert();
            }
            'x' => {
                let (line, col) = self.cursor;
                let end = (col + count).min(self.buffer.line_len(line));
                if col < end {
                    self.buffer.begin_change(self.cursor);
                    let text = self.buffer.delete_range_in_line(line, col, end);
                    self.buffer.commit_change();
                    self.store(
                        register,
                        Register {
                            lines: vec![text],
                            linewise: false,
                        },
                    );
                }
                self.clamp_cursor();
            }
            'X' => {
                let (line, col) = self.cursor;
                let start = col.saturating_sub(count);
                if start < col {
                    self.buffer.begin_change(self.cursor);
                    let text = self.buffer.delete_range_in_line(line, start, col);
                    self.buffer.commit_change();
                    self.store(
                        register,
                        Register {
                            lines: vec![text],
                            linewise: false,
                        },
                    );
                    self.cursor.1 = start;
                }
            }
            'D' => {
                let (line, col) = self.cursor;
                self.buffer.begin_change(self.cursor);
                let text = self
                    .buffer
                    .delete_range_in_line(line, col, self.buffer.line_len(line));
                self.buffer.commit_change();
                self.store(
                    register,
                    Register {
                        lines: vec![text],
                        linewise: false,
                    },
                );
                self.clamp_cursor();
            }
            'C' => {
                let (line, col) = self.cursor;
                self.buffer.begin_change(self.cursor);
                let text = self
                    .buffer
                    .delete_range_in_line(line, col, self.buffer.line_len(line));
                self.store(
                    register,
                    Register {
                        lines: vec![text],
                        linewise: false,
                    },
                );
                self.enter_insert();
            }
            's' => {
                let (line, col) = self.cursor;
                let end = (col + count).min(self.buffer.line_len(line));
                self.buffer.begin_change(self.cursor);
                let text = self.buffer.delete_range_in_line(line, col, end);
                self.store(
                    register,
                    Register {
                        lines: vec![text],
                        linewise: false,
                    },
                );
                self.enter_insert();
            }
            'S' => {
                let from = self.cursor.0;
                let to = (from + count - 1).min(self.buffer.line_count() - 1);
                self.apply_operator(Operator::Change, Span::Lines { from, to });
            }
            'p' | 'P' => self.paste(register, ch == 'p', count),
            'u' => {
                if let Some(cursor) = self.buffer.undo(self.cursor) {
                    self.cursor = cursor;
                    self.clamp_cursor();
                } else {
                    self.message = Some(Message {
                        text: "Already at oldest change".into(),
                        error: false,
                    });
                }
            }
            'J' => {
                self.buffer.begin_change(self.cursor);
                let mut at = self.cursor.1;
                for _ in 0..count.max(2) - 1 {
                    match self.buffer.join_lines(self.cursor.0, true) {
                        Some(column) => at = column,
                        None => break,
                    }
                }
                self.buffer.commit_change();
                self.cursor.1 = at;
                self.clamp_cursor();
            }
            '~' => {
                let (line, col) = self.cursor;
                if col < self.buffer.line_len(line) {
                    self.buffer.begin_change(self.cursor);
                    let mut row: Vec<char> = self.buffer.line(line).to_vec();
                    let end = (col + count).min(row.len());
                    for slot in row.iter_mut().take(end).skip(col) {
                        *slot = flip_case(*slot);
                    }
                    self.buffer.replace_line(line, row);
                    self.buffer.commit_change();
                    self.cursor.1 = end.min(self.buffer.line_len(line).saturating_sub(1));
                }
            }
            'v' => self.toggle_visual(Visual::Char),
            'V' => self.toggle_visual(Visual::Line),
            ':' | '/' | '?' => {
                self.cmdline_prefix = ch;
                self.cmdline.clear();
                self.cmdline_cursor = 0;
                self.mode = Mode::Cmdline;
            }
            'n' => self.search_next(self.last_search_forward),
            'N' => self.search_next(!self.last_search_forward),
            'Z' => self.pending.awaiting = Some(Await::G),
            _ => {}
        }
    }

    fn consume_motion(&mut self, mut motion: Motion) {
        let count = self.pending.total_count();
        if let Some(operator) = self.pending.operator {
            // vim's one documented irregularity: `cw` on a non-blank changes the
            // word without its trailing space, i.e. it behaves like `ce`. Reading
            // it as a plain `w` deletes the separator and joins two words.
            if operator == Operator::Change {
                if let Motion::WordForward { big } = motion {
                    let on_blank = self
                        .buffer
                        .line(self.cursor.0)
                        .get(self.cursor.1)
                        .is_none_or(|c| c.is_whitespace());
                    if !on_blank {
                        motion = Motion::WordEnd { big };
                    }
                }
            }
            let span = self.motion_span(motion, count);
            self.apply_operator(operator, span);
            self.pending.clear();
            return;
        }
        self.pending.clear();
        self.move_by(motion, count);
    }

    fn move_by(&mut self, motion: Motion, count: usize) {
        let target = motion::apply(&self.buffer, self.cursor, self.desired_col, motion, count);
        self.cursor = target;
        if !motion.keeps_desired_column() {
            self.desired_col = self.cursor.1;
        }
        self.clamp_cursor();
    }

    fn motion_span(&self, motion: Motion, count: usize) -> Span {
        let target = motion::apply(&self.buffer, self.cursor, self.desired_col, motion, count);
        match motion.kind() {
            Kind::Linewise => {
                let (from, to) = if target.0 <= self.cursor.0 {
                    (target.0, self.cursor.0)
                } else {
                    (self.cursor.0, target.0)
                };
                Span::Lines { from, to }
            }
            Kind::Inclusive | Kind::Exclusive => {
                let inclusive = motion.kind() == Kind::Inclusive;
                let (mut start, mut end) = if target <= self.cursor {
                    (target, self.cursor)
                } else {
                    (self.cursor, target)
                };
                // An inclusive motion covers the cell it lands on; the span is
                // stored end-exclusive, so widen it by one here rather than at
                // every use.
                if inclusive && target > self.cursor {
                    end.1 += 1;
                } else if inclusive && target < self.cursor {
                    end.1 = (end.1 + 1).min(self.buffer.line_len(end.0));
                }
                if start.0 == end.0 {
                    start.1 = start.1.min(self.buffer.line_len(start.0));
                    end.1 = end.1.min(self.buffer.line_len(end.0));
                }
                Span::Chars { start, end }
            }
        }
    }

    fn visual_span(&self, kind: Visual) -> Span {
        let (start, end) = match self.visual_range() {
            Some(range) => range,
            None => (self.cursor, self.cursor),
        };
        match kind {
            Visual::Line => Span::Lines {
                from: start.0,
                to: end.0,
            },
            Visual::Char => Span::Chars {
                start,
                end: (end.0, (end.1 + 1).min(self.buffer.line_len(end.0))),
            },
        }
    }

    fn apply_operator(&mut self, operator: Operator, span: Span) {
        let register = self.pending.register;
        match (operator, span) {
            (Operator::Yank, Span::Lines { from, to }) => {
                let lines = self.buffer.lines_text(from, to + 1);
                self.store(
                    register,
                    Register {
                        lines,
                        linewise: true,
                    },
                );
                self.cursor.0 = from;
                self.clamp_cursor();
            }
            (Operator::Yank, Span::Chars { start, end }) => {
                let text = self.slice(start, end);
                self.store(
                    register,
                    Register {
                        lines: text,
                        linewise: false,
                    },
                );
                self.cursor = start;
                self.clamp_cursor();
            }
            (Operator::Delete, Span::Lines { from, to }) => {
                self.buffer.begin_change(self.cursor);
                let removed = self.buffer.remove_lines(from, to - from + 1);
                self.buffer.commit_change();
                self.store(
                    register,
                    Register {
                        lines: removed,
                        linewise: true,
                    },
                );
                self.cursor.0 = from.min(self.buffer.line_count() - 1);
                self.cursor.1 = first_non_blank(&self.buffer, self.cursor.0);
                self.clamp_cursor();
            }
            (Operator::Delete, Span::Chars { start, end }) => {
                self.buffer.begin_change(self.cursor);
                let text = self.cut(start, end);
                self.buffer.commit_change();
                self.store(
                    register,
                    Register {
                        lines: text,
                        linewise: false,
                    },
                );
                self.cursor = start;
                self.clamp_cursor();
            }
            (Operator::Change, Span::Lines { from, to }) => {
                self.buffer.begin_change(self.cursor);
                let indent = indent_of(&self.buffer, from);
                let removed = self.buffer.remove_lines(from, to - from + 1);
                self.store(
                    register,
                    Register {
                        lines: removed,
                        linewise: true,
                    },
                );
                self.buffer.insert_line(from, indent.chars().collect());
                self.cursor = (from, indent.chars().count());
                self.enter_insert();
            }
            (Operator::Change, Span::Chars { start, end }) => {
                self.buffer.begin_change(self.cursor);
                let text = self.cut(start, end);
                self.store(
                    register,
                    Register {
                        lines: text,
                        linewise: false,
                    },
                );
                self.cursor = start;
                self.enter_insert();
            }
            // substitute.nvim: the span is replaced by the unnamed register
            // rather than deleted. What it removes does not go into a register
            // — that is the whole point of having it beside `d`.
            (Operator::Substitute, span) => {
                let replacement = self.registers.get(&'"').cloned().unwrap_or_default();
                self.buffer.begin_change(self.cursor);
                match span {
                    Span::Lines { from, to } => {
                        self.buffer.remove_lines(from, to - from + 1);
                        let mut at = from;
                        for line in &replacement.lines {
                            self.buffer.insert_line(at, line.chars().collect());
                            at += 1;
                        }
                        self.cursor = (from.min(self.buffer.line_count() - 1), 0);
                    }
                    Span::Chars { start, end } => {
                        self.cut(start, end);
                        let text = replacement.lines.join("\n");
                        let text = text.replace('\n', " ");
                        self.buffer.insert_str(start.0, start.1, &text);
                        self.cursor = (start.0, start.1 + text.chars().count());
                    }
                }
                self.buffer.commit_change();
                self.clamp_cursor();
            }
            (Operator::Indent | Operator::Dedent, span) => {
                let (from, to) = match span {
                    Span::Lines { from, to } => (from, to),
                    Span::Chars { start, end } => (start.0, end.0),
                };
                self.buffer.begin_change(self.cursor);
                for line in from..=to.min(self.buffer.line_count() - 1) {
                    let mut row: Vec<char> = self.buffer.line(line).to_vec();
                    let width = self.options.shiftwidth;
                    if operator == Operator::Indent {
                        if !row.is_empty() {
                            for _ in 0..width {
                                row.insert(0, ' ');
                            }
                        }
                    } else {
                        for _ in 0..width {
                            if row.first() == Some(&' ') {
                                row.remove(0);
                            }
                        }
                    }
                    self.buffer.replace_line(line, row);
                }
                self.buffer.commit_change();
                self.cursor.0 = from;
                self.cursor.1 = first_non_blank(&self.buffer, from);
                self.clamp_cursor();
            }
        }
    }

    /// The text between two positions, as one string per line it spans.
    fn slice(&self, start: (usize, usize), end: (usize, usize)) -> Vec<String> {
        if start.0 == end.0 {
            let row = self.buffer.line(start.0);
            let from = start.1.min(row.len());
            let to = end.1.min(row.len());
            return vec![row[from.min(to)..to].iter().collect()];
        }
        let mut out = Vec::new();
        let first = self.buffer.line(start.0);
        out.push(first[start.1.min(first.len())..].iter().collect());
        for line in start.0 + 1..end.0 {
            out.push(self.buffer.line_text(line));
        }
        let last = self.buffer.line(end.0);
        out.push(last[..end.1.min(last.len())].iter().collect());
        out
    }

    fn cut(&mut self, start: (usize, usize), end: (usize, usize)) -> Vec<String> {
        let text = self.slice(start, end);
        if start.0 == end.0 {
            self.buffer.delete_range_in_line(start.0, start.1, end.1);
        } else {
            let tail: String = {
                let row = self.buffer.line(end.0);
                row[end.1.min(row.len())..].iter().collect()
            };
            self.buffer
                .delete_range_in_line(start.0, start.1, self.buffer.line_len(start.0));
            self.buffer.remove_lines(start.0 + 1, end.0 - start.0);
            self.buffer.insert_str(start.0, start.1, &tail);
        }
        text
    }

    fn store(&mut self, register: Option<char>, value: Register) {
        if let Some(name) = register {
            self.registers.insert(name, value.clone());
        }
        if self.options.clipboard_unnamed {
            // A failed clipboard write loses the clipboard, not the yank: the
            // register is set either way.
            let mut text = value.lines.join("\n");
            if value.linewise {
                text.push('\n');
            }
            crate::clipboard::write(&text);
        }
        self.registers.insert('"', value);
    }

    fn paste(&mut self, register: Option<char>, after: bool, count: usize) {
        let name = register.unwrap_or('"');
        // With `clipboard = unnamedplus` the unnamed register *is* the system
        // clipboard, so a copy made in another program is what `p` puts.
        if register.is_none() && self.options.clipboard_unnamed {
            if let Some(text) = crate::clipboard::read() {
                let linewise = text.ends_with('\n');
                let body = text.strip_suffix('\n').unwrap_or(&text);
                self.registers.insert(
                    '"',
                    Register {
                        lines: body.split('\n').map(str::to_string).collect(),
                        linewise,
                    },
                );
            }
        }
        let Some(value) = self.registers.get(&name).cloned() else {
            return;
        };
        if value.lines.is_empty() {
            return;
        }
        self.buffer.begin_change(self.cursor);
        if value.linewise {
            let at = if after {
                self.cursor.0 + 1
            } else {
                self.cursor.0
            };
            let mut index = at;
            for _ in 0..count {
                for line in &value.lines {
                    self.buffer.insert_line(index, line.chars().collect());
                    index += 1;
                }
            }
            self.cursor = (at.min(self.buffer.line_count() - 1), 0);
            self.cursor.1 = first_non_blank(&self.buffer, self.cursor.0);
        } else {
            let (line, col) = self.cursor;
            let at = if after && self.buffer.line_len(line) > 0 {
                col + 1
            } else {
                col
            };
            if value.lines.len() == 1 {
                let text = value.lines[0].repeat(count);
                self.buffer.insert_str(line, at, &text);
                self.cursor.1 = at + text.chars().count().saturating_sub(1);
            } else {
                self.buffer.split_line(line, at);
                let mut index = line;
                for (offset, chunk) in value.lines.iter().enumerate() {
                    if offset == 0 {
                        self.buffer.insert_str(index, at, chunk);
                    } else if offset == value.lines.len() - 1 {
                        self.buffer.insert_str(index + 1, 0, chunk);
                        index += 1;
                    } else {
                        self.buffer.insert_line(index + 1, chunk.chars().collect());
                        index += 1;
                    }
                }
                self.cursor = (line + 1, 0);
            }
        }
        self.buffer.commit_change();
        self.clamp_cursor();
    }

    fn toggle_visual(&mut self, kind: Visual) {
        match self.mode {
            Mode::Visual(current) if current == kind => self.mode = Mode::Normal,
            _ => {
                self.visual_anchor = self.cursor;
                self.mode = Mode::Visual(kind);
            }
        }
        self.pending.clear();
    }

    fn enter_insert(&mut self) {
        self.buffer.begin_change(self.cursor);
        self.mode = Mode::Insert;
    }

    /// Whether matches should be painted. `hlsearch = false` is in the owner's
    /// config, and the editor was highlighting every match regardless.
    pub fn highlight_search(&self) -> bool {
        self.options.hlsearch
    }

    pub fn search_next(&mut self, forward: bool) {
        let Some(pattern) = self.last_search.clone() else {
            return;
        };
        if pattern.is_empty() {
            return;
        }
        let lines = self.buffer.line_count();
        let (line, col) = self.cursor;
        for step in 0..=lines {
            let index = if forward {
                (line + step) % lines
            } else {
                (line + lines - step % lines) % lines
            };
            let raw = self.buffer.line_text(index);
            let sensitive = self.options.case_sensitive(&pattern);
            let text = if sensitive {
                raw.clone()
            } else {
                raw.to_lowercase()
            };
            let pattern = if sensitive {
                pattern.clone()
            } else {
                pattern.to_lowercase()
            };
            let found = if forward {
                let from = if step == 0 {
                    char_boundary(&text, col + 1)
                } else {
                    0
                };
                if from > text.len() {
                    None
                } else {
                    text[from..].find(&pattern).map(|at| from + at)
                }
            } else {
                let to = if step == 0 {
                    char_boundary(&text, col)
                } else {
                    text.len()
                };
                text[..to.min(text.len())].rfind(&pattern)
            };
            if let Some(byte) = found {
                self.cursor = (index, text[..byte].chars().count());
                self.desired_col = self.cursor.1;
                self.clamp_cursor();
                return;
            }
        }
        self.message = Some(Message {
            text: format!("E486: Pattern not found: {pattern}"),
            error: true,
        });
    }

    fn max_top_line(&self) -> usize {
        self.buffer.line_count().saturating_sub(1)
    }

    fn save_focused_view(&mut self) {
        let focus = self.focus_window();
        let id = self.buffers.current_id();
        if let Some(view) = self.views.get_mut(&focus) {
            view.cursor = self.cursor;
            view.desired_col = self.desired_col;
            view.top_line = self.top_line;
            view.rows = self.view_rows;
            view.buffer = id;
        }
        if let Some(slot) = self.buffers.get_mut(id) {
            *slot = self.buffer.clone();
        }
    }

    fn load_focused_view(&mut self) {
        let focus = self.focus_window();
        if let Some(view) = self.views.get(&focus).cloned() {
            self.buffers.set_current(view.buffer);
            if let Some(buffer) = self.buffers.get(view.buffer) {
                self.buffer = buffer.clone();
            }
            self.cursor = view.cursor;
            self.desired_col = view.desired_col;
            self.top_line = view.top_line;
            self.view_rows = view.rows.max(1);
        }
        self.clamp_cursor();
    }

    fn switch_focused_to_buffer(&mut self, id: BufferId, reset_view: bool) -> bool {
        if !self.buffers.set_current(id) {
            return false;
        }
        if let Some(buffer) = self.buffers.get(id) {
            self.buffer = buffer.clone();
        }
        self.vcs.clear();
        if reset_view {
            self.cursor = (0, 0);
            self.desired_col = 0;
            self.top_line = 0;
        }
        if let Some(view) = self.views.get_mut(&self.focus_window()) {
            view.buffer = id;
            if reset_view {
                view.cursor = self.cursor;
                view.desired_col = self.desired_col;
                view.top_line = self.top_line;
            }
        }
        self.clamp_cursor();
        self.save_focused_view();
        self.reveal_current_in_tree();
        true
    }

    fn window_command(&mut self, ch: char) {
        self.save_focused_view();
        match ch {
            'h' | 'j' | 'k' | 'l' => {
                let dir = Direction::from_vim(ch).unwrap();
                self.focus_window_dir(dir);
            }
            's' | 'S' => self.split_window(false),
            'v' => self.split_window(true),
            'c' | 'q' => {
                self.close_window();
            }
            'o' => self.only_window(),
            'w' => self.cycle_window_focus(),
            _ => {}
        }
        self.pending.clear();
    }

    fn scroll_into_view(&mut self) {
        let rows = self.view_rows.max(1);
        if self.cursor.0 < self.top_line {
            self.top_line = self.cursor.0;
        } else if self.cursor.0 >= self.top_line + rows {
            self.top_line = self.cursor.0 + 1 - rows;
        }
        self.top_line = self.top_line.min(self.max_top_line());
    }

    fn clamp_cursor(&mut self) {
        let last = self.buffer.line_count().saturating_sub(1);
        self.cursor.0 = self.cursor.0.min(last);
        let len = self.buffer.line_len(self.cursor.0);
        // Normal mode sits on a character; insert mode may sit one past the end.
        let limit = match self.mode {
            Mode::Insert | Mode::Cmdline => len,
            _ => len.saturating_sub(1),
        };
        self.cursor.1 = self.cursor.1.min(limit);
    }
}

fn flip_case(ch: char) -> char {
    if ch.is_lowercase() {
        ch.to_uppercase().next().unwrap_or(ch)
    } else if ch.is_uppercase() {
        ch.to_lowercase().next().unwrap_or(ch)
    } else {
        ch
    }
}

fn indent_of(buffer: &Buffer, line: usize) -> String {
    buffer
        .line(line)
        .iter()
        .take_while(|c| c.is_whitespace())
        .collect()
}

fn first_non_blank(buffer: &Buffer, line: usize) -> usize {
    buffer
        .line(line)
        .iter()
        .position(|c| !c.is_whitespace())
        .unwrap_or(0)
}

/// The byte offset of a character index, clamped to the end of the string.
fn char_boundary(text: &str, index: usize) -> usize {
    text.char_indices()
        .nth(index)
        .map(|(byte, _)| byte)
        .unwrap_or(text.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor(text: &str) -> Editor {
        Editor::new(Buffer::from_text(text))
    }

    fn temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "nvimglsl-editor-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn insert_then_escape_leaves_normal_mode_on_the_last_character() {
        let mut e = editor("");
        e.feed_str("ihello");
        assert_eq!(e.mode, Mode::Insert);
        e.feed_str("<Esc>");
        assert_eq!(e.mode, Mode::Normal);
        assert_eq!(e.buffer.line_text(0), "hello");
        assert_eq!(e.cursor, (0, 4));
    }

    #[test]
    fn hjkl_moves() {
        let mut e = editor("one\ntwo\n");
        e.feed_str("lj");
        assert_eq!(e.cursor, (1, 1));
        e.feed_str("kh");
        assert_eq!(e.cursor, (0, 0));
    }

    #[test]
    fn bracket_c_moves_between_git_hunks() {
        let mut e = editor("one\ntwo\nthree\nfour\n");
        e.vcs.hunks = vec![
            crate::core::vcs::Hunk {
                old_start: 0,
                old_len: 1,
                new_start: 1,
                new_len: 1,
                kind: crate::core::vcs::SignKind::Change,
            },
            crate::core::vcs::Hunk {
                old_start: 3,
                old_len: 0,
                new_start: 3,
                new_len: 1,
                kind: crate::core::vcs::SignKind::Add,
            },
        ];
        e.feed_str("]c");
        assert_eq!(e.cursor.0, 1);
        e.feed_str("]c");
        assert_eq!(e.cursor.0, 3);
        e.feed_str("[c");
        assert_eq!(e.cursor.0, 1);
    }

    #[test]
    fn counts_multiply_across_an_operator() {
        let mut e = editor("a b c d e f g\n");
        e.feed_str("2d3w");
        assert_eq!(e.buffer.line_text(0), "g");
    }

    #[test]
    fn dd_removes_the_line_and_dollar_d_removes_to_the_end() {
        let mut e = editor("one\ntwo\nthree\n");
        e.feed_str("dd");
        assert_eq!(e.buffer.line_text(0), "two");
        assert_eq!(e.buffer.line_count(), 2);
        e.feed_str("ld$");
        assert_eq!(e.buffer.line_text(0), "t");
    }

    #[test]
    fn de_includes_the_last_character_of_the_word() {
        let mut e = editor("one two\n");
        e.feed_str("de");
        assert_eq!(e.buffer.line_text(0), " two");
    }

    #[test]
    fn yank_and_put_round_trip_linewise() {
        let mut e = editor("one\ntwo\n");
        e.feed_str("yyp");
        assert_eq!(e.buffer.lines_text(0, 3), vec!["one", "one", "two"]);
    }

    #[test]
    fn undo_and_redo_walk_the_same_edit() {
        let mut e = editor("one\n");
        e.feed_str("x");
        assert_eq!(e.buffer.line_text(0), "ne");
        e.feed_str("u");
        assert_eq!(e.buffer.line_text(0), "one");
        e.feed_str("<C-r>");
        assert_eq!(e.buffer.line_text(0), "ne");
    }

    #[test]
    fn visual_line_delete_takes_whole_lines() {
        let mut e = editor("one\ntwo\nthree\n");
        e.feed_str("Vjd");
        assert_eq!(e.buffer.line_count(), 1);
        assert_eq!(e.buffer.line_text(0), "three");
    }

    #[test]
    fn cw_leaves_insert_mode_open_where_the_word_was() {
        let mut e = editor("one two\n");
        e.feed_str("cwX");
        assert_eq!(e.mode, Mode::Insert);
        assert_eq!(e.buffer.line_text(0), "X two");
    }

    #[test]
    fn o_opens_a_line_below_and_keeps_the_indentation() {
        let mut e = editor("    one\n");
        e.feed_str("oX<Esc>");
        assert_eq!(e.buffer.line_text(1), "    X");
    }

    #[test]
    fn search_moves_to_the_next_match_and_wraps() {
        let mut e = editor("alpha\nbeta\nalpha\n");
        e.feed_str("/beta<CR>");
        assert_eq!(e.cursor, (1, 0));
        e.feed_str("/alpha<CR>");
        assert_eq!(e.cursor, (2, 0));
        e.feed_str("n");
        assert_eq!(e.cursor, (0, 0));
    }

    #[test]
    fn the_leader_mapping_asks_the_host_for_the_navigation_surface() {
        let mut e = editor("");
        e.feed_str(" o");
        assert_eq!(e.requests, vec![Request::OpenNavigation(Scope::Notes)]);
        e.requests.clear();
        e.feed_str(" f");
        assert_eq!(e.requests, vec![Request::OpenNavigation(Scope::Files)]);
    }

    #[test]
    fn file_browser_mapping_opens_a_left_unlisted_tree_and_reveals_the_buffer() {
        let root = temp_dir("file-browser");
        std::fs::create_dir_all(root.join("src")).unwrap();
        let file = root.join("src/main.rs");
        std::fs::write(&file, "fn main() {}\n").unwrap();
        std::fs::write(root.join("note.md"), "# note\n").unwrap();

        let mut config = crate::luaconf::NvimConfig::default();
        config.globals.insert(
            "mapleader".into(),
            crate::luaconf::Setting::Text(" ".into()),
        );
        config.mappings.push(crate::luaconf::Mapping {
            mode: "n".into(),
            lhs: "<leader>e".into(),
            rhs: "<cmd>Telescope file_browser path=%:p:h select_buffer=true<CR>".into(),
        });
        let buffer = Buffer::open(&file).unwrap();
        let mut e = Editor::with_config(buffer, &config);
        e.feed_str(" e");
        let tree = e.tree.as_ref().expect("tree pane");
        assert_eq!(e.focus_window(), tree.window);
        assert!(!e.buffers.list().contains(&tree.buffer));
        let canonical = file.canonicalize().unwrap();
        assert_eq!(tree.model.selected_path(), Some(canonical.as_path()));
        let rects = e.tabs.rects(
            Rect::new(0, 0, 10, 40),
            e.options.splitbelow,
            e.options.splitright,
        );
        let tree_rect = rects.text[&tree.window];
        let other = e.first_non_tree_window().unwrap();
        assert!(tree_rect.col < rects.text[&other].col);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn tree_focus_still_resolves_owner_keymaps_before_tree_keys() {
        let root = temp_dir("tree-keymap");
        std::fs::create_dir_all(&root).unwrap();
        for name in ["a.md", "b.md", "c.md"] {
            std::fs::write(root.join(name), "").unwrap();
        }
        let mut config = crate::luaconf::NvimConfig::default();
        config.mappings.push(crate::luaconf::Mapping {
            mode: "n".into(),
            lhs: "fj".into(),
            rhs: "2j".into(),
        });
        let mut e = Editor::with_config(Buffer::from_text("x\n"), &config);
        e.open_file_tree(root.clone(), None);
        assert_eq!(e.tree.as_ref().unwrap().model.selected(), 0);
        e.feed_str("fj");
        assert_eq!(e.tree.as_ref().unwrap().model.selected(), 2);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn gf_asks_the_host_to_follow_a_link() {
        let mut e = editor("see [[alpha]]\n");
        e.feed_str("gf");
        assert_eq!(e.requests, vec![Request::FollowLink]);
    }

    #[test]
    fn gd_asks_the_host_for_lsp_definition() {
        let mut e = editor("symbol\n");
        e.feed_str("gd");
        assert_eq!(e.requests, vec![Request::Definition]);
    }

    #[test]
    fn ctrl_w_moves_focus_without_moving_the_cursor_column() {
        let mut e = editor("abcd\n");
        e.options.splitright = true;
        e.feed_str("ll<C-w>v");
        let right = e.focus_window();
        assert_eq!(e.cursor.1, 2);
        e.feed_str("<C-w>h");
        assert_ne!(e.focus_window(), right);
        assert_eq!(e.cursor.1, 2);
        e.feed_str("<C-w>w");
        assert_eq!(e.focus_window(), right);
        assert_eq!(e.cursor.1, 2);
    }

    #[test]
    fn ctrl_w_split_close_and_only_update_the_window_model() {
        let mut e = editor("one\n");
        assert_eq!(e.views.len(), 1);
        e.feed_str("<C-w>s");
        assert_eq!(e.views.len(), 2);
        e.feed_str("<C-w>v");
        assert_eq!(e.views.len(), 3);
        e.feed_str("<C-w>c");
        assert_eq!(e.views.len(), 2);
        e.feed_str("<C-w>o");
        assert_eq!(e.views.len(), 1);
    }

    #[test]
    fn space_p_asks_the_host_for_a_vertical_page() {
        let mut e = editor("本文。\n");
        e.feed_str(" p");
        assert_eq!(e.requests, vec![Request::Preview]);
    }

    #[test]
    fn japanese_text_is_edited_by_character_not_byte() {
        let mut e = editor("あいう\n");
        e.feed_str("lx");
        assert_eq!(e.buffer.line_text(0), "あう");
    }

    #[test]
    fn indent_moves_by_the_configured_shiftwidth() {
        // With no config to read, vim's own default applies rather than a
        // number of this editor's choosing.
        let mut e = editor("one\ntwo\n");
        assert_eq!(e.options.shiftwidth, 8);
        e.feed_str(">j");
        assert_eq!(e.buffer.line_text(0), "        one");
        e.feed_str("<<");
        assert_eq!(e.buffer.line_text(0), "one");

        let mut config = crate::luaconf::NvimConfig::default();
        config
            .options
            .insert("shiftwidth".into(), crate::luaconf::Setting::Number(2.0));
        config
            .options
            .insert("expandtab".into(), crate::luaconf::Setting::Bool(true));
        let mut e = Editor::with_config(Buffer::from_text("one\ntwo\n"), &config);
        e.feed_str(">j");
        assert_eq!(e.buffer.line_text(0), "  one");
        assert_eq!(e.buffer.line_text(1), "  two");
    }

    /// The mappings are the owner's, not this editor's. These come from a
    /// config built in the test, so what is checked is that the engine applies
    /// whatever it read.
    #[test]
    fn a_mapped_key_sequence_replaces_the_builtin() {
        let mut config = crate::luaconf::NvimConfig::default();
        config.globals.insert(
            "mapleader".into(),
            crate::luaconf::Setting::Text(" ".into()),
        );
        for (mode, lhs, rhs) in [
            ("n", "Y", "y$"),
            ("n", "H", "^"),
            ("n", "L", "$"),
            ("i", "kj", "<ESC>"),
            ("n", "fj", "2j"),
        ] {
            config.mappings.push(crate::luaconf::Mapping {
                mode: mode.into(),
                lhs: lhs.into(),
                rhs: rhs.into(),
            });
        }
        let mut e = Editor::with_config(Buffer::from_text("  alpha beta\ntwo\nthree\n"), &config);

        // Y is y$, not yy: putting it back gives characters, not a line.
        e.feed_str("wwY$p");
        assert_eq!(e.buffer.line_text(0), "  alpha betabeta");

        e.feed_str("H");
        assert_eq!(e.cursor.1, 2, "H should be ^, the first non-blank");
        e.feed_str("L");
        assert_eq!(e.cursor.1, e.buffer.line_len(0) - 1, "L should be $");

        // kj leaves insert mode; the k must not be left in the buffer.
        e.feed_str("ggIx");
        assert_eq!(e.mode, Mode::Insert);
        e.feed_str("kj");
        assert_eq!(e.mode, Mode::Normal);
        assert!(
            e.buffer.line_text(0).starts_with("  x"),
            "{}",
            e.buffer.line_text(0)
        );

        // fj is a mapping; f followed by anything else is still find-character.
        // A fresh buffer, because the edits above moved the columns.
        let mut e = Editor::with_config(Buffer::from_text("  alpha beta\ntwo\nthree\n"), &config);
        e.feed_str("fj");
        assert_eq!(e.cursor.0, 2, "fj should be the mapping, not find-j");
        e.feed_str("gg0fb");
        assert_eq!(e.cursor, (0, 8), "f followed by anything else still finds");
    }

    #[test]
    fn a_mapped_command_reaches_the_host_and_an_unknown_one_says_so() {
        let mut config = crate::luaconf::NvimConfig::default();
        config.globals.insert(
            "mapleader".into(),
            crate::luaconf::Setting::Text(" ".into()),
        );
        for (mode, lhs, rhs) in [
            ("n", "<space>o", "<cmd>Telescope find_files<cr>"),
            ("n", "<Leader>q", "<cmd>q<CR>"),
            ("n", "<F5>", "<cmd>Jaq<CR>"),
        ] {
            config.mappings.push(crate::luaconf::Mapping {
                mode: mode.into(),
                lhs: lhs.into(),
                rhs: rhs.into(),
            });
        }
        let mut e = Editor::with_config(Buffer::from_text("x\n"), &config);
        e.feed_str(" o");
        assert_eq!(e.requests, vec![Request::OpenNavigation(Scope::Files)]);
        e.feed_str(" q");
        assert!(e.requests.contains(&Request::Quit));
        e.feed_str("<F5>");
        assert!(
            e.message.as_ref().unwrap().text.contains("no such command"),
            "an unrunnable mapping must say so: {:?}",
            e.message
        );
    }

    #[test]
    fn search_follows_ignorecase_and_smartcase() {
        let mut config = crate::luaconf::NvimConfig::default();
        config
            .options
            .insert("ignorecase".into(), crate::luaconf::Setting::Bool(true));
        config
            .options
            .insert("smartcase".into(), crate::luaconf::Setting::Bool(true));
        let mut e = Editor::with_config(Buffer::from_text("alpha\nAlpha\n"), &config);
        e.feed_str("/alpha<CR>");
        assert_eq!(
            e.cursor.0, 1,
            "a lowercase pattern should match either case"
        );
        e.feed_str("gg/Alpha<CR>");
        assert_eq!(e.cursor.0, 1, "a capital in the pattern means it was meant");
    }

    #[test]
    fn a_named_register_survives_an_intervening_delete() {
        let mut e = editor("one\ntwo\n");
        e.feed_str("\"ayy");
        e.feed_str("jdd");
        e.feed_str("\"ap");
        assert_eq!(e.buffer.lines_text(0, 2), vec!["one", "one"]);
    }

    #[test]
    fn G_and_gg_go_to_the_ends_and_a_count_goes_to_a_line() {
        let mut e = editor("one\ntwo\nthree\n");
        e.feed_str("G");
        assert_eq!(e.cursor.0, 2);
        e.feed_str("gg");
        assert_eq!(e.cursor.0, 0);
        e.feed_str("2G");
        assert_eq!(e.cursor.0, 1);
    }

    #[test]
    fn the_real_config_tab_window_keys_are_focus_keys_when_present() {
        let config = crate::luaconf::load_default();
        if config.path.is_none() {
            return;
        }
        let mut e = Editor::with_config(Buffer::from_text("abcd\n"), &config);
        e.feed_str("l<C-w>v");
        let right = e.focus_window();
        e.feed_str("l");
        assert_eq!(e.cursor.1, 2);
        e.feed_str("<tab>h");
        assert_ne!(e.focus_window(), right);
        assert_eq!(e.cursor.1, 1);
        e.feed_str("<tab>l");
        assert_eq!(e.focus_window(), right);
        assert_eq!(e.cursor.1, 2);
    }
}
