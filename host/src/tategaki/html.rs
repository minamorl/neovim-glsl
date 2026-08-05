//! Document + page dimensions → one HTML file.
//!
//! The result stands alone. The stylesheet is embedded and nothing is fetched —
//! no file, no CDN, no webfont. A preview exists to look at the note being
//! edited, and there is no reason for how it looks to depend on whether the
//! machine can reach the network. Mincho is picked from the faces the machine
//! actually has (`--tategaki-serif`).
//!
//! Nothing is typeset here. This module lowers a tree with meaning on it into
//! elements with meaning on them; breaking lines, applying kinsoku and placing
//! ruby are all the engine's. Only the vertical-writing judgements the engine
//! does not make are left:
//!
//! - **Tate-chu-yoko** is wrapped in `span.tcy`. No engine implements
//!   `text-combine-upright: digits 2`, so [`super::doc`] decides which digits
//!   stand upright and this wraps them.
//! - **List markers** go inside the element rather than outside it. `content` on
//!   `::before` cannot be combined upright, so an arabic numeral there would be
//!   the one thing on the line lying on its side. Kanji numerals stand as they
//!   are.

use super::doc::{Block, Document, Run};
use super::style::{Style, STYLESHEET};

/// Page turning and type size. The engine sets vertical text but has no notion
/// of a page, so the only thing added is movement by exactly the page's width.
///
/// That width is line advance times line count, so the head of the page turned
/// to always coincides with the head of a line. Pages never cut a line in half
/// because the width is not held as a number of its own.
const PAGING: &str = r#"
(() => {
  const root = document.documentElement;
  const scroll = document.getElementById('tategaki-scroll');
  const folio = document.getElementById('tategaki-folio');
  const folios = document.getElementById('tategaki-folios');
  const prop = name => parseFloat(getComputedStyle(root).getPropertyValue(name));
  const advance = () => {
    // An engine implementing @property returns a computed length. One that
    // does not gets the same expression rebuilt from its parts, so the line
    // advance is never defined in two places.
    const computed = prop('--tategaki-line-advance');
    if (computed > 0) return computed;
    return prop('--tategaki-font-size') * prop('--tategaki-line-height');
  };
  // The page is what the reading area actually holds, cut to whole lines. Taken
  // from the declared width instead, a turn would move by less than a screen on
  // a wide window and the folio would count pages nobody turned.
  const page = () => {
    const a = advance();
    const n = scroll.clientWidth / a;
    // clientWidth is a whole number of pixels and the line advance is not, so a
    // reading area holding exactly 24 lines measures 23.99 of them. Whole lines
    // are counted with a pixel of tolerance; without it every page would drop
    // its last line to a rounding error.
    const lines = Math.abs(n - Math.round(n)) * a < 1 ? Math.round(n) : Math.floor(n);
    return Math.max(1, lines) * a;
  };
  const span = () => scroll.scrollWidth - scroll.clientWidth;
  const pages = () => Math.max(1, Math.ceil(scroll.scrollWidth / page() - 0.02));

  // Which scrollLeft means the right edge is not a thing to assume. The text is
  // vertical-rl but the scroller around it is not, and engines differ anyway:
  // some run -span..0 with the right edge at 0, others 0..span with it at span.
  // So the page finds out — it drives to the right edge, which a large positive
  // value reaches under either convention, and reads back what it landed on.
  let rightIsZero = true;
  const opening = () => (rightIsZero ? 0 : span());
  // Distance read so far, measured from the opening rather than from whichever
  // end the engine calls zero.
  const read = () => Math.abs(scroll.scrollLeft - opening());
  const at = () => Math.min(pages(), Math.round(read() / page()) + 1);
  const show = () => { folio.textContent = at(); folios.textContent = pages(); };
  const turn = n => scroll.scrollBy({ left: -n * page(), behavior: 'smooth' });

  // Vertical text opens on the right, so that is where the reader starts. This
  // waits for the fonts: before they land the text is narrower than it will be,
  // and a scroller with nothing to scroll answers the convention question wrong.
  const home = () => {
    const was = scroll.style.scrollBehavior;
    scroll.style.scrollBehavior = 'auto';
    scroll.scrollLeft = scroll.scrollWidth;
    if (span() > 0) rightIsZero = scroll.scrollLeft <= 0;
    scroll.scrollLeft = opening();
    scroll.style.scrollBehavior = was;
    show();
  };

  const resize = d => {
    const next = Math.min(64, Math.max(8, prop('--tategaki-font-size') + d));
    root.style.setProperty('--tategaki-font-size', next + 'px');
    show();
  };
  scroll.addEventListener('scroll', show, { passive: true });
  addEventListener('resize', show);
  addEventListener('keydown', e => {
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    switch (e.key) {
      case 'l': case 'j': case 'ArrowLeft': case 'ArrowDown': case 'PageDown': turn(1); break;
      case 'h': case 'k': case 'ArrowRight': case 'ArrowUp': case 'PageUp': turn(-1); break;
      case ' ': turn(e.shiftKey ? -1 : 1); break;
      case 'g': scroll.scrollTo({ left: opening(), behavior: 'smooth' }); break;
      case 'G': scroll.scrollTo({ left: rightIsZero ? -span() : 0, behavior: 'smooth' }); break;
      case '+': case '=': resize(1); break;
      case '-': resize(-1); break;
      case 't': root.dataset.scheme = root.dataset.scheme === 'night' ? 'paper' : 'night'; break;
      default: return;
    }
    e.preventDefault();
  });
  // Vertical text reads right to left, so the left half advances and the
  // right half goes back.
  addEventListener('click', e => turn(e.clientX < innerWidth / 2 ? 1 : -1));

  home();
  if (document.fonts && document.fonts.ready) document.fonts.ready.then(home);
  addEventListener('load', home);
})();
"#;

pub fn render(document: &Document, style: &Style) -> String {
    let style = style.clone().clamped();
    let title = document.title.clone().unwrap_or_else(|| "無題".to_string());

    let mut body = String::new();
    if document.title.is_some() {
        body.push_str(&format!(
            "<p class=\"tategaki-title\">{}</p>\n",
            escape(&title)
        ));
    }
    if let Some(byline) = &document.byline {
        body.push_str(&format!(
            "<p class=\"tategaki-byline\">{}</p>\n",
            escape(byline)
        ));
    }
    for block in &document.blocks {
        body.push_str(&render_block(block));
    }

    format!(
        r#"<!doctype html>
<html lang="ja" data-scheme="{scheme}">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<style>
{stylesheet}
/* the page dimensions the host overrode */
:root {{
{overrides}}}
</style>
</head>
<body>
<div class="tategaki-runner">{title}</div>
<div class="tategaki-scroll" id="tategaki-scroll">
<div class="tategaki">
{body}</div>
</div>
<div class="tategaki-folio"><span id="tategaki-folio">1</span> ／ <span id="tategaki-folios">1</span></div>
<div class="tategaki-keys">h l ␣ 頁　+ − 字　t 紙</div>
<script>{paging}</script>
</body>
</html>
"#,
        scheme = style.scheme.as_str(),
        title = escape(&title),
        stylesheet = STYLESHEET,
        overrides = style.custom_properties(),
        body = body,
        paging = PAGING,
    )
}

fn render_block(block: &Block) -> String {
    match block {
        Block::Heading { level, runs } => {
            let tag = format!("h{}", (*level).clamp(1, 4));
            format!("<{tag}>{}</{tag}>\n", render_runs(runs))
        }
        Block::Paragraph(runs) => format!("<p>{}</p>\n", render_runs(runs)),
        Block::Quote(blocks) => {
            let inner: String = blocks.iter().map(render_block).collect();
            format!("<blockquote>\n{inner}</blockquote>\n")
        }
        Block::List { ordered, items } => {
            let tag = if *ordered { "ol" } else { "ul" };
            let mut out = format!("<{tag}>\n");
            for (index, item) in items.iter().enumerate() {
                // An arabic numeral emitted through `::before` is the one
                // thing on the line lying on its side. Kanji numerals in a
                // vertical list are not decoration — they simply stand.
                let marker = if *ordered {
                    kansuji(index as u32 + 1)
                } else {
                    "・".to_string()
                };
                out.push_str(&format!(
                    "<li><span class=\"tategaki-marker\">{}</span>{}</li>\n",
                    escape(&marker),
                    render_runs(item)
                ));
            }
            out.push_str(&format!("</{tag}>\n"));
            out
        }
        Block::Code { lang, text } => {
            let class = match lang {
                Some(lang) => format!(" class=\"language-{}\"", escape(lang)),
                None => String::new(),
            };
            format!("<pre><code{class}>{}</code></pre>\n", escape(text))
        }
        Block::Rule => "<hr>\n".to_string(),
    }
}

fn render_runs(runs: &[Run]) -> String {
    runs.iter().map(render_run).collect()
}

fn render_run(run: &Run) -> String {
    match run {
        Run::Text(text) => escape(text),
        Run::Tcy(digits) => format!("<span class=\"tcy\">{}</span>", escape(digits)),
        Run::Ruby { base, reading } => {
            format!("<ruby>{}<rt>{}</rt></ruby>", escape(base), escape(reading))
        }
        Run::Emphasis(inner) => format!("<em>{}</em>", render_runs(inner)),
        Run::Strong(inner) => format!("<strong>{}</strong>", render_runs(inner)),
        Run::Code(text) => format!("<code>{}</code>", escape(text)),
        Run::Link { runs, target } => {
            let inner = render_runs(runs);
            // A `[[link]]` between notes is not a URL. Giving it an href makes
            // a link that leads nowhere when pressed. The destination is kept in
            // a data attribute instead, where a surface can still pick it up.
            if is_locator(target) {
                format!("<a href=\"{}\">{inner}</a>", escape(target))
            } else {
                format!(
                    "<a class=\"tategaki-note\" data-note=\"{}\">{inner}</a>",
                    escape(target)
                )
            }
        }
    }
}

fn is_locator(target: &str) -> bool {
    target.contains("://")
        || target.starts_with('#')
        || target.starts_with('/')
        || target.starts_with("mailto:")
}

/// One to ninety-nine, as numbers that stand upright on the page. Used for
/// list markers.
fn kansuji(n: u32) -> String {
    const DIGITS: [&str; 10] = ["", "一", "二", "三", "四", "五", "六", "七", "八", "九"];
    match n {
        0 => "〇".to_string(),
        1..=9 => DIGITS[n as usize].to_string(),
        10..=19 => format!("十{}", DIGITS[(n % 10) as usize]),
        20..=99 => format!(
            "{}十{}",
            DIGITS[(n / 10) as usize],
            DIGITS[(n % 10) as usize]
        ),
        _ => n.to_string(),
    }
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::style::Scheme;
    use super::*;

    fn html(markdown: &str) -> String {
        render(&Document::from_markdown(markdown), &Style::default())
    }

    #[test]
    fn the_page_carries_its_own_stylesheet_and_fetches_nothing() {
        let out = html("本文。");
        assert!(
            out.contains("writing-mode: vertical-rl"),
            "the stylesheet is not embedded"
        );
        assert!(!out.contains("<link "), "an external stylesheet is fetched");
        assert!(
            !out.contains("<script src"),
            "an external script is fetched"
        );
        assert!(!out.contains("@import"), "external CSS is fetched");
    }

    #[test]
    fn the_host_overrides_land_after_the_stylesheet_so_they_win() {
        let style = Style {
            font_size_px: 24.0,
            measure: 20,
            ..Style::default()
        };
        let out = render(&Document::from_markdown("本文。"), &style);
        let declared_at = out
            .find("--tategaki-font-size: 17px")
            .expect("the stylesheet default");
        let override_at = out
            .find("--tategaki-font-size: 24px")
            .expect("the host override");
        assert!(
            override_at > declared_at,
            "the override is emitted before the default it must beat"
        );
        assert!(out.contains("--tategaki-measure: 20;"));
    }

    #[test]
    fn ruby_becomes_a_ruby_element() {
        assert!(html("｜黄昏《たそがれ》").contains("<ruby>黄昏<rt>たそがれ</rt></ruby>"));
    }

    #[test]
    fn two_digit_numbers_are_wrapped_for_tate_chu_yoko() {
        let out = html("第17回");
        assert!(out.contains("第<span class=\"tcy\">17</span>回"), "{out}");
    }

    #[test]
    fn emphasis_becomes_em_so_the_stylesheet_can_put_kenten_on_it() {
        assert!(html("*ここ*").contains("<em>ここ</em>"));
        assert!(STYLESHEET.contains("text-emphasis: filled sesame"));
    }

    #[test]
    fn the_title_appears_as_the_runner_the_folio_head_and_the_document_title() {
        let out = html("# 草枕\n\n本文。");
        assert!(out.contains("<title>草枕</title>"));
        assert!(out.contains("<div class=\"tategaki-runner\">草枕</div>"));
        assert!(out.contains("<p class=\"tategaki-title\">草枕</p>"));
    }

    #[test]
    fn a_note_without_a_title_still_renders_and_says_so() {
        let out = html("本文だけ。");
        assert!(out.contains("<title>無題</title>"));
        assert!(
            !out.contains("class=\"tategaki-title\""),
            "a title page appears for a note that has no title"
        );
    }

    #[test]
    fn note_text_cannot_close_the_document_and_run_as_markup() {
        let out = html("<script>alert(\"x\")</script> と & 記号");
        assert!(!out.contains("<script>alert"), "note text became markup");
        assert!(
            out.contains("&lt;script&gt;alert(&quot;x&quot;)&lt;/script&gt;"),
            "{out}"
        );
        assert!(out.contains("&amp; 記号"));
    }

    #[test]
    fn a_bare_digit_in_prose_still_stands_upright() {
        // A number like the (1) in "(1) first" is a number in the prose, so it
        // stands. Escaping and tate-chu-yoko are applied separately, so a digit
        // that stands upright does not slip past the escaping.
        let out = html("(1) まず<b>");
        assert!(
            out.contains("(<span class=\"tcy\">1</span>) まず&lt;b&gt;"),
            "{out}"
        );
    }

    #[test]
    fn an_ordered_list_numbers_in_kanji_so_the_number_stands_upright() {
        let out = html("1. 甲\n2. 乙");
        assert!(
            out.contains("<span class=\"tategaki-marker\">一</span>甲"),
            "{out}"
        );
        assert!(
            out.contains("<span class=\"tategaki-marker\">二</span>乙"),
            "{out}"
        );
    }

    #[test]
    fn kansuji_covers_the_range_a_list_reaches() {
        assert_eq!(kansuji(1), "一");
        assert_eq!(kansuji(10), "十");
        assert_eq!(kansuji(11), "十一");
        assert_eq!(kansuji(20), "二十");
        assert_eq!(kansuji(37), "三十七");
        assert_eq!(kansuji(99), "九十九");
        assert_eq!(kansuji(100), "100");
    }

    #[test]
    fn a_wiki_link_keeps_its_target_without_pretending_to_be_a_url() {
        let out = html("[[草枕]]を読む");
        assert!(out.contains("data-note=\"草枕\""), "{out}");
        assert!(
            !out.contains("href=\"草枕\""),
            "a note name was made into an href"
        );
    }

    #[test]
    fn a_real_url_keeps_its_href() {
        assert!(html("[題](https://example.com)").contains("href=\"https://example.com\""));
    }

    #[test]
    fn code_blocks_survive_as_preformatted_text() {
        let out = html("```rust\nlet a = 1;\n```");
        assert!(
            out.contains("<pre><code class=\"language-rust\">let a = 1;</code></pre>"),
            "{out}"
        );
        assert!(
            STYLESHEET.contains("writing-mode: horizontal-tb"),
            "a fenced block would stay vertical"
        );
    }

    #[test]
    fn the_night_scheme_reaches_the_root_element() {
        let style = Style {
            scheme: Scheme::Night,
            ..Style::default()
        };
        let out = render(&Document::from_markdown("本文。"), &style);
        assert!(
            out.contains(r#"<html lang="ja" data-scheme="night">"#),
            "{}",
            &out[..200]
        );
    }

    #[test]
    fn the_document_declares_japanese_so_the_engine_applies_japanese_line_breaking() {
        assert!(html("本文。").contains("lang=\"ja\""));
    }

    #[test]
    fn quotes_and_rules_reach_their_elements() {
        let out = html("> 引用。\n\n---\n\n本文。");
        assert!(out.contains("<blockquote>"));
        assert!(out.contains("<hr>"));
    }

    #[test]
    fn paging_moves_by_exactly_one_page_width() {
        // The invariant that the page width is never an independent number has
        // to hold on the script side too.
        assert!(
            PAGING.contains("--tategaki-line-advance"),
            "the line advance is not taken from the stylesheet"
        );
        assert!(
            PAGING.contains("scroll.clientWidth / a"),
            "a page is not cut to whole lines"
        );
        assert!(PAGING.contains("scrollBy"), "there is no page turn");
        // The reader must open at the opening. Engines disagree about which end
        // of a vertical-rl scroller is zero, so the page settles it by landing
        // at the right edge and reading back what it got.
        assert!(
            PAGING.contains("scroll.scrollLeft = scroll.scrollWidth"),
            "the page does not open at its opening"
        );
        assert!(
            PAGING.contains("rightIsZero"),
            "the scrollLeft convention is assumed rather than measured"
        );
        // Fonts change how wide the text is, and a scroller with nothing to
        // scroll cannot answer which end is the opening.
        assert!(
            PAGING.contains("document.fonts.ready"),
            "the opening is settled before the fonts land"
        );
    }
}
