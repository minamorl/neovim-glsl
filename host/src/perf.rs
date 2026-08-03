//! The distribution helper the picker's own benchmark reports through.
//!
//! This is the small part of the candidate's `perf` module that
//! `picker_state.rs` needs, restated here rather than reached by path: the
//! candidate's version carries tests that read `evaluation/evidence/*.json`
//! relative to its own manifest, and compiling those into this crate would make
//! the product's test run fail over an evidence file that is not this crate's
//! to hold.
//!
//! The two rules that matter are kept exactly: an empty run has no distribution
//! (`None`, never zeros, because zeros read as "measured and fast"), and a
//! percentile is a nearest-rank observation that actually happened rather than
//! an interpolation between two that did not.

use serde::Serialize;

const DECIMALS: u32 = 4;

fn round(value: f64) -> f64 {
    let scale = 10f64.powi(DECIMALS as i32);
    (value * scale).round() / scale
}

#[derive(Debug, Default, Clone)]
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

    /// `None` when nothing was observed.
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
}

/// Nearest-rank percentile on an ascending slice: the smallest observation at
/// or above the `p`-th position.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_run_has_no_distribution() {
        assert!(Samples::default().summary().is_none());
    }

    #[test]
    fn percentiles_use_nearest_rank_and_do_not_depend_on_arrival_order() {
        let mut values: Vec<f64> = (1..=100).map(|v| v as f64).collect();
        values.rotate_left(37);
        let mut samples = Samples::default();
        for value in values {
            samples.push(value);
        }
        let summary = samples.summary().unwrap();
        assert_eq!(summary.p50, 50.0);
        assert_eq!(summary.p90, 90.0);
        assert_eq!(summary.p99, 99.0);
        assert_eq!((summary.min, summary.max), (1.0, 100.0));
    }

    #[test]
    fn every_reported_percentile_is_an_observation_that_happened() {
        let mut samples = Samples::default();
        for value in [1.0, 2.0, 100.0] {
            samples.push(value);
        }
        let summary = samples.summary().unwrap();
        for reported in [summary.p50, summary.p90, summary.p95, summary.p99] {
            assert!([1.0, 2.0, 100.0].contains(&reported), "{reported} was interpolated");
        }
    }
}
