//! nvimgl — Neovim's screen, drawn by GLSL.
//!
//! Neovim itself is untouched: it runs as `nvim --embed` and remains the
//! editing engine. This process owns only pixels and input.

mod aish;
mod bench;
mod cmap;
mod ext_ui;
mod gl;
mod grid;
mod nvim;
mod panel;
mod perf;
mod picker_state;
mod platform;
mod root_ui;
mod screen;
mod surface_locus;
mod text;

use std::path::PathBuf;
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
    lua: Option<String>,
    preedit: Option<String>,
    aish: Option<PathBuf>,
    platform_report: Option<PathBuf>,
    root_ui_evaluation: Option<PathBuf>,
    /// A JSON file of free surfaces to draw over the grid. Evidence for
    /// `open_question neovim_glsl.navigation_surface_decision`; absent by
    /// default, because a host that always draws a panel has decided something.
    panels: Option<PathBuf>,
    /// Where to write what the panel pass actually emitted.
    panel_report: Option<PathBuf>,
    /// Where to write the observed locus of those surfaces. Evidence for the
    /// v0.8 pins that made the locus a requirement; see `surface_locus`.
    locus_report: Option<PathBuf>,
    /// Which of the multigrid / popupmenu / cmdline / message surfaces this
    /// host draws instead of letting Neovim paint them into the grid.
    ui_options: nvim::UiOptions,
    /// Measure the live session. Off unless asked for: see `perf` module docs.
    perf: bool,
    perf_report: Option<PathBuf>,
    /// Frame count for the headless deterministic benchmark. `Some` selects it.
    perf_bench: Option<u64>,
    perf_warmup: u64,
    perf_seed: u64,
    perf_frame_budget_ms: Option<f64>,
    /// A scripted picker session to run against host-owned state. `Some`
    /// selects a headless measurement for
    /// `open_question neovim_glsl.navigation_state_owner`, which v0.9 left open
    /// after the human gate answered 「わからない」. It measures one arrangement;
    /// it chooses neither.
    picker_script: Option<String>,
    /// Candidates to filter, one per line. Absent means a small built-in corpus,
    /// which is only useful for a smoke run.
    picker_corpus: Option<PathBuf>,
    picker_report: Option<PathBuf>,
    picker_visible_rows: usize,
    nvim_args: Vec<String>,
}

impl Args {
    /// Asking for a report is asking for the measurement that fills it.
    fn perf_enabled(&self) -> bool {
        self.perf || self.perf_report.is_some()
    }
}

fn parse_args() -> Args {
    parse_args_from(std::env::args().skip(1).collect())
}

fn parse_args_from(argv: Vec<String>) -> Args {
    let mut a = Args {
        snapshot: None,
        input: None,
        cols: 80,
        rows: 24,
        font_size: 15.0,
        lua: None,
        preedit: None,
        aish: None,
        platform_report: None,
        root_ui_evaluation: None,
        panels: None,
        panel_report: None,
        locus_report: None,
        ui_options: nvim::UiOptions::default(),
        perf: false,
        perf_report: None,
        perf_bench: None,
        perf_warmup: 0,
        perf_seed: 1,
        perf_frame_budget_ms: None,
        picker_script: None,
        picker_corpus: None,
        picker_report: None,
        picker_visible_rows: 12,
        nvim_args: Vec::new(),
    };
    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "--snapshot" => { a.snapshot = argv.get(i + 1).cloned(); i += 2 }
            "--input" => { a.input = argv.get(i + 1).cloned(); i += 2 }
            "--cols" => { a.cols = argv[i + 1].parse().unwrap_or(80); i += 2 }
            "--rows" => { a.rows = argv[i + 1].parse().unwrap_or(24); i += 2 }
            "--font-size" => { a.font_size = argv[i + 1].parse().unwrap_or(15.0); i += 2 }
            "--lua" => { a.lua = argv.get(i + 1).cloned(); i += 2 }
            "--aish" => { a.aish = argv.get(i + 1).map(PathBuf::from); i += 2 }
            "--platform-report" => {
                a.platform_report = argv.get(i + 1).map(PathBuf::from);
                i += 2
            }
            "--root-ui-evaluation" => {
                a.root_ui_evaluation = argv.get(i + 1).map(PathBuf::from);
                i += 2
            }
            "--panels" => { a.panels = argv.get(i + 1).map(PathBuf::from); i += 2 }
            "--panel-report" => {
                a.panel_report = argv.get(i + 1).map(PathBuf::from);
                i += 2
            }
            "--locus-report" => {
                a.locus_report = argv.get(i + 1).map(PathBuf::from);
                i += 2
            }
            // Measurement. Absent flags mean absent measurement, and an
            // unparsable value leaves the setting unobserved rather than
            // silently substituting a number the user never chose.
            "--perf" => { a.perf = true; i += 1 }
            "--perf-report" => { a.perf_report = argv.get(i + 1).map(PathBuf::from); i += 2 }
            "--perf-bench" => {
                a.perf_bench = argv.get(i + 1).and_then(|v| v.parse().ok());
                i += 2
            }
            "--perf-warmup" => {
                a.perf_warmup = argv.get(i + 1).and_then(|v| v.parse().ok()).unwrap_or(0);
                i += 2
            }
            "--perf-seed" => {
                a.perf_seed = argv.get(i + 1).and_then(|v| v.parse().ok()).unwrap_or(1);
                i += 2
            }
            "--perf-frame-budget-ms" => {
                a.perf_frame_budget_ms = argv.get(i + 1).and_then(|v| v.parse().ok());
                i += 2
            }
            // Injects a composition string so the preedit rendering can be checked
            // without a human driving a real IME.
            "--picker-script" => { a.picker_script = argv.get(i + 1).cloned(); i += 2 }
            "--picker-corpus" => { a.picker_corpus = argv.get(i + 1).map(PathBuf::from); i += 2 }
            "--picker-report" => { a.picker_report = argv.get(i + 1).map(PathBuf::from); i += 2 }
            "--picker-visible-rows" => {
                a.picker_visible_rows = argv
                    .get(i + 1)
                    .and_then(|v| v.parse().ok())
                    .filter(|n| *n > 0)
                    .unwrap_or(12);
                i += 2
            }
            "--preedit" => { a.preedit = argv.get(i + 1).cloned(); i += 2 }
            "--no-multigrid" => { a.ui_options.ext_multigrid = false; i += 1 }
            // Hands the popupmenu, command line and messages back to Neovim's
            // own grid rendering, for comparing the two side by side.
            "--no-ext-ui" => {
                // Multigrid is a separate promise with its own flag, so it is
                // carried across rather than switched off along with these.
                a.ui_options = nvim::UiOptions {
                    ext_multigrid: a.ui_options.ext_multigrid,
                    ..nvim::UiOptions::none()
                };
                i += 1
            }
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
    screen: Option<screen::Screen>,
    /// The popupmenu, command line and messages Neovim no longer draws itself.
    ext_ui: ext_ui::ExtUi,
    nvim: Option<nvim::Nvim>,
    aish: aish::Bridge,
    graphics_probe: Option<platform::GraphicsProbe>,
    evaluation_written: bool,
    perf: perf::Recorder,
    perf_report_written: bool,
    mods: ModifiersState,
    /// Uncommitted IME composition. Owned by the IME, not by Neovim: it is drawn
    /// locally and only reaches nvim once the IME commits it.
    preedit: String,
    /// Non-text surfaces placed by Lua. Neovim's grid knows nothing about these.
    images: Vec<gl::Image>,
    /// Free surfaces loaded from `--panels`, drawn over the grid in pixels.
    panels: Vec<panel::Panel>,
    /// What the last panel pass emitted, kept for `--panel-report`.
    panel_stats: Vec<panel::PanelStats>,
    /// Vertices in the buffer after the grid pass and after the surface pass, in
    /// that order. Recorded at the two points where they are true rather than
    /// recomputed later: what they witness is that both passes write into one
    /// buffer, and a recomputation would keep saying so after they stopped.
    grid_vertices: usize,
    total_vertices: usize,
}

impl App {
    fn new(args: Args) -> Self {
        let aish = aish::Bridge::new(args.aish.clone());
        let perf = perf::Recorder::new(args.perf_enabled());
        Self {
            args,
            started: false,
            win: None,
            ctx: None,
            surface: None,
            gl: None,
            renderer: None,
            atlas: None,
            screen: None,
            ext_ui: ext_ui::ExtUi::new(),
            nvim: None,
            aish,
            graphics_probe: None,
            evaluation_written: false,
            perf,
            perf_report_written: false,
            mods: ModifiersState::empty(),
            preedit: String::new(),
            images: Vec::new(),
            panels: Vec::new(),
            panel_stats: Vec::new(),
            grid_vertices: 0,
            total_vertices: 0,
        }
    }

    /// Read the panel file, if one was asked for. A malformed file is reported
    /// and then ignored: a broken evidence input must not take the session with
    /// it, and silently drawing nothing would look like a panel that emitted
    /// nothing.
    fn load_panels(&mut self) {
        let Some(path) = self.args.panels.clone() else { return };
        match std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|t| serde_json::from_str::<Vec<panel::Panel>>(&t).map_err(|e| e.to_string()))
        {
            Ok(panels) => self.panels = panels,
            Err(error) => eprintln!("panel file {} unusable: {error}", path.display()),
        }
    }

    /// Write where the surfaces were drawn, as observed rather than asserted.
    ///
    /// Separate from the panel report on purpose. That one counts what the pass
    /// emitted and is evidence for a question; this one records the locus, which
    /// v0.8 turned into a requirement. Mixing them would let a change to the
    /// measurement quietly rewrite the conformance record.
    fn write_locus_report(&mut self) {
        let Some(path) = self.args.locus_report.clone() else { return };
        let Some(atlas) = self.atlas.as_ref() else { return };
        let observation = surface_locus::observe(
            &self.panels,
            &self.panel_stats,
            self.grid_vertices,
            self.total_vertices,
            atlas.cell_w,
            atlas.cell_h,
        );
        match surface_locus::write(&path, &observation) {
            Ok(()) => eprintln!("WROTE {}", path.display()),
            Err(error) => eprintln!("locus report write failed: {error}"),
        }
    }

    /// Write what the panel pass emitted. Counts only; no thresholds, no verdict.
    fn write_panel_report(&mut self) {
        let Some(path) = self.args.panel_report.clone() else { return };
        let report = serde_json::json!({
            "schema": "neovim-glsl.free-surface-measurement/v1",
            "panels": self.panels.len(),
            "stats": self.panel_stats,
        });
        match serde_json::to_string_pretty(&report)
            .map_err(|e| e.to_string())
            .and_then(|t| std::fs::write(&path, t + "\n").map_err(|e| e.to_string()))
        {
            Ok(()) => eprintln!("WROTE {}", path.display()),
            Err(error) => eprintln!("panel report write failed: {error}"),
        }
    }

    /// Lua asked the UI to put something on screen. The grid is untouched.
    fn handle_notifications(&mut self) {
        let notes = match self.nvim.as_mut() {
            Some(nv) => nv.take_notifications(),
            None => return,
        };
        let mut placed = Vec::new();
        for (name, params) in notes {
            match name.as_str() {
                "nvimgl_aish" => match aish::Request::from_rpc(&params) {
                    Ok(request) => self.aish.submit(request),
                    Err(error) => self.aish.submit_error(error),
                },
                "nvimgl_image" => {
                    let num =
                        |i: usize| params.get(i).and_then(|v| v.as_u64()).unwrap_or(0) as f32;
                    let Some(path) = params
                        .first()
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                    else {
                        continue;
                    };
                    let (Some(glc), Some(atlas)) = (self.gl.as_ref(), self.atlas.as_ref())
                    else {
                        continue;
                    };
                    let (row, col) = (num(1), num(2));
                    let (cols, rows) = (num(3).max(1.0), num(4).max(1.0));
                    match load_png(&path) {
                        Some((rgba, w, h)) => {
                            let tex = gl::Renderer::upload_rgba(glc, &rgba, w, h);
                            eprintln!(
                                "image: {path} {w}x{h} -> cell ({row},{col}) span {cols}x{rows}"
                            );
                            placed.push(gl::Image {
                                tex,
                                x: col * atlas.cell_w,
                                y: row * atlas.cell_h,
                                w: cols * atlas.cell_w,
                                h: rows * atlas.cell_h,
                            });
                        }
                        None => eprintln!("image: cannot load {path}"),
                    }
                }
                _ => eprintln!("note: {name} (no handler)"),
            }
        }
        self.images.extend(placed);
    }

    fn handle_aish_results(&mut self) {
        let results = self.aish.take_results();
        let Some(nvim) = self.nvim.as_mut() else {
            return;
        };
        for result in results {
            if let Err(error) = nvim.show_json_scratch(&result.title, &result.body) {
                eprintln!("aish result display failed: {error}");
            }
        }
    }

    fn write_evaluations(&mut self) {
        if self.evaluation_written {
            return;
        }
        let mut requested = false;
        if let (Some(path), Some(screen)) =
            (self.args.root_ui_evaluation.as_deref(), self.screen.as_ref())
        {
            requested = true;
            if let Err(error) = root_ui::write_evaluation(path, screen) {
                eprintln!("root-ui evaluation write failed: {error}");
            } else {
                eprintln!("WROTE {}", path.display());
            }
        }
        if let (Some(path), Some(graphics)) = (
            self.args.platform_report.as_deref(),
            self.graphics_probe.clone(),
        ) {
            requested = true;
            if let Err(error) = platform::write_report(path, graphics) {
                eprintln!("platform report write failed: {error}");
            } else {
                eprintln!("WROTE {}", path.display());
            }
        }
        self.evaluation_written = requested;
    }

    /// One redraw batch reaches both mirrors: the screen keeps what Neovim
    /// still paints, and the external surfaces keep what it handed over.
    fn apply_redraw(&mut self, events: &[nvim::RedrawEvent]) {
        if events.is_empty() {
            return;
        }
        if let Some(screen) = self.screen.as_mut() {
            screen.apply(events);
        }
        self.ext_ui.apply(events);
    }

    fn pump(&mut self) -> bool {
        let batch = self.perf.span();
        let (events, closed) = match self.nvim.as_mut() {
            Some(nv) => nv.drain_redraw(),
            None => return false,
        };
        self.apply_redraw(&events);
        // An empty drain is the idle poll, not a redraw batch. Timing it would
        // fill the distribution with the cost of finding nothing to do.
        if !events.is_empty() {
            self.perf.record_event_apply(batch);
            self.perf.record_batch(&events);
        }
        self.handle_notifications();
        self.handle_aish_results();
        closed
    }

    /// The external surfaces, placed into the current screen.
    fn overlay(&self) -> ext_ui::Overlay {
        match self.screen.as_ref() {
            Some(screen) if !self.ext_ui.is_idle() => {
                self.ext_ui.layout(screen.cols(), screen.rows())
            }
            _ => ext_ui::Overlay::default(),
        }
    }

    /// What a windowed or snapshot run is measuring.
    ///
    /// Both paths report under this: `render` presents to the surface and
    /// `run_snapshot` presents to an offscreen target, but each times the same
    /// two stages, and neither includes redraw application — that happens in
    /// the event pump and is reported on its own.
    fn live_measurement() -> perf::Measurement {
        perf::Measurement {
            mode: "live_session",
            event_source: "nvim_ext_linegrid",
            // A live session is driven by a human and by Neovim's own
            // scheduling; nothing here is replayable.
            workload_deterministic: false,
            gpu_submit_measured: true,
            frame_total_stages: perf::STAGES_LIVE,
            // Frames are drawn when Neovim flushes, not continuously.
            presentation_model: perf::PRESENTATION_ON_DEMAND,
        }
    }

    fn write_perf_report(&mut self) {
        if !self.perf.is_enabled() || self.perf_report_written {
            return;
        }
        self.perf_report_written = true;
        self.perf.set_recording(false);

        // Report the screen actually composited, which a resize may have changed
        // away from the requested geometry.
        let (cols, rows) = self
            .screen
            .as_ref()
            .map(|screen| (screen.cols(), screen.rows()))
            .unwrap_or((self.args.cols, self.args.rows));
        let atlas = self
            .atlas
            .as_ref()
            .map(perf::AtlasSnapshot::of)
            .unwrap_or_else(perf::AtlasSnapshot::absent);

        let report = self.perf.report(
            Self::live_measurement(),
            perf::Environment::observe(self.graphics_probe.clone()),
            perf::Parameters {
                cols,
                rows,
                font_size_px: self.args.font_size,
                frames_requested: None,
                warmup_frames: 0,
                seed: None,
                frame_budget_ms: self.args.perf_frame_budget_ms,
            },
            atlas,
        );
        if let Err(error) = perf::emit(&report, self.args.perf_report.as_deref()) {
            eprintln!("perf report write failed: {error}");
        }
    }

    fn render(&mut self) {
        let overlay = self.overlay();
        // Keep the candidate window under the text cursor — which is the one
        // inside the command line whenever that surface owns it. Outside the
        // frame span: this tells the OS where to put the IME candidate window,
        // which is not a stage of drawing a frame and is not in
        // `measurement.frame_total_stages`.
        if let (Some(w), Some(atlas), Some((row, col))) = (
            self.win.as_ref(),
            self.atlas.as_ref(),
            overlay
                .cursor
                .map(|c| (c.row, c.col))
                .or_else(|| self.screen.as_ref().and_then(screen::Screen::cursor)),
        ) {
            w.set_ime_cursor_area(
                winit::dpi::PhysicalPosition::new(
                    col as f64 * atlas.cell_w as f64,
                    (row + 1) as f64 * atlas.cell_h as f64,
                ),
                winit::dpi::PhysicalSize::new(atlas.cell_w as f64, atlas.cell_h as f64),
            );
        }
        let App {
            gl, renderer, atlas, screen, surface, ctx, win, preedit, images, ext_ui, perf,
            panels, panel_stats, grid_vertices, total_vertices, ..
        } = self;
        let (Some(gl), Some(r), Some(atlas), Some(screen), Some(surface), Some(ctx), Some(win)) = (
            gl.as_ref(),
            renderer.as_mut(),
            atlas.as_mut(),
            screen.as_ref(),
            surface.as_ref(),
            ctx.as_ref(),
            win.as_ref(),
        ) else {
            return;
        };
        let size = win.inner_size();

        let frame = perf.span();
        let build = perf.span();
        r.build(screen, atlas, preedit, ext_ui, &overlay);
        // Read between the passes, not after: the gap between these two counts is
        // the whole evidence that the surfaces went into the grid's own buffer.
        *grid_vertices = r.vertex_count();
        if !panels.is_empty() {
            *panel_stats = r.push_panels(panels, atlas);
        }
        *total_vertices = r.vertex_count();
        perf.record_vertex_build(build);
        let vertices = r.vertex_count();

        // The swap is part of submission: without it the measurement would stop
        // before the driver is asked to present anything.
        let submit = perf.span();
        r.draw(gl, atlas, size.width as i32, size.height as i32, images);
        let _ = surface.swap_buffers(ctx);
        perf.record_gpu_submit(submit);
        perf.record_frame_total(frame);

        // Bookkeeping only, so it stays outside the frame it books.
        perf.record_present(vertices);
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.started {
            return;
        }
        self.started = true;

        let snapshot = self.args.snapshot.is_some();
        // The window has to exist before the glyphs do: cell metrics depend on the
        // display's scale factor, and rasterising at logical pixels on a HiDPI or
        // 4K screen is exactly what makes the text come out too small.
        let attrs = Window::default_attributes()
            .with_title("nvimgl")
            .with_visible(!snapshot)
            .with_inner_size(winit::dpi::LogicalSize::new(900.0, 560.0));

        let (window, gl_config) = DisplayBuilder::new()
            .with_window_attributes(Some(attrs))
            .build(el, ConfigTemplateBuilder::new(), |c| c.last().expect("no GL config"))
            .expect("display build failed");
        let window = window.expect("no window");
        // Without this macOS never routes composition to us and Japanese input is
        // impossible regardless of how the key events are handled.
        window.set_ime_allowed(true);

        let scale = window.scale_factor() as f32;
        let atlas = text::Atlas::new(self.args.font_size * scale);
        let win_w = (atlas.cell_w * self.args.cols as f32).ceil() as u32;
        let win_h = (atlas.cell_h * self.args.rows as f32).ceil() as u32;
        let _ = window.request_inner_size(winit::dpi::PhysicalSize::new(win_w, win_h));
        eprintln!(
            "scale={scale} font={}pt -> {:.1}px  cell={:.1}x{:.1}px  window={win_w}x{win_h}px",
            self.args.font_size,
            self.args.font_size * scale,
            atlas.cell_w,
            atlas.cell_h
        );

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
        let graphics_probe = unsafe {
            platform::GraphicsProbe {
                api_version: glc.get_parameter_string(glow::VERSION),
                renderer: glc.get_parameter_string(glow::RENDERER),
                shading_language_version: glc
                    .get_parameter_string(glow::SHADING_LANGUAGE_VERSION),
            }
        };
        eprintln!(
            "GL {} | {} | GLSL {}",
            graphics_probe.api_version,
            graphics_probe.renderer,
            graphics_probe.shading_language_version
        );

        let renderer = gl::Renderer::new(&glc);
        let mut nv = nvim::Nvim::spawn(&self.args.nvim_args).expect("nvim --embed failed to start");
        let host_channel = nv
            .api_channel_id()
            .expect("nvim_get_api_info did not return the embedded RPC channel");
        nv.ui_attach(self.args.cols as u32, self.args.rows as u32, self.args.ui_options)
            .expect("ui_attach");
        nv.exec_lua_with_args(
            include_str!("../integration/aish.lua"),
            vec![rmpv::Value::from(host_channel)],
        )
        .expect("install read-only aish commands");
        if let Some(code) = self.args.lua.clone() {
            nv.exec_lua(&code).expect("exec_lua");
        }
        let screen = screen::Screen::new(self.args.cols, self.args.rows);

        self.win = Some(window);
        self.ctx = Some(ctx);
        self.surface = Some(surface);
        self.gl = Some(glc);
        self.renderer = Some(renderer);
        self.atlas = Some(atlas);
        self.screen = Some(screen);
        self.nvim = Some(nv);
        self.graphics_probe = Some(graphics_probe);
        self.load_panels();

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
        let flushed = self.screen.as_mut().is_some_and(|screen| {
            let flushed = screen.flushed;
            screen.flushed = false;
            flushed
        });
        if flushed {
            self.write_evaluations();
            if let Some(w) = self.win.as_ref() {
                w.request_redraw();
            }
        }
        el.set_control_flow(ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(8)));
    }

    /// Last point at which the session's observations still exist.
    fn exiting(&mut self, _: &ActiveEventLoop) {
        self.write_perf_report();
    }
}

/// Block for redraw traffic, then decode and apply it, timing only the second
/// half. Returns true when Neovim closed the pipe.
///
/// The ordering here is the entire point of the function. `wait_ready` blocks
/// for up to `timeout` doing nothing; if the span opened before it, every
/// recorded `event_apply_ms` would be idle wait plus work, and a run that spent
/// 60 ms waiting for a keystroke would publish a 60 ms "apply" cost. The span
/// therefore opens after the wait returns, which also makes this measure the
/// same thing [`App::pump`] does — that path drains without blocking, so it
/// never had the problem.
fn wait_and_apply(
    queue: &mut nvim::RedrawQueue,
    screen: Option<&mut screen::Screen>,
    ext_ui: &mut ext_ui::ExtUi,
    perf: &mut perf::Recorder,
    timeout: Duration,
) -> bool {
    let closed_while_waiting = queue.wait_ready(timeout) == nvim::Ready::Closed;

    let batch = perf.span();
    let (events, closed) = queue.drain_redraw();
    if !events.is_empty() {
        if let Some(screen) = screen {
            screen.apply(&events);
        }
        ext_ui.apply(&events);
    }
    if !events.is_empty() {
        perf.record_event_apply(batch);
        perf.record_batch(&events);
    }
    closed || closed_while_waiting
}

impl App {
    /// Wait once for redraw traffic and fold it into both mirrors. Returns true when
    /// Neovim closed the pipe. Measured like the live loop's [`App::pump`], so a
    /// snapshot run observes the same batch costs a session does.
    fn settle(&mut self, timeout: Duration) -> bool {
        let App {
            nvim, screen, ext_ui, perf, ..
        } = self;
        let closed = wait_and_apply(
            nvim.as_mut().unwrap().queue_mut(),
            screen.as_mut(),
            ext_ui,
            perf,
            timeout,
        );
        self.handle_notifications();
        self.handle_aish_results();
        closed
    }

    /// Render one settled frame offscreen and write it as a PNG, so the result
    /// can be checked without a human watching a window.
    fn run_snapshot(&mut self, path: &str, w: u32, h: u32) {
        if let Some(keys) = self.args.input.clone() {
            // Let the initial screen settle before typing into it.
            let deadline = Instant::now() + Duration::from_millis(800);
            while Instant::now() < deadline {
                self.settle(Duration::from_millis(60));
            }
            let _ = self.nvim.as_mut().unwrap().input(&keys);
        }

        let deadline = Instant::now() + Duration::from_millis(1500);
        while Instant::now() < deadline {
            if self.settle(Duration::from_millis(80)) {
                break;
            }
        }
        self.handle_notifications();
        self.handle_aish_results();
        if let Some(p) = self.args.preedit.clone() {
            self.preedit = p;
        }
        self.write_evaluations();

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

            // The frame opens here, not above: creating the offscreen target is
            // what a snapshot does to have somewhere to draw, and charging it to
            // the frame would make `total_ms` mean something no live frame pays.
            let overlay = self.overlay();
            let r = self.renderer.as_mut().unwrap();
            let frame = self.perf.span();
            let build = self.perf.span();
            r.build(
                self.screen.as_ref().unwrap(),
                self.atlas.as_mut().unwrap(),
                &self.preedit,
                &self.ext_ui,
                &overlay,
            );
            self.grid_vertices = r.vertex_count();
            if !self.panels.is_empty() {
                self.panel_stats = r.push_panels(&self.panels, self.atlas.as_mut().unwrap());
            }
            self.total_vertices = r.vertex_count();
            self.perf.record_vertex_build(build);
            let vertices = r.vertex_count();

            // `finish` is what makes this a measurement rather than a queue
            // depth: it does not return until the GPU is done with the frame.
            let submit = self.perf.span();
            r.draw(gl, self.atlas.as_mut().unwrap(), w as i32, h as i32, &self.images);
            gl.finish();
            self.perf.record_gpu_submit(submit);
            self.perf.record_frame_total(frame);
            self.perf.record_present(vertices);

            // Reading the pixels back is what a snapshot does, not what a frame
            // does, so it stays outside every span above.
            let mut buf = vec![0u8; (w * h * 4) as usize];
            gl.read_pixels(
                0, 0, w as i32, h as i32, glow::RGBA, glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(&mut buf),
            );
            buf
        };
        self.write_panel_report();
        self.write_locus_report();
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

fn load_png(path: &str) -> Option<(Vec<u8>, u32, u32)> {
    let f = std::fs::File::open(path).ok()?;
    let mut reader = png::Decoder::new(std::io::BufReader::new(f)).read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    if info.bit_depth != png::BitDepth::Eight {
        return None;
    }
    buf.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf.chunks(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::GrayscaleAlpha => {
            buf.chunks(2).flat_map(|c| [c[0], c[0], c[0], c[1]]).collect()
        }
        png::ColorType::Indexed => return None,
    };
    Some((rgba, info.width, info.height))
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

/// Read the candidate list. Blank lines are dropped; nothing else is
/// interpreted, so a path with spaces or non-ASCII survives intact.
///
/// The built-in fallback is deliberately tiny. A default corpus large enough to
/// look like a measurement would invite reading a smoke run as one.
fn load_picker_corpus(path: Option<&std::path::Path>) -> std::io::Result<Vec<String>> {
    let Some(path) = path else {
        return Ok(vec![
            "alpha.glsl".to_string(),
            "beta.glsl".to_string(),
            "shader/water.vert".to_string(),
            "shader/lighting.frag".to_string(),
            "moving/move_me.glsl".to_string(),
            "README.md".to_string(),
        ]);
    };
    let text = std::fs::read_to_string(path)?;
    Ok(text
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect())
}

fn main() {
    let args = parse_args();

    // The benchmark is headless by construction: it must not touch a window
    // system, a GL context or a Neovim process, so it returns before any of
    // them are created.
    if let Some(frames) = args.perf_bench {
        let report = bench::run(&bench::BenchParams {
            cols: args.cols,
            rows: args.rows,
            font_size_px: args.font_size,
            frames,
            warmup: args.perf_warmup,
            seed: args.perf_seed,
            frame_budget_ms: args.perf_frame_budget_ms,
        });
        if let Err(error) = perf::emit(&report, args.perf_report.as_deref()) {
            eprintln!("perf report write failed: {error}");
            std::process::exit(1);
        }
        return;
    }

    // Host-owned picker state, measured without a window, a GL context or a
    // Neovim process — the same discipline as the perf benchmark above.
    if let Some(script) = args.picker_script.as_deref() {
        let corpus = match load_picker_corpus(args.picker_corpus.as_deref()) {
            Ok(corpus) => corpus,
            Err(error) => {
                eprintln!("picker corpus read failed: {error}");
                std::process::exit(1);
            }
        };
        let report = picker_state::bench(corpus, script, args.picker_visible_rows);
        let json = picker_state::to_json(&report);
        match args.picker_report.as_deref() {
            Some(path) => {
                if let Err(error) = std::fs::write(path, json) {
                    eprintln!("picker report write failed: {error}");
                    std::process::exit(1);
                }
            }
            None => print!("{json}"),
        }
        return;
    }

    let el = EventLoop::new().expect("event loop");
    el.set_control_flow(ControlFlow::Wait);
    let mut app = App::new(args);
    el.run_app(&mut app).expect("run_app");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args_of(argv: &[&str]) -> Args {
        parse_args_from(argv.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn the_picker_measurement_is_off_unless_it_is_asked_for() {
        let a = args_of(&[]);
        assert!(a.picker_script.is_none());
        assert!(a.picker_corpus.is_none());
        assert!(a.picker_report.is_none());
        assert_eq!(a.picker_visible_rows, 12);
    }

    #[test]
    fn the_picker_script_and_its_corpus_are_taken_as_given() {
        let a = args_of(&[
            "--picker-script",
            "al<c-n>",
            "--picker-corpus",
            "/tmp/corpus.txt",
            "--picker-visible-rows",
            "30",
        ]);
        assert_eq!(a.picker_script.as_deref(), Some("al<c-n>"));
        assert_eq!(a.picker_corpus.as_deref(), Some(std::path::Path::new("/tmp/corpus.txt")));
        assert_eq!(a.picker_visible_rows, 30);
    }

    #[test]
    fn a_nonsense_visible_row_count_falls_back_rather_than_dividing_by_zero() {
        assert_eq!(args_of(&["--picker-visible-rows", "0"]).picker_visible_rows, 12);
        assert_eq!(args_of(&["--picker-visible-rows", "nope"]).picker_visible_rows, 12);
    }

    #[test]
    fn the_builtin_corpus_is_small_enough_not_to_look_like_a_measurement() {
        let corpus = load_picker_corpus(None).expect("built-in corpus");
        assert!(corpus.len() < 10, "{} entries", corpus.len());
    }

    #[test]
    fn measurement_is_off_unless_it_is_asked_for() {
        let a = args_of(&[]);
        assert!(!a.perf);
        assert!(!a.perf_enabled());
        assert!(a.perf_report.is_none());
        assert!(a.perf_bench.is_none());
        assert!(a.perf_frame_budget_ms.is_none());
        assert!(!perf::Recorder::new(a.perf_enabled()).is_enabled());
    }

    #[test]
    fn existing_flags_keep_working_alongside_the_new_ones() {
        let a = args_of(&["--cols", "100", "--rows", "40", "--snapshot", "out.png"]);
        assert_eq!((a.cols, a.rows), (100, 40));
        assert_eq!(a.snapshot.as_deref(), Some("out.png"));
        assert!(!a.perf_enabled());
        assert!(a.nvim_args.is_empty());
    }

    #[test]
    fn perf_enables_live_measurement() {
        let a = args_of(&["--perf"]);
        assert!(a.perf && a.perf_enabled());
        assert!(perf::Recorder::new(a.perf_enabled()).is_enabled());
    }

    #[test]
    fn asking_for_a_report_implies_the_measurement_that_fills_it() {
        let a = args_of(&["--perf-report", "/tmp/perf.json"]);
        assert!(!a.perf, "the bare flag was not passed");
        assert!(a.perf_enabled(), "a report with no measurement would be empty");
        assert_eq!(a.perf_report, Some(PathBuf::from("/tmp/perf.json")));
    }

    #[test]
    fn bench_mode_takes_its_frame_count_and_stays_off_by_default() {
        assert!(args_of(&[]).perf_bench.is_none());
        let a = args_of(&["--perf-bench", "250"]);
        assert_eq!(a.perf_bench, Some(250));
    }

    #[test]
    fn bench_parameters_have_stable_defaults_and_are_overridable() {
        let a = args_of(&["--perf-bench", "10"]);
        assert_eq!(a.perf_warmup, 0, "no frames are silently discarded by default");
        assert_eq!(a.perf_seed, 1);

        let a = args_of(&["--perf-bench", "10", "--perf-warmup", "5", "--perf-seed", "99"]);
        assert_eq!((a.perf_warmup, a.perf_seed), (5, 99));
    }

    #[test]
    fn a_frame_budget_exists_only_when_the_caller_supplies_one() {
        assert!(args_of(&["--perf-bench", "10"]).perf_frame_budget_ms.is_none());
        let a = args_of(&["--perf-frame-budget-ms", "16.67"]);
        assert_eq!(a.perf_frame_budget_ms, Some(16.67));
    }

    #[test]
    fn an_unparsable_value_leaves_the_setting_unobserved() {
        // Better an absent budget than one the user did not choose.
        assert!(args_of(&["--perf-frame-budget-ms", "soon"]).perf_frame_budget_ms.is_none());
        assert!(args_of(&["--perf-bench", "lots"]).perf_bench.is_none());
    }

    #[test]
    fn a_flag_missing_its_value_leaves_that_setting_unobserved() {
        // A trailing flag with nothing after it must not panic, and must not
        // invent a value either: the setting stays exactly as unset as it was.
        let defaults = args_of(&[]);
        for argv in [
            vec!["--perf-report"],
            vec!["--perf-bench"],
            vec!["--perf-warmup"],
            vec!["--perf-seed"],
            vec!["--perf-frame-budget-ms"],
        ] {
            let a = args_of(&argv);
            assert!(a.perf_report.is_none(), "{argv:?} invented a report path");
            assert!(a.perf_bench.is_none(), "{argv:?} invented a frame count");
            assert!(
                a.perf_frame_budget_ms.is_none(),
                "{argv:?} invented a budget"
            );
            assert_eq!(a.perf_warmup, defaults.perf_warmup, "{argv:?}");
            assert_eq!(a.perf_seed, defaults.perf_seed, "{argv:?}");
            // The dangling flag is consumed by us, never handed to Neovim.
            assert!(a.nvim_args.is_empty(), "{argv:?} leaked a flag to nvim");
        }
    }

    // --- The instrumentation boundary --------------------------------------

    fn redraw_message(name: &str) -> rmpv::Value {
        rmpv::Value::Array(vec![
            rmpv::Value::from(2u8),
            rmpv::Value::from("redraw"),
            rmpv::Value::Array(vec![rmpv::Value::Array(vec![
                rmpv::Value::from(name),
                rmpv::Value::Array(vec![
                    rmpv::Value::from(1u64),
                    rmpv::Value::from(0u64),
                    rmpv::Value::from(0u64),
                ]),
            ])]),
        ])
    }

    fn apply_report(recorder: &perf::Recorder) -> perf::Report {
        recorder.report(
            App::live_measurement(),
            perf::Environment::observe(None),
            perf::Parameters {
                cols: 8,
                rows: 4,
                font_size_px: 15.0,
                frames_requested: None,
                warmup_frames: 0,
                seed: None,
                frame_budget_ms: None,
            },
            perf::AtlasSnapshot::absent(),
        )
    }

    /// The defect this guards: the span used to open *before* the blocking
    /// wait, so `event_apply_ms` reported however long Neovim stayed quiet.
    #[test]
    fn waiting_for_neovim_is_not_charged_to_applying_its_events() {
        const IDLE: Duration = Duration::from_millis(250);

        // The test holds `tx` for the whole call, so the pipe is genuinely open
        // throughout: a `closed` verdict below would be a real one, not the
        // sender thread having finished and hung up.
        let (tx, rx) = std::sync::mpsc::channel();
        let late = tx.clone();
        let sender = std::thread::spawn(move || {
            std::thread::sleep(IDLE);
            let _ = late.send(redraw_message("grid_cursor_goto"));
        });

        let mut queue = nvim::RedrawQueue::new(rx);
        let mut recorder = perf::Recorder::enabled();
        let mut g = screen::Screen::new(8, 4);

        let waited = Instant::now();
        let closed = wait_and_apply(
            &mut queue,
            Some(&mut g),
            &mut ext_ui::ExtUi::new(),
            &mut recorder,
            Duration::from_secs(5),
        );
        let wall = waited.elapsed();
        sender.join().unwrap();
        drop(tx);

        assert!(!closed, "the channel was still open");
        assert!(
            wall >= IDLE,
            "the call did not actually block, so it proves nothing"
        );

        let apply = apply_report(&recorder)
            .frame
            .event_apply_ms
            .expect("the batch was applied and timed");
        assert_eq!(apply.count, 1);
        assert!(
            apply.max < IDLE.as_secs_f64() * 1000.0 / 10.0,
            "applying one cursor move was recorded as {} ms after a {} ms idle \
             wait; the idle time is being measured as work",
            apply.max,
            IDLE.as_millis()
        );
    }

    #[test]
    fn a_wait_that_timed_out_records_nothing_at_all() {
        let (_tx, rx) = std::sync::mpsc::channel::<rmpv::Value>();
        let mut queue = nvim::RedrawQueue::new(rx);
        let mut recorder = perf::Recorder::enabled();
        let mut g = screen::Screen::new(8, 4);

        let closed = wait_and_apply(
            &mut queue,
            Some(&mut g),
            &mut ext_ui::ExtUi::new(),
            &mut recorder,
            Duration::from_millis(20),
        );

        assert!(!closed, "a quiet channel is not a closed one");
        let report = apply_report(&recorder);
        assert!(
            report.frame.event_apply_ms.is_none(),
            "an idle poll is not a batch and must not enter the distribution"
        );
        assert_eq!(report.redraw.batches, 0);
    }

    #[test]
    fn the_message_that_ended_the_wait_is_still_applied() {
        // Buffering the raw message to move the span must not lose it.
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(redraw_message("grid_cursor_goto")).unwrap();
        tx.send(redraw_message("flush")).unwrap();

        let mut queue = nvim::RedrawQueue::new(rx);
        let mut recorder = perf::Recorder::enabled();
        let mut g = screen::Screen::new(8, 4);

        assert!(!wait_and_apply(
            &mut queue,
            Some(&mut g),
            &mut ext_ui::ExtUi::new(),
            &mut recorder,
            Duration::from_secs(1),
        ));

        let report = apply_report(&recorder);
        assert_eq!(report.redraw.batches, 1, "one drain is one batch");
        assert_eq!(
            report.redraw.events_total, 2,
            "the buffered message and the one behind it both arrived"
        );
        assert_eq!(report.redraw.events_by_kind["grid_cursor_goto"], 1);
        assert_eq!(report.redraw.events_by_kind["flush"], 1);
    }

    #[test]
    fn a_closed_pipe_is_reported_however_it_was_noticed() {
        let (tx, rx) = std::sync::mpsc::channel::<rmpv::Value>();
        drop(tx);
        let mut queue = nvim::RedrawQueue::new(rx);
        let mut recorder = perf::Recorder::disabled();

        assert!(wait_and_apply(
            &mut queue,
            None,
            &mut ext_ui::ExtUi::new(),
            &mut recorder,
            Duration::from_millis(50),
        ));
    }

    #[test]
    fn the_live_report_declares_the_stages_its_frame_total_covers() {
        let m = App::live_measurement();
        assert_eq!(m.frame_total_stages, perf::STAGES_LIVE);
        assert_eq!(m.presentation_model, perf::PRESENTATION_ON_DEMAND);
        // Redraw application happens in the event pump, in a different call
        // from the one the frame span wraps. Claiming it here would make a live
        // `total_ms` look like a headless one and mean something else.
        assert!(!m.frame_total_stages.contains(&"event_apply"));
        assert!(m.gpu_submit_measured);
        assert!(m.frame_total_stages.contains(&"gpu_submit"));
    }

    #[test]
    fn perf_flags_are_not_forwarded_to_neovim() {
        let a = args_of(&[
            "--perf", "--perf-bench", "8", "--perf-seed", "3",
            "--perf-frame-budget-ms", "16.6", "--", "-u", "NONE",
        ]);
        assert_eq!(a.nvim_args, vec!["-u".to_string(), "NONE".to_string()]);
        assert_eq!(a.perf_bench, Some(8));
    }
}
