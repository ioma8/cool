use core::convert::Infallible;

use embedded_hal::digital::{ErrorType, OutputPin};
use esp32c3::{GPIO as GpioRegs, IO_MUX as IoMuxRegs};

pub const SD_CS_PIN: u8 = 12;
pub const SD_POWER_PIN: u8 = 13;

pub fn power_on_sd_card() {
    let mut power = RawGpioOutput::new(SD_POWER_PIN, true);
    let _ = power.set_high();
}

pub fn sd_cs_output() -> RawGpioOutput {
    RawGpioOutput::new(SD_CS_PIN, high_idle_level())
}

fn high_idle_level() -> bool {
    true
}

#[derive(Debug, Clone, Copy)]
pub struct RawGpioOutput {
    pin: u8,
}

impl RawGpioOutput {
    pub fn new(pin: u8, initial_high: bool) -> Self {
        configure_gpio_output(pin, initial_high);
        Self { pin }
    }

    fn mask(&self) -> u32 {
        1u32 << self.pin
    }

    fn set_level(&mut self, high: bool) {
        let gpio = unsafe { GpioRegs::steal() };
        if high {
            gpio.out_w1ts().write(|w| unsafe { w.bits(self.mask()) });
        } else {
            gpio.out_w1tc().write(|w| unsafe { w.bits(self.mask()) });
        }
    }
}

impl ErrorType for RawGpioOutput {
    type Error = Infallible;
}

impl OutputPin for RawGpioOutput {
    fn set_low(&mut self) -> Result<(), Self::Error> {
        self.set_level(false);
        Ok(())
    }

    fn set_high(&mut self) -> Result<(), Self::Error> {
        self.set_level(true);
        Ok(())
    }
}

fn configure_gpio_output(pin: u8, initial_high: bool) {
    let gpio = unsafe { GpioRegs::steal() };
    let io_mux = unsafe { IoMuxRegs::steal() };
    let mask = 1u32 << pin;

    io_mux.gpio(pin as usize).modify(|_, w| unsafe {
        w.mcu_sel().bits(1);
        w.fun_ie().clear_bit();
        w.fun_wpd().clear_bit();
        w.fun_wpu().clear_bit();
        w.slp_sel().clear_bit()
    });

    if initial_high {
        gpio.out_w1ts().write(|w| unsafe { w.bits(mask) });
    } else {
        gpio.out_w1tc().write(|w| unsafe { w.bits(mask) });
    }
    gpio.enable_w1ts().write(|w| unsafe { w.bits(mask) });
}
