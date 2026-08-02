//! Runtime performance observation for the rendering path.
//!
//! Everything in this module reports what was *measured during this process's
//! own execution*. There are no targets, no thresholds, no pass/fail verdicts
//! and no defaults standing in for a measurement that did not happen: a
//! quantity that was not observed is serialised as `null`, never as a number.
//!
//! The domain spec quarantines performance criteria (`performance_criteria`,
//! `performance.numeric_targets`, `performance_acceptance`) as undetermined and
//! awaiting a human gate. This module therefore produces *evidence for* that
//! gate and deliberately does not anticipate it: the only threshold that can
//! ever appear in a report is one the caller supplied on the command line, and
//! when none is supplied the slow-frame section says so in words rather than
//! inventing one.
//!
//! Overhead: the timing instrumentation is off unless explicitly enabled. When
//! disabled, [`Recorder::span`] returns `None` without reading the clock and
//! every `record_*` call returns after a single boolean test, so the normal
//! rendering path pays a predictable branch and nothing else.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use serde::Serialize;
use ulid::Ulid;

use crate::nvim::RedrawEvent;
use crate::platform::GraphicsProbe;
use crate::text::AtlasStats;

/// Bumped from `v1` when the report stopped calling a presentation count a
/// frame rate and started naming the stages `frame.total_ms` covers. A reader
/// that knows only `v1` would read the same field names as different quantities,
/// so the version has to move with them.
pub const SCHEMA: &str = "nvimgl.perf-observation/v2";

/// Reported figures are rounded to this many decimal places (milliseconds, so
/// this is 0.1 µs). Rounding is presentation only; comparisons and percentiles
/// run on the unrounded observations.
const DECIMALS: u32 = 4;

fn round(value: f64) -> f64 {
    let scale = 10f64.powi(DECIMALS as i32);
    (value * scale).round() / scale
}

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

/// A retained set of observations of one quantity.
///
/// Every sample is kept rather than folded into a running estimate, so the
/// reported percentiles are exact for the run instead of approximations whose
/// error would itself need justifying.
#[derive(Default, Debug)]
pub struct Samples {
    values: Vec<f64>,
}

impl Samples {
    pub fn push(&mut self, value: f64) {
        self.values.push(value);
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// `None` when nothing was observed — an empty run has no distribution, and
    /// zeros would read as "measured and fast".
    pub fn summary(&self) -> Option<Summary> {
        if self.is_empty() {
            return None;
        }
        let mut sorted = self.values.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).expect("observations are never NaN"));
        let sum: f64 = sorted.iter().sum();
        Some(Summary {
            count: sorted.len() as u64,
            min: round(sorted[0]),
            mean: round(sum / sorted.len() as f64),
            p50: round(percentile(&sorted, 50.0)),
            p90: round(percentile(&sorted, 90.0)),
            p95: round(percentile(&sorted, 95.0)),
            p99: round(percentile(&sorted, 99.0)),
            max: round(sorted[sorted.len() - 1]),
        })
    }

    /// How many observations strictly exceeded `threshold`, and by how much the
    /// worst one did. The threshold is always caller-supplied.
    ///
    /// The overrun is `None` when nothing exceeded the threshold: a run with no
    /// overrun has no worst overrun, and reporting `0.0` would claim one was
    /// observed and measured at zero.
    fn exceedances(&self, threshold: f64) -> (u64, Option<f64>) {
        let mut count = 0;
        let mut worst: Option<f64> = None;
        for &value in &self.values {
            if value > threshold {
                count += 1;
                let overrun = value - threshold;
                worst = Some(worst.map_or(overrun, |w: f64| w.max(overrun)));
            }
        }
        (count, worst)
    }
}

/// Nearest-rank percentile on an ascending slice: the smallest observation at
/// or above the `p`-th position. Every returned value is an observation that
/// actually happened, never an interpolation between two that did not.
fn percentile(sorted: &[f64], p: f64) -> f64 {
    debug_assert!(!sorted.is_empty());
    let rank = (p / 100.0 * sorted.len() as f64).ceil().max(1.0) as usize;
    sorted[rank.min(sorted.len()) - 1]
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Summary {
    pub count: u64,
    pub min: f64,
    pub mean: f64,
    pub p50: f64,
    pub p90: f64,
    pub p95: f64,
    pub p99: f64,
    pub max: f64,
}

/// Stages that `frame.total_ms` spans in a headless benchmark. Redraw
/// application and vertex building happen in the same call there, so both are
/// inside the frame; there is no GL context, so nothing is submitted.
pub const STAGES_HEADLESS: &[&str] = &["event_apply", "vertex_build"];

/// Stages that `frame.total_ms` spans in a live or snapshot session. Redraw
/// application happens in a *different* call (the event pump), so it is not in
/// this total; it is reported on its own as `frame.event_apply_ms`.
pub const STAGES_LIVE: &[&str] = &["vertex_build", "gpu_submit"];

/// A renderer that only draws when Neovim says something changed. Presentations
/// divided by wall clock then measures how much happened, not how fast the
/// renderer can go.
pub const PRESENTATION_ON_DEMAND: &str = "on_demand_redraw";

/// A loop that presents as fast as it can, with nothing throttling it.
pub const PRESENTATION_UNTHROTTLED: &str = "unthrottled_loop";

/// Where the events being measured came from, and which stages were reachable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Measurement {
    /// `"headless_benchmark"` or `"live_session"`.
    pub mode: &'static str,
    /// `"synthetic_deterministic_script"` or `"nvim_ext_linegrid"`.
    pub event_source: &'static str,
    /// True when re-running with the same parameters replays the same workload.
    /// It never claims the *timings* repeat: those are fresh observations.
    pub workload_deterministic: bool,
    /// Whether GPU submission was on the measured path at all. Headless runs
    /// have no context, and report `null` for those figures rather than 0.
    pub gpu_submit_measured: bool,
    /// Exactly which stages `frame.total_ms` covers, in order.
    ///
    /// The two modes do not compose a frame the same way, and the field name
    /// alone cannot say so. Without this, `total_ms` from a headless run and
    /// `total_ms` from a live run look comparable under one schema and are not:
    /// see [`STAGES_HEADLESS`] and [`STAGES_LIVE`]. Anything derived from
    /// `total_ms` — `slow_frames.frames_over_budget` above all — classifies
    /// whatever this list says, and nothing else.
    pub frame_total_stages: &'static [&'static str],
    /// How presentations were paced. Both presentation figures in `frame` are
    /// readable only against this: under [`PRESENTATION_ON_DEMAND`] they measure
    /// how much redrawing was asked for, not what the renderer could achieve.
    /// Neither is named as a frame rate, for that reason.
    pub presentation_model: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct Environment {
    pub os: &'static str,
    pub architecture: &'static str,
    pub family: &'static str,
    pub pointer_width: u32,
    pub available_parallelism: Option<usize>,
    /// A debug build's numbers describe a debug build. Recording this stops a
    /// report from being read as if it came from an optimised one.
    pub debug_assertions: bool,
    pub graphics: Option<GraphicsProbe>,
}

impl Environment {
    pub fn observe(graphics: Option<GraphicsProbe>) -> Self {
        Self {
            os: std::env::consts::OS,
            architecture: std::env::consts::ARCH,
            family: std::env::consts::FAMILY,
            pointer_width: usize::BITS,
            available_parallelism: std::thread::available_parallelism().ok().map(Into::into),
            debug_assertions: cfg!(debug_assertions),
            graphics,
        }
    }
}

/// The knobs the run was given. Reproducing a report means reproducing these.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Parameters {
    pub cols: usize,
    pub rows: usize,
    pub font_size_px: f32,
    pub frames_requested: Option<u64>,
    pub warmup_frames: u64,
    pub seed: Option<u64>,
    /// Only ever what the caller passed with `--perf-frame-budget-ms`.
    pub frame_budget_ms: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FrameSection {
    pub frames_recorded: u64,
    pub warmup_frames_excluded: u64,
    pub wall_clock_ms: Option<f64>,
    pub total_ms: Option<Summary>,
    /// Cost of folding one drained batch of `redraw` events into the grid.
    ///
    /// The blocking wait for that traffic is never inside this span; the wait
    /// ends first and the span opens after it (`wait_and_apply` in `main.rs`,
    /// and the pump drains without blocking at all). What *is* inside differs
    /// slightly by path and
    /// cannot: headless applies events the benchmark already holds, while a live
    /// session also turns the received msgpack values into events first. Both
    /// measure the work of taking a batch and having applied it, neither
    /// measures waiting for one.
    pub event_apply_ms: Option<Summary>,
    pub vertex_build_ms: Option<Summary>,
    pub gpu_submit_ms: Option<Summary>,
    pub present_interval_ms: Option<Summary>,
    /// Distribution of 1000/interval over the observed presentation intervals.
    ///
    /// Named for what it counts rather than as an FPS figure: under
    /// [`PRESENTATION_ON_DEMAND`] the interval is set by how long Neovim stayed
    /// quiet, so this is the rate at which frames were *asked for*, and it is a
    /// rate the renderer sustained only under [`PRESENTATION_UNTHROTTLED`].
    pub presentation_rate_hz: Option<Summary>,
    /// Presentations divided by the measured wall clock of the recorded window.
    ///
    /// Read with `measurement.presentation_model`, and read the name literally:
    /// this is a count over a duration. Calling it a mean frame rate would claim
    /// the idle stretches of an on-demand session were the renderer failing to
    /// keep up, which is not what was observed.
    pub presentations_per_wall_clock_second: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RedrawSection {
    pub batches: u64,
    pub events_total: u64,
    pub events_per_batch: Option<Summary>,
    /// The same observations as `frame.event_apply_ms`, not a second
    /// measurement of them. Applying a drained batch *is* the frame's event
    /// apply stage; the figure appears in both sections because each is
    /// readable on its own, and the two can never disagree.
    pub batch_apply_ms: Option<Summary>,
    /// Observed count per `redraw` event name, so an expensive frame can be
    /// attributed to the traffic that caused it.
    pub events_by_kind: BTreeMap<String, u64>,
}

/// Counters cover the whole process, not the recorded frame window: the atlas
/// is a cache whose state at report time is the thing worth knowing, and
/// splitting it by window would report a hit ratio for a cache half of whose
/// fills are hidden.
pub const ATLAS_WINDOW: &str = "process_lifetime_including_warmup";

#[derive(Debug, Clone, Serialize)]
pub struct AtlasSection {
    /// Which observations the counters below cover. Deliberately *not* the
    /// frame window `warmup_frames_excluded` describes: see [`ATLAS_WINDOW`].
    pub counters_window: &'static str,
    #[serde(flatten)]
    pub counters: AtlasStats,
    /// Texture uploads, or `null` when there was no GPU on the measured path to
    /// upload to. A headless run cannot reach that code, and reporting `0`
    /// would claim it ran and never uploaded.
    pub uploads: Option<u64>,
    /// `null` before any lookup: a ratio over zero lookups is not an observation.
    pub hit_ratio: Option<f64>,
    pub cached_entries: u64,
    pub atlas_side_px: u64,
    pub packed_height_px: u64,
    pub packed_height_ratio: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct VertexSection {
    pub per_frame: Option<Summary>,
    pub total: u64,
}

/// Frames that ran long, and frames that were coalesced away.
///
/// The two are reported separately because only one of them is threshold-free.
/// `flushes_not_presented` is a pure observation: Neovim said "frame complete"
/// N times and the renderer presented M ≤ N times, so N − M completed frames
/// never reached the screen as distinct presentations. `frames_over_budget`
/// needs a budget, and a budget is not something this code is entitled to pick.
#[derive(Debug, Clone, Serialize)]
pub struct SlowFrameSection {
    pub criterion: &'static str,
    pub frame_budget_ms: Option<f64>,
    pub frames_over_budget: Option<u64>,
    pub frames_over_budget_ratio: Option<f64>,
    pub worst_overrun_ms: Option<f64>,
    pub flushes_observed: u64,
    pub presentations_observed: u64,
    pub flushes_not_presented: u64,
}

pub const CRITERION_UNSET: &str = "unset_awaiting_human_gate";
pub const CRITERION_SUPPLIED: &str = "caller_supplied_frame_budget_ms";

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub schema: &'static str,
    pub trace_id: String,
    pub evaluation_candidate: bool,
    /// This candidate is not the canonical stack, and these numbers are not
    /// acceptance criteria for anything.
    pub canonical_performance_criteria: bool,
    pub performance_acceptance: &'static str,
    pub measurement: Measurement,
    pub environment: Environment,
    pub parameters: Parameters,
    pub frame: FrameSection,
    pub redraw: RedrawSection,
    pub glyph_atlas: AtlasSection,
    pub vertices: VertexSection,
    pub slow_frames: SlowFrameSection,
}

/// Collects observations. Disabled by default; see the module docs on overhead.
#[derive(Default)]
pub struct Recorder {
    enabled: bool,
    /// Cleared during warm-up so early frames influence cache state without
    /// entering the distribution.
    recording: bool,
    warmup_excluded: u64,
    recording_started: Option<Instant>,
    recording_ended: Option<Instant>,
    last_present: Option<Instant>,

    frame_total: Samples,
    event_apply: Samples,
    vertex_build: Samples,
    gpu_submit: Samples,
    present_interval: Samples,
    events_per_batch: Samples,
    vertices_per_frame: Samples,

    batches: u64,
    events_total: u64,
    events_by_kind: BTreeMap<String, u64>,
    flushes: u64,
    presentations: u64,
    vertices_total: u64,
}

impl Recorder {
    /// A recorder that observes nothing at all. This is what the normal
    /// rendering path gets unless the user asked for measurement.
    pub fn disabled() -> Self {
        Self::default()
    }

    pub fn enabled() -> Self {
        Self {
            enabled: true,
            recording: true,
            ..Self::default()
        }
    }

    pub fn new(enabled: bool) -> Self {
        if enabled {
            Self::enabled()
        } else {
            Self::disabled()
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Exclude subsequent frames from the distribution while still running them.
    pub fn set_recording(&mut self, recording: bool) {
        if !self.enabled {
            return;
        }
        if self.recording && !recording {
            self.recording_ended = Some(Instant::now());
        }
        self.recording = recording;
        if recording {
            self.recording_started.get_or_insert_with(Instant::now);
            self.recording_ended = None;
        }
    }

    pub fn note_warmup_frame(&mut self) {
        self.warmup_excluded += 1;
    }

    /// Start of a timed span, or `None` when measurement is off. No clock is
    /// read in the disabled case.
    #[inline]
    pub fn span(&self) -> Option<Instant> {
        if self.enabled {
            Some(Instant::now())
        } else {
            None
        }
    }

    /// The recorded window opens at the first observation, not at construction.
    /// A live session builds its recorder before Neovim and the GL context
    /// exist, and charging that startup to the frame rate would understate it.
    #[inline]
    fn open_window(&mut self) {
        if self.recording_started.is_none() {
            self.recording_started = Some(Instant::now());
        }
    }

    #[inline]
    fn observe(&mut self, pick: fn(&mut Self) -> &mut Samples, span: Option<Instant>) {
        if !self.enabled || !self.recording {
            return;
        }
        if let Some(start) = span {
            let elapsed = ms(start.elapsed());
            self.open_window();
            pick(self).push(elapsed);
        }
    }

    pub fn record_event_apply(&mut self, span: Option<Instant>) {
        self.observe(|s| &mut s.event_apply, span);
    }

    pub fn record_vertex_build(&mut self, span: Option<Instant>) {
        self.observe(|s| &mut s.vertex_build, span);
    }

    pub fn record_gpu_submit(&mut self, span: Option<Instant>) {
        self.observe(|s| &mut s.gpu_submit, span);
    }

    pub fn record_frame_total(&mut self, span: Option<Instant>) {
        self.observe(|s| &mut s.frame_total, span);
    }

    /// One drained batch of `redraw` events, counted by kind. `flush` is tallied
    /// here because it is Neovim's own statement that a frame was complete.
    pub fn record_batch(&mut self, events: &[RedrawEvent]) {
        if !self.enabled || !self.recording {
            return;
        }
        self.open_window();
        self.batches += 1;
        self.events_total += events.len() as u64;
        self.events_per_batch.push(events.len() as f64);
        for (name, _) in events {
            *self.events_by_kind.entry(name.clone()).or_insert(0) += 1;
            if name == "flush" {
                self.flushes += 1;
            }
        }
    }

    /// A frame reached the screen (or, headless, the vertex stream was completed
    /// for one). Also closes the interval since the previous presentation.
    pub fn record_present(&mut self, vertex_count: usize) {
        if !self.enabled || !self.recording {
            return;
        }
        self.open_window();
        let now = Instant::now();
        if let Some(previous) = self.last_present {
            self.present_interval.push(ms(now - previous));
        }
        self.last_present = Some(now);
        self.presentations += 1;
        self.vertices_total += vertex_count as u64;
        self.vertices_per_frame.push(vertex_count as f64);
    }

    fn wall_clock_ms(&self) -> Option<f64> {
        let started = self.recording_started?;
        let ended = self.recording_ended.unwrap_or_else(Instant::now);
        Some(ms(ended.saturating_duration_since(started)))
    }

    fn presentation_rate_samples(&self) -> Samples {
        let mut rates = Samples::default();
        for &interval in &self.present_interval.values {
            if interval > 0.0 {
                rates.push(1000.0 / interval);
            }
        }
        rates
    }

    fn slow_frames(&self, budget_ms: Option<f64>) -> SlowFrameSection {
        let flushes_not_presented = self.flushes.saturating_sub(self.presentations);
        let observed = self.frame_total.len() as u64;
        match budget_ms {
            Some(budget) if observed > 0 => {
                let (over, worst) = self.frame_total.exceedances(budget);
                SlowFrameSection {
                    criterion: CRITERION_SUPPLIED,
                    frame_budget_ms: Some(budget),
                    frames_over_budget: Some(over),
                    frames_over_budget_ratio: Some(round(over as f64 / observed as f64)),
                    worst_overrun_ms: worst.map(round),
                    flushes_observed: self.flushes,
                    presentations_observed: self.presentations,
                    flushes_not_presented,
                }
            }
            // Either no budget was given, or nothing was timed to compare
            // against one. Both cases stay empty rather than defaulting to a
            // reassuring zero.
            _ => SlowFrameSection {
                criterion: CRITERION_UNSET,
                frame_budget_ms: budget_ms,
                frames_over_budget: None,
                frames_over_budget_ratio: None,
                worst_overrun_ms: None,
                flushes_observed: self.flushes,
                presentations_observed: self.presentations,
                flushes_not_presented,
            },
        }
    }

    pub fn report(
        &self,
        measurement: Measurement,
        environment: Environment,
        parameters: Parameters,
        atlas: AtlasSnapshot,
    ) -> Report {
        let wall_clock = self.wall_clock_ms();
        let presentation_rate = wall_clock.and_then(|elapsed| {
            (elapsed > 0.0 && self.presentations > 0)
                .then(|| round(self.presentations as f64 * 1000.0 / elapsed))
        });
        let gpu_submit = measurement
            .gpu_submit_measured
            .then(|| self.gpu_submit.summary())
            .flatten();

        Report {
            schema: SCHEMA,
            trace_id: Ulid::new().to_string(),
            evaluation_candidate: true,
            canonical_performance_criteria: false,
            performance_acceptance: CRITERION_UNSET,
            measurement,
            environment,
            frame: FrameSection {
                frames_recorded: self.frame_total.len() as u64,
                warmup_frames_excluded: self.warmup_excluded,
                wall_clock_ms: wall_clock.map(round),
                total_ms: self.frame_total.summary(),
                event_apply_ms: self.event_apply.summary(),
                vertex_build_ms: self.vertex_build.summary(),
                gpu_submit_ms: gpu_submit,
                present_interval_ms: self.present_interval.summary(),
                presentation_rate_hz: self.presentation_rate_samples().summary(),
                presentations_per_wall_clock_second: presentation_rate,
            },
            redraw: RedrawSection {
                batches: self.batches,
                events_total: self.events_total,
                events_per_batch: self.events_per_batch.summary(),
                batch_apply_ms: self.event_apply.summary(),
                events_by_kind: self.events_by_kind.clone(),
            },
            glyph_atlas: AtlasSection {
                counters_window: ATLAS_WINDOW,
                counters: atlas.counters,
                uploads: measurement
                    .gpu_submit_measured
                    .then_some(atlas.counters.uploads),
                hit_ratio: (atlas.counters.lookups > 0)
                    .then(|| round(atlas.counters.hits as f64 / atlas.counters.lookups as f64)),
                cached_entries: atlas.cached_entries as u64,
                atlas_side_px: atlas.side_px as u64,
                packed_height_px: atlas.packed_height_px as u64,
                packed_height_ratio: round(
                    atlas.packed_height_px as f64 / atlas.side_px.max(1) as f64,
                ),
            },
            vertices: VertexSection {
                per_frame: self.vertices_per_frame.summary(),
                total: self.vertices_total,
            },
            slow_frames: self.slow_frames(parameters.frame_budget_ms),
            parameters,
        }
    }
}

/// What the atlas looked like when the report was taken.
#[derive(Debug, Clone, Copy)]
pub struct AtlasSnapshot {
    pub counters: AtlasStats,
    pub cached_entries: usize,
    pub side_px: usize,
    pub packed_height_px: usize,
}

impl AtlasSnapshot {
    pub fn of(atlas: &crate::text::Atlas) -> Self {
        Self {
            counters: atlas.stats,
            cached_entries: atlas.cached_entries(),
            side_px: crate::text::ATLAS,
            packed_height_px: atlas.packed_height_px(),
        }
    }

    /// For paths that never built an atlas.
    pub fn absent() -> Self {
        Self {
            counters: AtlasStats::default(),
            cached_entries: 0,
            side_px: crate::text::ATLAS,
            packed_height_px: 0,
        }
    }
}

pub fn to_json(report: &Report) -> String {
    serde_json::to_string_pretty(report).expect("perf report is serialisable")
}

/// Write to `path`, or to stdout when no path was given.
pub fn emit(report: &Report, path: Option<&std::path::Path>) -> std::io::Result<()> {
    let json = to_json(report);
    match path {
        Some(path) => {
            std::fs::write(path, format!("{json}\n"))?;
            eprintln!("WROTE {}", path.display());
            Ok(())
        }
        None => {
            println!("{json}");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmpv::Value;

    fn samples(values: &[f64]) -> Samples {
        let mut s = Samples::default();
        for &v in values {
            s.push(v);
        }
        s
    }

    fn event(name: &str) -> RedrawEvent {
        (name.to_string(), vec![Value::from(1u64)])
    }

    fn parameters(budget: Option<f64>) -> Parameters {
        Parameters {
            cols: 80,
            rows: 24,
            font_size_px: 15.0,
            frames_requested: Some(3),
            warmup_frames: 0,
            seed: Some(1),
            frame_budget_ms: budget,
        }
    }

    fn measurement() -> Measurement {
        Measurement {
            mode: "headless_benchmark",
            event_source: "synthetic_deterministic_script",
            workload_deterministic: true,
            gpu_submit_measured: false,
            frame_total_stages: STAGES_HEADLESS,
            presentation_model: PRESENTATION_UNTHROTTLED,
        }
    }

    fn gpu_measurement() -> Measurement {
        Measurement {
            mode: "live_session",
            event_source: "nvim_ext_linegrid",
            workload_deterministic: false,
            gpu_submit_measured: true,
            frame_total_stages: STAGES_LIVE,
            presentation_model: PRESENTATION_ON_DEMAND,
        }
    }

    fn report_of(recorder: &Recorder, budget: Option<f64>) -> Report {
        recorder.report(
            measurement(),
            Environment::observe(None),
            parameters(budget),
            AtlasSnapshot::absent(),
        )
    }

    #[test]
    fn empty_samples_have_no_distribution() {
        assert!(Samples::default().summary().is_none());
    }

    #[test]
    fn a_single_observation_is_every_percentile() {
        let s = samples(&[7.5]).summary().unwrap();
        assert_eq!(s.count, 1);
        assert_eq!(
            (s.min, s.p50, s.p95, s.p99, s.max),
            (7.5, 7.5, 7.5, 7.5, 7.5)
        );
        assert_eq!(s.mean, 7.5);
    }

    #[test]
    fn percentiles_use_nearest_rank_over_the_sorted_observations() {
        // 1..=100 shuffled: the answer must not depend on arrival order.
        let mut values: Vec<f64> = (1..=100).map(|v| v as f64).collect();
        values.rotate_left(37);
        values.swap(0, 99);
        let s = samples(&values).summary().unwrap();
        assert_eq!(s.count, 100);
        assert_eq!(s.min, 1.0);
        assert_eq!(s.max, 100.0);
        assert_eq!(s.p50, 50.0);
        assert_eq!(s.p90, 90.0);
        assert_eq!(s.p95, 95.0);
        assert_eq!(s.p99, 99.0);
        assert_eq!(s.mean, 50.5);
    }

    #[test]
    fn every_reported_percentile_is_an_observation_that_happened() {
        let values = [3.0, 3.0, 3.0, 100.0];
        let s = samples(&values).summary().unwrap();
        for reported in [s.min, s.p50, s.p90, s.p95, s.p99, s.max] {
            assert!(
                values.contains(&reported),
                "{reported} was interpolated, not observed"
            );
        }
        // The rank must round up, so a lone outlier owns the top percentiles.
        assert_eq!(s.p50, 3.0);
        assert_eq!(s.p95, 100.0);
    }

    #[test]
    fn a_disabled_recorder_observes_nothing_and_reads_no_clock() {
        let mut recorder = Recorder::disabled();
        assert!(!recorder.is_enabled());
        assert!(recorder.span().is_none());
        // Spans handed in from elsewhere are still ignored.
        recorder.record_frame_total(Some(Instant::now()));
        recorder.record_batch(&[event("grid_line"), event("flush")]);
        recorder.record_present(4096);

        let report = report_of(&recorder, None);
        assert_eq!(report.frame.frames_recorded, 0);
        assert!(report.frame.total_ms.is_none());
        assert!(report.frame.presentation_rate_hz.is_none());
        assert_eq!(report.redraw.batches, 0);
        assert_eq!(report.redraw.events_total, 0);
        assert!(report.redraw.events_by_kind.is_empty());
        assert_eq!(report.vertices.total, 0);
        assert!(report.vertices.per_frame.is_none());
    }

    #[test]
    fn an_enabled_recorder_counts_batches_by_event_kind() {
        let mut recorder = Recorder::enabled();
        recorder.record_batch(&[event("grid_line"), event("grid_line"), event("flush")]);
        recorder.record_batch(&[event("grid_scroll"), event("flush")]);

        let report = report_of(&recorder, None);
        assert_eq!(report.redraw.batches, 2);
        assert_eq!(report.redraw.events_total, 5);
        assert_eq!(report.redraw.events_by_kind["grid_line"], 2);
        assert_eq!(report.redraw.events_by_kind["grid_scroll"], 1);
        assert_eq!(report.redraw.events_by_kind["flush"], 2);
        let per_batch = report.redraw.events_per_batch.unwrap();
        assert_eq!((per_batch.min, per_batch.max), (2.0, 3.0));
    }

    #[test]
    fn warmup_frames_are_excluded_from_the_distribution_but_still_counted() {
        let mut recorder = Recorder::enabled();
        recorder.set_recording(false);
        recorder.record_present(999);
        recorder.record_batch(&[event("flush")]);
        recorder.note_warmup_frame();
        recorder.set_recording(true);
        recorder.record_present(10);
        recorder.record_present(20);

        let report = report_of(&recorder, None);
        assert_eq!(report.frame.warmup_frames_excluded, 1);
        assert_eq!(report.vertices.total, 30, "warm-up vertices must not count");
        assert_eq!(report.redraw.events_total, 0);
        assert_eq!(report.slow_frames.presentations_observed, 2);
    }

    #[test]
    fn without_a_budget_no_slow_frame_verdict_is_produced() {
        let mut recorder = Recorder::enabled();
        recorder.record_frame_total(Some(Instant::now()));

        let slow = report_of(&recorder, None).slow_frames;
        assert_eq!(slow.criterion, CRITERION_UNSET);
        assert!(slow.frame_budget_ms.is_none());
        assert!(slow.frames_over_budget.is_none());
        assert!(slow.frames_over_budget_ratio.is_none());
        assert!(slow.worst_overrun_ms.is_none());
    }

    #[test]
    fn a_supplied_budget_classifies_only_against_that_budget() {
        let mut recorder = Recorder::enabled();
        recorder.set_recording(true);
        recorder.frame_total = samples(&[1.0, 2.0, 30.0, 4.0]);

        let slow = report_of(&recorder, Some(5.0)).slow_frames;
        assert_eq!(slow.criterion, CRITERION_SUPPLIED);
        assert_eq!(slow.frame_budget_ms, Some(5.0));
        assert_eq!(slow.frames_over_budget, Some(1));
        assert_eq!(slow.frames_over_budget_ratio, Some(0.25));
        assert_eq!(slow.worst_overrun_ms, Some(25.0));
    }

    #[test]
    fn a_run_that_never_overran_reports_no_worst_overrun_rather_than_zero() {
        let mut recorder = Recorder::enabled();
        recorder.frame_total = samples(&[1.0, 2.0, 4.0]);

        let slow = report_of(&recorder, Some(5.0)).slow_frames;
        // The budget was applied — this is a measured verdict, not an absent one.
        assert_eq!(slow.criterion, CRITERION_SUPPLIED);
        assert_eq!(slow.frames_over_budget, Some(0));
        assert_eq!(slow.frames_over_budget_ratio, Some(0.0));
        assert!(
            slow.worst_overrun_ms.is_none(),
            "no frame overran, so there is no worst overrun to measure"
        );

        let value: serde_json::Value =
            serde_json::from_str(&to_json(&report_of(&recorder, Some(5.0)))).unwrap();
        assert!(value["slow_frames"]["worst_overrun_ms"].is_null());
    }

    #[test]
    fn an_exact_hit_on_the_budget_is_not_an_overrun() {
        let mut recorder = Recorder::enabled();
        recorder.frame_total = samples(&[5.0, 5.0]);
        let slow = report_of(&recorder, Some(5.0)).slow_frames;
        assert_eq!(slow.frames_over_budget, Some(0));
        assert!(slow.worst_overrun_ms.is_none());
    }

    #[test]
    fn a_budget_with_nothing_timed_stays_unclassified() {
        let recorder = Recorder::enabled();
        let slow = report_of(&recorder, Some(16.0)).slow_frames;
        assert_eq!(slow.criterion, CRITERION_UNSET);
        assert!(slow.frames_over_budget.is_none());
    }

    #[test]
    fn completed_frames_that_never_reached_the_screen_are_counted_without_a_threshold() {
        let mut recorder = Recorder::enabled();
        // Three complete frames from nvim, coalesced into one presentation.
        recorder.record_batch(&[event("flush"), event("flush"), event("flush")]);
        recorder.record_present(128);

        let slow = report_of(&recorder, None).slow_frames;
        assert_eq!(slow.flushes_observed, 3);
        assert_eq!(slow.presentations_observed, 1);
        assert_eq!(slow.flushes_not_presented, 2);
        // Still no budget involved.
        assert_eq!(slow.criterion, CRITERION_UNSET);
    }

    #[test]
    fn gpu_figures_are_absent_when_the_gpu_was_not_on_the_measured_path() {
        let mut recorder = Recorder::enabled();
        recorder.record_gpu_submit(Some(Instant::now()));
        let report = report_of(&recorder, None);
        assert!(!report.measurement.gpu_submit_measured);
        assert!(
            report.frame.gpu_submit_ms.is_none(),
            "a headless run must not publish GPU timings"
        );
    }

    #[test]
    fn the_presentation_rate_is_derived_from_observed_intervals_only() {
        let mut recorder = Recorder::enabled();
        recorder.present_interval = samples(&[10.0, 20.0, 40.0]);
        let rate = report_of(&recorder, None).frame;
        let rate = rate.presentation_rate_hz.unwrap();
        assert_eq!(rate.count, 3);
        assert_eq!(rate.min, 25.0); // 1000/40
        assert_eq!(rate.max, 100.0); // 1000/10
        assert_eq!(rate.p50, 50.0); // 1000/20
    }

    #[test]
    fn a_single_presentation_yields_no_interval_and_no_rate() {
        let mut recorder = Recorder::enabled();
        recorder.record_present(64);
        let frame = report_of(&recorder, None).frame;
        assert!(frame.present_interval_ms.is_none());
        assert!(frame.presentation_rate_hz.is_none());
    }

    #[test]
    fn the_recorded_window_opens_at_the_first_observation() {
        let recorder = Recorder::enabled();
        // Nothing observed yet, so there is no window to report a rate over.
        let frame = report_of(&recorder, None).frame;
        assert!(frame.wall_clock_ms.is_none());
        assert!(frame.presentations_per_wall_clock_second.is_none());

        let mut recorder = Recorder::enabled();
        recorder.record_present(10);
        recorder.record_present(20);
        let frame = report_of(&recorder, None).frame;
        let elapsed = frame.wall_clock_ms.expect("a window opened");
        assert!(elapsed >= 0.0);
        assert!(
            frame.presentations_per_wall_clock_second.is_some(),
            "two presentations over a measured window is a rate"
        );
    }

    #[test]
    fn a_disabled_recorder_never_opens_a_window() {
        let mut recorder = Recorder::disabled();
        recorder.record_present(10);
        recorder.record_batch(&[event("flush")]);
        let frame = report_of(&recorder, None).frame;
        assert!(frame.wall_clock_ms.is_none());
        assert!(frame.presentations_per_wall_clock_second.is_none());
    }

    #[test]
    fn atlas_ratios_are_absent_until_something_was_looked_up() {
        let recorder = Recorder::enabled();
        let report = report_of(&recorder, None);
        assert!(report.glyph_atlas.hit_ratio.is_none());
        assert_eq!(report.glyph_atlas.counters, AtlasStats::default());
    }

    #[test]
    fn atlas_section_reports_the_observed_hit_ratio_and_packing() {
        let recorder = Recorder::enabled();
        let snapshot = AtlasSnapshot {
            counters: AtlasStats {
                lookups: 8,
                hits: 6,
                misses: 2,
                rasterizations: 2,
                empty_glyphs: 0,
                rejections_atlas_full: 0,
                uploads: 1,
            },
            cached_entries: 2,
            side_px: 1024,
            packed_height_px: 256,
        };
        let report = recorder.report(
            measurement(),
            Environment::observe(None),
            parameters(None),
            snapshot,
        );
        assert_eq!(report.glyph_atlas.hit_ratio, Some(0.75));
        assert_eq!(report.glyph_atlas.cached_entries, 2);
        assert_eq!(report.glyph_atlas.packed_height_ratio, 0.25);
    }

    fn atlas_with_uploads(uploads: u64) -> AtlasSnapshot {
        AtlasSnapshot {
            counters: AtlasStats {
                lookups: 4,
                hits: 3,
                misses: 1,
                rasterizations: 1,
                empty_glyphs: 0,
                rejections_atlas_full: 0,
                uploads,
            },
            cached_entries: 1,
            side_px: 1024,
            packed_height_px: 16,
        }
    }

    #[test]
    fn texture_uploads_are_absent_when_there_was_no_gpu_to_upload_to() {
        let recorder = Recorder::enabled();
        let report = recorder.report(
            measurement(),
            Environment::observe(None),
            parameters(None),
            atlas_with_uploads(0),
        );
        assert!(!report.measurement.gpu_submit_measured);
        assert!(
            report.glyph_atlas.uploads.is_none(),
            "a headless run never reaches the upload path; 0 would claim it did"
        );

        let value: serde_json::Value = serde_json::from_str(&to_json(&report)).unwrap();
        assert!(value["glyph_atlas"]["uploads"].is_null());
        // The counter is reported once, through the section that can suppress it.
        assert_eq!(
            value["glyph_atlas"]
                .as_object()
                .unwrap()
                .keys()
                .filter(|k| k.as_str() == "uploads")
                .count(),
            1
        );
    }

    #[test]
    fn texture_uploads_are_reported_when_the_gpu_was_on_the_path() {
        let recorder = Recorder::enabled();
        let report = recorder.report(
            gpu_measurement(),
            Environment::observe(None),
            parameters(None),
            atlas_with_uploads(3),
        );
        assert_eq!(report.glyph_atlas.uploads, Some(3));

        // Zero uploads on a GPU path is an observation, not an absence.
        let none_yet = recorder.report(
            gpu_measurement(),
            Environment::observe(None),
            parameters(None),
            atlas_with_uploads(0),
        );
        assert_eq!(none_yet.glyph_atlas.uploads, Some(0));
    }

    #[test]
    fn atlas_counters_declare_the_window_they_cover() {
        let mut recorder = Recorder::enabled();
        recorder.note_warmup_frame();
        let report = report_of(&recorder, None);
        // The frame window excludes warm-up; the atlas counters do not, and the
        // report has to say so rather than let one window be read for both.
        assert_eq!(report.frame.warmup_frames_excluded, 1);
        assert_eq!(report.glyph_atlas.counters_window, ATLAS_WINDOW);

        let value: serde_json::Value = serde_json::from_str(&to_json(&report)).unwrap();
        assert_eq!(value["glyph_atlas"]["counters_window"], ATLAS_WINDOW);
    }

    #[test]
    fn the_report_names_the_stages_its_frame_total_covers() {
        let recorder = Recorder::enabled();

        let headless = report_of(&recorder, None);
        assert_eq!(headless.measurement.frame_total_stages, STAGES_HEADLESS);

        let live = recorder.report(
            gpu_measurement(),
            Environment::observe(None),
            parameters(None),
            AtlasSnapshot::absent(),
        );
        assert_eq!(live.measurement.frame_total_stages, STAGES_LIVE);

        // The two modes compose a frame differently. That is the fact the field
        // exists to publish, so a schema change that made them equal without
        // unifying the spans would be a regression, not a simplification.
        assert_ne!(STAGES_HEADLESS, STAGES_LIVE);
        assert!(STAGES_HEADLESS.contains(&"event_apply"));
        assert!(
            !STAGES_LIVE.contains(&"event_apply"),
            "live frames apply redraw in the event pump, not in the frame"
        );
        // A stage that was never measured must not be claimed as covered.
        assert!(!STAGES_HEADLESS.contains(&"gpu_submit"));

        let value: serde_json::Value = serde_json::from_str(&to_json(&headless)).unwrap();
        let stages: Vec<&str> = value["measurement"]["frame_total_stages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(stages, STAGES_HEADLESS);
    }

    #[test]
    fn the_report_says_how_presentations_were_paced() {
        let recorder = Recorder::enabled();
        assert_eq!(
            report_of(&recorder, None).measurement.presentation_model,
            PRESENTATION_UNTHROTTLED
        );

        let live = recorder.report(
            gpu_measurement(),
            Environment::observe(None),
            parameters(None),
            AtlasSnapshot::absent(),
        );
        // Without this, `presentations_per_wall_clock_second` from an idle on-demand
        // session reads as a frame rate the renderer could not exceed.
        assert_eq!(live.measurement.presentation_model, PRESENTATION_ON_DEMAND);

        let value: serde_json::Value = serde_json::from_str(&to_json(&live)).unwrap();
        assert_eq!(
            value["measurement"]["presentation_model"],
            PRESENTATION_ON_DEMAND
        );
    }

    #[test]
    fn batch_apply_and_event_apply_are_the_same_observations() {
        let mut recorder = Recorder::enabled();
        recorder.event_apply = samples(&[1.0, 2.0, 3.0]);
        let report = report_of(&recorder, None);
        assert_eq!(report.frame.event_apply_ms, report.redraw.batch_apply_ms);
    }

    #[test]
    fn report_json_carries_the_schema_and_refuses_to_imply_acceptance() {
        let recorder = Recorder::enabled();
        let value: serde_json::Value =
            serde_json::from_str(&to_json(&report_of(&recorder, None))).unwrap();

        assert_eq!(value["schema"], SCHEMA);
        assert_eq!(value["evaluation_candidate"], true);
        assert_eq!(value["canonical_performance_criteria"], false);
        assert_eq!(value["performance_acceptance"], CRITERION_UNSET);
        assert!(!value["trace_id"].as_str().unwrap().is_empty());

        // Environment and parameters must be present for a report to be
        // interpretable at all.
        assert_eq!(value["environment"]["os"], std::env::consts::OS);
        assert_eq!(value["environment"]["architecture"], std::env::consts::ARCH);
        assert!(value["environment"]["debug_assertions"].is_boolean());
        assert_eq!(value["parameters"]["cols"], 80);
        assert_eq!(value["parameters"]["rows"], 24);
        assert_eq!(value["parameters"]["seed"], 1);
        assert!(value["parameters"]["frame_budget_ms"].is_null());

        // Unobserved quantities are null, never a stand-in number.
        assert!(value["frame"]["total_ms"].is_null());
        assert!(value["frame"]["presentations_per_wall_clock_second"].is_null());
        assert!(value["slow_frames"]["frames_over_budget"].is_null());

        // The atlas counters are flattened in, not nested under a wrapper.
        assert_eq!(value["glyph_atlas"]["lookups"], 0);
        assert_eq!(value["glyph_atlas"]["atlas_side_px"], 1024);
    }

    #[test]
    fn report_sections_required_by_the_schema_are_all_present() {
        let recorder = Recorder::enabled();
        let value: serde_json::Value =
            serde_json::from_str(&to_json(&report_of(&recorder, None))).unwrap();
        for section in [
            "schema",
            "trace_id",
            "measurement",
            "environment",
            "parameters",
            "frame",
            "redraw",
            "glyph_atlas",
            "vertices",
            "slow_frames",
        ] {
            assert!(!value[section].is_null(), "report is missing `{section}`");
        }
        for field in [
            "frames_recorded",
            "total_ms",
            "event_apply_ms",
            "vertex_build_ms",
            "gpu_submit_ms",
            "present_interval_ms",
            "presentation_rate_hz",
            "presentations_per_wall_clock_second",
        ] {
            assert!(
                value["frame"].get(field).is_some(),
                "frame section is missing `{field}`"
            );
        }
    }

    #[test]
    fn summary_percentiles_serialise_under_stable_names() {
        let mut recorder = Recorder::enabled();
        recorder.frame_total = samples(&[1.0, 2.0, 3.0]);
        let value: serde_json::Value =
            serde_json::from_str(&to_json(&report_of(&recorder, None))).unwrap();
        let total = &value["frame"]["total_ms"];
        assert_eq!(total["count"], 3);
        assert_eq!(total["min"], 1.0);
        assert_eq!(total["p50"], 2.0);
        assert_eq!(total["max"], 3.0);
        assert!(total["p90"].is_number());
        assert!(total["p99"].is_number());
    }

    #[test]
    fn rounding_is_presentation_only_and_keeps_sub_microsecond_detail() {
        let s = samples(&[0.000_123_456, 0.000_2]).summary().unwrap();
        assert_eq!(s.min, 0.0001); // 4 decimal places of a millisecond
        assert!(s.max > 0.0);
    }

    // --- The committed evidence -------------------------------------------
    //
    // `evaluation/evidence/*.json` and the prose in `evaluation/README.md` are
    // both published artefacts, and the commit that carries them claims they
    // are honest observations. Nothing but a check like this stops the two from
    // drifting apart, or stops a schema change from leaving a stale file behind
    // still claiming the current schema.

    fn evidence_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("evidence")
    }

    fn evidence(name: &str) -> serde_json::Value {
        let path = evidence_dir().join(name);
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("evidence {} is unreadable: {e}", path.display()));
        serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("evidence {} is not JSON: {e}", path.display()))
    }

    fn readme() -> String {
        let path = evidence_dir().join("..").join("README.md");
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{} is unreadable: {e}", path.display()))
    }

    fn count(atlas: &serde_json::Value, field: &str) -> u64 {
        atlas[field]
            .as_u64()
            .unwrap_or_else(|| panic!("glyph_atlas.{field} is not a count"))
    }

    #[test]
    fn the_committed_evidence_still_matches_the_schema_it_claims() {
        for name in ["perf-headless-bench.json", "perf-live-session.json"] {
            let report = evidence(name);
            assert_eq!(report["schema"], SCHEMA, "{name} declares another schema");
            assert_eq!(report["performance_acceptance"], CRITERION_UNSET, "{name}");
            assert_eq!(report["canonical_performance_criteria"], false, "{name}");

            let measurement = &report["measurement"];
            let expected_stages = match measurement["mode"].as_str() {
                Some("headless_benchmark") => STAGES_HEADLESS,
                Some("live_session") => STAGES_LIVE,
                other => panic!("{name} has an unknown mode {other:?}"),
            };
            let stages: Vec<&str> = measurement["frame_total_stages"]
                .as_array()
                .unwrap_or_else(|| panic!("{name} does not say what total_ms covers"))
                .iter()
                .map(|v| v.as_str().unwrap())
                .collect();
            assert_eq!(
                stages, expected_stages,
                "{name} was produced by an instrumentation layout this build no longer has"
            );
            assert!(
                measurement["presentation_model"].is_string(),
                "{name} does not say how presentations were paced"
            );

            let atlas = &report["glyph_atlas"];
            assert_eq!(atlas["counters_window"], ATLAS_WINDOW, "{name}");
            assert_eq!(
                count(atlas, "hits") + count(atlas, "misses"),
                count(atlas, "lookups"),
                "{name}: every lookup is a hit or a miss"
            );
            assert_eq!(
                count(atlas, "rasterizations")
                    + count(atlas, "empty_glyphs")
                    + count(atlas, "rejections_atlas_full"),
                count(atlas, "misses"),
                "{name}: every miss has exactly one outcome"
            );

            // Uploads are reported exactly when the GPU was on the path.
            let gpu = measurement["gpu_submit_measured"].as_bool().unwrap();
            assert_eq!(
                atlas["uploads"].is_null(),
                !gpu,
                "{name}: upload count disagrees with whether a GPU was measured"
            );
            assert_eq!(
                report["frame"]["gpu_submit_ms"].is_null(),
                !gpu,
                "{name}: GPU timings disagree with whether a GPU was measured"
            );
        }
    }

    /// One `| label | p50 ms | p99 ms | max ms |` row, exactly as the README
    /// table writes it. Unrounded on purpose: a rounded quotation cannot be
    /// checked against the artefact without inventing a tolerance, and a
    /// tolerance is the thing that lets prose drift.
    fn readme_row(label: &str, summary: &serde_json::Value) -> String {
        let at = |key: &str| {
            summary[key]
                .as_f64()
                .unwrap_or_else(|| panic!("{label} has no {key}"))
        };
        format!(
            "| {label} | {} ms | {} ms | {} ms |",
            at("p50"),
            at("p99"),
            at("max")
        )
    }

    #[test]
    fn the_readme_quotes_the_evidence_it_cites() {
        let readme = readme();
        let bench = evidence("perf-headless-bench.json");
        let atlas = &bench["glyph_atlas"];
        let frame = &bench["frame"];

        // The atlas figures are exactly where the prose drifted from the
        // artefact once already: the README claimed 1.58M lookups against 2.64M
        // measured, and a 38px atlas against 35px packed. The table rows are
        // pinned for the same reason, before they get a chance to.
        let mut quoted = vec![
            format!("lookup {} 回", count(atlas, "lookups")),
            format!("rasterize は {} 回", count(atlas, "rasterizations")),
            format!(
                "atlas 使用高さ {}px / {}px",
                count(atlas, "packed_height_px"),
                count(atlas, "atlas_side_px")
            ),
            readme_row("frame 全体", &frame["total_ms"]),
            readme_row("vertex 構築", &frame["vertex_build_ms"]),
            readme_row("redraw batch 適用", &frame["event_apply_ms"]),
        ];

        // The live run is not reproducible, so only the figures the prose names
        // outright are pinned — but those must still be the measured ones.
        let live = evidence("perf-live-session.json");
        let submit = &live["frame"]["gpu_submit_ms"];
        quoted.push(format!(
            "GPU 提出は 1 frame 目が {} ms、2 frame 目が {} ms",
            submit["max"].as_f64().unwrap(),
            submit["min"].as_f64().unwrap()
        ));
        let apply = &live["frame"]["event_apply_ms"];
        quoted.push(format!(
            "redraw batch 適用は p50 {} ms・max {} ms",
            apply["p50"].as_f64().unwrap(),
            apply["max"].as_f64().unwrap()
        ));
        quoted.push(format!(
            "`presentations_per_wall_clock_second` は {} だが",
            live["frame"]["presentations_per_wall_clock_second"]
                .as_f64()
                .unwrap()
        ));
        quoted.push(format!(
            "`presentation_rate_hz` は {}。",
            live["frame"]["presentation_rate_hz"]["p50"]
                .as_f64()
                .unwrap()
        ));
        for kind in ["hl_group_set", "grid_line", "option_set"] {
            quoted.push(format!(
                "`{kind}` {} 件",
                live["redraw"]["events_by_kind"][kind].as_u64().unwrap()
            ));
        }

        for quoted in quoted {
            assert!(
                readme.contains(&quoted),
                "evaluation/README.md does not say `{quoted}`, which is what the \
                 evidence it cites actually measured"
            );
        }
    }
}
