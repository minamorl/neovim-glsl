//! nvimgl — Neovim's screen, drawn by GLSL.
//!
//! Neovim itself is untouched: it runs as `nvim --embed` and remains the
//! editing engine. This process owns only pixels and input.

mod gl;
mod grid;
mod nvim;
mod text;

use std::time::{Duration, Instant};

use glow::HasContext;
use glutin::config::ConfigTemplateBuilder;
use glutin::context::{
    ContextApi, ContextAttributesBuilder, GlProfile, NotCurrentGlContext, PossiblyCurrentContext,
    Version,
};
use glutin::display::{GetGlDisplay, GlDisplay};
use glutin::surface::{GlSurface, Surface, SurfaceAttributesBuilder, WindowSurface};
use glutin_winit::{DisplayBuilder, GlWindow};
use raw_window_handle::HasWindowHandle;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, Ime, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowId};

struct Args {
    snapshot: Option<String>,
    input: Option<String>,
    cols: usize,
    rows: usize,
    font_size: f32,
    nvim_args: Vec<String>,
}

fn parse_args() -> Args {
    let mut a = Args {
        snapshot: None,
        input: None,
        cols: 80,
        rows: 24,
        font_size: 15.0,
        nvim_args: Vec::new(),
    };
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "--snapshot" => { a.snapshot = argv.get(i + 1).cloned(); i += 2 }
            "--input" => { a.input = argv.get(i + 1).cloned(); i += 2 }
            "--cols" => { a.cols = argv[i + 1].parse().unwrap_or(80); i += 2 }
            "--rows" => { a.rows = argv[i + 1].parse().unwrap_or(24); i += 2 }
            "--font-size" => { a.font_size = argv[i + 1].parse().unwrap_or(15.0); i += 2 }
            "--" => { a.nvim_args.extend_from_slice(&argv[i + 1..]); break }
            other => { a.nvim_args.push(other.to_string()); i += 1 }
        }
    }
    a
}

struct App {
    args: Args,
    started: bool,
    win: Option<Window>,
    ctx: Option<PossiblyCurrentContext>,
    surface: Option<Surface<WindowSurface>>,
    gl: Option<glow::Context>,
    renderer: Option<gl::Renderer>,
    atlas: Option<text::Atlas>,
    grid: Option<grid::Grid>,
    nvim: Option<nvim::Nvim>,
    mods: ModifiersState,
    /// Uncommitted IME composition. Owned by the IME, not by Neovim: it is drawn
    /// locally and only reaches nvim once the IME commits it.
    preedit: String,
}

impl App {
    fn new(args: Args) -> Self {
        Self {
            args,
            started: false,
            win: None,
            ctx: None,
            surface: None,
            gl: None,
            renderer: None,
            atlas: None,
            grid: None,
            nvim: None,
            mods: ModifiersState::empty(),
            preedit: String::new(),
        }
    }

    fn pump(&mut self) -> bool {
        let (Some(nv), Some(g)) = (self.nvim.as_mut(), self.grid.as_mut()) else {
            return false;
        };
        let (events, closed) = nv.drain_redraw();
        if !events.is_empty() {
            g.apply(&events);
        }
        closed
    }

    fn render(&mut self) {
        // Keep the candidate window under the text cursor.
        if let (Some(w), Some(atlas), Some(grid)) =
            (self.win.as_ref(), self.atlas.as_ref(), self.grid.as_ref())
        {
            let (row, col) = grid.cursor;
            w.set_ime_cursor_area(
                winit::dpi::PhysicalPosition::new(
                    col as f64 * atlas.cell_w as f64,
                    (row + 1) as f64 * atlas.cell_h as f64,
                ),
                winit::dpi::PhysicalSize::new(atlas.cell_w as f64, atlas.cell_h as f64),
            );
        }
        let (Some(gl), Some(r), Some(atlas), Some(grid), Some(surface), Some(ctx)) = (
            self.gl.as_ref(),
            self.renderer.as_mut(),
            self.atlas.as_mut(),
            self.grid.as_ref(),
            self.surface.as_ref(),
            self.ctx.as_ref(),
        ) else {
            return;
        };
        let size = self.win.as_ref().unwrap().inner_size();
        r.build(grid, atlas, &self.preedit);
        r.draw(gl, atlas, size.width as i32, size.height as i32);
        let _ = surface.swap_buffers(ctx);
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.started {
            return;
        }
        self.started = true;

        let snapshot = self.args.snapshot.is_some();
        let atlas = text::Atlas::new(self.args.font_size);
        let win_w = (atlas.cell_w * self.args.cols as f32).ceil() as u32;
        let win_h = (atlas.cell_h * self.args.rows as f32).ceil() as u32;

        let attrs = Window::default_attributes()
            .with_title("nvimgl")
            .with_visible(!snapshot)
            .with_inner_size(winit::dpi::PhysicalSize::new(win_w, win_h));

        let (window, gl_config) = DisplayBuilder::new()
            .with_window_attributes(Some(attrs))
            .build(el, ConfigTemplateBuilder::new(), |c| c.last().expect("no GL config"))
            .expect("display build failed");
        let window = window.expect("no window");
        // Without this macOS never routes composition to us and Japanese input is
        // impossible regardless of how the key events are handled.
        window.set_ime_allowed(true);

        let raw = window.window_handle().ok().map(|h| h.as_raw());
        let display = gl_config.display();
        let ctx_attrs = ContextAttributesBuilder::new()
            .with_context_api(ContextApi::OpenGl(Some(Version::new(3, 3))))
            .with_profile(GlProfile::Core)
            .build(raw);
        let not_current =
            unsafe { display.create_context(&gl_config, &ctx_attrs).expect("create_context") };
        let sa: SurfaceAttributesBuilder<WindowSurface> = Default::default();
        let sa = window.build_surface_attributes(sa).expect("surface attrs");
        let surface =
            unsafe { display.create_window_surface(&gl_config, &sa).expect("window surface") };
        let ctx = not_current.make_current(&surface).expect("make_current");
        let glc = unsafe {
            glow::Context::from_loader_function_cstr(|s| display.get_proc_address(s))
        };
        unsafe {
            eprintln!(
                "GL {} | {} | GLSL {}",
                glc.get_parameter_string(glow::VERSION),
                glc.get_parameter_string(glow::RENDERER),
                glc.get_parameter_string(glow::SHADING_LANGUAGE_VERSION)
            );
        }

        let renderer = gl::Renderer::new(&glc);
        let mut nv = nvim::Nvim::spawn(&self.args.nvim_args).expect("nvim --embed failed to start");
        nv.ui_attach(self.args.cols as u32, self.args.rows as u32).expect("ui_attach");
        let grid = grid::Grid::new(self.args.cols, self.args.rows);

        self.win = Some(window);
        self.ctx = Some(ctx);
        self.surface = Some(surface);
        self.gl = Some(glc);
        self.renderer = Some(renderer);
        self.atlas = Some(atlas);
        self.grid = Some(grid);
        self.nvim = Some(nv);

        if let Some(path) = self.args.snapshot.clone() {
            self.run_snapshot(&path, win_w, win_h);
            el.exit();
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Focused(f) => {
                eprintln!("focus: {f}");
                // The first set_ime_allowed happens before the window is focused,
                // which some backends ignore. Re-assert once we actually have it.
                if f {
                    if let Some(w) = self.win.as_ref() {
                        w.set_ime_allowed(true);
                    }
                }
            }
            WindowEvent::ModifiersChanged(m) => self.mods = m.state(),
            WindowEvent::Resized(size) => {
                if let (Some(atlas), Some(nv), Some(surface), Some(ctx)) = (
                    self.atlas.as_ref(),
                    self.nvim.as_mut(),
                    self.surface.as_ref(),
                    self.ctx.as_ref(),
                ) {
                    let cols = (size.width as f32 / atlas.cell_w).floor().max(1.0) as u32;
                    let rows = (size.height as f32 / atlas.cell_h).floor().max(1.0) as u32;
                    let _ = nv.try_resize(cols, rows);
                    if let (Some(w), Some(h)) = (
                        std::num::NonZeroU32::new(size.width),
                        std::num::NonZeroU32::new(size.height),
                    ) {
                        surface.resize(ctx, w, h);
                    }
                }
            }
            WindowEvent::Ime(ime) => {
                match ime {
                    Ime::Enabled => eprintln!("IME: enabled"),
                    Ime::Preedit(text, range) => {
                        eprintln!("IME: preedit {text:?} cursor={range:?}");
                        self.preedit = text;
                    }
                    Ime::Commit(text) => {
                        eprintln!("IME: commit {text:?}");
                        self.preedit.clear();
                        if let Some(nv) = self.nvim.as_mut() {
                            let _ = nv.input(&text.replace('<', "<lt>"));
                        }
                    }
                    Ime::Disabled => {
                        eprintln!("IME: disabled");
                        self.preedit.clear();
                    }
                }
                if let Some(w) = self.win.as_ref() {
                    w.request_redraw();
                }
            }
            // While a composition is open the IME owns the keystrokes; forwarding
            // them too would double-insert whatever is being composed.
            WindowEvent::KeyboardInput { .. } if !self.preedit.is_empty() => {}
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                if let Some(keys) = encode_key(&event.logical_key, self.mods) {
                    if let Some(nv) = self.nvim.as_mut() {
                        let _ = nv.input(&keys);
                    }
                }
            }
            WindowEvent::RedrawRequested => self.render(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if self.args.snapshot.is_some() {
            return;
        }
        if self.pump() {
            el.exit();
            return;
        }
        if let Some(g) = self.grid.as_mut() {
            if g.flushed {
                g.flushed = false;
                if let Some(w) = self.win.as_ref() {
                    w.request_redraw();
                }
            }
        }
        el.set_control_flow(ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(8)));
    }
}

impl App {
    /// Render one settled frame offscreen and write it as a PNG, so the result
    /// can be checked without a human watching a window.
    fn run_snapshot(&mut self, path: &str, w: u32, h: u32) {
        if let (Some(nv), Some(keys)) = (self.nvim.as_mut(), self.args.input.clone()) {
            // Let the initial screen settle before typing into it.
            let deadline = Instant::now() + Duration::from_millis(800);
            while Instant::now() < deadline {
                let (ev, _) = nv.wait_redraw(Duration::from_millis(60));
                if let Some(g) = self.grid.as_mut() {
                    g.apply(&ev);
                }
            }
            let _ = self.nvim.as_mut().unwrap().input(&keys);
        }

        let deadline = Instant::now() + Duration::from_millis(1500);
        while Instant::now() < deadline {
            let (ev, closed) = self.nvim.as_mut().unwrap().wait_redraw(Duration::from_millis(80));
            if let Some(g) = self.grid.as_mut() {
                g.apply(&ev);
            }
            if closed {
                break;
            }
        }

        let gl = self.gl.as_ref().unwrap();
        let buf = unsafe {
            let tex = gl.create_texture().unwrap();
            gl.bind_texture(glow::TEXTURE_2D, Some(tex));
            gl.tex_image_2d(
                glow::TEXTURE_2D, 0, glow::RGBA8 as i32, w as i32, h as i32, 0,
                glow::RGBA, glow::UNSIGNED_BYTE, None,
            );
            let fbo = gl.create_framebuffer().unwrap();
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
            gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0, glow::TEXTURE_2D, Some(tex), 0,
            );
            assert_eq!(
                gl.check_framebuffer_status(glow::FRAMEBUFFER),
                glow::FRAMEBUFFER_COMPLETE,
                "offscreen target incomplete"
            );

            let r = self.renderer.as_mut().unwrap();
            r.build(self.grid.as_ref().unwrap(), self.atlas.as_mut().unwrap(), "");
            r.draw(gl, self.atlas.as_mut().unwrap(), w as i32, h as i32);
            gl.finish();

            let mut buf = vec![0u8; (w * h * 4) as usize];
            gl.read_pixels(
                0, 0, w as i32, h as i32, glow::RGBA, glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(&mut buf),
            );
            buf
        };
        write_png(path, &buf, w, h);
        eprintln!("WROTE {path} ({w}x{h})");
    }
}

fn encode_key(key: &Key, mods: ModifiersState) -> Option<String> {
    let named = |s: &str| Some(wrap(s, mods));
    match key {
        Key::Named(n) => match n {
            NamedKey::Enter => named("CR"),
            NamedKey::Escape => named("Esc"),
            NamedKey::Backspace => named("BS"),
            NamedKey::Tab => named("Tab"),
            NamedKey::Space => Some(if mods.control_key() { "<C-Space>".into() } else { " ".into() }),
            NamedKey::ArrowLeft => named("Left"),
            NamedKey::ArrowRight => named("Right"),
            NamedKey::ArrowUp => named("Up"),
            NamedKey::ArrowDown => named("Down"),
            NamedKey::Home => named("Home"),
            NamedKey::End => named("End"),
            NamedKey::PageUp => named("PageUp"),
            NamedKey::PageDown => named("PageDown"),
            NamedKey::Delete => named("Del"),
            NamedKey::F1 => named("F1"),
            NamedKey::F2 => named("F2"),
            NamedKey::F3 => named("F3"),
            NamedKey::F4 => named("F4"),
            _ => None,
        },
        Key::Character(s) => {
            if mods.control_key() || mods.super_key() || mods.alt_key() {
                let base = s.as_str();
                Some(wrap(base, mods))
            } else if s.as_str() == "<" {
                Some("<lt>".into())
            } else {
                Some(s.to_string())
            }
        }
        _ => None,
    }
}

fn wrap(base: &str, mods: ModifiersState) -> String {
    let mut p = String::new();
    if mods.control_key() { p.push_str("C-") }
    if mods.alt_key() { p.push_str("A-") }
    if mods.super_key() { p.push_str("D-") }
    if mods.shift_key() && base.len() > 1 { p.push_str("S-") }
    format!("<{p}{base}>")
}

fn write_png(path: &str, rgba_bottom_up: &[u8], w: u32, h: u32) {
    let stride = (w * 4) as usize;
    let mut flipped = vec![0u8; rgba_bottom_up.len()];
    for y in 0..h as usize {
        let src = (h as usize - 1 - y) * stride;
        flipped[y * stride..(y + 1) * stride]
            .copy_from_slice(&rgba_bottom_up[src..src + stride]);
    }
    let f = std::fs::File::create(path).expect("create png");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(f), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&flipped).unwrap();
}

fn main() {
    let args = parse_args();
    let el = EventLoop::new().expect("event loop");
    el.set_control_flow(ControlFlow::Wait);
    let mut app = App::new(args);
    el.run_app(&mut app).expect("run_app");
}
