//! Read-only VCS state held by the editing core.
//!
//! This module is data and pure queries only. Process spawning belongs to
//! `crate::git`; the editor should know what a hunk is, not how Git is run.
//!
//! Decisions left open by the ledger and chosen here:
//! - A non-repository buffer shows no signs and reports `NotRepository`.
//! - A repository with no commit is `Unborn`; every buffer line is an addition.
//! - Detached HEAD is a distinct label supplied by the host.
//! - Large buffers use `TooLarge` and the host must not spawn Git for them.
//! - Blame belongs to the buffer revision it was computed for; after an edit,
//!   cursor-line blame is suppressed until blame is refreshed.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SignKind {
    Add,
    Change,
    Delete,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Hunk {
    pub old_start: usize,
    pub old_len: usize,
    pub new_start: usize,
    pub new_len: usize,
    pub kind: SignKind,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum HeadLabel {
    Branch(String),
    Detached(String),
    Unborn,
}

impl HeadLabel {
    pub fn text(&self) -> &str {
        match self {
            HeadLabel::Branch(name) => name,
            HeadLabel::Detached(short) => short,
            HeadLabel::Unborn => "unborn",
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum VcsStatus {
    Unknown,
    Ready,
    NotRepository,
    Unborn,
    TooLarge,
    Error(String),
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BlameLine {
    pub line: usize,
    pub commit: String,
    pub author: String,
    pub time: Option<i64>,
    pub summary: String,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct VcsState {
    pub status: VcsStatus,
    pub head: Option<HeadLabel>,
    pub hunks: Vec<Hunk>,
    pub deleted_above: usize,
    pub blame: Vec<BlameLine>,
    pub diff_revision: Option<u64>,
    pub blame_revision: Option<u64>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Counts {
    pub added: usize,
    pub changed: usize,
    pub removed: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VcsRequest {
    Blame,
    Hunks,
    Diff,
}

impl Default for VcsState {
    fn default() -> Self {
        Self {
            status: VcsStatus::Unknown,
            head: None,
            hunks: Vec::new(),
            deleted_above: 0,
            blame: Vec::new(),
            diff_revision: None,
            blame_revision: None,
        }
    }
}

impl VcsState {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn sign_at(&self, line: usize, line_count: usize) -> Option<SignKind> {
        for hunk in &self.hunks {
            match hunk.kind {
                SignKind::Add | SignKind::Change => {
                    if line >= hunk.new_start && line < hunk.new_start + hunk.new_len {
                        return Some(hunk.kind);
                    }
                }
                SignKind::Delete => {
                    let marker = if hunk.new_start >= line_count {
                        line_count.saturating_sub(1)
                    } else {
                        hunk.new_start
                    };
                    if hunk.new_start > 0 && line == marker {
                        return Some(SignKind::Delete);
                    }
                }
            }
        }
        None
    }

    pub fn next_hunk(&self, cursor_line: usize, forward: bool) -> Option<usize> {
        if self.hunks.is_empty() {
            return None;
        }
        let mut starts: Vec<usize> = self.hunks.iter().map(|hunk| hunk.new_start).collect();
        starts.sort_unstable();
        starts.dedup();
        if forward {
            starts
                .iter()
                .copied()
                .find(|line| *line > cursor_line)
                .or_else(|| starts.first().copied())
        } else {
            starts
                .iter()
                .rev()
                .copied()
                .find(|line| *line < cursor_line)
                .or_else(|| starts.last().copied())
        }
    }

    pub fn counts(&self) -> Counts {
        let mut counts = Counts {
            added: 0,
            changed: 0,
            removed: 0,
        };
        for hunk in &self.hunks {
            match hunk.kind {
                SignKind::Add => counts.added += hunk.new_len,
                SignKind::Change => counts.changed += hunk.new_len.max(hunk.old_len),
                SignKind::Delete => counts.removed += hunk.old_len,
            }
        }
        counts
    }

    pub fn cursor_blame(&self, line: usize, revision: u64) -> Option<&BlameLine> {
        if self.blame_revision != Some(revision) {
            return None;
        }
        self.blame.iter().find(|entry| entry.line == line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deletion_blocks_mark_the_next_line_but_not_above_line_zero() {
        let mut vcs = VcsState::default();
        vcs.hunks = vec![
            Hunk {
                old_start: 0,
                old_len: 1,
                new_start: 0,
                new_len: 0,
                kind: SignKind::Delete,
            },
            Hunk {
                old_start: 3,
                old_len: 2,
                new_start: 2,
                new_len: 0,
                kind: SignKind::Delete,
            },
        ];
        assert_eq!(vcs.sign_at(0, 3), None);
        assert_eq!(vcs.sign_at(2, 3), Some(SignKind::Delete));
    }

    #[test]
    fn hunk_motion_wraps_in_both_directions() {
        let mut vcs = VcsState::default();
        vcs.hunks = vec![
            Hunk {
                old_start: 0,
                old_len: 0,
                new_start: 2,
                new_len: 1,
                kind: SignKind::Add,
            },
            Hunk {
                old_start: 4,
                old_len: 1,
                new_start: 8,
                new_len: 1,
                kind: SignKind::Change,
            },
        ];
        assert_eq!(vcs.next_hunk(2, true), Some(8));
        assert_eq!(vcs.next_hunk(8, true), Some(2));
        assert_eq!(vcs.next_hunk(8, false), Some(2));
        assert_eq!(vcs.next_hunk(2, false), Some(8));
    }
}
