//! The page's dimensions.
//!
//! Characters per line, line advance, lines per page, type size, paper. Every
//! number the typesetting depends on gathers here.
//!
//! **The canonical copy is not this file — it is `:root` in
//! `assets/tategaki.css`.** What lives here is a copy, typed so the values can
//! be overridden. When a copy drifts, the result is a state where nobody can
//! say which is true: the stylesheet was edited and nothing changed, or Rust was
//! edited and CSS won anyway. So a test watches the copy — the defaults must
//! equal what the stylesheet declares, and every property name written from here
//! must be one the stylesheet declares.

/// The typesetting itself, embedded verbatim into the page so the result
/// stands alone as one file.
pub const STYLESHEET: &str = include_str!("../../assets/tategaki.css");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scheme {
    /// Paper. The default.
    Paper,
    /// Night.
    Night,
}

impl Scheme {
    pub fn as_str(self) -> &'static str {
        match self {
            Scheme::Paper => "paper",
            Scheme::Night => "night",
        }
    }

    /// Takes the host's `--scheme` as it comes: `light` is paper, `dark` is night.
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "night" | "dark" => Scheme::Night,
            _ => Scheme::Paper,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Style {
    pub font_size_px: f32,
    /// Line advance, as a multiple of the type size.
    pub line_height: f32,
    /// Characters per line. The inline axis runs vertically here, so this is
    /// the height of the text block.
    pub measure: u32,
    /// The widest a page gets, in lines. Past this the reading area stops
    /// growing and sits centred; a turn always moves by the whole lines that
    /// fit inside it, so a page boundary lands on a line boundary.
    pub lines_per_page: u32,
    pub scheme: Scheme,
    /// Replaces the mincho stack. `None` keeps the stylesheet's own, which is
    /// what falls back through the faces a given machine actually has.
    pub serif: Option<String>,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            font_size_px: 17.0,
            line_height: 1.9,
            measure: 28,
            lines_per_page: 24,
            scheme: Scheme::Paper,
            serif: None,
        }
    }
}

impl Style {
    /// Pull the numbers into the range a page can actually be set in.
    ///
    /// Rounded rather than refused, because this is how a preview looks. Asked
    /// for 200px type, showing the largest type that can be set beats showing
    /// nothing at all.
    pub fn clamped(mut self) -> Self {
        self.font_size_px = self.font_size_px.clamp(8.0, 64.0);
        self.line_height = self.line_height.clamp(1.0, 3.0);
        self.measure = self.measure.clamp(8, 80);
        self.lines_per_page = self.lines_per_page.clamp(2, 60);
        self
    }

    /// The declarations that override the stylesheet's `:root`.
    pub fn custom_properties(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "  --tategaki-font-size: {}px;\n",
            trim_float(self.font_size_px)
        ));
        out.push_str(&format!(
            "  --tategaki-line-height: {};\n",
            trim_float(self.line_height)
        ));
        out.push_str(&format!("  --tategaki-measure: {};\n", self.measure));
        out.push_str(&format!(
            "  --tategaki-lines-per-page: {};\n",
            self.lines_per_page
        ));
        if let Some(serif) = &self.serif {
            out.push_str(&format!("  --tategaki-serif: {serif};\n"));
        }
        out
    }

    /// Every property this module can override. The test matches them against
    /// the stylesheet.
    pub fn property_names() -> &'static [&'static str] {
        &[
            "--tategaki-font-size",
            "--tategaki-line-height",
            "--tategaki-measure",
            "--tategaki-lines-per-page",
            "--tategaki-serif",
        ]
    }
}

fn trim_float(value: f32) -> String {
    let text = format!("{value:.3}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    text.to_string()
}

/// Read a value the stylesheet's first `:root { ... }` declares.
///
/// Not a CSS parser. It exists so a test can see that the copy has not drifted,
/// and it looks only at `--tategaki-*` declarations.
pub fn declared(property: &str) -> Option<String> {
    let root = STYLESHEET.split(":root {").nth(1)?;
    let root = root.split('}').next()?;
    let root = strip_comments(root);
    for declaration in root.split(';') {
        let Some((name, value)) = declaration.split_once(':') else {
            continue;
        };
        if name.trim() == property {
            // Values such as the font stack run over several lines, so
            // whitespace collapses to one.
            return Some(value.split_whitespace().collect::<Vec<_>>().join(" "));
        }
    }
    None
}

fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(open) = rest.find("/*") {
        out.push_str(&rest[..open]);
        match rest[open + 2..].find("*/") {
            Some(close) => rest = &rest[open + 2 + close + 2..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_the_stylesheets_own_defaults() {
        let style = Style::default();
        assert_eq!(declared("--tategaki-font-size").as_deref(), Some("17px"));
        assert_eq!(format!("{}px", trim_float(style.font_size_px)), "17px");
        assert_eq!(declared("--tategaki-line-height").as_deref(), Some("1.9"));
        assert_eq!(trim_float(style.line_height), "1.9");
        assert_eq!(declared("--tategaki-measure").as_deref(), Some("28"));
        assert_eq!(style.measure.to_string(), "28");
        assert_eq!(declared("--tategaki-lines-per-page").as_deref(), Some("24"));
        assert_eq!(style.lines_per_page.to_string(), "24");
    }

    #[test]
    fn every_property_this_module_writes_is_declared_by_the_stylesheet() {
        for name in Style::property_names() {
            assert!(
                declared(name).is_some(),
                "{name} exists only on the Rust side; the stylesheet is canonical, so the copy has drifted"
            );
        }
    }

    #[test]
    fn the_derived_page_width_is_defined_in_terms_of_the_overridable_values() {
        // As long as the page width is written as line advance times line
        // count, changing the type size is enough to keep a page turn landing on
        // a line boundary. The moment it becomes an independent number, page
        // boundaries start cutting lines in half.
        let page = declared("--tategaki-page").expect("--tategaki-page");
        assert!(page.contains("--tategaki-line-advance"), "{page}");
        assert!(page.contains("--tategaki-lines-per-page"), "{page}");
        let advance = declared("--tategaki-line-advance").expect("--tategaki-line-advance");
        assert!(advance.contains("--tategaki-font-size"), "{advance}");
        assert!(advance.contains("--tategaki-line-height"), "{advance}");
    }

    #[test]
    fn overrides_are_emitted_for_everything_that_differs() {
        let style = Style {
            font_size_px: 21.0,
            measure: 32,
            ..Style::default()
        };
        let css = style.custom_properties();
        assert!(css.contains("--tategaki-font-size: 21px;"), "{css}");
        assert!(css.contains("--tategaki-measure: 32;"), "{css}");
        assert!(css.contains("--tategaki-line-height: 1.9;"), "{css}");
        // An unrequested font stack is not written. Writing one would kill the
        // stylesheet's own fallback chain.
        assert!(!css.contains("--tategaki-serif"), "{css}");
    }

    #[test]
    fn a_serif_override_is_emitted_when_asked_for() {
        let style = Style {
            serif: Some("\"Yu Mincho\", serif".into()),
            ..Style::default()
        };
        assert!(style
            .custom_properties()
            .contains("--tategaki-serif: \"Yu Mincho\", serif;"));
    }

    #[test]
    fn out_of_range_numbers_are_rounded_into_the_page_rather_than_refused() {
        let style = Style {
            font_size_px: 400.0,
            line_height: 0.1,
            measure: 4000,
            lines_per_page: 0,
            ..Style::default()
        }
        .clamped();
        assert_eq!(style.font_size_px, 64.0);
        assert_eq!(style.line_height, 1.0);
        assert_eq!(style.measure, 80);
        assert_eq!(style.lines_per_page, 2);
    }

    #[test]
    fn the_hosts_scheme_flag_maps_onto_paper_and_night() {
        assert_eq!(Scheme::parse("dark"), Scheme::Night);
        assert_eq!(Scheme::parse("night"), Scheme::Night);
        assert_eq!(Scheme::parse("light"), Scheme::Paper);
        assert_eq!(Scheme::parse(""), Scheme::Paper);
    }

    #[test]
    fn the_night_scheme_is_a_separate_root_in_the_stylesheet() {
        assert!(STYLESHEET.contains(r#":root[data-scheme="night"]"#));
    }
}
