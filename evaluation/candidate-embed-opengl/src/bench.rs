//! A deterministic, headless, repeatable measurement mode.
//!
//! The workload is a scripted stream of `redraw` events generated from a seed,
//! so the same `(seed, cols, rows, frames)` always drives the renderer through
//! the same work. That is where the determinism lives — and where it stops.
//! The *timings* are fresh observations of this machine on this run, and the
//! report says so rather than pretending a benchmark can reproduce a duration.
//!
//! No window, no GL context and no Neovim process are involved, so this runs
//! anywhere the crate builds. The code under measurement is not a stand-in: it
//! is [`crate::grid::Grid::apply`] and [`crate::gl::build_vertices`], the same
//! functions the live window calls.

use rmpv::Value;

use crate::gl::{self, VERTEX_FLOATS};
use crate::grid::Grid;
use crate::nvim::RedrawEvent;
use crate::perf::{self, AtlasSnapshot, Environment, Measurement, Parameters, Recorder};
use crate::text::Atlas;

/// Characters the synthetic screen is filled from. Deliberately wider than
/// ASCII: CJK and box-drawing glyphs are what make the atlas miss, and a
/// benchmark that only ever typed `a` would report a cache that never works.
const CHARSET: &str = "abcdefghijklmnopqrstuvwxyz\
ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 .,;:(){}[]<>+-*/=_|&#@!?\"'`~^%$\
日本語描画性能測定→←↑↓─│┌┐└┘█▓▒░";

/// The highlight ids the prologue defines, plus 0 (the default).
const HL_IDS: u64 = 5;

pub struct BenchParams {
    pub cols: usize,
    pub rows: usize,
    pub font_size_px: f32,
    pub frames: u64,
    pub warmup: u64,
    pub seed: u64,
    pub frame_budget_ms: Option<f64>,
}

/// A small linear congruential generator, so the workload depends on the seed
/// alone: no clock, no address, no thread scheduling.
struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        // Any odd offset works; seeding through one step avoids a degenerate
        // first draw for seed 0.
        Self(
            seed.wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407),
        )
    }

    fn next_u32(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) as u32
    }

    fn below(&mut self, bound: usize) -> usize {
        if bound == 0 {
            0
        } else {
            self.next_u32() as usize % bound
        }
    }
}

/// The scripted event source. Reproducible from `(seed, cols, rows)` and the
/// number of frames drawn so far.
pub struct Workload {
    rng: Lcg,
    cols: usize,
    rows: usize,
    charset: Vec<char>,
    frame: u64,
}

impl Workload {
    pub fn new(seed: u64, cols: usize, rows: usize) -> Self {
        Self {
            rng: Lcg::new(seed),
            cols: cols.max(1),
            rows: rows.max(1),
            charset: CHARSET.chars().collect(),
            frame: 0,
        }
    }

    /// Grid geometry, default colours and a handful of highlight groups. Sent
    /// once, exactly as Neovim does on attach.
    pub fn prologue(&self) -> Vec<RedrawEvent> {
        let mut events = vec![
            (
                "grid_resize".to_string(),
                vec![
                    Value::from(1u64),
                    Value::from(self.cols as u64),
                    Value::from(self.rows as u64),
                ],
            ),
            (
                "default_colors_set".to_string(),
                vec![
                    Value::from(0xd0d0d0u64),
                    Value::from(0x101014u64),
                    Value::from(0u64),
                ],
            ),
        ];
        let attrs: [(u64, &[(&str, Value)]); 4] = [
            (1, &[("foreground", Value::from(0xff8080u32))]),
            (2, &[("bold", Value::from(true))]),
            (
                3,
                &[
                    ("italic", Value::from(true)),
                    ("underline", Value::from(true)),
                ],
            ),
            (4, &[("reverse", Value::from(true))]),
        ];
        for (id, pairs) in attrs {
            let map = pairs
                .iter()
                .map(|(k, v)| (Value::from(*k), v.clone()))
                .collect::<Vec<_>>();
            events.push((
                "hl_attr_define".to_string(),
                vec![
                    Value::from(id),
                    Value::Map(map),
                    Value::Map(vec![]),
                    Value::Array(vec![]),
                ],
            ));
        }
        events
    }

    /// One frame's worth of traffic, ending in the `flush` that tells the UI the
    /// frame is complete.
    pub fn next_frame(&mut self) -> Vec<RedrawEvent> {
        let mut events = Vec::new();

        // Every fourth frame scrolls, which is the cheap-events/expensive-repaint
        // case that a per-event cost alone would misrepresent.
        if self.frame % 4 == 3 {
            events.push((
                "grid_scroll".to_string(),
                vec![
                    Value::from(1u64),
                    Value::from(0u64),
                    Value::from(self.rows as u64),
                    Value::from(0u64),
                    Value::from(self.cols as u64),
                    Value::from(1i64),
                    Value::from(0i64),
                ],
            ));
        }

        let lines = 1 + self.rng.below(self.rows);
        for _ in 0..lines {
            events.push(self.line_event());
        }

        let (row, col) = (self.rng.below(self.rows), self.rng.below(self.cols));
        events.push((
            "grid_cursor_goto".to_string(),
            vec![
                Value::from(1u64),
                Value::from(row as u64),
                Value::from(col as u64),
            ],
        ));
        events.push(("flush".to_string(), vec![]));

        self.frame += 1;
        events
    }

    fn line_event(&mut self) -> RedrawEvent {
        let row = self.rng.below(self.rows);
        let col0 = self.rng.below(self.cols);
        let span = 1 + self.rng.below((self.cols - col0).max(1));
        let mut cells = Vec::with_capacity(span);
        for _ in 0..span {
            let ch = self.charset[self.rng.below(self.charset.len())];
            let hl = self.rng.below(HL_IDS as usize) as u64;
            cells.push(Value::Array(vec![
                Value::from(ch.to_string()),
                Value::from(hl),
            ]));
        }
        (
            "grid_line".to_string(),
            vec![
                Value::from(1u64),
                Value::from(row as u64),
                Value::from(col0 as u64),
                Value::Array(cells),
                Value::from(false),
            ],
        )
    }
}

fn measurement() -> Measurement {
    Measurement {
        mode: "headless_benchmark",
        event_source: "synthetic_deterministic_script",
        workload_deterministic: true,
        // No GL context exists here, so GPU submission is not on this path and
        // its figures stay absent rather than being reported as zero.
        gpu_submit_measured: false,
    }
}

/// Drive one frame through the real apply/build path, timing each stage.
fn run_frame(
    recorder: &mut Recorder,
    grid: &mut Grid,
    atlas: &mut Atlas,
    verts: &mut Vec<f32>,
    events: &[RedrawEvent],
) {
    let frame = recorder.span();

    let apply = recorder.span();
    grid.apply(events);
    recorder.record_event_apply(apply);
    recorder.record_batch(events);

    let build = recorder.span();
    gl::build_vertices(verts, grid, atlas, "");
    recorder.record_vertex_build(build);

    recorder.record_present(verts.len() / VERTEX_FLOATS);
    recorder.record_frame_total(frame);
}

/// Run the benchmark and return what was observed.
///
/// Panics only if no usable font exists, which is the same condition that stops
/// the application itself from starting.
pub fn run(params: &BenchParams) -> perf::Report {
    assert!(
        crate::text::font_available(),
        "no usable font on this host; set NVIMGL_FONT_PATHS to a font file"
    );
    let mut grid = Grid::new(params.cols, params.rows);
    let mut atlas = Atlas::new(params.font_size_px);
    let mut verts: Vec<f32> = Vec::new();
    let mut workload = Workload::new(params.seed, params.cols, params.rows);
    let mut recorder = Recorder::enabled();

    // The prologue and the warm-up frames populate the grid, the glyph cache and
    // the vertex buffer's capacity. They are executed, not measured: their cost
    // is a first-frame cost and reporting it inside a steady-state distribution
    // would misattribute it.
    recorder.set_recording(false);
    let prologue = workload.prologue();
    grid.apply(&prologue);
    for _ in 0..params.warmup {
        let events = workload.next_frame();
        run_frame(&mut recorder, &mut grid, &mut atlas, &mut verts, &events);
        recorder.note_warmup_frame();
    }

    recorder.set_recording(true);
    for _ in 0..params.frames {
        let events = workload.next_frame();
        run_frame(&mut recorder, &mut grid, &mut atlas, &mut verts, &events);
    }
    recorder.set_recording(false);

    recorder.report(
        measurement(),
        Environment::observe(None),
        Parameters {
            cols: params.cols,
            rows: params.rows,
            font_size_px: params.font_size_px,
            frames_requested: Some(params.frames),
            warmup_frames: params.warmup,
            seed: Some(params.seed),
            frame_budget_ms: params.frame_budget_ms,
        },
        AtlasSnapshot::of(&atlas),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(frames: u64, warmup: u64) -> BenchParams {
        BenchParams {
            cols: 40,
            rows: 12,
            font_size_px: 14.0,
            frames,
            warmup,
            seed: 7,
            frame_budget_ms: None,
        }
    }

    fn frames_of(seed: u64, count: usize) -> Vec<Vec<RedrawEvent>> {
        let mut workload = Workload::new(seed, 40, 12);
        (0..count).map(|_| workload.next_frame()).collect()
    }

    #[test]
    fn the_same_seed_replays_the_same_workload() {
        assert_eq!(frames_of(11, 8), frames_of(11, 8));
        assert_eq!(
            Workload::new(11, 40, 12).prologue(),
            Workload::new(11, 40, 12).prologue()
        );
    }

    #[test]
    fn a_different_seed_drives_different_work() {
        assert_ne!(frames_of(11, 8), frames_of(12, 8));
    }

    #[test]
    fn every_frame_ends_with_the_flush_that_declares_it_complete() {
        for frame in frames_of(3, 6) {
            assert_eq!(frame.last().unwrap().0, "flush");
            assert_eq!(
                frame.iter().filter(|(name, _)| name == "flush").count(),
                1,
                "a frame must declare completion exactly once"
            );
        }
    }

    #[test]
    fn the_workload_stays_inside_the_grid_it_declared() {
        let (cols, rows) = (40usize, 12usize);
        let mut workload = Workload::new(5, cols, rows);
        for _ in 0..20 {
            for (name, args) in workload.next_frame() {
                if name != "grid_line" {
                    continue;
                }
                let row = args[1].as_u64().unwrap() as usize;
                let col0 = args[2].as_u64().unwrap() as usize;
                let span = args[3].as_array().unwrap().len();
                assert!(row < rows, "row {row} is outside {rows}");
                assert!(col0 < cols, "col {col0} is outside {cols}");
                assert!(col0 + span <= cols, "line runs past the right edge");
            }
        }
    }

    #[test]
    fn the_workload_scrolls_and_moves_the_cursor_not_only_paints() {
        let kinds: Vec<String> = frames_of(2, 12)
            .into_iter()
            .flatten()
            .map(|(name, _)| name)
            .collect();
        for expected in ["grid_line", "grid_scroll", "grid_cursor_goto", "flush"] {
            assert!(
                kinds.iter().any(|k| k == expected),
                "workload never emitted `{expected}`"
            );
        }
    }

    #[test]
    fn a_headless_run_observes_every_frame_it_was_asked_for() {
        if !crate::text::font_available() {
            eprintln!("skipped: no usable font on this host");
            return;
        }
        let report = run(&params(12, 3));

        assert_eq!(report.frame.frames_recorded, 12);
        assert_eq!(report.frame.warmup_frames_excluded, 3);
        assert_eq!(report.parameters.frames_requested, Some(12));
        assert_eq!(report.slow_frames.presentations_observed, 12);

        // Timings exist and are real elapsed durations.
        let total = report.frame.total_ms.expect("frames were timed");
        assert_eq!(total.count, 12);
        assert!(total.max >= total.p50 && total.p50 >= total.min);
        assert!(report.frame.vertex_build_ms.is_some());
        assert!(report.frame.event_apply_ms.is_some());

        // The GPU was never on this path.
        assert!(report.frame.gpu_submit_ms.is_none());
        assert!(!report.measurement.gpu_submit_measured);
    }

    #[test]
    fn a_headless_run_exercises_the_grid_and_the_glyph_cache_for_real() {
        if !crate::text::font_available() {
            eprintln!("skipped: no usable font on this host");
            return;
        }
        let report = run(&params(10, 2));

        // Vertices: at minimum one background quad per cell, six vertices each.
        let per_frame = report.vertices.per_frame.expect("vertices were counted");
        assert!(
            per_frame.min >= (40 * 12 * 6) as f64,
            "expected at least one background quad per cell, saw {}",
            per_frame.min
        );
        assert_eq!(per_frame.count, 10);
        assert!(report.vertices.total >= per_frame.min as u64 * per_frame.count);

        // The cache was consulted, and warm-up means most of it hits.
        let atlas = &report.glyph_atlas;
        assert!(atlas.counters.lookups > 0);
        assert_eq!(
            atlas.counters.hits + atlas.counters.misses,
            atlas.counters.lookups
        );
        assert!(
            atlas.counters.rasterizations > 0,
            "no glyph was ever rasterised"
        );
        assert!(atlas.hit_ratio.unwrap() > 0.0);
        assert!(atlas.cached_entries > 0);

        // Redraw traffic was attributed by kind.
        assert!(report.redraw.batches == 10);
        assert!(report.redraw.events_by_kind["grid_line"] > 0);
        assert_eq!(report.redraw.events_by_kind["flush"], 10);
    }

    #[test]
    fn a_zero_frame_run_reports_nothing_rather_than_zeroes() {
        if !crate::text::font_available() {
            eprintln!("skipped: no usable font on this host");
            return;
        }
        let report = run(&params(0, 0));
        assert_eq!(report.frame.frames_recorded, 0);
        assert!(report.frame.total_ms.is_none());
        assert!(report.vertices.per_frame.is_none());
        assert_eq!(report.slow_frames.criterion, perf::CRITERION_UNSET);
    }
}
