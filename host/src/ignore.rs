//! File tree ignore rules.
//!
//! This is intentionally smaller than git's full ignore language. When a rule
//! cannot be interpreted, the file is shown; hiding too much is worse than
//! showing a little noise in the tree.

use std::path::{Component, Path, PathBuf};

const BUILTIN: [&str; 6] = [
    ".git",
    "target",
    "node_modules",
    ".venv",
    "dist",
    "__pycache__",
];

#[derive(Clone, Debug)]
pub struct IgnoreRules {
    root: PathBuf,
    patterns: Vec<Pattern>,
}

#[derive(Clone, Debug)]
struct Pattern {
    text: String,
    negated: bool,
    anchored: bool,
    dir_only: bool,
    has_slash: bool,
}

impl IgnoreRules {
    pub fn load(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let mut rules = Self {
            root: root.clone(),
            patterns: Vec::new(),
        };
        let path = root.join(".gitignore");
        if let Ok(text) = std::fs::read_to_string(path) {
            rules.patterns = parse_gitignore(&text);
        }
        rules
    }

    pub fn empty(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            patterns: Vec::new(),
        }
    }

    pub fn ignored(&self, path: &Path, is_dir: bool) -> bool {
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| BUILTIN.contains(&name))
        {
            return true;
        }

        let Some(relative) = relative_components(&self.root, path) else {
            return false;
        };
        let mut ignored = false;
        for pattern in &self.patterns {
            if pattern.dir_only && !is_dir {
                continue;
            }
            if pattern.matches(&relative) {
                ignored = !pattern.negated;
            }
        }
        ignored
    }
}

fn parse_gitignore(text: &str) -> Vec<Pattern> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let (negated, line) = line
                .strip_prefix('!')
                .map(|rest| (true, rest))
                .unwrap_or((false, line));
            if line.is_empty() {
                return None;
            }
            let (dir_only, line) = line
                .strip_suffix('/')
                .map(|rest| (true, rest))
                .unwrap_or((false, line));
            let (anchored, line) = line
                .strip_prefix('/')
                .map(|rest| (true, rest))
                .unwrap_or((false, line));
            if line.is_empty() {
                return None;
            }
            Some(Pattern {
                text: line.to_string(),
                negated,
                anchored,
                dir_only,
                has_slash: line.contains('/'),
            })
        })
        .collect()
}

impl Pattern {
    fn matches(&self, path: &[String]) -> bool {
        if path.is_empty() {
            return false;
        }
        let pattern: Vec<&str> = self
            .text
            .split('/')
            .filter(|part| !part.is_empty())
            .collect();
        if pattern.is_empty() {
            return false;
        }
        if self.anchored || self.has_slash {
            return match_components(&pattern, path);
        }
        path.iter()
            .any(|component| glob_segment(&self.text, component))
    }
}

fn relative_components(root: &Path, path: &Path) -> Option<Vec<String>> {
    let relative = path.strip_prefix(root).ok().unwrap_or(path);
    let mut out = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(name) => out.push(name.to_string_lossy().to_string()),
            Component::CurDir => {}
            _ => return None,
        }
    }
    Some(out)
}

fn match_components(pattern: &[&str], path: &[String]) -> bool {
    if pattern.is_empty() {
        return path.is_empty();
    }
    if pattern[0] == "**" {
        return match_components(&pattern[1..], path)
            || (!path.is_empty() && match_components(pattern, &path[1..]));
    }
    if path.is_empty() {
        return false;
    }
    glob_segment(pattern[0], &path[0]) && match_components(&pattern[1..], &path[1..])
}

fn glob_segment(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    glob_chars(&pattern, &text)
}

fn glob_chars(pattern: &[char], text: &[char]) -> bool {
    match (pattern.split_first(), text.split_first()) {
        (None, None) => true,
        (None, Some(_)) => false,
        (Some((&'*', rest)), _) => {
            glob_chars(rest, text) || (!text.is_empty() && glob_chars(pattern, &text[1..]))
        }
        (Some((&'?', rest)), Some((_, text_rest))) => glob_chars(rest, text_rest),
        (Some((want, rest)), Some((got, text_rest))) if want == got => glob_chars(rest, text_rest),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "nvimglsl-ignore-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn builtin_noise_is_always_hidden() {
        let root = temp("builtin");
        let rules = IgnoreRules::empty(&root);
        assert!(rules.ignored(&root.join("target"), true));
        assert!(rules.ignored(&root.join("src/target"), true));
        assert!(!rules.ignored(&root.join("src/main.rs"), false));
    }

    #[test]
    fn gitignore_subset_honours_last_match_and_negation() {
        let root = temp("gitignore");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join(".gitignore"),
            "# comment\n*.log\n!important.log\n/build/\n/src/**/tmp?.rs\n",
        )
        .unwrap();
        let rules = IgnoreRules::load(&root);
        assert!(rules.ignored(&root.join("notes/debug.log"), false));
        assert!(!rules.ignored(&root.join("important.log"), false));
        assert!(rules.ignored(&root.join("build"), true));
        assert!(!rules.ignored(&root.join("docs/build"), true));
        assert!(rules.ignored(&root.join("src/a/b/tmp1.rs"), false));
        assert!(!rules.ignored(&root.join("src/a/b/tmp-long.rs"), false));
        let _ = std::fs::remove_dir_all(root);
    }
}
