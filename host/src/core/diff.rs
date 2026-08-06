//! Line diff between a HEAD blob and the current buffer text.
//!
//! This deliberately does not parse `git diff`: the reference is a blob and the
//! right hand side is the buffer, including edits that have not been written to
//! disk.

use super::vcs::{Hunk, SignKind};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Op {
    Equal,
    Delete,
    Add,
}

pub fn hunks_from_text(head: &str, buffer: &[String]) -> (Vec<Hunk>, usize) {
    let old = split_lines(head);
    hunks(&old, buffer)
}

fn split_lines(text: &str) -> Vec<String> {
    let body = text.strip_suffix('\n').unwrap_or(text);
    if body.is_empty() {
        return Vec::new();
    }
    body.split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_string())
        .collect()
}

pub fn hunks(old: &[String], new: &[String]) -> (Vec<Hunk>, usize) {
    let ops = diff_ops(old, new);
    let mut hunks = Vec::new();
    let mut old_line = 0usize;
    let mut new_line = 0usize;
    let mut index = 0usize;
    let mut deleted_above = 0usize;

    while index < ops.len() {
        if ops[index] == Op::Equal {
            old_line += 1;
            new_line += 1;
            index += 1;
            continue;
        }

        let old_start = old_line;
        let new_start = new_line;
        let mut old_len = 0usize;
        let mut new_len = 0usize;
        while index < ops.len() && ops[index] != Op::Equal {
            match ops[index] {
                Op::Delete => {
                    old_len += 1;
                    old_line += 1;
                }
                Op::Add => {
                    new_len += 1;
                    new_line += 1;
                }
                Op::Equal => unreachable!(),
            }
            index += 1;
        }

        if old_len > 0 && new_len == 0 && new_start == 0 {
            deleted_above += old_len;
        }
        let kind = match (old_len, new_len) {
            (0, _) => SignKind::Add,
            (_, 0) => SignKind::Delete,
            _ => SignKind::Change,
        };
        hunks.push(Hunk {
            old_start,
            old_len,
            new_start,
            new_len,
            kind,
        });
    }

    (hunks, deleted_above)
}

fn diff_ops(old: &[String], new: &[String]) -> Vec<Op> {
    let rows = old.len();
    let cols = new.len();
    let mut dp = vec![vec![0usize; cols + 1]; rows + 1];
    for i in (0..rows).rev() {
        for j in (0..cols).rev() {
            dp[i][j] = if old[i] == new[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }

    let mut out = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < rows && j < cols {
        if old[i] == new[j] {
            out.push(Op::Equal);
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            out.push(Op::Delete);
            i += 1;
        } else {
            out.push(Op::Add);
            j += 1;
        }
    }
    out.extend(std::iter::repeat(Op::Delete).take(rows - i));
    out.extend(std::iter::repeat(Op::Add).take(cols - j));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn additions_changes_and_deletions_become_hunks() {
        let old = lines(&["one", "two", "three", "gone"]);
        let new = lines(&["zero", "one", "TWO", "three"]);
        let (hunks, deleted_above) = hunks(&old, &new);
        assert_eq!(deleted_above, 0);
        assert_eq!(hunks.len(), 3);
        assert_eq!(hunks[0].kind, SignKind::Add);
        assert_eq!(hunks[1].kind, SignKind::Change);
        assert_eq!(hunks[2].kind, SignKind::Delete);
        assert_eq!(hunks[2].new_start, 4);
    }

    #[test]
    fn deletion_before_the_first_line_is_counted_separately() {
        let old = lines(&["gone", "stay"]);
        let new = lines(&["stay"]);
        let (hunks, deleted_above) = hunks(&old, &new);
        assert_eq!(deleted_above, 1);
        assert_eq!(hunks[0].kind, SignKind::Delete);
        assert_eq!(hunks[0].new_start, 0);
    }
}
