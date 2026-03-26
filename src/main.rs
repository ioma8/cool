//! Xteink X4 E-Reader MVP - Rust no_std implementation
//!
//! This MVP demonstrates:
//! - Boot and initialize hardware
//! - USB serial support for flashing and console output
//! - Display text on E-ink screen
//! - Enter deep sleep mode (without clearing screen)
//! - Wake on any button press
//! - Stay awake for 5 minutes before sleeping again

#![no_std]
#![no_main]

use core::time::Duration;
use esp_backtrace as _;
use esp_hal::{
    delay::Delay,
    gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull},
    main,
    rtc_cntl::{reset_reason, sleep::TimerWakeupSource, wakeup_cause, Rtc},
    spi::master::{Config as SpiConfig, Spi},
    system::Cpu,
    time::{Instant, Rate},
};

mod display;
mod hal;

use display::SSD1677Display;
use hal::{Buttons, WakeupReason};

// For ESP-IDF bootloader support
esp_bootloader_esp_idf::esp_app_desc!();

// Awake timeout: 5 minutes in milliseconds
const AWAKE_TIMEOUT_MS: u64 = 5 * 60 * 1000;

#[main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default());
    
    // Initialize delay early (needed for USB wait)
    let delay = Delay::new();
    
    // Check USB connection status via GPIO20 (UART0_RXD)
    // This pin reads HIGH when USB is connected
    let usb_detect = Input::new(peripherals.GPIO20, InputConfig::default());
    let usb_connected = usb_detect.is_high();
    
    // If USB is connected, wait a moment for serial to be ready
    // This allows capturing early boot messages when flashing/debugging
    if usb_connected {
        delay.delay_millis(500);
    }
    
    esp_println::println!("");
    esp_println::println!("================================");
    esp_println::println!("Xteink X4 Rust MVP - Booting...");
    esp_println::println!("USB Connected: {}", usb_connected);
    esp_println::println!("================================");
    
    // Check reset/wakeup reason
    let reason = reset_reason(Cpu::ProCpu);
    let wake_reason = wakeup_cause();
    esp_println::println!("Reset reason: {:?}", reason);
    esp_println::println!("Wake cause: {:?}", wake_reason);
    
    // Determine the actual wakeup reason (matches C codebase logic)
    let wakeup_reason = hal::get_wakeup_reason(usb_connected);
    esp_println::println!("Wakeup reason: {:?}", wakeup_reason);
    
    // Initialize RTC for sleep management
    let mut rtc = Rtc::new(peripherals.LPWR);
    
    // Handle different wakeup scenarios (matching C codebase behavior)
    match wakeup_reason {
        WakeupReason::AfterUSBPower => {
            // USB power caused cold boot - go back to sleep
            // This prevents the device from staying awake just because USB is plugged in
            esp_println::println!("USB power boot detected, going back to sleep...");
            delay.delay_millis(100);
            let timer = TimerWakeupSource::new(Duration::from_secs(3600));
            rtc.sleep_deep(&[&timer]);
        }
        WakeupReason::AfterFlash => {
            // After flashing, proceed to boot normally
            esp_println::println!("Boot after flash - proceeding normally");
        }
        WakeupReason::PowerButton => {
            esp_println::println!("Power button boot - proceeding normally");
        }
        WakeupReason::Other => {
            esp_println::println!("Other wakeup reason - proceeding normally");
        }
    }
    
    // Initialize SPI for display
    // Pin mapping from C code (HalGPIO.h):
    // EPD_SCLK=8, EPD_MOSI=10, EPD_CS=21, EPD_DC=4, EPD_RST=5, EPD_BUSY=6
    // SPI_MISO=7 (shared with SD card)
    let sclk = peripherals.GPIO8;
    let mosi = peripherals.GPIO10;
    let miso = peripherals.GPIO7;  // MISO needed even for write-only display
    let cs_pin = peripherals.GPIO21;
    let dc_pin = peripherals.GPIO4;
    let rst_pin = peripherals.GPIO5;
    let busy_pin = peripherals.GPIO6;
    
    // Configure control pins (Output::new takes pin, level, config)
    let cs = Output::new(cs_pin, Level::High, OutputConfig::default());
    let dc = Output::new(dc_pin, Level::High, OutputConfig::default());
    let rst = Output::new(rst_pin, Level::High, OutputConfig::default());
    let busy = Input::new(busy_pin, InputConfig::default());
    
    // Initialize SPI bus with MISO (like C code: SPI.begin(EPD_SCLK, SPI_MISO, EPD_MOSI, EPD_CS))
    let spi = Spi::new(
        peripherals.SPI2,
        SpiConfig::default()
            .with_frequency(Rate::from_mhz(40))
            .with_mode(esp_hal::spi::Mode::_0),
    )
    .unwrap()
    .with_sck(sclk)
    .with_mosi(mosi)
    .with_miso(miso);
    
    // Initialize display
    esp_println::println!("Initializing E-ink display...");
    let mut display = SSD1677Display::new(spi, cs, dc, rst, busy, delay);
    display.init();
    
    // Clear screen and draw info text
    esp_println::println!("Drawing to display...");
    display.clear(0xFF); // White
    display.draw_text(10, 10, b"Xteink X4 Rust MVP");
    display.draw_text(10, 30, b"ESP32-C3 no_std");
    display.draw_text(10, 50, b"Press any button to wake");
    display.draw_text(10, 70, b"Sleep after 5 min idle");
    display.refresh_full();
    
    esp_println::println!("Display initialized and content shown!");
    
    // Initialize button inputs for reading
    let adc_pin1 = peripherals.GPIO1;
    let adc_pin2 = peripherals.GPIO2;  
    let power_pin = peripherals.GPIO3;
    
    let mut buttons = Buttons::new(
        Input::new(adc_pin1, InputConfig::default()),
        Input::new(adc_pin2, InputConfig::default()),
        Input::new(power_pin, InputConfig::default().with_pull(Pull::Up)),
    );
    
    esp_println::println!("Entering main loop - will sleep after {} ms of inactivity", AWAKE_TIMEOUT_MS);
    
    let mut last_activity = Instant::now();
    let loop_delay = Delay::new();
    
    loop {
        // Check for button activity
        let button_state = buttons.read_state();
        
        if button_state.any_pressed() {
            esp_println::println!("Button press detected!");
            last_activity = Instant::now();
        }
        
        // Check if we've exceeded the awake timeout
        let elapsed = last_activity.elapsed().as_millis() as u64;
        if elapsed >= AWAKE_TIMEOUT_MS {
            esp_println::println!("Idle timeout reached, entering deep sleep...");
            
            // Put display into deep sleep (keeps content on screen)
            display.deep_sleep();
            
            // Configure GPIO13 for battery latch (must be low during sleep)
            // Note: On ESP32-C3, we need to use a valid GPIO for power latch
            // For this MVP, we skip the battery latch as it requires low-level register access
            
            // Enter deep sleep - wake on timer (as fallback) or power button
            // Note: On this device, power button is hard-wired to provide power on press,
            // so it will wake regardless. Timer is backup.
            let timer = TimerWakeupSource::new(Duration::from_secs(3600)); // 1 hour fallback
            
            esp_println::println!("Sleeping now...");
            loop_delay.delay_millis(100); // Allow print to flush
            
            rtc.sleep_deep(&[&timer]);
            
            // Should never reach here after deep sleep
        }
        
        // Small delay to prevent tight loop
        loop_delay.delay_millis(50);
    }
}

// Unit tests - these run on host with cargo test (not on device)
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_awake_timeout_is_5_minutes() {
        assert_eq!(AWAKE_TIMEOUT_MS, 5 * 60 * 1000);
    }
}

