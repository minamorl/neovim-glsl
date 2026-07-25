//! External UI surfaces: the completion popupmenu, the command line and messages.
//!
//! With `ext_popupmenu`, `ext_cmdline` and `ext_messages` attached, Neovim stops
//! painting those surfaces into the grid and sends them as structured events
//! instead. This module mirrors that state and turns it into positioned spans.
//!
//! It decides *placement*, never content: which rows the command line lands on
//! is ours, what the command line says is Neovim's. Every event is parsed
//! defensively — a truncated or mistyped event leaves the previous state intact
//! rather than panicking, because a UI that dies on a malformed frame takes the
//! editing session with it.

use std::collections::HashMap;

use rmpv::Value;

use crate::grid::Grid;
use crate::nvim::RedrawEvent;

/// Rows the popupmenu may occupy before it starts scrolling instead of growing.
const MAX_POPUP_ROWS: usize = 10;

/// Upper bound on retained `msg_show` lines. Neovim clears messages with
/// `msg_clear`, but a session that never clears must not grow without bound.
const MAX_MESSAGES: usize = 64;

/// Upper bound on the chunks one retained message may accumulate. `MAX_MESSAGES`
/// bounds how many messages are kept; a run of appending `echon` calls extends a
/// single message instead, and that path needs its own ceiling.
const MAX_MESSAGE_CHUNKS: usize = 512;

/// Columns between the `showcmd` field and the ruler, matching Vim's layout.
const SHOWCMD_GAP: usize = 11;

/// Cell width of a character. The grid is monospaced, so this is the same rule
/// the renderer uses for the IME preedit: anything outside the Latin/symbol
/// range occupies two cells.
pub fn char_cells(c: char) -> usize {
    if (c as u32) < 0x2500 {
        1
    } else {
        2
    }
}

/// Cell width of a string.
pub fn display_width(s: &str) -> usize {
    s.chars().map(char_cells).sum()
}

/// The character occupying a given cell of `text`, or a space past the end.
fn char_at_cell(text: &str, cell: usize) -> char {
    let mut at = 0;
    for c in text.chars() {
        if at == cell {
            return c;
        }
        at += char_cells(c);
        if at > cell {
            // The cell is the trailing half of a double-width glyph.
            return ' ';
        }
    }
    ' '
}

/// Built-in highlight groups Neovim reports through `hl_group_set`, for UI
/// elements it no longer draws itself.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UiHl {
    Pmenu,
    PmenuSel,
    PmenuSbar,
    PmenuThumb,
}

impl UiHl {
    pub fn group(self) -> &'static str {
        match self {
            UiHl::Pmenu => "Pmenu",
            UiHl::PmenuSel => "PmenuSel",
            UiHl::PmenuSbar => "PmenuSbar",
            UiHl::PmenuThumb => "PmenuThumb",
        }
    }

    /// How far to move the background towards the foreground when the colour
    /// scheme has not told us what this group looks like.
    fn fallback_mix(self) -> f32 {
        match self {
            UiHl::Pmenu => 0.14,
            UiHl::PmenuSel => 0.42,
            UiHl::PmenuSbar => 0.24,
            UiHl::PmenuThumb => 0.70,
        }
    }
}

/// Where a span takes its colours from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HlRef {
    /// A highlight id from a `hl_attr_define`, as carried by message and
    /// cmdline content chunks.
    Attr(u64),
    /// A built-in UI highlight group.
    Ui(UiHl),
}

/// A run of text placed at a cell position, in draw order.
#[derive(Clone, PartialEq, Debug)]
pub struct Span {
    pub row: usize,
    pub col: usize,
    pub text: String,
    pub hl: HlRef,
}

/// The cursor an external surface owns. While it exists the grid cursor is not
/// the one the user is looking at.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Cursor {
    pub row: usize,
    pub col: usize,
    /// The character under the cursor, so the block can be drawn inverted the
    /// same way the grid cursor is.
    pub ch: char,
}

/// One frame of external UI, ready to draw.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct Overlay {
    pub spans: Vec<Span>,
    pub cursor: Option<Cursor>,
    /// Rows at the bottom of the screen the surfaces claimed this frame.
    pub reserved_rows: usize,
}

/// One completion candidate as `popupmenu_show` describes it.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct PopupItem {
    pub word: String,
    pub kind: String,
    pub menu: String,
    pub info: String,
}

impl PopupItem {
    /// The single line the menu shows for this item.
    fn label(&self) -> String {
        let mut out = self.word.clone();
        for extra in [&self.kind, &self.menu] {
            if !extra.is_empty() {
                out.push(' ');
                out.push_str(extra);
            }
        }
        out
    }
}

#[derive(Clone, Default, PartialEq, Debug)]
pub struct PopupMenu {
    pub items: Vec<PopupItem>,
    /// `None` when nothing is selected; Neovim sends -1 for that.
    pub selected: Option<usize>,
    pub row: usize,
    pub col: usize,
    /// The grid the anchor is relative to. -1 means the command line.
    pub grid: i64,
}

/// One command line level. `:` opening `:h` opens level 2 over level 1, so this
/// is a stack rather than a single value.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct Cmdline {
    pub level: u64,
    pub firstc: String,
    pub prompt: String,
    pub indent: usize,
    pub content: Vec<(u64, String)>,
    /// Cursor position as a *byte* offset into the concatenated content, which
    /// is what `cmdline_show` and `cmdline_pos` report.
    pub pos: usize,
    /// A literal character being inserted (`<C-v>`, `<C-k>` digraphs).
    pub special: Option<(char, bool)>,
}

impl Cmdline {
    fn text(&self) -> String {
        self.content.iter().map(|(_, t)| t.as_str()).collect()
    }
}

/// A message as `msg_show` or `msg_history_show` delivers it.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct Message {
    pub kind: String,
    pub chunks: Vec<(u64, String)>,
}

/// Everything Neovim externalised, mirrored.
#[derive(Default)]
pub struct ExtUi {
    pub popupmenu: Option<PopupMenu>,
    pub cmdlines: Vec<Cmdline>,
    pub cmdline_block: Vec<Vec<(u64, String)>>,
    pub messages: Vec<Message>,
    pub history: Vec<Message>,
    pub history_visible: bool,
    pub showmode: Vec<(u64, String)>,
    pub showcmd: Vec<(u64, String)>,
    pub ruler: Vec<(u64, String)>,
    /// `hl_group_set`: built-in group name -> highlight id.
    pub ui_groups: HashMap<String, u64>,
}

impl ExtUi {
    pub fn new() -> Self {
        Self::default()
    }

    /// True when no external surface wants any pixels this frame.
    pub fn is_idle(&self) -> bool {
        self.popupmenu.is_none()
            && self.cmdlines.is_empty()
            && self.cmdline_block.is_empty()
            && self.messages.is_empty()
            && !self.history_visible
            && self.showmode.is_empty()
            && self.showcmd.is_empty()
            && self.ruler.is_empty()
    }

    /// The command line the user is typing into, i.e. the innermost level.
    pub fn active_cmdline(&self) -> Option<&Cmdline> {
        self.cmdlines.last()
    }

    pub fn apply(&mut self, events: &[RedrawEvent]) {
        for (name, args) in events {
            match name.as_str() {
                "hl_group_set" => self.ev_hl_group_set(args),

                "popupmenu_show" => self.ev_popupmenu_show(args),
                "popupmenu_select" => self.ev_popupmenu_select(args),
                "popupmenu_hide" => self.popupmenu = None,

                "cmdline_show" => self.ev_cmdline_show(args),
                "cmdline_pos" => self.ev_cmdline_pos(args),
                "cmdline_special_char" => self.ev_cmdline_special_char(args),
                "cmdline_hide" => self.ev_cmdline_hide(args),
                "cmdline_block_show" => self.ev_cmdline_block_show(args),
                "cmdline_block_append" => self.ev_cmdline_block_append(args),
                "cmdline_block_hide" => self.cmdline_block.clear(),

                "msg_show" => self.ev_msg_show(args),
                "msg_clear" => self.ev_msg_clear(),
                "msg_showmode" => self.showmode = parse_chunks(args.first()),
                "msg_showcmd" => self.showcmd = parse_chunks(args.first()),
                "msg_ruler" => self.ruler = parse_chunks(args.first()),
                "msg_history_show" => self.ev_msg_history_show(args),
                "msg_history_clear" => {
                    self.history.clear();
                    self.history_visible = false;
                }
                _ => {}
            }
        }
    }

    fn ev_hl_group_set(&mut self, a: &[Value]) {
        let (Some(name), Some(id)) = (a.first().and_then(Value::as_str), u(a, 1)) else {
            return;
        };
        self.ui_groups.insert(name.to_string(), id);
    }

    /// `popupmenu_show(items, selected, row, col, grid)`, where each item is
    /// `[word, kind, menu, info]`.
    fn ev_popupmenu_show(&mut self, a: &[Value]) {
        let Some(raw_items) = a.first().and_then(Value::as_array) else {
            return;
        };
        let (Some(row), Some(col)) = (u(a, 2), u(a, 3)) else {
            return;
        };
        let items = raw_items
            .iter()
            .map(|item| {
                let parts = item.as_array().map(|p| p.as_slice()).unwrap_or(&[]);
                let field = |i: usize| {
                    parts
                        .get(i)
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string()
                };
                PopupItem {
                    word: field(0),
                    kind: field(1),
                    menu: field(2),
                    info: field(3),
                }
            })
            .collect::<Vec<_>>();
        let selected = a.get(1).and_then(Value::as_i64).unwrap_or(-1);
        self.popupmenu = Some(PopupMenu {
            selected: normalize_selection(selected, items.len()),
            items,
            row: row as usize,
            col: col as usize,
            grid: a.get(4).and_then(Value::as_i64).unwrap_or(1),
        });
    }

    fn ev_popupmenu_select(&mut self, a: &[Value]) {
        let Some(menu) = self.popupmenu.as_mut() else {
            return;
        };
        let Some(selected) = a.first().and_then(Value::as_i64) else {
            return;
        };
        menu.selected = normalize_selection(selected, menu.items.len());
    }

    /// `cmdline_show(content, pos, firstc, prompt, indent, level)`.
    fn ev_cmdline_show(&mut self, a: &[Value]) {
        let Some(level) = u(a, 5) else { return };
        let line = Cmdline {
            level,
            firstc: str_at(a, 2).to_string(),
            prompt: str_at(a, 3).to_string(),
            indent: u(a, 4).unwrap_or(0) as usize,
            content: parse_chunks(a.first()),
            pos: u(a, 1).unwrap_or(0) as usize,
            // A new frame for this level supersedes any pending literal input.
            special: None,
        };
        match self.cmdlines.iter().position(|c| c.level == level) {
            Some(at) => {
                self.cmdlines.truncate(at + 1);
                self.cmdlines[at] = line;
            }
            None => self.cmdlines.push(line),
        }
    }

    fn ev_cmdline_pos(&mut self, a: &[Value]) {
        let (Some(pos), Some(level)) = (u(a, 0), u(a, 1)) else {
            return;
        };
        if let Some(line) = self.cmdlines.iter_mut().find(|c| c.level == level) {
            line.pos = pos as usize;
        }
    }

    /// `cmdline_special_char(c, shift, level)`.
    fn ev_cmdline_special_char(&mut self, a: &[Value]) {
        let Some(level) = u(a, 2) else { return };
        let Some(c) = a
            .first()
            .and_then(Value::as_str)
            .and_then(|s| s.chars().next())
        else {
            return;
        };
        let shift = a.get(1).and_then(Value::as_bool).unwrap_or(false);
        if let Some(line) = self.cmdlines.iter_mut().find(|l| l.level == level) {
            line.special = Some((c, shift));
        }
    }

    /// `cmdline_hide(level)`. Closing a level closes everything nested inside it.
    fn ev_cmdline_hide(&mut self, a: &[Value]) {
        // Without a usable level there is no way to know how much of the stack
        // to unwind, and tearing all of it down would lose a command line the
        // user is still typing into. Leave the state alone, as every other
        // handler does on a bad parse.
        let Some(level) = u(a, 0) else { return };
        self.cmdlines.retain(|c| c.level < level);
        if self.cmdlines.is_empty() {
            self.cmdline_block.clear();
        }
    }

    fn ev_cmdline_block_show(&mut self, a: &[Value]) {
        let Some(lines) = a.first().and_then(Value::as_array) else {
            return;
        };
        self.cmdline_block = lines.iter().map(|l| parse_chunks(Some(l))).collect();
    }

    fn ev_cmdline_block_append(&mut self, a: &[Value]) {
        // A mistyped argument would otherwise append a blank line to the block.
        let Some(line) = a.first().filter(|v| v.as_array().is_some()) else {
            return;
        };
        self.cmdline_block.push(parse_chunks(Some(line)));
    }

    /// `msg_show(kind, content, replace_last[, history, append])`. The two
    /// trailing flags only exist on newer Neovim; their absence is not an error.
    fn ev_msg_show(&mut self, a: &[Value]) {
        let Some(content) = a.get(1) else { return };
        let chunks = parse_chunks(Some(content));
        let kind = str_at(a, 0).to_string();
        let replace_last = a.get(2).and_then(Value::as_bool).unwrap_or(false);
        let append = a.get(4).and_then(Value::as_bool).unwrap_or(false);

        if append {
            if let Some(last) = self.messages.last_mut() {
                last.chunks.extend(chunks);
                if last.chunks.len() > MAX_MESSAGE_CHUNKS {
                    let overflow = last.chunks.len() - MAX_MESSAGE_CHUNKS;
                    last.chunks.drain(..overflow);
                }
                return;
            }
        }
        if replace_last {
            self.messages.pop();
        }
        self.messages.push(Message { kind, chunks });
        if self.messages.len() > MAX_MESSAGES {
            let overflow = self.messages.len() - MAX_MESSAGES;
            self.messages.drain(..overflow);
        }
    }

    fn ev_msg_clear(&mut self) {
        self.messages.clear();
        // The history overlay is dismissed by the same keypress that clears the
        // message area; Neovim sends no separate event for it.
        self.history_visible = false;
    }

    /// `msg_history_show(entries)`, each entry `[kind, content]`.
    fn ev_msg_history_show(&mut self, a: &[Value]) {
        let Some(entries) = a.first().and_then(Value::as_array) else {
            return;
        };
        self.history = entries
            .iter()
            .filter_map(|entry| {
                let parts = entry.as_array()?;
                Some(Message {
                    kind: parts
                        .first()
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    chunks: parse_chunks(parts.get(1)),
                })
            })
            .collect();
        self.history_visible = true;
    }

    /// Resolve a span's colours through the grid's highlight table, falling back
    /// to a shade of the default background when the colour scheme has not
    /// defined the group.
    pub fn colors(&self, grid: &Grid, hl: HlRef) -> (u32, u32) {
        match hl {
            HlRef::Attr(id) => grid.colors(id),
            HlRef::Ui(group) => match self.ui_groups.get(group.group()) {
                Some(&id) => grid.colors(id),
                None => (
                    grid.default_fg,
                    blend(grid.default_bg, grid.default_fg, group.fallback_mix()),
                ),
            },
        }
    }

    /// Place every active surface into a `cols` x `rows` screen.
    pub fn layout(&self, cols: usize, rows: usize) -> Overlay {
        let mut overlay = Overlay::default();
        if cols == 0 || rows == 0 {
            return overlay;
        }

        let lines = self.bottom_lines(cols);
        let shown = lines.len().min(rows);
        let start = rows - shown;
        // When more lines exist than fit, the newest ones win: the command line
        // being typed into must never be the line that got dropped.
        let skipped = lines.len() - shown;
        overlay.reserved_rows = shown;

        let mut cmdline_row = None;
        for (i, line) in lines[skipped..].iter().enumerate() {
            let row = start + i;
            overlay.spans.push(Span {
                row,
                col: 0,
                text: " ".repeat(cols),
                hl: HlRef::Attr(0),
            });
            let mut col = 0;
            for (attr, text) in &line.chunks {
                if col >= cols {
                    break;
                }
                overlay.spans.push(Span {
                    row,
                    col,
                    text: text.clone(),
                    hl: HlRef::Attr(*attr),
                });
                col += display_width(text);
            }
            if let Some(cursor_col) = line.cursor {
                cmdline_row = Some(row);
                overlay.cursor = Some(Cursor {
                    row,
                    col: cursor_col.min(cols - 1),
                    ch: char_at_cell(&line.text(), cursor_col),
                });
            }
        }

        self.layout_popupmenu(cols, rows, shown, cmdline_row, &mut overlay);
        overlay
    }

    /// The lines the bottom of the screen is given over to, top to bottom.
    ///
    /// Everything is fitted to `cols` here rather than left for the renderer to
    /// clip. Neovim wrapped messages and scrolled the command line itself while
    /// it still painted those surfaces into the grid; asking for `ext_messages`
    /// and `ext_cmdline` moves that job here, and dropping the overflow instead
    /// would silently lose the tail of any message wider than the window.
    fn bottom_lines(&self, cols: usize) -> Vec<LayoutLine> {
        let mut unwrapped = Vec::new();

        if self.history_visible {
            for entry in &self.history {
                unwrapped.extend(chunks_to_lines(&entry.chunks));
            }
        }
        for message in &self.messages {
            unwrapped.extend(chunks_to_lines(&message.chunks));
        }
        for block in &self.cmdline_block {
            unwrapped.extend(chunks_to_lines(block));
        }
        let mut lines: Vec<LayoutLine> = unwrapped
            .into_iter()
            .flat_map(|line| wrap_line(line, cols))
            .collect();

        match self.active_cmdline() {
            Some(cmdline) => lines.push(self.cmdline_line(cmdline, cols)),
            None => {
                if let Some(line) = self.status_line(cols) {
                    lines.push(line);
                }
            }
        }
        lines
    }

    /// The command line itself: prefix, content, and the cursor inside it.
    fn cmdline_line(&self, cmdline: &Cmdline, cols: usize) -> LayoutLine {
        let prefix = format!(
            "{}{}{}",
            cmdline.firstc,
            cmdline.prompt,
            " ".repeat(cmdline.indent)
        );
        let text = cmdline.text();
        // `pos` is a byte offset; a multibyte command line would otherwise put
        // the cursor in the middle of a character. Floor to the boundary at or
        // before `pos` so an off-boundary offset lands next to the intended
        // character rather than at the far end of the line.
        let head = &text[..floor_char_boundary(&text, cmdline.pos)];
        let cursor = display_width(&prefix) + display_width(head);

        let mut chunks = vec![(0u64, prefix)];
        chunks.extend(cmdline.content.iter().cloned());
        if let Some((c, _shift)) = cmdline.special {
            // The literal being composed sits at the cursor and is not yet part
            // of the command line's content.
            chunks.push((0, String::new()));
            return scroll_to_cursor(
                LayoutLine {
                    chunks: insert_at_cell(chunks, cursor, c),
                    cursor: Some(cursor),
                },
                cols,
            );
        }
        scroll_to_cursor(
            LayoutLine {
                chunks,
                cursor: Some(cursor),
            },
            cols,
        )
    }

    /// The `showmode` / `showcmd` / ruler line Neovim externalises along with
    /// messages. Only drawn when no command line is open, which is where Vim
    /// itself puts it.
    fn status_line(&self, cols: usize) -> Option<LayoutLine> {
        if self.showmode.is_empty() && self.showcmd.is_empty() && self.ruler.is_empty() {
            return None;
        }
        let mut chunks = self.showmode.clone();
        let mut at = chunks_width(&chunks);

        let ruler_w = chunks_width(&self.ruler);
        let showcmd_w = chunks_width(&self.showcmd);
        let ruler_col = cols.saturating_sub(ruler_w);
        let showcmd_col = ruler_col.saturating_sub(SHOWCMD_GAP);

        if showcmd_w > 0 && showcmd_col >= at {
            chunks.push((0, " ".repeat(showcmd_col - at)));
            chunks.extend(self.showcmd.iter().cloned());
            at = showcmd_col + showcmd_w;
        }
        if ruler_w > 0 && ruler_col >= at {
            chunks.push((0, " ".repeat(ruler_col - at)));
            chunks.extend(self.ruler.iter().cloned());
        }
        // A `showmode` longer than the window would otherwise run off the edge.
        // This line is positioned absolutely, so it truncates rather than wraps.
        let (chunks, _) = split_chunks_at_cell(&chunks, cols);
        Some(LayoutLine {
            chunks,
            cursor: None,
        })
    }

    fn layout_popupmenu(
        &self,
        cols: usize,
        rows: usize,
        reserved_rows: usize,
        cmdline_row: Option<usize>,
        overlay: &mut Overlay,
    ) {
        let Some(menu) = self.popupmenu.as_ref() else {
            return;
        };
        if menu.items.is_empty() {
            return;
        }

        // A grid of -1 means the menu belongs to the command line's wildmenu, so
        // it anchors to wherever the command line actually landed.
        let anchor_row = match (menu.grid, cmdline_row) {
            (-1, Some(row)) => row,
            _ => menu.row,
        };
        // A stale anchor from before a resize can sit below the screen; the menu
        // still has to land somewhere the user can see it.
        let anchor_row = anchor_row.min(rows - 1);

        // Below the anchor is where Neovim expects the menu; above is the
        // fallback when the anchor sits too close to the bottom of the screen.
        // Whichever side is taken, the box has to fit *within* that side: it may
        // never grow back across the anchor row, because the row under the
        // anchor is the command line the wildmenu belongs to.
        //
        // Growing *downwards* is bounded by the rows the bottom surfaces already
        // claimed, so a tall menu cannot bury the command line the user is
        // typing into. A menu whose anchor is itself inside that region is the
        // wildmenu: it only ever opens upwards, over the message area, which is
        // where Vim puts it too.
        let reserved_top = rows.saturating_sub(reserved_rows);
        let ceiling = if anchor_row < reserved_top {
            reserved_top
        } else {
            rows
        };
        let wanted = menu.items.len().min(MAX_POPUP_ROWS);
        let below = ceiling.saturating_sub(anchor_row + 1);
        let above = anchor_row;
        let (row, height) = if wanted <= below {
            (anchor_row + 1, wanted)
        } else if wanted <= above {
            (anchor_row - wanted, wanted)
        } else if below >= above {
            (anchor_row + 1, below)
        } else {
            (anchor_row - above, above)
        };
        if height == 0 {
            return;
        }

        let scrollable = menu.items.len() > height;
        let label_w = menu
            .items
            .iter()
            .map(|item| display_width(&item.label()))
            .max()
            .unwrap_or(1)
            .max(1);
        let bar_w = usize::from(scrollable);
        let width = label_w.min(cols.saturating_sub(bar_w)).max(1);
        let col = anchor_row_col(menu.col, width + bar_w, cols);

        let selected = menu.selected.unwrap_or(0);
        let top = selected.saturating_sub(height.saturating_sub(1));

        for i in 0..height {
            let index = top + i;
            let Some(item) = menu.items.get(index) else {
                break;
            };
            let hl = if menu.selected == Some(index) {
                HlRef::Ui(UiHl::PmenuSel)
            } else {
                HlRef::Ui(UiHl::Pmenu)
            };
            overlay.spans.push(Span {
                row: row + i,
                col,
                text: pad_to(&item.label(), width),
                hl,
            });
        }

        if scrollable {
            let count = menu.items.len();
            let thumb_len = (height * height / count).max(1);
            let thumb_top = (top * height / count).min(height - thumb_len);
            for i in 0..height {
                let hl = if i >= thumb_top && i < thumb_top + thumb_len {
                    HlRef::Ui(UiHl::PmenuThumb)
                } else {
                    HlRef::Ui(UiHl::PmenuSbar)
                };
                overlay.spans.push(Span {
                    row: row + i,
                    col: col + width,
                    text: " ".to_string(),
                    hl,
                });
            }
        }
    }
}

/// One laid-out screen line: content chunks plus the cursor cell inside it.
#[derive(Clone, PartialEq, Debug)]
struct LayoutLine {
    chunks: Vec<(u64, String)>,
    cursor: Option<usize>,
}

impl LayoutLine {
    fn text(&self) -> String {
        self.chunks.iter().map(|(_, t)| t.as_str()).collect()
    }
}

fn chunks_width(chunks: &[(u64, String)]) -> usize {
    chunks.iter().map(|(_, t)| display_width(t)).sum()
}

/// Split chunked content on newlines. A single `msg_show` routinely carries a
/// multi-line message in one chunk.
fn chunks_to_lines(chunks: &[(u64, String)]) -> Vec<LayoutLine> {
    let mut lines = vec![LayoutLine {
        chunks: Vec::new(),
        cursor: None,
    }];
    for (attr, text) in chunks {
        let mut parts = text.split('\n');
        if let Some(first) = parts.next() {
            if !first.is_empty() {
                lines
                    .last_mut()
                    .expect("never empty")
                    .chunks
                    .push((*attr, first.to_string()));
            }
        }
        for part in parts {
            lines.push(LayoutLine {
                chunks: if part.is_empty() {
                    Vec::new()
                } else {
                    vec![(*attr, part.to_string())]
                },
                cursor: None,
            });
        }
    }
    lines
}

/// Split chunked content at cell `at`, returning the part before and the part
/// from that cell on. A double-width glyph straddling the boundary belongs to
/// neither side, so it becomes a space on each — half a glyph is not drawable
/// and dropping it would shift everything after it by a cell.
fn split_chunks_at_cell(
    chunks: &[(u64, String)],
    at: usize,
) -> (Vec<(u64, String)>, Vec<(u64, String)>) {
    let mut head = Vec::new();
    let mut tail = Vec::new();
    let mut cell = 0;
    for (attr, text) in chunks {
        let width = display_width(text);
        if cell >= at {
            tail.push((*attr, text.clone()));
        } else if cell + width <= at {
            head.push((*attr, text.clone()));
        } else {
            let (mut h, mut t) = (String::new(), String::new());
            let mut c = cell;
            for ch in text.chars() {
                let w = char_cells(ch);
                if c + w <= at {
                    h.push(ch);
                } else if c < at {
                    h.push(' ');
                    t.push(' ');
                } else {
                    t.push(ch);
                }
                c += w;
            }
            if !h.is_empty() {
                head.push((*attr, h));
            }
            if !t.is_empty() {
                tail.push((*attr, t));
            }
        }
        cell += width;
    }
    (head, tail)
}

/// The last glyph boundary at or before `cols`, so a wrapped row never ends in
/// half a wide character. A glyph wider than the entire screen has no such
/// boundary; that falls back to `cols`, which loses the glyph but keeps wrapping
/// making progress rather than looping forever on a row it can never fit.
fn break_cell(chunks: &[(u64, String)], cols: usize) -> usize {
    let mut cell = 0;
    for (_, text) in chunks {
        for c in text.chars() {
            let next = cell + char_cells(c);
            if next > cols {
                return if cell == 0 { cols } else { cell };
            }
            cell = next;
        }
    }
    cols
}

/// Break a line across as many screen rows as its content needs. A line that
/// owns the cursor is scrolled by `scroll_to_cursor` instead and passes through
/// untouched: wrapping the command line would move the caret off its own row.
fn wrap_line(line: LayoutLine, cols: usize) -> Vec<LayoutLine> {
    if line.cursor.is_some() || chunks_width(&line.chunks) <= cols {
        return vec![line];
    }
    let mut out = Vec::new();
    let mut rest = line.chunks;
    // Every split moves at least one cell into `head`, so this terminates for
    // `cols` >= 1, which `layout` guarantees by returning early on a zero-width
    // screen.
    while chunks_width(&rest) > cols {
        let (head, tail) = split_chunks_at_cell(&rest, break_cell(&rest, cols));
        out.push(LayoutLine {
            chunks: head,
            cursor: None,
        });
        rest = tail;
    }
    out.push(LayoutLine {
        chunks: rest,
        cursor: None,
    });
    out
}

/// Slide a line left until its cursor is on screen. Vim scrolls the command
/// line horizontally rather than wrapping it, so the end of a long `:%s/…/…/`
/// stays visible while it is being typed.
fn scroll_to_cursor(line: LayoutLine, cols: usize) -> LayoutLine {
    let Some(cursor) = line.cursor else {
        return line;
    };
    let offset = (cursor + 1).saturating_sub(cols);
    if offset == 0 {
        return line;
    }
    let (_, chunks) = split_chunks_at_cell(&line.chunks, offset);
    LayoutLine {
        chunks,
        cursor: Some(cursor - offset),
    }
}

/// The largest character boundary at or before `at`. `str::floor_char_boundary`
/// is still unstable, and `pos` arrives from Neovim as a byte offset that a
/// racing `cmdline_pos` can leave pointing inside a character.
fn floor_char_boundary(text: &str, at: usize) -> usize {
    let mut at = at.min(text.len());
    while at > 0 && !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// Insert `c` at a cell position within already-built chunks, splitting whatever
/// chunk straddles that cell.
fn insert_at_cell(chunks: Vec<(u64, String)>, cell: usize, c: char) -> Vec<(u64, String)> {
    let mut out = Vec::with_capacity(chunks.len() + 1);
    let mut at = 0;
    let mut done = false;
    for (attr, text) in chunks {
        let width = display_width(&text);
        if done || at + width < cell {
            at += width;
            out.push((attr, text));
            continue;
        }
        let mut head = String::new();
        let mut tail = String::new();
        let mut cursor = at;
        for ch in text.chars() {
            if cursor < cell {
                head.push(ch);
            } else {
                tail.push(ch);
            }
            cursor += char_cells(ch);
        }
        if !head.is_empty() {
            out.push((attr, head));
        }
        out.push((attr, c.to_string()));
        if !tail.is_empty() {
            out.push((attr, tail));
        }
        at += width;
        done = true;
    }
    if !done {
        out.push((0, c.to_string()));
    }
    out
}

/// Truncate or space-pad to exactly `width` cells.
fn pad_to(text: &str, width: usize) -> String {
    let mut out = String::new();
    let mut at = 0;
    for c in text.chars() {
        let w = char_cells(c);
        if at + w > width {
            break;
        }
        out.push(c);
        at += w;
    }
    for _ in at..width {
        out.push(' ');
    }
    out
}

/// Keep a box of `width` cells fully on screen when anchored at `col`.
fn anchor_row_col(col: usize, width: usize, cols: usize) -> usize {
    col.min(cols.saturating_sub(width))
}

/// Neovim reports "nothing selected" as -1, and an out-of-range index is not
/// something to index the item list with.
fn normalize_selection(selected: i64, count: usize) -> Option<usize> {
    let index = usize::try_from(selected).ok()?;
    (index < count).then_some(index)
}

/// Move `from` a fraction `t` of the way towards `to`, per channel.
fn blend(from: u32, to: u32, t: f32) -> u32 {
    let t = t.clamp(0.0, 1.0);
    let channel = |shift: u32| {
        let a = ((from >> shift) & 0xff) as f32;
        let b = ((to >> shift) & 0xff) as f32;
        ((a + (b - a) * t).round().clamp(0.0, 255.0) as u32) << shift
    };
    channel(16) | channel(8) | channel(0)
}

/// Parse `[[attr_id, text], …]` content. Tolerates bare strings, missing
/// attribute ids and non-array entries, all of which mean "draw what you can".
fn parse_chunks(value: Option<&Value>) -> Vec<(u64, String)> {
    let Some(entries) = value.and_then(Value::as_array) else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| {
            if let Some(text) = entry.as_str() {
                return Some((0, text.to_string()));
            }
            let parts = entry.as_array()?;
            let text = parts.get(1).and_then(Value::as_str)?;
            Some((
                parts.first().and_then(Value::as_u64).unwrap_or(0),
                text.to_string(),
            ))
        })
        .collect()
}

fn u(a: &[Value], i: usize) -> Option<u64> {
    a.get(i).and_then(Value::as_u64)
}

fn str_at(a: &[Value], i: usize) -> &str {
    a.get(i).and_then(Value::as_str).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(name: &str, args: Vec<Value>) -> RedrawEvent {
        (name.to_string(), args)
    }

    fn item(word: &str, kind: &str, menu: &str) -> Value {
        Value::Array(vec![
            Value::from(word),
            Value::from(kind),
            Value::from(menu),
            Value::from(""),
        ])
    }

    fn content(chunks: &[(u64, &str)]) -> Value {
        Value::Array(
            chunks
                .iter()
                .map(|(a, t)| Value::Array(vec![Value::from(*a), Value::from(*t)]))
                .collect(),
        )
    }

    fn popup_show(items: Vec<Value>, selected: i64, row: u64, col: u64) -> RedrawEvent {
        popup_show_on_grid(items, selected, row, col, 1)
    }

    fn popup_show_on_grid(
        items: Vec<Value>,
        selected: i64,
        row: u64,
        col: u64,
        grid: i64,
    ) -> RedrawEvent {
        ev(
            "popupmenu_show",
            vec![
                Value::Array(items),
                Value::from(selected),
                Value::from(row),
                Value::from(col),
                Value::from(grid),
            ],
        )
    }

    fn msg_show(text: &str) -> RedrawEvent {
        ev(
            "msg_show",
            vec![Value::from(""), content(&[(0, text)]), Value::from(false)],
        )
    }

    fn cmdline_show(text: &str, pos: u64, firstc: &str, level: u64) -> RedrawEvent {
        ev(
            "cmdline_show",
            vec![
                content(&[(0, text)]),
                Value::from(pos),
                Value::from(firstc),
                Value::from(""),
                Value::from(0u64),
                Value::from(level),
            ],
        )
    }

    /// Spans covering a row, ignoring the full-width background fill.
    fn text_spans_on(overlay: &Overlay, row: usize) -> Vec<&Span> {
        overlay
            .spans
            .iter()
            .filter(|s| s.row == row && s.text.trim() != "")
            .collect()
    }

    fn row_text(overlay: &Overlay, row: usize) -> String {
        overlay
            .spans
            .iter()
            .filter(|s| s.row == row && !(s.col == 0 && s.text.trim().is_empty()))
            .map(|s| s.text.as_str())
            .collect()
    }

    // ---- popupmenu state ---------------------------------------------------

    #[test]
    fn popupmenu_show_select_hide_is_a_full_cycle() {
        let mut ui = ExtUi::new();
        ui.apply(&[popup_show(
            vec![item("alpha", "v", ""), item("beta", "f", "")],
            0,
            3,
            10,
        )]);
        let menu = ui.popupmenu.as_ref().expect("shown");
        assert_eq!(menu.items.len(), 2);
        assert_eq!(menu.items[0].word, "alpha");
        assert_eq!(menu.items[1].kind, "f");
        assert_eq!(menu.selected, Some(0));
        assert_eq!((menu.row, menu.col), (3, 10));

        ui.apply(&[ev("popupmenu_select", vec![Value::from(1i64)])]);
        assert_eq!(ui.popupmenu.as_ref().unwrap().selected, Some(1));

        ui.apply(&[ev("popupmenu_hide", vec![])]);
        assert!(ui.popupmenu.is_none());
        assert!(ui.is_idle());
    }

    #[test]
    fn selection_of_minus_one_means_nothing_is_selected() {
        let mut ui = ExtUi::new();
        ui.apply(&[popup_show(
            vec![item("a", "", ""), item("b", "", "")],
            1,
            0,
            0,
        )]);
        assert_eq!(ui.popupmenu.as_ref().unwrap().selected, Some(1));
        ui.apply(&[ev("popupmenu_select", vec![Value::from(-1i64)])]);
        assert_eq!(ui.popupmenu.as_ref().unwrap().selected, None);
    }

    #[test]
    fn out_of_range_selection_is_dropped_rather_than_indexed() {
        let mut ui = ExtUi::new();
        ui.apply(&[popup_show(vec![item("a", "", "")], 7, 0, 0)]);
        assert_eq!(ui.popupmenu.as_ref().unwrap().selected, None);
    }

    #[test]
    fn select_without_a_visible_menu_is_ignored() {
        let mut ui = ExtUi::new();
        ui.apply(&[ev("popupmenu_select", vec![Value::from(2i64)])]);
        assert!(ui.popupmenu.is_none());
    }

    // ---- popupmenu layout --------------------------------------------------

    #[test]
    fn popupmenu_sits_under_its_anchor_and_marks_the_selection() {
        let mut ui = ExtUi::new();
        ui.apply(&[popup_show(
            vec![item("alpha", "", ""), item("beta", "", "")],
            1,
            2,
            4,
        )]);
        let overlay = ui.layout(40, 20);
        let first = overlay
            .spans
            .iter()
            .find(|s| s.row == 3)
            .expect("row below anchor");
        assert_eq!(first.col, 4);
        assert_eq!(first.text, "alpha");
        assert_eq!(first.hl, HlRef::Ui(UiHl::Pmenu));
        let second = overlay
            .spans
            .iter()
            .find(|s| s.row == 4)
            .expect("second item");
        assert_eq!(second.text, "beta ");
        assert_eq!(second.hl, HlRef::Ui(UiHl::PmenuSel));
    }

    #[test]
    fn popupmenu_flips_above_the_anchor_when_it_does_not_fit_below() {
        let mut ui = ExtUi::new();
        ui.apply(&[popup_show(
            vec![
                item("one", "", ""),
                item("two", "", ""),
                item("three", "", ""),
            ],
            0,
            8,
            0,
        )]);
        let overlay = ui.layout(20, 10);
        let rows: Vec<usize> = overlay.spans.iter().map(|s| s.row).collect();
        assert!(rows.contains(&5) && rows.contains(&7), "rows were {rows:?}");
        assert!(!rows.contains(&9), "must not run off the bottom: {rows:?}");
    }

    #[test]
    fn popupmenu_is_pulled_left_so_it_stays_on_screen() {
        let mut ui = ExtUi::new();
        ui.apply(&[popup_show(vec![item("wide_candidate", "", "")], 0, 0, 18)]);
        let overlay = ui.layout(20, 10);
        let span = overlay.spans.iter().find(|s| s.row == 1).unwrap();
        assert_eq!(span.col, 20 - display_width("wide_candidate"));
    }

    #[test]
    fn popupmenu_scrolls_to_keep_the_selection_visible() {
        let items: Vec<Value> = (0..30).map(|i| item(&format!("item{i}"), "", "")).collect();
        let mut ui = ExtUi::new();
        ui.apply(&[popup_show(items, 25, 0, 0)]);
        let overlay = ui.layout(40, 30);
        let labels: Vec<String> = overlay
            .spans
            .iter()
            .filter(|s| matches!(s.hl, HlRef::Ui(UiHl::Pmenu) | HlRef::Ui(UiHl::PmenuSel)))
            .map(|s| s.text.trim().to_string())
            .collect();
        assert_eq!(labels.len(), MAX_POPUP_ROWS);
        assert!(labels.contains(&"item25".to_string()), "{labels:?}");
        assert_eq!(labels.last().unwrap(), "item25");
        // A scrollbar column appears once the list is longer than the box.
        assert!(overlay
            .spans
            .iter()
            .any(|s| s.hl == HlRef::Ui(UiHl::PmenuThumb)));
    }

    #[test]
    fn popupmenu_label_carries_kind_and_menu() {
        let mut ui = ExtUi::new();
        ui.apply(&[popup_show(vec![item("push", "f", "vim.api")], 0, 0, 0)]);
        let overlay = ui.layout(40, 10);
        let span = overlay.spans.iter().find(|s| s.row == 1).unwrap();
        assert_eq!(span.text.trim_end(), "push f vim.api");
    }

    #[test]
    fn popupmenu_never_grows_back_across_its_own_anchor() {
        let items: Vec<Value> = (0..9).map(|i| item(&format!("item{i}"), "", "")).collect();
        let mut ui = ExtUi::new();
        // Nine items fit neither below row 5 (two rows left) nor above it (five),
        // so the taller side wins and the box has to stop at the anchor.
        ui.apply(&[popup_show(items, 0, 5, 0)]);
        let overlay = ui.layout(20, 8);
        let rows: Vec<usize> = overlay.spans.iter().map(|s| s.row).collect();
        assert!(!rows.is_empty(), "the menu must still be drawn");
        assert!(
            rows.iter().all(|&r| r < 5),
            "must not cover the anchor row or the cmdline: {rows:?}"
        );
    }

    #[test]
    fn popupmenu_stops_above_the_rows_the_bottom_surfaces_claimed() {
        let items: Vec<Value> = (0..10).map(|i| item(&format!("item{i}"), "", "")).collect();
        let mut ui = ExtUi::new();
        // A buffer-anchored completion menu near the top, with a message and a
        // command line already holding the last two rows.
        ui.apply(&[msg_show("written"), cmdline_show("w", 1, ":", 1)]);
        ui.apply(&[popup_show(items, 0, 2, 0)]);
        let overlay = ui.layout(20, 10);
        assert_eq!(overlay.reserved_rows, 2);

        let menu_rows: Vec<usize> = overlay
            .spans
            .iter()
            .filter(|s| matches!(s.hl, HlRef::Ui(UiHl::Pmenu) | HlRef::Ui(UiHl::PmenuSel)))
            .map(|s| s.row)
            .collect();
        assert!(!menu_rows.is_empty(), "the menu must still be drawn");
        assert!(
            menu_rows.iter().all(|&r| r < 8),
            "must not bury the message or the cmdline: {menu_rows:?}"
        );
        // And the surfaces underneath are still the ones on screen.
        assert_eq!(row_text(&overlay, 8), "written");
        assert_eq!(row_text(&overlay, 9), ":w");
    }

    #[test]
    fn wildmenu_anchors_to_the_row_the_cmdline_landed_on() {
        let mut ui = ExtUi::new();
        ui.apply(&[
            cmdline_show("e src/", 6, ":", 1),
            // Grid -1 is Neovim's way of saying "this menu belongs to the
            // command line", wherever that turned out to be.
            popup_show_on_grid(
                vec![item("ext_ui.rs", "", ""), item("gl.rs", "", "")],
                0,
                0,
                2,
                -1,
            ),
        ]);
        let overlay = ui.layout(30, 10);
        let labels: Vec<&Span> = overlay
            .spans
            .iter()
            .filter(|s| matches!(s.hl, HlRef::Ui(UiHl::Pmenu) | HlRef::Ui(UiHl::PmenuSel)))
            .collect();
        assert_eq!(labels.len(), 2);
        // The cmdline owns row 9, so the menu sits directly above it.
        assert_eq!(labels[0].row, 7);
        assert_eq!(labels[1].row, 8);
        assert_eq!(row_text(&overlay, 9), ":e src/");
    }

    // ---- cmdline -----------------------------------------------------------

    #[test]
    fn cmdline_show_pos_and_hide_track_one_level() {
        let mut ui = ExtUi::new();
        ui.apply(&[cmdline_show("wq", 2, ":", 1)]);
        let line = ui.active_cmdline().expect("open");
        assert_eq!(line.firstc, ":");
        assert_eq!(line.text(), "wq");
        assert_eq!(line.pos, 2);

        ui.apply(&[ev(
            "cmdline_pos",
            vec![Value::from(1u64), Value::from(1u64)],
        )]);
        assert_eq!(ui.active_cmdline().unwrap().pos, 1);

        ui.apply(&[ev("cmdline_hide", vec![Value::from(1u64)])]);
        assert!(ui.active_cmdline().is_none());
        assert!(ui.is_idle());
    }

    #[test]
    fn nested_cmdline_levels_stack_and_unwind_together() {
        let mut ui = ExtUi::new();
        ui.apply(&[
            cmdline_show("outer", 5, ":", 1),
            cmdline_show("inner", 5, "=", 2),
        ]);
        assert_eq!(ui.cmdlines.len(), 2);
        assert_eq!(ui.active_cmdline().unwrap().level, 2);

        ui.apply(&[ev("cmdline_hide", vec![Value::from(2u64)])]);
        assert_eq!(ui.cmdlines.len(), 1);
        assert_eq!(ui.active_cmdline().unwrap().text(), "outer");

        // Hiding the outer level takes any surviving inner level with it.
        ui.apply(&[cmdline_show("inner", 5, "=", 2)]);
        ui.apply(&[ev("cmdline_hide", vec![Value::from(1u64)])]);
        assert!(ui.cmdlines.is_empty());
    }

    #[test]
    fn redisplaying_a_level_replaces_it_instead_of_stacking() {
        let mut ui = ExtUi::new();
        ui.apply(&[cmdline_show("w", 1, ":", 1), cmdline_show("wq", 2, ":", 1)]);
        assert_eq!(ui.cmdlines.len(), 1);
        assert_eq!(ui.active_cmdline().unwrap().text(), "wq");
    }

    #[test]
    fn cmdline_lands_on_the_last_row_with_its_prefix_and_cursor() {
        let mut ui = ExtUi::new();
        ui.apply(&[cmdline_show("write", 5, ":", 1)]);
        let overlay = ui.layout(20, 6);
        assert_eq!(row_text(&overlay, 5), ":write");
        let cursor = overlay.cursor.expect("cmdline owns the cursor");
        assert_eq!((cursor.row, cursor.col), (5, 6));
        assert_eq!(cursor.ch, ' ');
        assert_eq!(overlay.reserved_rows, 1);
    }

    #[test]
    fn cmdline_cursor_counts_prompt_indent_and_wide_characters() {
        let mut ui = ExtUi::new();
        ui.apply(&[ev(
            "cmdline_show",
            vec![
                content(&[(0, "あい")]),
                // Two three-byte characters: byte offset 6 is the end of "あい".
                Value::from(6u64),
                Value::from(":"),
                Value::from("go"),
                Value::from(2u64),
                Value::from(1u64),
            ],
        )]);
        let overlay = ui.layout(40, 4);
        // ":" + "go" + two spaces of indent = 5 cells, then two wide chars.
        assert_eq!(overlay.cursor.unwrap().col, 5 + 4);
        assert_eq!(row_text(&overlay, 3), ":go  あい");
    }

    #[test]
    fn cmdline_cursor_reports_the_character_it_covers() {
        let mut ui = ExtUi::new();
        ui.apply(&[cmdline_show("set", 1, ":", 1)]);
        let overlay = ui.layout(20, 4);
        let cursor = overlay.cursor.unwrap();
        assert_eq!(cursor.col, 2);
        assert_eq!(cursor.ch, 'e');
    }

    #[test]
    fn special_char_is_drawn_at_the_cursor_without_entering_the_content() {
        let mut ui = ExtUi::new();
        ui.apply(&[cmdline_show("ab", 1, ":", 1)]);
        ui.apply(&[ev(
            "cmdline_special_char",
            vec![Value::from("^"), Value::from(false), Value::from(1u64)],
        )]);
        assert_eq!(ui.active_cmdline().unwrap().text(), "ab");
        let overlay = ui.layout(20, 4);
        assert_eq!(row_text(&overlay, 3), ":a^b");
        // A fresh frame for the level clears the pending literal.
        ui.apply(&[cmdline_show("a^b", 3, ":", 1)]);
        assert!(ui.active_cmdline().unwrap().special.is_none());
    }

    #[test]
    fn cmdline_block_lines_sit_above_the_active_cmdline() {
        let mut ui = ExtUi::new();
        ui.apply(&[
            cmdline_show("echo 1", 6, ":", 1),
            ev(
                "cmdline_block_show",
                vec![Value::Array(vec![content(&[(0, "function F()")])])],
            ),
            ev("cmdline_block_append", vec![content(&[(0, "  echo 1")])]),
        ]);
        let overlay = ui.layout(30, 10);
        assert_eq!(row_text(&overlay, 7), "function F()");
        assert_eq!(row_text(&overlay, 8), "  echo 1");
        assert_eq!(row_text(&overlay, 9), ":echo 1");

        ui.apply(&[ev("cmdline_block_hide", vec![])]);
        assert!(ui.cmdline_block.is_empty());
    }

    // ---- messages ----------------------------------------------------------

    #[test]
    fn messages_accumulate_replace_and_clear() {
        let mut ui = ExtUi::new();
        ui.apply(&[ev(
            "msg_show",
            vec![
                Value::from("echomsg"),
                content(&[(0, "first")]),
                Value::from(false),
            ],
        )]);
        ui.apply(&[ev(
            "msg_show",
            vec![
                Value::from("emsg"),
                content(&[(2, "second")]),
                Value::from(false),
            ],
        )]);
        assert_eq!(ui.messages.len(), 2);
        assert_eq!(ui.messages[1].kind, "emsg");
        assert_eq!(ui.messages[1].chunks[0], (2, "second".to_string()));

        ui.apply(&[ev(
            "msg_show",
            vec![
                Value::from("emsg"),
                content(&[(2, "third")]),
                Value::from(true),
            ],
        )]);
        assert_eq!(ui.messages.len(), 2);
        assert_eq!(ui.messages[1].chunks[0].1, "third");

        ui.apply(&[ev("msg_clear", vec![])]);
        assert!(ui.messages.is_empty());
    }

    #[test]
    fn append_extends_the_previous_message_in_place() {
        let mut ui = ExtUi::new();
        ui.apply(&[ev(
            "msg_show",
            vec![Value::from(""), content(&[(0, "part")]), Value::from(false)],
        )]);
        ui.apply(&[ev(
            "msg_show",
            vec![
                Value::from(""),
                content(&[(0, "-two")]),
                Value::from(false),
                Value::from(false),
                Value::from(true),
            ],
        )]);
        assert_eq!(ui.messages.len(), 1);
        assert_eq!(ui.messages[0].chunks.len(), 2);
    }

    #[test]
    fn retained_messages_are_bounded() {
        let mut ui = ExtUi::new();
        for i in 0..MAX_MESSAGES + 10 {
            ui.apply(&[ev(
                "msg_show",
                vec![
                    Value::from(""),
                    content(&[(0, &format!("m{i}"))]),
                    Value::from(false),
                ],
            )]);
        }
        assert_eq!(ui.messages.len(), MAX_MESSAGES);
        assert_eq!(ui.messages[0].chunks[0].1, "m10");
    }

    #[test]
    fn messages_stack_above_the_cmdline_newest_last() {
        let mut ui = ExtUi::new();
        ui.apply(&[
            ev(
                "msg_show",
                vec![
                    Value::from(""),
                    content(&[(0, "older")]),
                    Value::from(false),
                ],
            ),
            ev(
                "msg_show",
                vec![
                    Value::from(""),
                    content(&[(0, "newer")]),
                    Value::from(false),
                ],
            ),
            cmdline_show("q", 1, ":", 1),
        ]);
        let overlay = ui.layout(20, 8);
        assert_eq!(row_text(&overlay, 5), "older");
        assert_eq!(row_text(&overlay, 6), "newer");
        assert_eq!(row_text(&overlay, 7), ":q");
        assert_eq!(overlay.reserved_rows, 3);
    }

    #[test]
    fn a_multi_line_message_occupies_one_row_per_line() {
        let mut ui = ExtUi::new();
        ui.apply(&[ev(
            "msg_show",
            vec![
                Value::from(""),
                content(&[(0, "one\ntwo\nthree")]),
                Value::from(false),
            ],
        )]);
        let overlay = ui.layout(20, 10);
        assert_eq!(row_text(&overlay, 7), "one");
        assert_eq!(row_text(&overlay, 8), "two");
        assert_eq!(row_text(&overlay, 9), "three");
    }

    #[test]
    fn more_lines_than_rows_keeps_the_newest_and_never_overflows() {
        let mut ui = ExtUi::new();
        for i in 0..10 {
            ui.apply(&[ev(
                "msg_show",
                vec![
                    Value::from(""),
                    content(&[(0, &format!("line{i}"))]),
                    Value::from(false),
                ],
            )]);
        }
        ui.apply(&[cmdline_show("x", 1, ":", 1)]);
        let overlay = ui.layout(20, 3);
        assert!(overlay.spans.iter().all(|s| s.row < 3));
        assert_eq!(row_text(&overlay, 2), ":x");
        assert_eq!(row_text(&overlay, 0), "line8");
    }

    #[test]
    fn message_history_shows_then_clears() {
        let mut ui = ExtUi::new();
        ui.apply(&[ev(
            "msg_history_show",
            vec![Value::Array(vec![
                Value::Array(vec![Value::from("echomsg"), content(&[(0, "hist one")])]),
                Value::Array(vec![Value::from("emsg"), content(&[(0, "hist two")])]),
            ])],
        )]);
        assert!(ui.history_visible);
        assert_eq!(ui.history.len(), 2);
        let overlay = ui.layout(20, 6);
        assert_eq!(row_text(&overlay, 4), "hist one");
        assert_eq!(row_text(&overlay, 5), "hist two");

        ui.apply(&[ev("msg_history_clear", vec![])]);
        assert!(ui.history.is_empty());
        assert!(!ui.history_visible);
    }

    #[test]
    fn clearing_messages_also_dismisses_the_history_overlay() {
        let mut ui = ExtUi::new();
        ui.apply(&[ev(
            "msg_history_show",
            vec![Value::Array(vec![Value::Array(vec![
                Value::from(""),
                content(&[(0, "old")]),
            ])])],
        )]);
        ui.apply(&[ev("msg_clear", vec![])]);
        assert!(!ui.history_visible);
        assert!(ui.layout(20, 6).spans.is_empty());
    }

    #[test]
    fn showmode_and_ruler_share_the_last_row_when_no_cmdline_is_open() {
        let mut ui = ExtUi::new();
        ui.apply(&[
            ev("msg_showmode", vec![content(&[(0, "-- INSERT --")])]),
            ev("msg_ruler", vec![content(&[(0, "1,1")])]),
        ]);
        let overlay = ui.layout(40, 5);
        let spans = text_spans_on(&overlay, 4);
        assert_eq!(spans[0].text, "-- INSERT --");
        assert_eq!(spans[0].col, 0);
        let ruler = spans.last().unwrap();
        assert_eq!(ruler.text, "1,1");
        assert_eq!(ruler.col, 37);
        assert!(overlay.cursor.is_none(), "the grid still owns the cursor");
    }

    #[test]
    fn an_open_cmdline_takes_the_row_from_the_mode_line() {
        let mut ui = ExtUi::new();
        ui.apply(&[
            ev("msg_showmode", vec![content(&[(0, "-- INSERT --")])]),
            cmdline_show("s/a/b", 5, ":", 1),
        ]);
        let overlay = ui.layout(40, 5);
        assert_eq!(row_text(&overlay, 4), ":s/a/b");
        assert_eq!(overlay.reserved_rows, 1);
    }

    // ---- fitting content to the window -------------------------------------

    #[test]
    fn a_message_wider_than_the_window_wraps_instead_of_losing_its_tail() {
        let mut ui = ExtUi::new();
        ui.apply(&[msg_show("0123456789abcdefghijkl")]);
        let overlay = ui.layout(10, 5);
        assert_eq!(row_text(&overlay, 2), "0123456789");
        assert_eq!(row_text(&overlay, 3), "abcdefghij");
        assert_eq!(row_text(&overlay, 4), "kl");
        assert_eq!(overlay.reserved_rows, 3);
    }

    #[test]
    fn wrapping_keeps_attributes_and_never_splits_a_wide_character() {
        let mut ui = ExtUi::new();
        ui.apply(&[ev(
            "msg_show",
            vec![
                Value::from(""),
                content(&[(0, "あい"), (7, "うえ")]),
                Value::from(false),
            ],
        )]);
        // Five columns hold two wide characters; the third has to move down
        // whole rather than be halved across the break.
        let overlay = ui.layout(5, 4);
        assert_eq!(row_text(&overlay, 2), "あい");
        assert_eq!(row_text(&overlay, 3), "うえ");
        let carried = text_spans_on(&overlay, 3);
        assert_eq!(carried[0].hl, HlRef::Attr(7));
    }

    #[test]
    fn a_command_line_wider_than_the_window_scrolls_to_follow_the_cursor() {
        let mut ui = ExtUi::new();
        ui.apply(&[cmdline_show("0123456789abcde", 15, ":", 1)]);
        let overlay = ui.layout(10, 3);
        // The caret is at the end, so the end is what stays on screen.
        assert_eq!(row_text(&overlay, 2), "6789abcde");
        let cursor = overlay.cursor.expect("cmdline owns the cursor");
        assert_eq!((cursor.row, cursor.col), (2, 9));

        // Move the caret back to the front and the head comes into view again.
        ui.apply(&[ev(
            "cmdline_pos",
            vec![Value::from(0u64), Value::from(1u64)],
        )]);
        let overlay = ui.layout(10, 3);
        assert_eq!(row_text(&overlay, 2), ":0123456789abcde");
        assert_eq!(overlay.cursor.unwrap().col, 1);
    }

    #[test]
    fn a_showmode_longer_than_the_window_is_truncated_not_overrun() {
        let mut ui = ExtUi::new();
        ui.apply(&[ev(
            "msg_showmode",
            vec![content(&[(0, "-- VISUAL BLOCK --")])],
        )]);
        let overlay = ui.layout(10, 3);
        assert_eq!(row_text(&overlay, 2), "-- VISUAL ");
    }

    #[test]
    fn showcmd_sits_a_fixed_gap_to_the_left_of_the_ruler() {
        let mut ui = ExtUi::new();
        ui.apply(&[
            ev("msg_showcmd", vec![content(&[(0, "2d")])]),
            ev("msg_ruler", vec![content(&[(0, "1,1")])]),
        ]);
        let overlay = ui.layout(40, 5);
        let spans = text_spans_on(&overlay, 4);
        assert_eq!(spans[0].text, "2d");
        assert_eq!(spans[0].col, 40 - 3 - SHOWCMD_GAP);
        assert_eq!(spans[1].text, "1,1");
        assert_eq!(spans[1].col, 37);
    }

    #[test]
    fn every_reserved_row_is_covered_edge_to_edge() {
        // The renderer stops painting the grid at `reserved_rows`, which is only
        // sound because the overlay fills each of those rows itself.
        let mut ui = ExtUi::new();
        ui.apply(&[msg_show("short"), cmdline_show("q", 1, ":", 1)]);
        let overlay = ui.layout(20, 6);
        assert_eq!(overlay.reserved_rows, 2);
        for row in 6 - overlay.reserved_rows..6 {
            assert!(
                overlay
                    .spans
                    .iter()
                    .any(|s| s.row == row && s.col == 0 && display_width(&s.text) == 20),
                "row {row} is not fully covered"
            );
        }
    }

    // ---- malformed and partial events -------------------------------------

    #[test]
    fn malformed_events_leave_the_previous_state_intact() {
        let mut ui = ExtUi::new();
        ui.apply(&[
            popup_show(vec![item("keep", "", "")], 0, 1, 1),
            cmdline_show("ok", 2, ":", 1),
        ]);

        ui.apply(&[
            // No arguments at all.
            ev("popupmenu_show", vec![]),
            ev("cmdline_show", vec![]),
            ev("cmdline_pos", vec![]),
            ev("msg_show", vec![]),
            ev("msg_history_show", vec![]),
            ev("cmdline_special_char", vec![]),
            ev("hl_group_set", vec![]),
            // Right arity, wrong types.
            ev(
                "popupmenu_show",
                vec![Value::from("not-a-list"), Value::from(0i64)],
            ),
            ev(
                "cmdline_show",
                vec![
                    Value::Nil,
                    Value::Nil,
                    Value::Nil,
                    Value::Nil,
                    Value::Nil,
                    Value::Nil,
                ],
            ),
            ev("popupmenu_select", vec![Value::from("nope")]),
            ev("msg_showmode", vec![Value::from(3u64)]),
        ]);

        assert_eq!(ui.popupmenu.as_ref().unwrap().items[0].word, "keep");
        assert_eq!(ui.active_cmdline().unwrap().text(), "ok");
        assert!(ui.messages.is_empty());
        assert!(ui.showmode.is_empty());
        assert!(!ui.layout(20, 5).spans.is_empty());
    }

    #[test]
    fn malformed_cmdline_hide_and_block_events_leave_the_stack_intact() {
        let mut ui = ExtUi::new();
        ui.apply(&[
            cmdline_show("echo 1", 6, ":", 1),
            ev(
                "cmdline_block_show",
                vec![Value::Array(vec![content(&[(0, "function F()")])])],
            ),
        ]);

        ui.apply(&[
            // No level to unwind to must not tear down a command line the user
            // is still typing into.
            ev("cmdline_hide", vec![]),
            ev("cmdline_hide", vec![Value::Nil]),
            ev("cmdline_hide", vec![Value::from(-1i64)]),
            ev("cmdline_block_show", vec![Value::from("not-a-list")]),
            // A mistyped append must not add a blank line to the block.
            ev("cmdline_block_append", vec![]),
            ev("cmdline_block_append", vec![Value::Nil]),
        ]);

        assert_eq!(ui.active_cmdline().unwrap().text(), "echo 1");
        assert_eq!(ui.cmdline_block.len(), 1);
        assert_eq!(ui.cmdline_block[0], vec![(0, "function F()".to_string())]);
    }

    #[test]
    fn one_message_cannot_grow_without_bound_through_append() {
        let mut ui = ExtUi::new();
        ui.apply(&[msg_show("start")]);
        for i in 0..MAX_MESSAGE_CHUNKS + 10 {
            ui.apply(&[ev(
                "msg_show",
                vec![
                    Value::from(""),
                    content(&[(0, &format!("+{i}"))]),
                    Value::from(false),
                    Value::from(false),
                    Value::from(true),
                ],
            )]);
        }
        assert_eq!(ui.messages.len(), 1);
        assert_eq!(ui.messages[0].chunks.len(), MAX_MESSAGE_CHUNKS);
        // The newest text is the text that survives.
        let last = &ui.messages[0].chunks.last().unwrap().1;
        assert_eq!(last, &format!("+{}", MAX_MESSAGE_CHUNKS + 9));
    }

    #[test]
    fn partial_popupmenu_items_fall_back_to_empty_fields() {
        let mut ui = ExtUi::new();
        ui.apply(&[popup_show(
            vec![
                Value::Array(vec![Value::from("only-word")]),
                Value::from("not-an-item"),
                Value::Array(vec![Value::from(7u64), Value::from("kind")]),
            ],
            0,
            0,
            0,
        )]);
        let menu = ui.popupmenu.as_ref().unwrap();
        assert_eq!(menu.items.len(), 3);
        assert_eq!(menu.items[0].word, "only-word");
        assert_eq!(menu.items[0].kind, "");
        assert_eq!(menu.items[1], PopupItem::default());
        // A non-string word is dropped, but the item keeps its place so the
        // selection index Neovim sends still refers to the right row.
        assert_eq!(menu.items[2].word, "");
        assert_eq!(menu.items[2].kind, "kind");
        // Layout must survive a menu whose widest label is empty.
        assert!(!ui.layout(20, 5).spans.is_empty());
    }

    #[test]
    fn content_chunks_tolerate_bare_strings_and_missing_attributes() {
        let mut ui = ExtUi::new();
        ui.apply(&[ev(
            "msg_show",
            vec![
                Value::from(""),
                Value::Array(vec![
                    Value::from("bare"),
                    Value::Array(vec![Value::from(4u64), Value::from("attr")]),
                    Value::Array(vec![Value::from(9u64)]),
                    Value::Nil,
                ]),
                Value::from(false),
            ],
        )]);
        assert_eq!(
            ui.messages[0].chunks,
            vec![(0, "bare".to_string()), (4, "attr".to_string())]
        );
    }

    #[test]
    fn a_zero_sized_screen_produces_no_spans() {
        let mut ui = ExtUi::new();
        ui.apply(&[
            cmdline_show("x", 1, ":", 1),
            popup_show(vec![item("a", "", "")], 0, 0, 0),
        ]);
        assert_eq!(ui.layout(0, 0), Overlay::default());
        assert_eq!(ui.layout(10, 0), Overlay::default());
        assert_eq!(ui.layout(0, 10), Overlay::default());
    }

    #[test]
    fn cmdline_pos_past_the_end_does_not_split_a_character() {
        let mut ui = ExtUi::new();
        ui.apply(&[ev(
            "cmdline_show",
            vec![
                content(&[(0, "あ")]),
                // A byte offset inside the multibyte character, then past it.
                Value::from(1u64),
                Value::from(":"),
                Value::from(""),
                Value::from(0u64),
                Value::from(1u64),
            ],
        )]);
        let overlay = ui.layout(10, 3);
        // An offset inside the character floors to the boundary before it, so
        // the caret lands on the character rather than skipping past the line.
        assert_eq!(overlay.cursor.unwrap().col, 1);
        assert_eq!(overlay.cursor.unwrap().ch, 'あ');
        ui.apply(&[ev(
            "cmdline_pos",
            vec![Value::from(99u64), Value::from(1u64)],
        )]);
        assert_eq!(ui.layout(10, 3).cursor.unwrap().col, 3);
    }

    // ---- colour resolution -------------------------------------------------

    fn grid_with_hl(id: u64, fg: u32, bg: u32) -> Grid {
        let mut grid = Grid::new(4, 1);
        grid.apply(&[(
            "hl_attr_define".to_string(),
            vec![
                Value::from(id),
                Value::Map(vec![
                    (Value::from("foreground"), Value::from(fg)),
                    (Value::from("background"), Value::from(bg)),
                ]),
                Value::Map(vec![]),
                Value::Array(vec![]),
            ],
        )]);
        grid
    }

    #[test]
    fn ui_groups_resolve_through_hl_group_set() {
        let grid = grid_with_hl(5, 0x112233, 0x445566);
        let mut ui = ExtUi::new();
        ui.apply(&[ev(
            "hl_group_set",
            vec![Value::from("PmenuSel"), Value::from(5u64)],
        )]);
        assert_eq!(
            ui.colors(&grid, HlRef::Ui(UiHl::PmenuSel)),
            (0x112233, 0x445566)
        );
    }

    #[test]
    fn unmapped_ui_groups_fall_back_to_a_shade_of_the_default_background() {
        let grid = Grid::new(4, 1);
        let ui = ExtUi::new();
        let (fg, pmenu_bg) = ui.colors(&grid, HlRef::Ui(UiHl::Pmenu));
        let (_, sel_bg) = ui.colors(&grid, HlRef::Ui(UiHl::PmenuSel));
        assert_eq!(fg, grid.default_fg);
        assert_ne!(pmenu_bg, grid.default_bg, "the box must be distinguishable");
        assert_ne!(
            sel_bg, pmenu_bg,
            "the selection must stand out from the box"
        );
    }

    #[test]
    fn attribute_spans_go_through_the_grid_highlight_table() {
        let grid = grid_with_hl(3, 0xaabbcc, 0x010203);
        let ui = ExtUi::new();
        assert_eq!(ui.colors(&grid, HlRef::Attr(3)), (0xaabbcc, 0x010203));
        assert_eq!(
            ui.colors(&grid, HlRef::Attr(0)),
            (grid.default_fg, grid.default_bg)
        );
    }

    #[test]
    fn blend_moves_between_the_endpoints() {
        assert_eq!(blend(0x000000, 0xffffff, 0.0), 0x000000);
        assert_eq!(blend(0x000000, 0xffffff, 1.0), 0xffffff);
        assert_eq!(blend(0x000000, 0xffffff, 0.5), 0x808080);
        assert_eq!(blend(0x102030, 0x102030, 0.7), 0x102030);
    }

    // ---- helpers -----------------------------------------------------------

    #[test]
    fn pad_to_truncates_on_cell_boundaries() {
        assert_eq!(pad_to("ab", 4), "ab  ");
        assert_eq!(pad_to("abcdef", 3), "abc");
        // A wide character that would only half-fit is dropped entirely.
        assert_eq!(pad_to("あい", 3), "あ ");
        assert_eq!(pad_to("", 2), "  ");
    }

    #[test]
    fn char_at_cell_handles_wide_characters_and_overruns() {
        assert_eq!(char_at_cell("abc", 1), 'b');
        assert_eq!(char_at_cell("あb", 0), 'あ');
        assert_eq!(char_at_cell("あb", 1), ' ');
        assert_eq!(char_at_cell("あb", 2), 'b');
        assert_eq!(char_at_cell("ab", 9), ' ');
    }
}
