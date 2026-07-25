//! Cell storage for one Neovim grid, and the highlight table every grid shares.
//!
//! This is a mirror of what nvim decided to show. It never decides anything
//! itself — no wrapping, no scrolling policy, no syntax. Where each grid sits on
//! screen is not stored here either; that belongs to [`crate::screen`].

use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Debug)]
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

/// The highlight definitions and default colours, which nvim sends once for the
/// whole UI rather than per grid.
#[derive(Default)]
pub struct Styles {
    pub hls: HashMap<u64, Hl>,
    pub default_fg: u32,
    pub default_bg: u32,
}

impl Styles {
    pub fn new() -> Self {
        Self { hls: HashMap::new(), default_fg: 0xd0d0d0, default_bg: 0x101014 }
    }

    /// Resolve a cell's full style (decoration attributes plus the `sp` colour).
    /// The renderer reads this to draw bold/italic glyphs and the underline
    /// family; colours still go through [`Styles::colors`].
    pub fn style(&self, hl_id: u64) -> Hl {
        self.hls.get(&hl_id).copied().unwrap_or_default()
    }

    /// The colour a decoration (underline/strikethrough) should use for `hl_id`:
    /// the highlight's `sp` colour when present, otherwise its resolved
    /// foreground (which already accounts for reverse video and defaults).
    pub fn decoration_color(&self, hl_id: u64) -> u32 {
        self.style(hl_id).special.unwrap_or_else(|| self.colors(hl_id).0)
    }

    /// Resolve a cell's colours, applying reverse video and defaults.
    pub fn colors(&self, hl_id: u64) -> (u32, u32) {
        let hl = self.style(hl_id);
        let fg = hl.fg.unwrap_or(self.default_fg);
        let bg = hl.bg.unwrap_or(self.default_bg);
        if hl.reverse {
            (bg, fg)
        } else {
            (fg, bg)
        }
    }
}

/// One grid's cells. Without `ext_multigrid` there is exactly one of these; with
/// it there is one per window plus nvim's own outer grid.
#[derive(Default)]
pub struct Grid {
    pub cols: usize,
    pub rows: usize,
    pub cells: Vec<Cell>,
}

impl Grid {
    pub fn new(cols: usize, rows: usize) -> Self {
        Self { cols, rows, cells: vec![Cell::default(); cols * rows] }
    }

    /// Resizing keeps the cells that are still inside the grid. After a resize
    /// nvim sends only what changed — a window moved and resized by a `:split`
    /// is never resent in full — so dropping the old contents would leave real
    /// text missing from the screen.
    pub fn resize(&mut self, cols: usize, rows: usize) {
        if self.cols == cols && self.rows == rows {
            return;
        }
        let mut cells = vec![Cell::default(); cols * rows];
        for row in 0..rows.min(self.rows) {
            for col in 0..cols.min(self.cols) {
                cells[row * cols + col] = self.cells[row * self.cols + col];
            }
        }
        self.cols = cols;
        self.rows = rows;
        self.cells = cells;
    }

    pub fn cell(&self, row: usize, col: usize) -> Cell {
        self.cells
            .get(row * self.cols + col)
            .copied()
            .unwrap_or_default()
    }

    pub fn set(&mut self, row: usize, col: usize, cell: Cell) {
        if row < self.rows && col < self.cols {
            self.cells[row * self.cols + col] = cell;
        }
    }

    pub fn clear(&mut self) {
        self.cells.iter_mut().for_each(|c| *c = Cell::default());
    }

    /// `grid_scroll`: move a rectangle of rows within the grid. Positive `rows`
    /// scrolls text up (the region moves toward the top). The vacated rows are
    /// left as they are; nvim redraws them.
    pub fn scroll(&mut self, top: usize, bot: usize, left: usize, right: usize, rows: i64) {
        let cols = self.cols;
        let bot = bot.min(self.rows);
        let right = right.min(cols);
        let move_row = |dst: usize, src: usize, cells: &mut Vec<Cell>| {
            for c in left..right {
                cells[dst * cols + c] = cells[src * cols + c];
            }
        };
        if rows > 0 {
            let n = rows as usize;
            for r in (top + n)..bot {
                move_row(r - n, r, &mut self.cells);
            }
        } else if rows < 0 {
            let n = (-rows) as usize;
            for r in (top..bot.saturating_sub(n)).rev() {
                move_row(r + n, r, &mut self.cells);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resize_keeps_the_cells_that_are_still_inside_the_grid() {
        let mut grid = Grid::new(4, 2);
        grid.set(1, 2, Cell { ch: 'x', hl: 3 });
        grid.set(0, 3, Cell { ch: 'y', hl: 0 });

        grid.resize(4, 2);
        assert_eq!(grid.cell(1, 2).ch, 'x');

        // Growing keeps every cell where it was; the new area is blank.
        grid.resize(6, 3);
        assert_eq!(grid.cells.len(), 18);
        assert_eq!(grid.cell(1, 2), Cell { ch: 'x', hl: 3 });
        assert_eq!(grid.cell(2, 5).ch, ' ');

        // Shrinking drops what no longer fits and keeps the rest.
        grid.resize(3, 2);
        assert_eq!(grid.cell(1, 2).ch, 'x');
        assert_eq!(grid.cells.len(), 6);
    }

    #[test]
    fn scroll_moves_only_the_named_region() {
        let mut grid = Grid::new(3, 3);
        for row in 0..3 {
            grid.set(row, 0, Cell { ch: (b'a' + row as u8) as char, hl: 0 });
            grid.set(row, 2, Cell { ch: 'k', hl: 0 });
        }
        grid.scroll(0, 3, 0, 1, 1);
        assert_eq!(grid.cell(0, 0).ch, 'b');
        assert_eq!(grid.cell(1, 0).ch, 'c');
        // Outside [left, right) nothing moved.
        assert_eq!(grid.cell(0, 2).ch, 'k');
    }
}
