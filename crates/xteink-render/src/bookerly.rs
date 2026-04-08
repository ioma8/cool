use core_maths::CoreFloat;
use rustybuzz::{Face, Feature, UnicodeBuffer};
use rustybuzz::ttf_parser::Tag;

const FONT_SIZE_PX: f32 = 32.0;
static BOOKERLY_FONT_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../xteink-display/assets/Bookerly-Regular.ttf"
));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Glyph {
    pub codepoint: u32,
    pub width: u8,
    pub height: u8,
    pub advance_x: u16,
    pub left: i16,
    pub top: i16,
    pub data_offset: u32,
    pub data_length: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Interval {
    pub first: u32,
    pub last: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PairPositioning {
    pub key: u64,
    pub pen_adjust: i16,
    pub x_offset: i16,
    pub y_offset: i16,
    pub advance_adjust: i16,
}

#[derive(Debug)]
pub struct Font {
    pub bitmap: &'static [u8],
    pub glyphs: &'static [Glyph],
    pub intervals: &'static [Interval],
    pub pair_positioning: &'static [PairPositioning],
    pub ascender: i16,
    pub descender: i16,
    pub line_height: u16,
    pub replacement_index: usize,
}

include!(concat!(env!("OUT_DIR"), "/bookerly_generated.rs"));

impl Font {
    pub fn line_height_px(&self) -> u16 {
        self.line_height
    }

    pub fn ascender_px(&self) -> i16 {
        self.ascender
    }

    pub fn glyph_for_char(&self, ch: char) -> &Glyph {
        self.glyph_for_codepoint(ch as u32)
            .unwrap_or(&self.glyphs[self.replacement_index])
    }

    pub fn glyph_for_codepoint(&self, codepoint: u32) -> Option<&Glyph> {
        let index = self.glyph_index_for_codepoint(codepoint)?;
        self.glyphs.get(index)
    }

    pub fn positioning_for_pair(&self, left: char, right: char) -> PairPositioning {
        let key = pair_key(left as u32, right as u32);
        self.pair_positioning
            .binary_search_by_key(&key, |pair| pair.key)
            .ok()
            .map(|index| self.pair_positioning[index])
            .unwrap_or_default()
    }

    pub fn shape_text(&self, text: &str, mut on_glyph: impl FnMut(&Glyph, i32, i32)) -> i32 {
        with_bookerly_face(|face| {
            let mut buffer = UnicodeBuffer::new();
            buffer.push_str(text);
            let features = [
                Feature::new(Tag::from_bytes(b"liga"), 0, ..),
                Feature::new(Tag::from_bytes(b"clig"), 0, ..),
                Feature::new(Tag::from_bytes(b"calt"), 0, ..),
            ];
            let shaped = rustybuzz::shape(face, &features, buffer);
            let scale = FONT_SIZE_PX / face.units_per_em() as f32;
            let mut pen_x = 0i32;
            for (ch, position) in text.chars().zip(shaped.glyph_positions()) {
                let x_offset = CoreFloat::round(position.x_offset as f32 * scale) as i32;
                let y_offset = -(CoreFloat::round(position.y_offset as f32 * scale) as i32);
                let glyph = self.glyph_for_char(ch);
                on_glyph(glyph, pen_x + x_offset, y_offset);
                pen_x += CoreFloat::round(position.x_advance as f32 * scale) as i32;
            }

            pen_x
        })
    }

    fn glyph_index_for_codepoint(&self, codepoint: u32) -> Option<usize> {
        let mut left = 0usize;
        let mut right = self.intervals.len();

        while left < right {
            let mid = left + (right - left) / 2;
            let interval = self.intervals[mid];

            if codepoint < interval.first {
                right = mid;
            } else if codepoint > interval.last {
                left = mid + 1;
            } else {
                return Some((interval.offset + (codepoint - interval.first)) as usize);
            }
        }

        None
    }
}

const fn pair_key(left: u32, right: u32) -> u64 {
    ((left as u64) << 32) | (right as u64)
}

fn with_bookerly_face<R>(f: impl FnOnce(&Face<'static>) -> R) -> R {
    let face = Face::from_slice(BOOKERLY_FONT_BYTES, 0).expect("Bookerly face should parse");
    f(&face)
}
