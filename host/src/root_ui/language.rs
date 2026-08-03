//! A port of root-ui's design-language phases.
//!
//! `pin root_ui_replaces_current_surface` makes root-ui the implementation of
//! the editing surface, and the owner's own instruction was that root-ui may
//! stay a port. So this is a transcription of `root-ui/src/core/language.ts`,
//! not a design of our own: the phase order, the normalized layout, the
//! physical-pixel corner materialization and the user-owned colour scheme are
//! reproduced because they are root-ui's contract.
//!
//! The property that makes the port worth having, rather than a hand-drawn
//! panel, is the separation the phases enforce: layout is frozen before
//! decoration exists, and decoration is frozen before any colour is chosen. A
//! scheme change rebinds colour onto the same resolved layout instead of
//! re-laying-out the surface.

use std::collections::BTreeMap;
use std::fmt;

pub const FOUNDATION: &str = "root-ui-semantic-source-basis";

#[derive(Debug, PartialEq)]
pub enum Error {
    /// A value left the normalized 0..=1 range.
    Range(String),
    /// A semantic node was missing the source-basis shape.
    Semantic(String),
    UnknownScheme(String),
    MissingRole(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Range(what) => write!(f, "{what} must be within the normalized range 0..1"),
            Error::Semantic(what) => write!(f, "semantic node is incomplete: {what}"),
            Error::UnknownScheme(id) => write!(f, "unknown user color scheme: {id}"),
            Error::MissingRole(role) => write!(f, "scheme does not define color role: {role}"),
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// root-ui's source-basis shape. Kept whole rather than flattened, because the
/// widget catalogue keys off all three.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Semantic {
    pub name: String,
    pub variant: String,
    pub state: String,
}

impl Semantic {
    pub fn new(name: &str, variant: &str, state: &str) -> Self {
        Self { name: name.into(), variant: variant.into(), state: state.into() }
    }

    fn check(&self) -> Result<()> {
        for (field, value) in
            [("name", &self.name), ("variant", &self.variant), ("state", &self.state)]
        {
            if value.trim().is_empty() {
                return Err(Error::Semantic(field.to_string()));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// How a RoundBox states its corner.
///
/// Two forms, because one is not enough. A ratio of the shorter side keeps a
/// shape self-similar as it grows, which is what a capsule wants. An absolute
/// length keeps the corner the same size across components of different
/// heights, which is what a design system wants — and a ratio cannot say that
/// without a different number per component.
///
/// root-ui carried only the ratio until this was fixed upstream, and the
/// catalogue showed the cost: roughly half its explicit radii were `0.5`,
/// because a capsule is the one shape that looks the same at every size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CornerRadius {
    ShorterSideRatio(f32),
    /// Density-independent pixels, multiplied by the target scale once.
    Pixels(f32),
}

impl CornerRadius {
    /// The physical radius on one target, with overlap correction: a radius
    /// wider than half the shorter side has no rectangle left to round, so it
    /// is cut to the capsule instead of inverting the shape.
    pub fn physical(self, shorter_side: f32, scale: f32) -> f32 {
        match self {
            CornerRadius::ShorterSideRatio(ratio) => ratio * shorter_side,
            CornerRadius::Pixels(px) => (px * scale).min(shorter_side / 2.0),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoxKind {
    Box,
    RoundBox,
}

impl BoxKind {
    pub fn as_str(self) -> &'static str {
        match self {
            BoxKind::Box => "box",
            BoxKind::RoundBox => "round-box",
        }
    }
}

/// Material that reaches the shader but never participates in layout.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Decoration {
    pub stroke_width: f32,
}

/// Semantic colour roles, independent of any concrete scheme.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColorIntent {
    pub fill_role: String,
    pub stroke_role: String,
}

impl ColorIntent {
    pub fn new(fill_role: &str, stroke_role: &str) -> Self {
        Self { fill_role: fill_role.into(), stroke_role: stroke_role.into() }
    }
}

pub type Rgba = [f32; 4];

#[derive(Clone, Debug)]
pub struct ColorScheme {
    pub id: String,
    pub colors: BTreeMap<String, Rgba>,
}

#[derive(Clone, Debug)]
pub struct ColorRuntime {
    /// The preference is the user's; nothing in the phases may invent one.
    pub scheme_id: String,
    pub schemes: Vec<ColorScheme>,
}

#[derive(Clone, Debug)]
pub struct Sample {
    pub semantic: Semantic,
    pub kind: BoxKind,
    pub bounds: Bounds,
    pub decoration: Decoration,
    pub color: ColorIntent,
    pub corner_radius: CornerRadius,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedLayout {
    pub identity: String,
    pub normalized_bounds: [f32; 4],
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedDecoration {
    pub layout_identity: String,
    pub box_kind: BoxKind,
    pub corner_radius: CornerRadius,
    pub stroke_width_ratio: f32,
}

/// Physical geometry for one render target. Both corner axes take the same
/// pixel radius, so a change of aspect ratio cannot stretch a circular corner
/// into a UV-space ellipse.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PixelBoxGeometry {
    pub kind: BoxKind,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub corner_radius_x: f32,
    pub corner_radius_y: f32,
    pub stroke_width: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedColor {
    pub scheme_id: String,
    pub fill: Rgba,
    pub stroke: Rgba,
}

/// Layout and decoration, prepared exactly once. A colour preference change
/// binds a new [`ResolvedColor`] to this same frozen result.
#[derive(Clone, Debug)]
pub struct PreparedDesign {
    pub semantic: Semantic,
    pub color_intent: ColorIntent,
    pub layout: ResolvedLayout,
    pub decoration: ResolvedDecoration,
}

#[derive(Clone, Debug)]
pub struct ShaderOutput {
    pub semantic: Semantic,
    pub primitive: BoxKind,
    pub layout: ResolvedLayout,
    pub decoration: ResolvedDecoration,
    pub color: ResolvedColor,
}

fn unit(value: f32, field: &str) -> Result<f32> {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err(Error::Range(field.to_string()));
    }
    Ok(value)
}

/// This function cannot observe decoration or colour: neither is in its
/// signature.
pub fn resolve_layout(semantic: &Semantic, bounds: Bounds) -> Result<ResolvedLayout> {
    semantic.check()?;
    let normalized = [
        unit(bounds.x, "layout.bounds.x")?,
        unit(bounds.y, "layout.bounds.y")?,
        unit(bounds.width, "layout.bounds.width")?,
        unit(bounds.height, "layout.bounds.height")?,
    ];
    if normalized[0] + normalized[2] > 1.0 || normalized[1] + normalized[3] > 1.0 {
        return Err(Error::Range("layout.bounds".into()));
    }
    Ok(ResolvedLayout {
        identity: format!(
            "normalized-rectangle:{}:{}:{}:{}",
            normalized[0], normalized[1], normalized[2], normalized[3]
        ),
        normalized_bounds: normalized,
    })
}

/// Decoration consumes layout and returns no placement field of its own.
pub fn resolve_non_color_decoration(
    layout: &ResolvedLayout,
    kind: BoxKind,
    corner_radius: CornerRadius,
    decoration: Decoration,
) -> Result<ResolvedDecoration> {
    let stroke = unit(decoration.stroke_width, "decoration.strokeWidth")?;
    if stroke > 0.5 {
        return Err(Error::Range("decoration stroke width".into()));
    }
    let corner = match kind {
        BoxKind::RoundBox => match corner_radius {
            CornerRadius::ShorterSideRatio(ratio) => {
                let ratio = unit(ratio, "roundBox.cornerRadius")?;
                if ratio > 0.5 {
                    return Err(Error::Range("roundBox.cornerRadius".into()));
                }
                CornerRadius::ShorterSideRatio(ratio)
            }
            CornerRadius::Pixels(px) if px.is_finite() && px >= 0.0 => CornerRadius::Pixels(px),
            CornerRadius::Pixels(_) => {
                return Err(Error::Range("roundBox.cornerRadius in pixels".into()))
            }
        },
        BoxKind::Box => CornerRadius::ShorterSideRatio(0.0),
    };
    Ok(ResolvedDecoration {
        layout_identity: layout.identity.clone(),
        box_kind: kind,
        corner_radius: corner,
        stroke_width_ratio: stroke,
    })
}

pub fn materialize_pixel_box_geometry(
    layout: &ResolvedLayout,
    decoration: &ResolvedDecoration,
    target_width: f32,
    target_height: f32,
    scale: f32,
) -> Result<PixelBoxGeometry> {
    if !(target_width.is_finite() && target_height.is_finite())
        || target_width <= 0.0
        || target_height <= 0.0
    {
        return Err(Error::Range("box geometry target".into()));
    }
    if !scale.is_finite() || scale <= 0.0 {
        return Err(Error::Range("box geometry target scale".into()));
    }
    let [x, y, normalized_width, normalized_height] = layout.normalized_bounds;
    let width = normalized_width * target_width;
    let height = normalized_height * target_height;
    let shorter = width.min(height);
    let radius = decoration.corner_radius.physical(shorter, scale);
    Ok(PixelBoxGeometry {
        kind: decoration.box_kind,
        x: x * target_width,
        y: y * target_height,
        width,
        height,
        corner_radius_x: radius,
        corner_radius_y: radius,
        stroke_width: decoration.stroke_width_ratio * shorter,
    })
}

/// Resolve semantic colour roles against a runtime, user-owned preference.
pub fn resolve_color(intent: &ColorIntent, runtime: &ColorRuntime) -> Result<ResolvedColor> {
    let scheme = runtime
        .schemes
        .iter()
        .find(|scheme| scheme.id == runtime.scheme_id)
        .ok_or_else(|| Error::UnknownScheme(runtime.scheme_id.clone()))?;
    let fill = *scheme
        .colors
        .get(&intent.fill_role)
        .ok_or_else(|| Error::MissingRole(intent.fill_role.clone()))?;
    let stroke = *scheme
        .colors
        .get(&intent.stroke_role)
        .ok_or_else(|| Error::MissingRole(intent.stroke_role.clone()))?;
    Ok(ResolvedColor { scheme_id: scheme.id.clone(), fill, stroke })
}

pub fn prepare_design_language(sample: &Sample) -> Result<PreparedDesign> {
    let layout = resolve_layout(&sample.semantic, sample.bounds)?;
    let decoration = resolve_non_color_decoration(
        &layout,
        sample.kind,
        sample.corner_radius,
        sample.decoration,
    )?;
    Ok(PreparedDesign {
        semantic: sample.semantic.clone(),
        color_intent: sample.color.clone(),
        layout,
        decoration,
    })
}

pub fn bind_user_color_scheme(
    prepared: &PreparedDesign,
    runtime: &ColorRuntime,
) -> Result<ShaderOutput> {
    Ok(ShaderOutput {
        semantic: prepared.semantic.clone(),
        primitive: prepared.decoration.box_kind,
        layout: prepared.layout.clone(),
        decoration: prepared.decoration.clone(),
        color: resolve_color(&prepared.color_intent, runtime)?,
    })
}

/// `#rrggbb` / `#rrggbbaa` into the normalized tuple the phases speak.
pub fn rgba(hex: &str, alpha: f32) -> Rgba {
    let digits = hex.trim_start_matches('#');
    let value = u32::from_str_radix(&digits[..6.min(digits.len())], 16).unwrap_or(0);
    [
        ((value >> 16) & 0xFF) as f32 / 255.0,
        ((value >> 8) & 0xFF) as f32 / 255.0,
        (value & 0xFF) as f32 / 255.0,
        alpha.clamp(0.0, 1.0),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Sample {
        Sample {
            semantic: Semantic::new("Dialog", "surface", "open"),
            kind: BoxKind::RoundBox,
            bounds: Bounds { x: 0.1, y: 0.2, width: 0.5, height: 0.25 },
            decoration: Decoration { stroke_width: 0.01 },
            color: ColorIntent::new("surface", "outline"),
            corner_radius: CornerRadius::ShorterSideRatio(0.08),
        }
    }

    fn runtime() -> ColorRuntime {
        let mut dark = BTreeMap::new();
        dark.insert("surface".to_string(), rgba("#12141c", 0.92));
        dark.insert("outline".to_string(), rgba("#3b6ea5", 1.0));
        let mut light = BTreeMap::new();
        light.insert("surface".to_string(), rgba("#ffffff", 0.92));
        light.insert("outline".to_string(), rgba("#3b6ea5", 1.0));
        ColorRuntime {
            scheme_id: "dark".into(),
            schemes: vec![
                ColorScheme { id: "dark".into(), colors: dark },
                ColorScheme { id: "light".into(), colors: light },
            ],
        }
    }

    #[test]
    fn a_scheme_change_reuses_the_same_resolved_layout() {
        let prepared = prepare_design_language(&sample()).unwrap();
        let mut runtime = runtime();
        let dark = bind_user_color_scheme(&prepared, &runtime).unwrap();
        runtime.scheme_id = "light".into();
        let light = bind_user_color_scheme(&prepared, &runtime).unwrap();
        assert_eq!(dark.layout, light.layout);
        assert_eq!(dark.decoration, light.decoration);
        assert_ne!(dark.color, light.color);
    }

    #[test]
    fn a_circular_corner_stays_circular_across_aspect_ratios() {
        let prepared = prepare_design_language(&sample()).unwrap();
        let wide = materialize_pixel_box_geometry(
            &prepared.layout,
            &prepared.decoration,
            1600.0,
            400.0,
            1.0,
        )
        .unwrap();
        assert_eq!(wide.corner_radius_x, wide.corner_radius_y);
        let tall =
            materialize_pixel_box_geometry(&prepared.layout, &prepared.decoration, 400.0, 1600.0, 1.0)
                .unwrap();
        assert_eq!(tall.corner_radius_x, tall.corner_radius_y);
    }

    #[test]
    fn the_corner_radius_is_a_fraction_of_the_shorter_physical_side() {
        let prepared = prepare_design_language(&sample()).unwrap();
        let geometry =
            materialize_pixel_box_geometry(&prepared.layout, &prepared.decoration, 1000.0, 800.0, 1.0)
                .unwrap();
        let shorter = geometry.width.min(geometry.height);
        assert!((geometry.corner_radius_x - 0.08 * shorter).abs() < 1e-4);
    }

    #[test]
    fn a_plain_box_has_no_corner_radius_even_if_one_was_supplied() {
        let mut sample = sample();
        sample.kind = BoxKind::Box;
        let prepared = prepare_design_language(&sample).unwrap();
        assert_eq!(prepared.decoration.corner_radius, CornerRadius::ShorterSideRatio(0.0));
    }

    #[test]
    fn one_absolute_radius_gives_boxes_of_different_heights_the_same_corner() {
        // The reason the absolute form exists: a ratio cannot say this, and the
        // value a catalogue reaches for instead is the capsule.
        let radius_for = |height: f32| {
            let mut sample = sample();
            sample.corner_radius = CornerRadius::Pixels(8.0);
            sample.bounds = Bounds { x: 0.0, y: 0.0, width: 1.0, height };
            let prepared = prepare_design_language(&sample).unwrap();
            materialize_pixel_box_geometry(&prepared.layout, &prepared.decoration, 1000.0, 800.0, 1.0)
                .unwrap()
                .corner_radius_x
        };
        assert_eq!(radius_for(0.05), 8.0);
        assert_eq!(radius_for(0.6), 8.0);
    }

    #[test]
    fn an_absolute_radius_scales_with_display_density_and_a_ratio_does_not() {
        let mut absolute = sample();
        absolute.corner_radius = CornerRadius::Pixels(8.0);
        let prepared = prepare_design_language(&absolute).unwrap();
        let at = |scale: f32| {
            materialize_pixel_box_geometry(
                &prepared.layout,
                &prepared.decoration,
                1000.0,
                800.0,
                scale,
            )
            .unwrap()
            .corner_radius_x
        };
        assert_eq!(at(1.0) * 2.0, at(2.0));

        let ratio = prepare_design_language(&sample()).unwrap();
        let ratio_at = |scale: f32| {
            materialize_pixel_box_geometry(&ratio.layout, &ratio.decoration, 1000.0, 800.0, scale)
                .unwrap()
                .corner_radius_x
        };
        assert_eq!(ratio_at(1.0), ratio_at(2.0));
    }

    #[test]
    fn an_absolute_radius_wider_than_the_box_is_cut_to_the_capsule() {
        let mut sample = sample();
        sample.corner_radius = CornerRadius::Pixels(9999.0);
        sample.bounds = Bounds { x: 0.0, y: 0.0, width: 1.0, height: 0.05 };
        let prepared = prepare_design_language(&sample).unwrap();
        let geometry =
            materialize_pixel_box_geometry(&prepared.layout, &prepared.decoration, 1000.0, 800.0, 1.0)
                .unwrap();
        assert_eq!(geometry.corner_radius_x, geometry.height / 2.0);
    }

    #[test]
    fn a_negative_absolute_radius_is_refused() {
        let mut sample = sample();
        sample.corner_radius = CornerRadius::Pixels(-1.0);
        assert!(matches!(prepare_design_language(&sample), Err(Error::Range(_))));
    }

    #[test]
    fn bounds_that_leave_the_target_are_refused() {
        let mut sample = sample();
        sample.bounds = Bounds { x: 0.8, y: 0.0, width: 0.5, height: 0.1 };
        assert!(matches!(prepare_design_language(&sample), Err(Error::Range(_))));
    }

    #[test]
    fn an_empty_semantic_field_is_refused_before_layout_exists() {
        let mut sample = sample();
        sample.semantic.variant = "  ".into();
        assert!(matches!(prepare_design_language(&sample), Err(Error::Semantic(_))));
    }

    #[test]
    fn an_unknown_scheme_or_role_is_named_in_the_error() {
        let prepared = prepare_design_language(&sample()).unwrap();
        let mut runtime = runtime();
        runtime.scheme_id = "solarized".into();
        assert_eq!(
            bind_user_color_scheme(&prepared, &runtime).err(),
            Some(Error::UnknownScheme("solarized".into()))
        );
    }
}
