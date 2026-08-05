//! The vertical-writing preview.
//!
//! A way to read the owner's note in the shape it would have if it were really
//! typeset. `pin primary_object` makes a markdown note the first-class object,
//! and a first-class object that can only be *edited* is half implemented: the
//! form it takes when read is part of what it is.
//!
//! **No typesetting engine is written here.** The page is set by
//! `assets/tategaki.css` and whatever engine reads it. CSS Writing Modes'
//! vertical Japanese — `writing-mode: vertical-rl`, `text-orientation`,
//! `text-combine-upright`, `ruby`, `line-break: strict`, `hanging-punctuation` —
//! is the same path electronic books actually travel, with mincho metrics,
//! kinsoku line-breaking rules and punctuation compression already implemented.
//! Rewriting that here would produce a worse copy of something already correct.
//!
//! Three parts, and no more:
//!
//! - [`doc`] — read markdown into a document that knows what it is. Ruby,
//!   kenten and tate-chu-yoko have no markdown syntax, so they are made here.
//! - [`style`] — the page's dimensions: characters per line, line advance,
//!   lines per page, type size, paper. The CSS custom property names are the
//!   stylesheet's; this is a copy of them, and a test says so.
//! - [`html`] — assemble one self-contained page from a document and a style.
//!
//! **Nothing is drawn.** The output is a string, and whether it is written to a
//! file or handed to an engine is the caller's decision. A GLSL surface wanting
//! the same document would enter at [`doc::Document`] — which is part of why
//! the page's dimensions are kept out of the document model.

pub mod doc;
pub mod html;
pub mod style;

use std::io;
use std::path::{Path, PathBuf};

pub use doc::Document;
pub use html::render;
pub use style::{Scheme, Style};

/// Typeset `markdown` and write the page to `to`.
pub fn write(markdown: &str, style: &Style, to: &Path) -> io::Result<()> {
    if let Some(parent) = to.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(to, render(&Document::from_markdown(markdown), style))
}

/// Where a preview of `name` goes when the caller did not say.
///
/// The path is derived from the note's name rather than made unique, so
/// previewing the same note twice replaces the same file and an already-open
/// tab shows the new setting on reload. A fresh file each time would leave a
/// trail of stale pages and a browser full of tabs that all claim to be current.
pub fn preview_path(name: &str) -> PathBuf {
    let slug: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug.trim_matches('-').to_string();
    let slug = if slug.is_empty() {
        "note".to_string()
    } else {
        slug
    };
    std::env::temp_dir().join(format!("nvimglsl-tategaki-{slug}.html"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_preview_path_is_stable_for_the_same_note() {
        assert_eq!(preview_path("kusamakura.md"), preview_path("kusamakura.md"));
    }

    #[test]
    fn a_preview_path_survives_a_name_that_is_not_a_filename() {
        let path = preview_path("日記/2026-08-05.md");
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        assert!(name.starts_with("nvimglsl-tategaki-"), "{name}");
        assert!(name.ends_with(".html"), "{name}");
        assert!(!name.contains('/'), "{name}");
    }

    #[test]
    fn an_unnamed_buffer_still_has_somewhere_to_go() {
        let path = preview_path("");
        assert!(
            path.to_string_lossy()
                .ends_with("nvimglsl-tategaki-note.html"),
            "{path:?}"
        );
    }

    #[test]
    fn writing_a_preview_produces_a_page_that_stands_alone() {
        let to = std::env::temp_dir().join("nvimglsl-tategaki-write-test.html");
        write("# 題\n\n本文。", &Style::default(), &to).expect("write");
        let page = std::fs::read_to_string(&to).expect("read");
        assert!(page.contains("writing-mode: vertical-rl"));
        assert!(page.contains("<title>題</title>"));
        let _ = std::fs::remove_file(&to);
    }
}
