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
    /// Misses that produced no ink (space, unmapped, zero-coverage bitmap).
    pub empty_glyphs: u64,
    /// Glyphs refused because the shelf allocator ran out of atlas.
    pub rejections_atlas_full: u64,
    /// Times the atlas texture was re-uploaded to the GPU.
    pub uploads: u64,
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
        let mut fonts = Vec::new();
        for (path, idx) in candidate_font_paths() {
            if let Ok(bytes) = std::fs::read(path) {
                let settings = FontSettings {
                    collection_index: idx,
                    scale: px,
                    ..Default::default()
                };
                if let Ok(f) = Font::from_bytes(bytes, settings) {
                    fonts.push(f);
                }
            }
        }
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
        let g = self.rasterize(ch, bold, italic);
        if g.is_some() {
            self.stats.rasterizations += 1;
        } else {
            self.stats.empty_glyphs += 1;
        }
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

    fn rasterize(&mut self, ch: char, bold: bool, italic: bool) -> Option<Glyph> {
        if ch == ' ' || ch == '\0' {
            return None;
        }
        let font = self
            .fonts
            .iter()
            .find(|f| f.lookup_glyph_index(ch) != 0)?;
        let (m, base) = font.rasterize(ch, self.px);
        if m.width == 0 || m.height == 0 {
            return None;
        }

        // Synthesise the requested face from the upright coverage bitmap. The
        // cell advance is fixed by the grid, so styling only reshapes ink; it
        // never changes how many columns the character occupies.
        let (bitmap, w, h) = synthesize(&base, m.width, m.height, bold, italic);
        if w == 0 || h == 0 || w >= ATLAS || h >= ATLAS {
            return None;
        }

        if self.shelf_x + w >= ATLAS {
            self.shelf_x = 0;
            self.shelf_y += self.shelf_h + 1;
            self.shelf_h = 0;
        }
        if self.shelf_y + h >= ATLAS {
            self.stats.rejections_atlas_full += 1;
            return None; // Atlas full; v0.1 does not evict.
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
        Some(Glyph {
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

/// Whether any candidate font path is readable on this host. [`Atlas::new`]
/// asserts on the empty case, so callers that must degrade rather than abort
/// (a benchmark on a font-less CI box) can ask first.
pub fn font_available() -> bool {
    candidate_font_paths()
        .into_iter()
        .any(|(path, _)| path.is_file())
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

    #[test]
    fn bold_italic_compose_without_panicking_and_preserve_height() {
        let base = vec![200u8; 5 * 6];
        let (out, w, h) = synthesize(&base, 5, 6, true, true);
        assert_eq!(h, 6);
        assert!(w >= 6); // widened by bold (+1) and shear
        assert_eq!(out.len(), w * h);
    }
}
