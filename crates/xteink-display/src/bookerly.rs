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

#[derive(Debug)]
pub struct Font {
    pub bitmap: &'static [u8],
    pub glyphs: &'static [Glyph],
    pub intervals: &'static [Interval],
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
