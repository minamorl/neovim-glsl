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

/// Floats per vertex in the interleaved stream: pos.xy, uv.xy, colour.rgba.
pub const VERTEX_FLOATS: usize = 8;

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
        build_vertices(&mut self.verts, grid, atlas, preedit);
    }

    /// Vertices the last [`Renderer::build`] produced.
    pub fn vertex_count(&self) -> usize {
        self.verts.len() / VERTEX_FLOATS
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
                atlas.stats.uploads += 1;
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

/// Turn a grid into the interleaved vertex stream the shader consumes.
///
/// This is the whole CPU side of a frame and it touches no GL object, so a
/// headless benchmark can time exactly the code the window runs rather than a
/// re-implementation of it.
pub fn build_vertices(verts: &mut Vec<f32>, grid: &Grid, atlas: &mut Atlas, preedit: &str) {
    verts.clear();
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

    // Pass 2: glyphs.
    for row in 0..grid.rows {
        for col in 0..grid.cols {
            let cell = grid.cell(row, col);
            let (fg, bg) = grid.colors(cell.hl);
            let is_cursor = grid.cursor == (row, col);
            let ink = if is_cursor { bg } else { fg };
            let Some(g) = atlas.glyph(cell.ch) else { continue };
            let x = col as f32 * cw + g.bearing_x;
            let y = row as f32 * ch + atlas.ascent - g.bearing_y;
            push_quad(verts, x, y, g.w, g.h, (g.u0, g.v0, g.u1, g.v1), rgb(ink, 1.0));
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
