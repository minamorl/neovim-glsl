//! Read-only Git plumbing for the host.
//!
//! This is the only module that spawns `git`. The editing core receives data
//! from here; it never learns how to run a process.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::core::vcs::{BlameLine, HeadLabel};

const DEADLINE: Duration = Duration::from_millis(1500);

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

pub fn head_blob(repo: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(repo).ok().unwrap_or(path);
    let rel = rel.to_string_lossy();
    let tree = run_git(repo, &["ls-tree", "-z", "HEAD", "--", &rel], None)?;
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
    String::from_utf8(blob.stdout).ok()
}

pub fn blame(repo: &Path, path: &Path, contents: &str) -> Option<Vec<BlameLine>> {
    let rel = path.strip_prefix(repo).ok().unwrap_or(path);
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
    let stdout_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });
    let stderr_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf);
        buf
    });

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

    let stdout = stdout_reader.join().unwrap_or_default();
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
}
