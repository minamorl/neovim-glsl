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
                zindex: None,
                hidden: false,
            };
            if let Some(window) = screen.window(id) {
                view.hidden = window.hidden;
                match window.placement {
                    Placement::Window { row, col, width, height } => {
                        view.placement = "window";
                        (view.row, view.col) = (Some(row), Some(col));
                        (view.width, view.height) = (Some(width), Some(height));
                    }
                    Placement::Float { row, col, zindex, .. } => {
                        view.placement = "float";
                        (view.row, view.col) = (Some(row as i64), Some(col as i64));
                        view.zindex = Some(zindex);
                    }
                    Placement::Message { row, zindex } => {
                        view.placement = "message";
                        view.row = Some(row);
                        view.zindex = Some(zindex);
                    }
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
    use super::*;

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
            ("grid_resize".into(), vec![2u64.into(), 3u64.into(), 1u64.into()]),
            (
                "win_pos".into(),
                vec![2u64.into(), 2u64.into(), 1u64.into(), 3u64.into(), 3u64.into(), 1u64.into()],
            ),
            ("grid_resize".into(), vec![5u64.into(), 4u64.into(), 1u64.into()]),
            (
                "win_float_pos".into(),
                vec![
                    5u64.into(),
                    5u64.into(),
                    "NW".into(),
                    1u64.into(),
                    1u64.into(),
                    1u64.into(),
                    true.into(),
                    70i64.into(),
                ],
            ),
        ]);

        let value = serde_json::to_value(evaluate(&screen)).unwrap();
        let grids = value["scene"]["grids"].as_array().unwrap().clone();
        assert_eq!(grids.len(), 3);
        assert_eq!(grids[1]["placement"], "window");
        assert_eq!((grids[1]["row"].clone(), grids[1]["col"].clone()), (1.into(), 3.into()));
        assert_eq!(grids[1]["width"], 3);
        assert_eq!(grids[2]["placement"], "float");
        assert_eq!(grids[2]["zindex"], 70);
        assert_eq!(grids[2]["hidden"], false);
    }
}
