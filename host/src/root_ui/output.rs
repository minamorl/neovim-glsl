//! Task output, composed as a root-ui flat scene.
//!
//! This panel is not navigation. It is a process-output surface, so it may move
//! to a real window later; for this lane it stays an overlay surface and uses
//! the same adapter discipline as navigation.

use std::collections::BTreeMap;

use super::language::{
    rgba, Bounds, BoxKind, ColorIntent, ColorRuntime, ColorScheme, CornerRadius, Decoration,
    Sample, Semantic,
};
use super::FlatScene;

const PANEL_RADIUS: f32 = 8.0;

pub struct TextRun {
    pub x: f32,
    pub baseline: f32,
    pub text: String,
    pub role: &'static str,
    pub max_x: f32,
}

pub struct OutputSurface {
    pub scene: FlatScene,
    pub texts: Vec<TextRun>,
}

#[derive(Clone)]
pub struct SegmentInput {
    pub text: String,
    pub role: crate::run::Role,
}

#[derive(Clone, Default)]
pub struct LineInput {
    pub segments: Vec<SegmentInput>,
}

pub struct Input<'a> {
    pub argv: &'a [String],
    pub cwd: &'a str,
    pub status: Option<&'a str>,
    pub lines: &'a [LineInput],
    pub scroll: usize,
    pub window_w: f32,
    pub window_h: f32,
    pub cell_w: f32,
    pub cell_h: f32,
    pub ascent: f32,
    pub scale: f32,
}

pub fn dark_scheme() -> ColorScheme {
    let mut colors = BTreeMap::new();
    colors.insert("surface".into(), rgba("#111722", 0.98));
    colors.insert("surface_raised".into(), rgba("#1a2230", 1.0));
    colors.insert("outline".into(), rgba("#334050", 1.0));
    colors.insert("separator".into(), rgba("#263241", 1.0));
    colors.insert("muted".into(), rgba("#8a93a6", 1.0));
    colors.insert("stdout".into(), rgba("#dce6f2", 1.0));
    colors.insert("stderr".into(), rgba("#ff8f8f", 1.0));
    colors.insert("black".into(), rgba("#6f788b", 1.0));
    colors.insert("red".into(), rgba("#ff6b6b", 1.0));
    colors.insert("green".into(), rgba("#69d58a", 1.0));
    colors.insert("yellow".into(), rgba("#e8c35a", 1.0));
    colors.insert("blue".into(), rgba("#7fa7f5", 1.0));
    colors.insert("magenta".into(), rgba("#d78cff", 1.0));
    colors.insert("cyan".into(), rgba("#5dd7df", 1.0));
    colors.insert("white".into(), rgba("#f2f6fb", 1.0));
    colors.insert("bright_black".into(), rgba("#9aa4b6", 1.0));
    colors.insert("bright_red".into(), rgba("#ff9b9b", 1.0));
    colors.insert("bright_green".into(), rgba("#9af0b0", 1.0));
    colors.insert("bright_yellow".into(), rgba("#f5dc7a", 1.0));
    colors.insert("bright_blue".into(), rgba("#a9c5ff", 1.0));
    colors.insert("bright_magenta".into(), rgba("#e3b3ff", 1.0));
    colors.insert("bright_cyan".into(), rgba("#91edf1", 1.0));
    colors.insert("bright_white".into(), rgba("#ffffff", 1.0));
    ColorScheme {
        id: "dark".into(),
        colors,
    }
}

pub fn light_scheme() -> ColorScheme {
    let mut colors = BTreeMap::new();
    colors.insert("surface".into(), rgba("#ffffff", 0.98));
    colors.insert("surface_raised".into(), rgba("#edf2fb", 1.0));
    colors.insert("outline".into(), rgba("#b9c3d3", 1.0));
    colors.insert("separator".into(), rgba("#dbe2ec", 1.0));
    colors.insert("muted".into(), rgba("#6f788b", 1.0));
    colors.insert("stdout".into(), rgba("#1b2430", 1.0));
    colors.insert("stderr".into(), rgba("#b42318", 1.0));
    colors.insert("black".into(), rgba("#4b5563", 1.0));
    colors.insert("red".into(), rgba("#c2410c", 1.0));
    colors.insert("green".into(), rgba("#15803d", 1.0));
    colors.insert("yellow".into(), rgba("#a16207", 1.0));
    colors.insert("blue".into(), rgba("#1d4ed8", 1.0));
    colors.insert("magenta".into(), rgba("#9d2fcb", 1.0));
    colors.insert("cyan".into(), rgba("#047481", 1.0));
    colors.insert("white".into(), rgba("#111827", 1.0));
    colors.insert("bright_black".into(), rgba("#6b7280", 1.0));
    colors.insert("bright_red".into(), rgba("#dc2626", 1.0));
    colors.insert("bright_green".into(), rgba("#16a34a", 1.0));
    colors.insert("bright_yellow".into(), rgba("#ca8a04", 1.0));
    colors.insert("bright_blue".into(), rgba("#2563eb", 1.0));
    colors.insert("bright_magenta".into(), rgba("#c026d3", 1.0));
    colors.insert("bright_cyan".into(), rgba("#0891b2", 1.0));
    colors.insert("bright_white".into(), rgba("#000000", 1.0));
    ColorScheme {
        id: "light".into(),
        colors,
    }
}

pub fn color_runtime(scheme_id: &str) -> ColorRuntime {
    ColorRuntime {
        scheme_id: scheme_id.to_string(),
        schemes: vec![dark_scheme(), light_scheme()],
    }
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

    fn shape(&mut self, id: &str, name: &str, rect: (f32, f32, f32, f32), radius: f32, fill: &str) {
        let (x, y, w, h) = rect;
        self.surfaces.push((
            id.to_string(),
            Sample {
                semantic: Semantic::new(name, "output", "open"),
                kind: if radius > 0.0 {
                    BoxKind::RoundBox
                } else {
                    BoxKind::Box
                },
                bounds: self.normalized(x, y, w, h),
                decoration: Decoration {
                    stroke_width: self.hairline(w, h),
                    shadow: None,
                },
                color: ColorIntent::new(fill, "outline"),
                corner_radius: CornerRadius::Pixels(radius),
            },
        ));
    }

    fn text(&mut self, x: f32, baseline: f32, text: String, role: &'static str, max_x: f32) {
        if !text.is_empty() {
            self.texts.push(TextRun {
                x,
                baseline,
                text,
                role,
                max_x,
            });
        }
    }
}

fn advance(text: &str, cell_w: f32) -> f32 {
    text.chars()
        .map(|c| crate::proto::paint::char_width(c) as f32 * cell_w)
        .sum()
}

pub fn build(input: Input<'_>) -> OutputSurface {
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

    let panel_x = dp(20.0);
    let panel_w = window_w - dp(40.0);
    // The editor's status line owns the last cell row of the window, and a
    // margin smaller than that row puts the panel on top of it — which is how
    // `exit status 0` came out printed over the file name.
    let bottom_gap = input.cell_h + dp(8.0);
    let panel_h = (window_h * 0.42).clamp(
        dp(180.0),
        (window_h - bottom_gap - dp(20.0)).max(dp(180.0)),
    );
    let panel_y = window_h - panel_h - bottom_gap;
    composer.shape(
        "output.panel",
        "Panel",
        (panel_x, panel_y, panel_w, panel_h),
        PANEL_RADIUS,
        "surface",
    );

    let pad = dp(14.0);
    let header_h = input.cell_h * 1.6;
    let argv = format!("$ {}  [{}]", input.argv.join(" "), input.cwd);
    composer.text(
        panel_x + pad,
        panel_y + (header_h + input.ascent) / 2.0,
        argv,
        "muted",
        panel_x + panel_w - pad,
    );
    composer.shape(
        "output.rule",
        "Separator",
        (
            panel_x + pad,
            panel_y + header_h,
            panel_w - pad * 2.0,
            scale.max(1.0),
        ),
        0.0,
        "separator",
    );

    let status_h = input.cell_h * 1.5;
    let body_top = panel_y + header_h + dp(8.0);
    let body_bottom = panel_y + panel_h - status_h - dp(4.0);
    let row_h = input.cell_h * 1.25;
    let visible = ((body_bottom - body_top) / row_h).floor().max(0.0) as usize;
    for (row, line) in input
        .lines
        .iter()
        .skip(input.scroll)
        .take(visible)
        .enumerate()
    {
        let baseline = body_top + row_h * row as f32 + input.ascent;
        let mut x = panel_x + pad;
        for segment in &line.segments {
            let role = role_name(segment.role);
            composer.text(
                x,
                baseline,
                segment.text.clone(),
                role,
                panel_x + panel_w - pad,
            );
            x += advance(&segment.text, input.cell_w);
            if x > panel_x + panel_w - pad {
                break;
            }
        }
    }

    if let Some(status) = input.status {
        composer.text(
            panel_x + pad,
            panel_y + panel_h - (status_h - input.ascent) / 2.0,
            status.to_string(),
            "muted",
            panel_x + panel_w - pad,
        );
    }

    if input.lines.len() > visible && visible > 0 {
        let track_h = body_bottom - body_top;
        let thumb_h = (track_h * visible as f32 / input.lines.len() as f32).max(dp(18.0));
        let travel = (track_h - thumb_h).max(0.0);
        let progress = input.scroll as f32 / (input.lines.len() - visible).max(1) as f32;
        composer.shape(
            "output.scroll",
            "ScrollBar",
            (
                panel_x + panel_w - dp(7.0),
                body_top + travel * progress.clamp(0.0, 1.0),
                dp(3.0),
                thumb_h,
            ),
            1.5,
            "muted",
        );
    }

    OutputSurface {
        scene: FlatScene {
            surfaces: composer.surfaces,
        },
        texts: composer.texts,
    }
}

fn role_name(role: crate::run::Role) -> &'static str {
    use crate::run::{AnsiColor as C, Stream};
    match role.color {
        Some(C::Black) => "black",
        Some(C::Red) => "red",
        Some(C::Green) => "green",
        Some(C::Yellow) => "yellow",
        Some(C::Blue) => "blue",
        Some(C::Magenta) => "magenta",
        Some(C::Cyan) => "cyan",
        Some(C::White) => "white",
        Some(C::BrightBlack) => "bright_black",
        Some(C::BrightRed) => "bright_red",
        Some(C::BrightGreen) => "bright_green",
        Some(C::BrightYellow) => "bright_yellow",
        Some(C::BrightBlue) => "bright_blue",
        Some(C::BrightMagenta) => "bright_magenta",
        Some(C::BrightCyan) => "bright_cyan",
        Some(C::BrightWhite) => "bright_white",
        None if role.stream == Stream::Stderr => "stderr",
        None => "stdout",
    }
}
