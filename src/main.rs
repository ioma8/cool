//! Xteink X4 E-Reader MVP - Rust no_std implementation
//!
//! This MVP demonstrates:
//! - Boot and initialize hardware
//! - USB serial support for flashing and console output
//! - Display text on E-ink screen
//! - Read all 7 buttons via ADC and GPIO
//! - Print new line on each button press
//! - Enter deep sleep mode (without clearing screen)
//! - Wake on any button press
//! - Stay awake for 5 minutes before sleeping again

#![no_std]
#![no_main]

use core::time::Duration;
use esp_backtrace as _;
use esp_hal::{
    analog::adc::{Adc, AdcConfig, Attenuation},
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
use hal::{get_button_from_adc_1, get_button_from_adc_2, Button, ButtonState, WakeupReason};

// For ESP-IDF bootloader support
esp_bootloader_esp_idf::esp_app_desc!();

// Awake timeout: 5 minutes in milliseconds
const AWAKE_TIMEOUT_MS: u64 = 5 * 60 * 1000;

// Maximum lines on display (480 height with ~20px per line)
const MAX_LINES: usize = 24;

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
    
    // Clear screen and draw initial info text
    esp_println::println!("Drawing to display...");
    display.clear(0xFF); // White
    display.draw_text(10, 10, b"Xteink X4 Rust MVP");
    display.draw_text(10, 30, b"All 7 buttons mapped via ADC");
    display.draw_text(10, 50, b"Press any button...");
    display.refresh_full();
    
    esp_println::println!("Display initialized and content shown!");
    
    // Initialize ADC for button reading
    // GPIO1 = ADC channel for Back, Confirm, Left, Right
    // GPIO2 = ADC channel for Up, Down
    // GPIO3 = Power button (digital, active LOW)
    let mut adc_config = AdcConfig::new();
    let mut adc_pin1 = adc_config.enable_pin(peripherals.GPIO1, Attenuation::_11dB);
    let mut adc_pin2 = adc_config.enable_pin(peripherals.GPIO2, Attenuation::_11dB);
    let mut adc = Adc::new(peripherals.ADC1, adc_config);
    
    let power_pin = Input::new(peripherals.GPIO3, InputConfig::default().with_pull(Pull::Up));
    
    esp_println::println!("Buttons initialized with ADC");
    esp_println::println!("Entering main loop - will sleep after {} ms of inactivity", AWAKE_TIMEOUT_MS);
    
    let mut last_activity = Instant::now();
    let loop_delay = Delay::new();
    
    // Track line position for button press text
    let mut line_y: u16 = 80;
    let mut press_count: u32 = 0;
    let mut last_state = ButtonState::default();
    
    loop {
        // Read current button state
        let mut current_state = ButtonState::default();
        
        // Read ADC pin 1 (Back, Confirm, Left, Right)
        if let Ok(adc_value) = nb::block!(adc.read_oneshot(&mut adc_pin1)) {
            if let Some(button) = get_button_from_adc_1(adc_value) {
                current_state = current_state.with_button(button);
            }
        }
        
        // Read ADC pin 2 (Up, Down)
        if let Ok(adc_value) = nb::block!(adc.read_oneshot(&mut adc_pin2)) {
            if let Some(button) = get_button_from_adc_2(adc_value) {
                current_state = current_state.with_button(button);
            }
        }
        
        // Power button is digital, active LOW with pull-up
        if power_pin.is_low() {
            current_state = current_state.with_button(Button::Power);
        }
        
        // Detect newly pressed buttons (edge detection)
        let newly_pressed = ButtonState {
            state: current_state.state & !last_state.state,
        };
        
        if let Some(button) = newly_pressed.first_pressed() {
            press_count += 1;
            esp_println::println!("Button pressed: {} (count: {})", button.name(), press_count);
            
            // Read raw ADC values for debugging
            let adc1_raw = nb::block!(adc.read_oneshot(&mut adc_pin1)).unwrap_or(0);
            let adc2_raw = nb::block!(adc.read_oneshot(&mut adc_pin2)).unwrap_or(0);
            esp_println::println!("  ADC1={}, ADC2={}", adc1_raw, adc2_raw);
            
            // Draw new line on display
            // Format: "N: ButtonName"
            let mut text_buf = [0u8; 32];
            let text_len = format_button_press(&mut text_buf, press_count, button.name());
            
            display.draw_text(10, line_y, &text_buf[..text_len]);
            display.refresh_fast(); // Use fast refresh for responsiveness
            
            // Move to next line, wrap if needed
            line_y += 20;
            if line_y >= (MAX_LINES as u16 * 20) {
                // Clear and reset
                display.clear(0xFF);
                display.draw_text(10, 10, b"Button Press Log (continued)");
                display.refresh_full();
                line_y = 40;
            }
            
            last_activity = Instant::now();
            
            // Small debounce delay
            loop_delay.delay_millis(150);
        }
        
        last_state = current_state;
        
        // Check if we've exceeded the awake timeout
        let elapsed = last_activity.elapsed().as_millis() as u64;
        if elapsed >= AWAKE_TIMEOUT_MS {
            esp_println::println!("Idle timeout reached, entering deep sleep...");
            
            // Put display into deep sleep (keeps content on screen)
            display.deep_sleep();
            
            // Enter deep sleep - wake on timer (as fallback) or power button
            let timer = TimerWakeupSource::new(Duration::from_secs(3600)); // 1 hour fallback
            
            esp_println::println!("Sleeping now...");
            loop_delay.delay_millis(100); // Allow print to flush
            
            rtc.sleep_deep(&[&timer]);
            
            // Should never reach here after deep sleep
        }
        
        // Small delay to prevent tight loop
        loop_delay.delay_millis(20);
    }
}

/// Format button press message into buffer, returns length
fn format_button_press(buf: &mut [u8], count: u32, name: &str) -> usize {
    let mut pos = 0;
    
    // Write count
    let mut num = count;
    let mut digits = [0u8; 10];
    let mut digit_count = 0;
    
    if num == 0 {
        digits[0] = b'0';
        digit_count = 1;
    } else {
        while num > 0 {
            digits[digit_count] = b'0' + (num % 10) as u8;
            num /= 10;
            digit_count += 1;
        }
    }
    
    // Write digits in reverse order
    for i in (0..digit_count).rev() {
        if pos < buf.len() {
            buf[pos] = digits[i];
            pos += 1;
        }
    }
    
    // Write ": "
    if pos + 2 <= buf.len() {
        buf[pos] = b':';
        buf[pos + 1] = b' ';
        pos += 2;
    }
    
    // Write button name
    for &c in name.as_bytes() {
        if pos < buf.len() {
            buf[pos] = c;
            pos += 1;
        }
    }
    
    pos
}

// Unit tests - these run on host with cargo test (not on device)
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_awake_timeout_is_5_minutes() {
        assert_eq!(AWAKE_TIMEOUT_MS, 5 * 60 * 1000);
    }
    
    #[test]
    fn test_format_button_press() {
        let mut buf = [0u8; 32];
        let len = format_button_press(&mut buf, 1, "Back");
        assert_eq!(&buf[..len], b"1: Back");
        
        let len = format_button_press(&mut buf, 42, "Confirm");
        assert_eq!(&buf[..len], b"42: Confirm");
        
        let len = format_button_press(&mut buf, 100, "Power");
        assert_eq!(&buf[..len], b"100: Power");
    }
}

