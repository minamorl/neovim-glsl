//! Glyph rasterisation into a single-channel atlas.
//!
//! Glyphs are rasterised on demand and packed with a shelf allocator. Texel
//! (0,0) is forced opaque so background quads can share the glyph shader by
//! sampling it.

use std::collections::HashMap;
use std::path::PathBuf;

use fontdue::{Font, FontSettings};
use serde::Serialize;

pub const ATLAS: usize = 1024;

/// Counters for what the glyph cache and the atlas allocator actually did.
///
/// These are plain increments on paths that already do far more expensive work
/// (a hash lookup at best, a rasterisation at worst), so they are always on:
/// they read no clock and allocate nothing. Only the timing instrumentation in
/// [`crate::perf`] is gated behind a switch.
///
/// Every miss lands in exactly one of `rasterizations`, `empty_glyphs` and
/// `rejections_atlas_full`, so `misses == rasterizations + empty_glyphs +
/// rejections_atlas_full` always holds. Those three are different facts — ink
/// stored, no ink to store, ink that had nowhere to go — and a run that
/// exhausted the atlas must not read as a run that drew nothing but spaces.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Serialize)]
pub struct AtlasStats {
    /// Glyph requests reaching the cache.
    pub lookups: u64,
    /// Requests served from the cache.
    pub hits: u64,
    /// Requests that had to go to the rasteriser.
    pub misses: u64,
    /// Misses that produced ink and consumed atlas space.
    pub rasterizations: u64,
    /// Misses that produced no ink to store: a space, a character no font maps,
    /// or a bitmap with zero coverage. Never a glyph that was refused space.
    pub empty_glyphs: u64,
    /// Glyphs that had ink but were refused because the shelf allocator ran out
    /// of atlas. These are dropped from the screen, not blank.
    pub rejections_atlas_full: u64,
    /// Times the atlas texture was re-uploaded to the GPU. Reported through
    /// [`crate::perf::AtlasSection::uploads`], which suppresses it entirely on
    /// paths where no GPU existed to upload to, so it is not serialised here.
    #[serde(skip)]
    pub uploads: u64,
}

/// What the rasteriser did with one cache miss. The caller turns this into
/// exactly one counter increment, so the three outcomes cannot be conflated.
enum Rasterized {
    /// Ink, packed into the atlas.
    Inked(Glyph),
    /// Nothing to draw.
    NoInk,
    /// Ink that would not fit; the atlas is out of room.
    AtlasFull,
}

#[derive(Clone, Copy)]
pub struct Glyph {
    pub u0: f32,
    pub v0: f32,
    pub u1: f32,
    pub v1: f32,
    pub w: f32,
    pub h: f32,
    /// Offset from the cell's left edge / baseline, in pixels.
    pub bearing_x: f32,
    pub bearing_y: f32,
}

/// Cache key for a rasterised glyph: the character plus the synthetic style
/// applied to it (fontdue has no bold/italic faces, so we synthesise them).
pub type StyleKey = (char, bool, bool);

pub struct Atlas {
    pub pixels: Vec<u8>,
    pub dirty: bool,
    /// Observed cache/allocator behaviour. Written only by this module and by
    /// the renderer's upload path; never by the reporter.
    pub stats: AtlasStats,
    fonts: Vec<Font>,
    /// One per font, parsed from the same bytes.
    cmaps: Vec<crate::cmap::Cmap>,
    cache: HashMap<StyleKey, Option<Glyph>>,
    px: f32,
    shelf_x: usize,
    shelf_y: usize,
    shelf_h: usize,
    pub cell_w: f32,
    pub cell_h: f32,
    pub ascent: f32,
    /// Height of a lowercase `x` above the baseline. The renderer centres
    /// strikethrough on it, so the rule crosses the body of the text rather than
    /// a guessed fraction of the ascent.
    pub x_height: f32,
}

impl Atlas {
    pub fn new(px: f32) -> Self {
        // The first available font is the metric source; later fonts only fill
        // missing glyphs so the grid stays monospaced. The environment override
        // keeps this evaluation candidate portable without declaring any one
        // platform font stack canonical.
        let (fonts, cmaps) = load_fonts(px);
        assert!(!fonts.is_empty(), "no usable font found");

        let primary = &fonts[0];
        let cell_w = primary.metrics('M', px).advance_width.max(1.0).round();
        let lm = primary.horizontal_line_metrics(px).expect("line metrics");
        let cell_h = (lm.ascent - lm.descent + lm.line_gap).ceil().max(1.0);
        let x_height = measure_x_height(primary, px, lm.ascent, 'x');

        let mut pixels = vec![0u8; ATLAS * ATLAS];
        // Opaque texel for background quads.
        for y in 0..2 {
            for x in 0..2 {
                pixels[y * ATLAS + x] = 255;
            }
        }

        Self {
            pixels,
            dirty: true,
            stats: AtlasStats::default(),
            fonts,
            cmaps,
            cache: HashMap::new(),
            px,
            shelf_x: 4,
            shelf_y: 0,
            shelf_h: 0,
            cell_w,
            cell_h,
            ascent: lm.ascent,
            x_height,
        }
    }

    /// UV of the forced-opaque texel, for untextured (background) quads.
    pub fn white_uv(&self) -> (f32, f32) {
        (0.5 / ATLAS as f32, 0.5 / ATLAS as f32)
    }

    /// Upright, unweighted glyph. Kept for callers that do not carry a style
    /// (e.g. the IME preedit pass).
    pub fn glyph(&mut self, ch: char) -> Option<Glyph> {
        self.styled_glyph(ch, false, false)
    }

    /// Glyph rasterised with synthetic `bold` (horizontal dilation) and/or
    /// `italic` (shear). Each (char, bold, italic) triple is atlas-cached
    /// independently, so the same character can appear plain and styled at once.
    pub fn styled_glyph(&mut self, ch: char, bold: bool, italic: bool) -> Option<Glyph> {
        let key: StyleKey = (ch, bold, italic);
        self.stats.lookups += 1;
        if let Some(g) = self.cache.get(&key) {
            self.stats.hits += 1;
            return *g;
        }
        self.stats.misses += 1;
        let g = match self.rasterize(ch, bold, italic) {
            Rasterized::Inked(glyph) => {
                self.stats.rasterizations += 1;
                Some(glyph)
            }
            Rasterized::NoInk => {
                self.stats.empty_glyphs += 1;
                None
            }
            Rasterized::AtlasFull => {
                self.stats.rejections_atlas_full += 1;
                None
            }
        };
        // A rejection is cached like any other outcome: v0.1 does not evict, so
        // a full atlas stays full and re-rasterising every frame would only
        // spend the cost again to reach the same answer.
        self.cache.insert(key, g);
        g
    }

    /// Number of distinct (char, bold, italic) keys the cache is holding.
    pub fn cached_entries(&self) -> usize {
        self.cache.len()
    }

    /// Atlas rows consumed by the shelf allocator so far, in texels. This is
    /// where the allocator currently stands, not a prediction of capacity.
    pub fn packed_height_px(&self) -> usize {
        (self.shelf_y + self.shelf_h).min(ATLAS)
    }

    /// Drive the shelf allocator to the bottom of the atlas, so the next inked
    /// glyph is refused. Reaching this state through real text takes thousands
    /// of distinct glyphs; the accounting on that path should not go untested
    /// for want of a way to get there.
    #[cfg(test)]
    fn exhaust_shelves(&mut self) {
        self.shelf_x = 0;
        self.shelf_y = ATLAS;
        self.shelf_h = 0;
    }

    fn rasterize(&mut self, ch: char, bold: bool, italic: bool) -> Rasterized {
        if ch == ' ' || ch == '\0' {
            return Rasterized::NoInk;
        }
        // The font's own Unicode subtable decides coverage, not the
        // rasteriser's char lookup — that one takes the legacy Macintosh table
        // on Menlo and Monaco and answers with a MacRoman glyph for anything in
        // Latin-1. See [`crate::cmap`].
        let Some((font, glyph_index)) = self.fonts.iter().zip(&self.cmaps).find_map(|(font, cmap)| {
            let index = if cmap.is_usable() { cmap.glyph(ch) } else { font.lookup_glyph_index(ch) };
            (index != 0).then_some((font, index))
        }) else {
            return Rasterized::NoInk;
        };
        // Fit the glyph's own em box to the cells it occupies, then centre what
        // is left over.
        //
        // A fallback face has no reason to advance exactly as wide as this
        // grid's cells. Measured here, a full-width Japanese glyph advances
        // 30px where two cells are 38: left-aligned, the whole 8px lands on the
        // right and the text reads as though every character were followed by a
        // gap. Japanese sets solid, so the fix is to make the em fill the cells
        // rather than to spread the gap around it — but not past the line box,
        // which is what the cap is for. Whatever slack survives is then split,
        // and it is the *advance* that gets centred, not the ink: a character
        // deliberately off-centre inside its own em — 。 sits low and left in
        // its square — has to stay where the font put it.
        let cells = cell_span(ch) as f32;
        let em = font.metrics_indexed(glyph_index, self.px).advance_width;
        let fit = if em > 0.0 {
            (cells * self.cell_w / em).min(self.cell_h / self.px).max(0.1)
        } else {
            1.0
        };
        let px = if (fit - 1.0).abs() < 0.01 { self.px } else { self.px * fit };

        let (m, base) = font.rasterize_indexed(glyph_index, px);
        if m.width == 0 || m.height == 0 {
            return Rasterized::NoInk;
        }
        let slack = (cells * self.cell_w - m.advance_width).max(0.0);
        let centring = (slack / 2.0).round();

        // Synthesise the requested face from the upright coverage bitmap. The
        // cell advance is fixed by the grid, so styling only reshapes ink; it
        // never changes how many columns the character occupies.
        let (bitmap, w, h) = synthesize(&base, m.width, m.height, bold, italic);
        if w == 0 || h == 0 {
            return Rasterized::NoInk;
        }
        // Ink that could never fit any atlas is still ink with nowhere to go.
        if w >= ATLAS || h >= ATLAS {
            return Rasterized::AtlasFull;
        }

        if self.shelf_x + w >= ATLAS {
            self.shelf_x = 0;
            self.shelf_y += self.shelf_h + 1;
            self.shelf_h = 0;
        }
        if self.shelf_y + h >= ATLAS {
            return Rasterized::AtlasFull; // v0.1 does not evict.
        }

        let (ox, oy) = (self.shelf_x, self.shelf_y);
        for row in 0..h {
            let dst = (oy + row) * ATLAS + ox;
            self.pixels[dst..dst + w].copy_from_slice(&bitmap[row * w..(row + 1) * w]);
        }
        self.shelf_x += w + 1;
        self.shelf_h = self.shelf_h.max(h);
        self.dirty = true;

        let s = ATLAS as f32;
        Rasterized::Inked(Glyph {
            u0: ox as f32 / s,
            v0: oy as f32 / s,
            u1: (ox + w) as f32 / s,
            v1: (oy + h) as f32 / s,
            w: w as f32,
            h: h as f32,
            bearing_x: m.xmin as f32 + centring,
            bearing_y: (h as i32 + m.ymin) as f32,
        })
    }
}

/// How many grid cells a character occupies.
///
/// East Asian Wide and Fullwidth take two; everything else here takes one. This
/// is the renderer's half of the same question the host answers when it lays
/// out the grid, and the two have to agree or the ink lands in the wrong cell.
fn cell_span(ch: char) -> usize {
    let c = ch as u32;
    let wide = matches!(c,
        0x1100..=0x115F
            | 0x2E80..=0x303E
            | 0x3041..=0x33FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xA000..=0xA4CF
            | 0xAC00..=0xD7A3
            | 0xF900..=0xFAFF
            | 0xFE30..=0xFE4F
            | 0xFF00..=0xFF60
            | 0xFFE0..=0xFFE6
            | 0x1F300..=0x1F64F
            | 0x1F900..=0x1F9FF
            | 0x20000..=0x2FFFD
            | 0x30000..=0x3FFFD);
    if wide {
        2
    } else {
        1
    }
}

/// Distance from the baseline to the top of `probe` (normally `x`), in pixels.
///
/// fontdue exposes no OS/2 x-height, so measure it: the rasterised height of a
/// lowercase `x` is exactly that distance. `Font::metrics` resolves a character
/// the font does not cover to glyph 0 (`.notdef`), which many faces draw as a
/// full-height box — trusting that would report roughly cap height and push
/// strikethrough well above the body of the text. So ask the cmap first and fall
/// back to a typical ratio of the ascent when the probe is genuinely absent.
fn measure_x_height(font: &Font, px: f32, ascent: f32, probe: char) -> f32 {
    let fallback = ascent * 0.52;
    if font.lookup_glyph_index(probe) == 0 {
        return fallback;
    }
    match font.metrics(probe, px).height as f32 {
        h if h > 0.0 => h,
        _ => fallback,
    }
}

/// Faux emboldening slant, in ink-columns per row of height. Chosen to read as
/// oblique without pushing ink outside a monospace cell for typical faces.
const ITALIC_SLANT: f32 = 0.22;

/// Synthesise a bold and/or italic coverage bitmap from an upright one.
///
/// * bold  — dilate horizontally: `out[x] = max(in[x], in[x-1])`, widening ink
///           by one column so strokes read heavier.
/// * italic— shear rows above the baseline to the right by `slant * dist`.
///
/// Returns `(pixels, width, height)`. With `bold == italic == false` it returns
/// the input unchanged. Pure and font-free so it is unit-testable off-GPU.
pub fn synthesize(base: &[u8], w: usize, h: usize, bold: bool, italic: bool) -> (Vec<u8>, usize, usize) {
    if !bold && !italic {
        return (base.to_vec(), w, h);
    }

    // Bold first: dilate within a one-column-wider canvas.
    let (mut src, mut sw) = if bold {
        let nw = w + 1;
        let mut out = vec![0u8; nw * h];
        for r in 0..h {
            for x in 0..nw {
                let a = if x < w { base[r * w + x] } else { 0 };
                let b = if x >= 1 && x - 1 < w { base[r * w + (x - 1)] } else { 0 };
                out[r * nw + x] = a.max(b);
            }
        }
        (out, nw)
    } else {
        (base.to_vec(), w)
    };

    if italic {
        // Max rightward shift is at the top row (furthest above the baseline).
        let max_shift = (ITALIC_SLANT * h as f32).round() as usize;
        let nw = sw + max_shift;
        let mut out = vec![0u8; nw * h];
        for r in 0..h {
            let dist_above = (h - 1 - r) as f32; // 0 at bottom, h-1 at top
            let shift = (ITALIC_SLANT * dist_above).round() as usize;
            for x in 0..sw {
                out[r * nw + (x + shift)] = src[r * sw + x];
            }
        }
        src = out;
        sw = nw;
    }

    (src, sw, h)
}

/// Size the availability probe parses at. Parse success is a property of the
/// file format, not of the size, so any size answers the question.
const PROBE_PX: f32 = 15.0;

fn parse_font(bytes: Vec<u8>, idx: u32, px: f32) -> Option<Font> {
    let settings = FontSettings {
        collection_index: idx,
        scale: px,
        ..Default::default()
    };
    Font::from_bytes(bytes, settings).ok()
}

/// Every candidate font that reads *and parses*, in preference order.
fn load_fonts(px: f32) -> (Vec<Font>, Vec<crate::cmap::Cmap>) {
    let mut fonts = Vec::new();
    let mut cmaps = Vec::new();
    for (path, idx) in candidate_font_paths() {
        let Some(bytes) = std::fs::read(path).ok() else { continue };
        let cmap = crate::cmap::Cmap::parse(&bytes, idx);
        let Some(font) = parse_font(bytes, idx, px) else { continue };
        fonts.push(font);
        cmaps.push(cmap);
    }
    (fonts, cmaps)
}

/// Whether [`Atlas::new`] would succeed on this host, so callers that must
/// degrade rather than abort (a benchmark on a font-less CI box) can ask first.
///
/// This runs the same load as `Atlas::new` and applies the same two conditions
/// — at least one font parses, and the primary one carries line metrics. A
/// readable-but-unparsable font file passed the old `is_file()` check and then
/// panicked in `Atlas::new`, which made the guard worse than no guard.
pub fn font_available() -> bool {
    load_fonts(PROBE_PX)
        .0
        .first()
        .is_some_and(|primary| primary.horizontal_line_metrics(PROBE_PX).is_some())
}

fn candidate_font_paths() -> Vec<(PathBuf, u32)> {
    let mut paths = Vec::new();
    if let Some(configured) = std::env::var_os("NVIMGL_FONT_PATHS") {
        paths.extend(std::env::split_paths(&configured).map(|path| (path, 0)));
    }

    // Order is meaning: the first face that parses sets the cell metrics, and
    // every later one only fills glyphs the earlier ones lack. So the Latin
    // stack comes first and Japanese after it, never the other way round.
    //
    // The Japanese face is Hiragino Kaku Gothic ProN, not Hiragino Sans GB. GB
    // is the Simplified Chinese face: it covers the same kanji and draws them
    // in Chinese forms, so 直 and 骨 and 令 come out subtly wrong in Japanese
    // text while looking, at a glance, like they rendered fine.
    #[cfg(target_os = "macos")]
    paths.extend(
        [
            "/System/Library/Fonts/Menlo.ttc",
            "/System/Library/Fonts/Monaco.ttf",
            "/System/Library/Fonts/Supplemental/Courier New.ttf",
            "/System/Library/Fonts/SFNSMono.ttf",
            "/System/Library/Fonts/Supplemental/Andale Mono.ttf",
            "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
            "/System/Library/Fonts/ヒラギノ明朝 ProN.ttc",
            "/System/Library/Fonts/Hiragino Sans GB.ttc",
            "/System/Library/Fonts/Apple Color Emoji.ttc",
        ]
        .into_iter()
        .map(|path| (PathBuf::from(path), 0)),
    );

    #[cfg(target_os = "linux")]
    paths.extend(
        [
            "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
            "/usr/share/fonts/truetype/noto/NotoSansMono-Regular.ttf",
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/run/current-system/sw/share/X11/fonts/TTF/DejaVuSansMono.ttf",
        ]
        .into_iter()
        .map(|path| (PathBuf::from(path), 0)),
    );

    #[cfg(target_os = "windows")]
    if let Some(windows) = std::env::var_os("WINDIR") {
        let fonts = PathBuf::from(windows).join("Fonts");
        paths.push((fonts.join("consola.ttf"), 0));
        paths.push((fonts.join("msgothic.ttc"), 0));
    }

    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    fn total_ink(px: &[u8]) -> u32 {
        px.iter().map(|&v| v as u32).sum()
    }

    #[test]
    fn x_height_falls_back_when_the_probe_is_not_in_the_font() {
        let atlas = Atlas::new(20.0);
        let font = &atlas.fonts[0];
        // A private-use codepoint the font does not cover. `Font::metrics`
        // resolves it to `.notdef`, which many faces draw as a full-height box —
        // measuring that would put strikethrough near cap height.
        let missing = ('\u{E000}'..='\u{E0FF}')
            .find(|&c| font.lookup_glyph_index(c) == 0)
            .expect("some private-use codepoint outside the font");
        assert_eq!(measure_x_height(font, 20.0, 100.0, missing), 100.0f32 * 0.52);

        // Whichever branch the host font takes, the atlas uses this number, and
        // it stays inside the ascent so strikethrough lands on the body of the
        // text rather than above it.
        let measured = measure_x_height(font, 20.0, atlas.ascent, 'x');
        assert_eq!(measured, atlas.x_height, "the atlas must use the measured probe");
        assert!(
            measured > 0.0 && measured < atlas.ascent,
            "implausible x-height {measured} against ascent {}",
            atlas.ascent
        );
    }

    #[test]
    fn plain_synthesis_is_identity() {
        let base = vec![0, 255, 0, 128];
        let (out, w, h) = synthesize(&base, 2, 2, false, false);
        assert_eq!((w, h), (2, 2));
        assert_eq!(out, base);
    }

    #[test]
    fn bold_widens_by_one_column_and_adds_ink() {
        // A single lit column should smear right, so coverage grows.
        let base = vec![255, 0, 0, 255, 0, 0]; // 3 wide, 2 tall, col 0 lit
        let (out, w, h) = synthesize(&base, 3, 2, true, false);
        assert_eq!((w, h), (4, 2));
        assert!(total_ink(&out) > total_ink(&base));
        // Column 1 becomes lit from the dilation of column 0.
        assert_eq!(out[1], 255);
    }

    #[test]
    fn italic_shears_top_rows_further_than_the_baseline_row() {
        // 1px lit at x=0 in every row; after shear the top row shifts right more
        // than the bottom (baseline) row.
        let h = 4;
        let w = 1;
        let base = vec![255u8; w * h];
        let (out, nw, oh) = synthesize(&base, w, h, false, true);
        assert_eq!(oh, h);
        assert!(nw > w);
        // Bottom row (r = h-1): shift 0 -> lit at x=0.
        assert_eq!(out[(h - 1) * nw + 0], 255);
        // Top row (r = 0): shifted right, so x=0 is now empty but some x>0 is lit.
        assert_eq!(out[0], 0);
        assert!((0..nw).any(|x| out[x] == 255));
    }

    /// Every miss lands in exactly one outcome bucket.
    fn misses_are_fully_attributed(s: &AtlasStats) {
        assert_eq!(s.hits + s.misses, s.lookups);
        assert_eq!(
            s.rasterizations + s.empty_glyphs + s.rejections_atlas_full,
            s.misses,
            "a miss was counted twice or not at all: {s:?}"
        );
    }

    #[test]
    fn a_glyph_refused_for_want_of_atlas_space_is_not_counted_as_inkless() {
        if !font_available() {
            eprintln!("skipped: no usable font on this host");
            return;
        }
        let mut atlas = Atlas::new(15.0);
        // A glyph that does have ink, so the only reason to refuse it is space.
        assert!(atlas.styled_glyph('W', false, false).is_some());
        assert_eq!(atlas.stats.rasterizations, 1);

        atlas.exhaust_shelves();
        assert!(atlas.styled_glyph('M', false, false).is_none());

        assert_eq!(
            atlas.stats.rejections_atlas_full, 1,
            "the refusal must be recorded as a refusal"
        );
        assert_eq!(
            atlas.stats.empty_glyphs, 0,
            "`M` has ink; calling it inkless hides that the atlas ran out"
        );
        assert_eq!(atlas.stats.rasterizations, 1, "nothing new was stored");
        misses_are_fully_attributed(&atlas.stats);
    }

    #[test]
    fn a_space_is_inkless_and_not_a_rejection() {
        if !font_available() {
            eprintln!("skipped: no usable font on this host");
            return;
        }
        let mut atlas = Atlas::new(15.0);
        assert!(atlas.styled_glyph(' ', false, false).is_none());
        assert_eq!(atlas.stats.empty_glyphs, 1);
        assert_eq!(atlas.stats.rejections_atlas_full, 0);
        misses_are_fully_attributed(&atlas.stats);
    }

    #[test]
    fn a_refused_glyph_is_a_miss_once_and_a_hit_after() {
        if !font_available() {
            eprintln!("skipped: no usable font on this host");
            return;
        }
        let mut atlas = Atlas::new(15.0);
        atlas.exhaust_shelves();
        for _ in 0..5 {
            assert!(atlas.styled_glyph('W', false, false).is_none());
        }
        assert_eq!(atlas.stats.lookups, 5);
        assert_eq!(
            atlas.stats.misses, 1,
            "the outcome is cached, not re-derived"
        );
        assert_eq!(atlas.stats.hits, 4);
        assert_eq!(atlas.stats.rejections_atlas_full, 1);
        misses_are_fully_attributed(&atlas.stats);
    }

    #[test]
    fn availability_agrees_with_what_building_an_atlas_needs() {
        // The guard exists so callers can degrade instead of hitting the assert
        // in `Atlas::new`. If it answers yes, `Atlas::new` must not panic.
        if font_available() {
            let atlas = Atlas::new(15.0);
            assert!(atlas.cell_w > 0.0 && atlas.cell_h > 0.0);
        }
    }

    #[test]
    fn a_readable_but_unparsable_file_yields_no_font() {
        // The old guard asked only whether the path was a readable file, so a
        // file like this passed it and `Atlas::new` then panicked. Availability
        // now runs this same parse, which is the thing that can actually fail.
        let bogus = b"this file is readable and is not a font".to_vec();
        assert!(parse_font(bogus, 0, PROBE_PX).is_none());
        assert!(parse_font(Vec::new(), 0, PROBE_PX).is_none());
    }

    #[test]
    fn bold_italic_compose_without_panicking_and_preserve_height() {
        let base = vec![200u8; 5 * 6];
        let (out, w, h) = synthesize(&base, 5, 6, true, true);
        assert_eq!(h, 6);
        assert!(w >= 6); // widened by bold (+1) and shear
        assert_eq!(out.len(), w * h);
    }
    /// A full-width character has to fill the two cells it claims.
    ///
    /// The defect this pins: the fallback face advances about one em where two
    /// cells are wider than that, so a left-aligned glyph put the whole
    /// difference on its right and Japanese read as though every character were
    /// followed by a space.
    #[test]
    fn a_full_width_glyph_fills_its_two_cells() {
        let mut atlas = Atlas::new(30.0);
        if !font_available() {
            return;
        }
        let cells = atlas.cell_w * 2.0;
        // Thin strokes are included, because the em is what gets fitted; ノ is
        // narrow ink inside a full-width square and must stay that way.
        for ch in ['漢', '速', 'あ', 'ノ'] {
            let Some(g) = atlas.glyph(ch) else { continue };
            let left = g.bearing_x;
            let right = cells - (g.bearing_x + g.w);
            assert!(right >= -1.5, "{ch} overflows its cells by {:.1}px", -right);
            assert!(
                (left - right).abs() <= 3.0,
                "{ch} sits off-centre: {left:.1}px left, {right:.1}px right",
            );
        }
        // Dense characters do have to cover their square, which is the part the
        // old left-alignment gave away.
        for ch in ['漢', '速'] {
            let Some(g) = atlas.glyph(ch) else { continue };
            assert!(
                g.w >= cells * 0.85,
                "{ch} covers only {:.0}% of its two cells",
                g.w / cells * 100.0
            );
        }
    }

    /// Scaling a wide glyph must not push it out of the line box.
    #[test]
    fn a_fitted_glyph_still_fits_the_line() {
        let mut atlas = Atlas::new(30.0);
        if !font_available() {
            return;
        }
        for ch in ['漢', '速', 'ー'] {
            let Some(g) = atlas.glyph(ch) else { continue };
            assert!(
                g.h <= atlas.cell_h + 1.0,
                "{ch} is {:.1}px tall in a {:.1}px line",
                g.h,
                atlas.cell_h
            );
        }
    }

    /// A halfwidth character is left where it was: the primary face already
    /// advances exactly one cell, so nothing should move.
    #[test]
    fn a_halfwidth_glyph_is_not_recentred(){
        let mut atlas = Atlas::new(30.0);
        if !font_available() {
            return;
        }
        let Some(g) = atlas.glyph('M') else { return };
        assert!(g.bearing_x.abs() <= 2.0, "M moved to {:.1}", g.bearing_x);
    }

}
