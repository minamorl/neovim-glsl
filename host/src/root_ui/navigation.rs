//! The navigation surface, composed as a root-ui flat scene.
//!
//! Every rectangle here is a root-ui `Box` or `RoundBox` with a semantic name, a
//! normalized layout, decoration that never touches layout, and a colour
//! requested by role. Nothing in this file picks a colour: the scheme is a
//! runtime input, so one resolved layout serves a dark and a light user.
//!
//! Corners are stated in density-independent pixels, which root-ui gained for
//! this surface. Under the old ratio-only form every element's corner scaled
//! with its own height, so a query row 40px tall and a panel 480px tall could
//! not share a corner — the catalogue's answer was `0.5` everywhere, and the
//! result was a picker made of nested capsules.

use std::collections::BTreeMap;

use super::language::{
    rgba, Bounds, BoxKind, ColorIntent, ColorRuntime, ColorScheme, CornerRadius, Decoration,
    Sample, Semantic,
};
use super::FlatScene;

/// Corner radii in density-independent pixels. One scale for the whole surface:
/// a panel and a row inside it differ in size, not in how sharp their corners
/// are.
const PANEL_RADIUS: f32 = 10.0;
const ROW_RADIUS: f32 = 5.0;

/// A run of text placed in the surface's own pixels, drawn after the shapes it
/// sits on. The colour is a role, resolved by the caller from the same runtime
/// the shapes use.
pub struct TextRun {
    pub x: f32,
    pub baseline: f32,
    pub text: String,
    pub role: &'static str,
    pub max_x: f32,
}

pub struct NavigationSurface {
    pub scene: FlatScene,
    pub texts: Vec<TextRun>,
    /// The panel rectangle in pixels, for callers that need to know where the
    /// surface landed without re-deriving it.
    pub frame: (f32, f32, f32, f32),
}

/// One candidate as the surface receives it.
pub struct RowInput {
    pub text: String,
    /// Character offsets the query matched, so they can be picked out. A picker
    /// that cannot show *why* a row matched is a list, not a picker.
    pub positions: Vec<u32>,
    pub selected: bool,
}

pub const ROLES: [&str; 7] = [
    "surface",
    "surface_raised",
    "outline",
    "separator",
    "on_surface",
    "on_surface_muted",
    "accent",
];

/// The panel is opaque in both schemes. A translucent surface is something the
/// free-surface work measured and this renderer can do, but a picker's job is to
/// be read: even 3% of a bright grid behind it shows through as ghost text
/// across the candidate list.
pub fn dark_scheme() -> ColorScheme {
    let mut colors = BTreeMap::new();
    colors.insert("surface".into(), rgba("#14161f", 1.0));
    colors.insert("surface_raised".into(), rgba("#232838", 1.0));
    colors.insert("outline".into(), rgba("#2c3242", 1.0));
    colors.insert("separator".into(), rgba("#252b3a", 1.0));
    colors.insert("on_surface".into(), rgba("#dbe1ec", 1.0));
    colors.insert("on_surface_muted".into(), rgba("#6e7688", 1.0));
    colors.insert("accent".into(), rgba("#7aa2f7", 1.0));
    ColorScheme { id: "dark".into(), colors }
}

pub fn light_scheme() -> ColorScheme {
    let mut colors = BTreeMap::new();
    colors.insert("surface".into(), rgba("#fbfcfe", 1.0));
    colors.insert("surface_raised".into(), rgba("#e7ecf6", 1.0));
    colors.insert("outline".into(), rgba("#c8cfdd", 1.0));
    colors.insert("separator".into(), rgba("#e2e7f0", 1.0));
    colors.insert("on_surface".into(), rgba("#1b2030", 1.0));
    colors.insert("on_surface_muted".into(), rgba("#767e91", 1.0));
    colors.insert("accent".into(), rgba("#2f5fd0", 1.0));
    ColorScheme { id: "light".into(), colors }
}

pub fn color_runtime(scheme_id: &str) -> ColorRuntime {
    ColorRuntime { scheme_id: scheme_id.to_string(), schemes: vec![dark_scheme(), light_scheme()] }
}

pub struct Input<'a> {
    pub label: &'a str,
    pub query: &'a str,
    pub matched: usize,
    pub total: usize,
    pub rows: &'a [RowInput],
    /// How many rows the surface has room for. The pitch comes from the budget,
    /// not from how many candidates survived.
    pub row_budget: usize,
    /// Where the visible window starts in the match list, for the scroll bar.
    pub offset: usize,
    pub window_w: f32,
    pub window_h: f32,
    pub cell_w: f32,
    pub cell_h: f32,
    pub ascent: f32,
    /// Physical pixels per density-independent pixel.
    pub scale: f32,
}

struct Composer {
    surfaces: Vec<(String, Sample)>,
    texts: Vec<TextRun>,
    window_w: f32,
    window_h: f32,
    scale: f32,
}

impl Composer {
    fn normalized(&self, x: f32, y: f32, w: f32, h: f32) -> Bounds {
        let nx = (x / self.window_w).clamp(0.0, 1.0);
        let ny = (y / self.window_h).clamp(0.0, 1.0);
        Bounds {
            x: nx,
            y: ny,
            width: (w / self.window_w).clamp(0.0, 1.0 - nx),
            height: (h / self.window_h).clamp(0.0, 1.0 - ny),
        }
    }

    /// A hairline is one physical pixel wide however big the box is, so the
    /// ratio is computed from the box rather than written as a constant.
    fn hairline(&self, w: f32, h: f32) -> f32 {
        let shorter = w.min(h).max(1.0);
        (self.scale / shorter).min(0.5)
    }

    #[allow(clippy::too_many_arguments)]
    fn shape(
        &mut self,
        id: &str,
        name: &str,
        state: &str,
        rect: (f32, f32, f32, f32),
        radius: Option<f32>,
        fill_role: &str,
        stroke_role: &str,
        stroke_ratio: f32,
    ) {
        let (x, y, w, h) = rect;
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        self.surfaces.push((
            id.to_string(),
            Sample {
                semantic: Semantic::new(name, "navigation", state),
                kind: if radius.is_some() { BoxKind::RoundBox } else { BoxKind::Box },
                bounds: self.normalized(x, y, w, h),
                decoration: Decoration { stroke_width: stroke_ratio },
                color: ColorIntent::new(fill_role, stroke_role),
                corner_radius: CornerRadius::Pixels(radius.unwrap_or(0.0)),
            },
        ));
    }

    fn text(&mut self, x: f32, baseline: f32, text: String, role: &'static str, max_x: f32) {
        if text.is_empty() {
            return;
        }
        self.texts.push(TextRun { x, baseline, text, role, max_x });
    }
}

/// Width of a run of text in this monospaced cell grid.
fn advance(text: &str, cell_w: f32) -> f32 {
    text.chars().map(|c| crate::proto::paint::char_width(c) as f32 * cell_w).sum()
}

/// Compose the surface.
///
/// The panel is placed by fraction of the window and the row pitch comes from
/// the text metrics, not from the cell raster. Both are what
/// `pin navigation_not_in_grid` buys.
pub fn build(input: Input<'_>) -> NavigationSurface {
    let window_w = input.window_w.max(1.0);
    let window_h = input.window_h.max(1.0);
    let scale = input.scale.max(0.5);
    let dp = |value: f32| value * scale;

    let mut composer =
        Composer { surfaces: Vec::new(), texts: Vec::new(), window_w, window_h, scale };

    let panel_w = (window_w * 0.66).clamp(320.0, (window_w - dp(32.0)).max(320.0));
    let panel_h = (window_h * 0.62).clamp(200.0, (window_h - dp(32.0)).max(200.0));
    let panel_x = ((window_w - panel_w) / 2.0).floor();
    let panel_y = ((window_h - panel_h) / 3.0).floor();

    let pad_x = dp(16.0);
    let pad_y = dp(12.0);
    let query_h = input.cell_h * 1.9;
    let row_h = input.cell_h * 1.45;

    composer.shape(
        "navigation.panel",
        "Dialog",
        "open",
        (panel_x, panel_y, panel_w, panel_h),
        Some(PANEL_RADIUS),
        "surface",
        "outline",
        composer.hairline(panel_w, panel_h),
    );

    // The query line is text on the panel with a rule under it, not a box
    // inside a box. A field drawn as its own filled shape reads as a second
    // surface, and stacking those is what made this look like nested capsules.
    let baseline = panel_y + (query_h + input.ascent) / 2.0;
    let prompt_x = panel_x + pad_x;
    composer.text(prompt_x, baseline, "›".into(), "accent", prompt_x + input.cell_w * 2.0);

    let counter = format!("{}/{}", input.matched, input.total);
    let counter_w = advance(&counter, input.cell_w);
    let counter_x = panel_x + panel_w - pad_x - counter_w;
    composer.text(counter_x, baseline, counter, "on_surface_muted", panel_x + panel_w - pad_x);

    let label_x = prompt_x + input.cell_w * 2.0;
    let label = format!("{}  ", input.label);
    let label_w = advance(&label, input.cell_w);
    composer.text(label_x, baseline, label, "on_surface_muted", counter_x - input.cell_w);

    let query_x = label_x + label_w;
    let query = if input.query.is_empty() { "…".to_string() } else { input.query.to_string() };
    let query_role = if input.query.is_empty() { "on_surface_muted" } else { "on_surface" };
    composer.text(query_x, baseline, query, query_role, counter_x - input.cell_w);

    let rule_y = (panel_y + query_h).floor();
    composer.shape(
        "navigation.rule",
        "Separator",
        "rest",
        (panel_x + pad_x, rule_y, panel_w - pad_x * 2.0, scale.max(1.0)),
        None,
        "separator",
        "separator",
        0.0,
    );

    let list_top = rule_y + pad_y;
    let list_bottom = panel_y + panel_h - pad_y;
    let budget = input.row_budget.max(input.rows.len()).max(1);

    for (index, row) in input.rows.iter().enumerate() {
        let y = list_top + row_h * index as f32;
        if y + row_h > list_bottom + 0.5 {
            break;
        }
        if row.selected {
            composer.shape(
                &format!("navigation.row.{index}"),
                "ListItem",
                "selected",
                (panel_x + dp(6.0), y, panel_w - dp(12.0), row_h),
                Some(ROW_RADIUS),
                "surface_raised",
                "surface_raised",
                0.0,
            );
        }
        let text_x = panel_x + pad_x;
        let text_baseline = y + (row_h + input.ascent) / 2.0 - dp(1.0);
        let max_x = panel_x + panel_w - pad_x - dp(8.0);

        // Matched characters are drawn as their own runs over the same
        // baseline, so the reason a row survived the query is visible.
        for (text, role, offset) in split_on_matches(&row.text, &row.positions) {
            let prefix: String = row.text.chars().take(offset).collect();
            let x = text_x + advance(&prefix, input.cell_w);
            if x >= max_x {
                break;
            }
            composer.text(x, text_baseline, text, role, max_x);
        }
    }

    // A scroll bar only when the list does not fit, sized to what is off screen
    // rather than to a fixed thumb.
    if input.matched > budget {
        let track_h = list_bottom - list_top;
        let thumb_h = (track_h * budget as f32 / input.matched as f32).max(dp(18.0));
        let travel = (track_h - thumb_h).max(0.0);
        let progress = input.offset as f32 / (input.matched - budget).max(1) as f32;
        composer.shape(
            "navigation.scroll",
            "ScrollBar",
            "rest",
            (
                panel_x + panel_w - dp(7.0),
                list_top + travel * progress.clamp(0.0, 1.0),
                dp(3.0),
                thumb_h,
            ),
            Some(1.5),
            "on_surface_muted",
            "on_surface_muted",
            0.0,
        );
    }

    NavigationSurface {
        scene: FlatScene { surfaces: composer.surfaces },
        texts: composer.texts,
        frame: (panel_x, panel_y, panel_w, panel_h),
    }
}

/// Split a candidate into runs of matched and unmatched characters, each with
/// the character offset it starts at.
fn split_on_matches(text: &str, positions: &[u32]) -> Vec<(String, &'static str, usize)> {
    let chars: Vec<char> = text.chars().collect();
    let mut hit = vec![false; chars.len()];
    for &position in positions {
        if let Some(slot) = hit.get_mut(position as usize) {
            *slot = true;
        }
    }
    let mut runs: Vec<(String, &'static str, usize)> = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        let matched = hit[index];
        let start = index;
        while index < chars.len() && hit[index] == matched {
            index += 1;
        }
        runs.push((
            chars[start..index].iter().collect(),
            if matched { "accent" } else { "on_surface" },
            start,
        ));
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::root_ui::language::{materialize_pixel_box_geometry, prepare_design_language};
    use crate::root_ui::{
        bind_flat_scene_user_color_scheme, flat_scene_layout_identity, prepare_flat_scene,
    };

    fn rows() -> Vec<RowInput> {
        vec![
            RowInput { text: "notes/alpha.md".into(), positions: vec![6, 7], selected: true },
            RowInput { text: "notes/beta.md".into(), positions: vec![], selected: false },
            RowInput { text: "src/main.rs".into(), positions: vec![], selected: false },
        ]
    }

    fn input(rows: &[RowInput]) -> Input<'_> {
        Input {
            label: "obsidian",
            query: "al",
            matched: 3,
            total: 40,
            rows,
            row_budget: 12,
            offset: 0,
            window_w: 1200.0,
            window_h: 800.0,
            cell_w: 9.0,
            cell_h: 20.0,
            ascent: 14.0,
            scale: 1.0,
        }
    }

    fn surface() -> NavigationSurface {
        let rows = rows();
        build(input(&rows))
    }

    fn geometry_of(
        surface: &NavigationSurface,
        id: &str,
    ) -> crate::root_ui::language::PixelBoxGeometry {
        let sample = surface.scene.surfaces.iter().find(|(name, _)| name == id).expect(id);
        let prepared = prepare_design_language(&sample.1).unwrap();
        materialize_pixel_box_geometry(&prepared.layout, &prepared.decoration, 1200.0, 800.0, 1.0)
            .unwrap()
    }

    #[test]
    fn the_panel_and_a_row_share_one_corner_scale() {
        // The defect this replaced: with a ratio of the shorter side, a 29px row
        // and a 496px panel could not both look like one design system, and the
        // only value that looked consistent was the capsule.
        let surface = surface();
        assert_eq!(geometry_of(&surface, "navigation.panel").corner_radius_x, PANEL_RADIUS);
        assert_eq!(geometry_of(&surface, "navigation.row.0").corner_radius_x, ROW_RADIUS);
    }

    #[test]
    fn nothing_on_the_surface_is_a_capsule() {
        let surface = surface();
        for (id, _) in &surface.scene.surfaces {
            let geometry = geometry_of(&surface, id);
            let shorter = geometry.width.min(geometry.height);
            assert!(
                geometry.corner_radius_x < shorter / 2.0 || shorter <= 4.0,
                "{id} radius {} against shorter side {shorter}",
                geometry.corner_radius_x
            );
        }
    }

    #[test]
    fn the_scene_resolves_and_every_role_exists_in_both_schemes() {
        let surface = surface();
        let prepared = prepare_flat_scene(&surface.scene).expect("prepared");
        for scheme in ["dark", "light"] {
            bind_flat_scene_user_color_scheme(&prepared, &color_runtime(scheme))
                .unwrap_or_else(|error| panic!("{scheme} scheme: {error}"));
        }
        let runtime = color_runtime("dark");
        for run in &surface.texts {
            assert!(
                runtime.schemes[0].colors.contains_key(run.role),
                "no colour for role {}",
                run.role
            );
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
        assert_eq!(selected, vec!["navigation.row.0"]);
    }

    #[test]
    fn one_surviving_candidate_keeps_a_normal_row_height() {
        let one = vec![RowInput { text: "only.md".into(), positions: vec![], selected: true }];
        let mut spec = input(&one);
        spec.matched = 1;
        let surface = build(spec);
        let height = geometry_of(&surface, "navigation.row.0").height;
        assert!((height - 20.0 * 1.45).abs() < 1.0, "a single match drew a row {height}px tall");
    }

    #[test]
    fn matched_characters_become_their_own_runs() {
        let runs = split_on_matches("notes/alpha.md", &[6, 7]);
        assert_eq!(runs[0], ("notes/".to_string(), "on_surface", 0));
        assert_eq!(runs[1], ("al".to_string(), "accent", 6));
        assert_eq!(runs[2], ("pha.md".to_string(), "on_surface", 8));
    }

    #[test]
    fn a_row_with_no_match_positions_is_one_run() {
        assert_eq!(split_on_matches("plain.md", &[]).len(), 1);
        // Out-of-range positions cannot panic or shift the text.
        assert_eq!(split_on_matches("ab", &[99]).len(), 1);
    }

    #[test]
    fn the_scroll_bar_appears_only_when_the_list_does_not_fit() {
        let rows = rows();
        let mut spec = input(&rows);
        spec.matched = 3;
        assert!(!build(spec).scene.surfaces.iter().any(|(id, _)| id == "navigation.scroll"));

        let mut spec = input(&rows);
        spec.matched = 400;
        assert!(build(spec).scene.surfaces.iter().any(|(id, _)| id == "navigation.scroll"));
    }

    #[test]
    fn rows_do_not_spill_past_the_bottom_of_the_panel() {
        let many: Vec<RowInput> = (0..60)
            .map(|i| RowInput { text: format!("row{i}.md"), positions: vec![], selected: i == 0 })
            .collect();
        let mut spec = input(&many);
        spec.row_budget = 60;
        spec.matched = 60;
        let surface = build(spec);
        let (_, panel_y, _, panel_h) = surface.frame;
        for run in &surface.texts {
            assert!(run.baseline <= panel_y + panel_h, "a row was drawn past the panel");
        }
    }

    #[test]
    fn the_outline_stays_one_physical_pixel_however_big_the_panel() {
        let surface = surface();
        let stroke = geometry_of(&surface, "navigation.panel").stroke_width;
        assert!((stroke - 1.0).abs() < 0.05, "{stroke}");
    }
}
