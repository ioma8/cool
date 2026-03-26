#![no_std]
#![no_main]

use esp_backtrace as _;
use esp_hal::{
    analog::adc::{Adc, AdcConfig, Attenuation},
    clock::CpuClock,
    delay::Delay,
    gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull},
    main,
    rtc_cntl::{SocResetReason, reset_reason, wakeup_cause},
    spi::master::{Config as SpiConfig, Spi},
    system::{Cpu, SleepSource},
    time::Instant,
    time::Rate,
};
use xteink_buttons::{Button, ButtonState, get_button_from_adc_1, get_button_from_adc_2};
use xteink_display::SSD1677Display;
use xteink_power::{ResetReason, WakeCause, WakeupReason, classify_wakeup_reason};

esp_bootloader_esp_idf::esp_app_desc!();

const DEBOUNCE_DELAY_MS: u64 = 5;
const BUTTON_LABEL_HEADER: &str = "Buttons:";

#[main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));

    let delay = Delay::new();
    let usb_detect = Input::new(
        peripherals.GPIO20,
        InputConfig::default().with_pull(Pull::None),
    );
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

    match wakeup_reason {
        WakeupReason::AfterUSBPower => {
            esp_println::println!("USB power boot detected - continuing without deep sleep");
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

    let spi = Spi::new(
        peripherals.SPI2,
        SpiConfig::default()
            .with_frequency(Rate::from_mhz(40))
            .with_mode(esp_hal::spi::Mode::_0),
    )
    .unwrap()
    .with_sck(peripherals.GPIO8)
    .with_mosi(peripherals.GPIO10)
    .with_miso(peripherals.GPIO7);

    let display_cs = Output::new(peripherals.GPIO21, Level::High, OutputConfig::default());
    let display_dc = Output::new(peripherals.GPIO4, Level::High, OutputConfig::default());
    let display_rst = Output::new(peripherals.GPIO5, Level::High, OutputConfig::default());
    let display_busy = Input::new(peripherals.GPIO6, InputConfig::default());

    let mut display = SSD1677Display::new(
        spi,
        display_cs,
        display_dc,
        display_rst,
        display_busy,
        delay,
    );

    esp_println::println!("Initializing E-ink display with embedded EPUB demo...");
    if let Err(err) = xteink_display::show_embedded_epub_demo(&mut display) {
        esp_println::println!("EPUB demo render failed: {:?}", err);
        display.init();
        display.clear(0xFF);
        display.draw_text(4, 4, BUTTON_LABEL_HEADER);
        display.refresh_full();
    }
    esp_println::println!("Display initialized and EPUB content shown");

    let mut adc_config = AdcConfig::new();
    let mut adc_pin1 = adc_config.enable_pin(peripherals.GPIO1, Attenuation::_11dB);
    let mut adc_pin2 = adc_config.enable_pin(peripherals.GPIO2, Attenuation::_11dB);
    let mut adc = Adc::new(peripherals.ADC1, adc_config);
    let power_button = Input::new(
        peripherals.GPIO3,
        InputConfig::default().with_pull(Pull::Up),
    );
    let line_height = xteink_display::bookerly::BOOKERLY.line_height_px();
    let mut display_row = 4u16 + line_height;

    esp_println::println!("Buttons initialized with ADC");
    esp_println::println!("Entering main loop");

    let loop_delay = Delay::new();
    let mut current_state = ButtonState::default();
    let mut last_state = ButtonState::default();
    let mut last_debounce_time = Instant::now();

    loop {
        let mut raw_state = ButtonState::default();
        let adc1_value =
            nb::block!(adc.read_oneshot(&mut adc_pin1)).unwrap_or(xteink_buttons::ADC_MAX);
        if let Some(button) = get_button_from_adc_1(adc1_value) {
            raw_state = raw_state.with_button(button);
        }

        let adc2_value =
            nb::block!(adc.read_oneshot(&mut adc_pin2)).unwrap_or(xteink_buttons::ADC_MAX);
        if let Some(button) = get_button_from_adc_2(adc2_value) {
            raw_state = raw_state.with_button(button);
        }
        let power_pressed = power_button.is_low();
        if power_pressed {
            raw_state = raw_state.with_button(Button::Power);
        }

        if raw_state.state != last_state.state {
            last_state = raw_state;
            last_debounce_time = Instant::now();
        }

        let mut pressed_events = ButtonState::default();
        if last_debounce_time.elapsed().as_millis() >= DEBOUNCE_DELAY_MS
            && raw_state.state != current_state.state
        {
            pressed_events = ButtonState {
                state: raw_state.state & !current_state.state,
            };
            current_state = raw_state;
        }

        const ORDERED_BUTTONS: [Button; 7] = [
            Button::Back,
            Button::Confirm,
            Button::Left,
            Button::Right,
            Button::Up,
            Button::Down,
            Button::Power,
        ];
        if pressed_events.any_pressed() {
            for button in ORDERED_BUTTONS {
                if pressed_events.is_pressed(button) {
                    esp_println::println!("Button pressed: {}", button.name());
                    if display_row + line_height >= xteink_display::DISPLAY_HEIGHT {
                        display.clear(0xFF);
                        display.draw_text(4, 4, BUTTON_LABEL_HEADER);
                        display_row = 4 + line_height;
                    }
                    display.draw_text(4, display_row, button.name());
                    display.refresh_fast();
                    display_row += line_height;
                }
            }
        }

        loop_delay.delay_millis(1);
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

// keep type names for any future debug extension
#[allow(dead_code)]
fn button_label(button: Button) -> &'static str {
    button.name()
}
