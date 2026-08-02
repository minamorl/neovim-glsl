//! The GLSL renderer. Every visible pixel — background, glyph and cursor —
//! goes through this one shader pair as a textured quad.

use glow::HasContext;

use crate::grid::{Cell, Hl};
use crate::screen::Screen;
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

    /// Composited screen -> vertices. The scene builder itself lives
    /// outside the renderer so it stays testable without a GL context.
    pub fn build(&mut self, screen: &Screen, atlas: &mut Atlas, preedit: &str) {
        self.verts.clear();
        build_scene(&mut self.verts, screen, atlas, preedit);
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

/// What the scene builder needs from whatever it is drawing.
///
/// Two things satisfy it: a bare [`Grid`] (one flat grid, which is what the
/// unit tests drive) and a [`Screen`] (every grid nvim placed, already
/// composited). Keeping the builder generic means multigrid composition and
/// styled-text rendering share exactly one implementation.
pub trait Surface {
    fn n_rows(&self) -> usize;
    fn n_cols(&self) -> usize;
    fn cell_at(&self, row: usize, col: usize) -> Cell;
    /// `None` when the cursor is not on screen at all.
    fn cursor_pos(&self) -> Option<(usize, usize)>;
    fn hl_style(&self, hl_id: u64) -> Hl;
    fn hl_colors(&self, hl_id: u64) -> (u32, u32);
    fn hl_decoration_color(&self, hl_id: u64) -> u32;
}

impl Surface for Screen {
    fn n_rows(&self) -> usize { self.rows() }
    fn n_cols(&self) -> usize { self.cols() }
    fn cell_at(&self, row: usize, col: usize) -> Cell { self.cell(row, col) }
    fn cursor_pos(&self) -> Option<(usize, usize)> { self.cursor() }
    fn hl_style(&self, hl_id: u64) -> Hl { self.style(hl_id) }
    fn hl_colors(&self, hl_id: u64) -> (u32, u32) { self.colors(hl_id) }
    fn hl_decoration_color(&self, hl_id: u64) -> u32 { self.decoration_color(hl_id) }
}

/// Build the whole grid scene (backgrounds, glyphs, text decorations and the
/// IME preedit) into `verts`. Free of any GL handle so it is unit-testable on
/// the host with only a font-backed [`Atlas`], no GPU context.
pub fn build_scene(verts: &mut Vec<f32>, surface: &impl Surface, atlas: &mut Atlas, preedit: &str) {
    let (cw, ch) = (atlas.cell_w, atlas.cell_h);
    let (wu, wv) = atlas.white_uv();
    let white = (wu, wv, wu, wv);

    // Pass 1: every background, including the cursor block, so glyphs drawn
    // afterwards always land on top of their own cell's background.
    for row in 0..surface.n_rows() {
        for col in 0..surface.n_cols() {
            let cell = surface.cell_at(row, col);
            let (fg, bg) = surface.hl_colors(cell.hl);
            let is_cursor = surface.cursor_pos() == Some((row, col));
            let paint = if is_cursor { fg } else { bg };
            push_quad(verts, col as f32 * cw, row as f32 * ch, cw, ch, white, rgb(paint, 1.0));
        }
    }

    // Pass 2: glyphs, now rasterised with the cell's synthetic bold/italic face.
    for row in 0..surface.n_rows() {
        for col in 0..surface.n_cols() {
            let cell = surface.cell_at(row, col);
            let (fg, bg) = surface.hl_colors(cell.hl);
            let st = surface.hl_style(cell.hl);
            let is_cursor = surface.cursor_pos() == Some((row, col));
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
    for row in 0..surface.n_rows() {
        for col in 0..surface.n_cols() {
            let cell = surface.cell_at(row, col);
            let st = surface.hl_style(cell.hl);
            if !st.any_underline() && !st.strikethrough {
                continue;
            }
            // A decoration follows the glyph ink: nvim's `sp` when it gave one,
            // otherwise whatever colour the glyph itself was drawn in. The cursor
            // cell inverts that ink, so its decoration inverts with it and stays
            // visible against the block instead of vanishing into it.
            let color = if surface.cursor_pos() == Some((row, col)) {
                st.special.unwrap_or_else(|| surface.hl_colors(cell.hl).1)
            } else {
                surface.hl_decoration_color(cell.hl)
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
        let Some((row, col)) = surface.cursor_pos() else { return };
        let (fg, bg) = surface.hl_colors(surface.cell_at(row, col).hl);
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
    // the wave occupies `2 * amp + t` px vertically. The sample grid is locked to
    // absolute x exactly as `segmented` is, not restarted at the cell edge: a
    // sample straddling the boundary is clipped into two quads at one shared y,
    // so the wave joins up instead of kinking once per cell.
    let curl = |verts: &mut Vec<f32>, y: f32, amp: f32| {
        let period = (6.0 * t).max(4.0);
        let step = (period / 8.0).max(1.0);
        let mut x = x0 - x0 % step;
        while x < x0 + cw {
            let (lo, hi) = (x.max(x0), (x + step).min(x0 + cw));
            if hi > lo {
                // Phase from the unclipped sample centre, so both halves of a
                // clipped sample land on the same y.
                let phase = (x + step * 0.5) / period * std::f32::consts::TAU;
                push_quad(verts, lo, y + amp * phase.sin(), hi - lo, t, white, color);
            }
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
    use crate::grid::{Cell, Grid, Hl, Styles};
    use crate::text::Atlas;

    /// One flat grid with its own styles and cursor: what the renderer saw
    /// before ext_multigrid, kept here so the scene-building laws are still
    /// checked without standing up a whole composited [`Screen`].
    struct Flat {
        grid: Grid,
        styles: Styles,
        cursor: (usize, usize),
    }

    impl Flat {
        fn new(cols: usize, rows: usize) -> Self {
            Self { grid: Grid::new(cols, rows), styles: Styles::new(), cursor: (0, 0) }
        }
    }

    impl Surface for Flat {
        fn n_rows(&self) -> usize { self.grid.rows }
        fn n_cols(&self) -> usize { self.grid.cols }
        fn cell_at(&self, row: usize, col: usize) -> Cell { self.grid.cell(row, col) }
        fn cursor_pos(&self) -> Option<(usize, usize)> { Some(self.cursor) }
        fn hl_style(&self, hl_id: u64) -> Hl { self.styles.style(hl_id) }
        fn hl_colors(&self, hl_id: u64) -> (u32, u32) { self.styles.colors(hl_id) }
        fn hl_decoration_color(&self, hl_id: u64) -> u32 { self.styles.decoration_color(hl_id) }
    }

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
        let mut flat = Flat::new(2, 1);
        flat.cursor = cursor;
        flat.styles.hls.insert(1, hl);
        flat.grid.cells[0] = Cell { ch: 'x', hl: 1 };
        let mut verts = Vec::new();
        build_scene(&mut verts, &flat, &mut atlas, "");
        verts
    }

    /// The same scene with the cursor parked off-grid, so no cell flips to
    /// cursor colours.
    fn scene(hl: Hl) -> Vec<f32> {
        scene_with(hl, (0, 5))
    }

    /// A 2x1 grid whose *both* cells are `x` styled by `hl`, so decorations are
    /// emitted at two different cell origins through the real `build_scene`
    /// path. Cursor parked off-grid.
    fn run_scene(hl: Hl) -> Vec<f32> {
        let mut atlas = Atlas::new(20.0);
        let mut flat = Flat::new(2, 1);
        flat.cursor = (0, 5);
        flat.styles.hls.insert(1, hl);
        flat.grid.cells[0] = Cell { ch: 'x', hl: 1 };
        flat.grid.cells[1] = Cell { ch: 'x', hl: 1 };
        let mut verts = Vec::new();
        build_scene(&mut verts, &flat, &mut atlas, "");
        verts
    }

    /// Two backgrounds plus two glyphs precede the decorations in `run_scene`.
    /// Pinned by `plain_run_emits_only_backgrounds_and_glyphs`.
    const RUN_UNDECORATED_QUADS: usize = 4;

    fn run_scene_decorations(hl: Hl) -> Vec<Quad> {
        quads(&run_scene(hl))[RUN_UNDECORATED_QUADS..].to_vec()
    }

    /// Cell geometry pinned for the cross-cell tests: `(cw, ch, ascent,
    /// x_height)`. Font-derived metrics would leave these tests at the mercy of
    /// whichever font the host has; here `cw` is deliberately *not* a whole
    /// number of dash/dot periods, so an absolute phase lock and a per-cell
    /// restart cannot produce the same output.
    const RUN: (f32, f32, f32, f32) = (7.0, 14.0, 11.0, 7.0);
    /// The same, at double thickness (`t == 2`), which makes the curl's sample
    /// step 1.5px — again not a divisor of `cw`.
    const TALL_RUN: (f32, f32, f32, f32) = (7.0, 28.0, 22.0, 14.0);

    /// Decorations for a horizontal run of `cells` identically styled cells,
    /// laid out left to right exactly as `build_scene` lays them out.
    fn run_decorations(hl: Hl, geom: (f32, f32, f32, f32), cells: usize) -> Vec<Quad> {
        let (cw, ch, ascent, x_height) = geom;
        let mut verts = Vec::new();
        for i in 0..cells {
            push_cell_decorations(
                &mut verts,
                (0.0, 0.0, 0.0, 0.0),
                [1.0, 1.0, 1.0, 1.0],
                hl,
                i as f32 * cw,
                0.0,
                cw,
                ch,
                ascent,
                x_height,
            );
        }
        quads(&verts)
    }

    /// The cell a quad belongs to, taken from its centre so a quad sitting flush
    /// against a boundary is attributed unambiguously.
    fn owning_cell(q: &Quad, cw: f32) -> f32 {
        ((q.x + q.w * 0.5) / cw).floor()
    }

    /// Merge quads that abut exactly, so a mark clipped at a cell boundary reads
    /// back as the single mark it is drawn to be. Input must be in ascending x.
    fn merge_marks(qs: &[Quad]) -> Vec<(f32, f32)> {
        let mut spans: Vec<(f32, f32)> = Vec::new();
        for q in qs {
            if matches!(spans.last(), Some(last) if (last.1 - q.x).abs() < 1e-4) {
                spans.last_mut().unwrap().1 = q.x + q.w;
            } else {
                spans.push((q.x, q.x + q.w));
            }
        }
        spans
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
        for (name, hl) in decoration_styles() {
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

    /// Every style that emits decoration quads, by name.
    fn decoration_styles() -> Vec<(&'static str, Hl)> {
        vec![
            ("underline", with(|h| h.underline = true)),
            ("undercurl", with(|h| h.undercurl = true)),
            ("underdouble", with(|h| h.underdouble = true)),
            ("underdotted", with(|h| h.underdotted = true)),
            ("underdashed", with(|h| h.underdashed = true)),
            ("strikethrough", with(|h| h.strikethrough = true)),
        ]
    }

    #[test]
    fn plain_run_emits_only_backgrounds_and_glyphs() {
        // Pins `RUN_UNDECORATED_QUADS`: if the prologue ever grows, the run
        // tests would silently start reading a background as a decoration.
        assert_eq!(run_scene(plain()).len(), RUN_UNDECORATED_QUADS * QUAD_FLOATS);
    }

    #[test]
    fn segmented_underlines_are_phase_locked_across_cell_boundaries() {
        // The headline claim: a run of cells sharing one highlight reads as a
        // single pattern. Restarting the phase at each cell would show up here
        // as a short or doubled mark exactly at the boundary.
        for (name, hl) in [
            ("underdotted", with(|h| h.underdotted = true)),
            ("underdashed", with(|h| h.underdashed = true)),
        ] {
            let marks = merge_marks(&run_decorations(hl, RUN, 2));
            assert!(marks.len() >= 3, "{name}: expected a repeating pattern, got {marks:?}");
            let width = |m: &(f32, f32)| m.1 - m.0;
            let (first, last) = (width(&marks[0]), width(marks.last().unwrap()));
            // Only the final mark may be short, clipped by the end of the run.
            for m in &marks[..marks.len() - 1] {
                assert!(
                    (width(m) - first).abs() < 1e-4,
                    "{name}: uneven mark {m:?} in {marks:?}"
                );
            }
            assert!(last <= first + 1e-4, "{name}: trailing mark grew: {marks:?}");
            let pitch = marks[1].0 - marks[0].0;
            for w in marks.windows(2) {
                assert!(
                    ((w[1].0 - w[0].0) - pitch).abs() < 1e-4,
                    "{name}: uneven spacing in {marks:?}"
                );
            }
            // And the pattern really does cross the boundary rather than
            // coincidentally lining up with it.
            let (cw, ..) = RUN;
            assert!(
                marks.iter().any(|m| m.0 < cw && m.1 > cw)
                    || marks.iter().all(|m| (m.0 % pitch - marks[0].0 % pitch).abs() < 1e-4),
                "{name}: pattern does not continue past x={cw}: {marks:?}"
            );
        }
    }

    #[test]
    fn undercurl_samples_are_phase_locked_across_cell_boundaries() {
        // At `TALL_RUN` the sample step is 1.5px, so one sample straddles the
        // 7px cell edge. Both halves belong to the same sample and must sit at
        // the same y, or the wave kinks once per cell.
        let (cw, ..) = TALL_RUN;
        let curl = run_decorations(with(|h| h.undercurl = true), TALL_RUN, 2);
        let ends_at_edge = curl
            .iter()
            .find(|q| (q.x + q.w - cw).abs() < 1e-4)
            .expect("a sample clipped by the cell's right edge");
        let starts_at_edge = curl
            .iter()
            .find(|q| (q.x - cw).abs() < 1e-4)
            .expect("a sample resuming at the next cell's left edge");
        assert!(
            (ends_at_edge.y - starts_at_edge.y).abs() < 1e-4,
            "curl jumps at the cell boundary: {} -> {}",
            ends_at_edge.y,
            starts_at_edge.y
        );
        // The two halves reconstitute exactly one 1.5px sample: neither is
        // dropped, and the next cell does not start a fresh full-width one.
        assert!(
            (ends_at_edge.w + starts_at_edge.w - 1.5).abs() < 1e-4,
            "split sample halves are {} + {}, expected one 1.5px step",
            ends_at_edge.w,
            starts_at_edge.w
        );
        // Still a wave, not a flattened rule.
        let top = curl.iter().map(|q| q.y).fold(f32::MAX, f32::min);
        let bottom = curl.iter().map(|q| q.y).fold(f32::MIN, f32::max);
        assert!(bottom - top > 0.0, "curl steps must vary in y");
    }

    #[test]
    fn every_decoration_stays_inside_its_own_cell_anywhere_in_a_run() {
        // `every_decoration_stays_inside_its_cell` only ever looks at x0 == 0,
        // where clipping and a per-cell restart are indistinguishable. Here the
        // decorations start at three different cell origins.
        for geom in [RUN, TALL_RUN] {
            let (cw, ch, ..) = geom;
            for (name, hl) in decoration_styles() {
                for q in run_decorations(hl, geom, 3) {
                    let cell = owning_cell(&q, cw);
                    assert!(
                        q.x >= cell * cw - 1e-4 && q.x + q.w <= (cell + 1.0) * cw + 1e-4,
                        "{name} escapes cell {cell} horizontally: {q:?}"
                    );
                    assert!(
                        q.y >= -1e-4 && q.y + q.h <= ch + 1e-4,
                        "{name} escapes the cell vertically: {q:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn build_scene_decorates_each_cell_of_a_run_at_its_own_origin() {
        // The same property through the real scene builder, so the per-cell
        // origin `build_scene` passes down is covered too.
        let (cw, ch, ..) = metrics();
        for (name, hl) in decoration_styles() {
            let d = run_scene_decorations(hl);
            assert!(
                d.iter().any(|q| q.x >= cw - 1e-4),
                "{name}: the second cell of the run got no decoration"
            );
            for q in &d {
                let cell = owning_cell(q, cw);
                assert!(
                    q.x >= cell * cw - 1e-4 && q.x + q.w <= (cell + 1.0) * cw + 1e-4,
                    "{name} escapes cell {cell} horizontally: {q:?}"
                );
                assert!(q.y >= -1e-4 && q.y + q.h <= ch + 1e-4, "{name} escapes vertically: {q:?}");
            }
        }
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
