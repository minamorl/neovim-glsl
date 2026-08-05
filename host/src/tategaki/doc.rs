//! markdown → the vertical-writing document model.
//!
//! What is decided here is **what the text says**, never where it lands. Line
//! breaking, kinsoku, punctuation compression and ruby placement belong to
//! `assets/tategaki.css` and the engine reading it. What this module hands to
//! the typesetter is a tree with meaning on it, and nothing else.
//!
//! The split is not tidiness. Characters per line and kinsoku are only decided
//! once a font is chosen and the glyphs are actually set; a model that broke
//! lines would be making that decision without knowing the font.
//!
//! Only the judgements peculiar to vertical Japanese stay here:
//!
//! - **Ruby** is read in the Aozora Bunko notation (`｜base《reading》`, and
//!   `《》` directly after a run of kanji). Markdown has no ruby, so the
//!   notation the owner's notes can already carry was the one to take.
//! - **Kenten** (emphasis dots) come from `*emphasis*`. Japanese emphasis is
//!   sesame dots, not italics, and a vertical italic is not typesetting.
//! - **Tate-chu-yoko** applies to one- and two-digit numerals only. Longer runs
//!   staying sideways is not a defect; it is how vertical text is set. No engine
//!   implements `text-combine-upright: digits`, so which digits stand upright is
//!   decided here and wrapped in a span by [`super::html`].

/// Everything at or above the paragraph.
#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Heading {
        level: u8,
        runs: Vec<Run>,
    },
    Paragraph(Vec<Run>),
    Quote(Vec<Block>),
    List {
        ordered: bool,
        items: Vec<Vec<Run>>,
    },
    /// Programs are horizontal objects, so this is the one block that lies
    /// flat inside the vertical flow.
    Code {
        lang: Option<String>,
        text: String,
    },
    Rule,
}

/// Everything inside a line.
#[derive(Clone, Debug, PartialEq)]
pub enum Run {
    Text(String),
    /// Tate-chu-yoko: a one- or two-digit number standing upright in vertical text.
    Tcy(String),
    /// Ruby: `reading` set alongside `base`.
    Ruby {
        base: String,
        reading: String,
    },
    /// Kenten — emphasis dots.
    Emphasis(Vec<Run>),
    Strong(Vec<Run>),
    Code(String),
    Link {
        runs: Vec<Run>,
        target: String,
    },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Document {
    pub title: Option<String>,
    pub byline: Option<String>,
    pub blocks: Vec<Block>,
}

impl Document {
    pub fn from_markdown(source: &str) -> Self {
        let lines: Vec<&str> = source.lines().collect();
        let mut at = 0;

        let mut title = None;
        let mut byline = None;
        if let Some((front_title, front_byline, next)) = frontmatter(&lines) {
            title = front_title;
            byline = front_byline;
            at = next;
        }

        let mut blocks = parse_blocks(&lines[at..]);

        // With no title in the frontmatter the first heading becomes one. It
        // leaves the body when it does: a title set at the head and left in the
        // text as well is the same line read twice.
        if title.is_none() {
            if let Some(index) = blocks
                .iter()
                .position(|b| matches!(b, Block::Heading { level: 1, .. }))
            {
                if let Block::Heading { runs, .. } = &blocks[index] {
                    title = Some(plain_text(runs));
                }
                blocks.remove(index);
            }
        }

        Document {
            title,
            byline,
            blocks,
        }
    }
}

/// The YAML-ish frontmatter between leading `---` lines, which yui's notes
/// actually carry. Only `title` and `author` are read and the rest dropped:
/// this is not the place that decides a note's schema (`free note_schema` is
/// still open).
fn frontmatter(lines: &[&str]) -> Option<(Option<String>, Option<String>, usize)> {
    if lines.first().map(|l| l.trim_end()) != Some("---") {
        return None;
    }
    let close = lines.iter().skip(1).position(|l| l.trim_end() == "---")? + 1;

    let mut title = None;
    let mut byline = None;
    for line in &lines[1..close] {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .to_string();
        if value.is_empty() {
            continue;
        }
        match key.trim() {
            "title" => title = Some(value),
            "author" | "byline" => byline = Some(value),
            _ => {}
        }
    }
    Some((title, byline, close + 1))
}

fn parse_blocks(lines: &[&str]) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut at = 0;

    while at < lines.len() {
        let line = lines[at];
        let trimmed = line.trim();

        if trimmed.is_empty() {
            at += 1;
            continue;
        }

        // A fence. One that never closes still takes everything from the
        // opening line to the end of the document as its body.
        if let Some(fence) = trimmed
            .strip_prefix("```")
            .or_else(|| trimmed.strip_prefix("~~~"))
        {
            let lang = (!fence.trim().is_empty()).then(|| fence.trim().to_string());
            let mut body = Vec::new();
            at += 1;
            while at < lines.len() {
                let t = lines[at].trim();
                if t.starts_with("```") || t.starts_with("~~~") {
                    at += 1;
                    break;
                }
                body.push(lines[at]);
                at += 1;
            }
            blocks.push(Block::Code {
                lang,
                text: body.join("\n"),
            });
            continue;
        }

        if is_rule(trimmed) {
            blocks.push(Block::Rule);
            at += 1;
            continue;
        }

        if let Some((level, rest)) = heading(trimmed) {
            blocks.push(Block::Heading {
                level,
                runs: parse_runs(rest),
            });
            at += 1;
            continue;
        }

        if trimmed.starts_with('>') {
            let mut inner = Vec::new();
            while at < lines.len() && lines[at].trim().starts_with('>') {
                let t = lines[at].trim();
                inner.push(t[1..].strip_prefix(' ').unwrap_or(&t[1..]).to_string());
                at += 1;
            }
            let borrowed: Vec<&str> = inner.iter().map(|s| s.as_str()).collect();
            blocks.push(Block::Quote(parse_blocks(&borrowed)));
            continue;
        }

        if let Some((ordered, _)) = list_item(trimmed) {
            let mut items = Vec::new();
            while at < lines.len() {
                let Some((this_ordered, body)) = list_item(lines[at].trim()) else {
                    break;
                };
                if this_ordered != ordered {
                    break;
                }
                items.push(parse_runs(body));
                at += 1;
            }
            blocks.push(Block::List { ordered, items });
            continue;
        }

        // A paragraph: everything up to the next blank line, folded into one.
        let mut paragraph = String::new();
        while at < lines.len() {
            let t = lines[at].trim();
            if t.is_empty()
                || is_rule(t)
                || heading(t).is_some()
                || t.starts_with('>')
                || t.starts_with("```")
                || t.starts_with("~~~")
                || list_item(t).is_some()
            {
                break;
            }
            join_soft_break(&mut paragraph, t);
            at += 1;
        }
        if !paragraph.is_empty() {
            blocks.push(Block::Paragraph(parse_runs(&paragraph)));
        }
    }

    blocks
}

/// How a line break inside a paragraph is joined.
///
/// Markdown turns one into a single space, which is a rule about Latin text.
/// Doing it to Japanese leaves a one-character hole at that point in the set
/// line. So: a space only when both sides are Latin, and nothing when either
/// side is CJK.
fn join_soft_break(paragraph: &mut String, next: &str) {
    if paragraph.is_empty() {
        paragraph.push_str(next);
        return;
    }
    let last = paragraph.chars().last();
    let first = next.chars().next();
    let both_latin = matches!(last, Some(c) if c.is_ascii_alphanumeric() || c.is_ascii_punctuation())
        && matches!(first, Some(c) if c.is_ascii_alphanumeric());
    if both_latin {
        paragraph.push(' ');
    }
    paragraph.push_str(next);
}

fn is_rule(line: &str) -> bool {
    let stripped: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    stripped.len() >= 3
        && (stripped.chars().all(|c| c == '-')
            || stripped.chars().all(|c| c == '*')
            || stripped.chars().all(|c| c == '_'))
}

fn heading(line: &str) -> Option<(u8, &str)> {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = &line[hashes..];
    let body = rest.strip_prefix(' ')?;
    Some((hashes as u8, body.trim()))
}

fn list_item(line: &str) -> Option<(bool, &str)> {
    for marker in ["- ", "* ", "+ "] {
        if let Some(rest) = line.strip_prefix(marker) {
            return Some((false, rest));
        }
    }
    let digits = line.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 {
        let rest = &line[digits..];
        for marker in [". ", ") "] {
            if let Some(body) = rest.strip_prefix(marker) {
                return Some((true, body));
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Inline
// ---------------------------------------------------------------------------

pub fn parse_runs(source: &str) -> Vec<Run> {
    let chars: Vec<char> = source.chars().collect();
    let mut runs = Vec::new();
    let mut pending = String::new();
    // Where a `｜` opened a ruby base. Held as a byte offset rather than a
    // char count, because that is what `split_off` takes.
    let mut ruby_base_at: Option<usize> = None;
    let mut at = 0;

    while at < chars.len() {
        let c = chars[at];
        match c {
            // No notation is read inside a code span. `` `*` `` opens nothing.
            '`' => {
                if let Some(end) = find(&chars, at + 1, '`') {
                    flush(&mut runs, &mut pending, &mut ruby_base_at);
                    runs.push(Run::Code(chars[at + 1..end].iter().collect()));
                    at = end + 1;
                    continue;
                }
                pending.push(c);
                at += 1;
            }
            // Aozora ruby. `｜` only marks where the base text starts; it is
            // never set itself.
            '｜' => {
                ruby_base_at = Some(pending.len());
                at += 1;
            }
            '《' => {
                if let Some(end) = find(&chars, at + 1, '》') {
                    let reading: String = chars[at + 1..end].iter().collect();
                    let base = take_ruby_base(&mut pending, ruby_base_at.take());
                    match base {
                        Some(base) if !reading.is_empty() => {
                            flush(&mut runs, &mut pending, &mut ruby_base_at);
                            runs.push(Run::Ruby { base, reading });
                            at = end + 1;
                            continue;
                        }
                        // With no base to attach to, these are just the
                        // characters 《》 and not ruby at all.
                        _ => {
                            pending.push(c);
                            at += 1;
                        }
                    }
                } else {
                    pending.push(c);
                    at += 1;
                }
            }
            '*' if at + 1 < chars.len() && chars[at + 1] == '*' => {
                if let Some(end) = find_pair(&chars, at + 2, '*') {
                    flush(&mut runs, &mut pending, &mut ruby_base_at);
                    let inner: String = chars[at + 2..end].iter().collect();
                    runs.push(Run::Strong(parse_runs(&inner)));
                    at = end + 2;
                    continue;
                }
                pending.push(c);
                at += 1;
            }
            '*' | '_' => {
                if let Some(end) = find(&chars, at + 1, c) {
                    if end > at + 1 {
                        flush(&mut runs, &mut pending, &mut ruby_base_at);
                        let inner: String = chars[at + 1..end].iter().collect();
                        runs.push(Run::Emphasis(parse_runs(&inner)));
                        at = end + 1;
                        continue;
                    }
                }
                pending.push(c);
                at += 1;
            }
            '[' if at + 1 < chars.len() && chars[at + 1] == '[' => {
                if let Some(end) = find_str(&chars, at + 2, "]]") {
                    flush(&mut runs, &mut pending, &mut ruby_base_at);
                    let body: String = chars[at + 2..end].iter().collect();
                    let (target, label) = match body.split_once('|') {
                        Some((target, label)) => {
                            (target.trim().to_string(), label.trim().to_string())
                        }
                        None => (body.trim().to_string(), body.trim().to_string()),
                    };
                    runs.push(Run::Link {
                        runs: parse_runs(&label),
                        target,
                    });
                    at = end + 2;
                    continue;
                }
                pending.push(c);
                at += 1;
            }
            '[' => {
                if let Some(close) = find(&chars, at + 1, ']') {
                    if chars.get(close + 1) == Some(&'(') {
                        if let Some(paren) = find(&chars, close + 2, ')') {
                            flush(&mut runs, &mut pending, &mut ruby_base_at);
                            let label: String = chars[at + 1..close].iter().collect();
                            let target: String = chars[close + 2..paren].iter().collect();
                            runs.push(Run::Link {
                                runs: parse_runs(&label),
                                target,
                            });
                            at = paren + 1;
                            continue;
                        }
                    }
                }
                pending.push(c);
                at += 1;
            }
            _ => {
                pending.push(c);
                at += 1;
            }
        }
    }

    flush(&mut runs, &mut pending, &mut ruby_base_at);
    runs
}

fn flush(runs: &mut Vec<Run>, pending: &mut String, ruby_base_at: &mut Option<usize>) {
    if !pending.is_empty() {
        runs.extend(split_tcy(pending));
        pending.clear();
    }
    *ruby_base_at = None;
}

/// Take the ruby base from the text before `《》`: from `｜` if one was seen,
/// otherwise the run of kanji immediately before.
fn take_ruby_base(pending: &mut String, bar_at: Option<usize>) -> Option<String> {
    if let Some(at) = bar_at {
        let base = pending.split_off(at);
        return (!base.is_empty()).then_some(base);
    }
    let start = pending
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_kanji(*c))
        .last()
        .map(|(i, _)| i)?;
    let base = pending.split_off(start);
    (!base.is_empty()).then_some(base)
}

fn is_kanji(c: char) -> bool {
    matches!(c,
        '\u{3005}'          // 々
        | '\u{3006}'        // 〆
        | '\u{3007}'        // 〇
        | '\u{4E00}'..='\u{9FFF}'
        | '\u{3400}'..='\u{4DBF}'
        | '\u{F900}'..='\u{FAFF}')
}

/// Cut out the numerals that stand upright.
///
/// One and two digits only. Three or more staying sideways is ordinary vertical
/// setting, and rewriting 1999 as 一九九九 would be editing the text rather than
/// setting it.
///
/// A digit run touching a Latin letter, another digit, or `.` `_` `-` `/` is
/// left alone. The `0` in `v0.9` and the `8` in `UTF-8` are parts of an
/// identifier rather than numbers in the prose, and standing one of them upright
/// splits the word.
fn split_tcy(text: &str) -> Vec<Run> {
    let chars: Vec<char> = text.chars().collect();
    let mut runs = Vec::new();
    let mut plain = String::new();
    let mut at = 0;

    while at < chars.len() {
        if !chars[at].is_ascii_digit() {
            plain.push(chars[at]);
            at += 1;
            continue;
        }
        let end = at
            + chars[at..]
                .iter()
                .take_while(|c| c.is_ascii_digit())
                .count();
        let len = end - at;
        let glued = |c: Option<&char>| matches!(c, Some(c) if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/'));
        let before = if at > 0 { chars.get(at - 1) } else { None };
        let standalone = !glued(before) && !glued(chars.get(end));

        if (1..=2).contains(&len) && standalone {
            if !plain.is_empty() {
                runs.push(Run::Text(std::mem::take(&mut plain)));
            }
            runs.push(Run::Tcy(chars[at..end].iter().collect()));
        } else {
            plain.extend(&chars[at..end]);
        }
        at = end;
    }

    if !plain.is_empty() {
        runs.push(Run::Text(plain));
    }
    runs
}

fn find(chars: &[char], from: usize, needle: char) -> Option<usize> {
    (from..chars.len()).find(|&i| chars[i] == needle)
}

/// The close of `**`. A pair is required so a single `*` after the opener is
/// not mistaken for one.
fn find_pair(chars: &[char], from: usize, needle: char) -> Option<usize> {
    (from..chars.len().saturating_sub(1)).find(|&i| chars[i] == needle && chars[i + 1] == needle)
}

fn find_str(chars: &[char], from: usize, needle: &str) -> Option<usize> {
    let needle: Vec<char> = needle.chars().collect();
    (from..chars.len().saturating_sub(needle.len() - 1))
        .find(|&i| chars[i..i + needle.len()] == needle[..])
}

/// Flattened text, for the places that need characters without any of the
/// decoration — a title, for one.
pub fn plain_text(runs: &[Run]) -> String {
    let mut out = String::new();
    for run in runs {
        match run {
            Run::Text(t) | Run::Tcy(t) | Run::Code(t) => out.push_str(t),
            Run::Ruby { base, .. } => out.push_str(base),
            Run::Emphasis(inner) | Run::Strong(inner) | Run::Link { runs: inner, .. } => {
                out.push_str(&plain_text(inner))
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> Run {
        Run::Text(s.into())
    }

    #[test]
    fn a_paragraph_keeps_japanese_lines_together_without_a_space() {
        let doc = Document::from_markdown("吾輩は猫である。\n名前はまだ無い。");
        assert_eq!(
            doc.blocks,
            vec![Block::Paragraph(vec![text(
                "吾輩は猫である。名前はまだ無い。"
            )])]
        );
    }

    #[test]
    fn latin_lines_are_joined_with_the_space_the_markdown_meant() {
        let doc = Document::from_markdown("the quick brown\nfox jumps");
        assert_eq!(
            doc.blocks,
            vec![Block::Paragraph(vec![text("the quick brown fox jumps")])]
        );
    }

    #[test]
    fn a_blank_line_separates_paragraphs() {
        let doc = Document::from_markdown("一つ目。\n\n二つ目。");
        assert_eq!(
            doc.blocks,
            vec![
                Block::Paragraph(vec![text("一つ目。")]),
                Block::Paragraph(vec![text("二つ目。")]),
            ]
        );
    }

    #[test]
    fn the_first_h1_becomes_the_title_and_leaves_the_body() {
        let doc = Document::from_markdown("# 草枕\n\n山路を登りながら。");
        assert_eq!(doc.title.as_deref(), Some("草枕"));
        assert_eq!(
            doc.blocks,
            vec![Block::Paragraph(vec![text("山路を登りながら。")])]
        );
    }

    #[test]
    fn frontmatter_supplies_the_title_and_is_not_typeset() {
        let doc = Document::from_markdown(
            "---\ntitle: 一房の葡萄\nauthor: 有島武郎\ntags: [a]\n---\n\n本文。",
        );
        assert_eq!(doc.title.as_deref(), Some("一房の葡萄"));
        assert_eq!(doc.byline.as_deref(), Some("有島武郎"));
        assert_eq!(doc.blocks, vec![Block::Paragraph(vec![text("本文。")])]);
    }

    #[test]
    fn a_rule_on_the_first_line_is_a_rule_when_no_frontmatter_closes() {
        let doc = Document::from_markdown("---\n\n本文。");
        assert_eq!(
            doc.blocks,
            vec![Block::Rule, Block::Paragraph(vec![text("本文。")])]
        );
    }

    #[test]
    fn ruby_reads_the_aozora_bar_form() {
        assert_eq!(
            parse_runs("｜黄昏《たそがれ》の"),
            vec![
                Run::Ruby {
                    base: "黄昏".into(),
                    reading: "たそがれ".into()
                },
                text("の")
            ]
        );
    }

    #[test]
    fn ruby_without_a_bar_takes_the_kanji_run_before_it() {
        assert_eq!(
            parse_runs("その硝子《ガラス》を"),
            vec![
                text("その"),
                Run::Ruby {
                    base: "硝子".into(),
                    reading: "ガラス".into()
                },
                text("を"),
            ]
        );
    }

    #[test]
    fn guillemets_after_kana_are_not_ruby() {
        assert_eq!(parse_runs("ゆき《ゆき》"), vec![text("ゆき《ゆき》")]);
    }

    #[test]
    fn emphasis_is_kenten_and_strong_is_weight() {
        assert_eq!(
            parse_runs("*ここ*"),
            vec![Run::Emphasis(vec![text("ここ")])]
        );
        assert_eq!(
            parse_runs("**ここ**"),
            vec![Run::Strong(vec![text("ここ")])]
        );
    }

    #[test]
    fn code_spans_are_not_read_for_markup() {
        assert_eq!(parse_runs("`a*b*c`"), vec![Run::Code("a*b*c".into())]);
    }

    #[test]
    fn one_and_two_digit_numbers_become_tcy() {
        assert_eq!(
            parse_runs("第3章から17行目"),
            vec![
                text("第"),
                Run::Tcy("3".into()),
                text("章から"),
                Run::Tcy("17".into()),
                text("行目"),
            ]
        );
    }

    #[test]
    fn three_or_more_digits_stay_sideways() {
        assert_eq!(parse_runs("2026年"), vec![text("2026年")]);
    }

    #[test]
    fn digits_glued_to_an_identifier_are_left_alone() {
        assert_eq!(parse_runs("spec v0.9 を"), vec![text("spec v0.9 を")]);
        assert_eq!(parse_runs("UTF-8 で"), vec![text("UTF-8 で")]);
    }

    #[test]
    fn wiki_links_carry_their_target() {
        assert_eq!(
            parse_runs("[[草枕|あの本]]"),
            vec![Run::Link {
                runs: vec![text("あの本")],
                target: "草枕".into()
            }]
        );
        assert_eq!(
            parse_runs("[[草枕]]"),
            vec![Run::Link {
                runs: vec![text("草枕")],
                target: "草枕".into()
            }]
        );
    }

    #[test]
    fn inline_links_carry_their_target() {
        assert_eq!(
            parse_runs("[題](https://example.com)"),
            vec![Run::Link {
                runs: vec![text("題")],
                target: "https://example.com".into()
            }]
        );
    }

    #[test]
    fn quotes_nest_their_own_blocks() {
        let doc = Document::from_markdown("> 引用の一行目。\n> 続き。\n\n地の文。");
        assert_eq!(
            doc.blocks,
            vec![
                Block::Quote(vec![Block::Paragraph(vec![text("引用の一行目。続き。")])]),
                Block::Paragraph(vec![text("地の文。")]),
            ]
        );
    }

    #[test]
    fn lists_keep_their_kind() {
        let doc = Document::from_markdown("- 一\n- 二\n\n1. 甲\n2. 乙");
        assert_eq!(
            doc.blocks,
            vec![
                Block::List {
                    ordered: false,
                    items: vec![vec![text("一")], vec![text("二")]]
                },
                Block::List {
                    ordered: true,
                    items: vec![vec![text("甲")], vec![text("乙")]]
                },
            ]
        );
    }

    #[test]
    fn a_fence_keeps_its_body_verbatim_including_markup() {
        let doc = Document::from_markdown("```rust\nlet a = *b;\n# not a heading\n```");
        assert_eq!(
            doc.blocks,
            vec![Block::Code {
                lang: Some("rust".into()),
                text: "let a = *b;\n# not a heading".into()
            }]
        );
    }

    #[test]
    fn an_unclosed_fence_still_ends_the_document() {
        let doc = Document::from_markdown("```\nabc");
        assert_eq!(
            doc.blocks,
            vec![Block::Code {
                lang: None,
                text: "abc".into()
            }]
        );
    }

    #[test]
    fn headings_below_the_title_stay_in_the_body() {
        let doc = Document::from_markdown("# 題\n\n## 一章\n\n本文。");
        assert_eq!(doc.title.as_deref(), Some("題"));
        assert_eq!(
            doc.blocks,
            vec![
                Block::Heading {
                    level: 2,
                    runs: vec![text("一章")]
                },
                Block::Paragraph(vec![text("本文。")]),
            ]
        );
    }

    #[test]
    fn plain_text_flattens_every_run_kind() {
        let runs = parse_runs("｜漢字《かんじ》と**太**と*点*と`code`と[[note]]と17");
        assert_eq!(plain_text(&runs), "漢字と太と点とcodeとnoteと17");
    }
}
