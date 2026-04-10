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
    TextStyle,
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
pub(crate) const CACHE_HEADING_START_MARKER: char = '\u{0001}';
pub(crate) const CACHE_HEADING_END_MARKER: char = '\u{0002}';
pub(crate) const CACHE_BOLD_START_MARKER: char = '\u{0003}';
pub(crate) const CACHE_BOLD_END_MARKER: char = '\u{0004}';
pub(crate) const CACHE_ITALIC_START_MARKER: char = '\u{0005}';
pub(crate) const CACHE_ITALIC_END_MARKER: char = '\u{0006}';
pub(crate) const CACHE_QUOTE_START_MARKER: char = '\u{0007}';
pub(crate) const CACHE_QUOTE_END_MARKER: char = '\u{0008}';
pub const DISPLAY_WIDTH_BYTES: u16 = PHYSICAL_WIDTH / 8;
pub const BUFFER_SIZE: usize = (DISPLAY_WIDTH_BYTES as usize) * (PHYSICAL_HEIGHT as usize);
pub const GRAY_LEVELS: u8 = 4;
pub const SHADE_BITS_PER_PIXEL: u8 = 2;
pub const SHADE_BUFFER_SIZE: usize =
    (DISPLAY_WIDTH as usize * DISPLAY_HEIGHT as usize * SHADE_BITS_PER_PIXEL as usize).div_ceil(8);
pub const SHADE_WHITE: u8 = 0;
pub const SHADE_LIGHT: u8 = 1;
pub const SHADE_DARK: u8 = 2;
pub const SHADE_BLACK: u8 = GRAY_LEVELS - 1;

pub const DISPLAY_WIDTH: u16 = 480;
pub const DISPLAY_HEIGHT: u16 = 800;
pub(crate) const QUOTE_INDENT_PX: u16 = 24;

pub fn reader_footer_height() -> u16 {
    footer_line_height_px().saturating_add(8)
}

pub fn reader_content_height() -> u16 {
    DISPLAY_HEIGHT.saturating_sub(reader_footer_height())
}

pub const READER_FOOTER_HORIZONTAL_PADDING: u16 = 4;

pub fn body_line_height_px() -> u16 {
    bookerly::BOOKERLY_BODY.line_height_px()
}

pub fn heading_line_height_px() -> u16 {
    bookerly::BOOKERLY_HEADING.line_height_px()
}

pub fn footer_line_height_px() -> u16 {
    bookerly::BOOKERLY_FOOTER.line_height_px()
}

pub struct ReaderFooterLayout<'a> {
    pub left_x: u16,
    pub left_text: Option<&'a str>,
    pub middle_x: u16,
    pub middle_text: Option<&'a str>,
    pub right_x: u16,
    pub right_text: &'a str,
}

pub fn layout_reader_footer<'a>(
    left_text: Option<&'a str>,
    middle_text: Option<&'a str>,
    right_text: &'a str,
) -> ReaderFooterLayout<'a> {
    let left_x = READER_FOOTER_HORIZONTAL_PADDING;
    let right_width = footer_text_width(right_text);
    let right_x = DISPLAY_WIDTH
        .saturating_sub(READER_FOOTER_HORIZONTAL_PADDING)
        .saturating_sub(right_width);

    let space_width = footer_text_width(" ");
    let left_limit = right_x.saturating_sub(space_width);
    let left_text = left_text.and_then(|text| {
        fitted_text_prefix_with_font(
            text,
            left_limit.saturating_sub(left_x),
            &bookerly::BOOKERLY_FOOTER,
        )
    });
    let left_width = left_text.map_or(0, footer_text_width);
    let middle_x = if left_text.is_some() {
        left_x
            .saturating_add(left_width)
            .saturating_add(space_width)
    } else {
        left_x
    };
    let middle_limit = right_x.saturating_sub(space_width);
    let available_middle_width = middle_limit.saturating_sub(middle_x);
    let middle_text = middle_text.and_then(|text| {
        fitted_text_prefix_with_font(text, available_middle_width, &bookerly::BOOKERLY_FOOTER)
    });

    ReaderFooterLayout {
        left_x,
        left_text,
        middle_x,
        middle_text,
        right_x,
        right_text,
    }
}

pub fn text_width(text: &str) -> u16 {
    text_width_with_font(text, &bookerly::BOOKERLY_BODY)
}

pub fn footer_text_width(text: &str) -> u16 {
    text_width_with_font(text, &bookerly::BOOKERLY_FOOTER)
}

fn text_width_with_font(text: &str, font: &bookerly::Font) -> u16 {
    u16::try_from(font.shape_text(text, |_, _, _| {})).unwrap_or(u16::MAX)
}

fn fitted_text_prefix_with_font<'a>(
    text: &'a str,
    max_width: u16,
    font: &bookerly::Font,
) -> Option<&'a str> {
    if max_width == 0 || text.is_empty() {
        return None;
    }
    if text_width_with_font(text, font) <= max_width {
        return Some(text);
    }

    let mut end = 0usize;
    for (index, ch) in text.char_indices() {
        let next_end = index + ch.len_utf8();
        if text_width_with_font(&text[..next_end], font) > max_width {
            break;
        }
        end = next_end;
    }

    if end == 0 { None } else { Some(&text[..end]) }
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
    shades: [u8; SHADE_BUFFER_SIZE],
}

impl Framebuffer {
    #[must_use]
    pub fn new() -> Self {
        Self {
            shades: [0; SHADE_BUFFER_SIZE],
        }
    }

    #[must_use]
    pub fn shade_storage(&self) -> &[u8; SHADE_BUFFER_SIZE] {
        &self.shades
    }

    #[must_use]
    pub fn bytes(&self) -> [u8; BUFFER_SIZE] {
        let mut out = [0xFF; BUFFER_SIZE];
        self.write_binary_mask(SHADE_DARK, &mut out);
        out
    }

    #[must_use]
    pub fn shade_at(&self, x: u16, y: u16) -> u8 {
        if x >= DISPLAY_WIDTH || y >= DISPLAY_HEIGHT {
            return SHADE_WHITE;
        }

        let pixel_index = usize::from(y) * usize::from(DISPLAY_WIDTH) + usize::from(x);
        let byte_index = pixel_index / 4;
        let shift = 6 - ((pixel_index % 4) * 2);
        (self.shades[byte_index] >> shift) & 0b11
    }

    pub fn set_shade(&mut self, x: u16, y: u16, shade: u8) {
        if x >= DISPLAY_WIDTH || y >= DISPLAY_HEIGHT {
            return;
        }

        let shade = shade.min(SHADE_BLACK);
        let pixel_index = usize::from(y) * usize::from(DISPLAY_WIDTH) + usize::from(x);
        let byte_index = pixel_index / 4;
        let shift = 6 - ((pixel_index % 4) * 2);
        let clear_mask = !(0b11 << shift);
        self.shades[byte_index] = (self.shades[byte_index] & clear_mask) | (shade << shift);
    }

    #[must_use]
    pub fn has_intermediate_shades(&self) -> bool {
        for y in 0..DISPLAY_HEIGHT {
            for x in 0..DISPLAY_WIDTH {
                let shade = self.shade_at(x, y);
                if shade != SHADE_WHITE && shade != SHADE_BLACK {
                    return true;
                }
            }
        }
        false
    }

    pub fn write_binary_mask(&self, threshold: u8, out: &mut [u8; BUFFER_SIZE]) {
        out.fill(0xFF);

        for y in 0..DISPLAY_HEIGHT {
            for x in 0..DISPLAY_WIDTH {
                if self.shade_at(x, y) < threshold {
                    continue;
                }

                let px = y;
                let py = (DISPLAY_WIDTH - 1) - x;
                let idx = (py as usize) * (DISPLAY_WIDTH_BYTES as usize) + (px as usize / 8);
                let bit = 7 - (px % 8);
                out[idx] &= !(1 << bit);
            }
        }
    }

    pub fn clear(&mut self, color: u8) {
        self.fill_shade(if color == 0 { SHADE_BLACK } else { SHADE_WHITE });
    }

    pub fn fill_rect(&mut self, x: u16, y: u16, width: u16, height: u16, color: u8) {
        let end_x = x.saturating_add(width).min(DISPLAY_WIDTH);
        let end_y = y.saturating_add(height).min(DISPLAY_HEIGHT);
        let shade = if color == 0 { SHADE_BLACK } else { SHADE_WHITE };
        for py in y..end_y {
            for px in x..end_x {
                self.set_shade(px, py, shade);
            }
        }
    }

    pub fn set_pixel(&mut self, x: u16, y: u16, black: bool) {
        self.set_shade(x, y, if black { SHADE_BLACK } else { SHADE_WHITE });
    }

    pub fn draw_text(&mut self, x: u16, y: u16, text: &str) {
        self.draw_text_with_font(&bookerly::BOOKERLY_BODY, x, y, text, false, false);
    }

    pub fn draw_heading_text(&mut self, x: u16, y: u16, text: &str) {
        self.draw_text_with_font(&bookerly::BOOKERLY_HEADING, x, y, text, true, false);
    }

    pub fn draw_footer_text(&mut self, x: u16, y: u16, text: &str) {
        self.draw_text_with_font(&bookerly::BOOKERLY_FOOTER, x, y, text, false, false);
    }

    fn draw_text_with_style(&mut self, x: u16, y: u16, text: &str, style: TextStyle) {
        let font = if style.heading && (style.italic || style.quote) {
            &bookerly::BOOKERLY_HEADING_ITALIC
        } else if style.heading {
            &bookerly::BOOKERLY_HEADING
        } else if style.italic || style.quote {
            &bookerly::BOOKERLY_BODY_ITALIC
        } else {
            &bookerly::BOOKERLY_BODY
        };
        let synthetic_bold = style.heading || style.bold;
        let synthetic_italic = (style.italic || style.quote) && !bookerly::BOOKERLY_HAS_REAL_ITALIC;
        self.draw_text_with_font(font, x, y, text, synthetic_bold, synthetic_italic);
    }

    fn draw_text_with_font(
        &mut self,
        font: &bookerly::Font,
        x: u16,
        y: u16,
        text: &str,
        synthetic_bold: bool,
        synthetic_italic: bool,
    ) {
        let base_x = i32::from(x);
        let mut cursor_y = i32::from(y);
        let line_height = i32::from(font.line_height_px());

        for line in text.split('\n') {
            font.shape_text(line, |glyph, glyph_x, glyph_y| {
                let left = base_x + glyph_x + i32::from(glyph.left);
                let top = cursor_y + glyph_y + i32::from(glyph.top);
                self.draw_glyph(font, glyph, left, top, synthetic_italic);
                if synthetic_bold {
                    self.draw_glyph(font, glyph, left + 1, top, synthetic_italic);
                }
            });
            cursor_y += line_height;
        }
    }

    pub fn draw_wrapped_text(&mut self, x: u16, y: u16, text: &str, max_y: u16) -> u16 {
        self.layout_wrapped_text_internal(x, y, text, max_y, TextStyle::default(), true)
    }

    fn layout_wrapped_text_internal(
        &mut self,
        x: u16,
        y: u16,
        text: &str,
        max_y: u16,
        style: TextStyle,
        draw: bool,
    ) -> u16 {
        const LINE_BUF_LEN: usize = 512;
        let mut line = WrappedLine::<LINE_BUF_LEN>::new();
        let result = layout_wrapped_text_page(
            &mut line,
            x,
            y,
            text,
            max_y,
            style,
            |draw_x, draw_y, line_text| {
                if draw {
                    self.draw_text_with_style(draw_x, draw_y, line_text, style);
                }
            },
        );
        result.next_y
    }

    pub(crate) fn layout_wrapped_text_page_result(
        &mut self,
        x: u16,
        y: u16,
        text: &str,
        max_y: u16,
        style: TextStyle,
        draw: bool,
    ) -> text::WrappedTextLayoutResult {
        const LINE_BUF_LEN: usize = 512;
        let mut line = WrappedLine::<LINE_BUF_LEN>::new();
        layout_wrapped_text_page(
            &mut line,
            x,
            y,
            text,
            max_y,
            style,
            |draw_x, draw_y, line_text| {
                if draw {
                    self.draw_text_with_style(draw_x, draw_y, line_text, style);
                }
            },
        )
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
                                next_page_start_byte: page_start_byte
                                    .saturating_add(consumed_bytes),
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
                            let progress =
                                replay_cached_events(self, &mut state, target_page, text)?;
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

    fn draw_glyph(
        &mut self,
        font: &bookerly::Font,
        glyph: &Glyph,
        x: i32,
        y: i32,
        synthetic_italic: bool,
    ) {
        for row in 0..glyph.height {
            for col in 0..glyph.width {
                let coverage = font.glyph_coverage(glyph, col, row);
                if coverage == 0 {
                    continue;
                }

                let italic_shift = if synthetic_italic {
                    (i32::from(glyph.height.saturating_sub(row).saturating_sub(1))) / 4
                } else {
                    0
                };
                let px = x + italic_shift + i32::from(col);
                let py = y + i32::from(row);
                if px < 0
                    || py < 0
                    || px >= i32::from(DISPLAY_WIDTH)
                    || py >= i32::from(DISPLAY_HEIGHT)
                {
                    continue;
                }

                let current = self.shade_at(px as u16, py as u16);
                self.set_shade(px as u16, py as u16, current.max(coverage));
            }
        }
    }

    fn fill_shade(&mut self, shade: u8) {
        let shade = shade.min(SHADE_BLACK);
        let packed = (shade << 6) | (shade << 4) | (shade << 2) | shade;
        self.shades.fill(packed);
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
            CACHE_LAYOUT_STREAM_MARKER => state.feed(
                framebuffer,
                &mut observer,
                config,
                PaginationEvent::EnableExplicitBreaks,
            )?,
            CACHE_HEADING_START_MARKER => state.feed(
                framebuffer,
                &mut observer,
                config,
                PaginationEvent::HeadingStart,
            )?,
            CACHE_HEADING_END_MARKER => state.feed(
                framebuffer,
                &mut observer,
                config,
                PaginationEvent::HeadingEnd,
            )?,
            CACHE_BOLD_START_MARKER => state.feed(
                framebuffer,
                &mut observer,
                config,
                PaginationEvent::BoldStart,
            )?,
            CACHE_BOLD_END_MARKER => {
                state.feed(framebuffer, &mut observer, config, PaginationEvent::BoldEnd)?
            }
            CACHE_ITALIC_START_MARKER => state.feed(
                framebuffer,
                &mut observer,
                config,
                PaginationEvent::ItalicStart,
            )?,
            CACHE_ITALIC_END_MARKER => state.feed(
                framebuffer,
                &mut observer,
                config,
                PaginationEvent::ItalicEnd,
            )?,
            CACHE_QUOTE_START_MARKER => state.feed(
                framebuffer,
                &mut observer,
                config,
                PaginationEvent::QuoteStart,
            )?,
            CACHE_QUOTE_END_MARKER => state.feed(
                framebuffer,
                &mut observer,
                config,
                PaginationEvent::QuoteEnd,
            )?,
            CACHE_PAGE_BREAK_MARKER => state.feed(
                framebuffer,
                &mut observer,
                config,
                PaginationEvent::ExplicitPageBreak,
            )?,
            CACHE_LINE_BREAK_MARKER | '\n' => state.feed(
                framebuffer,
                &mut observer,
                config,
                PaginationEvent::LineBreak,
            )?,
            CACHE_PARAGRAPH_BREAK_MARKER => state.feed(
                framebuffer,
                &mut observer,
                config,
                PaginationEvent::ParagraphBreak,
            )?,
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
        style: TextStyle,
    ) -> text::WrappedTextLayoutResult {
        self.layout_wrapped_text_page_result(x, y, text, max_y, style, true)
    }

    fn measure_wrapped_text_block(
        &mut self,
        x: u16,
        y: u16,
        text: &str,
        max_y: u16,
        style: TextStyle,
    ) -> text::WrappedTextLayoutResult {
        self.layout_wrapped_text_page_result(x, y, text, max_y, style, false)
    }

    fn display_height(&self) -> u16 {
        reader_content_height()
    }

    fn line_height(&self, style: TextStyle) -> u16 {
        if style.heading {
            heading_line_height_px()
        } else {
            body_line_height_px()
        }
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
    use std::format;
    use std::string::String;

    #[test]
    fn wrapped_text_page_result_preserves_unconsumed_tail() {
        let mut framebuffer = Framebuffer::new();
        let text =
            "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu ".repeat(200);

        let first = framebuffer.layout_wrapped_text_page_result(
            0,
            0,
            &text,
            DISPLAY_HEIGHT,
            TextStyle::default(),
            false,
        );
        assert!(first.consumed > 0);
        assert!(first.consumed < text.len());

        let second = framebuffer.layout_wrapped_text_page_result(
            0,
            0,
            &text[first.consumed..],
            DISPLAY_HEIGHT,
            TextStyle::default(),
            false,
        );
        assert!(second.consumed > 0);
    }

    #[test]
    fn cached_render_does_not_drop_long_plain_text_runs() {
        let text = "word ".repeat(2000);
        let target_page = 1usize;

        let mut direct = Framebuffer::new();
        let first = direct.layout_wrapped_text_page_result(
            0,
            0,
            &text,
            reader_content_height(),
            TextStyle::default(),
            false,
        );
        assert!(first.consumed > 0);
        let remaining = &text[first.consumed..];
        direct.clear(0xFF);
        let _ = direct.layout_wrapped_text_page_result(
            0,
            0,
            remaining,
            reader_content_height(),
            TextStyle::default(),
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
        assert_eq!(
            cached_result.expect("cached render should succeed"),
            target_page
        );
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
            TextStyle::default(),
            |_, _, line_text| drawn.push(line_text.to_owned()),
        );

        assert_eq!(drawn.len(), 1, "test should fill exactly one drawn line");
        assert!(
            result.consumed < text.len(),
            "test should leave text for next page"
        );
        assert_eq!(
            text[..result.consumed].trim_end(),
            drawn[0].trim_end(),
            "consumed text must match what was actually drawn on the page"
        );
    }

    #[test]
    fn footer_layout_clips_left_text_before_right_progress() {
        let layout = layout_reader_footer(
            Some("An Extremely Long Chapter Title That Must Be Clipped"),
            None,
            "100%",
        );

        let clipped = layout.left_text.expect("left text should still render");
        assert!(clipped.len() < "An Extremely Long Chapter Title That Must Be Clipped".len());
        let space_width = footer_text_width(" ");
        assert!(
            layout
                .left_x
                .saturating_add(footer_text_width(clipped))
                .saturating_add(space_width)
                <= layout.right_x,
            "left text must leave room for a separating space before progress"
        );
    }

    #[test]
    fn cached_heading_markers_render_heading_run_with_body_following() {
        let text = format!(
            "{layout}{heading_start}Chapter{heading_end}{line}{line}Body",
            layout = CACHE_LAYOUT_STREAM_MARKER,
            heading_start = CACHE_HEADING_START_MARKER,
            heading_end = CACHE_HEADING_END_MARKER,
            line = CACHE_LINE_BREAK_MARKER,
        );
        let bytes = text.into_bytes();

        let mut cached = Framebuffer::new();
        let mut offset = 0usize;
        let result = cached.render_cached_text_page(
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
            0,
        );
        assert_eq!(result.expect("cached render should succeed"), 0);

        let mut direct = Framebuffer::new();
        direct.clear(0xFF);
        direct.draw_heading_text(0, 0, "Chapter");
        let body_y = heading_line_height_px().saturating_add(body_line_height_px() * 2);
        direct.draw_text(0, body_y, "Body");

        assert_eq!(cached.bytes(), direct.bytes());
    }

    #[test]
    fn italic_style_renders_differently_from_body_text() {
        let mut body = Framebuffer::new();
        let _ = body.layout_wrapped_text_page_result(
            0,
            0,
            "Italic",
            DISPLAY_HEIGHT,
            TextStyle::default(),
            true,
        );

        let mut italic = Framebuffer::new();
        let _ = italic.layout_wrapped_text_page_result(
            0,
            0,
            "Italic",
            DISPLAY_HEIGHT,
            TextStyle {
                heading: false,
                bold: false,
                italic: true,
                quote: false,
            },
            true,
        );

        assert_ne!(italic.bytes(), body.bytes());
    }

    #[test]
    fn real_italic_font_is_available_when_italic_asset_is_present() {
        assert!(bookerly::BOOKERLY_HAS_REAL_ITALIC);
    }

    #[test]
    fn cached_quote_markers_indent_visible_text() {
        let text = format!(
            "{layout}{quote_start}Quote{quote_end}",
            layout = CACHE_LAYOUT_STREAM_MARKER,
            quote_start = CACHE_QUOTE_START_MARKER,
            quote_end = CACHE_QUOTE_END_MARKER,
        );
        let bytes = text.into_bytes();
        let mut cached = Framebuffer::new();
        let mut offset = 0usize;
        let result = cached.render_cached_text_page(
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
            0,
        );
        assert_eq!(result.expect("cached render should succeed"), 0);

        let mut min_ink_x = DISPLAY_WIDTH;
        for y in 0..DISPLAY_HEIGHT {
            for x in 0..DISPLAY_WIDTH {
                if cached.shade_at(x, y) != SHADE_WHITE {
                    min_ink_x = min_ink_x.min(x);
                }
            }
        }

        assert!(
            min_ink_x >= QUOTE_INDENT_PX,
            "expected quote text to start at or after indent, got {}",
            min_ink_x
        );
    }
}
