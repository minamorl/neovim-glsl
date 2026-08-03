//! Picker state held by the host, so that holding it can be measured.
//!
//! spec v0.8 fixed *where* the navigation surface is drawn (a GLSL surface over
//! the grid, in this window, by this process) and left *who owns the picker's
//! state* open. On 2026-08-03 the human gate answered that question with
//! 「わからない」, which is not a choice, so v0.9 pinned nothing and turned the
//! question from one waiting on a person into one waiting on an observation.
//!
//! This module is one half of that observation: the arrangement where the host
//! owns the query, the candidate set and the selection, and the plugin owns
//! nothing. The other half lives in `evaluation/state-ownership/probe.py`, which
//! drives the owner's real telescope and measures the arrangement where the
//! plugin owns all three and the host must extract them.
//!
//! It decides nothing. `open_question neovim_glsl.navigation_state_owner` and
//! `open_question neovim_glsl.navigation_input_routing` are both still open, and
//! this file must not be read as the answer to either: it is what the answer
//! "host" would cost, measured instead of guessed.
//!
//! ## What is deliberately *not* claimed
//!
//! The matcher below is *a* matcher, not *the* matcher. telescope sorts with
//! fzy; this sorts with a span-penalised subsequence scan. Comparing their
//! durations would compare two algorithms, and the open question is not about
//! algorithms — so the report separates the cost that belongs to the
//! *arrangement* (how many process boundaries a keystroke crosses before rows
//! exist, which is zero here and not zero there) from the cost that belongs to
//! whatever matcher a host would end up shipping.

use std::time::Instant;

use serde::Serialize;

use crate::perf::{Samples, Summary};

/// One matched candidate: which corpus entry, how well it scored, and where the
/// query characters landed.
///
/// The positions are carried because a surface that draws a picker draws them —
/// the matched characters are highlighted. A host-owned picker has to produce
/// them itself, and a measurement that skipped them would be measuring a
/// narrower job than the real one.
#[derive(Clone, Debug, PartialEq)]
pub struct Match {
    pub index: usize,
    pub score: i32,
    pub positions: Vec<u32>,
}

/// A row as the surface would receive it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Row {
    pub text: String,
    pub positions: Vec<u32>,
    pub selected: bool,
}

/// What one keystroke did to the candidate set.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Delta {
    pub rows_before: usize,
    pub rows_after: usize,
    /// True when the new set was computed from the previous set rather than from
    /// the whole corpus. Appending a character can only remove candidates, so
    /// this is sound for `push_char`; `pop_char` cannot use it.
    pub narrowed: bool,
}

/// Query, candidate set and selection, owned here.
pub struct PickerState {
    corpus: Vec<String>,
    query: String,
    matches: Vec<Match>,
    selection: usize,
}

impl PickerState {
    pub fn new(corpus: Vec<String>) -> Self {
        let matches = (0..corpus.len())
            .map(|index| Match { index, score: 0, positions: Vec::new() })
            .collect();
        Self { corpus, query: String::new(), matches, selection: 0 }
    }

    pub fn corpus_len(&self) -> usize {
        self.corpus.len()
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn len(&self) -> usize {
        self.matches.len()
    }

    pub fn is_empty(&self) -> bool {
        self.matches.is_empty()
    }

    pub fn selection_row(&self) -> Option<usize> {
        if self.matches.is_empty() {
            None
        } else {
            Some(self.selection)
        }
    }

    pub fn selection(&self) -> Option<&str> {
        self.matches
            .get(self.selection)
            .map(|m| self.corpus[m.index].as_str())
    }

    /// Append a character to the query and narrow the existing set.
    pub fn push_char(&mut self, ch: char) -> Delta {
        let before = self.matches.len();
        self.query.push(ch);
        let query: Vec<char> = self.query.chars().collect();
        let previous = std::mem::take(&mut self.matches);
        self.matches = previous
            .into_iter()
            .filter_map(|m| score(&self.corpus[m.index], &query).map(|(score, positions)| Match {
                index: m.index,
                score,
                positions,
            }))
            .collect();
        self.sort_matches();
        self.clamp_selection();
        Delta { rows_before: before, rows_after: self.matches.len(), narrowed: true }
    }

    /// Remove the last character of the query. The candidate set can only grow,
    /// so this must be recomputed from the corpus.
    pub fn pop_char(&mut self) -> Delta {
        let before = self.matches.len();
        if self.query.pop().is_none() {
            return Delta { rows_before: before, rows_after: before, narrowed: false };
        }
        self.refilter();
        Delta { rows_before: before, rows_after: self.matches.len(), narrowed: false }
    }

    /// Recompute from the whole corpus. Used by `pop_char`, and by the tests
    /// that check narrowing agrees with it.
    pub fn refilter(&mut self) {
        let query: Vec<char> = self.query.chars().collect();
        if query.is_empty() {
            self.matches = (0..self.corpus.len())
                .map(|index| Match { index, score: 0, positions: Vec::new() })
                .collect();
        } else {
            self.matches = self
                .corpus
                .iter()
                .enumerate()
                .filter_map(|(index, text)| {
                    score(text, &query).map(|(score, positions)| Match { index, score, positions })
                })
                .collect();
            self.sort_matches();
        }
        self.clamp_selection();
    }

    /// Move the selection by `delta` rows, clamped at both ends.
    ///
    /// Clamped rather than wrapped, because wrapping is a behaviour a picker
    /// might or might not want and this file is not the place that decides it.
    pub fn move_selection(&mut self, delta: i64) {
        if self.matches.is_empty() {
            self.selection = 0;
            return;
        }
        let last = (self.matches.len() - 1) as i64;
        let next = (self.selection as i64 + delta).clamp(0, last);
        self.selection = next as usize;
    }

    /// The rows a surface would draw: a window into the match list.
    pub fn rows(&self, offset: usize, limit: usize) -> Vec<Row> {
        self.matches
            .iter()
            .enumerate()
            .skip(offset)
            .take(limit)
            .map(|(row, m)| Row {
                text: self.corpus[m.index].clone(),
                positions: m.positions.clone(),
                selected: row == self.selection,
            })
            .collect()
    }

    /// Stable order: better score first, then corpus order. Ties broken by index
    /// rather than left to the sort, so the same corpus and query always produce
    /// the same rows.
    fn sort_matches(&mut self) {
        self.matches.sort_by(|a, b| b.score.cmp(&a.score).then(a.index.cmp(&b.index)));
    }

    fn clamp_selection(&mut self) {
        if self.matches.is_empty() {
            self.selection = 0;
        } else if self.selection >= self.matches.len() {
            self.selection = self.matches.len() - 1;
        }
    }
}

/// Score one candidate against the query, or `None` when the query is not a
/// subsequence of it.
///
/// Higher is better. The span between the first and last matched character is
/// penalised, matches at a path or word boundary are rewarded, and an exact
/// prefix is rewarded again — the shape most pickers converge on. Case is
/// folded, so `Alpha` matches `alpha`.
fn score(text: &str, query: &[char]) -> Option<(i32, Vec<u32>)> {
    if query.is_empty() {
        return Some((0, Vec::new()));
    }
    let mut positions = Vec::with_capacity(query.len());
    let mut qi = 0usize;
    let mut previous_was_boundary = true;
    let mut bonus = 0i32;
    for (offset, ch) in text.chars().enumerate() {
        if qi >= query.len() {
            break;
        }
        let hit = ch.to_lowercase().eq(query[qi].to_lowercase());
        if hit {
            positions.push(offset as u32);
            if previous_was_boundary {
                bonus += 8;
            }
            if offset == qi {
                bonus += 4;
            }
            qi += 1;
        }
        previous_was_boundary = matches!(ch, '/' | '_' | '-' | '.' | ' ');
    }
    if qi < query.len() {
        return None;
    }
    let span = (positions[positions.len() - 1] - positions[0] + 1) as i32;
    let length = text.chars().count() as i32;
    Some((bonus - span - length / 32, positions))
}

// ---------------------------------------------------------------------------
// Measurement
// ---------------------------------------------------------------------------

/// One scripted operation.
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    Push(char),
    Pop,
    Move(i64),
}

/// Parse a script: plain characters are typed, `<bs>` deletes, `<c-n>` and
/// `<c-p>` move the selection. The names match the keys telescope binds, so the
/// two halves of the measurement can be driven by the same string.
pub fn parse_script(script: &str) -> Vec<Op> {
    let mut ops = Vec::new();
    let chars: Vec<char> = script.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '<' {
            if let Some(end) = chars[i..].iter().position(|c| *c == '>') {
                let name: String = chars[i + 1..i + end].iter().collect::<String>().to_lowercase();
                let op = match name.as_str() {
                    "bs" => Some(Op::Pop),
                    "c-n" | "down" => Some(Op::Move(1)),
                    "c-p" | "up" => Some(Op::Move(-1)),
                    _ => None,
                };
                if let Some(op) = op {
                    ops.push(op);
                    i += end + 1;
                    continue;
                }
            }
        }
        ops.push(Op::Push(chars[i]));
        i += 1;
    }
    ops
}

#[derive(Serialize)]
pub struct StepReport {
    pub op: String,
    pub query_after: String,
    pub rows_before: usize,
    pub rows_after: usize,
    pub narrowed: bool,
    pub state_update_ms: f64,
    pub rows_build_ms: f64,
    pub selection_after: Option<usize>,
    /// Which candidate the selection is on, not just which row. A row number
    /// alone cannot be checked against the corpus afterwards.
    pub selected_after: Option<String>,
    /// True when the query matched nothing. Reported rather than left implicit,
    /// because `rows_after: 0` and "the picker is showing an empty list" are the
    /// same observation and a reader should not have to infer one from the other.
    pub empty_after: bool,
}

#[derive(Serialize)]
pub struct PickerBenchReport {
    pub schema: &'static str,
    pub arrangement: &'static str,
    pub records_for: Vec<&'static str>,
    pub decides: &'static str,
    pub corpus_entries: usize,
    pub visible_rows: usize,
    pub script: String,
    /// Round trips to another process per keystroke. Zero here by construction:
    /// the state is in this address space. This is the figure that belongs to
    /// the arrangement rather than to the matcher.
    pub process_boundaries_crossed_per_keystroke: usize,
    pub rpc_requests_per_keystroke: usize,
    pub state_update_ms: Option<Summary>,
    pub rows_build_ms: Option<Summary>,
    pub steps: Vec<StepReport>,
    pub matcher_note: &'static str,
    pub notes: Vec<&'static str>,
}

/// Run a scripted session against a host-owned picker and report what it cost.
pub fn bench(corpus: Vec<String>, script: &str, visible_rows: usize) -> PickerBenchReport {
    let ops = parse_script(script);
    let mut state = PickerState::new(corpus);
    let corpus_entries = state.corpus_len();
    let mut update = Samples::default();
    let mut build = Samples::default();
    let mut steps = Vec::new();

    for op in &ops {
        let started = Instant::now();
        let (label, delta) = match op {
            Op::Push(ch) => (format!("push {ch}"), state.push_char(*ch)),
            Op::Pop => ("pop".to_string(), state.pop_char()),
            Op::Move(by) => {
                let before = state.len();
                state.move_selection(*by);
                (format!("move {by}"), Delta { rows_before: before, rows_after: before, narrowed: false })
            }
        };
        let state_update_ms = started.elapsed().as_secs_f64() * 1000.0;

        let started = Instant::now();
        let rows = state.rows(0, visible_rows);
        let rows_build_ms = started.elapsed().as_secs_f64() * 1000.0;
        debug_assert!(rows.len() <= visible_rows);

        update.push(state_update_ms);
        build.push(rows_build_ms);
        steps.push(StepReport {
            op: label,
            query_after: state.query().to_string(),
            rows_before: delta.rows_before,
            rows_after: delta.rows_after,
            narrowed: delta.narrowed,
            state_update_ms: (state_update_ms * 10_000.0).round() / 10_000.0,
            rows_build_ms: (rows_build_ms * 10_000.0).round() / 10_000.0,
            selection_after: state.selection_row(),
            selected_after: state.selection().map(str::to_string),
            empty_after: state.is_empty(),
        });
    }

    PickerBenchReport {
        schema: "neovim-glsl.picker-state-host-owned/v1",
        arrangement: "host_owned_state",
        records_for: vec![
            "open_question neovim_glsl.navigation_state_owner",
            "open_question neovim_glsl.navigation_input_routing",
        ],
        decides: "nothing; both questions remain open at spec v0.9",
        corpus_entries,
        visible_rows,
        script: script.to_string(),
        process_boundaries_crossed_per_keystroke: 0,
        rpc_requests_per_keystroke: 0,
        state_update_ms: update.summary(),
        rows_build_ms: build.summary(),
        steps,
        matcher_note: "a span-penalised subsequence matcher, not telescope's fzy; \
durations here are not comparable to the plugin-owned half as algorithms, only \
as arrangements",
        notes: vec![
            "The timings are fresh observations of this machine on this run.",
            "The corpus is supplied by the caller so both halves filter the same input.",
            "Narrowing on push is sound because appending a character cannot admit a new candidate.",
        ],
    }
}

pub fn to_json(report: &PickerBenchReport) -> String {
    serde_json::to_string_pretty(report).expect("report serialises") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn corpus() -> Vec<String> {
        vec![
            "alpha.glsl".to_string(),
            "beta.glsl".to_string(),
            "shader/water.vert".to_string(),
            "shader/lighting.frag".to_string(),
            "moving/move_me.glsl".to_string(),
            "moving/keep_me.txt".to_string(),
            "日本語/描画.md".to_string(),
            "README.md".to_string(),
        ]
    }

    #[test]
    fn an_empty_query_matches_everything_in_corpus_order() {
        let state = PickerState::new(corpus());
        assert_eq!(state.len(), 8);
        let rows = state.rows(0, 8);
        assert_eq!(rows[0].text, "alpha.glsl");
        assert_eq!(rows[7].text, "README.md");
    }

    #[test]
    fn narrowing_agrees_with_filtering_from_the_corpus() {
        for query in ["a", "al", "alpha", "sh", "shw", "mvg", "md", "描画", "zzz"] {
            let mut narrowed = PickerState::new(corpus());
            for ch in query.chars() {
                narrowed.push_char(ch);
            }
            let mut whole = PickerState::new(corpus());
            for ch in query.chars() {
                whole.query.push(ch);
            }
            whole.refilter();
            assert_eq!(
                narrowed.rows(0, 64),
                whole.rows(0, 64),
                "narrowing disagreed with a full refilter for {query:?}"
            );
        }
    }

    #[test]
    fn a_query_that_matches_nothing_leaves_no_selection() {
        let mut state = PickerState::new(corpus());
        for ch in "zzzz".chars() {
            state.push_char(ch);
        }
        assert!(state.is_empty());
        assert_eq!(state.selection_row(), None);
        assert_eq!(state.selection(), None);
        assert!(state.rows(0, 8).is_empty());
    }

    #[test]
    fn backspace_widens_the_set_again() {
        let mut state = PickerState::new(corpus());
        for ch in "alpha".chars() {
            state.push_char(ch);
        }
        let narrow = state.len();
        let delta = state.pop_char();
        assert!(!delta.narrowed);
        assert!(state.len() >= narrow);
        assert_eq!(state.query(), "alph");
    }

    #[test]
    fn selection_clamps_at_both_ends() {
        let mut state = PickerState::new(corpus());
        state.move_selection(-5);
        assert_eq!(state.selection_row(), Some(0));
        state.move_selection(100);
        assert_eq!(state.selection_row(), Some(7));
    }

    #[test]
    fn selection_survives_a_shrinking_set() {
        let mut state = PickerState::new(corpus());
        state.move_selection(7);
        assert_eq!(state.selection_row(), Some(7));
        for ch in "sh".chars() {
            state.push_char(ch);
        }
        let row = state.selection_row().expect("still something selected");
        assert!(row < state.len(), "selection {row} outside {} rows", state.len());
    }

    #[test]
    fn matching_folds_case_and_accepts_non_ascii() {
        let mut state = PickerState::new(corpus());
        for ch in "ALPHA".chars() {
            state.push_char(ch);
        }
        assert_eq!(state.selection(), Some("alpha.glsl"));

        let mut state = PickerState::new(corpus());
        for ch in "描画".chars() {
            state.push_char(ch);
        }
        assert_eq!(state.len(), 1);
        assert_eq!(state.selection(), Some("日本語/描画.md"));
    }

    #[test]
    fn positions_point_at_the_matched_characters() {
        let mut state = PickerState::new(corpus());
        for ch in "alp".chars() {
            state.push_char(ch);
        }
        let row = &state.rows(0, 1)[0];
        assert_eq!(row.text, "alpha.glsl");
        assert_eq!(row.positions, vec![0, 1, 2]);
        assert!(row.selected);
    }

    #[test]
    fn a_boundary_match_outranks_a_scattered_one() {
        let corpus = vec![
            "xxwxxaxxtxxexxrxx".to_string(),
            "shader/water.vert".to_string(),
        ];
        let mut state = PickerState::new(corpus);
        for ch in "water".chars() {
            state.push_char(ch);
        }
        assert_eq!(state.rows(0, 1)[0].text, "shader/water.vert");
    }

    #[test]
    fn the_script_parses_the_keys_telescope_binds() {
        assert_eq!(
            parse_script("al<bs><c-n><c-p><down>"),
            vec![
                Op::Push('a'),
                Op::Push('l'),
                Op::Pop,
                Op::Move(1),
                Op::Move(-1),
                Op::Move(1),
            ]
        );
    }

    #[test]
    fn an_unknown_angle_name_is_typed_rather_than_guessed() {
        assert_eq!(parse_script("<nope>"), parse_script("<nope>"));
        let ops = parse_script("<x>");
        assert_eq!(ops.first(), Some(&Op::Push('<')));
    }

    #[test]
    fn the_report_names_the_selected_candidate_and_says_when_nothing_matched() {
        let report = bench(corpus(), "alp", 10);
        let last = report.steps.last().expect("three steps");
        assert_eq!(last.selected_after.as_deref(), Some("alpha.glsl"));
        assert!(!last.empty_after);

        let report = bench(corpus(), "zzzz", 10);
        let last = report.steps.last().expect("four steps");
        assert!(last.empty_after);
        assert_eq!(last.selected_after, None);
        assert_eq!(last.rows_after, 0);
    }

    #[test]
    fn the_report_states_that_it_decides_nothing() {
        let report = bench(corpus(), "al<c-n>", 10);
        assert_eq!(report.process_boundaries_crossed_per_keystroke, 0);
        assert_eq!(report.rpc_requests_per_keystroke, 0);
        assert_eq!(report.steps.len(), 3);
        assert!(report.decides.contains("nothing"));
        assert!(report.state_update_ms.is_some());
        let json = to_json(&report);
        assert!(json.contains("navigation_state_owner"));
    }
}
