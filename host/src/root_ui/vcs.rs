//! VCS read-only surfaces described in root-ui terms.
//!
//! The protocol host can also expose this information as scratch buffers, but
//! the surface vocabulary lives here so the visual design stays role-based.

use super::language::{
    Bounds, BoxKind, ColorIntent, CornerRadius, Decoration, Sample, Semantic, Shadow,
};
use super::navigation::TextRun;
use super::FlatScene;

const PANEL_RADIUS: f32 = 10.0;
const ROW_RADIUS: f32 = 5.0;

pub struct RowInput {
    pub title: String,
    pub detail: String,
    pub selected: bool,
}

pub struct Input<'a> {
    pub title: &'a str,
    pub rows: &'a [RowInput],
    pub window_w: f32,
    pub window_h: f32,
    pub cell_w: f32,
    pub cell_h: f32,
    pub ascent: f32,
    pub scale: f32,
}

pub struct VcsSurface {
    pub scene: FlatScene,
    pub texts: Vec<TextRun>,
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

    fn hairline(&self, w: f32, h: f32) -> f32 {
        (self.scale / w.min(h).max(1.0)).min(0.5)
    }

    fn shape(
        &mut self,
        id: String,
        name: &'static str,
        state: &'static str,
        rect: (f32, f32, f32, f32),
        radius: Option<f32>,
        fill_role: &'static str,
        stroke_role: &'static str,
        shadow: bool,
    ) {
        let (x, y, w, h) = rect;
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        self.surfaces.push((
            id,
            Sample {
                semantic: Semantic::new(name, "vcs", state),
                kind: if radius.is_some() {
                    BoxKind::RoundBox
                } else {
                    BoxKind::Box
                },
                bounds: self.normalized(x, y, w, h),
                decoration: Decoration {
                    stroke_width: self.hairline(w, h),
                    shadow: shadow.then(|| Shadow::drop(10.0, 24.0)),
                },
                color: if shadow {
                    ColorIntent::new(fill_role, stroke_role).with_shadow("shadow")
                } else {
                    ColorIntent::new(fill_role, stroke_role)
                },
                corner_radius: CornerRadius::Pixels(radius.unwrap_or(0.0)),
            },
        ));
    }

    fn text(&mut self, x: f32, baseline: f32, text: String, role: &'static str, max_x: f32) {
        if text.is_empty() {
            return;
        }
        self.texts.push(TextRun {
            x,
            baseline,
            text,
            role,
            max_x,
        });
    }
}

pub fn build(input: Input<'_>) -> VcsSurface {
    let window_w = input.window_w.max(1.0);
    let window_h = input.window_h.max(1.0);
    let scale = input.scale.max(0.5);
    let dp = |value: f32| value * scale;
    let mut composer = Composer {
        surfaces: Vec::new(),
        texts: Vec::new(),
        window_w,
        window_h,
        scale,
    };

    let panel_w = (window_w * 0.58).clamp(360.0, (window_w - dp(32.0)).max(360.0));
    let panel_h = (window_h * 0.58).clamp(180.0, (window_h - dp(32.0)).max(180.0));
    let panel_x = ((window_w - panel_w) / 2.0).floor();
    let panel_y = ((window_h - panel_h) / 3.0).floor();
    composer.shape(
        "vcs.scrim".into(),
        "Scrim",
        "open",
        (0.0, 0.0, window_w, window_h),
        None,
        "scrim",
        "scrim",
        false,
    );
    composer.shape(
        "vcs.panel".into(),
        "Dialog",
        "open",
        (panel_x, panel_y, panel_w, panel_h),
        Some(PANEL_RADIUS),
        "surface",
        "outline",
        true,
    );

    let pad = dp(16.0);
    let title_h = input.cell_h * 2.0;
    composer.text(
        panel_x + pad,
        panel_y + (title_h + input.ascent) / 2.0,
        input.title.to_string(),
        "on_surface",
        panel_x + panel_w - pad,
    );

    let row_h = input.cell_h * 1.45;
    let top = panel_y + title_h + dp(8.0);
    for (index, row) in input.rows.iter().enumerate() {
        let y = top + row_h * index as f32;
        if y + row_h > panel_y + panel_h - pad {
            break;
        }
        if row.selected {
            composer.shape(
                format!("vcs.row.{index}"),
                "ListItem",
                "selected",
                (panel_x + dp(6.0), y, panel_w - dp(12.0), row_h),
                Some(ROW_RADIUS),
                "surface_raised",
                "surface_raised",
                false,
            );
        }
        let baseline = y + (row_h + input.ascent) / 2.0 - dp(1.0);
        composer.text(
            panel_x + pad,
            baseline,
            row.title.clone(),
            "on_surface",
            panel_x + panel_w * 0.45,
        );
        composer.text(
            panel_x + panel_w * 0.45,
            baseline,
            row.detail.clone(),
            "on_surface_muted",
            panel_x + panel_w - pad,
        );
    }

    VcsSurface {
        scene: FlatScene {
            surfaces: composer.surfaces,
        },
        texts: composer.texts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vcs_surface_uses_roles_not_colours() {
        let rows = vec![RowInput {
            title: "1 +".into(),
            detail: "added line".into(),
            selected: true,
        }];
        let surface = build(Input {
            title: "Hunks",
            rows: &rows,
            window_w: 800.0,
            window_h: 600.0,
            cell_w: 9.0,
            cell_h: 18.0,
            ascent: 13.0,
            scale: 2.0,
        });
        assert!(surface.texts.iter().any(|run| run.role == "on_surface"));
        assert!(surface
            .scene
            .surfaces
            .iter()
            .any(|(id, _)| id == "vcs.panel"));
    }
}
