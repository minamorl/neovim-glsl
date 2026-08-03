//! root-ui, ported.
//!
//! The surface this program draws over the grid is not a hand-placed panel: it
//! is a root-ui flat scene — semantic input resolved through layout, then
//! non-colour decoration, then a user-owned colour scheme — handed to a shader
//! adapter. `pin root_ui_surface_adoption` scopes root-ui to the editing
//! surface; the navigation surface is the first one built this way.

pub mod adapter;
pub mod language;
pub mod navigation;

use language::{
    bind_user_color_scheme, prepare_design_language, ColorRuntime, PreparedDesign, Result, Sample,
    ShaderOutput,
};

/// Painter order is array order. This is root-ui's own deliberately small
/// composition witness, not a scene graph.
pub struct FlatScene {
    pub surfaces: Vec<(String, Sample)>,
}

pub struct PreparedFlatScene {
    pub surfaces: Vec<(String, PreparedDesign)>,
}

pub struct ShaderFlatScene {
    pub surfaces: Vec<(String, ShaderOutput)>,
}

/// Resolve every surface through layout and decoration exactly once. The colour
/// runtime is deliberately absent from this boundary.
pub fn prepare_flat_scene(scene: &FlatScene) -> Result<PreparedFlatScene> {
    let mut surfaces = Vec::with_capacity(scene.surfaces.len());
    for (id, sample) in &scene.surfaces {
        surfaces.push((id.clone(), prepare_design_language(sample)?));
    }
    Ok(PreparedFlatScene { surfaces })
}

/// Bind one user scheme across an already prepared composition. Every output
/// reuses its surface's exact resolved layout and decoration.
pub fn bind_flat_scene_user_color_scheme(
    prepared: &PreparedFlatScene,
    runtime: &ColorRuntime,
) -> Result<ShaderFlatScene> {
    let mut surfaces = Vec::with_capacity(prepared.surfaces.len());
    for (id, design) in &prepared.surfaces {
        surfaces.push((id.clone(), bind_user_color_scheme(design, runtime)?));
    }
    Ok(ShaderFlatScene { surfaces })
}

/// Stable evidence for comparing composition layout across user schemes.
pub fn flat_scene_layout_identity(scene: &ShaderFlatScene) -> String {
    scene
        .surfaces
        .iter()
        .map(|(id, output)| format!("{id}={}", output.layout.identity))
        .collect::<Vec<_>>()
        .join("|")
}

#[cfg(test)]
mod tests {
    use super::language::*;
    use super::*;
    use std::collections::BTreeMap;

    fn scene() -> FlatScene {
        let sample = |name: &str, y: f32| Sample {
            semantic: Semantic::new(name, "surface", "rest"),
            kind: BoxKind::RoundBox,
            bounds: Bounds { x: 0.1, y, width: 0.5, height: 0.1 },
            decoration: Decoration { stroke_width: 0.0 },
            color: ColorIntent::new("surface", "outline"),
            corner_radius: 0.2,
        };
        FlatScene {
            surfaces: vec![
                ("panel".to_string(), sample("Dialog", 0.1)),
                ("row".to_string(), sample("ListItem", 0.3)),
            ],
        }
    }

    fn runtime(id: &str) -> ColorRuntime {
        let mut colors = BTreeMap::new();
        colors.insert("surface".to_string(), rgba("#12141c", 0.9));
        colors.insert("outline".to_string(), rgba("#3b6ea5", 1.0));
        let mut other = colors.clone();
        other.insert("surface".to_string(), rgba("#ffffff", 0.9));
        ColorRuntime {
            scheme_id: id.to_string(),
            schemes: vec![
                ColorScheme { id: "dark".into(), colors },
                ColorScheme { id: "light".into(), colors: other },
            ],
        }
    }

    #[test]
    fn the_layout_identity_survives_a_scheme_change() {
        let prepared = prepare_flat_scene(&scene()).unwrap();
        let dark = bind_flat_scene_user_color_scheme(&prepared, &runtime("dark")).unwrap();
        let light = bind_flat_scene_user_color_scheme(&prepared, &runtime("light")).unwrap();
        assert_eq!(flat_scene_layout_identity(&dark), flat_scene_layout_identity(&light));
        assert_ne!(dark.surfaces[0].1.color, light.surfaces[0].1.color);
    }

    #[test]
    fn preparation_keeps_painter_order() {
        let prepared = prepare_flat_scene(&scene()).unwrap();
        assert_eq!(prepared.surfaces[0].0, "panel");
        assert_eq!(prepared.surfaces[1].0, "row");
    }
}
