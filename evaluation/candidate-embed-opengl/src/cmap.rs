//! Character-to-glyph lookup, read from the font's Unicode `cmap` subtable.
//!
//! This exists because the lookup that came with the rasteriser is wrong on
//! macOS's own monospace fonts. Menlo and Monaco carry both a Unicode subtable
//! and a legacy Macintosh one, and the legacy table is indexed by *MacRoman
//! byte*: 0xB7 is `∑` there, not `·`. Taking that table makes every character
//! in Latin-1 render as something else — `é` as `È`, `°` as `∞`, `×` as `◊` —
//! while ASCII stays perfect, so the damage looks like a font problem rather
//! than a mapping one and can sit unnoticed for a long time.
//!
//! So the subtable is chosen here, explicitly and in the order the OpenType
//! specification recommends, and the rasteriser is then asked for a glyph
//! *index* rather than for a character.

/// A parsed Unicode `cmap` subtable: the ranges it covers, and how to read
/// them.
#[derive(Debug)]
pub struct Cmap {
    format: Format,
}

#[derive(Debug)]
enum Format {
    /// Segment mapping to delta values — the BMP workhorse.
    Four { segments: Vec<Segment4> },
    /// Segmented coverage, which is what carries anything above the BMP.
    Twelve { groups: Vec<Group12> },
    /// The font had no Unicode subtable this module understands.
    None,
}

#[derive(Debug)]
struct Segment4 {
    start: u32,
    end: u32,
    delta: u16,
    /// Byte offset into the subtable of this segment's glyph array, when it
    /// uses one.
    range_offset: Option<usize>,
}

#[derive(Debug)]
struct Group12 {
    start: u32,
    end: u32,
    start_glyph: u32,
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*bytes.get(at)?, *bytes.get(at + 1)?]))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes([
        *bytes.get(at)?,
        *bytes.get(at + 1)?,
        *bytes.get(at + 2)?,
        *bytes.get(at + 3)?,
    ]))
}

/// The table directory for one face, which is not at offset 0 in a collection.
fn table_directory(bytes: &[u8], face: u32) -> Option<usize> {
    if bytes.get(..4)? == b"ttcf" {
        let count = u32_at(bytes, 8)?;
        let face = face.min(count.saturating_sub(1));
        return u32_at(bytes, 12 + face as usize * 4).map(|at| at as usize);
    }
    Some(0)
}

fn find_table(bytes: &[u8], face: u32, tag: &[u8; 4]) -> Option<(usize, usize)> {
    let directory = table_directory(bytes, face)?;
    let count = u16_at(bytes, directory + 4)? as usize;
    for index in 0..count {
        let record = directory + 12 + index * 16;
        if bytes.get(record..record + 4)? == tag {
            let offset = u32_at(bytes, record + 8)? as usize;
            let length = u32_at(bytes, record + 12)? as usize;
            return Some((offset, length));
        }
    }
    None
}

impl Cmap {
    /// Parse the best Unicode subtable of `face` in `bytes`.
    pub fn parse(bytes: &[u8], face: u32) -> Self {
        Self { format: parse_best(bytes, face).unwrap_or(Format::None) }
    }

    /// Whether a usable Unicode subtable was found. When it was not, callers
    /// should fall back rather than treat every character as missing.
    pub fn is_usable(&self) -> bool {
        !matches!(self.format, Format::None)
    }

    /// The glyph index for `ch`, or 0 for "this font does not have it" — the
    /// same convention the format itself uses.
    pub fn glyph(&self, ch: char) -> u16 {
        let code = ch as u32;
        match &self.format {
            Format::None => 0,
            Format::Twelve { groups } => groups
                .iter()
                .find(|group| (group.start..=group.end).contains(&code))
                .map(|group| (group.start_glyph + (code - group.start)) as u16)
                .unwrap_or(0),
            Format::Four { segments } => segments
                .iter()
                .find(|segment| (segment.start..=segment.end).contains(&code))
                .map(|segment| match segment.range_offset {
                    None => (code as u16).wrapping_add(segment.delta),
                    Some(glyph) => {
                        if glyph == 0 {
                            0
                        } else {
                            (glyph as u16).wrapping_add(segment.delta)
                        }
                    }
                })
                .unwrap_or(0),
        }
    }
}

/// Pick a subtable, in the order the specification recommends: full Unicode
/// before the BMP, and a platform-independent table before a Windows one.
/// A Macintosh (platform 1) table is never chosen — that is the whole point.
fn parse_best(bytes: &[u8], face: u32) -> Option<Format> {
    let (cmap, _) = find_table(bytes, face, b"cmap")?;
    let count = u16_at(bytes, cmap + 2)? as usize;

    let mut best: Option<(u8, usize)> = None;
    for index in 0..count {
        let record = cmap + 4 + index * 8;
        let platform = u16_at(bytes, record)?;
        let encoding = u16_at(bytes, record + 2)?;
        let offset = cmap + u32_at(bytes, record + 4)? as usize;
        let rank = match (platform, encoding) {
            (3, 10) => 4, // Windows, full Unicode
            (0, 4) | (0, 6) => 3, // Unicode, full
            (3, 1) => 2,  // Windows, BMP
            (0, _) => 1,  // Unicode, BMP
            _ => continue, // platform 1 is Macintosh: never
        };
        if best.is_none_or(|(current, _)| rank > current) {
            best = Some((rank, offset));
        }
    }

    let (_, offset) = best?;
    match u16_at(bytes, offset)? {
        4 => parse_format_4(bytes, offset),
        12 => parse_format_12(bytes, offset),
        _ => None,
    }
}

fn parse_format_4(bytes: &[u8], at: usize) -> Option<Format> {
    let seg_x2 = u16_at(bytes, at + 6)? as usize;
    let segments = seg_x2 / 2;
    let ends = at + 14;
    let starts = ends + seg_x2 + 2;
    let deltas = starts + seg_x2;
    let offsets = deltas + seg_x2;

    let mut out = Vec::with_capacity(segments);
    for index in 0..segments {
        let end = u16_at(bytes, ends + index * 2)? as u32;
        let start = u16_at(bytes, starts + index * 2)? as u32;
        let delta = u16_at(bytes, deltas + index * 2)?;
        let range_offset = u16_at(bytes, offsets + index * 2)?;
        if start > end {
            continue;
        }
        // A non-zero idRangeOffset points into a glyph array that follows, and
        // the offset is measured from the offset entry itself. Resolving it per
        // segment would need the character, so segments that use one are
        // expanded here into one entry per character instead.
        if range_offset == 0 {
            out.push(Segment4 { start, end, delta, range_offset: None });
        } else {
            for code in start..=end.min(start + 0xFFFF) {
                let glyph_at =
                    offsets + index * 2 + range_offset as usize + (code - start) as usize * 2;
                let glyph = u16_at(bytes, glyph_at).unwrap_or(0);
                out.push(Segment4 {
                    start: code,
                    end: code,
                    delta,
                    range_offset: Some(glyph as usize),
                });
            }
        }
    }
    (!out.is_empty()).then_some(Format::Four { segments: out })
}

fn parse_format_12(bytes: &[u8], at: usize) -> Option<Format> {
    let count = u32_at(bytes, at + 12)? as usize;
    let mut groups = Vec::with_capacity(count);
    for index in 0..count {
        let record = at + 16 + index * 12;
        groups.push(Group12 {
            start: u32_at(bytes, record)?,
            end: u32_at(bytes, record + 4)?,
            start_glyph: u32_at(bytes, record + 8)?,
        });
    }
    (!groups.is_empty()).then_some(Format::Twelve { groups })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menlo() -> Option<Vec<u8>> {
        std::fs::read("/System/Library/Fonts/Menlo.ttc").ok()
    }

    #[test]
    fn latin1_stops_colliding_with_the_macroman_symbols() {
        // The defect this replaced, stated as the collision it produced: the
        // legacy Macintosh subtable maps byte 0xB7 to ∑ and 0xB0 to ∞, so a
        // lookup that took it returned one glyph for two different characters.
        let Some(bytes) = menlo() else { return };
        let cmap = Cmap::parse(&bytes, 0);
        assert!(cmap.is_usable());
        for (a, b) in [('\u{00B7}', '\u{2211}'), ('\u{00B0}', '\u{221E}'), ('\u{00D7}', '\u{25CA}')]
        {
            let (ga, gb) = (cmap.glyph(a), cmap.glyph(b));
            assert!(ga != 0 && gb != 0, "{a:?}/{b:?} missing from the font");
            assert_ne!(ga, gb, "{a:?} and {b:?} still share a glyph");
        }
    }

    #[test]
    fn ascii_and_accented_letters_are_distinct_and_present() {
        let Some(bytes) = menlo() else { return };
        let cmap = Cmap::parse(&bytes, 0);
        let mut seen = std::collections::HashSet::new();
        for ch in ['A', 'a', 'e', '\u{00E9}', '\u{00FC}', '\u{00E7}', '\u{00CA}'] {
            let glyph = cmap.glyph(ch);
            assert_ne!(glyph, 0, "{ch:?} is missing");
            assert!(seen.insert(glyph), "{ch:?} shares a glyph with an earlier character");
        }
    }

    #[test]
    fn a_character_the_font_lacks_is_zero_rather_than_a_wrong_glyph() {
        let Some(bytes) = menlo() else { return };
        let cmap = Cmap::parse(&bytes, 0);
        assert_eq!(cmap.glyph('\u{4E00}'), 0, "Menlo should not claim to have kanji");
    }

    #[test]
    fn every_face_of_a_collection_parses() {
        let Some(bytes) = menlo() else { return };
        for face in 0..4 {
            let cmap = Cmap::parse(&bytes, face);
            assert!(cmap.is_usable(), "face {face} had no Unicode subtable");
            assert_ne!(cmap.glyph('A'), 0);
        }
    }

    #[test]
    fn a_face_index_past_the_end_of_a_collection_is_clamped_rather_than_panicking() {
        let Some(bytes) = menlo() else { return };
        assert!(Cmap::parse(&bytes, 99).is_usable());
    }

    #[test]
    fn nonsense_bytes_are_unusable_rather_than_a_panic() {
        assert!(!Cmap::parse(&[0u8; 8], 0).is_usable());
        assert!(!Cmap::parse(b"ttcf", 0).is_usable());
        assert_eq!(Cmap::parse(&[], 0).glyph('A'), 0);
    }

    #[test]
    fn a_japanese_face_carries_the_kanji_the_latin_one_lacks() {
        let Ok(bytes) = std::fs::read("/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc") else {
            return;
        };
        let cmap = Cmap::parse(&bytes, 0);
        assert!(cmap.is_usable());
        for ch in ['\u{4E00}', '\u{3042}', '\u{9AA8}'] {
            assert_ne!(cmap.glyph(ch), 0, "{ch:?} is missing");
        }
    }
}
