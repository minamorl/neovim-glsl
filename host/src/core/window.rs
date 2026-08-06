//! Window and tab layout model.
//!
//! The renderer still receives the same single-grid output in W1. This module
//! is the edit-core model W2 can connect to multigrid events.

use std::collections::BTreeMap;

use super::buffers::BufferId;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct WindowId(pub u64);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    Left,
    Down,
    Up,
    Right,
}

impl Direction {
    pub fn from_vim(ch: char) -> Option<Self> {
        match ch {
            'h' => Some(Self::Left),
            'j' => Some(Self::Down),
            'k' => Some(Self::Up),
            'l' => Some(Self::Right),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Axis {
    Horizontal,
    Vertical,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Layout {
    Leaf(WindowId),
    Row(Vec<Layout>),
    Col(Vec<Layout>),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Rect {
    pub row: usize,
    pub col: usize,
    pub rows: usize,
    pub cols: usize,
}

impl Rect {
    pub fn new(row: usize, col: usize, rows: usize, cols: usize) -> Self {
        Self {
            row,
            col,
            rows,
            cols,
        }
    }

    fn right(self) -> usize {
        self.col + self.cols
    }

    fn bottom(self) -> usize {
        self.row + self.rows
    }

    fn center_x(self) -> isize {
        (self.col * 2 + self.cols) as isize
    }

    fn center_y(self) -> isize {
        (self.row * 2 + self.rows) as isize
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct LayoutRects {
    pub text: BTreeMap<WindowId, Rect>,
    pub status: Vec<Rect>,
    pub separators: Vec<Rect>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct WindowView {
    pub id: WindowId,
    pub grid: u64,
    pub buffer: BufferId,
    pub cursor: (usize, usize),
    pub desired_col: usize,
    pub top_line: usize,
    pub cols: usize,
    pub rows: usize,
}

impl WindowView {
    pub fn new(id: WindowId, grid: u64, buffer: BufferId) -> Self {
        Self {
            id,
            grid,
            buffer,
            cursor: (0, 0),
            desired_col: 0,
            top_line: 0,
            cols: 80,
            rows: 24,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
struct Tab {
    layout: Layout,
    focus: WindowId,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Tabs {
    tabs: Vec<Tab>,
    current: usize,
    next_window: u64,
    next_grid: u64,
    last_area: Rect,
    last_splitbelow: bool,
    last_splitright: bool,
}

impl Tabs {
    pub fn new(first: WindowId) -> Self {
        Self {
            tabs: vec![Tab {
                layout: Layout::Leaf(first),
                focus: first,
            }],
            current: 0,
            next_window: first.0 + 1,
            next_grid: 3,
            last_area: Rect::new(0, 0, 24, 80),
            last_splitbelow: true,
            last_splitright: true,
        }
    }

    pub fn focus(&self) -> WindowId {
        self.tabs[self.current].focus
    }

    pub fn current_layout(&self) -> &Layout {
        &self.tabs[self.current].layout
    }

    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    pub fn current_index(&self) -> usize {
        self.current
    }

    pub fn remember_geometry(&mut self, area: Rect, splitbelow: bool, splitright: bool) {
        self.last_area = area;
        self.last_splitbelow = splitbelow;
        self.last_splitright = splitright;
    }

    pub fn rects(&self, area: Rect, _splitbelow: bool, _splitright: bool) -> LayoutRects {
        let mut out = LayoutRects::default();
        layout_rects(self.current_layout(), area, &mut out);
        out
    }

    pub fn split_horizontal(&mut self, splitbelow: bool) -> WindowId {
        self.split(Axis::Vertical, splitbelow)
    }

    pub fn split_vertical(&mut self, splitright: bool) -> WindowId {
        self.split(Axis::Horizontal, splitright)
    }

    pub fn split_vertical_before(&mut self) -> WindowId {
        self.split(Axis::Horizontal, false)
    }

    pub fn focus_window(&mut self, id: WindowId) -> bool {
        if self.current_layout().leaves().contains(&id) {
            self.tabs[self.current].focus = id;
            true
        } else {
            false
        }
    }

    pub fn close(&mut self) -> Option<WindowId> {
        let focus = self.focus();
        if self.current_layout().leaves().len() <= 1 {
            return None;
        }
        let tab = &mut self.tabs[self.current];
        remove_leaf(&mut tab.layout, focus);
        collapse(&mut tab.layout);
        tab.focus = tab.layout.leaves().into_iter().next().unwrap_or(focus);
        Some(tab.focus)
    }

    pub fn only(&mut self) {
        let focus = self.focus();
        let tab = &mut self.tabs[self.current];
        tab.layout = Layout::Leaf(focus);
        tab.focus = focus;
    }

    pub fn focus_dir(&mut self, dir: Direction) -> Option<WindowId> {
        self.focus_dir_in(
            dir,
            self.last_area,
            self.last_splitbelow,
            self.last_splitright,
        )
    }

    pub fn focus_dir_in(
        &mut self,
        dir: Direction,
        area: Rect,
        splitbelow: bool,
        splitright: bool,
    ) -> Option<WindowId> {
        self.remember_geometry(area, splitbelow, splitright);
        let rects = self.rects(area, splitbelow, splitright);
        let current = rects.text.get(&self.focus()).copied()?;
        let mut candidates: Vec<(WindowId, Rect)> = rects
            .text
            .into_iter()
            .filter(|(id, rect)| *id != self.focus() && in_direction(current, *rect, dir))
            .collect();
        candidates.sort_by_key(|(_, rect)| distance(current, *rect, dir));
        let id = candidates.first().map(|(id, _)| *id)?;
        self.tabs[self.current].focus = id;
        Some(id)
    }

    pub fn cycle_focus(&mut self) -> WindowId {
        let leaves = self.current_layout().leaves();
        let at = leaves
            .iter()
            .position(|id| *id == self.focus())
            .unwrap_or(0);
        let next = leaves[(at + 1) % leaves.len()];
        self.tabs[self.current].focus = next;
        next
    }

    pub fn next_tab(&mut self) -> WindowId {
        self.current = (self.current + 1) % self.tabs.len();
        self.focus()
    }

    pub fn prev_tab(&mut self) -> WindowId {
        self.current = (self.current + self.tabs.len() - 1) % self.tabs.len();
        self.focus()
    }

    pub fn new_tab(&mut self) -> WindowId {
        let id = self.alloc_window();
        self.tabs.push(Tab {
            layout: Layout::Leaf(id),
            focus: id,
        });
        self.current = self.tabs.len() - 1;
        id
    }

    pub fn close_tab(&mut self) -> Option<Vec<WindowId>> {
        if self.tabs.len() <= 1 {
            return None;
        }
        let removed = self.tabs.remove(self.current).layout.leaves();
        if self.current >= self.tabs.len() {
            self.current = self.tabs.len() - 1;
        }
        Some(removed)
    }

    fn split(&mut self, axis: Axis, after: bool) -> WindowId {
        let id = self.alloc_window();
        let focus = self.focus();
        let tab = &mut self.tabs[self.current];
        insert_split(&mut tab.layout, focus, id, axis, after);
        tab.focus = id;
        id
    }

    fn alloc_window(&mut self) -> WindowId {
        let id = WindowId(self.next_window);
        self.next_window += 1;
        self.next_grid += 1;
        id
    }

    pub fn grid_for_new_window(&self) -> u64 {
        self.next_grid - 1
    }
}

impl Layout {
    pub fn leaves(&self) -> Vec<WindowId> {
        match self {
            Layout::Leaf(id) => vec![*id],
            Layout::Row(children) | Layout::Col(children) => {
                children.iter().flat_map(Layout::leaves).collect()
            }
        }
    }
}

fn insert_split(
    layout: &mut Layout,
    target: WindowId,
    new: WindowId,
    axis: Axis,
    after: bool,
) -> bool {
    match layout {
        Layout::Leaf(id) if *id == target => {
            let pair = if after {
                vec![Layout::Leaf(target), Layout::Leaf(new)]
            } else {
                vec![Layout::Leaf(new), Layout::Leaf(target)]
            };
            *layout = match axis {
                Axis::Horizontal => Layout::Row(pair),
                Axis::Vertical => Layout::Col(pair),
            };
            true
        }
        Layout::Row(children) if axis == Axis::Horizontal => {
            insert_into_children(children, target, new, after)
        }
        Layout::Col(children) if axis == Axis::Vertical => {
            insert_into_children(children, target, new, after)
        }
        Layout::Row(children) | Layout::Col(children) => children
            .iter_mut()
            .any(|child| insert_split(child, target, new, axis, after)),
        _ => false,
    }
}

fn insert_into_children(
    children: &mut Vec<Layout>,
    target: WindowId,
    new: WindowId,
    after: bool,
) -> bool {
    for index in 0..children.len() {
        if children[index].leaves().contains(&target) {
            if matches!(children[index], Layout::Leaf(id) if id == target) {
                let at = index + usize::from(after);
                children.insert(at, Layout::Leaf(new));
                return true;
            }
            let axis = if matches!(children[index], Layout::Row(_)) {
                Axis::Horizontal
            } else {
                Axis::Vertical
            };
            return insert_split(&mut children[index], target, new, axis, after);
        }
    }
    false
}

fn remove_leaf(layout: &mut Layout, target: WindowId) -> bool {
    match layout {
        Layout::Leaf(id) => *id == target,
        Layout::Row(children) | Layout::Col(children) => {
            children.retain_mut(|child| !remove_leaf(child, target));
            false
        }
    }
}

fn collapse(layout: &mut Layout) {
    match layout {
        Layout::Row(children) | Layout::Col(children) => {
            for child in children.iter_mut() {
                collapse(child);
            }
            if children.len() == 1 {
                *layout = children.remove(0);
            }
        }
        Layout::Leaf(_) => {}
    }
}

fn layout_rects(layout: &Layout, area: Rect, out: &mut LayoutRects) {
    match layout {
        Layout::Leaf(id) => {
            let text_rows = area.rows.saturating_sub(1).max(1);
            out.text
                .insert(*id, Rect::new(area.row, area.col, text_rows, area.cols));
            if area.rows > 1 {
                out.status
                    .push(Rect::new(area.row + text_rows, area.col, 1, area.cols));
            }
        }
        Layout::Row(children) => {
            if children.is_empty() {
                return;
            }
            let separators = children.len().saturating_sub(1);
            let available = area.cols.saturating_sub(separators);
            let mut col = area.col;
            for (index, child) in children.iter().enumerate() {
                let width = split_size(available, children.len(), index);
                layout_rects(child, Rect::new(area.row, col, area.rows, width), out);
                col += width;
                if index + 1 < children.len() {
                    out.separators.push(Rect::new(area.row, col, area.rows, 1));
                    col += 1;
                }
            }
        }
        Layout::Col(children) => {
            if children.is_empty() {
                return;
            }
            let mut row = area.row;
            for (index, child) in children.iter().enumerate() {
                let height = split_size(area.rows, children.len(), index);
                layout_rects(child, Rect::new(row, area.col, height, area.cols), out);
                row += height;
            }
        }
    }
}

fn split_size(total: usize, parts: usize, index: usize) -> usize {
    total / parts + usize::from(index < total % parts)
}

fn in_direction(current: Rect, candidate: Rect, dir: Direction) -> bool {
    match dir {
        Direction::Left => candidate.right() <= current.col,
        Direction::Right => candidate.col >= current.right(),
        Direction::Up => candidate.bottom() <= current.row,
        Direction::Down => candidate.row >= current.bottom(),
    }
}

fn distance(current: Rect, candidate: Rect, dir: Direction) -> (usize, usize) {
    let primary = match dir {
        Direction::Left => current.col.saturating_sub(candidate.right()),
        Direction::Right => candidate.col.saturating_sub(current.right()),
        Direction::Up => current.row.saturating_sub(candidate.bottom()),
        Direction::Down => candidate.row.saturating_sub(current.bottom()),
    };
    let secondary = match dir {
        Direction::Left | Direction::Right => current.center_y().abs_diff(candidate.center_y()),
        Direction::Up | Direction::Down => current.center_x().abs_diff(candidate.center_x()),
    };
    (primary, secondary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grids_start_at_two_and_increase() {
        let mut tabs = Tabs::new(WindowId(1));
        let a = tabs.split_vertical(true);
        assert_eq!(a, WindowId(2));
        assert_eq!(tabs.grid_for_new_window(), 3);
        let b = tabs.split_horizontal(true);
        assert_eq!(b, WindowId(3));
        assert_eq!(tabs.grid_for_new_window(), 4);
    }

    #[test]
    fn rects_return_text_and_chrome_separately() {
        let mut tabs = Tabs::new(WindowId(1));
        let right = tabs.split_vertical(true);
        let rects = tabs.rects(Rect::new(0, 0, 10, 21), true, true);
        assert_eq!(rects.text[&WindowId(1)], Rect::new(0, 0, 9, 10));
        assert_eq!(rects.text[&right], Rect::new(0, 11, 9, 10));
        assert_eq!(rects.status.len(), 2);
        assert_eq!(rects.separators, vec![Rect::new(0, 10, 10, 1)]);
    }

    #[test]
    fn focus_uses_rectangles_not_tree_neighbours() {
        let mut tabs = Tabs::new(WindowId(1));
        let right = tabs.split_vertical(true);
        let bottom_right = tabs.split_horizontal(true);
        assert_eq!(tabs.focus(), bottom_right);
        assert_eq!(
            tabs.focus_dir_in(Direction::Up, Rect::new(0, 0, 12, 25), true, true),
            Some(right)
        );
        assert_eq!(
            tabs.focus_dir_in(Direction::Left, Rect::new(0, 0, 12, 25), true, true),
            Some(WindowId(1))
        );
    }

    #[test]
    fn close_and_only_keep_a_valid_focus() {
        let mut tabs = Tabs::new(WindowId(1));
        let second = tabs.split_vertical(true);
        assert_eq!(tabs.focus(), second);
        assert_eq!(tabs.close(), Some(WindowId(1)));
        assert_eq!(tabs.focus(), WindowId(1));
        tabs.split_horizontal(true);
        tabs.only();
        assert!(matches!(tabs.current_layout(), Layout::Leaf(_)));
    }
}
