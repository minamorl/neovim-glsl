//! A surface that is not the grid.
//!
//! Everything else this program draws is addressed in cells: a glyph lands at
//! `col * cell_w`, a row occupies exactly `cell_h`, and a thing that wants to
//! sit half a cell lower has nowhere to sit. That is not a limitation of GLSL,
//! it is a limitation of speaking in a terminal's coordinates.
//!
//! This module addresses pixels instead. It exists to *measure* what the grid
//! cannot express — fractional origins, fractional scroll, translucency over
//! live text, glyphs cut mid-stroke at a boundary the cell raster has no name
//! for — so that the choice recorded in
//! `open_question neovim_glsl.navigation_surface_decision` can be made against
//! observation rather than against a guess.
//!
//! It decides nothing. It is not a picker: it has no matcher, no source, no
//! action and no key handling. It is the surface such a thing would need, drawn
//! and counted.

use serde::{Deserialize, Serialize};

use crate::text::Atlas;

/// A rectangle in pixels. Panels clip to it; nothing is drawn outside it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
}

/// One line of the panel body.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PanelRow {
    pub text: String,
    #[serde(default)]
    pub selected: bool,
}

/// A free surface: a translucent panel of text rows, placed and scrolled in
/// pixels rather than in cells.
///
/// Colours are `#rrggbb`. `alpha` applies to the panel's own background only —
/// the text stays opaque, because a picker whose labels fade with its backdrop
/// is unreadable, and this file is meant to show what is possible, not what is
/// pretty.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Panel {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// Height of one body row. Deliberately independent of `Atlas::cell_h`.
    pub row_height: f32,
    /// Body scroll in pixels. Fractional values leave the first visible row
    /// cut partway through its glyphs.
    #[serde(default)]
    pub scroll: f32,
    /// Panel background alpha, 0..=1.
    #[serde(default = "default_alpha")]
    pub alpha: f32,
    #[serde(default = "default_bg")]
    pub bg: String,
    #[serde(default = "default_fg")]
    pub fg: String,
    #[serde(default = "default_accent")]
    pub accent: String,
    /// Text of the query line drawn at the top of the panel. Empty hides it.
    #[serde(default)]
    pub query: String,
    /// Left inset for text, in pixels.
    #[serde(default = "default_padding")]
    pub padding: f32,
    pub rows: Vec<PanelRow>,
}

fn default_alpha() -> f32 {
    0.88
}
fn default_bg() -> String {
    "#161821".into()
}
fn default_fg() -> String {
    "#d8dee9".into()
}
fn default_accent() -> String {
    "#3b6ea5".into()
}
fn default_padding() -> f32 {
    10.0
}

/// What one panel pass actually emitted. Counts, not judgements.
#[derive(Clone, Copy, Default, PartialEq, Debug, Serialize)]
pub struct PanelStats {
    /// Quads pushed, including backgrounds and glyphs.
    pub quads: usize,
    /// Quads that met an edge and were cut, keeping their UV proportional.
    pub clipped_quads: usize,
    /// Body rows that contributed at least one pixel.
    pub rows_visible: usize,
    /// Rows the scroll offset pushed off the top entirely.
    pub rows_scrolled_out: usize,
    /// Pixels the first visible row lost to the top edge. Non-integral here is
    /// the whole point: a cell grid can only ever report 0.
    pub first_row_clip_px: f32,
    /// Pixels the last visible row lost to the bottom edge.
    pub last_row_clip_px: f32,
    /// True when the panel origin is not on a cell boundary.
    pub origin_off_grid: bool,
}

/// Parse `#rrggbb` (or `rrggbb`) into a packed RGB word.
pub fn parse_color(s: &str) -> u32 {
    let t = s.trim().trim_start_matches('#');
    u32::from_str_radix(t, 16).unwrap_or(0) & 0x00ff_ffff
}

fn rgba(c: u32, a: f32) -> [f32; 4] {
    [
        ((c >> 16) & 0xff) as f32 / 255.0,
        ((c >> 8) & 0xff) as f32 / 255.0,
        (c & 0xff) as f32 / 255.0,
        a,
    ]
}

/// Push one quad, cut to `clip`, with its UV rescaled by however much was cut.
///
/// The rescaling is the part that matters. Dropping a glyph that crosses the
/// edge is easy and wrong; keeping it whole and letting it spill is easy and
/// wrong. Cutting it and moving the texture coordinates with the cut is what
/// makes a boundary that no cell edge explains look like it was meant.
///
/// Returns `(pushed, was_clipped)`.
pub fn push_quad_clipped(
    verts: &mut Vec<f32>,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    uv: (f32, f32, f32, f32),
    col: [f32; 4],
    clip: Rect,
) -> (bool, bool) {
    if w <= 0.0 || h <= 0.0 {
        return (false, false);
    }
    let (x0, y0) = (x.max(clip.x), y.max(clip.y));
    let (x1, y1) = ((x + w).min(clip.right()), (y + h).min(clip.bottom()));
    if x1 <= x0 || y1 <= y0 {
        return (false, true);
    }
    let clipped = x0 > x || y0 > y || x1 < x + w || y1 < y + h;

    let (u0, v0, u1, v1) = uv;
    let (du, dv) = (u1 - u0, v1 - v0);
    let lerp = |a: f32, d: f32, t: f32| a + d * t;
    let (tu0, tu1) = ((x0 - x) / w, (x1 - x) / w);
    let (tv0, tv1) = ((y0 - y) / h, (y1 - y) / h);
    let (cu0, cu1) = (lerp(u0, du, tu0), lerp(u0, du, tu1));
    let (cv0, cv1) = (lerp(v0, dv, tv0), lerp(v0, dv, tv1));

    let quad = [
        (x0, y0, cu0, cv0),
        (x1, y0, cu1, cv0),
        (x1, y1, cu1, cv1),
        (x0, y0, cu0, cv0),
        (x1, y1, cu1, cv1),
        (x0, y1, cu0, cv1),
    ];
    for (px, py, pu, pv) in quad {
        verts.extend_from_slice(&[px, py, pu, pv, col[0], col[1], col[2], col[3]]);
    }
    (true, clipped)
}

/// Draw a string at a pixel origin, clipped to `clip`.
///
/// Advance comes from the atlas cell width so the panel stays monospaced —
/// proportional text is a separate question and this file does not open it.
fn push_text(
    verts: &mut Vec<f32>,
    atlas: &mut Atlas,
    text: &str,
    x: f32,
    baseline_top: f32,
    col: [f32; 4],
    clip: Rect,
    stats: &mut PanelStats,
) {
    let advance = atlas.cell_w;
    let mut pen = x;
    for ch in text.chars() {
        if pen >= clip.right() {
            break;
        }
        if let Some(g) = atlas.styled_glyph(ch, false, false) {
            let (pushed, clipped) = push_quad_clipped(
                verts,
                pen + g.bearing_x,
                baseline_top + atlas.ascent - g.bearing_y,
                g.w,
                g.h,
                (g.u0, g.v0, g.u1, g.v1),
                col,
                clip,
            );
            if pushed {
                stats.quads += 1;
            }
            if clipped {
                stats.clipped_quads += 1;
            }
        }
        pen += advance;
    }
}

impl Panel {
    pub fn rect(&self) -> Rect {
        Rect { x: self.x, y: self.y, w: self.w, h: self.h }
    }

    /// Height reserved at the top for the query line. Zero when there is none.
    fn header_height(&self) -> f32 {
        if self.query.is_empty() {
            0.0
        } else {
            self.row_height + self.padding * 0.5
        }
    }

    /// Emit the whole panel into `verts` and report what that took.
    pub fn push(&self, verts: &mut Vec<f32>, atlas: &mut Atlas) -> PanelStats {
        let mut stats = PanelStats::default();
        let clip = self.rect();
        let (wu, wv) = atlas.white_uv();
        let white = (wu, wv, wu, wv);
        let fg = parse_color(&self.fg);
        let bg = parse_color(&self.bg);
        let accent = parse_color(&self.accent);

        stats.origin_off_grid =
            (self.x % atlas.cell_w).abs() > f32::EPSILON || (self.y % atlas.cell_h).abs() > f32::EPSILON;

        // The backdrop. Translucent, so the editor text underneath stays
        // legible through it — a thing the grid cannot express at all, since a
        // cell holds one background colour and no opacity.
        let (pushed, _) = push_quad_clipped(
            verts, self.x, self.y, self.w, self.h, white, rgba(bg, self.alpha.clamp(0.0, 1.0)), clip,
        );
        if pushed {
            stats.quads += 1;
        }

        // Query line, if any, plus the rule under it.
        let header = self.header_height();
        if header > 0.0 {
            push_text(
                verts,
                atlas,
                &self.query,
                self.x + self.padding,
                self.y + self.padding * 0.25,
                rgba(fg, 1.0),
                clip,
                &mut stats,
            );
            let (pushed, clipped) = push_quad_clipped(
                verts,
                self.x + self.padding * 0.5,
                self.y + header - 1.0,
                self.w - self.padding,
                1.0,
                white,
                rgba(accent, 0.9),
                clip,
            );
            if pushed {
                stats.quads += 1;
            }
            if clipped {
                stats.clipped_quads += 1;
            }
        }

        // Body. `scroll` is in pixels, so the first visible row is generally
        // cut partway through, and the cut is where the evidence is.
        let body_top = self.y + header;
        let body = Rect { x: self.x, y: body_top, w: self.w, h: self.bottom_of_body() };
        let mut first_seen = false;
        for (i, row) in self.rows.iter().enumerate() {
            let top = body_top + i as f32 * self.row_height - self.scroll;
            let bottom = top + self.row_height;
            if bottom <= body.y {
                stats.rows_scrolled_out += 1;
                continue;
            }
            if top >= body.bottom() {
                break;
            }
            stats.rows_visible += 1;
            if !first_seen {
                stats.first_row_clip_px = (body.y - top).max(0.0);
                first_seen = true;
            }
            stats.last_row_clip_px = (bottom - body.bottom()).max(0.0);

            if row.selected {
                let (pushed, clipped) = push_quad_clipped(
                    verts,
                    self.x + self.padding * 0.4,
                    top,
                    self.w - self.padding * 0.8,
                    self.row_height,
                    white,
                    rgba(accent, 0.85),
                    body,
                );
                if pushed {
                    stats.quads += 1;
                }
                if clipped {
                    stats.clipped_quads += 1;
                }
            }
            push_text(
                verts,
                atlas,
                &row.text,
                self.x + self.padding,
                top + (self.row_height - atlas.cell_h) * 0.5,
                rgba(fg, 1.0),
                body,
                &mut stats,
            );
        }
        stats
    }

    fn bottom_of_body(&self) -> f32 {
        (self.h - self.header_height()).max(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gl::VERTEX_FLOATS;

    fn atlas() -> Atlas {
        Atlas::new(15.0)
    }

    fn vert(verts: &[f32], i: usize) -> (f32, f32, f32, f32, f32) {
        let o = i * VERTEX_FLOATS;
        (verts[o], verts[o + 1], verts[o + 2], verts[o + 3], verts[o + 7])
    }

    fn panel(rows: usize) -> Panel {
        Panel {
            x: 40.5,
            y: 30.25,
            w: 300.0,
            h: 120.0,
            row_height: 22.0,
            scroll: 0.0,
            alpha: 0.8,
            bg: "#161821".into(),
            fg: "#d8dee9".into(),
            accent: "#3b6ea5".into(),
            query: String::new(),
            padding: 10.0,
            rows: (0..rows)
                .map(|i| PanelRow { text: format!("row{i}"), selected: false })
                .collect(),
        }
    }

    /// The origin survives as given. A cell-addressed surface would have to
    /// round it to a column and a row; this one does not, and that is the only
    /// reason the panel can sit anywhere.
    #[test]
    fn panel_origin_keeps_its_fraction() {
        let mut a = atlas();
        let mut v = Vec::new();
        let p = panel(0);
        let stats = p.push(&mut v, &mut a);
        let (x, y, ..) = vert(&v, 0);
        assert_eq!(x, 40.5);
        assert_eq!(y, 30.25);
        assert!(stats.origin_off_grid);
    }

    /// Backdrop alpha reaches the vertex stream, so the text underneath shows
    /// through instead of being replaced.
    #[test]
    fn backdrop_carries_alpha() {
        let mut a = atlas();
        let mut v = Vec::new();
        panel(0).push(&mut v, &mut a);
        let (.., alpha) = vert(&v, 0);
        assert!((alpha - 0.8).abs() < 1e-6, "alpha was {alpha}");
    }

    /// A quad crossing the clip edge is cut, and its texture coordinates are
    /// cut by the same proportion. Half the quad shows half the glyph, not a
    /// squeezed whole one.
    #[test]
    fn clipping_rescales_uv_proportionally() {
        let mut v = Vec::new();
        let clip = Rect { x: 0.0, y: 0.0, w: 10.0, h: 100.0 };
        let (pushed, clipped) =
            push_quad_clipped(&mut v, 0.0, 0.0, 20.0, 10.0, (0.0, 0.0, 1.0, 1.0), [1.0; 4], clip);
        assert!(pushed && clipped);
        let (_, _, _, _, _) = vert(&v, 0);
        // Second vertex is the top-right corner: x cut to 10 of 20, u to 0.5.
        let (x, _, u, ..) = vert(&v, 1);
        assert_eq!(x, 10.0);
        assert!((u - 0.5).abs() < 1e-6, "u was {u}");
    }

    /// A quad entirely outside the clip contributes nothing but is reported as
    /// clipped, so the count distinguishes "cut" from "never asked for".
    #[test]
    fn fully_outside_quad_is_dropped() {
        let mut v = Vec::new();
        let clip = Rect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 };
        let (pushed, clipped) =
            push_quad_clipped(&mut v, 50.0, 50.0, 5.0, 5.0, (0.0, 0.0, 1.0, 1.0), [1.0; 4], clip);
        assert!(!pushed && clipped);
        assert!(v.is_empty());
    }

    /// Fractional scroll leaves the first visible row cut by a non-integral
    /// number of pixels. This is the observation the cell grid cannot make:
    /// its only legal answers are 0 and one whole row.
    #[test]
    fn fractional_scroll_cuts_the_first_row_partway() {
        let mut a = atlas();
        let mut v = Vec::new();
        let mut p = panel(6);
        p.scroll = 7.5;
        let stats = p.push(&mut v, &mut a);
        assert_eq!(stats.rows_scrolled_out, 0);
        assert!((stats.first_row_clip_px - 7.5).abs() < 1e-6, "{stats:?}");
    }

    /// Scrolling past a whole row retires it rather than clipping it.
    #[test]
    fn scroll_past_a_row_retires_it() {
        let mut a = atlas();
        let mut v = Vec::new();
        let mut p = panel(6);
        p.scroll = 30.0; // one 22px row plus 8
        let stats = p.push(&mut v, &mut a);
        assert_eq!(stats.rows_scrolled_out, 1);
        assert!((stats.first_row_clip_px - 8.0).abs() < 1e-6, "{stats:?}");
    }

    /// Rows below the panel are never emitted, however many the caller passes.
    #[test]
    fn rows_below_the_panel_are_not_emitted() {
        let mut a = atlas();
        let mut v = Vec::new();
        let p = panel(400);
        let stats = p.push(&mut v, &mut a);
        let capacity = (p.h / p.row_height).ceil() as usize + 1;
        assert!(stats.rows_visible <= capacity, "{stats:?}");
        assert!(stats.rows_visible > 0);
    }

    /// Row height is the panel's own, not the terminal cell's. Two rows are
    /// exactly `row_height` apart even when that is not `cell_h`.
    #[test]
    fn row_pitch_is_independent_of_cell_height() {
        let mut a = atlas();
        let cell_h = a.cell_h;
        let mut p = panel(2);
        p.row_height = cell_h + 7.0;
        p.rows[0].selected = true;
        p.rows[1].selected = true;
        let mut v = Vec::new();
        p.push(&mut v, &mut a);
        // Vertex 0 is the backdrop; the two selection bands follow in order,
        // each emitted before its own text.
        let bands: Vec<f32> = (0..v.len() / VERTEX_FLOATS)
            .map(|i| vert(&v, i))
            .filter(|(_, _, u, vv, alpha)| {
                // Selection bands use the white texel and the accent alpha.
                (*u - a.white_uv().0).abs() < 1e-9 && (*vv - a.white_uv().1).abs() < 1e-9
                    && (*alpha - 0.85).abs() < 1e-6
            })
            .map(|(_, y, ..)| y)
            .collect();
        assert!(bands.len() >= 12, "expected two bands of six vertices, got {}", bands.len());
        let first_top = bands[0];
        let second_top = bands[6];
        assert!(
            (second_top - first_top - p.row_height).abs() < 1e-4,
            "pitch was {} not {}",
            second_top - first_top,
            p.row_height
        );
    }

    /// Text stops at the panel's right edge instead of spilling across the
    /// screen, and the cut is counted.
    #[test]
    fn text_is_cut_at_the_right_edge() {
        let mut a = atlas();
        let mut p = panel(1);
        p.w = a.cell_w * 4.0;
        p.rows[0].text = "wwwwwwwwwwwwwwwwwwww".into();
        let mut v = Vec::new();
        let stats = p.push(&mut v, &mut a);
        let right = p.x + p.w;
        for i in 0..v.len() / VERTEX_FLOATS {
            let (x, ..) = vert(&v, i);
            assert!(x <= right + 1e-4, "vertex at {x} escaped right edge {right}");
        }
        assert!(stats.clipped_quads > 0, "{stats:?}");
    }

    /// The same panel emits the same geometry every time. Without this the
    /// vertex counts in the evidence file would not mean anything.
    #[test]
    fn panel_geometry_is_deterministic() {
        let mut a = atlas();
        let p = panel(8);
        let (mut v1, mut v2) = (Vec::new(), Vec::new());
        let s1 = p.push(&mut v1, &mut a);
        let s2 = p.push(&mut v2, &mut a);
        assert_eq!(s1, s2);
        assert_eq!(v1.len(), v2.len());
        assert!(v1.iter().zip(v2.iter()).all(|(a, b)| a == b));
    }

    /// A query line pushes the body down by its own height, so the first row
    /// never lands under the header.
    #[test]
    fn query_line_reserves_space_above_the_body() {
        let mut a = atlas();
        let mut p = panel(1);
        p.query = "alpha".into();
        p.rows[0].selected = true;
        let mut v = Vec::new();
        p.push(&mut v, &mut a);
        let body_top = p.y + p.row_height + p.padding * 0.5;
        let band_top = (0..v.len() / VERTEX_FLOATS)
            .map(|i| vert(&v, i))
            .find(|(_, _, u, vv, alpha)| {
                (*u - a.white_uv().0).abs() < 1e-9
                    && (*vv - a.white_uv().1).abs() < 1e-9
                    && (*alpha - 0.85).abs() < 1e-6
            })
            .map(|(_, y, ..)| y)
            .expect("selection band");
        assert!((band_top - body_top).abs() < 1e-4, "band at {band_top}, body at {body_top}");
    }

    #[test]
    fn colors_parse_with_or_without_hash() {
        assert_eq!(parse_color("#ff8000"), 0xff8000);
        assert_eq!(parse_color("ff8000"), 0xff8000);
        assert_eq!(parse_color("nonsense"), 0);
    }
}
