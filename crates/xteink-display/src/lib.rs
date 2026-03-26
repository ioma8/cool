#![no_std]

use embedded_hal::{
    delay::DelayNs,
    digital::{InputPin, OutputPin},
    spi::SpiBus,
};

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

const FONT_5X7: [[u8; 5]; 95] = [
    [0x00, 0x00, 0x00, 0x00, 0x00], [0x00, 0x00, 0x5F, 0x00, 0x00], [0x00, 0x07, 0x00, 0x07, 0x00],
    [0x14, 0x7F, 0x14, 0x7F, 0x14], [0x24, 0x2A, 0x7F, 0x2A, 0x12], [0x23, 0x13, 0x08, 0x64, 0x62],
    [0x36, 0x49, 0x55, 0x22, 0x50], [0x00, 0x05, 0x03, 0x00, 0x00], [0x00, 0x1C, 0x22, 0x41, 0x00],
    [0x00, 0x41, 0x22, 0x1C, 0x00], [0x08, 0x2A, 0x1C, 0x2A, 0x08], [0x08, 0x08, 0x3E, 0x08, 0x08],
    [0x00, 0x50, 0x30, 0x00, 0x00], [0x08, 0x08, 0x08, 0x08, 0x08], [0x00, 0x60, 0x60, 0x00, 0x00],
    [0x20, 0x10, 0x08, 0x04, 0x02], [0x3E, 0x51, 0x49, 0x45, 0x3E], [0x00, 0x42, 0x7F, 0x40, 0x00],
    [0x42, 0x61, 0x51, 0x49, 0x46], [0x21, 0x41, 0x45, 0x4B, 0x31], [0x18, 0x14, 0x12, 0x7F, 0x10],
    [0x27, 0x45, 0x45, 0x45, 0x39], [0x3C, 0x4A, 0x49, 0x49, 0x30], [0x01, 0x71, 0x09, 0x05, 0x03],
    [0x36, 0x49, 0x49, 0x49, 0x36], [0x06, 0x49, 0x49, 0x29, 0x1E], [0x00, 0x36, 0x36, 0x00, 0x00],
    [0x00, 0x56, 0x36, 0x00, 0x00], [0x08, 0x14, 0x22, 0x41, 0x00], [0x14, 0x14, 0x14, 0x14, 0x14],
    [0x00, 0x41, 0x22, 0x14, 0x08], [0x02, 0x01, 0x51, 0x09, 0x06], [0x32, 0x49, 0x79, 0x41, 0x3E],
    [0x7E, 0x11, 0x11, 0x11, 0x7E], [0x7F, 0x49, 0x49, 0x49, 0x36], [0x3E, 0x41, 0x41, 0x41, 0x22],
    [0x7F, 0x41, 0x41, 0x22, 0x1C], [0x7F, 0x49, 0x49, 0x49, 0x41], [0x7F, 0x09, 0x09, 0x09, 0x01],
    [0x3E, 0x41, 0x49, 0x49, 0x7A], [0x7F, 0x08, 0x08, 0x08, 0x7F], [0x00, 0x41, 0x7F, 0x41, 0x00],
    [0x20, 0x40, 0x41, 0x3F, 0x01], [0x7F, 0x08, 0x14, 0x22, 0x41], [0x7F, 0x40, 0x40, 0x40, 0x40],
    [0x7F, 0x02, 0x0C, 0x02, 0x7F], [0x7F, 0x04, 0x08, 0x10, 0x7F], [0x3E, 0x41, 0x41, 0x41, 0x3E],
    [0x7F, 0x09, 0x09, 0x09, 0x06], [0x3E, 0x41, 0x51, 0x21, 0x5E], [0x7F, 0x09, 0x19, 0x29, 0x46],
    [0x46, 0x49, 0x49, 0x49, 0x31], [0x01, 0x01, 0x7F, 0x01, 0x01], [0x3F, 0x40, 0x40, 0x40, 0x3F],
    [0x1F, 0x20, 0x40, 0x20, 0x1F], [0x3F, 0x40, 0x38, 0x40, 0x3F], [0x63, 0x14, 0x08, 0x14, 0x63],
    [0x07, 0x08, 0x70, 0x08, 0x07], [0x61, 0x51, 0x49, 0x45, 0x43], [0x00, 0x7F, 0x41, 0x41, 0x00],
    [0x02, 0x04, 0x08, 0x10, 0x20], [0x00, 0x41, 0x41, 0x7F, 0x00], [0x04, 0x02, 0x01, 0x02, 0x04],
    [0x40, 0x40, 0x40, 0x40, 0x40], [0x00, 0x01, 0x02, 0x04, 0x00], [0x20, 0x54, 0x54, 0x54, 0x78],
    [0x7F, 0x48, 0x44, 0x44, 0x38], [0x38, 0x44, 0x44, 0x44, 0x20], [0x38, 0x44, 0x44, 0x48, 0x7F],
    [0x38, 0x54, 0x54, 0x54, 0x18], [0x08, 0x7E, 0x09, 0x01, 0x02], [0x0C, 0x52, 0x52, 0x52, 0x3E],
    [0x7F, 0x08, 0x04, 0x04, 0x78], [0x00, 0x44, 0x7D, 0x40, 0x00], [0x20, 0x40, 0x44, 0x3D, 0x00],
    [0x7F, 0x10, 0x28, 0x44, 0x00], [0x00, 0x41, 0x7F, 0x40, 0x00], [0x7C, 0x04, 0x18, 0x04, 0x78],
    [0x7C, 0x08, 0x04, 0x04, 0x78], [0x38, 0x44, 0x44, 0x44, 0x38], [0x7C, 0x14, 0x14, 0x14, 0x08],
    [0x08, 0x14, 0x14, 0x18, 0x7C], [0x7C, 0x08, 0x04, 0x04, 0x08], [0x48, 0x54, 0x54, 0x54, 0x20],
    [0x04, 0x3F, 0x44, 0x40, 0x20], [0x3C, 0x40, 0x40, 0x20, 0x7C], [0x1C, 0x20, 0x40, 0x20, 0x1C],
    [0x3C, 0x40, 0x30, 0x40, 0x3C], [0x44, 0x28, 0x10, 0x28, 0x44], [0x0C, 0x50, 0x50, 0x50, 0x3C],
    [0x44, 0x64, 0x54, 0x4C, 0x44], [0x00, 0x08, 0x36, 0x41, 0x00], [0x00, 0x00, 0x7F, 0x00, 0x00],
    [0x00, 0x41, 0x36, 0x08, 0x00], [0x10, 0x08, 0x08, 0x10, 0x08],
];

fn get_char_bitmap(c: u8) -> [u8; 5] {
    if (32..127).contains(&c) {
        FONT_5X7[(c - 32) as usize]
    } else {
        FONT_5X7[0]
    }
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

    pub fn draw_text(&mut self, x: u16, y: u16, text: &[u8]) {
        let mut cx = x;
        for &c in text {
            if cx + 6 > DISPLAY_WIDTH {
                break;
            }
            let bmp = get_char_bitmap(c);
            for (col, bits) in bmp.iter().enumerate() {
                for row in 0..7 {
                    if (bits >> row) & 1 == 1 {
                        self.set_pixel(cx + col as u16, y + row as u16, true);
                    }
                }
            }
            cx += 6;
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
    fn draw_text_renders_5x7_glyphs_with_spacing() {
        let mut display = new_display();

        display.clear(0xFF);
        display.draw_text(0, 0, b"A");

        let idx = logical_pixel_index(0, 1);
        assert_eq!(display.framebuffer()[idx] & (1 << 6), 0);
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
            display.spi().writes.iter().filter(|write| write.as_slice() == [0x24]).count(),
            1
        );
        assert_eq!(
            display.spi().writes.iter().filter(|write| write.as_slice() == [0x26]).count(),
            1
        );
        assert_eq!(
            display.spi().writes.iter().filter(|write| write.as_slice() == [0x21]).count(),
            1
        );
        assert_eq!(
            display.spi().writes.iter().filter(|write| write.as_slice() == [0x22]).count(),
            1
        );
        assert_eq!(
            display.spi().writes.iter().filter(|write| write.as_slice() == [0x20]).count(),
            1
        );
    }

    #[test]
    fn fast_refresh_updates_red_ram_after_refresh() {
        let mut display = new_display();

        display.display_buffer(RefreshMode::Fast);

        assert_eq!(
            display.spi().writes.iter().filter(|write| write.as_slice() == [0x24]).count(),
            1
        );
        assert_eq!(
            display.spi().writes.iter().filter(|write| write.as_slice() == [0x26]).count(),
            1
        );
    }

    #[test]
    fn deep_sleep_powers_down_an_awake_screen() {
        let mut display = new_display();

        display.display_buffer(RefreshMode::Full);
        display.deep_sleep();

        assert!(display.spi().writes.iter().any(|write| write.as_slice() == [0x10]));
    }

    fn new_display(
    ) -> SSD1677Display<FakeSpi, FakeOutputPin, FakeOutputPin, FakeOutputPin, FakeInputPin, FakeDelay> {
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
