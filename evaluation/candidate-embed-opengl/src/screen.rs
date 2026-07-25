//! Every grid Neovim gives us, where each one sits, and the flat screen that
//! results from stacking them.
//!
//! With `ext_multigrid` nvim stops drawing windows into one screen-sized grid
//! and hands out a grid per window instead, plus placement events saying where
//! each one goes. Compositing them back into a single cell surface keeps the
//! renderer unchanged: it still draws one grid, and a session without splits or
//! floats composes to exactly the global grid it would have received anyway.
//!
//! Nothing here decides layout. nvim owns sizes and positions; this only
//! stacks, clips, and orders what it is told.

use std::collections::HashMap;

use rmpv::Value;

use crate::grid::{Cell, Grid, Hl, Styles};
use crate::nvim::RedrawEvent;

/// nvim's outermost grid. It exists either way: without `ext_multigrid` it is
/// the whole screen, with it the tabline, statuslines, separators and the
/// message area are still drawn on it and windows are composited over it.
pub const GLOBAL_GRID: u64 = 1;

/// Split windows sit below every float. nvim gives floats a default `zindex` of
/// 50 and the message grid 200; both may override it in the event.
const WINDOW_ZINDEX: i64 = 0;
const FLOAT_DEFAULT_ZINDEX: i64 = 50;
const MESSAGE_DEFAULT_ZINDEX: i64 = 200;

/// A float may be anchored to another float. Bounded so a malformed chain can
/// never spin.
const ANCHOR_DEPTH: usize = 16;

/// Which corner of a float `win_float_pos` positions.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Anchor {
    NorthWest,
    NorthEast,
    SouthWest,
    SouthEast,
}

impl Anchor {
    fn parse(s: &str) -> Self {
        match s {
            "NE" => Anchor::NorthEast,
            "SW" => Anchor::SouthWest,
            "SE" => Anchor::SouthEast,
            _ => Anchor::NorthWest,
        }
    }
}

/// Where nvim put a grid.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Placement {
    /// `win_pos`: a split, at a fixed cell of the global grid, with the size the
    /// window occupies there. The grid itself may be larger; the surplus is not
    /// on screen.
    Window { row: i64, col: i64, width: usize, height: usize },
    /// `win_float_pos`: anchored to another grid, possibly at fractional cells
    /// and possibly off the edge of the screen.
    Float { anchor: Anchor, anchor_grid: u64, row: f64, col: f64, zindex: i64 },
    /// `msg_set_pos`: the message grid, pinned to a row of the global grid.
    Message { row: i64, zindex: i64 },
    /// `win_external_pos`: nvim asks for a separate OS window for this grid.
    /// This UI owns one window, so there is nowhere to put it and it is left
    /// off the screen rather than drawn in the wrong place.
    External,
}

#[derive(Clone, Copy)]
pub struct Window {
    pub placement: Placement,
    pub hidden: bool,
    /// Placement order, used only to break ties between equal z-indexes so that
    /// the composed screen is a function of the event sequence.
    seq: u64,
}

pub struct Screen {
    /// Highlights and default colours, which nvim defines once for every grid.
    styles: Styles,
    /// Set when nvim says the frame is complete and ready to present.
    pub flushed: bool,
    grids: HashMap<u64, Grid>,
    windows: HashMap<u64, Window>,
    /// The flattened result the renderer draws.
    composed: Grid,
    cursor_grid: u64,
    /// Cursor position within `cursor_grid`, as nvim reported it.
    cursor_cell: (usize, usize),
    /// Cursor position on the composed screen, absent when its grid is not on
    /// screen at all.
    cursor: Option<(usize, usize)>,
    seq: u64,
}

impl Screen {
    pub fn new(cols: usize, rows: usize) -> Self {
        let mut grids = HashMap::new();
        grids.insert(GLOBAL_GRID, Grid::new(cols, rows));
        Self {
            styles: Styles::new(),
            flushed: false,
            grids,
            windows: HashMap::new(),
            composed: Grid::new(cols, rows),
            cursor_grid: GLOBAL_GRID,
            cursor_cell: (0, 0),
            cursor: Some((0, 0)),
            seq: 0,
        }
    }

    pub fn cols(&self) -> usize {
        self.composed.cols
    }

    pub fn rows(&self) -> usize {
        self.composed.rows
    }

    /// A cell of the composed screen.
    pub fn cell(&self, row: usize, col: usize) -> Cell {
        self.composed.cell(row, col)
    }

    /// Where the cursor is on the composed screen, if its grid is visible.
    pub fn cursor(&self) -> Option<(usize, usize)> {
        self.cursor
    }

    pub fn style(&self, hl_id: u64) -> Hl {
        self.styles.style(hl_id)
    }

    pub fn colors(&self, hl_id: u64) -> (u32, u32) {
        self.styles.colors(hl_id)
    }

    pub fn decoration_color(&self, hl_id: u64) -> u32 {
        self.styles.decoration_color(hl_id)
    }

    /// The placement of a grid, for tests and reporting.
    pub fn window(&self, grid: u64) -> Option<Window> {
        self.windows.get(&grid).copied()
    }

    /// The size nvim gave a grid, which is not always the extent shown.
    pub fn grid_size(&self, grid: u64) -> Option<(usize, usize)> {
        self.grids.get(&grid).map(|g| (g.cols, g.rows))
    }

    /// The grid ids nvim has created and not destroyed.
    pub fn grid_ids(&self) -> Vec<u64> {
        let mut ids: Vec<u64> = self.grids.keys().copied().collect();
        ids.sort_unstable();
        ids
    }

    pub fn apply(&mut self, events: &[RedrawEvent]) {
        for (name, args) in events {
            match name.as_str() {
                "grid_resize" => self.ev_grid_resize(args),
                "default_colors_set" => self.ev_default_colors(args),
                "hl_attr_define" => self.ev_hl_attr_define(args),
                "grid_line" => self.ev_grid_line(args),
                "grid_clear" => self.ev_grid_clear(args),
                "grid_scroll" => self.ev_grid_scroll(args),
                "grid_cursor_goto" => self.ev_cursor_goto(args),
                "grid_destroy" => self.ev_grid_destroy(args),
                "win_pos" => self.ev_win_pos(args),
                "win_float_pos" => self.ev_win_float_pos(args),
                "win_external_pos" => self.ev_win_external_pos(args),
                "win_hide" => self.ev_win_hide(args),
                "win_close" => self.ev_win_close(args),
                "msg_set_pos" => self.ev_msg_set_pos(args),
                "flush" => self.flushed = true,
                _ => {}
            }
        }
        self.compose();
    }

    fn place(&mut self, grid: u64, placement: Placement) {
        let seq = self.seq;
        self.seq += 1;
        self.windows.insert(grid, Window { placement, hidden: false, seq });
    }

    fn ev_grid_resize(&mut self, a: &[Value]) {
        let (Some(id), Some(w), Some(h)) = (u(a, 0), u(a, 1), u(a, 2)) else { return };
        self.grids
            .entry(id)
            .or_insert_with(|| Grid::new(0, 0))
            .resize(w as usize, h as usize);
    }

    fn ev_default_colors(&mut self, a: &[Value]) {
        if let Some(fg) = u(a, 0) {
            if fg != u64::MAX {
                self.styles.default_fg = fg as u32;
            }
        }
        if let Some(bg) = u(a, 1) {
            if bg != u64::MAX {
                self.styles.default_bg = bg as u32;
            }
        }
    }

    fn ev_hl_attr_define(&mut self, a: &[Value]) {
        let Some(id) = u(a, 0) else { return };
        let mut hl = Hl::default();
        if let Some(Value::Map(m)) = a.get(1) {
            for (k, v) in m {
                match k.as_str().unwrap_or("") {
                    "foreground" => hl.fg = v.as_u64().map(|x| x as u32),
                    "background" => hl.bg = v.as_u64().map(|x| x as u32),
                    "special" => hl.special = v.as_u64().map(|x| x as u32),
                    "reverse" => hl.reverse = v.as_bool().unwrap_or(false),
                    "bold" => hl.bold = v.as_bool().unwrap_or(false),
                    "italic" => hl.italic = v.as_bool().unwrap_or(false),
                    "underline" => hl.underline = v.as_bool().unwrap_or(false),
                    "undercurl" => hl.undercurl = v.as_bool().unwrap_or(false),
                    "underdouble" => hl.underdouble = v.as_bool().unwrap_or(false),
                    "underdotted" => hl.underdotted = v.as_bool().unwrap_or(false),
                    "underdashed" => hl.underdashed = v.as_bool().unwrap_or(false),
                    "strikethrough" => hl.strikethrough = v.as_bool().unwrap_or(false),
                    _ => {}
                }
            }
        }
        self.styles.hls.insert(id, hl);
    }

    /// `grid_line` is `[grid, row, col_start, cells, wrap]`, where each entry of
    /// `cells` is `[text]`, `[text, hl_id]` or `[text, hl_id, repeat]`. Omitted
    /// hl_id means "same as the previous cell in this run".
    fn ev_grid_line(&mut self, a: &[Value]) {
        let (Some(id), Some(row), Some(col0)) = (u(a, 0), u(a, 1), u(a, 2)) else { return };
        let Some(cells) = a.get(3).and_then(|v| v.as_array()) else { return };
        let Some(grid) = self.grids.get_mut(&id) else { return };
        let row = row as usize;
        if row >= grid.rows {
            return;
        }
        let mut col = col0 as usize;
        let mut hl: u64 = 0;
        for c in cells {
            let Some(parts) = c.as_array() else { continue };
            let text = parts.first().and_then(|t| t.as_str()).unwrap_or("");
            if let Some(h) = parts.get(1).and_then(|h| h.as_u64()) {
                hl = h;
            }
            let repeat = parts.get(2).and_then(|r| r.as_u64()).unwrap_or(1);
            // An empty string is the trailing half of a double-width glyph; nvim
            // still owns that cell, so keep it blank rather than skipping it.
            let ch = text.chars().next().unwrap_or(' ');
            for _ in 0..repeat {
                if col >= grid.cols {
                    break;
                }
                grid.set(row, col, Cell { ch, hl });
                col += 1;
            }
        }
    }

    fn ev_grid_clear(&mut self, a: &[Value]) {
        if let Some(grid) = u(a, 0).and_then(|id| self.grids.get_mut(&id)) {
            grid.clear();
        }
    }

    fn ev_grid_scroll(&mut self, a: &[Value]) {
        let (Some(id), Some(top), Some(bot), Some(left), Some(right), Some(rows)) =
            (u(a, 0), u(a, 1), u(a, 2), u(a, 3), u(a, 4), i(a, 5))
        else {
            return;
        };
        if let Some(grid) = self.grids.get_mut(&id) {
            grid.scroll(top as usize, bot as usize, left as usize, right as usize, rows);
        }
    }

    fn ev_cursor_goto(&mut self, a: &[Value]) {
        let (Some(id), Some(r), Some(c)) = (u(a, 0), u(a, 1), u(a, 2)) else { return };
        self.cursor_grid = id;
        self.cursor_cell = (r as usize, c as usize);
    }

    fn ev_grid_destroy(&mut self, a: &[Value]) {
        if let Some(id) = u(a, 0) {
            self.grids.remove(&id);
            self.windows.remove(&id);
        }
    }

    /// `win_pos` is `[grid, win, start_row, start_col, width, height]`.
    fn ev_win_pos(&mut self, a: &[Value]) {
        let (Some(id), Some(row), Some(col), Some(width), Some(height)) =
            (u(a, 0), u(a, 2), u(a, 3), u(a, 4), u(a, 5))
        else {
            return;
        };
        self.place(
            id,
            Placement::Window {
                row: row as i64,
                col: col as i64,
                width: width as usize,
                height: height as usize,
            },
        );
    }

    /// `win_float_pos` is
    /// `[grid, win, anchor, anchor_grid, anchor_row, anchor_col, focusable]`,
    /// with a trailing `zindex` on newer Neovim.
    fn ev_win_float_pos(&mut self, a: &[Value]) {
        let (Some(id), Some(anchor), Some(anchor_grid), Some(row), Some(col)) = (
            u(a, 0),
            a.get(2).and_then(|v| v.as_str()),
            u(a, 3),
            f(a, 4),
            f(a, 5),
        ) else {
            return;
        };
        self.place(
            id,
            Placement::Float {
                anchor: Anchor::parse(anchor),
                anchor_grid,
                row,
                col,
                zindex: i(a, 7).unwrap_or(FLOAT_DEFAULT_ZINDEX),
            },
        );
    }

    fn ev_win_external_pos(&mut self, a: &[Value]) {
        if let Some(id) = u(a, 0) {
            self.place(id, Placement::External);
        }
    }

    fn ev_win_hide(&mut self, a: &[Value]) {
        if let Some(win) = u(a, 0).and_then(|id| self.windows.get_mut(&id)) {
            win.hidden = true;
        }
    }

    fn ev_win_close(&mut self, a: &[Value]) {
        if let Some(id) = u(a, 0) {
            self.windows.remove(&id);
        }
    }

    /// `msg_set_pos` is `[grid, row, scrolled, sep_char]`, with a trailing
    /// `zindex` on newer Neovim.
    fn ev_msg_set_pos(&mut self, a: &[Value]) {
        let (Some(id), Some(row)) = (u(a, 0), u(a, 1)) else { return };
        self.place(
            id,
            Placement::Message {
                row: row as i64,
                zindex: i(a, 4).unwrap_or(MESSAGE_DEFAULT_ZINDEX),
            },
        );
    }

    /// The absolute top-left of a grid in screen cells, following float anchors
    /// through the grid they are anchored to.
    fn origin(&self, id: u64, depth: usize) -> Option<(f64, f64)> {
        if id == GLOBAL_GRID {
            return Some((0.0, 0.0));
        }
        if depth == 0 {
            return None;
        }
        match self.windows.get(&id)?.placement {
            Placement::Window { row, col, .. } => Some((row as f64, col as f64)),
            Placement::Message { row, .. } => Some((row as f64, 0.0)),
            Placement::Float { anchor, anchor_grid, row, col, .. } => {
                let (anchor_row, anchor_col) = self.origin(anchor_grid, depth - 1)?;
                let grid = self.grids.get(&id)?;
                let (h, w) = (grid.rows as f64, grid.cols as f64);
                let (r, c) = (anchor_row + row, anchor_col + col);
                Some(match anchor {
                    Anchor::NorthWest => (r, c),
                    Anchor::NorthEast => (r, c - w),
                    Anchor::SouthWest => (r - h, c),
                    Anchor::SouthEast => (r - h, c - w),
                })
            }
            Placement::External => None,
        }
    }

    /// The screen rectangle a placed grid occupies: its origin plus the extent
    /// nvim gave it (for a split) or its own size (for a float or the message
    /// grid). Not yet clipped — it may stick out of the screen on any side.
    fn rect(&self, id: u64) -> Option<(i64, i64, usize, usize, i64)> {
        let win = self.windows.get(&id)?;
        if win.hidden {
            return None;
        }
        let grid = self.grids.get(&id)?;
        let (row, col) = self.origin(id, ANCHOR_DEPTH)?;
        let (width, height, z) = match win.placement {
            Placement::Window { width, height, .. } => (width, height, WINDOW_ZINDEX),
            Placement::Float { zindex, .. } => (grid.cols, grid.rows, zindex),
            Placement::Message { zindex, .. } => (grid.cols, grid.rows, zindex),
            Placement::External => return None,
        };
        Some((row.floor() as i64, col.floor() as i64, width, height, z))
    }

    /// Stack every visible grid into one screen-sized surface, bottom up.
    fn compose(&mut self) {
        let (cols, rows) = match self.grids.get(&GLOBAL_GRID) {
            Some(global) => (global.cols, global.rows),
            None => (self.composed.cols, self.composed.rows),
        };
        // Taking the surface out keeps the source grids borrowable while it is
        // written; it goes straight back at the end.
        let mut composed = std::mem::take(&mut self.composed);
        composed.resize(cols, rows);
        composed.clear();

        let mut order: Vec<(i64, u64, u64)> = self
            .windows
            .keys()
            .filter(|id| **id != GLOBAL_GRID)
            .filter_map(|id| {
                let (_, _, _, _, z) = self.rect(*id)?;
                Some((z, self.windows[id].seq, *id))
            })
            .collect();
        order.sort_unstable();

        blit(&mut composed, self.grids.get(&GLOBAL_GRID), 0, 0, cols, rows);
        for (_, _, id) in order {
            let Some((row, col, width, height, _)) = self.rect(id) else { continue };
            blit(&mut composed, self.grids.get(&id), row, col, width, height);
        }

        self.composed = composed;
        self.cursor = self.cursor_position();
    }

    fn cursor_position(&self) -> Option<(usize, usize)> {
        let (row, col) = self.cursor_cell;
        let (origin_row, origin_col, width, height) = if self.cursor_grid == GLOBAL_GRID {
            (0, 0, self.composed.cols, self.composed.rows)
        } else {
            let (r, c, w, h, _) = self.rect(self.cursor_grid)?;
            (r, c, w, h)
        };
        // A cursor past the window's visible extent is not on screen even when
        // its grid still holds a cell there.
        if row >= height || col >= width {
            return None;
        }
        let (row, col) = (origin_row + row as i64, origin_col + col as i64);
        if row < 0 || col < 0 || row >= self.composed.rows as i64 || col >= self.composed.cols as i64
        {
            return None;
        }
        Some((row as usize, col as usize))
    }
}

/// Copy a grid onto the screen at `(row, col)`, clipped to both the requested
/// extent and the screen. Cells outside the screen are dropped, not wrapped.
fn blit(
    composed: &mut Grid,
    source: Option<&Grid>,
    row: i64,
    col: i64,
    width: usize,
    height: usize,
) {
    let Some(source) = source else { return };
    for r in 0..height.min(source.rows) {
        let dst_row = row + r as i64;
        if dst_row < 0 || dst_row >= composed.rows as i64 {
            continue;
        }
        for c in 0..width.min(source.cols) {
            let dst_col = col + c as i64;
            if dst_col < 0 || dst_col >= composed.cols as i64 {
                continue;
            }
            composed.set(dst_row as usize, dst_col as usize, source.cell(r, c));
        }
    }
}

fn u(a: &[Value], i: usize) -> Option<u64> {
    a.get(i).and_then(|v| v.as_u64())
}

fn i(a: &[Value], idx: usize) -> Option<i64> {
    a.get(idx).and_then(|v| v.as_i64())
}

/// Float anchors are cell coordinates that may be fractional.
fn f(a: &[Value], idx: usize) -> Option<f64> {
    a.get(idx)
        .and_then(|v| v.as_f64().or_else(|| v.as_i64().map(|n| n as f64)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(name: &str, args: Vec<Value>) -> RedrawEvent {
        (name.to_string(), args)
    }

    /// `hl_attr_define` is `[id, rgb_attrs, cterm_attrs, info]`.
    fn hl_define(id: u64, attrs: &[(&str, Value)]) -> RedrawEvent {
        let map = attrs
            .iter()
            .map(|(k, v)| (Value::from(*k), v.clone()))
            .collect::<Vec<_>>();
        ev(
            "hl_attr_define",
            vec![Value::from(id), Value::Map(map), Value::Map(vec![]), Value::Array(vec![])],
        )
    }

    fn resize(grid: u64, cols: usize, rows: usize) -> RedrawEvent {
        ev("grid_resize", vec![Value::from(grid), Value::from(cols), Value::from(rows)])
    }

    /// Write `text` at `(row, col)` of `grid` with one highlight id.
    fn line(grid: u64, row: usize, col: usize, text: &str, hl: u64) -> RedrawEvent {
        let cells = text
            .chars()
            .map(|c| Value::Array(vec![Value::from(c.to_string()), Value::from(hl)]))
            .collect();
        ev(
            "grid_line",
            vec![
                Value::from(grid),
                Value::from(row),
                Value::from(col),
                Value::Array(cells),
                Value::from(false),
            ],
        )
    }

    fn win_pos(grid: u64, row: i64, col: i64, width: usize, height: usize) -> RedrawEvent {
        ev(
            "win_pos",
            vec![
                Value::from(grid),
                Value::Ext(1, vec![grid as u8]),
                Value::from(row),
                Value::from(col),
                Value::from(width),
                Value::from(height),
            ],
        )
    }

    fn win_float_pos(
        grid: u64,
        anchor: &str,
        anchor_grid: u64,
        row: f64,
        col: f64,
        zindex: Option<i64>,
    ) -> RedrawEvent {
        let mut args = vec![
            Value::from(grid),
            Value::Ext(1, vec![grid as u8]),
            Value::from(anchor),
            Value::from(anchor_grid),
            Value::from(row),
            Value::from(col),
            Value::from(true),
        ];
        if let Some(z) = zindex {
            args.push(Value::from(z));
        }
        ev("win_float_pos", args)
    }

    /// The screen row as a string, for readable assertions.
    fn row_text(screen: &Screen, row: usize) -> String {
        (0..screen.cols()).map(|c| screen.cell(row, c).ch).collect()
    }

    fn filled(grid: u64, cols: usize, rows: usize, ch: char) -> Vec<RedrawEvent> {
        let mut events = vec![resize(grid, cols, rows)];
        let text: String = std::iter::repeat_n(ch, cols).collect();
        for row in 0..rows {
            events.push(line(grid, row, 0, &text, 0));
        }
        events
    }

    // --- highlight table (unchanged behaviour, now screen-wide) --------------

    #[test]
    fn parses_the_underline_family_and_special_colour() {
        let mut screen = Screen::new(4, 1);
        screen.apply(&[
            hl_define(1, &[("underline", Value::from(true)), ("special", Value::from(0xff0000u32))]),
            hl_define(2, &[("undercurl", Value::from(true))]),
            hl_define(3, &[("strikethrough", Value::from(true))]),
            hl_define(4, &[("underdouble", Value::from(true))]),
        ]);

        let s1 = screen.style(1);
        assert!(s1.underline && s1.any_underline());
        assert_eq!(s1.special, Some(0xff0000));
        assert!(!s1.undercurl && !s1.strikethrough);

        assert!(screen.style(2).undercurl && screen.style(2).any_underline());
        assert!(screen.style(3).strikethrough && !screen.style(3).any_underline());
        assert!(screen.style(4).underdouble && screen.style(4).any_underline());
    }

    #[test]
    fn keeps_bold_italic_alongside_the_new_attributes() {
        let mut screen = Screen::new(4, 1);
        screen.apply(&[hl_define(
            7,
            &[("bold", Value::from(true)), ("italic", Value::from(true)), ("underline", Value::from(true))],
        )]);
        let s = screen.style(7);
        assert!(s.bold && s.italic && s.underline);
    }

    #[test]
    fn unknown_highlight_id_is_the_default_style() {
        let screen = Screen::new(4, 1);
        assert_eq!(screen.style(99), Hl::default());
        assert!(!screen.style(99).any_underline());
    }

    #[test]
    fn decoration_color_prefers_special_then_falls_back_to_foreground() {
        let mut screen = Screen::new(4, 1);
        screen.apply(&[
            hl_define(1, &[("underline", Value::from(true)), ("special", Value::from(0x00ff00u32)), ("foreground", Value::from(0x111111u32))]),
            hl_define(2, &[("underline", Value::from(true)), ("foreground", Value::from(0x222222u32))]),
        ]);
        assert_eq!(screen.decoration_color(1), 0x00ff00);
        assert_eq!(screen.decoration_color(2), 0x222222);
    }

    // --- single grid: the behaviour that existed before multigrid -----------

    #[test]
    fn a_session_without_windows_composes_to_the_global_grid() {
        let mut screen = Screen::new(6, 2);
        screen.apply(&[
            line(GLOBAL_GRID, 0, 0, "hello", 3),
            line(GLOBAL_GRID, 1, 1, "abc", 0),
            ev("grid_cursor_goto", vec![Value::from(GLOBAL_GRID), Value::from(1u64), Value::from(2u64)]),
            ev("flush", vec![]),
        ]);
        assert_eq!(row_text(&screen, 0), "hello ");
        assert_eq!(row_text(&screen, 1), " abc  ");
        assert_eq!(screen.cell(0, 0).hl, 3);
        assert_eq!(screen.cursor(), Some((1, 2)));
        assert!(screen.flushed);
    }

    #[test]
    fn global_grid_resize_resizes_the_screen() {
        let mut screen = Screen::new(6, 2);
        screen.apply(&[resize(GLOBAL_GRID, 3, 4)]);
        assert_eq!((screen.cols(), screen.rows()), (3, 4));
    }

    #[test]
    fn grid_clear_and_scroll_stay_on_their_own_grid() {
        let mut screen = Screen::new(4, 2);
        let mut events = vec![line(GLOBAL_GRID, 0, 0, "abcd", 0), resize(2, 4, 2)];
        events.extend([line(2, 0, 0, "wxyz", 0), line(2, 1, 0, "1234", 0), win_pos(2, 1, 0, 4, 1)]);
        screen.apply(&events);
        assert_eq!(row_text(&screen, 0), "abcd");
        assert_eq!(row_text(&screen, 1), "wxyz");

        // Scrolling grid 2 moves its own rows only.
        screen.apply(&[ev(
            "grid_scroll",
            vec![
                Value::from(2u64),
                Value::from(0u64),
                Value::from(2u64),
                Value::from(0u64),
                Value::from(4u64),
                Value::from(1i64),
                Value::from(0u64),
            ],
        )]);
        assert_eq!(row_text(&screen, 0), "abcd");
        assert_eq!(row_text(&screen, 1), "1234");

        // And clearing it leaves the global grid intact.
        screen.apply(&[ev("grid_clear", vec![Value::from(2u64)])]);
        assert_eq!(row_text(&screen, 0), "abcd");
        assert_eq!(row_text(&screen, 1), "    ");
    }

    #[test]
    fn a_line_for_an_unknown_grid_is_dropped() {
        let mut screen = Screen::new(4, 1);
        screen.apply(&[line(9, 0, 0, "zzzz", 0)]);
        assert_eq!(row_text(&screen, 0), "    ");
        assert_eq!(screen.grid_ids(), vec![GLOBAL_GRID]);
    }

    // --- placement ----------------------------------------------------------

    #[test]
    fn win_pos_places_a_split_at_its_screen_position() {
        let mut screen = Screen::new(8, 3);
        let mut events = filled(GLOBAL_GRID, 8, 3, '.');
        events.extend(filled(2, 3, 2, 'w'));
        events.push(win_pos(2, 1, 5, 3, 2));
        screen.apply(&events);
        assert_eq!(row_text(&screen, 0), "........");
        assert_eq!(row_text(&screen, 1), ".....www");
        assert_eq!(row_text(&screen, 2), ".....www");
    }

    #[test]
    fn a_split_is_clipped_to_the_size_win_pos_declared() {
        let mut screen = Screen::new(8, 3);
        // The grid is wider and taller than the window nvim placed.
        let mut events = filled(GLOBAL_GRID, 8, 3, '.');
        events.extend(filled(2, 6, 3, 'w'));
        events.push(win_pos(2, 0, 0, 2, 1));
        screen.apply(&events);
        assert_eq!(row_text(&screen, 0), "ww......");
        assert_eq!(row_text(&screen, 1), "........");
    }

    #[test]
    fn a_window_hanging_off_the_screen_is_clipped_not_wrapped() {
        let mut screen = Screen::new(6, 2);
        let mut events = filled(GLOBAL_GRID, 6, 2, '.');
        events.extend(filled(2, 4, 2, 'w'));
        events.push(win_pos(2, 1, 4, 4, 2));
        screen.apply(&events);
        assert_eq!(row_text(&screen, 0), "......");
        assert_eq!(row_text(&screen, 1), "....ww");
    }

    #[test]
    fn a_float_anchored_off_the_left_edge_keeps_only_its_visible_part() {
        let mut screen = Screen::new(6, 2);
        let mut events = filled(GLOBAL_GRID, 6, 2, '.');
        events.extend(filled(3, 4, 1, 'f'));
        // NW at column -2: the first two columns of the float are off screen.
        events.push(win_float_pos(3, "NW", GLOBAL_GRID, 0.0, -2.0, None));
        screen.apply(&events);
        assert_eq!(row_text(&screen, 0), "ff....");
        assert_eq!(row_text(&screen, 1), "......");
    }

    #[test]
    fn every_float_anchor_corner_resolves_against_the_anchor_grid() {
        // A 2x2 float anchored at (2,2) of the global grid, one corner per case.
        let cases = [
            ("NW", (2usize, 2usize)),
            ("NE", (2, 0)),
            ("SW", (0, 2)),
            ("SE", (0, 0)),
        ];
        for (anchor, (row, col)) in cases {
            let mut screen = Screen::new(5, 5);
            let mut events = filled(GLOBAL_GRID, 5, 5, '.');
            events.extend(filled(4, 2, 2, 'f'));
            events.push(win_float_pos(4, anchor, GLOBAL_GRID, 2.0, 2.0, None));
            screen.apply(&events);
            assert_eq!(screen.cell(row, col).ch, 'f', "{anchor} top-left");
            assert_eq!(screen.cell(row + 1, col + 1).ch, 'f', "{anchor} bottom-right");
        }
    }

    #[test]
    fn a_float_anchored_to_another_float_follows_it() {
        let mut screen = Screen::new(8, 4);
        let mut events = filled(GLOBAL_GRID, 8, 4, '.');
        events.extend(filled(3, 2, 1, 'a'));
        events.extend(filled(4, 2, 1, 'b'));
        events.push(win_float_pos(3, "NW", GLOBAL_GRID, 1.0, 2.0, None));
        events.push(win_float_pos(4, "NW", 3, 1.0, 1.0, Some(60)));
        screen.apply(&events);
        assert_eq!(row_text(&screen, 1), "..aa....");
        assert_eq!(row_text(&screen, 2), "...bb...");
    }

    #[test]
    fn fractional_anchor_coordinates_land_on_the_cell_they_start_in() {
        let mut screen = Screen::new(6, 2);
        let mut events = filled(GLOBAL_GRID, 6, 2, '.');
        events.extend(filled(3, 2, 1, 'f'));
        events.push(win_float_pos(3, "NW", GLOBAL_GRID, 0.5, 1.5, None));
        screen.apply(&events);
        assert_eq!(row_text(&screen, 0), ".ff...");
    }

    // --- z-order ------------------------------------------------------------

    #[test]
    fn floats_sit_above_splits_and_higher_zindex_wins() {
        let mut screen = Screen::new(6, 1);
        let mut events = filled(GLOBAL_GRID, 6, 1, '.');
        events.extend(filled(2, 6, 1, 'w'));
        events.extend(filled(3, 6, 1, 'a'));
        events.extend(filled(4, 6, 1, 'b'));
        events.push(win_pos(2, 0, 0, 6, 1));
        // The higher zindex is placed first, so ordering cannot come from the
        // order the events arrived in.
        events.push(win_float_pos(4, "NW", GLOBAL_GRID, 0.0, 0.0, Some(90)));
        events.push(win_float_pos(3, "NW", GLOBAL_GRID, 0.0, 0.0, Some(50)));
        screen.apply(&events);
        assert_eq!(row_text(&screen, 0), "bbbbbb");
    }

    #[test]
    fn equal_zindex_is_broken_by_placement_order() {
        let mut screen = Screen::new(4, 1);
        let mut events = filled(GLOBAL_GRID, 4, 1, '.');
        events.extend(filled(3, 4, 1, 'a'));
        events.extend(filled(4, 4, 1, 'b'));
        events.push(win_float_pos(3, "NW", GLOBAL_GRID, 0.0, 0.0, Some(50)));
        events.push(win_float_pos(4, "NW", GLOBAL_GRID, 0.0, 0.0, Some(50)));
        screen.apply(&events);
        assert_eq!(row_text(&screen, 0), "bbbb");

        // Re-placing the older float puts it back on top.
        screen.apply(&[win_float_pos(3, "NW", GLOBAL_GRID, 0.0, 0.0, Some(50))]);
        assert_eq!(row_text(&screen, 0), "aaaa");
    }

    #[test]
    fn the_message_grid_sits_above_ordinary_floats() {
        let mut screen = Screen::new(5, 3);
        let mut events = filled(GLOBAL_GRID, 5, 3, '.');
        events.extend(filled(3, 5, 1, 'f'));
        events.extend(filled(5, 5, 1, 'm'));
        events.push(win_float_pos(3, "NW", GLOBAL_GRID, 2.0, 0.0, None));
        events.push(ev(
            "msg_set_pos",
            vec![Value::from(5u64), Value::from(2u64), Value::from(false), Value::from(" ")],
        ));
        screen.apply(&events);
        assert_eq!(row_text(&screen, 2), "mmmmm");
    }

    // --- lifecycle ----------------------------------------------------------

    #[test]
    fn win_hide_takes_a_window_off_the_screen_and_win_pos_brings_it_back() {
        let mut screen = Screen::new(4, 1);
        let mut events = filled(GLOBAL_GRID, 4, 1, '.');
        events.extend(filled(2, 4, 1, 'w'));
        events.push(win_pos(2, 0, 0, 4, 1));
        screen.apply(&events);
        assert_eq!(row_text(&screen, 0), "wwww");

        screen.apply(&[ev("win_hide", vec![Value::from(2u64)])]);
        assert_eq!(row_text(&screen, 0), "....");
        // The grid itself survives being hidden.
        assert!(screen.grid_ids().contains(&2));

        screen.apply(&[win_pos(2, 0, 0, 4, 1)]);
        assert_eq!(row_text(&screen, 0), "wwww");
    }

    #[test]
    fn win_close_then_grid_destroy_removes_the_window_and_its_grid() {
        let mut screen = Screen::new(4, 1);
        let mut events = filled(GLOBAL_GRID, 4, 1, '.');
        events.extend(filled(2, 4, 1, 'w'));
        events.push(win_pos(2, 0, 0, 4, 1));
        screen.apply(&events);

        screen.apply(&[ev("win_close", vec![Value::from(2u64)])]);
        assert_eq!(row_text(&screen, 0), "....");
        assert!(screen.window(2).is_none());
        assert!(screen.grid_ids().contains(&2));

        screen.apply(&[ev("grid_destroy", vec![Value::from(2u64)])]);
        assert_eq!(screen.grid_ids(), vec![GLOBAL_GRID]);
        // Content for a destroyed grid cannot come back on screen.
        screen.apply(&[line(2, 0, 0, "wwww", 0), win_pos(2, 0, 0, 4, 1)]);
        assert_eq!(row_text(&screen, 0), "....");
    }

    #[test]
    fn resizing_a_window_grid_reflows_what_is_composited() {
        let mut screen = Screen::new(6, 2);
        let mut events = filled(GLOBAL_GRID, 6, 2, '.');
        events.extend(filled(2, 3, 2, 'w'));
        events.push(win_pos(2, 0, 0, 3, 2));
        screen.apply(&events);
        assert_eq!(row_text(&screen, 0), "www...");
        assert_eq!(row_text(&screen, 1), "www...");

        // Widening keeps the text nvim will not resend and blanks the rest; the
        // dropped row stops being composited.
        screen.apply(&[resize(2, 5, 1), win_pos(2, 0, 0, 5, 1)]);
        assert_eq!(row_text(&screen, 0), "www  .");
        assert_eq!(row_text(&screen, 1), "......");

        screen.apply(&[line(2, 0, 0, "abcde", 0)]);
        assert_eq!(row_text(&screen, 0), "abcde.");
    }

    #[test]
    fn external_windows_are_left_off_the_screen() {
        let mut screen = Screen::new(4, 1);
        let mut events = filled(GLOBAL_GRID, 4, 1, '.');
        events.extend(filled(2, 4, 1, 'w'));
        events.push(ev("win_external_pos", vec![Value::from(2u64), Value::Ext(1, vec![2])]));
        screen.apply(&events);
        assert_eq!(row_text(&screen, 0), "....");
        assert_eq!(screen.window(2).map(|w| w.placement), Some(Placement::External));
    }

    // --- cursor -------------------------------------------------------------

    #[test]
    fn the_cursor_moves_with_the_window_it_is_in() {
        let mut screen = Screen::new(8, 4);
        let mut events = filled(GLOBAL_GRID, 8, 4, '.');
        events.extend(filled(2, 4, 2, 'w'));
        events.push(win_pos(2, 1, 3, 4, 2));
        events.push(ev(
            "grid_cursor_goto",
            vec![Value::from(2u64), Value::from(1u64), Value::from(2u64)],
        ));
        screen.apply(&events);
        assert_eq!(screen.cursor(), Some((2, 5)));

        screen.apply(&[win_pos(2, 0, 0, 4, 2)]);
        assert_eq!(screen.cursor(), Some((1, 2)));
    }

    #[test]
    fn a_cursor_on_a_grid_that_is_not_on_screen_has_no_position() {
        let mut screen = Screen::new(4, 2);
        let mut events = filled(GLOBAL_GRID, 4, 2, '.');
        events.extend(filled(2, 4, 2, 'w'));
        events.push(win_pos(2, 0, 0, 4, 2));
        events.push(ev(
            "grid_cursor_goto",
            vec![Value::from(2u64), Value::from(0u64), Value::from(1u64)],
        ));
        screen.apply(&events);
        assert_eq!(screen.cursor(), Some((0, 1)));

        screen.apply(&[ev("win_hide", vec![Value::from(2u64)])]);
        assert_eq!(screen.cursor(), None);

        screen.apply(&[win_pos(2, 0, 0, 4, 2), ev("grid_destroy", vec![Value::from(2u64)])]);
        assert_eq!(screen.cursor(), None);
    }

    #[test]
    fn a_cursor_past_the_visible_extent_of_its_window_is_not_drawn() {
        let mut screen = Screen::new(8, 2);
        let mut events = filled(GLOBAL_GRID, 8, 2, '.');
        events.extend(filled(2, 6, 2, 'w'));
        // The window shows 2 of the grid's 6 columns.
        events.push(win_pos(2, 0, 0, 2, 2));
        events.push(ev(
            "grid_cursor_goto",
            vec![Value::from(2u64), Value::from(0u64), Value::from(4u64)],
        ));
        screen.apply(&events);
        assert_eq!(screen.cursor(), None);
    }

    // --- whole redraw batches, as they arrive over RPC ----------------------

    /// A `:vsplit`-shaped batch: nvim resizes the global grid, creates two
    /// window grids, places them either side of a separator column it draws
    /// itself, then opens a float over both.
    #[test]
    fn a_vsplit_batch_from_the_wire_composes_into_one_screen() {
        let notification = Value::Array(vec![
            Value::from(2u8),
            Value::from("redraw"),
            Value::Array(vec![
                Value::Array(vec![
                    Value::from("grid_resize"),
                    Value::Array(vec![Value::from(1u64), Value::from(7u64), Value::from(2u64)]),
                    Value::Array(vec![Value::from(2u64), Value::from(3u64), Value::from(1u64)]),
                    Value::Array(vec![Value::from(4u64), Value::from(3u64), Value::from(1u64)]),
                ]),
                Value::Array(vec![
                    Value::from("grid_line"),
                    // The separator column and the status row nvim keeps on the
                    // global grid.
                    Value::Array(vec![
                        Value::from(1u64),
                        Value::from(0u64),
                        Value::from(3u64),
                        Value::Array(vec![Value::Array(vec![
                            Value::from("|"),
                            Value::from(0u64),
                        ])]),
                        Value::from(false),
                    ]),
                    Value::Array(vec![
                        Value::from(1u64),
                        Value::from(1u64),
                        Value::from(0u64),
                        Value::Array(vec![Value::Array(vec![
                            Value::from("-"),
                            Value::from(0u64),
                            Value::from(7u64),
                        ])]),
                        Value::from(false),
                    ]),
                    Value::Array(vec![
                        Value::from(2u64),
                        Value::from(0u64),
                        Value::from(0u64),
                        Value::Array(vec![Value::Array(vec![
                            Value::from("L"),
                            Value::from(0u64),
                            Value::from(3u64),
                        ])]),
                        Value::from(false),
                    ]),
                    Value::Array(vec![
                        Value::from(4u64),
                        Value::from(0u64),
                        Value::from(0u64),
                        Value::Array(vec![Value::Array(vec![
                            Value::from("R"),
                            Value::from(0u64),
                            Value::from(3u64),
                        ])]),
                        Value::from(false),
                    ]),
                ]),
                Value::Array(vec![
                    Value::from("win_pos"),
                    Value::Array(vec![
                        Value::from(2u64),
                        Value::Ext(1, vec![2]),
                        Value::from(0u64),
                        Value::from(0u64),
                        Value::from(3u64),
                        Value::from(1u64),
                    ]),
                    Value::Array(vec![
                        Value::from(4u64),
                        Value::Ext(1, vec![4]),
                        Value::from(0u64),
                        Value::from(4u64),
                        Value::from(3u64),
                        Value::from(1u64),
                    ]),
                ]),
                Value::Array(vec![
                    Value::from("grid_cursor_goto"),
                    Value::Array(vec![Value::from(4u64), Value::from(0u64), Value::from(1u64)]),
                ]),
                Value::Array(vec![Value::from("flush"), Value::Array(vec![])]),
            ]),
        ]);

        let (events, notes) = crate::nvim::split_notification(&notification);
        assert!(notes.is_empty());

        let mut screen = Screen::new(7, 2);
        screen.apply(&events);
        assert_eq!(row_text(&screen, 0), "LLL|RRR");
        assert_eq!(row_text(&screen, 1), "-------");
        assert_eq!(screen.cursor(), Some((0, 5)));
        assert_eq!(screen.grid_ids(), vec![1, 2, 4]);
    }
}
