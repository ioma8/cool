#![no_std]

use embedded_hal::{
    delay::DelayNs,
    digital::{InputPin, OutputPin},
    spi::SpiBus,
};

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
}
