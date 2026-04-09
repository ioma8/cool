#![no_std]

#[cfg(test)]
extern crate std;

pub mod bookerly;
mod epub;
mod paginator;
mod text;

use bookerly::Glyph;
pub use epub::{CacheBuildResult, EPUB_RENDER_WORKSPACE_BYTES};
use paginator::{
    NoopPaginationObserver, PaginationConfig, PaginationEvent, PaginationRenderer, PaginatorState,
};
use text::{WrappedLine, layout_wrapped_text_page};

const PHYSICAL_WIDTH: u16 = 800;
const PHYSICAL_HEIGHT: u16 = 480;
const CACHED_TEXT_CHUNK: usize = 1024;
const CACHED_LINE_LEN: usize = 1024;
pub(crate) const CACHE_LINE_BREAK_MARKER: char = '\u{001E}';
pub(crate) const CACHE_PARAGRAPH_BREAK_MARKER: char = '\u{001F}';
pub(crate) const CACHE_PAGE_BREAK_MARKER: char = '\u{001D}';
pub(crate) const CACHE_LAYOUT_STREAM_MARKER: char = '\u{001C}';
pub const DISPLAY_WIDTH_BYTES: u16 = PHYSICAL_WIDTH / 8;
pub const BUFFER_SIZE: usize = (DISPLAY_WIDTH_BYTES as usize) * (PHYSICAL_HEIGHT as usize);

pub const DISPLAY_WIDTH: u16 = 480;
pub const DISPLAY_HEIGHT: u16 = 800;

pub fn reader_footer_height() -> u16 {
    bookerly::BOOKERLY.line_height_px().saturating_add(8)
}

pub fn reader_content_height() -> u16 {
    DISPLAY_HEIGHT.saturating_sub(reader_footer_height())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CachedPageRenderResult {
    pub rendered_page: usize,
    pub consumed_bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CachedPageRenderFromOffsetResult {
    pub page_start_byte: usize,
    pub next_page_start_byte: usize,
    pub rendered_page: usize,
    pub consumed_bytes: usize,
}

#[derive(Clone, Copy)]
struct ReplayProgress {
    target_complete: bool,
    consumed_bytes: usize,
}

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

    pub fn fill_rect(&mut self, x: u16, y: u16, width: u16, height: u16, color: u8) {
        let end_x = x.saturating_add(width).min(DISPLAY_WIDTH);
        let end_y = y.saturating_add(height).min(DISPLAY_HEIGHT);
        for py in y..end_y {
            for px in x..end_x {
                self.set_pixel(px, py, color == 0);
            }
        }
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
        let base_x = i32::from(x);
        let mut cursor_y = i32::from(y);
        let line_height = i32::from(bookerly::BOOKERLY.line_height_px());

        for line in text.split('\n') {
            bookerly::BOOKERLY.shape_text(line, |glyph, glyph_x, glyph_y| {
                let left = base_x + glyph_x + i32::from(glyph.left);
                let top = cursor_y + glyph_y + i32::from(glyph.top);
                self.draw_glyph(glyph, left, top);
            });
            cursor_y += line_height;
        }
    }

    pub fn draw_wrapped_text(&mut self, x: u16, y: u16, text: &str, max_y: u16) -> u16 {
        self.layout_wrapped_text_internal(x, y, text, max_y, true)
    }

    fn layout_wrapped_text_internal(
        &mut self,
        x: u16,
        y: u16,
        text: &str,
        max_y: u16,
        draw: bool,
    ) -> u16 {
        const LINE_BUF_LEN: usize = 512;
        let mut line = WrappedLine::<LINE_BUF_LEN>::new();
        let result = layout_wrapped_text_page(&mut line, x, y, text, max_y, |draw_x, draw_y, line_text| {
            if draw {
                self.draw_text(draw_x, draw_y, line_text);
            }
        });
        result.next_y
    }

    pub(crate) fn layout_wrapped_text_page_result(
        &mut self,
        x: u16,
        y: u16,
        text: &str,
        max_y: u16,
        draw: bool,
    ) -> text::WrappedTextLayoutResult {
        const LINE_BUF_LEN: usize = 512;
        let mut line = WrappedLine::<LINE_BUF_LEN>::new();
        layout_wrapped_text_page(&mut line, x, y, text, max_y, |draw_x, draw_y, line_text| {
            if draw {
                self.draw_text(draw_x, draw_y, line_text);
            }
        })
    }

    pub fn render_cached_text_page<R>(
        &mut self,
        read_text: &mut R,
        target_page: usize,
    ) -> Result<usize, xteink_epub::EpubError>
    where
        R: FnMut(&mut [u8]) -> Result<usize, xteink_epub::EpubError>,
    {
        Ok(self
            .render_cached_text_page_with_progress(read_text, target_page, || false)?
            .rendered_page)
    }

    pub fn render_cached_text_page_with_cancel<R, C>(
        &mut self,
        read_text: &mut R,
        target_page: usize,
        should_cancel: C,
    ) -> Result<usize, xteink_epub::EpubError>
    where
        R: FnMut(&mut [u8]) -> Result<usize, xteink_epub::EpubError>,
        C: FnMut() -> bool,
    {
        Ok(self
            .render_cached_text_page_with_progress(read_text, target_page, should_cancel)?
            .rendered_page)
    }

    pub fn render_cached_text_page_with_progress<R, C>(
        &mut self,
        read_text: &mut R,
        target_page: usize,
        should_cancel: C,
    ) -> Result<CachedPageRenderResult, xteink_epub::EpubError>
    where
        R: FnMut(&mut [u8]) -> Result<usize, xteink_epub::EpubError>,
        C: FnMut() -> bool,
    {
        let result = self.render_cached_text_page_with_offset_from_reader(
            read_text,
            0,
            target_page,
            should_cancel,
        )?;
        Ok(CachedPageRenderResult {
            rendered_page: result.rendered_page,
            consumed_bytes: result.consumed_bytes,
        })
    }

    pub fn render_cached_text_page_from_offset<R>(
        &mut self,
        read_text: &mut R,
        page_start_byte: usize,
    ) -> Result<CachedPageRenderFromOffsetResult, xteink_epub::EpubError>
    where
        R: FnMut(&mut [u8]) -> Result<usize, xteink_epub::EpubError>,
    {
        self.render_cached_text_page_from_offset_with_progress(read_text, page_start_byte, || false)
    }

    pub fn render_cached_text_page_from_offset_with_progress<R, C>(
        &mut self,
        read_text: &mut R,
        page_start_byte: usize,
        should_cancel: C,
    ) -> Result<CachedPageRenderFromOffsetResult, xteink_epub::EpubError>
    where
        R: FnMut(&mut [u8]) -> Result<usize, xteink_epub::EpubError>,
        C: FnMut() -> bool,
    {
        self.render_cached_text_page_with_offset_from_reader(
            read_text,
            page_start_byte,
            0,
            should_cancel,
        )
    }

    pub fn render_cached_text_page_from_offset_for_page<R>(
        &mut self,
        read_text: &mut R,
        page_start_byte: usize,
        target_page: usize,
    ) -> Result<CachedPageRenderFromOffsetResult, xteink_epub::EpubError>
    where
        R: FnMut(&mut [u8]) -> Result<usize, xteink_epub::EpubError>,
    {
        self.render_cached_text_page_from_offset_for_page_with_progress(
            read_text,
            page_start_byte,
            target_page,
            || false,
        )
    }

    pub fn render_cached_text_page_from_offset_for_page_with_progress<R, C>(
        &mut self,
        read_text: &mut R,
        page_start_byte: usize,
        target_page: usize,
        should_cancel: C,
    ) -> Result<CachedPageRenderFromOffsetResult, xteink_epub::EpubError>
    where
        R: FnMut(&mut [u8]) -> Result<usize, xteink_epub::EpubError>,
        C: FnMut() -> bool,
    {
        self.render_cached_text_page_with_offset_from_reader(
            read_text,
            page_start_byte,
            target_page,
            should_cancel,
        )
    }

    fn render_cached_text_page_with_offset_from_reader<R, C>(
        &mut self,
        read_text: &mut R,
        page_start_byte: usize,
        target_page: usize,
        mut should_cancel: C,
    ) -> Result<CachedPageRenderFromOffsetResult, xteink_epub::EpubError>
    where
        R: FnMut(&mut [u8]) -> Result<usize, xteink_epub::EpubError>,
        C: FnMut() -> bool,
    {
        let mut state = PaginatorState::<CACHED_LINE_LEN>::new(PaginationConfig {
            target_page,
            draw_target_page: true,
            stop_after_target_page: true,
            preserve_target_page_framebuffer: false,
            start_page: 0,
            start_cursor_y: 0,
        });
        let mut read_buffer = [0u8; CACHED_TEXT_CHUNK];
        let mut utf8_carry = [0u8; 4];
        let mut utf8_carry_len = 0usize;
        let mut consumed_bytes = 0usize;
        let mut skip_remaining = page_start_byte;

        self.clear(0xFF);

        loop {
            if should_cancel() {
                return Err(xteink_epub::EpubError::Cancelled);
            }
            let read_len = read_text(&mut read_buffer)?;
            if read_len == 0 {
                break;
            }
            let mut source = &read_buffer[..read_len];
            if skip_remaining > 0 {
                let skip_len = skip_remaining.min(source.len());
                skip_remaining -= skip_len;
                source = &source[skip_len..];
                if source.is_empty() {
                    continue;
                }
            }

            let mut chunk = [0u8; CACHED_TEXT_CHUNK + 4];
            chunk[..utf8_carry_len].copy_from_slice(&utf8_carry[..utf8_carry_len]);
            chunk[utf8_carry_len..utf8_carry_len + source.len()].copy_from_slice(source);
            let mut source = &chunk[..utf8_carry_len + source.len()];
            utf8_carry_len = 0;

            while !source.is_empty() {
                match core::str::from_utf8(source) {
                    Ok(text) => {
                        let progress = replay_cached_events(self, &mut state, target_page, text)?;
                        consumed_bytes = consumed_bytes.saturating_add(progress.consumed_bytes);
                        if progress.target_complete {
                            return Ok(CachedPageRenderFromOffsetResult {
                                page_start_byte,
                                next_page_start_byte: page_start_byte.saturating_add(consumed_bytes),
                                rendered_page: state.current_page(),
                                consumed_bytes,
                            });
                        }
                        source = &[];
                    }
                    Err(err) => {
                        let valid = err.valid_up_to();
                        if valid > 0 {
                            let text = core::str::from_utf8(&source[..valid]).unwrap_or("");
                            let progress = replay_cached_events(self, &mut state, target_page, text)?;
                            consumed_bytes = consumed_bytes.saturating_add(progress.consumed_bytes);
                            if progress.target_complete {
                                return Ok(CachedPageRenderFromOffsetResult {
                                    page_start_byte,
                                    next_page_start_byte: page_start_byte
                                        .saturating_add(consumed_bytes),
                                    rendered_page: state.current_page(),
                                    consumed_bytes,
                                });
                            }
                        }
                        if err.error_len().is_none() {
                            utf8_carry_len = source.len().min(utf8_carry.len());
                            utf8_carry[..utf8_carry_len].copy_from_slice(&source[..utf8_carry_len]);
                            break;
                        }
                        source = &source[valid.saturating_add(err.error_len().unwrap_or(0))..];
                    }
                }
            }
        }

        if utf8_carry_len > 0 {
            let text = core::str::from_utf8(&utf8_carry[..utf8_carry_len]).unwrap_or("");
            let progress = replay_cached_events(self, &mut state, target_page, text)?;
            consumed_bytes = consumed_bytes.saturating_add(progress.consumed_bytes);
            if progress.target_complete {
                return Ok(CachedPageRenderFromOffsetResult {
                    page_start_byte,
                    next_page_start_byte: page_start_byte.saturating_add(consumed_bytes),
                    rendered_page: state.current_page(),
                    consumed_bytes,
                });
            }
        }

        let mut observer = NoopPaginationObserver;
        let _ = state.feed(
            self,
            &mut observer,
            PaginationConfig {
                target_page,
                draw_target_page: true,
                stop_after_target_page: true,
                preserve_target_page_framebuffer: false,
                start_page: 0,
                start_cursor_y: 0,
            },
            PaginationEvent::End,
        );

        if skip_remaining == 0 {
            Ok(CachedPageRenderFromOffsetResult {
                page_start_byte,
                next_page_start_byte: page_start_byte.saturating_add(consumed_bytes),
                rendered_page: state.current_page(),
                consumed_bytes,
            })
        } else {
            Ok(CachedPageRenderFromOffsetResult {
                page_start_byte,
                next_page_start_byte: page_start_byte,
                rendered_page: state.current_page(),
                consumed_bytes,
            })
        }
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

fn replay_cached_events(
    framebuffer: &mut Framebuffer,
    state: &mut PaginatorState<CACHED_LINE_LEN>,
    target_page: usize,
    text: &str,
) -> Result<ReplayProgress, xteink_epub::EpubError> {
    let mut observer = NoopPaginationObserver;
    let config = PaginationConfig {
        target_page,
        draw_target_page: true,
        stop_after_target_page: true,
        preserve_target_page_framebuffer: false,
        start_page: 0,
        start_cursor_y: 0,
    };

    for (idx, ch) in text.char_indices() {
        let char_bytes = ch.len_utf8();
        let progress = match ch {
            CACHE_LAYOUT_STREAM_MARKER => {
                state.feed(framebuffer, &mut observer, config, PaginationEvent::EnableExplicitBreaks)?
            }
            CACHE_PAGE_BREAK_MARKER => {
                state.feed(framebuffer, &mut observer, config, PaginationEvent::ExplicitPageBreak)?
            }
            CACHE_LINE_BREAK_MARKER | '\n' => {
                state.feed(framebuffer, &mut observer, config, PaginationEvent::LineBreak)?
            }
            CACHE_PARAGRAPH_BREAK_MARKER => {
                state.feed(framebuffer, &mut observer, config, PaginationEvent::ParagraphBreak)?
            }
            _ => {
                let mut encoded = [0u8; 4];
                state.feed(
                    framebuffer,
                    &mut observer,
                    config,
                    PaginationEvent::Text(ch.encode_utf8(&mut encoded)),
                )?
            }
        };
        if progress.target_complete {
            return Ok(ReplayProgress {
                target_complete: true,
                consumed_bytes: idx.saturating_add(char_bytes),
            });
        }
    }

    Ok(ReplayProgress {
        target_complete: false,
        consumed_bytes: text.len(),
    })
}

impl PaginationRenderer for Framebuffer {
    fn clear_to_white(&mut self) {
        self.clear(0xFF);
    }

    fn draw_wrapped_text_block(
        &mut self,
        x: u16,
        y: u16,
        text: &str,
        max_y: u16,
    ) -> text::WrappedTextLayoutResult {
        self.layout_wrapped_text_page_result(x, y, text, max_y, true)
    }

    fn measure_wrapped_text_block(
        &mut self,
        x: u16,
        y: u16,
        text: &str,
        max_y: u16,
    ) -> text::WrappedTextLayoutResult {
        self.layout_wrapped_text_page_result(x, y, text, max_y, false)
    }

    fn display_height(&self) -> u16 {
        reader_content_height()
    }
}

impl Default for Framebuffer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::{WrappedLine, layout_wrapped_text_page};
    use std::borrow::ToOwned;
    use std::string::String;

    #[test]
    fn wrapped_text_page_result_preserves_unconsumed_tail() {
        let mut framebuffer = Framebuffer::new();
        let text = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu "
            .repeat(200);

        let first = framebuffer.layout_wrapped_text_page_result(0, 0, &text, DISPLAY_HEIGHT, false);
        assert!(first.consumed > 0);
        assert!(first.consumed < text.len());

        let second = framebuffer.layout_wrapped_text_page_result(
            0,
            0,
            &text[first.consumed..],
            DISPLAY_HEIGHT,
            false,
        );
        assert!(second.consumed > 0);
    }

    #[test]
    fn cached_render_does_not_drop_long_plain_text_runs() {
        let text = "word ".repeat(2000);
        let target_page = 1usize;

        let mut direct = Framebuffer::new();
        let first = direct.layout_wrapped_text_page_result(0, 0, &text, reader_content_height(), false);
        assert!(first.consumed > 0);
        let remaining = &text[first.consumed..];
        direct.clear(0xFF);
        let _ = direct.layout_wrapped_text_page_result(
            0,
            0,
            remaining,
            reader_content_height(),
            true,
        );

        let bytes = text.into_bytes();
        let mut cached = Framebuffer::new();
        let mut offset = 0usize;
        let cached_result = cached.render_cached_text_page(
            &mut |buffer| {
                if offset >= bytes.len() {
                    return Ok(0);
                }
                let end = (offset + buffer.len()).min(bytes.len());
                let chunk = &bytes[offset..end];
                buffer[..chunk.len()].copy_from_slice(chunk);
                offset = end;
                Ok(chunk.len())
            },
            target_page,
        );
        assert_eq!(cached_result.expect("cached render should succeed"), target_page);
        assert_eq!(cached.bytes(), direct.bytes());
    }

    #[test]
    fn wrapped_text_does_not_consume_undrawn_line_when_page_is_full() {
        let text = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau";
        let mut line = WrappedLine::<512>::new();
        let mut drawn = std::vec::Vec::<String>::new();
        let result = layout_wrapped_text_page(
            &mut line,
            0,
            0,
            text,
            bookerly::BOOKERLY.line_height_px(),
            |_, _, line_text| drawn.push(line_text.to_owned()),
        );

        assert_eq!(drawn.len(), 1, "test should fill exactly one drawn line");
        assert!(result.consumed < text.len(), "test should leave text for next page");
        assert_eq!(
            text[..result.consumed].trim_end(),
            drawn[0].trim_end(),
            "consumed text must match what was actually drawn on the page"
        );
    }
}
