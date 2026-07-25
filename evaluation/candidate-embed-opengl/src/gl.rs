//! The GLSL renderer. Every visible pixel — background, glyph and cursor —
//! goes through this one shader pair as a textured quad.

use glow::HasContext;

use crate::grid::Grid;
use crate::text::{Atlas, ATLAS};

pub const VS: &str = r#"#version 330 core
layout (location = 0) in vec2 a_pos;
layout (location = 1) in vec2 a_uv;
layout (location = 2) in vec4 a_col;
uniform vec2 u_screen;
out vec2 v_uv;
out vec4 v_col;
void main() {
    vec2 ndc = vec2(a_pos.x / u_screen.x * 2.0 - 1.0,
                    1.0 - a_pos.y / u_screen.y * 2.0);
    gl_Position = vec4(ndc, 0.0, 1.0);
    v_uv = a_uv;
    v_col = a_col;
}
"#;

pub const FS: &str = r#"#version 330 core
in vec2 v_uv;
in vec4 v_col;
out vec4 frag;
uniform sampler2D u_atlas;
uniform int u_mode;
void main() {
    vec4 t = texture(u_atlas, v_uv);
    if (u_mode == 1) {
        // Arbitrary RGBA content — an image is just another textured quad in the
        // same scene as the text.
        frag = t * v_col;
    } else {
        // Background quads sample a forced-opaque texel, so coverage is 1 there
        // and antialiased glyph coverage elsewhere. One shader covers both.
        frag = vec4(v_col.rgb, v_col.a * t.r);
    }
}
"#;

pub struct Renderer {
    program: glow::Program,
    vao: glow::VertexArray,
    vbo: glow::Buffer,
    tex: glow::Texture,
    verts: Vec<f32>,
    u_screen: Option<glow::UniformLocation>,
    u_atlas: Option<glow::UniformLocation>,
    u_mode: Option<glow::UniformLocation>,
}

/// An arbitrary RGBA surface placed over the text grid, in pixels.
pub struct Image {
    pub tex: glow::Texture,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Renderer {
    pub fn new(gl: &glow::Context) -> Self {
        unsafe {
            let program = link(gl, VS, FS);
            let vao = gl.create_vertex_array().unwrap();
            gl.bind_vertex_array(Some(vao));
            let vbo = gl.create_buffer().unwrap();
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
            let stride = 8 * 4;
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, stride, 0);
            gl.enable_vertex_attrib_array(1);
            gl.vertex_attrib_pointer_f32(1, 2, glow::FLOAT, false, stride, 8);
            gl.enable_vertex_attrib_array(2);
            gl.vertex_attrib_pointer_f32(2, 4, glow::FLOAT, false, stride, 16);

            let tex = gl.create_texture().unwrap();
            gl.bind_texture(glow::TEXTURE_2D, Some(tex));
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);

            let u_screen = gl.get_uniform_location(program, "u_screen");
            let u_atlas = gl.get_uniform_location(program, "u_atlas");
            let u_mode = gl.get_uniform_location(program, "u_mode");

            gl.enable(glow::BLEND);
            gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);

            Self { program, vao, vbo, tex, verts: Vec::new(), u_screen, u_atlas, u_mode }
        }
    }

    pub fn build(&mut self, grid: &Grid, atlas: &mut Atlas, preedit: &str) {
        self.verts.clear();
        build_scene(&mut self.verts, grid, atlas, preedit);
    }

    /// Upload arbitrary RGBA pixels as a texture usable by `draw`.
    pub fn upload_rgba(gl: &glow::Context, rgba: &[u8], w: u32, h: u32) -> glow::Texture {
        unsafe {
            let tex = gl.create_texture().unwrap();
            gl.bind_texture(glow::TEXTURE_2D, Some(tex));
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
            gl.tex_image_2d(
                glow::TEXTURE_2D, 0, glow::RGBA8 as i32, w as i32, h as i32, 0,
                glow::RGBA, glow::UNSIGNED_BYTE, Some(rgba),
            );
            tex
        }
    }

    pub fn draw(
        &mut self,
        gl: &glow::Context,
        atlas: &mut Atlas,
        px_w: i32,
        px_h: i32,
        images: &[Image],
    ) {
        unsafe {
            gl.viewport(0, 0, px_w, px_h);
            gl.clear_color(0.0, 0.0, 0.0, 1.0);
            gl.clear(glow::COLOR_BUFFER_BIT);

            gl.bind_texture(glow::TEXTURE_2D, Some(self.tex));
            if atlas.dirty {
                gl.tex_image_2d(
                    glow::TEXTURE_2D, 0, glow::R8 as i32,
                    ATLAS as i32, ATLAS as i32, 0,
                    glow::RED, glow::UNSIGNED_BYTE, Some(&atlas.pixels),
                );
                atlas.dirty = false;
            }

            gl.use_program(Some(self.program));
            gl.uniform_2_f32(self.u_screen.as_ref(), px_w as f32, px_h as f32);
            gl.uniform_1_i32(self.u_atlas.as_ref(), 0);

            gl.bind_vertex_array(Some(self.vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo));
            gl.buffer_data_u8_slice(
                glow::ARRAY_BUFFER,
                core::slice::from_raw_parts(
                    self.verts.as_ptr() as *const u8,
                    self.verts.len() * 4,
                ),
                glow::STREAM_DRAW,
            );
            gl.uniform_1_i32(self.u_mode.as_ref(), 0);
            gl.draw_arrays(glow::TRIANGLES, 0, (self.verts.len() / 8) as i32);

            // Images live in the same scene, drawn over the text with the same
            // shader and the same coordinate system. Nothing about the grid has
            // to change for them to exist.
            if !images.is_empty() {
                gl.uniform_1_i32(self.u_mode.as_ref(), 1);
                for im in images {
                    gl.bind_texture(glow::TEXTURE_2D, Some(im.tex));
                    let q = image_quad(im);
                    gl.buffer_data_u8_slice(
                        glow::ARRAY_BUFFER,
                        core::slice::from_raw_parts(q.as_ptr() as *const u8, q.len() * 4),
                        glow::STREAM_DRAW,
                    );
                    gl.draw_arrays(glow::TRIANGLES, 0, 6);
                }
            }
        }
    }
}

fn push_quad(
    verts: &mut Vec<f32>,
    x: f32, y: f32, w: f32, h: f32,
    uv: (f32, f32, f32, f32),
    col: [f32; 4],
) {
    let (u0, v0, u1, v1) = uv;
    let c = col;
    let quad = [
        (x, y, u0, v0), (x + w, y, u1, v0), (x + w, y + h, u1, v1),
        (x, y, u0, v0), (x + w, y + h, u1, v1), (x, y + h, u0, v1),
    ];
    for (px, py, pu, pv) in quad {
        verts.extend_from_slice(&[px, py, pu, pv, c[0], c[1], c[2], c[3]]);
    }
}

/// Build the whole grid scene (backgrounds, glyphs, text decorations and the
/// IME preedit) into `verts`. Free of any GL handle so it is unit-testable on
/// the host with only a font-backed [`Atlas`], no GPU context.
pub fn build_scene(verts: &mut Vec<f32>, grid: &Grid, atlas: &mut Atlas, preedit: &str) {
    let (cw, ch) = (atlas.cell_w, atlas.cell_h);
    let (wu, wv) = atlas.white_uv();
    let white = (wu, wv, wu, wv);

    // Pass 1: every background, including the cursor block, so glyphs drawn
    // afterwards always land on top of their own cell's background.
    for row in 0..grid.rows {
        for col in 0..grid.cols {
            let cell = grid.cell(row, col);
            let (fg, bg) = grid.colors(cell.hl);
            let is_cursor = grid.cursor == (row, col);
            let paint = if is_cursor { fg } else { bg };
            push_quad(verts, col as f32 * cw, row as f32 * ch, cw, ch, white, rgb(paint, 1.0));
        }
    }

    // Pass 2: glyphs, now rasterised with the cell's synthetic bold/italic face.
    for row in 0..grid.rows {
        for col in 0..grid.cols {
            let cell = grid.cell(row, col);
            let (fg, bg) = grid.colors(cell.hl);
            let st = grid.style(cell.hl);
            let is_cursor = grid.cursor == (row, col);
            let ink = if is_cursor { bg } else { fg };
            let Some(g) = atlas.styled_glyph(cell.ch, st.bold, st.italic) else { continue };
            let x = col as f32 * cw + g.bearing_x;
            let y = row as f32 * ch + atlas.ascent - g.bearing_y;
            push_quad(verts, x, y, g.w, g.h, (g.u0, g.v0, g.u1, g.v1), rgb(ink, 1.0));
        }
    }

    // Pass 2.5: underline family and strikethrough. Drawn after glyphs so the
    // decoration reads on top of the ink, in the highlight's `sp` colour (or the
    // foreground when nvim gave no special colour).
    for row in 0..grid.rows {
        for col in 0..grid.cols {
            let cell = grid.cell(row, col);
            let st = grid.style(cell.hl);
            if !st.any_underline() && !st.strikethrough {
                continue;
            }
            // A decoration follows the glyph ink: nvim's `sp` when it gave one,
            // otherwise whatever colour the glyph itself was drawn in. The cursor
            // cell inverts that ink, so its decoration inverts with it and stays
            // visible against the block instead of vanishing into it.
            let color = if grid.cursor == (row, col) {
                st.special.unwrap_or_else(|| grid.colors(cell.hl).1)
            } else {
                grid.decoration_color(cell.hl)
            };
            let color = rgb(color, 1.0);
            push_cell_decorations(
                verts, white, color, st,
                col as f32 * cw, row as f32 * ch, cw, ch,
                atlas.ascent, atlas.x_height,
            );
        }
    }

    // Pass 3: the IME composition. It is not in any Neovim buffer yet, so it
    // is drawn inverted to read as "pending" rather than as text.
    if !preedit.is_empty() {
        let (row, col) = grid.cursor;
        let (fg, bg) = grid.colors(grid.cell(row, col).hl);
        let y = row as f32 * ch;
        let advance = |c: char| if (c as u32) < 0x2500 { cw } else { cw * 2.0 };

        let mut x = col as f32 * cw;
        for c in preedit.chars() {
            push_quad(verts, x, y, advance(c), ch, white, rgb(fg, 1.0));
            x += advance(c);
        }
        let mut x = col as f32 * cw;
        for c in preedit.chars() {
            if let Some(g) = atlas.glyph(c) {
                push_quad(
                    verts,
                    x + g.bearing_x, y + atlas.ascent - g.bearing_y,
                    g.w, g.h, (g.u0, g.v0, g.u1, g.v1), rgb(bg, 1.0),
                );
            }
            x += advance(c);
        }
    }
}

/// Emit the decoration quads for one cell: the underline family near the
/// baseline and strikethrough across the x-height.
///
/// Neovim's five underline styles are all drawn from the same quad primitive as
/// the rest of the scene, so nothing here needs a second shader:
///
/// * `underline`     — one solid rule on the baseline
/// * `underdouble`   — two solid rules straddling it
/// * `underdotted`   — square dots, one thickness wide
/// * `underdashed`   — longer dashes, three thicknesses wide
/// * `undercurl`     — a sampled sine, one short quad per step
///
/// The dash and curl phases are taken from the *absolute* x, so a run of cells
/// sharing a highlight reads as one continuous pattern rather than restarting at
/// every cell boundary. Every quad is clipped to `x0..x0+cw` and clamped to
/// `y0..y0+ch`, so a decoration never bleeds into a neighbouring cell.
#[allow(clippy::too_many_arguments)]
fn push_cell_decorations(
    verts: &mut Vec<f32>,
    white: (f32, f32, f32, f32),
    color: [f32; 4],
    st: crate::grid::Hl,
    x0: f32, y0: f32, cw: f32, ch: f32, ascent: f32, x_height: f32,
) {
    let t = (ch / 14.0).round().max(1.0); // decoration thickness in px
    let baseline = y0 + (ascent + 1.0).min(ch - t);
    // Keep a rule needing `above` px of headroom and `below` px of footroom
    // inside the cell. `.min().max()` rather than `clamp` because a very short
    // cell can invert the bounds, and `f32::clamp` panics on that.
    let fit = |y: f32, above: f32, below: f32| y.min(y0 + ch - below).max(y0 + above);

    let solid = |verts: &mut Vec<f32>, y: f32, th: f32| {
        push_quad(verts, x0, y, cw, th, white, color);
    };
    // Emit `seg`-long marks every `seg + gap`, phase-locked to absolute x and
    // clipped to this cell.
    let segmented = |verts: &mut Vec<f32>, y: f32, seg: f32, gap: f32| {
        let period = seg + gap;
        let mut x = x0 - x0 % period;
        while x < x0 + cw {
            let (lo, hi) = (x.max(x0), (x + seg).min(x0 + cw));
            if hi > lo {
                push_quad(verts, lo, y, hi - lo, t, white, color);
            }
            x += period;
        }
    };
    // A sine sampled into short quads. Amplitude is one thickness either way, so
    // the wave occupies `2 * amp + t` px vertically.
    let curl = |verts: &mut Vec<f32>, y: f32, amp: f32| {
        let period = (6.0 * t).max(4.0);
        let step = (period / 8.0).max(1.0);
        let mut x = x0;
        while x < x0 + cw {
            let w = step.min(x0 + cw - x);
            let phase = (x + w * 0.5) / period * std::f32::consts::TAU;
            push_quad(verts, x, y + amp * phase.sin(), w, t, white, color);
            x += step;
        }
    };

    if st.underdotted {
        segmented(verts, fit(baseline, 0.0, t), t, t);
    } else if st.underdashed {
        segmented(verts, fit(baseline, 0.0, t), 3.0 * t, 2.0 * t);
    } else if st.underdouble {
        let y = fit(baseline, t, 2.0 * t);
        solid(verts, y - t, t);
        solid(verts, y + t, t);
    } else if st.undercurl {
        let amp = t;
        curl(verts, fit(baseline, amp, t + amp), amp);
    } else if st.underline {
        solid(verts, fit(baseline, 0.0, t), t);
    }

    if st.strikethrough {
        // Centred on the x-height so the rule crosses the body of lowercase text
        // rather than riding above it.
        solid(verts, fit(y0 + ascent - x_height * 0.5 - t * 0.5, 0.0, t), t);
    }
}

fn image_quad(im: &Image) -> [f32; 48] {
    let (x, y, w, h) = (im.x, im.y, im.w, im.h);
    let mut v = [0f32; 48];
    let corners = [
        (x, y, 0.0, 0.0), (x + w, y, 1.0, 0.0), (x + w, y + h, 1.0, 1.0),
        (x, y, 0.0, 0.0), (x + w, y + h, 1.0, 1.0), (x, y + h, 0.0, 1.0),
    ];
    for (i, (px, py, u, vv)) in corners.into_iter().enumerate() {
        v[i * 8..i * 8 + 8].copy_from_slice(&[px, py, u, vv, 1.0, 1.0, 1.0, 1.0]);
    }
    v
}

fn rgb(c: u32, a: f32) -> [f32; 4] {
    [
        ((c >> 16) & 0xff) as f32 / 255.0,
        ((c >> 8) & 0xff) as f32 / 255.0,
        (c & 0xff) as f32 / 255.0,
        a,
    ]
}

unsafe fn link(gl: &glow::Context, vs: &str, fs: &str) -> glow::Program {
    let program = gl.create_program().unwrap();
    for (kind, src) in [(glow::VERTEX_SHADER, vs), (glow::FRAGMENT_SHADER, fs)] {
        let sh = gl.create_shader(kind).unwrap();
        gl.shader_source(sh, src);
        gl.compile_shader(sh);
        assert!(
            gl.get_shader_compile_status(sh),
            "shader compile failed: {}",
            gl.get_shader_info_log(sh)
        );
        gl.attach_shader(program, sh);
        gl.delete_shader(sh);
    }
    gl.link_program(program);
    assert!(
        gl.get_program_link_status(program),
        "link failed: {}",
        gl.get_program_info_log(program)
    );
    program
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::{Cell, Grid, Hl};
    use crate::text::Atlas;

    const QUAD_FLOATS: usize = 48; // 6 verts * 8 floats

    /// One emitted quad, recovered from the vertex buffer. Vertex 0 is the
    /// top-left corner and vertex 2 the bottom-right, so the box and its colour
    /// read straight back out.
    #[derive(Clone, Copy, Debug)]
    struct Quad {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        col: [f32; 4],
    }

    fn quads(verts: &[f32]) -> Vec<Quad> {
        verts
            .chunks_exact(QUAD_FLOATS)
            .map(|q| Quad {
                x: q[0],
                y: q[1],
                w: q[16] - q[0],
                h: q[17] - q[1],
                col: [q[4], q[5], q[6], q[7]],
            })
            .collect()
    }

    /// A 2x1 grid whose left cell is `x` styled by `hl`, rendered with the
    /// cursor at `cursor`. Returns the vertex buffer.
    fn scene_with(hl: Hl, cursor: (usize, usize)) -> Vec<f32> {
        let mut atlas = Atlas::new(20.0);
        let mut grid = Grid::new(2, 1);
        grid.cursor = cursor;
        grid.hls.insert(1, hl);
        grid.cells[0] = Cell { ch: 'x', hl: 1 };
        let mut verts = Vec::new();
        build_scene(&mut verts, &grid, &mut atlas, "");
        verts
    }

    /// The same scene with the cursor parked off-grid, so no cell flips to
    /// cursor colours.
    fn scene(hl: Hl) -> Vec<f32> {
        scene_with(hl, (0, 5))
    }

    fn scene_floats(hl: Hl) -> usize {
        scene(hl).len()
    }

    /// Pass 1 emits one background quad per cell (2) and pass 2 the glyph for
    /// `x` (1); everything after that is decoration.
    const UNDECORATED_QUADS: usize = 3;

    fn decorations(hl: Hl) -> Vec<Quad> {
        quads(&scene(hl))[UNDECORATED_QUADS..].to_vec()
    }

    /// Cell metrics for the grid the helpers above build:
    /// `(cell_w, cell_h, ascent, x_height)`.
    fn metrics() -> (f32, f32, f32, f32) {
        let a = Atlas::new(20.0);
        (a.cell_w, a.cell_h, a.ascent, a.x_height)
    }

    fn plain() -> Hl {
        Hl::default()
    }
    fn with(f: impl FnOnce(&mut Hl)) -> Hl {
        let mut h = Hl::default();
        f(&mut h);
        h
    }

    #[test]
    fn plain_cell_emits_no_decoration_quads() {
        // Backgrounds (2) + one glyph for 'x' = 3 quads. No decoration.
        assert_eq!(scene_floats(plain()), 3 * QUAD_FLOATS);
    }

    #[test]
    fn underline_adds_exactly_one_decoration_quad() {
        let d = scene_floats(with(|h| h.underline = true)) - scene_floats(plain());
        assert_eq!(d, QUAD_FLOATS);
    }

    #[test]
    fn strikethrough_adds_exactly_one_decoration_quad() {
        let d = scene_floats(with(|h| h.strikethrough = true)) - scene_floats(plain());
        assert_eq!(d, QUAD_FLOATS);
    }

    #[test]
    fn underdouble_adds_two_lines() {
        let d = scene_floats(with(|h| h.underdouble = true)) - scene_floats(plain());
        assert_eq!(d, 2 * QUAD_FLOATS);
    }

    #[test]
    fn underline_and_strikethrough_stack() {
        let d = scene_floats(with(|h| {
            h.underline = true;
            h.strikethrough = true;
        })) - scene_floats(plain());
        assert_eq!(d, 2 * QUAD_FLOATS);
    }

    #[test]
    fn dotted_underline_is_segmented_into_more_than_one_quad() {
        let dotted = scene_floats(with(|h| h.underdotted = true)) - scene_floats(plain());
        assert!(dotted > QUAD_FLOATS, "dotted should be multiple segments, got {dotted} floats");
    }

    #[test]
    fn dashed_marks_are_longer_and_sparser_than_dotted_ones() {
        let dotted = decorations(with(|h| h.underdotted = true));
        let dashed = decorations(with(|h| h.underdashed = true));
        assert!(dashed.len() > 1, "dashed should be segmented, got {}", dashed.len());
        assert!(
            dashed.len() < dotted.len(),
            "dashed ({}) should be sparser than dotted ({})",
            dashed.len(),
            dotted.len()
        );
        // A dash is longer than a dot, which is what distinguishes the two
        // styles on screen. Compared at full length: a mark straddling the cell
        // edge is clipped here and resumes in the next cell.
        let longest = |qs: &[Quad]| qs.iter().map(|q| q.w).fold(0.0f32, f32::max);
        assert!(longest(&dashed) > longest(&dotted));
    }

    #[test]
    fn undercurl_is_a_wave_not_a_flat_rule() {
        let curl = decorations(with(|h| h.undercurl = true));
        assert!(curl.len() > 2, "curl should be sampled into steps, got {}", curl.len());
        let top = curl.iter().map(|q| q.y).fold(f32::MAX, f32::min);
        let bottom = curl.iter().map(|q| q.y).fold(f32::MIN, f32::max);
        assert!(bottom - top > 0.0, "curl steps must vary in y to read as a wave");
        // A plain underline, by contrast, is one flat quad.
        assert_eq!(decorations(with(|h| h.underline = true)).len(), 1);
    }

    #[test]
    fn every_decoration_stays_inside_its_cell() {
        let (cw, ch, _, _) = metrics();
        for (name, hl) in [
            ("underline", with(|h| h.underline = true)),
            ("undercurl", with(|h| h.undercurl = true)),
            ("underdouble", with(|h| h.underdouble = true)),
            ("underdotted", with(|h| h.underdotted = true)),
            ("underdashed", with(|h| h.underdashed = true)),
            ("strikethrough", with(|h| h.strikethrough = true)),
        ] {
            for q in decorations(hl) {
                assert!(q.y >= 0.0 && q.y + q.h <= ch, "{name} escapes the cell vertically: {q:?}");
                assert!(q.x >= 0.0 && q.x + q.w <= cw, "{name} escapes the cell horizontally: {q:?}");
            }
        }
    }

    #[test]
    fn strikethrough_crosses_the_x_height_band() {
        let (_, _, ascent, x_height) = metrics();
        let d = decorations(with(|h| h.strikethrough = true));
        assert_eq!(d.len(), 1);
        let rule = d[0];
        // The band runs from the top of a lowercase `x` down to the baseline.
        assert!(
            rule.y > ascent - x_height && rule.y + rule.h < ascent,
            "strikethrough at {}..{} should lie inside the x-height band {}..{}",
            rule.y,
            rule.y + rule.h,
            ascent - x_height,
            ascent
        );
    }

    #[test]
    fn decoration_uses_the_special_colour_when_nvim_gave_one() {
        let hl = with(|h| {
            h.underline = true;
            h.fg = Some(0x00ff00);
            h.special = Some(0xff0000);
        });
        for q in decorations(hl) {
            assert_eq!(q.col, [1.0, 0.0, 0.0, 1.0], "underline should be drawn in `sp`");
        }
    }

    #[test]
    fn decoration_falls_back_to_the_foreground_without_a_special_colour() {
        let hl = with(|h| {
            h.underline = true;
            h.fg = Some(0xff0000);
        });
        for q in decorations(hl) {
            assert_eq!(q.col, [1.0, 0.0, 0.0, 1.0]);
        }
    }

    #[test]
    fn cursor_cell_decoration_inverts_with_the_glyph_ink() {
        // On the cursor the block is painted in fg and the glyph in bg, so an
        // underline without `sp` must follow the glyph into bg or it disappears.
        let hl = with(|h| {
            h.underline = true;
            h.fg = Some(0xff0000);
            h.bg = Some(0x0000ff);
        });
        let all = quads(&scene_with(hl, (0, 0)));
        for q in &all[UNDECORATED_QUADS..] {
            assert_eq!(q.col, [0.0, 0.0, 1.0, 1.0], "cursor underline should use bg");
        }

        // An explicit `sp` still wins over the inversion.
        let hl = with(|h| {
            h.underline = true;
            h.fg = Some(0xff0000);
            h.bg = Some(0x0000ff);
            h.special = Some(0x00ff00);
        });
        for q in &quads(&scene_with(hl, (0, 0)))[UNDECORATED_QUADS..] {
            assert_eq!(q.col, [0.0, 1.0, 0.0, 1.0], "`sp` should survive the cursor inversion");
        }
    }

    #[test]
    fn underline_sits_below_the_strikethrough() {
        let hl = with(|h| {
            h.underline = true;
            h.strikethrough = true;
        });
        let d = decorations(hl);
        assert_eq!(d.len(), 2);
        let (underline, strike) = (d[0], d[1]);
        assert!(
            underline.y > strike.y,
            "underline ({}) should sit below strikethrough ({})",
            underline.y,
            strike.y
        );
    }

    #[test]
    fn bold_and_italic_do_not_emit_decoration_quads() {
        // Synthetic face reshapes the glyph but must not add underline/strike quads.
        assert_eq!(scene_floats(with(|h| h.bold = true)), 3 * QUAD_FLOATS);
        assert_eq!(scene_floats(with(|h| h.italic = true)), 3 * QUAD_FLOATS);
    }

    #[test]
    fn bold_and_italic_reshape_the_rendered_glyph() {
        // The synthetic face must actually reach the atlas: same character, same
        // cell, but a differently shaped glyph quad than the upright one.
        let glyph = |hl: Hl| quads(&scene(hl))[2];
        let plain_g = glyph(plain());
        let bold_g = glyph(with(|h| h.bold = true));
        let italic_g = glyph(with(|h| h.italic = true));
        assert!(bold_g.w > plain_g.w, "bold should widen the ink");
        assert!(italic_g.w > plain_g.w, "italic shear should widen the ink box");
        assert_eq!(bold_g.h, plain_g.h, "styling must not change glyph height");
        assert_eq!(italic_g.h, plain_g.h);
    }
}
