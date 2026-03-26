#![no_std]
#![no_main]

use core::time::Duration;
use esp_backtrace as _;
use esp_hal::{
    analog::adc::{Adc, AdcConfig, Attenuation},
    delay::Delay,
    gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull},
    main,
    rtc_cntl::{reset_reason, sleep::TimerWakeupSource, wakeup_cause, Rtc, SocResetReason},
    spi::master::{Config as SpiConfig, Spi},
    system::{Cpu, SleepSource},
    time::{Instant, Rate},
};
use xteink_buttons::{get_button_from_adc_1, get_button_from_adc_2, Button, ButtonState};
use xteink_display::SSD1677Display;
use xteink_power::{
    classify_wakeup_reason, should_enter_sleep, ResetReason, WakeCause, WakeupReason,
    AWAKE_TIMEOUT_MS,
};

esp_bootloader_esp_idf::esp_app_desc!();

const MAX_LINES: usize = 24;

#[main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default());

    let delay = Delay::new();
    let usb_detect = Input::new(peripherals.GPIO20, InputConfig::default());
    let usb_connected = usb_detect.is_high();

    if usb_connected {
        delay.delay_millis(500);
    }

    esp_println::println!("");
    esp_println::println!("================================");
    esp_println::println!("Xteink X4 Rust MVP - Booting...");
    esp_println::println!("USB Connected: {}", usb_connected);
    esp_println::println!("================================");

    let reason = reset_reason(Cpu::ProCpu);
    let wake_reason = wakeup_cause();
    esp_println::println!("Reset reason: {:?}", reason);
    esp_println::println!("Wake cause: {:?}", wake_reason);

    let wakeup_reason = classify_wakeup_reason(
        map_wake_cause(wake_reason),
        map_reset_reason(reason),
        usb_connected,
    );
    esp_println::println!("Wakeup reason: {:?}", wakeup_reason);

    let mut rtc = Rtc::new(peripherals.LPWR);

    match wakeup_reason {
        WakeupReason::AfterUSBPower => {
            esp_println::println!("USB power boot detected, going back to sleep...");
            delay.delay_millis(100);
            let timer = TimerWakeupSource::new(Duration::from_secs(3600));
            rtc.sleep_deep(&[&timer]);
        }
        WakeupReason::AfterFlash => {
            esp_println::println!("Boot after flash - proceeding normally");
        }
        WakeupReason::PowerButton => {
            esp_println::println!("Power button boot - proceeding normally");
        }
        WakeupReason::Other => {
            esp_println::println!("Other wakeup reason - proceeding normally");
        }
    }

    let sclk = peripherals.GPIO8;
    let mosi = peripherals.GPIO10;
    let miso = peripherals.GPIO7;
    let cs_pin = peripherals.GPIO21;
    let dc_pin = peripherals.GPIO4;
    let rst_pin = peripherals.GPIO5;
    let busy_pin = peripherals.GPIO6;

    let cs = Output::new(cs_pin, Level::High, OutputConfig::default());
    let dc = Output::new(dc_pin, Level::High, OutputConfig::default());
    let rst = Output::new(rst_pin, Level::High, OutputConfig::default());
    let busy = Input::new(busy_pin, InputConfig::default());

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

    esp_println::println!("Initializing E-ink display...");
    let mut display = SSD1677Display::new(spi, cs, dc, rst, busy, delay);
    display.init();

    esp_println::println!("Drawing to display...");
    display.clear(0xFF);
    display.draw_text(10, 10, b"Xteink X4 Rust MVP");
    display.draw_text(10, 30, b"All 7 buttons mapped via ADC");
    display.draw_text(10, 50, b"Press any button...");
    display.refresh_full();

    esp_println::println!("Display initialized and content shown!");

    let mut adc_config = AdcConfig::new();
    let mut adc_pin1 = adc_config.enable_pin(peripherals.GPIO1, Attenuation::_11dB);
    let mut adc_pin2 = adc_config.enable_pin(peripherals.GPIO2, Attenuation::_11dB);
    let mut adc = Adc::new(peripherals.ADC1, adc_config);

    let power_pin = Input::new(peripherals.GPIO3, InputConfig::default().with_pull(Pull::Up));

    esp_println::println!("Buttons initialized with ADC");
    esp_println::println!(
        "Entering main loop - will sleep after {} ms of inactivity",
        AWAKE_TIMEOUT_MS
    );

    let mut last_activity = Instant::now();
    let loop_delay = Delay::new();
    let mut line_y: u16 = 80;
    let mut press_count: u32 = 0;
    let mut last_state = ButtonState::default();
    let mut debug_counter: u32 = 0;

    loop {
        let mut current_state = ButtonState::default();

        let adc1_value = nb::block!(adc.read_oneshot(&mut adc_pin1)).unwrap_or(9999);
        if let Some(button) = get_button_from_adc_1(adc1_value) {
            current_state = current_state.with_button(button);
        }

        let adc2_value = nb::block!(adc.read_oneshot(&mut adc_pin2)).unwrap_or(9999);
        if let Some(button) = get_button_from_adc_2(adc2_value) {
            current_state = current_state.with_button(button);
        }

        if power_pin.is_low() {
            current_state = current_state.with_button(Button::Power);
        }

        debug_counter += 1;
        if debug_counter % 100 == 0 {
            esp_println::println!(
                "ADC: adc1={}, adc2={}, state=0x{:02X}",
                adc1_value,
                adc2_value,
                current_state.state
            );
        }

        let newly_pressed = ButtonState {
            state: current_state.state & !last_state.state,
        };

        if let Some(button) = newly_pressed.first_pressed() {
            press_count += 1;
            esp_println::println!("Button pressed: {} (count: {})", button.name(), press_count);
            esp_println::println!("  ADC1={}, ADC2={}", adc1_value, adc2_value);

            let mut text_buf = [0u8; 32];
            let text_len = format_button_press(&mut text_buf, press_count, button.name());

            display.draw_text(10, line_y, &text_buf[..text_len]);
            display.refresh_fast();

            line_y += 20;
            if line_y >= (MAX_LINES as u16 * 20) {
                display.clear(0xFF);
                display.draw_text(10, 10, b"Button Press Log (continued)");
                display.refresh_full();
                line_y = 40;
            }

            last_activity = Instant::now();
            loop_delay.delay_millis(150);
        }

        last_state = current_state;

        let elapsed = last_activity.elapsed().as_millis() as u64;
        if should_enter_sleep(elapsed) {
            esp_println::println!("Idle timeout reached, entering deep sleep...");
            display.deep_sleep();

            let timer = TimerWakeupSource::new(Duration::from_secs(3600));

            esp_println::println!("Sleeping now...");
            loop_delay.delay_millis(100);
            rtc.sleep_deep(&[&timer]);
        }

        loop_delay.delay_millis(20);
    }
}

fn map_wake_cause(source: SleepSource) -> WakeCause {
    match source {
        SleepSource::Undefined => WakeCause::Undefined,
        SleepSource::Gpio => WakeCause::Gpio,
        _ => WakeCause::Other,
    }
}

fn map_reset_reason(reason: Option<SocResetReason>) -> Option<ResetReason> {
    match reason {
        Some(SocResetReason::ChipPowerOn) => Some(ResetReason::ChipPowerOn),
        Some(SocResetReason::CoreDeepSleep) => Some(ResetReason::CoreDeepSleep),
        Some(_) => Some(ResetReason::Other),
        None => None,
    }
}

fn format_button_press(buf: &mut [u8], count: u32, name: &str) -> usize {
    let mut pos = 0;
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

    for i in (0..digit_count).rev() {
        if pos < buf.len() {
            buf[pos] = digits[i];
            pos += 1;
        }
    }

    if pos + 2 <= buf.len() {
        buf[pos] = b':';
        buf[pos + 1] = b' ';
        pos += 2;
    }

    for &c in name.as_bytes() {
        if pos < buf.len() {
            buf[pos] = c;
            pos += 1;
        }
    }

    pos
}
