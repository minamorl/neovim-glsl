//! The navigation surface, composed as a root-ui flat scene.
//!
//! Every rectangle on this surface is a root-ui `Box` or `RoundBox` with a
//! semantic name, a normalized layout, decoration that never touches layout,
//! and a colour requested by role. Nothing here picks a colour: the scheme is a
//! runtime input, so the same resolved layout serves a dark and a light user.

use std::collections::BTreeMap;

use super::language::{
    rgba, Bounds, BoxKind, ColorIntent, ColorRuntime, ColorScheme, Decoration, Sample, Semantic,
};
use super::FlatScene;

/// A run of text placed in the surface's own pixels, drawn by the adapter after
/// the shapes it sits on.
pub struct TextRun {
    pub x: f32,
    pub baseline: f32,
    pub text: String,
    pub color: [f32; 4],
    pub max_x: f32,
}

pub struct NavigationSurface {
    pub scene: FlatScene,
    pub texts: Vec<TextRun>,
    /// The panel rectangle in pixels, for callers that need to know where the
    /// surface landed without re-deriving it.
    pub frame: (f32, f32, f32, f32),
}

/// The roles this surface asks for. Named rather than coloured, because
/// `ColorRuntime` is the user's.
pub const ROLES: [&str; 6] =
    ["surface", "surface_raised", "outline", "on_surface", "on_surface_muted", "accent"];

pub fn dark_scheme() -> ColorScheme {
    let mut colors = BTreeMap::new();
    colors.insert("surface".into(), rgba("#12141c", 0.94));
    colors.insert("surface_raised".into(), rgba("#1b2030", 0.98));
    colors.insert("outline".into(), rgba("#3b6ea5", 0.9));
    colors.insert("on_surface".into(), rgba("#d8dee9", 1.0));
    colors.insert("on_surface_muted".into(), rgba("#7f8798", 1.0));
    colors.insert("accent".into(), rgba("#7aa2f7", 1.0));
    ColorScheme { id: "dark".into(), colors }
}

pub fn light_scheme() -> ColorScheme {
    let mut colors = BTreeMap::new();
    colors.insert("surface".into(), rgba("#f7f8fa", 0.96));
    colors.insert("surface_raised".into(), rgba("#ffffff", 1.0));
    colors.insert("outline".into(), rgba("#3b6ea5", 0.6));
    colors.insert("on_surface".into(), rgba("#1b2030", 1.0));
    colors.insert("on_surface_muted".into(), rgba("#6b7280", 1.0));
    colors.insert("accent".into(), rgba("#2f5fd0", 1.0));
    ColorScheme { id: "light".into(), colors }
}

pub fn color_runtime(scheme_id: &str) -> ColorRuntime {
    ColorRuntime {
        scheme_id: scheme_id.to_string(),
        schemes: vec![dark_scheme(), light_scheme()],
    }
}

fn surface(
    name: &str,
    state: &str,
    kind: BoxKind,
    bounds: Bounds,
    fill_role: &str,
    stroke_role: &str,
    corner_radius: f32,
    stroke_width: f32,
) -> Sample {
    Sample {
        semantic: Semantic::new(name, "navigation", state),
        kind,
        bounds,
        decoration: Decoration { stroke_width },
        color: ColorIntent::new(fill_role, stroke_role),
        corner_radius,
    }
}

pub struct Input<'a> {
    pub label: &'a str,
    pub query: &'a str,
    pub matched: usize,
    pub total: usize,
    /// Visible rows, each with whether it is the selection.
    pub rows: &'a [(String, bool)],
    /// How many rows the surface has room for. The pitch comes from the budget,
    /// not from how many candidates survived: deriving it from `rows.len()`
    /// makes a single match a row as tall as the whole list.
    pub row_budget: usize,
    pub window_w: f32,
    pub window_h: f32,
    pub cell_w: f32,
    pub cell_h: f32,
    pub ascent: f32,
}

/// Compose the surface.
///
/// The panel is placed by fraction of the window, not by cell, and the row
/// pitch is derived from the panel rather than from `cell_h`. Both are what
/// `pin navigation_not_in_grid` buys: a surface addressed in the grid's
/// coordinates could do neither.
pub fn build(input: Input<'_>) -> NavigationSurface {
    let window_w = input.window_w.max(1.0);
    let window_h = input.window_h.max(1.0);

    let panel_w = (window_w * 0.62).clamp(280.0, window_w);
    let panel_h = (window_h * 0.56).clamp(180.0, window_h);
    let panel_x = (window_w - panel_w) / 2.0;
    let panel_y = (window_h - panel_h) / 3.0;

    let normalized = |x: f32, y: f32, w: f32, h: f32| Bounds {
        x: (x / window_w).clamp(0.0, 1.0),
        y: (y / window_h).clamp(0.0, 1.0),
        width: (w / window_w).clamp(0.0, 1.0 - (x / window_w).clamp(0.0, 1.0)),
        height: (h / window_h).clamp(0.0, 1.0 - (y / window_h).clamp(0.0, 1.0)),
    };

    let mut surfaces = vec![(
        "navigation.panel".to_string(),
        surface(
            "Dialog",
            "open",
            BoxKind::RoundBox,
            normalized(panel_x, panel_y, panel_w, panel_h),
            "surface",
            "outline",
            // A tenth of the shorter side, materialized in physical pixels.
            0.05,
            0.004,
        ),
    )];

    let padding = 16.0;
    let query_h = (input.cell_h * 1.9).max(28.0);
    surfaces.push((
        "navigation.query".to_string(),
        surface(
            "Input",
            "focused",
            BoxKind::RoundBox,
            normalized(
                panel_x + padding,
                panel_y + padding * 0.6,
                panel_w - padding * 2.0,
                query_h,
            ),
            "surface_raised",
            "accent",
            0.25,
            0.02,
        ),
    ));

    let list_top = panel_y + padding * 0.6 + query_h + padding * 0.5;
    let list_h = (panel_y + panel_h - padding) - list_top;
    let row_h = list_h / input.row_budget.max(input.rows.len()).max(1) as f32;

    let mut texts = Vec::new();
    // The counter is measured, not guessed at: a fixed inset wide enough for
    // "3/4" clips "1204/20000", and a clipped count is a wrong count.
    let counter = format!("{}/{}", input.matched, input.total);
    let counter_w = (counter.chars().count() as f32 + 1.0) * input.cell_w;
    let counter_x = panel_x + panel_w - padding - counter_w;

    for (index, (text, selected)) in input.rows.iter().enumerate() {
        let y = list_top + row_h * index as f32;
        if *selected {
            surfaces.push((
                format!("navigation.row.{index}"),
                surface(
                    "ListItem",
                    "selected",
                    BoxKind::RoundBox,
                    normalized(panel_x + padding * 0.5, y, panel_w - padding, row_h),
                    "surface_raised",
                    "accent",
                    0.3,
                    0.0,
                ),
            ));
        }
        texts.push(TextRun {
            x: panel_x + padding * 1.4,
            baseline: y + (row_h + input.ascent) / 2.0 - 1.0,
            text: text.clone(),
            // Colour is bound by the caller from the same runtime the shapes
            // use; the role is what this module names.
            color: [0.0, 0.0, 0.0, 0.0],
            max_x: panel_x + panel_w - padding,
        });
    }

    let query_baseline = panel_y + padding * 0.6 + (query_h + input.ascent) / 2.0 - 1.0;
    texts.push(TextRun {
        x: panel_x + padding * 1.6,
        baseline: query_baseline,
        text: format!("{}  ›  {}", input.label, input.query),
        color: [0.0, 0.0, 0.0, 0.0],
        max_x: counter_x - input.cell_w,
    });
    texts.push(TextRun {
        x: counter_x,
        baseline: query_baseline,
        text: counter,
        color: [0.0, 0.0, 0.0, 0.0],
        max_x: panel_x + panel_w - padding * 0.5,
    });

    NavigationSurface {
        scene: FlatScene { surfaces },
        texts,
        frame: (panel_x, panel_y, panel_w, panel_h),
    }
}

/// Which role each text run wants. Index-aligned with `texts` from [`build`]:
/// rows first, then the query line, then the counter.
pub fn text_roles(rows: usize) -> Vec<&'static str> {
    let mut roles = vec!["on_surface"; rows];
    roles.push("on_surface");
    roles.push("on_surface_muted");
    roles
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::root_ui::{
        bind_flat_scene_user_color_scheme, flat_scene_layout_identity, prepare_flat_scene,
    };

    fn rows() -> Vec<(String, bool)> {
        vec![
            ("notes/alpha.md".to_string(), true),
            ("notes/beta.md".to_string(), false),
            ("src/main.rs".to_string(), false),
        ]
    }

    fn surface() -> NavigationSurface {
        let rows = rows();
        build(Input {
            label: "neovim-glsl",
            query: "al",
            matched: 1,
            total: 3,
            rows: &rows,
            row_budget: 12,
            window_w: 900.0,
            window_h: 560.0,
            cell_w: 9.0,
            cell_h: 18.0,
            ascent: 13.0,
        })
    }

    #[test]
    fn the_scene_resolves_and_every_requested_role_exists_in_both_schemes() {
        let surface = surface();
        let prepared = prepare_flat_scene(&surface.scene).expect("prepared");
        for scheme in ["dark", "light"] {
            bind_flat_scene_user_color_scheme(&prepared, &color_runtime(scheme))
                .unwrap_or_else(|error| panic!("{scheme} scheme: {error}"));
        }
    }

    #[test]
    fn switching_scheme_does_not_move_anything() {
        let surface = surface();
        let prepared = prepare_flat_scene(&surface.scene).unwrap();
        let dark = bind_flat_scene_user_color_scheme(&prepared, &color_runtime("dark")).unwrap();
        let light = bind_flat_scene_user_color_scheme(&prepared, &color_runtime("light")).unwrap();
        assert_eq!(flat_scene_layout_identity(&dark), flat_scene_layout_identity(&light));
    }

    #[test]
    fn only_the_selected_row_gets_a_surface_of_its_own() {
        let surface = surface();
        let selected: Vec<&String> = surface
            .scene
            .surfaces
            .iter()
            .map(|(id, _)| id)
            .filter(|id| id.starts_with("navigation.row."))
            .collect();
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0], "navigation.row.0");
    }

    #[test]
    fn one_surviving_candidate_keeps_a_normal_row_height() {
        let one = [("only.md".to_string(), true)];
        let surface = build(Input {
            label: "x",
            query: "only",
            matched: 1,
            total: 40,
            rows: &one,
            row_budget: 12,
            window_w: 900.0,
            window_h: 560.0,
            cell_w: 9.0,
            cell_h: 18.0,
            ascent: 13.0,
        });
        let row = surface
            .scene
            .surfaces
            .iter()
            .find(|(id, _)| id.starts_with("navigation.row."))
            .expect("the selection has a surface");
        let height = row.1.bounds.height * 560.0;
        assert!(height < 40.0, "a single match drew a row {height}px tall");
    }

    #[test]
    fn the_row_pitch_is_not_the_cell_height() {
        let rows = rows();
        let surface = build(Input {
            label: "x",
            query: "",
            matched: 3,
            total: 3,
            rows: &rows,
            row_budget: 12,
            window_w: 900.0,
            window_h: 560.0,
            cell_w: 9.0,
            cell_h: 18.0,
            ascent: 13.0,
        });
        let first = surface.texts[0].baseline;
        let second = surface.texts[1].baseline;
        assert!((second - first - 18.0).abs() > 1.0, "the surface is following the cell raster");
    }

    #[test]
    fn every_text_run_has_a_role() {
        let surface = surface();
        assert_eq!(text_roles(rows().len()).len(), surface.texts.len());
    }

    #[test]
    fn the_panel_is_centred_horizontally_and_high_of_centre() {
        let surface = surface();
        let (x, y, w, h) = surface.frame;
        assert!((x + w / 2.0 - 450.0).abs() < 1.0);
        assert!(y + h / 2.0 < 280.0);
    }
}
