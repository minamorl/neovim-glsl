//! Machine-readable Root-ui integration hypothesis.
//!
//! This projection is evidence, not an adoption decision. It keeps Neovim as
//! editor authority and records the Root-ui ownership questions as unresolved.
//!
//! What it projects is the composited screen — with `ext_multigrid` a cell can
//! come from any of Neovim's grids, so cells are identified by screen position
//! rather than by grid.

use std::path::Path;

use serde::Serialize;
use ulid::Ulid;

use crate::screen::{Placement, Screen, GLOBAL_GRID};

#[derive(Serialize)]
struct Evaluation<'a> {
    schema: &'static str,
    trace_id: String,
    evaluation_candidate: bool,
    canonical_integration: bool,
    adoption_decision: &'static str,
    editor_basis: &'static str,
    ownership: Ownership,
    root_ui_constraints_observed: RootUiConstraints,
    scene: Scene<'a>,
}

#[derive(Serialize)]
struct Ownership {
    buffer_and_editing_semantics: &'static str,
    visual_primitive_owner: &'static str,
    text_editing_host_port_owner: &'static str,
}

#[derive(Serialize)]
struct RootUiConstraints {
    react_footprint: &'static str,
    text_editing_host_port: &'static str,
    phase_order: &'static str,
}

#[derive(Serialize)]
struct Scene<'a> {
    kind: &'static str,
    root_ui_primitive_program: bool,
    cols: usize,
    rows: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    cursor: Option<Cursor>,
    /// What the screen was composited from. Without `ext_multigrid` this is the
    /// single global grid; with it, one entry per window plus nvim's own.
    grids: Vec<GridView>,
    cells: Vec<CellView<'a>>,
}

#[derive(Serialize)]
struct Cursor {
    row: usize,
    col: usize,
}

/// Where one grid sits on the composited screen.
///
/// `row`/`col`/`width`/`height` are one coordinate space for every placement
/// kind: absolute screen cells, floored the way the compositor floors them. A
/// float's own anchor is reported separately in `anchor*`, so a consumer can
/// locate the float without re-deriving the anchor chain and never has to guess
/// which space a key is in. The rectangle is the one submitted for compositing,
/// not the part that survives it: it may hang off any edge, and the message
/// grid is allocated full-height, so `row + height` routinely exceeds the
/// screen. Clipping to `scene.cols`/`scene.rows` is the consumer's to do.
///
/// The coordinates are absent only when the grid has no place on this screen at
/// all: an `external` window, a `float` whose anchor chain does not resolve, or
/// an `unplaced` grid nvim has allocated but not positioned — either not yet, or
/// no longer, after `win_close` without a `grid_destroy`.
/// `hidden` is nvim's own per-window flag; a float is also off screen when
/// anything in its `anchor_grid` chain is hidden.
#[derive(Serialize)]
struct GridView {
    id: u64,
    cols: usize,
    rows: usize,
    placement: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    row: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    col: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    width: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    height: Option<usize>,
    /// Which corner of the float `win_float_pos` positioned, and the grid and
    /// (possibly fractional, possibly negative) cell it positioned it against.
    #[serde(skip_serializing_if = "Option::is_none")]
    anchor: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    anchor_grid: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    anchor_row: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    anchor_col: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    zindex: Option<i64>,
    hidden: bool,
}

fn grids(screen: &Screen) -> Vec<GridView> {
    screen
        .grid_ids()
        .into_iter()
        .filter_map(|id| {
            let (cols, rows) = screen.grid_size(id)?;
            let mut view = GridView {
                id,
                cols,
                rows,
                placement: if id == GLOBAL_GRID { "global" } else { "unplaced" },
                row: None,
                col: None,
                width: None,
                height: None,
                anchor: None,
                anchor_grid: None,
                anchor_row: None,
                anchor_col: None,
                zindex: None,
                hidden: false,
            };
            if id == GLOBAL_GRID {
                (view.row, view.col) = (Some(0), Some(0));
                (view.width, view.height) = (Some(cols), Some(rows));
            }
            if let Some(window) = screen.window(id) {
                view.hidden = window.hidden;
                (view.row, view.col) = match screen.screen_origin(id) {
                    Some((row, col)) => (Some(row), Some(col)),
                    None => (None, None),
                };
                match window.placement {
                    // A split shows the extent nvim gave it, which may be less
                    // than the grid it was handed.
                    Placement::Window { width, height, .. } => {
                        view.placement = "window";
                        (view.width, view.height) = (Some(width), Some(height));
                    }
                    // A float and the message grid are shown whole.
                    Placement::Float { anchor, anchor_grid, row, col, zindex } => {
                        view.placement = "float";
                        (view.width, view.height) = (Some(cols), Some(rows));
                        view.anchor = Some(anchor.name());
                        view.anchor_grid = Some(anchor_grid);
                        (view.anchor_row, view.anchor_col) = (Some(row), Some(col));
                        view.zindex = Some(zindex);
                    }
                    Placement::Message { zindex, .. } => {
                        view.placement = "message";
                        (view.width, view.height) = (Some(cols), Some(rows));
                        view.zindex = Some(zindex);
                    }
                    // Not on this screen at all, so `screen_origin` gave it no
                    // coordinates to report.
                    Placement::External => view.placement = "external",
                }
            }
            Some(view)
        })
        .collect()
}

#[derive(Serialize)]
struct CellView<'a> {
    id: String,
    row: usize,
    col: usize,
    text: String,
    highlight_id: u64,
    foreground: String,
    background: String,
    #[serde(skip_serializing_if = "str::is_empty")]
    note: &'a str,
}

pub fn write_evaluation(path: &Path, screen: &Screen) -> std::io::Result<()> {
    let json = serde_json::to_vec_pretty(&evaluate(screen))
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    std::fs::write(path, json)
}

fn evaluate(screen: &Screen) -> Evaluation<'_> {
    let mut cells = Vec::with_capacity(screen.cols() * screen.rows());
    for row in 0..screen.rows() {
        for col in 0..screen.cols() {
            let cell = screen.cell(row, col);
            let (foreground, background) = screen.colors(cell.hl);
            cells.push(CellView {
                id: format!("nvim-screen-cell-{row}-{col}"),
                row,
                col,
                text: cell.ch.to_string(),
                highlight_id: cell.hl,
                foreground: format!("#{foreground:06x}"),
                background: format!("#{background:06x}"),
                note: "",
            });
        }
    }

    Evaluation {
        schema: "nvimgl.root-ui-integration-evaluation/v2",
        trace_id: Ulid::new().to_string(),
        evaluation_candidate: true,
        canonical_integration: false,
        adoption_decision: "awaiting_human_gate",
        editor_basis: "neovim",
        ownership: Ownership {
            buffer_and_editing_semantics: "neovim",
            visual_primitive_owner: "unresolved",
            text_editing_host_port_owner: "unresolved",
        },
        root_ui_constraints_observed: RootUiConstraints {
            react_footprint: "zero",
            text_editing_host_port: "replaceable_framework_neutral",
            phase_order:
                "semantic_to_layout_to_non_color_decoration_to_user_color_to_shader_output",
        },
        scene: Scene {
            kind: "neovim_grid_semantic_projection",
            root_ui_primitive_program: false,
            cols: screen.cols(),
            rows: screen.rows(),
            // The projection describes the composited screen, so an off-screen
            // cursor is reported as absent rather than at a cell it is not on.
            cursor: screen.cursor().map(|(row, col)| Cursor { row, col }),
            grids: grids(screen),
            cells,
        },
    }
}

#[cfg(test)]
mod tests {
    use rmpv::Value;

    use super::*;
    use crate::nvim::RedrawEvent;

    fn resize(grid: u64, cols: usize, rows: usize) -> RedrawEvent {
        ("grid_resize".into(), vec![Value::from(grid), Value::from(cols), Value::from(rows)])
    }

    fn win_pos(grid: u64, row: i64, col: i64, width: usize, height: usize) -> RedrawEvent {
        (
            "win_pos".into(),
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
        ("win_float_pos".into(), args)
    }

    /// The projected grid entry for `id`.
    fn grid_view(value: &serde_json::Value, id: u64) -> serde_json::Value {
        value["scene"]["grids"]
            .as_array()
            .unwrap()
            .iter()
            .find(|g| g["id"] == id)
            .unwrap()
            .clone()
    }

    #[test]
    fn projection_is_evidence_without_root_ui_adoption() {
        let screen = Screen::new(2, 1);
        let value = serde_json::to_value(evaluate(&screen)).unwrap();
        assert_eq!(value["evaluation_candidate"], true);
        assert_eq!(value["canonical_integration"], false);
        assert_eq!(value["adoption_decision"], "awaiting_human_gate");
        assert_eq!(value["editor_basis"], "neovim");
        assert_eq!(value["scene"]["root_ui_primitive_program"], false);
        // v2 projects the composited screen: cells no longer belong to one grid.
        assert_eq!(value["schema"], "nvimgl.root-ui-integration-evaluation/v2");
        assert_eq!(value["scene"]["cells"][0]["id"], "nvim-screen-cell-0-0");
        // A session with no windows placed is still one grid: the global one.
        assert_eq!(value["scene"]["grids"][0]["id"], 1);
        assert_eq!(value["scene"]["grids"][0]["placement"], "global");
    }

    #[test]
    fn the_projection_records_where_each_grid_was_placed() {
        let mut screen = Screen::new(6, 2);
        screen.apply(&[
            resize(2, 3, 1),
            win_pos(2, 1, 3, 3, 1),
            resize(5, 4, 1),
            win_float_pos(5, "NW", GLOBAL_GRID, 1.0, 1.0, Some(70)),
        ]);

        let value = serde_json::to_value(evaluate(&screen)).unwrap();
        let grids = value["scene"]["grids"].as_array().unwrap().clone();
        assert_eq!(grids.len(), 3);

        // The global grid is the screen, so it is locatable like any other.
        assert_eq!(grids[0]["placement"], "global");
        assert_eq!((grids[0]["row"].clone(), grids[0]["col"].clone()), (0.into(), 0.into()));
        assert_eq!((grids[0]["width"].clone(), grids[0]["height"].clone()), (6.into(), 2.into()));

        assert_eq!(grids[1]["placement"], "window");
        assert_eq!((grids[1]["row"].clone(), grids[1]["col"].clone()), (1.into(), 3.into()));
        assert_eq!(grids[1]["width"], 3);

        assert_eq!(grids[2]["placement"], "float");
        assert_eq!(grids[2]["zindex"], 70);
        assert_eq!(grids[2]["hidden"], false);
        // A float is shown whole, and its anchor is recorded as nvim gave it.
        assert_eq!((grids[2]["width"].clone(), grids[2]["height"].clone()), (4.into(), 1.into()));
        assert_eq!(grids[2]["anchor"], "NW");
        assert_eq!(grids[2]["anchor_grid"], 1);
        assert_eq!(grids[2]["anchor_row"], 1.0);
        assert_eq!(grids[2]["anchor_col"], 1.0);
    }

    /// `row`/`col` mean the same thing for every placement: the screen cell the
    /// compositor puts the grid's top-left in. A float's anchor is relative and
    /// may be fractional or negative, so it cannot be reported in those keys.
    #[test]
    fn a_float_is_projected_at_the_screen_cell_it_composites_at() {
        let mut screen = Screen::new(8, 4);
        screen.apply(&[
            resize(3, 2, 1),
            win_float_pos(3, "NW", GLOBAL_GRID, 1.0, 2.0, None),
            // Anchored to the float above, not to the screen.
            resize(4, 2, 1),
            win_float_pos(4, "NW", 3, 1.0, 1.0, Some(60)),
            // Off the left edge, at a fractional column.
            resize(5, 4, 1),
            win_float_pos(5, "NW", GLOBAL_GRID, 0.0, -2.5, None),
        ]);

        let value = serde_json::to_value(evaluate(&screen)).unwrap();

        let nested = grid_view(&value, 4);
        assert_eq!((nested["row"].clone(), nested["col"].clone()), (2.into(), 3.into()));
        assert_eq!(nested["anchor_grid"], 3);
        assert_eq!(nested["anchor_row"], 1.0);
        assert_eq!(nested["anchor_col"], 1.0);
        // Which is where it actually lands on the composited screen.
        assert_eq!(screen.screen_origin(4), Some((2, 3)));

        // Floored, not truncated: -2.5 composites in column -3.
        let offscreen = grid_view(&value, 5);
        assert_eq!((offscreen["row"].clone(), offscreen["col"].clone()), (0.into(), (-3).into()));
        assert_eq!(offscreen["anchor_col"], -2.5);
    }

    #[test]
    fn a_grid_with_no_place_on_the_screen_is_projected_without_coordinates() {
        let mut screen = Screen::new(4, 2);
        screen.apply(&[
            resize(2, 2, 1),
            ("win_external_pos".into(), vec![Value::from(2u64), Value::Ext(1, vec![2])]),
            // A float anchored to itself: the chain never reaches the screen.
            resize(3, 2, 1),
            win_float_pos(3, "NW", 3, 0.0, 0.0, None),
        ]);

        let value = serde_json::to_value(evaluate(&screen)).unwrap();

        let external = grid_view(&value, 2);
        assert_eq!(external["placement"], "external");
        assert!(external.get("row").is_none() && external.get("col").is_none());

        let unanchored = grid_view(&value, 3);
        assert_eq!(unanchored["placement"], "float");
        assert!(unanchored.get("row").is_none() && unanchored.get("col").is_none());
        // The anchor it was given is still reported, so the dangling one is visible.
        assert_eq!(unanchored["anchor_grid"], 3);
    }

    /// `win_close` drops the placement but leaves the grid allocated until
    /// `grid_destroy`. It is still a grid on the projection, just one with
    /// nowhere to be — distinct from `hidden`, which keeps its position.
    #[test]
    fn a_closed_but_undestroyed_grid_is_projected_as_unplaced() {
        let mut screen = Screen::new(6, 2);
        screen.apply(&[resize(2, 3, 1), win_pos(2, 1, 3, 3, 1)]);
        let placed = grid_view(&serde_json::to_value(evaluate(&screen)).unwrap(), 2);
        assert_eq!(placed["placement"], "window");

        screen.apply(&[("win_close".into(), vec![Value::from(2u64)])]);
        let view = grid_view(&serde_json::to_value(evaluate(&screen)).unwrap(), 2);
        assert_eq!(view["placement"], "unplaced");
        assert_eq!(view["hidden"], false);
        assert!(view.get("row").is_none() && view.get("col").is_none());
        // The grid itself is still there, at the size nvim gave it.
        assert_eq!((view["cols"].clone(), view["rows"].clone()), (3.into(), 1.into()));
    }

    #[test]
    fn a_hidden_window_still_reports_where_it_sits() {
        let mut screen = Screen::new(6, 2);
        screen.apply(&[
            resize(2, 3, 1),
            win_pos(2, 1, 3, 3, 1),
            ("win_hide".into(), vec![Value::from(2u64)]),
        ]);

        let view = grid_view(&serde_json::to_value(evaluate(&screen)).unwrap(), 2);
        assert_eq!(view["hidden"], true);
        assert_eq!((view["row"].clone(), view["col"].clone()), (1.into(), 3.into()));
    }
}
