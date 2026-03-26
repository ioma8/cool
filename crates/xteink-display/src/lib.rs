#![no_std]

use embedded_hal::{
    delay::DelayNs,
    digital::{InputPin, OutputPin},
    spi::SpiBus,
};
use xteink_epub::{Epub, EpubEvent, EpubSource, ReaderBuffers};

pub mod bookerly;

const PHYSICAL_WIDTH: u16 = 800;
const PHYSICAL_HEIGHT: u16 = 480;
pub const DISPLAY_WIDTH_BYTES: u16 = PHYSICAL_WIDTH / 8;
pub const BUFFER_SIZE: usize = (DISPLAY_WIDTH_BYTES as usize) * (PHYSICAL_HEIGHT as usize);

pub const DISPLAY_WIDTH: u16 = 480;
pub const DISPLAY_HEIGHT: u16 = 800;

const CMD_SOFT_RESET: u8 = 0x12;
const CMD_BOOSTER_SOFT_START: u8 = 0x0C;
const CMD_DRIVER_OUTPUT_CONTROL: u8 = 0x01;
const CMD_BORDER_WAVEFORM: u8 = 0x3C;
const CMD_TEMP_SENSOR_CONTROL: u8 = 0x18;
const CMD_DATA_ENTRY_MODE: u8 = 0x11;
const CMD_SET_RAM_X_RANGE: u8 = 0x44;
const CMD_SET_RAM_Y_RANGE: u8 = 0x45;
const CMD_SET_RAM_X_COUNTER: u8 = 0x4E;
const CMD_SET_RAM_Y_COUNTER: u8 = 0x4F;
const CMD_WRITE_RAM_BW: u8 = 0x24;
const CMD_WRITE_RAM_RED: u8 = 0x26;
const CMD_DISPLAY_UPDATE_CTRL1: u8 = 0x21;
const CMD_DISPLAY_UPDATE_CTRL2: u8 = 0x22;
const CMD_MASTER_ACTIVATION: u8 = 0x20;
const CMD_WRITE_TEMP: u8 = 0x1A;
const CMD_DEEP_SLEEP: u8 = 0x10;
const EMBEDDED_EPUB_BYTES: &[u8] = include_bytes!("../../../test/epubs/test_display_none.epub");

const CTRL1_NORMAL: u8 = 0x00;
const CTRL1_BYPASS_RED: u8 = 0x40;
const DATA_ENTRY_X_INC_Y_DEC: u8 = 0x01;
const TEMP_SENSOR_INTERNAL: u8 = 0x80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshMode {
    Full,
    Half,
    Fast,
}

pub struct SSD1677Display<SPI, CS, DC, RST, BUSY, DELAY>
where
    SPI: SpiBus,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
    DELAY: DelayNs,
{
    spi: SPI,
    cs: CS,
    dc: DC,
    rst: RST,
    busy: BUSY,
    delay: DELAY,
    framebuffer: [u8; BUFFER_SIZE],
    is_screen_on: bool,
}

impl<SPI, CS, DC, RST, BUSY, DELAY> SSD1677Display<SPI, CS, DC, RST, BUSY, DELAY>
where
    SPI: SpiBus,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
    DELAY: DelayNs,
{
    pub fn new(spi: SPI, cs: CS, dc: DC, rst: RST, busy: BUSY, delay: DELAY) -> Self {
        Self {
            spi,
            cs,
            dc,
            rst,
            busy,
            delay,
            framebuffer: [0xFF; BUFFER_SIZE],
            is_screen_on: false,
        }
    }

    pub fn init(&mut self) {
        self.reset_display();
        self.init_display_controller();
    }

    pub fn clear(&mut self, color: u8) {
        self.framebuffer.fill(color);
    }

    pub fn framebuffer(&self) -> &[u8; BUFFER_SIZE] {
        &self.framebuffer
    }

    pub fn spi(&self) -> &SPI {
        &self.spi
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
            self.framebuffer[idx] &= !(1 << bit);
        } else {
            self.framebuffer[idx] |= 1 << bit;
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
                let word_width = self.measure_text_width(word);
                let word_fits = if line.is_empty() {
                    word_width <= available_width
                } else {
                    line.width + self.measure_text_width(" ") + word_width <= available_width
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

    pub fn render_embedded_epub_first_screen(&mut self) -> Result<(), xteink_epub::EpubError> {
        const ZIP_CD_LEN: usize = 16 * 1024;
        const INFLATE_LEN: usize = 32 * 1024;
        const XML_LEN: usize = 4 * 1024;
        const CATALOG_LEN: usize = 4 * 1024;
        const PATH_LEN: usize = 512;
        const TEXT_LEN: usize = 2048;

        let mut epub = Epub::open(EmbeddedEpub)?;
        let mut zip_cd = [0u8; ZIP_CD_LEN];
        let mut inflate = [0u8; INFLATE_LEN];
        let mut xml = [0u8; XML_LEN];
        let mut catalog = [0u8; CATALOG_LEN];
        let mut path_buf = [0u8; PATH_LEN];
        let mut text = TextBuffer::<TEXT_LEN>::new();
        let mut cursor_y = 0u16;
        let line_height = bookerly::BOOKERLY.line_height_px();

        loop {
            let event = epub.next_event(ReaderBuffers {
                zip_cd: &mut zip_cd,
                inflate: &mut inflate,
                xml: &mut xml,
                catalog: &mut catalog,
                path_buf: &mut path_buf,
            })?;

            let Some(event) = event else {
                break;
            };

            match event {
                EpubEvent::Text(chunk) => text.push(chunk),
                EpubEvent::LineBreak => {
                    cursor_y = self.flush_text_buffer(&mut text, cursor_y);
                    cursor_y = cursor_y.saturating_add(line_height);
                }
                EpubEvent::ParagraphStart | EpubEvent::HeadingStart(_) => {}
                EpubEvent::ParagraphEnd | EpubEvent::HeadingEnd => {
                    cursor_y = self.flush_text_buffer(&mut text, cursor_y);
                    cursor_y = cursor_y.saturating_add(line_height / 2);
                }
                EpubEvent::Image { alt, .. } => {
                    if let Some(alt) = alt {
                        text.push(alt);
                    }
                }
                EpubEvent::UnsupportedTag => {}
            }

            if cursor_y >= DISPLAY_HEIGHT {
                break;
            }
        }

        let _ = self.flush_text_buffer(&mut text, cursor_y);
        Ok(())
    }

    fn draw_glyph(&mut self, glyph: &bookerly::Glyph, x: i32, y: i32) {
        if glyph.width == 0 || glyph.height == 0 || glyph.data_length == 0 {
            return;
        }

        let row_bytes = usize::from(glyph.width).div_ceil(8);
        let start = glyph.data_offset as usize;
        let end = start + glyph.data_length as usize;
        let bitmap = &bookerly::BOOKERLY.bitmap[start..end];

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

    fn measure_text_width(&self, text: &str) -> i32 {
        text.chars()
            .map(|ch| i32::from(bookerly::BOOKERLY.glyph_for_char(ch).advance_x))
            .sum()
    }

    fn flush_text_buffer<const N: usize>(
        &mut self,
        buffer: &mut TextBuffer<N>,
        cursor_y: u16,
    ) -> u16 {
        if buffer.is_empty() {
            return cursor_y;
        }

        let next_y = self.draw_wrapped_text(0, cursor_y, buffer.as_str(), DISPLAY_HEIGHT);
        buffer.clear();
        next_y
    }

    pub fn display_buffer(&mut self, mode: RefreshMode) {
        self.set_ram_area(0, 0, PHYSICAL_WIDTH, PHYSICAL_HEIGHT);

        if mode != RefreshMode::Fast {
            self.send_command(CMD_WRITE_RAM_BW);
            self.write_framebuffer();
            self.send_command(CMD_WRITE_RAM_RED);
            self.write_framebuffer();
        } else {
            self.send_command(CMD_WRITE_RAM_BW);
            self.write_framebuffer();
        }

        self.refresh_display(mode, false);

        if mode == RefreshMode::Fast {
            self.set_ram_area(0, 0, PHYSICAL_WIDTH, PHYSICAL_HEIGHT);
            self.send_command(CMD_WRITE_RAM_RED);
            self.write_framebuffer();
        }
    }

    pub fn refresh_full(&mut self) {
        self.display_buffer(RefreshMode::Full);
    }

    pub fn refresh_fast(&mut self) {
        self.display_buffer(RefreshMode::Fast);
    }

    pub fn deep_sleep(&mut self) {
        if self.is_screen_on {
            self.send_command(CMD_DISPLAY_UPDATE_CTRL1);
            self.send_data_byte(CTRL1_BYPASS_RED);
            self.send_command(CMD_DISPLAY_UPDATE_CTRL2);
            self.send_data_byte(0x03);
            self.send_command(CMD_MASTER_ACTIVATION);
            self.wait_while_busy();
            self.is_screen_on = false;
        }

        self.send_command(CMD_DEEP_SLEEP);
        self.send_data_byte(0x01);
    }

    fn reset_display(&mut self) {
        let _ = self.rst.set_high();
        self.delay.delay_ms(20);
        let _ = self.rst.set_low();
        self.delay.delay_ms(2);
        let _ = self.rst.set_high();
        self.delay.delay_ms(20);
    }

    fn init_display_controller(&mut self) {
        self.send_command(CMD_SOFT_RESET);
        self.wait_while_busy();
        self.delay.delay_ms(10);

        self.send_command(CMD_TEMP_SENSOR_CONTROL);
        self.send_data_byte(TEMP_SENSOR_INTERNAL);

        self.send_command(CMD_BOOSTER_SOFT_START);
        self.send_data_byte(0xAE);
        self.send_data_byte(0xC7);
        self.send_data_byte(0xC3);
        self.send_data_byte(0xC0);
        self.send_data_byte(0x40);

        self.send_command(CMD_DRIVER_OUTPUT_CONTROL);
        self.send_data_byte(((PHYSICAL_HEIGHT - 1) & 0xFF) as u8);
        self.send_data_byte((((PHYSICAL_HEIGHT - 1) >> 8) & 0xFF) as u8);
        self.send_data_byte(0x02);

        self.send_command(CMD_BORDER_WAVEFORM);
        self.send_data_byte(0x01);
        self.set_ram_area(0, 0, PHYSICAL_WIDTH, PHYSICAL_HEIGHT);
    }

    fn set_ram_area(&mut self, x: u16, y: u16, w: u16, h: u16) {
        let y = PHYSICAL_HEIGHT - y - h;

        self.send_command(CMD_DATA_ENTRY_MODE);
        self.send_data_byte(DATA_ENTRY_X_INC_Y_DEC);

        self.send_command(CMD_SET_RAM_X_RANGE);
        self.send_data_byte((x & 0xFF) as u8);
        self.send_data_byte(((x >> 8) & 0xFF) as u8);
        self.send_data_byte(((x + w - 1) & 0xFF) as u8);
        self.send_data_byte((((x + w - 1) >> 8) & 0xFF) as u8);

        self.send_command(CMD_SET_RAM_Y_RANGE);
        self.send_data_byte(((y + h - 1) & 0xFF) as u8);
        self.send_data_byte((((y + h - 1) >> 8) & 0xFF) as u8);
        self.send_data_byte((y & 0xFF) as u8);
        self.send_data_byte(((y >> 8) & 0xFF) as u8);

        self.send_command(CMD_SET_RAM_X_COUNTER);
        self.send_data_byte((x & 0xFF) as u8);
        self.send_data_byte(((x >> 8) & 0xFF) as u8);

        self.send_command(CMD_SET_RAM_Y_COUNTER);
        self.send_data_byte(((y + h - 1) & 0xFF) as u8);
        self.send_data_byte((((y + h - 1) >> 8) & 0xFF) as u8);
    }

    fn refresh_display(&mut self, mode: RefreshMode, turn_off_screen: bool) {
        self.send_command(CMD_DISPLAY_UPDATE_CTRL1);
        let ctrl1 = if mode == RefreshMode::Fast {
            CTRL1_NORMAL
        } else {
            CTRL1_BYPASS_RED
        };
        self.send_data_byte(ctrl1);

        let mut display_mode = 0x00;
        if !self.is_screen_on {
            self.is_screen_on = true;
            display_mode |= 0xC0;
        }

        if turn_off_screen {
            self.is_screen_on = false;
            display_mode |= 0x03;
        }

        match mode {
            RefreshMode::Full => display_mode |= 0x34,
            RefreshMode::Half => {
                self.send_command(CMD_WRITE_TEMP);
                self.send_data_byte(0x5A);
                display_mode |= 0xD4;
            }
            RefreshMode::Fast => display_mode |= 0x1C,
        }

        self.send_command(CMD_DISPLAY_UPDATE_CTRL2);
        self.send_data_byte(display_mode);
        self.send_command(CMD_MASTER_ACTIVATION);
        self.delay.delay_ms(10);
        self.wait_while_busy();
    }

    fn wait_while_busy(&mut self) {
        for _ in 0..10_000 {
            if !self.busy.is_high().unwrap_or(false) {
                break;
            }
            self.delay.delay_ms(1);
        }
    }

    fn send_command(&mut self, cmd: u8) {
        let _ = self.dc.set_low();
        for _ in 0..10 {
            core::hint::spin_loop();
        }
        let _ = self.cs.set_low();
        let _ = self.spi.write(&[cmd]);
        let _ = self.spi.flush();
        let _ = self.cs.set_high();
    }

    fn send_data_byte(&mut self, data: u8) {
        let _ = self.dc.set_high();
        for _ in 0..10 {
            core::hint::spin_loop();
        }
        let _ = self.cs.set_low();
        let _ = self.spi.write(&[data]);
        let _ = self.spi.flush();
        let _ = self.cs.set_high();
    }

    fn write_framebuffer(&mut self) {
        let _ = self.dc.set_high();
        for _ in 0..10 {
            core::hint::spin_loop();
        }
        let _ = self.cs.set_low();
        let mut offset = 0;
        while offset < BUFFER_SIZE {
            let end = (offset + 4096).min(BUFFER_SIZE);
            let _ = self.spi.write(&self.framebuffer[offset..end]);
            offset = end;
        }
        let _ = self.spi.flush();
        let _ = self.cs.set_high();
    }
}

pub trait DemoDisplay {
    type Error;

    fn init(&mut self);
    fn clear(&mut self, color: u8);
    fn render_embedded_epub_first_screen(&mut self) -> Result<(), Self::Error>;
    fn refresh_full(&mut self);
}

pub fn show_embedded_epub_demo<D: DemoDisplay>(display: &mut D) -> Result<(), D::Error> {
    display.init();
    display.clear(0xFF);
    display.render_embedded_epub_first_screen()?;
    display.refresh_full();
    Ok(())
}

impl<SPI, CS, DC, RST, BUSY, DELAY> DemoDisplay for SSD1677Display<SPI, CS, DC, RST, BUSY, DELAY>
where
    SPI: SpiBus,
    CS: OutputPin,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
    DELAY: DelayNs,
{
    type Error = xteink_epub::EpubError;

    fn init(&mut self) {
        SSD1677Display::init(self);
    }

    fn clear(&mut self, color: u8) {
        SSD1677Display::clear(self, color);
    }

    fn render_embedded_epub_first_screen(&mut self) -> Result<(), Self::Error> {
        SSD1677Display::render_embedded_epub_first_screen(self)
    }

    fn refresh_full(&mut self) {
        SSD1677Display::refresh_full(self);
    }
}

struct WrappedLine<const N: usize> {
    buf: [u8; N],
    len: usize,
    width: i32,
}

struct TextBuffer<const N: usize> {
    buf: [u8; N],
    len: usize,
}

impl<const N: usize> TextBuffer<N> {
    const fn new() -> Self {
        Self {
            buf: [0; N],
            len: 0,
        }
    }

    fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn clear(&mut self) {
        self.len = 0;
    }

    fn push(&mut self, text: &str) {
        let bytes = text.as_bytes();
        let needs_space = self.len > 0
            && !self.buf[self.len - 1].is_ascii_whitespace()
            && !bytes.first().copied().unwrap_or(b' ').is_ascii_whitespace();

        if needs_space && self.len < self.buf.len() {
            self.buf[self.len] = b' ';
            self.len += 1;
        }

        let remaining = self.buf.len().saturating_sub(self.len);
        let copy_len = core::cmp::min(remaining, bytes.len());
        self.buf[self.len..self.len + copy_len].copy_from_slice(&bytes[..copy_len]);
        self.len += copy_len;
    }

    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }
}

struct EmbeddedEpub;

impl EpubSource for EmbeddedEpub {
    fn len(&self) -> usize {
        EMBEDDED_EPUB_BYTES.len()
    }

    fn read_at(&self, offset: u64, buffer: &mut [u8]) -> Result<usize, xteink_epub::EpubError> {
        let offset = usize::try_from(offset).map_err(|_| xteink_epub::EpubError::InvalidFormat)?;
        if offset >= EMBEDDED_EPUB_BYTES.len() {
            return Ok(0);
        }

        let end = core::cmp::min(EMBEDDED_EPUB_BYTES.len(), offset + buffer.len());
        let len = end - offset;
        buffer[..len].copy_from_slice(&EMBEDDED_EPUB_BYTES[offset..end]);
        Ok(len)
    }
}

impl<const N: usize> WrappedLine<N> {
    const fn new() -> Self {
        Self {
            buf: [0; N],
            len: 0,
            width: 0,
        }
    }

    fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn clear(&mut self) {
        self.len = 0;
        self.width = 0;
    }

    fn push_space(&mut self) {
        if self.len < self.buf.len() {
            self.buf[self.len] = b' ';
            self.len += 1;
            self.width += i32::from(bookerly::BOOKERLY.glyph_for_char(' ').advance_x);
        }
    }

    fn push_str(&mut self, text: &str) {
        let bytes = text.as_bytes();
        let remaining = self.buf.len().saturating_sub(self.len);
        let copy_len = core::cmp::min(remaining, bytes.len());
        self.buf[self.len..self.len + copy_len].copy_from_slice(&bytes[..copy_len]);
        self.len += copy_len;
        self.width += text
            .chars()
            .map(|ch| i32::from(bookerly::BOOKERLY.glyph_for_char(ch).advance_x))
            .sum::<i32>();
    }

    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use super::*;
    use core::convert::Infallible;
    use std::{vec, vec::Vec};

    #[derive(Debug, Default)]
    struct FakeSpi {
        writes: Vec<Vec<u8>>,
        flushes: usize,
    }

    impl embedded_hal::spi::ErrorType for FakeSpi {
        type Error = Infallible;
    }

    impl SpiBus for FakeSpi {
        fn read(&mut self, words: &mut [u8]) -> Result<(), Self::Error> {
            for word in words {
                *word = 0;
            }
            Ok(())
        }

        fn write(&mut self, words: &[u8]) -> Result<(), Self::Error> {
            self.writes.push(words.to_vec());
            Ok(())
        }

        fn transfer(&mut self, read: &mut [u8], write: &[u8]) -> Result<(), Self::Error> {
            let len = read.len().min(write.len());
            read[..len].copy_from_slice(&write[..len]);
            self.writes.push(write.to_vec());
            Ok(())
        }

        fn transfer_in_place(&mut self, words: &mut [u8]) -> Result<(), Self::Error> {
            self.writes.push(words.to_vec());
            Ok(())
        }

        fn flush(&mut self) -> Result<(), Self::Error> {
            self.flushes += 1;
            Ok(())
        }
    }

    #[derive(Debug, Default)]
    struct FakeOutputPin {
        states: Vec<bool>,
    }

    impl embedded_hal::digital::ErrorType for FakeOutputPin {
        type Error = Infallible;
    }

    impl OutputPin for FakeOutputPin {
        fn set_low(&mut self) -> Result<(), Self::Error> {
            self.states.push(false);
            Ok(())
        }

        fn set_high(&mut self) -> Result<(), Self::Error> {
            self.states.push(true);
            Ok(())
        }
    }

    #[derive(Debug, Default)]
    struct FakeInputPin {
        states: Vec<bool>,
        cursor: usize,
    }

    impl embedded_hal::digital::ErrorType for FakeInputPin {
        type Error = Infallible;
    }

    impl InputPin for FakeInputPin {
        fn is_high(&mut self) -> Result<bool, Self::Error> {
            let value = self.states.get(self.cursor).copied().unwrap_or(false);
            if self.cursor < self.states.len() {
                self.cursor += 1;
            }
            Ok(value)
        }

        fn is_low(&mut self) -> Result<bool, Self::Error> {
            self.is_high().map(|value| !value)
        }
    }

    #[derive(Debug, Default)]
    struct FakeDelay {
        ms: Vec<u32>,
        us: Vec<u32>,
        ns: Vec<u32>,
    }

    impl DelayNs for FakeDelay {
        fn delay_ns(&mut self, ns: u32) {
            self.ns.push(ns);
        }

        fn delay_us(&mut self, us: u32) {
            self.us.push(us);
        }

        fn delay_ms(&mut self, ms: u32) {
            self.ms.push(ms);
        }
    }

    #[test]
    fn clear_fills_the_framebuffer() {
        let mut display = new_display();

        display.clear(0x00);

        assert!(display.framebuffer().iter().all(|&byte| byte == 0x00));
    }

    #[test]
    fn set_pixel_uses_the_rotated_physical_buffer_coordinates() {
        let mut display = new_display();

        display.clear(0xFF);
        display.set_pixel(0, 0, true);
        display.set_pixel(479, 799, true);

        let first_idx = logical_pixel_index(0, 0);
        let last_idx = logical_pixel_index(479, 799);
        assert_eq!(display.framebuffer()[first_idx], 0x7F);
        assert_eq!(display.framebuffer()[last_idx], 0xFE);
    }

    #[test]
    fn draw_text_renders_bookerly_glyphs_with_spacing() {
        let mut display = new_display();

        display.clear(0xFF);
        display.draw_text(0, 0, "A");

        assert!(display.framebuffer().iter().any(|&byte| byte != 0xFF));
    }

    #[test]
    fn draw_text_accepts_utf8_text() {
        let mut display = new_display();

        display.clear(0xFF);
        display.draw_text(0, 0, "é");

        assert!(display.framebuffer().iter().any(|&byte| byte != 0xFF));
    }

    #[test]
    fn draw_wrapped_text_wraps_words_and_stops_at_the_requested_height() {
        let mut display = new_display();
        let line_height = bookerly::BOOKERLY.line_height_px();

        display.clear(0xFF);
        let next_y = display.draw_wrapped_text(
            0,
            0,
            "ALPHA BETA GAMMA DELTA EPSILON ZETA ETA THETA IOTA KAPPA ".repeat(20).as_str(),
            line_height * 2,
        );

        assert!(band_has_ink(&display, 0, line_height));
        assert!(band_has_ink(&display, line_height, line_height * 2));
        assert!(!band_has_ink(&display, line_height * 2, line_height * 3));
        assert_eq!(next_y, line_height * 2);
    }

    #[test]
    fn render_embedded_epub_fixture_draws_some_text() {
        let mut display = new_display();

        display.clear(0xFF);
        display.render_embedded_epub_first_screen().unwrap();

        assert!(display.framebuffer().iter().any(|&byte| byte != 0xFF));
    }

    #[test]
    fn embedded_epub_demo_initializes_clears_renders_and_refreshes() {
        let mut display = DemoRecorder::default();

        show_embedded_epub_demo(&mut display).unwrap();

        assert_eq!(
            display.calls.as_slice(),
            &["init", "clear", "epub", "refresh"]
        );
    }

    #[test]
    fn init_performs_the_expected_reset_and_controller_sequence() {
        let mut display = new_display();

        display.init();

        assert_eq!(
            display.spi().writes,
            vec![
                vec![0x12],
                vec![0x18],
                vec![0x80],
                vec![0x0C],
                vec![0xAE],
                vec![0xC7],
                vec![0xC3],
                vec![0xC0],
                vec![0x40],
                vec![0x01],
                vec![0xDF],
                vec![0x01],
                vec![0x02],
                vec![0x3C],
                vec![0x01],
                vec![0x11],
                vec![0x01],
                vec![0x44],
                vec![0x00],
                vec![0x00],
                vec![0x1F],
                vec![0x03],
                vec![0x45],
                vec![0xDF],
                vec![0x01],
                vec![0x00],
                vec![0x00],
                vec![0x4E],
                vec![0x00],
                vec![0x00],
                vec![0x4F],
                vec![0xDF],
                vec![0x01],
            ]
        );
    }

    #[test]
    fn full_refresh_writes_both_rams_and_activates_the_panel() {
        let mut display = new_display();

        display.display_buffer(RefreshMode::Full);

        assert_eq!(
            display
                .spi()
                .writes
                .iter()
                .filter(|write| write.as_slice() == [0x24])
                .count(),
            1
        );
        assert_eq!(
            display
                .spi()
                .writes
                .iter()
                .filter(|write| write.as_slice() == [0x26])
                .count(),
            1
        );
        assert_eq!(
            display
                .spi()
                .writes
                .iter()
                .filter(|write| write.as_slice() == [0x21])
                .count(),
            1
        );
        assert_eq!(
            display
                .spi()
                .writes
                .iter()
                .filter(|write| write.as_slice() == [0x22])
                .count(),
            1
        );
        assert_eq!(
            display
                .spi()
                .writes
                .iter()
                .filter(|write| write.as_slice() == [0x20])
                .count(),
            1
        );
    }

    #[test]
    fn fast_refresh_updates_red_ram_after_refresh() {
        let mut display = new_display();

        display.display_buffer(RefreshMode::Fast);

        assert_eq!(
            display
                .spi()
                .writes
                .iter()
                .filter(|write| write.as_slice() == [0x24])
                .count(),
            1
        );
        assert_eq!(
            display
                .spi()
                .writes
                .iter()
                .filter(|write| write.as_slice() == [0x26])
                .count(),
            1
        );
    }

    #[test]
    fn deep_sleep_powers_down_an_awake_screen() {
        let mut display = new_display();

        display.display_buffer(RefreshMode::Full);
        display.deep_sleep();

        assert!(
            display
                .spi()
                .writes
                .iter()
                .any(|write| write.as_slice() == [0x10])
        );
    }

    fn new_display()
    -> SSD1677Display<FakeSpi, FakeOutputPin, FakeOutputPin, FakeOutputPin, FakeInputPin, FakeDelay>
    {
        SSD1677Display::new(
            FakeSpi::default(),
            FakeOutputPin::default(),
            FakeOutputPin::default(),
            FakeOutputPin::default(),
            FakeInputPin::default(),
            FakeDelay::default(),
        )
    }

    fn logical_pixel_index(x: u16, y: u16) -> usize {
        let physical_y = DISPLAY_WIDTH - 1 - x;
        let physical_x = y;
        (physical_y as usize) * (DISPLAY_WIDTH_BYTES as usize) + (physical_x as usize / 8)
    }

    fn band_has_ink(
        display: &SSD1677Display<FakeSpi, FakeOutputPin, FakeOutputPin, FakeOutputPin, FakeInputPin, FakeDelay>,
        y_start: u16,
        y_end: u16,
    ) -> bool {
        for y in y_start..y_end {
            for x in 0..DISPLAY_WIDTH {
                if display.framebuffer()[logical_pixel_index(x, y)] != 0xFF {
                    return true;
                }
            }
        }

        false
    }

    #[derive(Default)]
    struct DemoRecorder {
        calls: [&'static str; 4],
        len: usize,
    }

    impl DemoDisplay for DemoRecorder {
        type Error = core::convert::Infallible;

        fn init(&mut self) {
            self.calls[self.len] = "init";
            self.len += 1;
        }

        fn clear(&mut self, _color: u8) {
            self.calls[self.len] = "clear";
            self.len += 1;
        }

        fn render_embedded_epub_first_screen(&mut self) -> Result<(), Self::Error> {
            self.calls[self.len] = "epub";
            self.len += 1;
            Ok(())
        }

        fn refresh_full(&mut self) {
            self.calls[self.len] = "refresh";
            self.len += 1;
        }
    }
}
