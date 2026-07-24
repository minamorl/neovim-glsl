//! Glyph rasterisation into a single-channel atlas.
//!
//! Glyphs are rasterised on demand and packed with a shelf allocator. Texel
//! (0,0) is forced opaque so background quads can share the glyph shader by
//! sampling it.

use std::collections::HashMap;

use fontdue::{Font, FontSettings};

pub const ATLAS: usize = 1024;

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

pub struct Atlas {
    pub pixels: Vec<u8>,
    pub dirty: bool,
    fonts: Vec<Font>,
    cache: HashMap<char, Option<Glyph>>,
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
        // SF Mono is the metric source. Hiragino only fills in what SF Mono
        // lacks, so cell metrics stay monospaced.
        let mut fonts = Vec::new();
        for (path, idx) in [
            ("/System/Library/Fonts/SFNSMono.ttf", 0usize),
            ("/System/Library/Fonts/Supplemental/Andale Mono.ttf", 0),
            ("/System/Library/Fonts/Hiragino Sans GB.ttc", 0),
        ] {
            if let Ok(bytes) = std::fs::read(path) {
                let settings = FontSettings { collection_index: idx as u32, scale: px, ..Default::default() };
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

    pub fn glyph(&mut self, ch: char) -> Option<Glyph> {
        if let Some(g) = self.cache.get(&ch) {
            return *g;
        }
        let g = self.rasterize(ch);
        self.cache.insert(ch, g);
        g
    }

    fn rasterize(&mut self, ch: char) -> Option<Glyph> {
        if ch == ' ' || ch == '\0' {
            return None;
        }
        let font = self
            .fonts
            .iter()
            .find(|f| f.lookup_glyph_index(ch) != 0)?;
        let (m, bitmap) = font.rasterize(ch, self.px);
        if m.width == 0 || m.height == 0 {
            return None;
        }
        if m.width >= ATLAS || m.height >= ATLAS {
            return None;
        }

        if self.shelf_x + m.width >= ATLAS {
            self.shelf_x = 0;
            self.shelf_y += self.shelf_h + 1;
            self.shelf_h = 0;
        }
        if self.shelf_y + m.height >= ATLAS {
            return None; // Atlas full; v0.1 does not evict.
        }

        let (ox, oy) = (self.shelf_x, self.shelf_y);
        for row in 0..m.height {
            let dst = (oy + row) * ATLAS + ox;
            self.pixels[dst..dst + m.width]
                .copy_from_slice(&bitmap[row * m.width..(row + 1) * m.width]);
        }
        self.shelf_x += m.width + 1;
        self.shelf_h = self.shelf_h.max(m.height);
        self.dirty = true;

        let s = ATLAS as f32;
        Some(Glyph {
            u0: ox as f32 / s,
            v0: oy as f32 / s,
            u1: (ox + m.width) as f32 / s,
            v1: (oy + m.height) as f32 / s,
            w: m.width as f32,
            h: m.height as f32,
            bearing_x: m.xmin as f32,
            bearing_y: (m.height as i32 + m.ymin) as f32,
        })
    }
}
