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
    cache: HashMap<StyleKey, Option<Glyph>>,
    px: f32,
    shelf_x: usize,
    shelf_y: usize,
    shelf_h: usize,
    pub cell_w: f32,
    pub cell_h: f32,
    pub ascent: f32,
}

impl Atlas {
    pub fn new(px: f32) -> Self {
        // The first available font is the metric source; later fonts only fill
        // missing glyphs so the grid stays monospaced. The environment override
        // keeps this evaluation candidate portable without declaring any one
        // platform font stack canonical.
        let fonts = load_fonts(px);
        assert!(!fonts.is_empty(), "no usable font found");

        let primary = &fonts[0];
        let cell_w = primary.metrics('M', px).advance_width.max(1.0).round();
        let lm = primary.horizontal_line_metrics(px).expect("line metrics");
        let cell_h = (lm.ascent - lm.descent + lm.line_gap).ceil().max(1.0);

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
            cache: HashMap::new(),
            px,
            shelf_x: 4,
            shelf_y: 0,
            shelf_h: 0,
            cell_w,
            cell_h,
            ascent: lm.ascent,
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
        let Some(font) = self.fonts.iter().find(|f| f.lookup_glyph_index(ch) != 0) else {
            return Rasterized::NoInk;
        };
        let (m, base) = font.rasterize(ch, self.px);
        if m.width == 0 || m.height == 0 {
            return Rasterized::NoInk;
        }

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
            bearing_x: m.xmin as f32,
            bearing_y: (h as i32 + m.ymin) as f32,
        })
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
fn load_fonts(px: f32) -> Vec<Font> {
    candidate_font_paths()
        .into_iter()
        .filter_map(|(path, idx)| parse_font(std::fs::read(path).ok()?, idx, px))
        .collect()
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
        .first()
        .is_some_and(|primary| primary.horizontal_line_metrics(PROBE_PX).is_some())
}

fn candidate_font_paths() -> Vec<(PathBuf, u32)> {
    let mut paths = Vec::new();
    if let Some(configured) = std::env::var_os("NVIMGL_FONT_PATHS") {
        paths.extend(std::env::split_paths(&configured).map(|path| (path, 0)));
    }

    #[cfg(target_os = "macos")]
    paths.extend(
        [
            "/System/Library/Fonts/SFNSMono.ttf",
            "/System/Library/Fonts/Supplemental/Andale Mono.ttf",
            "/System/Library/Fonts/Hiragino Sans GB.ttc",
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
}
