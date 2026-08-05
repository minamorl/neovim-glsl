//! The `:` line.
//!
//! Only commands the owner's keymap actually reaches are here. An unknown
//! command reports `E492` rather than being ignored, because a silently
//! swallowed `:wq` looks exactly like a successful one until the file is gone.

use std::path::PathBuf;

use super::editor::{Editor, Message, Request, Scope};

pub fn execute(editor: &mut Editor, line: &str) {
    let line = line.trim();
    if line.is_empty() {
        return;
    }

    // `:42` is a line number, not a command name.
    if let Ok(number) = line.parse::<usize>() {
        let target = number.saturating_sub(1).min(editor.buffer.line_count() - 1);
        editor.cursor = (target, 0);
        return;
    }
    if line == "$" {
        editor.cursor = (editor.buffer.line_count() - 1, 0);
        return;
    }

    let range_all = line.starts_with('%');
    let body = if range_all { &line[1..] } else { line };
    if body.starts_with('s')
        && body.len() > 1
        && !body[1..].starts_with(|c: char| c.is_alphanumeric())
    {
        substitute(editor, &body[1..], range_all);
        return;
    }

    let (name, rest) = match body.find(' ') {
        Some(at) => (&body[..at], body[at + 1..].trim()),
        None => (body, ""),
    };
    let bang = name.ends_with('!');
    let name = name.trim_end_matches('!');

    match name {
        "w" | "write" => write(editor, rest, false),
        "wq" | "x" | "xit" => write(editor, rest, true),
        "q" | "quit" => {
            if editor.buffer.modified() && !bang {
                editor.message = Some(Message {
                    text: "E37: No write since last change (add ! to override)".into(),
                    error: true,
                });
            } else {
                editor.requests.push(Request::Quit);
            }
        }
        "qa" | "qall" | "quitall" => editor.requests.push(Request::Quit),
        "e" | "edit" => {
            if rest.is_empty() {
                match editor.buffer.path().map(PathBuf::from) {
                    Some(path) => editor.requests.push(Request::Edit(path)),
                    None => {
                        editor.message = Some(Message {
                            text: "E32: No file name".into(),
                            error: true,
                        })
                    }
                }
            } else if editor.buffer.modified() && !bang {
                editor.message = Some(Message {
                    text: "E37: No write since last change (add ! to override)".into(),
                    error: true,
                });
            } else {
                editor.requests.push(Request::Edit(expand(rest)));
            }
        }
        "noh" | "nohl" | "nohlsearch" => editor.last_search = None,
        // The navigation surface has `:` names as well as the `<Space>o`
        // mapping, so it can be reached without the leader.
        "Notes" | "notes" => editor.requests.push(Request::OpenNavigation(Scope::Notes)),
        "Files" | "files" | "Nav" | "nav" => {
            editor.requests.push(Request::OpenNavigation(Scope::Files))
        }
        "Note" | "note" => {
            if rest.is_empty() {
                editor.message = Some(Message {
                    text: "E471: Argument required: :Note <title>".into(),
                    error: true,
                });
            } else {
                editor.requests.push(Request::NewNote(rest.to_string()));
            }
        }
        _ if editor.is_plugin_command(name) => editor.requests.push(Request::Plugin {
            name: name.to_string(),
            argument: rest.to_string(),
        }),
        _ => {
            editor.message = Some(Message {
                text: format!("E492: Not an editor command: {name}"),
                error: true,
            });
        }
    }
}

fn write(editor: &mut Editor, rest: &str, then_quit: bool) {
    let target = if rest.is_empty() {
        None
    } else {
        Some(expand(rest))
    };
    match editor.buffer.write(target.as_deref()) {
        Ok(path) => {
            let lines = editor.buffer.line_count();
            editor.message = Some(Message {
                text: format!("\"{}\" {lines}L written", path.display()),
                error: false,
            });
            if then_quit {
                editor.requests.push(Request::Quit);
            }
        }
        Err(error) => {
            editor.message = Some(Message {
                text: format!("E212: {error}"),
                error: true,
            });
        }
    }
}

/// `:s/pattern/replacement/[g]` over the current line, or every line with `%`.
///
/// The pattern is a literal string. `free host_editing_core_design` does not ask
/// for a regular expression engine, and a half-implemented one that silently
/// treats `.` as any character would change what a replacement does without
/// saying so.
fn substitute(editor: &mut Editor, spec: &str, all_lines: bool) {
    let mut chars = spec.chars();
    let Some(sep) = chars.next() else { return };
    let parts: Vec<&str> = spec[sep.len_utf8()..].split(sep).collect();
    if parts.is_empty() || parts[0].is_empty() {
        editor.message = Some(Message {
            text: "E35: No previous regular expression".into(),
            error: true,
        });
        return;
    }
    let pattern = parts[0];
    let replacement = parts.get(1).copied().unwrap_or("");
    let global = parts.get(2).is_some_and(|flags| flags.contains('g'));

    let range = if all_lines {
        0..editor.buffer.line_count()
    } else {
        editor.cursor.0..editor.cursor.0 + 1
    };
    let mut changed = 0usize;
    let mut last_line = editor.cursor.0;
    editor.buffer.begin_change(editor.cursor);
    for index in range {
        let text = editor.buffer.line_text(index);
        if !text.contains(pattern) {
            continue;
        }
        let replaced = if global {
            text.replace(pattern, replacement)
        } else {
            text.replacen(pattern, replacement, 1)
        };
        editor
            .buffer
            .replace_line(index, replaced.chars().collect());
        changed += 1;
        last_line = index;
    }
    if changed == 0 {
        editor.buffer.abort_change();
        editor.message = Some(Message {
            text: format!("E486: Pattern not found: {pattern}"),
            error: true,
        });
        return;
    }
    editor.buffer.commit_change();
    editor.cursor = (last_line, 0);
    editor.message = Some(Message {
        text: format!(
            "{changed} substitution{} on {changed} line{}",
            plural(changed),
            plural(changed)
        ),
        error: false,
    });
}

fn plural(count: usize) -> &'static str {
    if count == 1 {
        ""
    } else {
        "s"
    }
}

fn expand(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::buffer::Buffer;

    fn editor(text: &str) -> Editor {
        Editor::new(Buffer::from_text(text))
    }

    #[test]
    fn a_bare_number_is_a_line_number() {
        let mut e = editor("one\ntwo\nthree\n");
        e.feed_str(":2<CR>");
        assert_eq!(e.cursor.0, 1);
    }

    #[test]
    fn quit_refuses_while_modified_and_bang_overrides() {
        let mut e = editor("one\n");
        e.feed_str("x");
        e.feed_str(":q<CR>");
        assert!(e.requests.is_empty());
        assert!(e.message.as_ref().unwrap().error);
        e.feed_str(":q!<CR>");
        assert_eq!(e.requests, vec![Request::Quit]);
    }

    #[test]
    fn substitute_replaces_on_the_current_line_only_without_a_range() {
        let mut e = editor("a a\na a\n");
        e.feed_str(":s/a/b/<CR>");
        assert_eq!(e.buffer.line_text(0), "b a");
        assert_eq!(e.buffer.line_text(1), "a a");
    }

    #[test]
    fn percent_s_with_g_replaces_everywhere() {
        let mut e = editor("a a\na a\n");
        e.feed_str(":%s/a/b/g<CR>");
        assert_eq!(e.buffer.line_text(0), "b b");
        assert_eq!(e.buffer.line_text(1), "b b");
    }

    #[test]
    fn a_substitution_that_matched_nothing_is_not_an_undo_step() {
        let mut e = editor("a\n");
        e.feed_str(":s/z/b/<CR>");
        assert!(e.message.as_ref().unwrap().error);
        assert!(!e.buffer.modified());
    }

    #[test]
    fn note_needs_a_title_and_passes_it_through() {
        let mut e = editor("");
        e.feed_str(":Note<CR>");
        assert!(e.message.as_ref().unwrap().error);
        e.feed_str(":Note daily/2026-08-03<CR>");
        assert_eq!(
            e.requests,
            vec![Request::NewNote("daily/2026-08-03".into())]
        );
    }

    #[test]
    fn an_unknown_command_says_so() {
        let mut e = editor("");
        e.feed_str(":frobnicate<CR>");
        assert!(e.message.as_ref().unwrap().text.starts_with("E492"));
    }

    #[test]
    fn writing_reports_the_path_and_clears_modified() {
        let dir = std::env::temp_dir().join("nvimglsl-command-test");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("note.md");
        let mut e = editor("one\n");
        e.feed_str("x");
        assert!(e.buffer.modified());
        e.feed_str(&format!(":w {}<CR>", path.display()));
        assert!(!e.buffer.modified());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "ne\n");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
