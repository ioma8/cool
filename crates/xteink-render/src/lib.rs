#![no_std]

pub mod bookerly;
mod epub;
mod pagination;
mod text;

use bookerly::Glyph;
use pagination::{CachedPaginationState, CachedTextRenderer};
use text::{WrappedLine, measure_text_width};

const PHYSICAL_WIDTH: u16 = 800;
const PHYSICAL_HEIGHT: u16 = 480;
const CACHED_TEXT_CHUNK: usize = 1024;
const CACHED_LINE_LEN: usize = 1024;
pub const DISPLAY_WIDTH_BYTES: u16 = PHYSICAL_WIDTH / 8;
pub const BUFFER_SIZE: usize = (DISPLAY_WIDTH_BYTES as usize) * (PHYSICAL_HEIGHT as usize);

pub const DISPLAY_WIDTH: u16 = 480;
pub const DISPLAY_HEIGHT: u16 = 800;

pub struct Framebuffer {
    bytes: [u8; BUFFER_SIZE],
}

impl Framebuffer {
    #[must_use]
    pub fn new() -> Self {
        Self {
            bytes: [0xFF; BUFFER_SIZE],
        }
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8; BUFFER_SIZE] {
        &self.bytes
    }

    pub fn clear(&mut self, color: u8) {
        self.bytes.fill(color);
    }

    pub fn set_pixel(&mut self, x: u16, y: u16, black: bool) {
        if x >= DISPLAY_WIDTH || y >= DISPLAY_HEIGHT {
            return;
        }

        let px = y;
        let py = (DISPLAY_WIDTH - 1) - x;
        let idx = (py as usize) * (DISPLAY_WIDTH_BYTES as usize) + (px as usize / 8);
        let bit = 7 - (px % 8);

        if black {
            self.bytes[idx] &= !(1 << bit);
        } else {
            self.bytes[idx] |= 1 << bit;
        }
    }

    pub fn draw_text(&mut self, x: u16, y: u16, text: &str) {
        let mut cursor_x = i32::from(x);
        let mut cursor_y = i32::from(y);
        let line_height = i32::from(bookerly::BOOKERLY.line_height_px());

        for ch in text.chars() {
            if ch == '\n' {
                cursor_x = i32::from(x);
                cursor_y += line_height;
                continue;
            }

            let glyph = bookerly::BOOKERLY.glyph_for_char(ch);
            let left = cursor_x + i32::from(glyph.left);
            let top = cursor_y + i32::from(glyph.top);
            self.draw_glyph(glyph, left, top);
            cursor_x += i32::from(glyph.advance_x);
        }
    }

    pub fn draw_wrapped_text(&mut self, x: u16, y: u16, text: &str, max_y: u16) -> u16 {
        const LINE_BUF_LEN: usize = 512;

        let mut cursor_y = y;
        let line_height = bookerly::BOOKERLY.line_height_px();
        let available_width = i32::from(DISPLAY_WIDTH.saturating_sub(x));
        let mut line = WrappedLine::<LINE_BUF_LEN>::new();

        for paragraph in text.split('\n') {
            for word in paragraph.split_whitespace() {
                let word_width = measure_text_width(word);
                let word_fits = if line.is_empty() {
                    word_width <= available_width
                } else {
                    line.width + measure_text_width(" ") + word_width <= available_width
                };

                if !word_fits && !line.is_empty() {
                    if cursor_y.saturating_add(line_height) > max_y {
                        return cursor_y;
                    }
                    self.draw_text(x, cursor_y, line.as_str());
                    cursor_y = cursor_y.saturating_add(line_height);
                    line.clear();
                }

                if !line.is_empty() {
                    line.push_space();
                }
                line.push_str(word);
            }

            if !line.is_empty() {
                if cursor_y.saturating_add(line_height) > max_y {
                    return cursor_y;
                }
                self.draw_text(x, cursor_y, line.as_str());
                cursor_y = cursor_y.saturating_add(line_height);
                line.clear();
            }
        }

        cursor_y
    }

    pub fn render_cached_text_page<R>(
        &mut self,
        read_text: &mut R,
        target_page: usize,
    ) -> Result<usize, xteink_epub::EpubError>
    where
        R: FnMut(&mut [u8]) -> Result<usize, xteink_epub::EpubError>,
    {
        let mut cursor_y = 0u16;
        let mut current_page = 0usize;
        let line_height = bookerly::BOOKERLY.line_height_px();
        let mut state = CachedPaginationState::<CACHED_LINE_LEN>::new();
        let mut read_buffer = [0u8; CACHED_TEXT_CHUNK];
        let mut done = false;

        self.clear(0xFF);

        loop {
            let read_len = read_text(&mut read_buffer)?;
            if read_len == 0 {
                break;
            }

            let mut source = &read_buffer[..read_len];
            while !source.is_empty() && !done {
                match core::str::from_utf8(source) {
                    Ok(text) => {
                        state.cursor_y = cursor_y;
                        state.current_page = current_page;
                        done = pagination::render_cached_text_snippet(
                            self,
                            &mut state,
                            target_page,
                            line_height,
                            text,
                        )?;
                        cursor_y = state.cursor_y;
                        current_page = state.current_page;
                        source = &[];
                    }
                    Err(err) => {
                        let valid = err.valid_up_to();
                        if valid > 0 {
                            state.cursor_y = cursor_y;
                            state.current_page = current_page;
                            done = pagination::render_cached_text_snippet(
                                self,
                                &mut state,
                                target_page,
                                line_height,
                                core::str::from_utf8(&source[..valid]).unwrap_or(""),
                            )?;
                            cursor_y = state.cursor_y;
                            current_page = state.current_page;
                        }
                        if err.error_len().is_none() {
                            break;
                        }
                        source = &source[valid.saturating_add(err.error_len().unwrap_or(0))..];
                    }
                }
            }

            if done {
                break;
            }
        }

        Ok(current_page)
    }

    fn draw_glyph(&mut self, glyph: &Glyph, x: i32, y: i32) {
        let bitmap = &bookerly::BOOKERLY.bitmap
            [glyph.data_offset as usize..(glyph.data_offset + glyph.data_length) as usize];
        let row_bytes = usize::from(glyph.width).div_ceil(8);

        for row in 0..glyph.height {
            let row_start = usize::from(row) * row_bytes;
            for col in 0..glyph.width {
                let byte = bitmap[row_start + usize::from(col / 8)];
                let mask = 1 << (7 - (col % 8));
                if byte & mask == 0 {
                    continue;
                }

                let px = x + i32::from(col);
                let py = y + i32::from(row);
                if px < 0
                    || py < 0
                    || px >= i32::from(DISPLAY_WIDTH)
                    || py >= i32::from(DISPLAY_HEIGHT)
                {
                    continue;
                }

                self.set_pixel(px as u16, py as u16, true);
            }
        }
    }
}

impl CachedTextRenderer for Framebuffer {
    fn clear_to_white(&mut self) {
        self.clear(0xFF);
    }

    fn draw_text(&mut self, x: u16, y: u16, text: &str) {
        self.draw_text(x, y, text);
    }

    fn measure_text_width(&self, text: &str) -> i32 {
        measure_text_width(text)
    }

    fn display_width(&self) -> u16 {
        DISPLAY_WIDTH
    }

    fn display_height(&self) -> u16 {
        DISPLAY_HEIGHT
    }
}

impl Default for Framebuffer {
    fn default() -> Self {
        Self::new()
    }
}
