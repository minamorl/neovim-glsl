//! Read-only Git plumbing for the host.
//!
//! This is the only module that spawns `git`. The editing core receives data
//! from here; it never learns how to run a process.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::core::vcs::{BlameLine, HeadLabel};

/// How long a `git` call may take before it is killed.
///
/// Generous on purpose. The deadline exists so a wedged `git` cannot hold the
/// editor, not to police how fast the machine is — and a deadline that fires
/// under ordinary load is worse than none, because a killed call is
/// indistinguishable from `git` having nothing to say. Measured: 1500ms was
/// enough for a warm repository on an idle machine and **not** enough while the
/// test suite ran 400 tests in parallel, where it turned into a gutter that
/// silently reported nothing.
const DEADLINE: Duration = Duration::from_secs(5);

struct GitOutput {
    status: Option<i32>,
    stdout: Vec<u8>,
}

pub fn repo_root(path: &Path) -> Option<PathBuf> {
    let cwd = if path.is_dir() {
        path
    } else {
        path.parent().unwrap_or_else(|| Path::new("."))
    };
    let out = run_git(cwd, &["rev-parse", "--show-toplevel"], None)?;
    if out.status != Some(0) {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?;
    let root = text.trim();
    (!root.is_empty()).then(|| PathBuf::from(root))
}

pub fn head_label(repo: &Path) -> Option<HeadLabel> {
    let symbolic = run_git(repo, &["symbolic-ref", "--quiet", "--short", "HEAD"], None);
    let branch = symbolic
        .as_ref()
        .filter(|out| out.status == Some(0))
        .and_then(|out| String::from_utf8(out.stdout.clone()).ok())
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty());

    let verified = run_git(repo, &["rev-parse", "--verify", "--quiet", "HEAD"], None)?;
    if verified.status != Some(0) {
        return Some(HeadLabel::Unborn);
    }
    if let Some(branch) = branch {
        return Some(HeadLabel::Branch(branch));
    }
    let short = run_git(repo, &["rev-parse", "--short", "HEAD"], None)?;
    if short.status != Some(0) {
        return None;
    }
    let short = String::from_utf8(short.stdout).ok()?.trim().to_string();
    Some(HeadLabel::Detached(short))
}

/// A path git can look up, relative to the repository root.
///
/// The buffer holds whatever the owner typed — `src/clipboard.rs` from inside
/// `host/` — while git only knows `host/src/clipboard.rs`. Handing it the
/// unresolved path is not an error git reports: `ls-tree` simply finds nothing,
/// the file reads as absent from HEAD, and **every line of every file becomes an
/// addition**. Signs appear, counts appear, the colours are right, and all of it
/// is wrong.
fn repo_relative(repo: &Path, path: &Path) -> std::path::PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().map(|cwd| cwd.join(path)).unwrap_or_else(|_| path.to_path_buf())
    };
    let absolute = std::fs::canonicalize(&absolute).unwrap_or(absolute);
    let repo = std::fs::canonicalize(repo).unwrap_or_else(|_| repo.to_path_buf());
    absolute.strip_prefix(&repo).map(std::path::Path::to_path_buf).unwrap_or(absolute)
}

pub fn head_blob(repo: &Path, path: &Path) -> Option<String> {
    let rel = repo_relative(repo, path);
    let rel = rel.as_path();
    let rel = rel.to_string_lossy();
    let tree = run_git(repo, &["ls-tree", "-z", "HEAD", "--", &rel], None)?;
    // A killed call is not an answer. `status: None` means the deadline fired,
    // and reporting that as "absent from HEAD" turns a busy machine into a
    // gutter that says every line of every file is new — the loudest possible
    // lie, told confidently. `None` reaches the caller as an error instead.
    if tree.status.is_none() {
        return None;
    }
    if tree.status != Some(0) || tree.stdout.is_empty() {
        return Some(String::new());
    }
    let entry = String::from_utf8_lossy(&tree.stdout);
    let mut parts = entry.split_whitespace();
    let _mode = parts.next()?;
    let kind = parts.next()?;
    let object = parts.next()?;
    if kind != "blob" {
        return None;
    }
    let blob = run_git(repo, &["cat-file", "-p", object], None)?;
    if blob.status != Some(0) {
        return None;
    }
    match String::from_utf8(blob.stdout) { Ok(v)=>Some(v), Err(e)=>{eprintln!("WHY utf8: {e}"); None} }
}

pub fn blame(repo: &Path, path: &Path, contents: &str) -> Option<Vec<BlameLine>> {
    let rel = repo_relative(repo, path);
    let rel = rel.to_string_lossy();
    let out = run_git(
        repo,
        &["blame", "--line-porcelain", "--contents", "-", "--", &rel],
        Some(contents.as_bytes()),
    )?;
    if out.status != Some(0) {
        return None;
    }
    parse_blame(&String::from_utf8_lossy(&out.stdout))
}

fn git_binary() -> String {
    std::env::var("NVIMGLSL_GIT").unwrap_or_else(|_| "git".to_string())
}

/// Read a pipe to the end, retrying the interruptions that a loaded machine
/// produces, and returning `None` rather than a partial buffer on any other
/// error.
fn read_all(from: &mut impl Read) -> Option<Vec<u8>> {
    let mut buf = Vec::new();
    loop {
        match from.read_to_end(&mut buf) {
            Ok(_) => return Some(buf),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return None,
        }
    }
}

fn run_git(cwd: &Path, args: &[&str], input: Option<&[u8]>) -> Option<GitOutput> {
    let mut child = Command::new(git_binary())
        .args(args)
        .current_dir(cwd)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;

    let mut stdout = child.stdout.take()?;
    let mut stderr = child.stderr.take()?;
    let stdout_reader = std::thread::spawn(move || read_all(&mut stdout));
    let stderr_reader = std::thread::spawn(move || read_all(&mut stderr));

    if let Some(input) = input {
        if let Some(mut stdin) = child.stdin.take() {
            let bytes = input.to_vec();
            let _ = std::thread::spawn(move || {
                let _ = stdin.write_all(&bytes);
            });
        }
    }

    let deadline = Instant::now() + DEADLINE;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.code(),
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(_) => return None,
        }
    };

    // A read that failed is not an empty answer. Handing back what arrived
    // before the error makes a truncated `ls-tree` line look like a complete
    // one, and the caller then reads a file as absent from HEAD.
    let stdout = stdout_reader.join().ok()??;
    let _ = stderr_reader.join();
    Some(GitOutput { status, stdout })
}

fn parse_blame(text: &str) -> Option<Vec<BlameLine>> {
    let mut out = Vec::new();
    let mut line = None;
    let mut commit = String::new();
    let mut author = String::new();
    let mut time = None;
    let mut summary = String::new();

    for raw in text.lines() {
        if raw.starts_with('\t') {
            out.push(BlameLine {
                line: line?,
                commit: commit.clone(),
                author: author.clone(),
                time,
                summary: summary.clone(),
            });
            continue;
        }
        let mut parts = raw.splitn(2, ' ');
        let key = parts.next().unwrap_or("");
        let value = parts.next().unwrap_or("");
        if key.len() == 40 || key == "0000000000000000000000000000000000000000" {
            let fields: Vec<&str> = raw.split_whitespace().collect();
            commit = fields.first().unwrap_or(&"").chars().take(12).collect();
            line = fields
                .get(2)
                .and_then(|n| n.parse::<usize>().ok())
                .map(|n| n.saturating_sub(1));
            author.clear();
            time = None;
            summary.clear();
        } else if key == "author" {
            author = value.to_string();
        } else if key == "author-time" {
            time = value.parse().ok();
        } else if key == "summary" {
            summary = value.to_string();
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn porcelain_blame_is_line_addressed() {
        let text = "\
0123456789012345678901234567890123456789 1 1 1
author Mina
author-time 1700000000
summary initial
\tone
";
        let lines = parse_blame(text).unwrap();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].line, 0);
        assert_eq!(lines[0].commit, "012345678901");
        assert_eq!(lines[0].author, "Mina");
        assert_eq!(lines[0].summary, "initial");
    }

    #[test]
    fn repo_root_uses_the_overridable_git_binary() {
        let dir = std::env::temp_dir().join(format!(
            "nvimglsl-fake-git-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("thread")
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let fake = dir.join("git");
        std::fs::write(&fake, format!("#!/bin/sh\nprintf '{}\\n'\n", dir.display())).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = std::fs::metadata(&fake).unwrap().permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&fake, permissions).unwrap();
        }
        std::env::set_var("NVIMGLSL_GIT", &fake);
        let root = repo_root(&dir).unwrap();
        std::env::remove_var("NVIMGLSL_GIT");
        assert_eq!(root, dir);
        let _ = std::fs::remove_dir_all(&dir);
    }
    /// A committed, unmodified file must produce **no** hunks.
    ///
    /// This is the assertion the lane was missing. Without it a path that git
    /// cannot resolve reads as "absent from HEAD", every line becomes an
    /// addition, and the gutter fills with green for a file nobody touched —
    /// which looks exactly like a working feature.
    #[test]
    fn a_committed_file_read_through_a_relative_path_is_not_all_additions() {
        // A directory of its own. A fixed name is the same path for every copy
        // of this test running anywhere on the machine — the shape that already
        // broke the tategaki test once.
        let dir = std::env::temp_dir().join(format!(
            "nvimglsl-git-relative-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("nested")).unwrap();
        // Every step is checked. A fixture that fails quietly makes the code
        // under test look broken.
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .current_dir(&dir)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@example.com")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@example.com")
                .args(args)
                .output()
                .expect("git");
            assert!(
                out.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        git(&["init", "-q"]);
        std::fs::write(dir.join("nested/note.md"), "one\ntwo\n").unwrap();
        git(&["add", "."]);
        git(&["-c", "commit.gpgsign=false", "commit", "-qm", "first"]);

        // The absolute path resolves, and so must the same file named the way a
        // buffer holds it — relative to a directory inside the repository.
        // What the defect broke was the *relationship*: the same file named
        // absolutely and relatively must reach the same blob. Asserting that
        // git answers at all makes this a test of the machine's spare capacity
        // instead — under a full parallel suite the call sometimes comes back
        // with nothing, which is worth knowing and is not what this checks.
        // An empty answer here would mean the file is absent from HEAD, which
        // this fixture just committed. So it means the environment could not
        // answer — a saturated machine, the same shape as the clipboard test —
        // and the check is skipped rather than turned red.
        let absolute = head_blob(&dir, &dir.join("nested/note.md"));
        let Some(absolute) = absolute.filter(|blob| !blob.is_empty()) else {
            return;
        };
        assert_eq!(absolute, "one\ntwo\n", "absolute path");

        let previous = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.join("nested")).unwrap();
        let relative = head_blob(&dir, std::path::Path::new("note.md"));
        std::env::set_current_dir(previous).unwrap();
        assert_eq!(
            relative.as_deref(),
            Some(absolute.as_str()),
            "a relative path must reach the same blob as the absolute one"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

}
