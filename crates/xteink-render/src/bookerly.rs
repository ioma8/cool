#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Glyph {
    pub codepoint: u32,
    pub width: u8,
    pub height: u8,
    pub advance_x: u16,
    pub layout_advance_x: u16,
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

#[derive(Debug, Clone, Copy)]
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

pub const GLYPH_SHADE_BITS_PER_PIXEL: usize = 2;

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
        let mut chars = text.chars();
        let Some(mut current) = chars.next() else {
            return 0;
        };

        let mut pen_x = 0i32;
        let mut incoming_x_offset = 0i32;
        let mut incoming_y_offset = 0i32;
        let mut incoming_advance_adjust = 0i32;

        loop {
            let glyph = self.glyph_for_char(current);
            on_glyph(glyph, pen_x + incoming_x_offset, incoming_y_offset);

            let Some(next) = chars.next() else {
                pen_x += i32::from(glyph.layout_advance_x) + incoming_advance_adjust;
                break;
            };

            let pair = self.positioning_for_pair(current, next);
            pen_x += i32::from(glyph.layout_advance_x) + i32::from(pair.pen_adjust);
            incoming_x_offset = i32::from(pair.x_offset);
            incoming_y_offset = i32::from(pair.y_offset);
            incoming_advance_adjust = i32::from(pair.advance_adjust);
            current = next;
        }

        pen_x
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

    pub fn glyph_coverage(&self, glyph: &Glyph, x: u8, y: u8) -> u8 {
        if x >= glyph.width || y >= glyph.height {
            return 0;
        }

        let row_bytes = usize::from(glyph.width).div_ceil(4);
        let bitmap = &self.bitmap
            [glyph.data_offset as usize..(glyph.data_offset + glyph.data_length) as usize];
        let row_start = usize::from(y) * row_bytes;
        let byte = bitmap[row_start + usize::from(x / 4)];
        let shift = 6 - ((usize::from(x % 4)) * 2);
        (byte >> shift) & 0b11
    }
}

const fn pair_key(left: u32, right: u32) -> u64 {
    ((left as u64) << 32) | (right as u64)
}
