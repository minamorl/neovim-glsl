//! Machine-readable Root-ui integration hypothesis.
//!
//! This projection is evidence, not an adoption decision. It keeps Neovim as
//! editor authority and records the Root-ui ownership questions as unresolved.

use std::path::Path;

use serde::Serialize;
use ulid::Ulid;

use crate::grid::Grid;

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
    cursor: Cursor,
    cells: Vec<CellView<'a>>,
}

#[derive(Serialize)]
struct Cursor {
    row: usize,
    col: usize,
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

pub fn write_evaluation(path: &Path, grid: &Grid) -> std::io::Result<()> {
    let json = serde_json::to_vec_pretty(&evaluate(grid))
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    std::fs::write(path, json)
}

fn evaluate(grid: &Grid) -> Evaluation<'_> {
    let mut cells = Vec::with_capacity(grid.cols * grid.rows);
    for row in 0..grid.rows {
        for col in 0..grid.cols {
            let cell = grid.cell(row, col);
            let (foreground, background) = grid.colors(cell.hl);
            cells.push(CellView {
                id: format!("nvim-grid-1-cell-{row}-{col}"),
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
        schema: "nvimgl.root-ui-integration-evaluation/v1",
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
            cols: grid.cols,
            rows: grid.rows,
            cursor: Cursor {
                row: grid.cursor.0,
                col: grid.cursor.1,
            },
            cells,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_is_evidence_without_root_ui_adoption() {
        let grid = Grid::new(2, 1);
        let value = serde_json::to_value(evaluate(&grid)).unwrap();
        assert_eq!(value["evaluation_candidate"], true);
        assert_eq!(value["canonical_integration"], false);
        assert_eq!(value["adoption_decision"], "awaiting_human_gate");
        assert_eq!(value["editor_basis"], "neovim");
        assert_eq!(value["scene"]["root_ui_primitive_program"], false);
    }
}
