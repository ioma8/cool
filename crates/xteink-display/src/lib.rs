#![no_std]

use embedded_hal::{
    delay::DelayNs,
    digital::{InputPin, OutputPin},
    spi::SpiDevice,
};

pub use xteink_render::{BUFFER_SIZE, DISPLAY_HEIGHT, DISPLAY_WIDTH, DISPLAY_WIDTH_BYTES};

const PHYSICAL_WIDTH: u16 = 800;
const PHYSICAL_HEIGHT: u16 = 480;

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
const DISPLAY_BUSY_TIMEOUT_MS: u16 = 250;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshMode {
    Full,
    Half,
    Fast,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshScheduleError {
    Busy,
}

pub struct SSD1677Display<SPI, DC, RST, BUSY, DELAY>
where
    SPI: SpiDevice,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
    DELAY: DelayNs,
{
    spi: SPI,
    dc: DC,
    rst: RST,
    busy: BUSY,
    delay: DELAY,
    is_screen_on: bool,
}

impl<SPI, DC, RST, BUSY, DELAY> SSD1677Display<SPI, DC, RST, BUSY, DELAY>
where
    SPI: SpiDevice,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
    DELAY: DelayNs,
{
    pub fn new(spi: SPI, dc: DC, rst: RST, busy: BUSY, delay: DELAY) -> Self {
        Self {
            spi,
            dc,
            rst,
            busy,
            delay,
            is_screen_on: false,
        }
    }

    pub fn init(&mut self) {
        self.reset_display();
        self.init_display_controller();
    }

    pub fn spi(&self) -> &SPI {
        &self.spi
    }

    pub fn display_buffer(
        &mut self,
        framebuffer: &[u8; BUFFER_SIZE],
        mode: RefreshMode,
        wait_for_ready: bool,
    ) -> Result<(), RefreshScheduleError> {
        if !wait_for_ready && self.is_busy() {
            return Err(RefreshScheduleError::Busy);
        }

        self.set_ram_area(0, 0, PHYSICAL_WIDTH, PHYSICAL_HEIGHT);

        if mode != RefreshMode::Fast {
            self.send_command(CMD_WRITE_RAM_BW);
            self.write_framebuffer(framebuffer);
            self.send_command(CMD_WRITE_RAM_RED);
            self.write_framebuffer(framebuffer);
        } else {
            self.send_command(CMD_WRITE_RAM_BW);
            self.write_framebuffer(framebuffer);
        }

        self.refresh_display(mode, false);

        if mode == RefreshMode::Fast {
            self.set_ram_area(0, 0, PHYSICAL_WIDTH, PHYSICAL_HEIGHT);
            self.send_command(CMD_WRITE_RAM_RED);
            self.write_framebuffer(framebuffer);
        }

        if wait_for_ready {
            self.wait_while_busy();
        }

        Ok(())
    }

    pub fn refresh_full(&mut self, framebuffer: &[u8; BUFFER_SIZE]) {
        let _ = self.display_buffer(framebuffer, RefreshMode::Full, true);
    }

    pub fn refresh_fast(&mut self, framebuffer: &[u8; BUFFER_SIZE]) {
        let _ = self.display_buffer(framebuffer, RefreshMode::Fast, true);
    }

    pub fn refresh_full_nonblocking(
        &mut self,
        framebuffer: &[u8; BUFFER_SIZE],
    ) -> Result<(), RefreshScheduleError> {
        self.display_buffer(framebuffer, RefreshMode::Full, false)
    }

    pub fn refresh_fast_nonblocking(
        &mut self,
        framebuffer: &[u8; BUFFER_SIZE],
    ) -> Result<(), RefreshScheduleError> {
        self.display_buffer(framebuffer, RefreshMode::Fast, false)
    }

    pub fn is_busy(&mut self) -> bool {
        self.busy.is_high().unwrap_or(false)
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
        for _ in 0..DISPLAY_BUSY_TIMEOUT_MS {
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
        let _ = self.spi.write(&[cmd]);
    }

    fn send_data_byte(&mut self, data: u8) {
        let _ = self.dc.set_high();
        for _ in 0..10 {
            core::hint::spin_loop();
        }
        let _ = self.spi.write(&[data]);
    }

    fn write_framebuffer(&mut self, framebuffer: &[u8; BUFFER_SIZE]) {
        let _ = self.dc.set_high();
        for _ in 0..10 {
            core::hint::spin_loop();
        }
        let mut offset = 0;
        while offset < BUFFER_SIZE {
            let end = (offset + 4096).min(BUFFER_SIZE);
            let _ = self.spi.write(&framebuffer[offset..end]);
            offset = end;
        }
    }
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use super::*;
    use core::convert::Infallible;
    use embedded_hal::spi::Operation;
    use std::vec;
    use std::vec::Vec;

    #[derive(Debug, Default)]
    struct FakeSpi {
        writes: Vec<Vec<u8>>,
    }

    impl embedded_hal::spi::ErrorType for FakeSpi {
        type Error = Infallible;
    }

    impl SpiDevice for FakeSpi {
        fn transaction(&mut self, operations: &mut [Operation<'_, u8>]) -> Result<(), Self::Error> {
            for operation in operations {
                match operation {
                    Operation::Read(words) => {
                        for word in words.iter_mut() {
                            *word = 0;
                        }
                    }
                    Operation::Write(words) => self.writes.push(words.to_vec()),
                    Operation::Transfer(read, write) => {
                        let len = read.len().min(write.len());
                        read[..len].copy_from_slice(&write[..len]);
                        self.writes.push(write.to_vec());
                    }
                    Operation::TransferInPlace(words) => self.writes.push(words.to_vec()),
                    Operation::DelayNs(_) => {}
                }
            }
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
    }

    impl DelayNs for FakeDelay {
        fn delay_ns(&mut self, _ns: u32) {}
        fn delay_us(&mut self, _us: u32) {}
        fn delay_ms(&mut self, ms: u32) {
            self.ms.push(ms);
        }
    }

    #[test]
    fn refresh_full_streams_borrowed_framebuffer_bytes() {
        let mut display = new_display();
        let mut framebuffer = [0xFF; BUFFER_SIZE];
        framebuffer[0] = 0x00;
        framebuffer[BUFFER_SIZE - 1] = 0xAA;

        display.refresh_full(&framebuffer);

        assert!(display
            .spi()
            .writes
            .iter()
            .any(|chunk| chunk.first() == Some(&0x00)));
        assert!(display
            .spi()
            .writes
            .iter()
            .any(|chunk| chunk.last() == Some(&0xAA)));
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

    fn new_display() -> SSD1677Display<FakeSpi, FakeOutputPin, FakeOutputPin, FakeInputPin, FakeDelay> {
        SSD1677Display::new(
            FakeSpi::default(),
            FakeOutputPin::default(),
            FakeOutputPin::default(),
            FakeInputPin::default(),
            FakeDelay::default(),
        )
    }
}
