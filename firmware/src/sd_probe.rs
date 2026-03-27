use core::cell::RefCell;

use embassy_embedded_hal::{SetConfig, shared_bus::blocking::spi::SpiDeviceWithConfig};
use embassy_sync::blocking_mutex::{raw::NoopRawMutex, Mutex};
use embedded_hal::spi::SpiBus;
use embedded_sdmmc::{SdCard, VolumeIdx, VolumeManager, sdcard::AcquireOpts};
use esp_hal::{
    delay::Delay,
    gpio::{AnyPin, Level, Output, OutputConfig},
    spi::master::Config as SpiConfig,
};

use crate::sd_browser::NoopTimeSource;

// Keep clear of strap pins and the display pins.
// GPIO20 is the only remaining non-conflicting candidate in the current wiring set.
pub const SD_CS_PROBE_PINS: &[u8] = &[20];

pub fn select_first_working_pin<T, F>(pins: &[T], mut probe: F) -> Option<T>
where
    T: Copy,
    F: FnMut(T) -> bool,
{
    for &pin in pins {
        if probe(pin) {
            return Some(pin);
        }
    }

    None
}

pub fn probe_sd_cs_pin<SPI>(
    spi_bus: &Mutex<NoopRawMutex, RefCell<SPI>>,
    sd_spi_config: SpiConfig,
) -> Option<u8>
where
    SPI: SpiBus + SetConfig<Config = SpiConfig>,
{
    select_first_working_pin(SD_CS_PROBE_PINS, |pin| probe_single_pin(spi_bus, sd_spi_config, pin))
}

fn probe_single_pin<SPI>(
    spi_bus: &Mutex<NoopRawMutex, RefCell<SPI>>,
    sd_spi_config: SpiConfig,
    pin: u8,
) -> bool
where
    SPI: SpiBus + SetConfig<Config = SpiConfig>,
{
    esp_println::println!("Probing SD card on GPIO{}...", pin);

    let cs = Output::new(
        unsafe { AnyPin::steal(pin) },
        Level::High,
        OutputConfig::default(),
    );
    let sd_spi = SpiDeviceWithConfig::new(spi_bus, cs, sd_spi_config);
    let sd_card = SdCard::new_with_options(
        sd_spi,
        Delay::new(),
        AcquireOpts {
            use_crc: true,
            acquire_retries: 1,
        },
    );
    let volume_mgr = VolumeManager::new(sd_card, NoopTimeSource);

    match volume_mgr.open_volume(VolumeIdx(0)) {
        Ok(_) => {
            esp_println::println!("SD card detected on GPIO{}", pin);
            true
        }
        Err(err) => {
            esp_println::println!("GPIO{} failed: {:?}", pin, err);
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::select_first_working_pin;

    #[test]
    fn returns_first_successful_candidate() {
        let pins = [0u8, 9, 18, 19];

        let selected = select_first_working_pin(&pins, |pin| pin >= 18);

        assert_eq!(selected, Some(18));
    }

    #[test]
    fn returns_none_when_all_candidates_fail() {
        let pins = [0u8, 9, 18, 19];

        let selected = select_first_working_pin(&pins, |_| false);

        assert_eq!(selected, None);
    }
}
