//! A GLSL shader adapter for root-ui flat scenes.
//!
//! root-ui ships adapters for WebGL2 and WebGPU; this is the one for the host's
//! own OpenGL context, and it is where `pin target_shading_language` and
//! `pin navigation_surface_renderer` meet: the surface is a signed-distance
//! rounded box evaluated in GLSL by us, over the grid, in the same window.
//!
//! It is a second program rather than an addition to the grid's shader. The
//! grid renderer is the measured candidate's and stays untouched; a rounded
//! corner materialized in physical pixels is not something a cell-quad shader
//! can be asked for without changing what that artefact is.

use crate::text::{Atlas, ATLAS};

use super::language::{materialize_pixel_box_geometry, BoxKind};
use super::ShaderFlatScene;

/// pos.xy, rect centre.xy, half extents.xy, (radius, stroke, softness),
/// fill.rgba, stroke.rgba, uv.xy, mode.
pub const VERTEX_FLOATS: usize = 20;

const VS: &str = r#"#version 330 core
layout (location = 0) in vec2 a_pos;
layout (location = 1) in vec2 a_center;
layout (location = 2) in vec2 a_half;
layout (location = 3) in vec3 a_params;
layout (location = 4) in vec4 a_fill;
layout (location = 5) in vec4 a_edge;
layout (location = 6) in vec2 a_uv;
layout (location = 7) in float a_mode;
uniform vec2 u_screen;
out vec2 v_local;
out vec2 v_half;
out vec3 v_params;
out vec4 v_fill;
out vec4 v_edge;
out vec2 v_uv;
out float v_mode;
void main() {
    vec2 ndc = vec2(a_pos.x / u_screen.x * 2.0 - 1.0,
                    1.0 - a_pos.y / u_screen.y * 2.0);
    gl_Position = vec4(ndc, 0.0, 1.0);
    v_local = a_pos - a_center;
    v_half = a_half;
    v_params = a_params;
    v_fill = a_fill;
    v_edge = a_edge;
    v_uv = a_uv;
    v_mode = a_mode;
}
"#;

// Both branches are evaluated before the mode is selected, so that `fwidth` is
// never reached through non-uniform control flow, where its result is undefined.
const FS: &str = r#"#version 330 core
in vec2 v_local;
in vec2 v_half;
in vec3 v_params;
in vec4 v_fill;
in vec4 v_edge;
in vec2 v_uv;
in float v_mode;
out vec4 frag;
uniform sampler2D u_atlas;

float sd_round_box(vec2 p, vec2 b, float r) {
    vec2 q = abs(p) - b + vec2(r);
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2(0.0))) - r;
}

void main() {
    float d = sd_round_box(v_local, v_half, v_params.x);
    // Softness widens the edge instead of adding a second shape: a shadow is
    // the same rounded-box field with a longer fade, so it cannot drift out of
    // agreement with the box it belongs to.
    float aa = max(max(fwidth(d), 1e-4), v_params.z);
    float coverage = 1.0 - smoothstep(-aa, aa, d);
    float stroke = v_params.y;
    float ring = stroke > 0.0
        ? 1.0 - smoothstep(-aa, aa, abs(d + stroke * 0.5) - stroke * 0.5)
        : 0.0;
    vec4 shape = mix(vec4(v_fill.rgb, v_fill.a * coverage),
                     vec4(v_edge.rgb, v_edge.a * ring), ring);
    float ink = texture(u_atlas, v_uv).r;
    frag = v_mode > 0.5 ? vec4(v_fill.rgb, v_fill.a * ink) : shape;
}
"#;

pub struct Adapter {
    program: glow::Program,
    vao: glow::VertexArray,
    vbo: glow::Buffer,
    texture: glow::Texture,
    uploaded: usize,
    verts: Vec<f32>,
    u_screen: Option<glow::UniformLocation>,
    u_atlas: Option<glow::UniformLocation>,
}

/// What one adapter pass emitted. Counts, not judgements — the same discipline
/// `evaluation/free-surface` kept.
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct AdapterStats {
    pub surfaces: usize,
    pub round_boxes: usize,
    pub shadows: usize,
    pub glyph_quads: usize,
    /// True when at least one surface origin is not on a cell boundary, which
    /// is the observable difference between this and a grid-addressed window.
    pub origin_off_grid: bool,
}

impl Adapter {
    pub fn new(gl: &glow::Context) -> Self {
        use glow::HasContext;
        unsafe {
            let program = link(gl, VS, FS);
            let vao = gl.create_vertex_array().unwrap();
            gl.bind_vertex_array(Some(vao));
            let vbo = gl.create_buffer().unwrap();
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
            let stride = (VERTEX_FLOATS * 4) as i32;
            for (index, size, offset) in
                [(0, 2, 0), (1, 2, 8), (2, 2, 16), (3, 3, 24), (4, 4, 36), (5, 4, 52), (6, 2, 68), (7, 1, 76)]
            {
                gl.enable_vertex_attrib_array(index);
                gl.vertex_attrib_pointer_f32(index, size, glow::FLOAT, false, stride, offset);
            }

            let texture = gl.create_texture().unwrap();
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);

            Self {
                program,
                vao,
                vbo,
                texture,
                // No rasterisation count can equal this, so the first draw
                // always uploads.
                uploaded: usize::MAX,
                verts: Vec::new(),
                u_screen: gl.get_uniform_location(program, "u_screen"),
                u_atlas: gl.get_uniform_location(program, "u_atlas"),
            }
        }
    }

    pub fn begin(&mut self) {
        self.verts.clear();
    }

    pub fn quads(&self) -> usize {
        self.verts.len() / (VERTEX_FLOATS * 6)
    }

    /// Push a resolved root-ui scene, materialised for this target.
    pub fn push_scene(
        &mut self,
        scene: &ShaderFlatScene,
        target_w: f32,
        target_h: f32,
        cell_h: f32,
        scale: f32,
    ) -> AdapterStats {
        let mut stats = AdapterStats::default();
        for (_, output) in &scene.surfaces {
            let Ok(geometry) = materialize_pixel_box_geometry(
                &output.layout,
                &output.decoration,
                target_w,
                target_h,
                scale,
            ) else {
                continue;
            };
            stats.surfaces += 1;
            if geometry.kind == BoxKind::RoundBox {
                stats.round_boxes += 1;
            }
            if cell_h > 0.0 && (geometry.y / cell_h).fract().abs() > 1e-4 {
                stats.origin_off_grid = true;
            }
            // Painter order within one surface: the shadow belongs behind its
            // own box, not behind the scene.
            if let (Some(shadow), Some(color)) = (geometry.shadow, output.color.shadow) {
                self.push_soft_box(
                    geometry.x + shadow.x,
                    geometry.y + shadow.y,
                    shadow.width,
                    shadow.height,
                    shadow.corner_radius_x,
                    0.0,
                    shadow.blur,
                    color,
                    color,
                );
                stats.shadows += 1;
            }
            self.push_box(
                geometry.x,
                geometry.y,
                geometry.width,
                geometry.height,
                geometry.corner_radius_x,
                geometry.stroke_width,
                output.color.fill,
                output.color.stroke,
            );
        }
        stats
    }

    #[allow(clippy::too_many_arguments)]
    pub fn push_box(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radius: f32,
        stroke: f32,
        fill: [f32; 4],
        edge: [f32; 4],
    ) {
        self.push_soft_box(x, y, w, h, radius, stroke, 0.0, fill, edge);
    }

    /// A box whose edge fades over `softness` pixels instead of one.
    #[allow(clippy::too_many_arguments)]
    pub fn push_soft_box(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radius: f32,
        stroke: f32,
        softness: f32,
        fill: [f32; 4],
        edge: [f32; 4],
    ) {
        // Enough bleed for the fade to land in. One pixel is right for an
        // antialiased edge; a shadow needs its whole blur, or the quad clips
        // the softest half of its own edge.
        let bleed = softness.max(1.0);
        let (x, y, w, h) = (x - bleed, y - bleed, w + bleed * 2.0, h + bleed * 2.0);
        let centre = (x + w / 2.0, y + h / 2.0);
        let half = (w / 2.0 - bleed, h / 2.0 - bleed);
        let corners = [
            (x, y),
            (x + w, y),
            (x + w, y + h),
            (x, y),
            (x + w, y + h),
            (x, y + h),
        ];
        for (px, py) in corners {
            self.vertex(
                px,
                py,
                centre,
                half,
                radius,
                stroke,
                softness,
                fill,
                edge,
                (0.0, 0.0),
                0.0,
            );
        }
    }

    /// A run of text at a baseline, in the surface's own pixels.
    pub fn push_text(
        &mut self,
        atlas: &mut Atlas,
        x: f32,
        baseline: f32,
        text: &str,
        color: [f32; 4],
        max_x: f32,
    ) -> usize {
        let mut pen = x;
        let mut drawn = 0;
        for ch in text.chars() {
            let advance = atlas.cell_w * crate::proto::paint::char_width(ch) as f32;
            if pen + advance > max_x {
                break;
            }
            if let Some(glyph) = atlas.styled_glyph(ch, false, false) {
                let gx = pen + glyph.bearing_x;
                let gy = baseline - glyph.bearing_y;
                let quad = [
                    (gx, gy, glyph.u0, glyph.v0),
                    (gx + glyph.w, gy, glyph.u1, glyph.v0),
                    (gx + glyph.w, gy + glyph.h, glyph.u1, glyph.v1),
                    (gx, gy, glyph.u0, glyph.v0),
                    (gx + glyph.w, gy + glyph.h, glyph.u1, glyph.v1),
                    (gx, gy + glyph.h, glyph.u0, glyph.v1),
                ];
                for (px, py, u, v) in quad {
                    self.vertex(
                        px,
                        py,
                        (0.0, 0.0),
                        (0.0, 0.0),
                        0.0,
                        0.0,
                        0.0,
                        color,
                        color,
                        (u, v),
                        1.0,
                    );
                }
                drawn += 1;
            }
            pen += advance;
        }
        drawn
    }

    #[allow(clippy::too_many_arguments)]
    fn vertex(
        &mut self,
        x: f32,
        y: f32,
        centre: (f32, f32),
        half: (f32, f32),
        radius: f32,
        stroke: f32,
        softness: f32,
        fill: [f32; 4],
        edge: [f32; 4],
        uv: (f32, f32),
        mode: f32,
    ) {
        self.verts.extend_from_slice(&[
            x, y, centre.0, centre.1, half.0, half.1, radius, stroke, softness, fill[0], fill[1],
            fill[2], fill[3], edge[0], edge[1], edge[2], edge[3], uv.0, uv.1, mode,
        ]);
    }

    pub fn draw(&mut self, gl: &glow::Context, atlas: &mut Atlas, width: i32, height: i32) {
        use glow::HasContext;
        if self.verts.is_empty() {
            return;
        }
        unsafe {
            gl.use_program(Some(self.program));
            gl.bind_vertex_array(Some(self.vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo));
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.texture));
            // Keyed on the rasterisation count, not on `atlas.dirty`: the grid
            // renderer draws first and clears that flag, so a shared flag would
            // leave this texture one upload behind every new glyph.
            if self.uploaded != atlas.stats.rasterizations as usize {
                gl.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    glow::R8 as i32,
                    ATLAS as i32,
                    ATLAS as i32,
                    0,
                    glow::RED,
                    glow::UNSIGNED_BYTE,
                    Some(atlas.pixels.as_slice()),
                );
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_SWIZZLE_R,
                    glow::RED as i32,
                );
                self.uploaded = atlas.stats.rasterizations as usize;
            }
            gl.uniform_1_i32(self.u_atlas.as_ref(), 0);
            gl.uniform_2_f32(self.u_screen.as_ref(), width as f32, height as f32);
            gl.enable(glow::BLEND);
            gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
            let bytes: &[u8] = std::slice::from_raw_parts(
                self.verts.as_ptr() as *const u8,
                std::mem::size_of_val(&self.verts[..]),
            );
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, glow::DYNAMIC_DRAW);
            gl.draw_arrays(glow::TRIANGLES, 0, (self.verts.len() / VERTEX_FLOATS) as i32);
        }
    }
}

fn link(gl: &glow::Context, vs: &str, fs: &str) -> glow::Program {
    use glow::HasContext;
    unsafe {
        let program = gl.create_program().expect("root-ui program");
        for (kind, source) in [(glow::VERTEX_SHADER, vs), (glow::FRAGMENT_SHADER, fs)] {
            let shader = gl.create_shader(kind).expect("root-ui shader");
            gl.shader_source(shader, source);
            gl.compile_shader(shader);
            assert!(
                gl.get_shader_compile_status(shader),
                "root-ui shader failed to compile: {}",
                gl.get_shader_info_log(shader)
            );
            gl.attach_shader(program, shader);
            gl.delete_shader(shader);
        }
        gl.link_program(program);
        assert!(
            gl.get_program_link_status(program),
            "root-ui program failed to link: {}",
            gl.get_program_info_log(program)
        );
        program
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_vertex_stride_matches_the_attribute_layout() {
        // pos2 + centre2 + half2 + params2 + fill4 + edge4 + uv2 + mode1
        assert_eq!(VERTEX_FLOATS, 2 + 2 + 2 + 3 + 4 + 4 + 2 + 1);
    }

    #[test]
    fn the_fragment_shader_takes_its_derivative_in_uniform_control_flow() {
        // `fwidth` after a branch is undefined, and the failure is a driver-
        // dependent halo rather than a compile error, so it is checked here.
        let body = FS;
        let derivative = body.find("fwidth").expect("fwidth present");
        let branch = body.find("v_mode > 0.5").expect("mode select present");
        assert!(derivative < branch, "the derivative must precede the mode branch");
    }
}
