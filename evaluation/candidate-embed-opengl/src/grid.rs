//! The screen state Neovim hands us over `ext_linegrid`.
//!
//! This is a mirror of what nvim decided to show. It never decides anything
//! itself — no wrapping, no scrolling policy, no syntax.

use std::collections::HashMap;

use rmpv::Value;

use crate::nvim::RedrawEvent;

#[derive(Clone, Copy, PartialEq)]
pub struct Cell {
    pub ch: char,
    pub hl: u64,
}

impl Default for Cell {
    fn default() -> Self {
        Cell { ch: ' ', hl: 0 }
    }
}

#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct Hl {
    pub fg: Option<u32>,
    pub bg: Option<u32>,
    pub reverse: bool,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub undercurl: bool,
    pub underdouble: bool,
    pub underdotted: bool,
    pub underdashed: bool,
    pub strikethrough: bool,
    /// The `sp` colour nvim attaches to underline family attributes. When unset
    /// the decoration inherits the cell foreground.
    pub special: Option<u32>,
}

impl Hl {
    /// True when any underline-family decoration is set.
    pub fn any_underline(&self) -> bool {
        self.underline || self.undercurl || self.underdouble || self.underdotted || self.underdashed
    }
}

pub struct Grid {
    pub cols: usize,
    pub rows: usize,
    pub cells: Vec<Cell>,
    pub cursor: (usize, usize), // (row, col)
    pub hls: HashMap<u64, Hl>,
    pub default_fg: u32,
    pub default_bg: u32,
    /// Set when nvim says the frame is complete and ready to present.
    pub flushed: bool,
}

impl Grid {
    pub fn new(cols: usize, rows: usize) -> Self {
        Self {
            cols,
            rows,
            cells: vec![Cell::default(); cols * rows],
            cursor: (0, 0),
            hls: HashMap::new(),
            default_fg: 0xd0d0d0,
            default_bg: 0x101014,
            flushed: false,
        }
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        self.cols = cols;
        self.rows = rows;
        self.cells = vec![Cell::default(); cols * rows];
    }

    pub fn cell(&self, row: usize, col: usize) -> Cell {
        self.cells
            .get(row * self.cols + col)
            .copied()
            .unwrap_or_default()
    }

    /// Resolve a cell's full style (decoration attributes plus the `sp` colour).
    /// The renderer reads this to draw bold/italic glyphs and the underline
    /// family; colours still go through [`Grid::colors`].
    pub fn style(&self, hl_id: u64) -> Hl {
        self.hls.get(&hl_id).copied().unwrap_or_default()
    }

    /// The colour a decoration (underline/strikethrough) should use for `hl_id`:
    /// the highlight's `sp` colour when present, otherwise its resolved
    /// foreground (which already accounts for reverse video and defaults).
    pub fn decoration_color(&self, hl_id: u64) -> u32 {
        let hl = self.style(hl_id);
        hl.special.unwrap_or_else(|| self.colors(hl_id).0)
    }

    /// Resolve a cell's colours, applying reverse video and defaults.
    pub fn colors(&self, hl_id: u64) -> (u32, u32) {
        let hl = self.hls.get(&hl_id).copied().unwrap_or_default();
        let fg = hl.fg.unwrap_or(self.default_fg);
        let bg = hl.bg.unwrap_or(self.default_bg);
        if hl.reverse {
            (bg, fg)
        } else {
            (fg, bg)
        }
    }

    pub fn apply(&mut self, events: &[RedrawEvent]) {
        for (name, args) in events {
            match name.as_str() {
                "grid_resize" => self.ev_grid_resize(args),
                "default_colors_set" => self.ev_default_colors(args),
                "hl_attr_define" => self.ev_hl_attr_define(args),
                "grid_line" => self.ev_grid_line(args),
                "grid_clear" => self.cells.iter_mut().for_each(|c| *c = Cell::default()),
                "grid_scroll" => self.ev_grid_scroll(args),
                "grid_cursor_goto" => self.ev_cursor_goto(args),
                "flush" => self.flushed = true,
                _ => {}
            }
        }
    }

    fn ev_grid_resize(&mut self, a: &[Value]) {
        if let (Some(w), Some(h)) = (u(a, 1), u(a, 2)) {
            self.resize(w as usize, h as usize);
        }
    }

    fn ev_default_colors(&mut self, a: &[Value]) {
        if let Some(fg) = u(a, 0) {
            if fg != u64::MAX {
                self.default_fg = fg as u32;
            }
        }
        if let Some(bg) = u(a, 1) {
            if bg != u64::MAX {
                self.default_bg = bg as u32;
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
        self.hls.insert(id, hl);
    }

    /// `grid_line` is `[grid, row, col_start, cells, wrap]`, where each entry of
    /// `cells` is `[text]`, `[text, hl_id]` or `[text, hl_id, repeat]`. Omitted
    /// hl_id means "same as the previous cell in this run".
    fn ev_grid_line(&mut self, a: &[Value]) {
        let (Some(row), Some(col0)) = (u(a, 1), u(a, 2)) else { return };
        let Some(cells) = a.get(3).and_then(|v| v.as_array()) else { return };
        let row = row as usize;
        if row >= self.rows {
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
                if col >= self.cols {
                    break;
                }
                self.cells[row * self.cols + col] = Cell { ch, hl };
                col += 1;
            }
        }
    }

    fn ev_grid_scroll(&mut self, a: &[Value]) {
        let (Some(top), Some(bot), Some(left), Some(right), Some(rows)) =
            (u(a, 1), u(a, 2), u(a, 3), u(a, 4), i(a, 5))
        else {
            return;
        };
        let (top, bot, left, right) = (top as usize, bot as usize, left as usize, right as usize);
        let move_row = |dst: usize, src: usize, cells: &mut Vec<Cell>, cols: usize| {
            for c in left..right.min(cols) {
                cells[dst * cols + c] = cells[src * cols + c];
            }
        };
        let cols = self.cols;
        if rows > 0 {
            for r in (top + rows as usize)..bot {
                move_row(r - rows as usize, r, &mut self.cells, cols);
            }
        } else if rows < 0 {
            let n = (-rows) as usize;
            for r in (top..bot.saturating_sub(n)).rev() {
                move_row(r + n, r, &mut self.cells, cols);
            }
        }
    }

    fn ev_cursor_goto(&mut self, a: &[Value]) {
        if let (Some(r), Some(c)) = (u(a, 1), u(a, 2)) {
            self.cursor = (r as usize, c as usize);
        }
    }
}

fn u(a: &[Value], i: usize) -> Option<u64> {
    a.get(i).and_then(|v| v.as_u64())
}

fn i(a: &[Value], idx: usize) -> Option<i64> {
    a.get(idx).and_then(|v| v.as_i64())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build an `hl_attr_define` redraw event: `[id, rgb_attrs, cterm_attrs, info]`.
    fn hl_define(id: u64, attrs: &[(&str, Value)]) -> RedrawEvent {
        let map = attrs
            .iter()
            .map(|(k, v)| (Value::from(*k), v.clone()))
            .collect::<Vec<_>>();
        (
            "hl_attr_define".to_string(),
            vec![Value::from(id), Value::Map(map), Value::Map(vec![]), Value::Array(vec![])],
        )
    }

    #[test]
    fn parses_the_underline_family_and_special_colour() {
        let mut grid = Grid::new(4, 1);
        grid.apply(&[
            hl_define(1, &[("underline", Value::from(true)), ("special", Value::from(0xff0000u32))]),
            hl_define(2, &[("undercurl", Value::from(true))]),
            hl_define(3, &[("strikethrough", Value::from(true))]),
            hl_define(4, &[("underdouble", Value::from(true))]),
        ]);

        let s1 = grid.style(1);
        assert!(s1.underline && s1.any_underline());
        assert_eq!(s1.special, Some(0xff0000));
        assert!(!s1.undercurl && !s1.strikethrough);

        assert!(grid.style(2).undercurl && grid.style(2).any_underline());
        assert!(grid.style(3).strikethrough && !grid.style(3).any_underline());
        assert!(grid.style(4).underdouble && grid.style(4).any_underline());
    }

    #[test]
    fn keeps_bold_italic_alongside_the_new_attributes() {
        let mut grid = Grid::new(4, 1);
        grid.apply(&[hl_define(
            7,
            &[("bold", Value::from(true)), ("italic", Value::from(true)), ("underline", Value::from(true))],
        )]);
        let s = grid.style(7);
        assert!(s.bold && s.italic && s.underline);
    }

    #[test]
    fn unknown_highlight_id_is_the_default_style() {
        let grid = Grid::new(4, 1);
        assert_eq!(grid.style(99), Hl::default());
        assert!(!grid.style(99).any_underline());
    }

    #[test]
    fn decoration_color_prefers_special_then_falls_back_to_foreground() {
        let mut grid = Grid::new(4, 1);
        grid.apply(&[
            hl_define(1, &[("underline", Value::from(true)), ("special", Value::from(0x00ff00u32)), ("foreground", Value::from(0x111111u32))]),
            hl_define(2, &[("underline", Value::from(true)), ("foreground", Value::from(0x222222u32))]),
        ]);
        assert_eq!(grid.decoration_color(1), 0x00ff00);
        assert_eq!(grid.decoration_color(2), 0x222222);
    }
}
