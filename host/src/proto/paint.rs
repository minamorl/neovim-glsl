//! Turning editor state into the `redraw` stream.
//!
//! This is where the host stops being an editor and starts being a Neovim UI
//! server: everything below is `grid_line`, `hl_attr_define` and the rest, in
//! the shapes a Neovim UI client already understands. The navigation surface is
//! deliberately absent — `pin navigation_not_in_grid` puts it outside the grid,
//! so a picker painted here would be a spec violation, not a shortcut.

use rmpv::Value;

use crate::core::buffer::Buffer;
use crate::core::editor::{Editor, Mode, Options, Visual};
use crate::core::window::WindowView;
use crate::nvim::{RedrawEvent, UiOptions};

pub const GRID: u64 = 1;

/// Highlight ids. Fixed rather than allocated, because both halves of this
/// process agree on them at compile time and a UI client only needs them to be
/// consistent within a session.
pub mod hl {
    pub const DEFAULT: u64 = 0;
    pub const LINE_NR: u64 = 1;
    pub const CURSOR_LINE_NR: u64 = 2;
    pub const STATUS: u64 = 3;
    pub const VISUAL: u64 = 4;
    pub const SEARCH: u64 = 5;
    pub const ERROR: u64 = 6;
    pub const NON_TEXT: u64 = 7;
    pub const HEADING: u64 = 8;
    pub const CODE: u64 = 9;
    pub const EMPHASIS: u64 = 10;
    pub const BULLET: u64 = 11;
    pub const LINK: u64 = 12;
    pub const MODIFIED: u64 = 13;
    pub const CURSOR_LINE: u64 = 14;
}

/// The editor's own palette.
///
/// One theme drives the grid and the navigation surface together. They used to
/// be chosen separately, which is how a light picker ended up floating over a
/// dark editor — two halves of one window disagreeing about what the user asked
/// for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    pub bg: u32,
    pub fg: u32,
    pub line_nr: u32,
    pub cursor_line_nr: u32,
    pub status_fg: u32,
    pub status_bg: u32,
    pub visual: u32,
    pub search_fg: u32,
    pub search_bg: u32,
    pub error: u32,
    pub non_text: u32,
    pub heading: u32,
    pub code: u32,
    pub emphasis: u32,
    pub bullet: u32,
    pub link: u32,
    pub modified_fg: u32,
    pub modified_bg: u32,
    /// `cursorline`: a wash under the line the cursor is on, which has to stay
    /// under the text rather than compete with the visual selection.
    pub cursor_line_bg: u32,
}

impl Theme {
    pub fn dark() -> Self {
        Self {
            bg: 0x0F1117,
            fg: 0xD3D9E4,
            line_nr: 0x454C5E,
            cursor_line_nr: 0xE0C285,
            status_fg: 0xB9C2D6,
            // A status line that is a slab of saturated blue reads as a
            // selection, not as chrome. It belongs a step above the background,
            // not on top of the palette.
            status_bg: 0x1B2130,
            visual: 0x2B3350,
            search_fg: 0x11141C,
            search_bg: 0xE0C285,
            error: 0xE86B7E,
            non_text: 0x272D3A,
            heading: 0x7FA7F5,
            code: 0x93C97E,
            emphasis: 0xB38DEF,
            bullet: 0xE0C285,
            link: 0x6FC7EF,
            modified_fg: 0xE86B7E,
            modified_bg: 0x1B2130,
            cursor_line_bg: 0x161A24,
        }
    }

    pub fn light() -> Self {
        Self {
            bg: 0xF7F8FB,
            fg: 0x2B3140,
            line_nr: 0xA8B0C2,
            cursor_line_nr: 0x9A6B12,
            status_fg: 0x3D4557,
            status_bg: 0xE6EAF3,
            visual: 0xD3DEF7,
            search_fg: 0x2B3140,
            search_bg: 0xF3D9A0,
            error: 0xC0324B,
            non_text: 0xD8DDE8,
            heading: 0x2F5FD0,
            code: 0x2F7A3F,
            emphasis: 0x7040B8,
            bullet: 0x9A6B12,
            link: 0x1C6B93,
            modified_fg: 0xC0324B,
            modified_bg: 0xE6EAF3,
            cursor_line_bg: 0xECEFF6,
        }
    }

    pub fn named(name: &str) -> Self {
        match name {
            "light" => Self::light(),
            _ => Self::dark(),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
struct Cell {
    text: char,
    hl: u64,
    /// A wide character owns the cell after it; that cell carries an empty
    /// string over the protocol so the client does not draw the glyph twice.
    continuation: bool,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            text: ' ',
            hl: hl::DEFAULT,
            continuation: false,
        }
    }
}

/// Display width in grid cells.
///
/// Columns in the editing core count characters; cells here count screen
/// positions. A CJK character is one of the former and two of the latter, and
/// conflating them puts the cursor one cell left of where it looks.
pub fn char_width(ch: char) -> usize {
    let c = ch as u32;
    let wide = matches!(c,
        0x1100..=0x115F
        | 0x2E80..=0x303E
        | 0x3041..=0x33FF
        | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF
        | 0xA000..=0xA4CF
        | 0xAC00..=0xD7A3
        | 0xF900..=0xFAFF
        | 0xFE30..=0xFE6F
        | 0xFF00..=0xFF60
        | 0xFFE0..=0xFFE6
        | 0x1F300..=0x1F64F
        | 0x1F900..=0x1F9FF
        | 0x20000..=0x2FFFD
        | 0x30000..=0x3FFFD);
    if wide {
        2
    } else if c == 0 {
        1
    } else {
        1
    }
}

pub struct GridPainter {
    grid: u64,
    cols: usize,
    rows: usize,
    previous: Vec<Cell>,
    current: Vec<Cell>,
    painted: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderCell {
    pub text: char,
    pub hl: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderRow {
    pub cells: Vec<RenderCell>,
    pub fill: u64,
}

pub struct FocusRender<'a> {
    pub visual_range: Option<((usize, usize), (usize, usize))>,
    pub visual_kind: Option<Visual>,
    pub last_search: Option<&'a str>,
    pub highlight_search: bool,
}

pub fn render_window_lines(
    view: &WindowView,
    buffer: &Buffer,
    options: &Options,
    focus: Option<FocusRender<'_>>,
) -> Vec<RenderRow> {
    let mut rows = Vec::new();
    let gutter = gutter_width(buffer.line_count());
    let visual = focus.as_ref().and_then(|state| state.visual_range);
    let visual_kind = focus.as_ref().and_then(|state| state.visual_kind);
    let last_search = focus.as_ref().and_then(|state| state.last_search);
    let highlight_search = focus.as_ref().is_some_and(|state| state.highlight_search);

    for screen_row in 0..view.rows {
        let line = view.top_line + screen_row;
        if line >= buffer.line_count() {
            rows.push(RenderRow {
                cells: vec![RenderCell {
                    text: '~',
                    hl: hl::NON_TEXT,
                }],
                fill: hl::DEFAULT,
            });
            continue;
        }

        let mut cells = Vec::new();
        let number = match options.line_number(line, view.cursor.0) {
            Some(value) => format!("{:>width$} ", value, width = gutter - 1),
            None => " ".repeat(gutter),
        };
        let number_hl = if line == view.cursor.0 {
            hl::CURSOR_LINE_NR
        } else {
            hl::LINE_NR
        };
        for ch in number.chars() {
            cells.push(RenderCell {
                text: ch,
                hl: number_hl,
            });
        }

        let text = buffer.line_text(line);
        let spans = markdown_spans(&text);
        let on_cursor_line = options.cursorline && line == view.cursor.0;
        let rest = if on_cursor_line {
            hl::CURSOR_LINE
        } else {
            hl::DEFAULT
        };
        let trailing_from = if options.list {
            text.char_indices()
                .rev()
                .take_while(|(_, c)| c.is_whitespace())
                .last()
                .map(|_| {
                    text.chars().count()
                        - text.chars().rev().take_while(|c| c.is_whitespace()).count()
                })
        } else {
            None
        };
        let mut col = gutter;
        for (index, ch) in text.chars().enumerate() {
            if col >= view.cols {
                break;
            }
            let mut style = spans.get(index).copied().unwrap_or(rest);
            let mut ch = ch;
            if options.list {
                if ch == '\t' {
                    ch = options.listchar_tab;
                    style = hl::NON_TEXT;
                } else if trailing_from.is_some_and(|from| index >= from) {
                    ch = options.listchar_trail;
                    style = hl::NON_TEXT;
                }
            }
            if in_visual(visual, visual_kind, line, index, buffer) {
                style = hl::VISUAL;
            } else if highlight_search {
                if let Some(pattern) = last_search {
                    if !pattern.is_empty() && matches_at(&text, pattern, index) {
                        style = hl::SEARCH;
                    }
                }
            }
            cells.push(RenderCell {
                text: ch,
                hl: style,
            });
            col += char_width(ch);
        }
        let fill = if visual_kind == Some(Visual::Line)
            && visual.is_some_and(|(a, b)| line >= a.0 && line <= b.0)
        {
            hl::VISUAL
        } else {
            rest
        };
        rows.push(RenderRow { cells, fill });
    }
    rows
}

impl GridPainter {
    pub fn new(grid: u64, cols: usize, rows: usize) -> Self {
        let cols = cols.max(1);
        let rows = rows.max(1);
        Self {
            grid,
            cols,
            rows,
            previous: vec![Cell::default(); cols * rows],
            current: vec![Cell::default(); cols * rows],
            painted: false,
        }
    }

    pub fn grid(&self) -> u64 {
        self.grid
    }

    pub fn cols(&self) -> usize {
        self.cols
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        self.cols = cols.max(1);
        self.rows = rows.max(1);
        self.previous = vec![Cell::default(); self.cols * self.rows];
        self.current = vec![Cell::default(); self.cols * self.rows];
        self.painted = false;
    }

    fn clear_current(&mut self) {
        for cell in &mut self.current {
            *cell = Cell::default();
        }
    }

    fn put(&mut self, row: usize, col: usize, ch: char, hl: u64) -> usize {
        if row >= self.rows || col >= self.cols {
            return col + 1;
        }
        let width = char_width(ch);
        self.current[row * self.cols + col] = Cell {
            text: ch,
            hl,
            continuation: false,
        };
        if width == 2 && col + 1 < self.cols {
            self.current[row * self.cols + col + 1] = Cell {
                text: ' ',
                hl,
                continuation: true,
            };
        }
        col + width
    }

    fn write(&mut self, row: usize, mut col: usize, text: &str, hl: u64) -> usize {
        for ch in text.chars() {
            if col >= self.cols {
                break;
            }
            col = self.put(row, col, ch, hl);
        }
        col
    }

    fn fill(&mut self, row: usize, from: usize, hl: u64) {
        for col in from..self.cols {
            self.current[row * self.cols + col] = Cell {
                text: ' ',
                hl,
                continuation: false,
            };
        }
    }

    fn diff(&mut self) -> Vec<RedrawEvent> {
        let mut events = Vec::new();
        if !self.painted {
            self.painted = true;
            events.push(("grid_clear".to_string(), vec![Value::from(self.grid)]));
        }
        for row in 0..self.rows {
            let from = row * self.cols;
            let to = from + self.cols;
            if self.previous[from..to] == self.current[from..to] {
                continue;
            }
            events.push((
                "grid_line".to_string(),
                vec![
                    Value::from(self.grid),
                    Value::from(row as u64),
                    Value::from(0u64),
                    Value::Array(encode_row(&self.current[from..to])),
                    Value::from(false),
                ],
            ));
        }
        self.previous.clone_from(&self.current);
        events
    }
}

pub struct Painter {
    theme: Theme,
    grid: GridPainter,
    last_mode: Option<&'static str>,
    last_cmdline: Option<String>,
    last_message: Option<String>,
    options: UiOptions,
}

impl Painter {
    pub fn new(cols: usize, rows: usize, options: UiOptions) -> Self {
        Self::themed(cols, rows, options, Theme::dark())
    }

    pub fn themed(cols: usize, rows: usize, options: UiOptions, theme: Theme) -> Self {
        let cols = cols.max(1);
        let rows = rows.max(1);
        Self {
            theme,
            grid: GridPainter::new(GRID, cols, rows),
            last_mode: None,
            last_cmdline: None,
            last_message: None,
            options,
        }
    }

    pub fn cols(&self) -> usize {
        self.grid.cols()
    }

    pub fn rows(&self) -> usize {
        self.grid.rows()
    }

    pub fn options(&self) -> UiOptions {
        self.options
    }

    /// How many rows of buffer text fit: everything except the status line, and
    /// except the message line when the client is not drawing it itself.
    pub fn text_rows(&self) -> usize {
        let reserved = if self.options.ext_messages { 1 } else { 2 };
        self.rows().saturating_sub(reserved).max(1)
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        self.grid.resize(cols, rows);
    }

    /// The events a freshly attached client needs before any content.
    pub fn attach_events(&self) -> Vec<RedrawEvent> {
        let mut events = vec![
            (
                "grid_resize".to_string(),
                vec![
                    Value::from(self.grid.grid()),
                    Value::from(self.cols() as u64),
                    Value::from(self.rows() as u64),
                ],
            ),
            (
                "default_colors_set".to_string(),
                vec![
                    Value::from(self.theme.fg),
                    Value::from(self.theme.bg),
                    Value::from(self.theme.fg),
                    Value::from(-1i64),
                    Value::from(-1i64),
                ],
            ),
        ];
        for (id, attrs) in highlight_table(self.theme) {
            events.push((
                "hl_attr_define".to_string(),
                vec![Value::from(id), attrs.clone(), attrs, Value::Array(vec![])],
            ));
        }
        events.push(("mode_info_set".to_string(), mode_info()));
        events
    }

    /// Everything that changed since the last call, ending in `flush`.
    pub fn render(&mut self, editor: &Editor) -> Vec<RedrawEvent> {
        self.compose(editor);
        let mut events = self.diff();

        let mode = editor.mode.protocol_name();
        if self.last_mode != Some(mode) {
            self.last_mode = Some(mode);
            events.push((
                "mode_change".to_string(),
                vec![Value::from(mode), Value::from(mode_index(editor.mode))],
            ));
        }

        events.extend(self.external_surfaces(editor));

        let (row, col) = self.cursor_cell(editor);
        events.push((
            "grid_cursor_goto".to_string(),
            vec![
                Value::from(self.grid.grid()),
                Value::from(row as u64),
                Value::from(col as u64),
            ],
        ));
        events.push(("flush".to_string(), vec![]));
        events
    }

    /// `cmdline_show` / `msg_show`, for a client that asked to draw those itself.
    fn external_surfaces(&mut self, editor: &Editor) -> Vec<RedrawEvent> {
        let mut events = Vec::new();
        if self.options.ext_cmdline {
            let showing = match editor.mode {
                Mode::Cmdline => Some(editor.cmdline.clone()),
                _ => None,
            };
            if showing != self.last_cmdline {
                match &showing {
                    Some(content) => events.push((
                        "cmdline_show".to_string(),
                        vec![
                            Value::Array(vec![Value::Array(vec![
                                Value::from(hl::DEFAULT),
                                Value::from(content.as_str()),
                            ])]),
                            Value::from(content.chars().count() as u64),
                            Value::from(editor.cmdline_prefix.to_string()),
                            Value::from(""),
                            Value::from(0u64),
                            Value::from(0u64),
                        ],
                    )),
                    None => events.push(("cmdline_hide".to_string(), vec![])),
                }
                self.last_cmdline = showing;
            }
        }
        if self.options.ext_messages {
            let text = editor.message.as_ref().map(|m| m.text.clone());
            if text != self.last_message {
                match (&text, editor.message.as_ref()) {
                    (Some(content), Some(message)) => events.push((
                        "msg_show".to_string(),
                        vec![
                            Value::from(if message.error { "emsg" } else { "" }),
                            Value::Array(vec![Value::Array(vec![
                                Value::from(if message.error {
                                    hl::ERROR
                                } else {
                                    hl::DEFAULT
                                }),
                                Value::from(content.as_str()),
                            ])]),
                            Value::from(false),
                        ],
                    )),
                    _ => events.push(("msg_clear".to_string(), vec![])),
                }
                self.last_message = text;
            }
        }
        events
    }

    // --- composition ------------------------------------------------------

    fn compose(&mut self, editor: &Editor) {
        self.grid.clear_current();
        let text_rows = self.text_rows();
        let mut view = editor.focused_view().clone();
        view.cursor = editor.cursor;
        view.top_line = editor.top_line;
        view.rows = text_rows;
        view.cols = self.cols();
        let rows = render_window_lines(
            &view,
            &editor.buffer,
            &editor.options,
            Some(FocusRender {
                visual_range: editor.visual_range(),
                visual_kind: editor.visual_kind(),
                last_search: editor.last_search.as_deref(),
                highlight_search: editor.highlight_search(),
            }),
        );

        for (screen_row, row) in rows.iter().enumerate() {
            let mut col = 0;
            for cell in &row.cells {
                if col >= self.cols() {
                    break;
                }
                col = self.grid.put(screen_row, col, cell.text, cell.hl);
            }
            self.grid.fill(screen_row, col, row.fill);
        }

        self.compose_status(editor);
        if !self.options.ext_messages {
            self.compose_message(editor);
        }
    }

    fn compose_status(&mut self, editor: &Editor) {
        let row = if self.options.ext_messages {
            self.rows() - 1
        } else {
            self.rows() - 2
        };
        if row >= self.rows() {
            return;
        }
        self.grid.fill(row, 0, hl::STATUS);
        let name = editor.buffer.name();
        let left = format!(
            " {} {}",
            editor.mode.short_name().to_uppercase(),
            shorten(&name, self.cols() / 2)
        );
        let mut col = self.grid.write(row, 0, &left, hl::STATUS);
        if editor.buffer.modified() {
            col = self.grid.write(row, col, " [+]", hl::MODIFIED);
        }
        let right = format!(
            "{}:{}  {}/{} ",
            editor.cursor.0 + 1,
            editor.cursor.1 + 1,
            editor.cursor.0 + 1,
            editor.buffer.line_count()
        );
        let start = self.cols().saturating_sub(right.chars().count());
        if start > col {
            self.grid.write(row, start, &right, hl::STATUS);
        }
    }

    fn compose_message(&mut self, editor: &Editor) {
        let row = self.rows() - 1;
        self.grid.fill(row, 0, hl::DEFAULT);
        match editor.mode {
            Mode::Cmdline if !self.options.ext_cmdline => {
                let line = format!("{}{}", editor.cmdline_prefix, editor.cmdline);
                self.grid.write(row, 0, &line, hl::DEFAULT);
            }
            _ => {
                if let Some(message) = &editor.message {
                    let style = if message.error {
                        hl::ERROR
                    } else {
                        hl::DEFAULT
                    };
                    self.grid.write(row, 0, &message.text, style);
                }
            }
        }
    }

    fn cursor_cell(&self, editor: &Editor) -> (usize, usize) {
        if editor.mode == Mode::Cmdline && !self.options.ext_cmdline {
            let col = 1 + editor.cmdline.chars().count();
            return (self.rows() - 1, col.min(self.cols() - 1));
        }
        let gutter = gutter_width(editor.buffer.line_count());
        let row = editor
            .cursor
            .0
            .saturating_sub(editor.top_line)
            .min(self.text_rows() - 1);
        let text = editor.buffer.line(editor.cursor.0);
        let width = crate::textpos::char_to_cell(text, editor.cursor.1);
        (row, (gutter + width).min(self.cols() - 1))
    }

    fn diff(&mut self) -> Vec<RedrawEvent> {
        self.grid.diff()
    }
}

/// One `grid_line` payload: `[text, hl_id, repeat]`, with `hl_id` omitted when
/// it repeats and `repeat` omitted when it is one.
fn encode_row(cells: &[Cell]) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    let mut last_hl: Option<u64> = None;
    let mut index = 0;
    while index < cells.len() {
        let cell = &cells[index];
        let text = if cell.continuation {
            String::new()
        } else {
            cell.text.to_string()
        };
        let mut run = 1;
        while index + run < cells.len()
            && !cells[index + run].continuation
            && !cell.continuation
            && cells[index + run].text == cell.text
            && cells[index + run].hl == cell.hl
        {
            run += 1;
        }
        let mut parts = vec![Value::from(text)];
        if last_hl != Some(cell.hl) || run > 1 {
            parts.push(Value::from(cell.hl));
            last_hl = Some(cell.hl);
        }
        if run > 1 {
            parts.push(Value::from(run as u64));
        }
        out.push(Value::Array(parts));
        index += run;
    }
    out
}

pub fn gutter_width(lines: usize) -> usize {
    lines.to_string().len().max(3) + 1
}

fn shorten(text: &str, limit: usize) -> String {
    let count = text.chars().count();
    if count <= limit.max(8) {
        return text.to_string();
    }
    let keep = limit.max(8) - 1;
    let skipped = count - keep;
    format!("…{}", text.chars().skip(skipped).collect::<String>())
}

fn matches_at(text: &str, pattern: &str, index: usize) -> bool {
    let chars: Vec<char> = text.chars().collect();
    let needle: Vec<char> = pattern.chars().collect();
    if needle.is_empty() {
        return false;
    }
    // A cell is highlighted when it falls inside any occurrence, so the scan
    // starts far enough back to catch one that began earlier on the line.
    let first = index.saturating_sub(needle.len() - 1);
    (first..=index).any(|start| {
        start + needle.len() <= chars.len() && chars[start..start + needle.len()] == needle[..]
    })
}

fn in_visual(
    range: Option<((usize, usize), (usize, usize))>,
    kind: Option<Visual>,
    line: usize,
    col: usize,
    buffer: &crate::core::Buffer,
) -> bool {
    let Some((start, end)) = range else {
        return false;
    };
    match kind {
        Some(Visual::Line) => line >= start.0 && line <= end.0,
        Some(Visual::Char) => {
            let _ = buffer;
            if line < start.0 || line > end.0 {
                return false;
            }
            let after_start = line > start.0 || col >= start.1;
            let before_end = line < end.0 || col <= end.1;
            after_start && before_end
        }
        None => false,
    }
}

/// Line-level markdown styling.
///
/// `pin primary_object` makes the markdown note the thing being edited, so the
/// default styling is the note's, not a programming language's. It is
/// deliberately shallow: what it cannot classify stays default rather than
/// guessing.
fn markdown_spans(text: &str) -> Vec<u64> {
    let chars: Vec<char> = text.chars().collect();
    let mut spans = vec![hl::DEFAULT; chars.len()];
    let trimmed = text.trim_start();
    let indent = chars.len() - trimmed.chars().count();

    if trimmed.starts_with('#') {
        return vec![hl::HEADING; chars.len()];
    }
    if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
        return vec![hl::CODE; chars.len()];
    }
    if trimmed.starts_with("> ") {
        return vec![hl::EMPHASIS; chars.len()];
    }
    if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ ") {
        for slot in spans.iter_mut().skip(indent).take(1) {
            *slot = hl::BULLET;
        }
    }

    let mut index = 0;
    while index < chars.len() {
        match chars[index] {
            '`' => {
                if let Some(close) = (index + 1..chars.len()).find(|&i| chars[i] == '`') {
                    for slot in spans.iter_mut().take(close + 1).skip(index) {
                        *slot = hl::CODE;
                    }
                    index = close + 1;
                    continue;
                }
            }
            '*' | '_' if index + 1 < chars.len() && chars[index + 1] == chars[index] => {
                let marker = chars[index];
                if let Some(close) = (index + 2..chars.len().saturating_sub(1))
                    .find(|&i| chars[i] == marker && chars[i + 1] == marker)
                {
                    for slot in spans.iter_mut().take(close + 2).skip(index) {
                        *slot = hl::EMPHASIS;
                    }
                    index = close + 2;
                    continue;
                }
            }
            '[' => {
                if let Some(close) = (index + 1..chars.len()).find(|&i| chars[i] == ']') {
                    for slot in spans.iter_mut().take(close + 1).skip(index) {
                        *slot = hl::LINK;
                    }
                    index = close + 1;
                    continue;
                }
            }
            _ => {}
        }
        index += 1;
    }
    spans
}

fn attrs(fg: Option<u32>, bg: Option<u32>, bold: bool, italic: bool) -> Value {
    let mut map = Vec::new();
    if let Some(fg) = fg {
        map.push((Value::from("foreground"), Value::from(fg)));
    }
    if let Some(bg) = bg {
        map.push((Value::from("background"), Value::from(bg)));
    }
    if bold {
        map.push((Value::from("bold"), Value::from(true)));
    }
    if italic {
        map.push((Value::from("italic"), Value::from(true)));
    }
    Value::Map(map)
}

fn highlight_table(theme: Theme) -> Vec<(u64, Value)> {
    vec![
        (hl::LINE_NR, attrs(Some(theme.line_nr), None, false, false)),
        (
            hl::CURSOR_LINE_NR,
            attrs(Some(theme.cursor_line_nr), None, true, false),
        ),
        (
            hl::STATUS,
            attrs(Some(theme.status_fg), Some(theme.status_bg), false, false),
        ),
        (hl::VISUAL, attrs(None, Some(theme.visual), false, false)),
        (
            hl::SEARCH,
            attrs(Some(theme.search_fg), Some(theme.search_bg), false, false),
        ),
        (hl::ERROR, attrs(Some(theme.error), None, true, false)),
        (
            hl::NON_TEXT,
            attrs(Some(theme.non_text), None, false, false),
        ),
        (hl::HEADING, attrs(Some(theme.heading), None, true, false)),
        (hl::CODE, attrs(Some(theme.code), None, false, false)),
        (hl::EMPHASIS, attrs(Some(theme.emphasis), None, false, true)),
        (hl::BULLET, attrs(Some(theme.bullet), None, true, false)),
        (hl::LINK, attrs(Some(theme.link), None, false, false)),
        (
            hl::MODIFIED,
            attrs(
                Some(theme.modified_fg),
                Some(theme.modified_bg),
                true,
                false,
            ),
        ),
        (
            hl::CURSOR_LINE,
            attrs(None, Some(theme.cursor_line_bg), false, false),
        ),
    ]
}

fn mode_index(mode: Mode) -> u64 {
    match mode {
        Mode::Normal => 0,
        Mode::Insert => 1,
        Mode::Visual(_) => 2,
        Mode::Cmdline => 3,
    }
}

fn mode_info() -> Vec<Value> {
    let entry = |name: &str, shape: &str| {
        Value::Map(vec![
            (Value::from("name"), Value::from(name)),
            (Value::from("cursor_shape"), Value::from(shape)),
            (
                Value::from("cell_percentage"),
                Value::from(if shape == "horizontal" { 20u64 } else { 100 }),
            ),
        ])
    };
    vec![
        Value::from(true),
        Value::Array(vec![
            entry("normal", "block"),
            entry("insert", "vertical"),
            entry("visual", "block"),
            entry("cmdline_normal", "horizontal"),
        ]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Buffer;

    fn painter() -> Painter {
        Painter::new(40, 8, UiOptions::none())
    }

    fn row_text(events: &[RedrawEvent], row: u64) -> Option<String> {
        events.iter().rev().find_map(|(name, args)| {
            if name != "grid_line" || args[1].as_u64() != Some(row) {
                return None;
            }
            let cells = args[3].as_array()?;
            let mut out = String::new();
            for cell in cells {
                let parts = cell.as_array()?;
                let text = parts[0].as_str().unwrap_or("");
                let repeat = parts.get(2).and_then(Value::as_u64).unwrap_or(1);
                for _ in 0..repeat {
                    out.push_str(text);
                }
            }
            Some(out)
        })
    }

    /// An editor with `number` on, since the gutter is what these read.
    /// Without a config the gutter is blank, which is vim's own default.
    fn numbered(text: &str) -> Editor {
        let mut editor = Editor::new(Buffer::from_text(text));
        editor.options.number = true;
        editor
    }

    #[test]
    fn render_window_lines_can_be_tested_without_an_editor() {
        let buffer = Buffer::from_text("# title\nplain  \n");
        let mut options = Options::default();
        options.number = true;
        options.list = true;
        options.listchar_trail = '.';
        let view = WindowView {
            id: crate::core::WindowId(1),
            grid: 1,
            buffer: crate::core::BufferId(1),
            cursor: (1, 0),
            desired_col: 0,
            top_line: 0,
            cols: 20,
            rows: 3,
        };
        let rows = render_window_lines(
            &view,
            &buffer,
            &options,
            Some(FocusRender {
                visual_range: None,
                visual_kind: None,
                last_search: Some("ain"),
                highlight_search: true,
            }),
        );
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].cells[4].text, '#');
        assert_eq!(rows[0].cells[4].hl, hl::HEADING);
        assert!(rows[1]
            .cells
            .iter()
            .any(|cell| cell.text == '.' && cell.hl == hl::NON_TEXT));
        assert_eq!(rows[2].cells[0].text, '~');
    }

    #[test]
    fn without_a_config_the_gutter_is_blank_as_vim_leaves_it() {
        let mut p = painter();
        let editor = Editor::new(Buffer::from_text("hello\n"));
        assert_eq!(
            row_text(&p.render(&editor), 0).unwrap().trim_end(),
            "    hello"
        );
    }

    #[test]
    fn the_first_paint_carries_the_buffer_text() {
        let mut p = painter();
        let editor = numbered("hello\n");
        let events = p.render(&editor);
        assert_eq!(row_text(&events, 0).unwrap().trim_end(), "  1 hello");
    }

    #[test]
    fn an_unchanged_row_is_not_sent_again() {
        let mut p = painter();
        let mut editor = numbered("one\ntwo\n");
        p.render(&editor);
        editor.feed_str("jx");
        let events = p.render(&editor);
        assert_eq!(row_text(&events, 1).unwrap().trim_end(), "  2 wo");
        // Row 0 *does* change here — the cursor line number moved off it — but
        // the empty rows past the end of the buffer did not.
        assert!(row_text(&events, 3).is_none(), "row 3 was resent unchanged");
        assert!(row_text(&events, 4).is_none(), "row 4 was resent unchanged");
    }

    #[test]
    fn the_cursor_lands_past_a_wide_character_by_two_cells() {
        let mut p = painter();
        let mut editor = Editor::new(Buffer::from_text("あい\n"));
        editor.feed_str("l");
        let events = p.render(&editor);
        let goto = events
            .iter()
            .rev()
            .find(|(name, _)| name == "grid_cursor_goto")
            .unwrap();
        assert_eq!(goto.1[2].as_u64(), Some(gutter_width(1) as u64 + 2));
    }

    #[test]
    fn a_wide_character_leaves_an_empty_continuation_cell() {
        let mut p = painter();
        let editor = Editor::new(Buffer::from_text("あ\n"));
        let events = p.render(&editor);
        let line = events
            .iter()
            .find(|(name, args)| name == "grid_line" && args[1].as_u64() == Some(0))
            .unwrap();
        let cells = line.1[3].as_array().unwrap();
        let texts: Vec<&str> = cells
            .iter()
            .map(|c| c.as_array().unwrap()[0].as_str().unwrap())
            .collect();
        let wide = texts.iter().position(|t| *t == "あ").unwrap();
        assert_eq!(texts[wide + 1], "");
    }

    #[test]
    fn mode_change_is_sent_only_when_the_mode_changed() {
        let mut p = painter();
        let mut editor = Editor::new(Buffer::from_text(""));
        p.render(&editor);
        editor.feed_str("i");
        let events = p.render(&editor);
        assert!(events.iter().any(|(name, _)| name == "mode_change"));
        let events = p.render(&editor);
        assert!(!events.iter().any(|(name, _)| name == "mode_change"));
    }

    #[test]
    fn every_render_ends_in_flush() {
        let mut p = painter();
        let editor = Editor::new(Buffer::from_text("x\n"));
        let events = p.render(&editor);
        assert_eq!(events.last().unwrap().0, "flush");
    }

    #[test]
    fn the_cmdline_goes_external_only_when_the_client_asked() {
        let mut editor = Editor::new(Buffer::from_text(""));
        editor.feed_str(":w");
        let mut external = Painter::new(
            40,
            8,
            UiOptions {
                ext_cmdline: true,
                ..UiOptions::none()
            },
        );
        let events = external.render(&editor);
        assert!(events.iter().any(|(name, _)| name == "cmdline_show"));

        let mut internal = painter();
        let events = internal.render(&editor);
        assert!(!events.iter().any(|(name, _)| name == "cmdline_show"));
        assert!(row_text(&events, 7).unwrap().starts_with(":w"));
    }

    #[test]
    fn markdown_headings_and_code_get_their_own_highlight() {
        let spans = markdown_spans("# title");
        assert!(spans.iter().all(|&s| s == hl::HEADING));
        let spans = markdown_spans("a `code` b");
        assert_eq!(spans[2], hl::CODE);
        assert_eq!(spans[0], hl::DEFAULT);
    }

    #[test]
    fn rows_past_the_end_of_the_buffer_show_a_tilde() {
        let mut p = painter();
        let editor = Editor::new(Buffer::from_text("one\n"));
        let events = p.render(&editor);
        assert!(row_text(&events, 1).unwrap().starts_with('~'));
    }
}
