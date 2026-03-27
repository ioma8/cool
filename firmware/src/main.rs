#![no_std]
#![no_main]

use core::cell::RefCell;
use embassy_embedded_hal::shared_bus::blocking::spi::SpiDeviceWithConfig;
use embassy_sync::blocking_mutex::{Mutex, raw::NoopRawMutex};
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
use heapless::String;
use xteink_browser::{Input as BrowserInput, PagedAction, PagedBrowser};
use xteink_buttons::{Button, ButtonState, get_button_from_adc_1, get_button_from_adc_2};
use xteink_display::{DISPLAY_HEIGHT, SSD1677Display, bookerly};
use xteink_power::{ResetReason, WakeCause, classify_wakeup_reason};

use embedded_hal::spi::{SpiBus, SpiDevice};

mod sd_hw;
mod sd_browser;
mod sd_path;
mod sd_ffi;

use sd_browser::{ListedEntry, load_directory_page, render_epub_from_entry, render_epub_page_from_entry};
use sd_ffi::init_sd;
use sd_path::join_child_path;

esp_bootloader_esp_idf::esp_app_desc!();

const DEBOUNCE_DELAY_MS: u64 = 5;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScreenMode {
    Browse,
    Reading,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BrowserRefresh {
    Full,
    Fast,
}

trait BrowserScreenDisplay {
    fn clear(&mut self, color: u8);
    fn draw_text(&mut self, x: u16, y: u16, text: &str);
    fn refresh_fast(&mut self);
    fn refresh_full(&mut self);
}

impl<SPI, DC, RST, BUSY, DELAY> BrowserScreenDisplay for SSD1677Display<SPI, DC, RST, BUSY, DELAY>
where
    SPI: SpiDevice,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin,
    BUSY: embedded_hal::digital::InputPin,
    DELAY: embedded_hal::delay::DelayNs,
{
    fn clear(&mut self, color: u8) {
        SSD1677Display::clear(self, color);
    }

    fn draw_text(&mut self, x: u16, y: u16, text: &str) {
        SSD1677Display::draw_text(self, x, y, text);
    }

    fn refresh_fast(&mut self) {
        SSD1677Display::refresh_fast(self);
    }

    fn refresh_full(&mut self) {
        SSD1677Display::refresh_full(self);
    }
}

#[main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));

    let boot_delay = Delay::new();
    let usb_detect = Input::new(
        peripherals.GPIO20,
        InputConfig::default().with_pull(Pull::None),
    );
    let usb_connected = usb_detect.is_high();

    if usb_connected {
        boot_delay.delay_millis(500);
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

    let display_spi_config = SpiConfig::default()
        .with_frequency(Rate::from_mhz(40))
        .with_mode(esp_hal::spi::Mode::_0);

    let mut spi = Spi::new(peripherals.SPI2, display_spi_config)
    .unwrap()
    .with_sck(peripherals.GPIO8)
    .with_mosi(peripherals.GPIO10)
    .with_miso(peripherals.GPIO7);

    let display_cs = Output::new(peripherals.GPIO21, Level::High, OutputConfig::default());
    let display_dc = Output::new(peripherals.GPIO4, Level::High, OutputConfig::default());
    let display_rst = Output::new(peripherals.GPIO5, Level::High, OutputConfig::default());
    let display_busy = Input::new(peripherals.GPIO6, InputConfig::default());

    let _ = spi.write(&[0xFF; 10]);
    let _ = spi.flush();

    let spi_bus = Mutex::<NoopRawMutex, _>::new(RefCell::new(spi));
    let sd = match init_sd(&spi_bus, display_spi_config, Delay::new()) {
        Ok(sd) => Some(sd),
        Err(err) => {
            esp_println::println!("SD init failed: {:?}", err);
            None
        }
    };
    let display_spi = SpiDeviceWithConfig::new(&spi_bus, display_cs, display_spi_config);
    let display_delay = Delay::new();
    let mut display = SSD1677Display::new(
        display_spi,
        display_dc,
        display_rst,
        display_busy,
        display_delay,
    );

    esp_println::println!("Initializing display and SD browser...");
    let mut current_path: String<256> = String::new();
    let _ = current_path.push('/');
    let page_size = browser_page_size();

    let Some(sd) = sd else {
        display.init();
        render_error_screen(&mut display, "SD init failed");
        loop {
            boot_delay.delay_millis(1000);
        }
    };

    let mut page = match load_directory_page(&sd, current_path.as_str(), 0, page_size) {
        Ok(page) => page,
        Err(err) => {
            esp_println::println!("Directory listing failed: {:?}", err);
            display.init();
            render_error_screen(&mut display, "Directory listing error");
            loop {
                boot_delay.delay_millis(1000);
            }
        }
    };

    esp_println::println!("Display init start");
    display.init();
    esp_println::println!("Display init complete");

    let mut browser = PagedBrowser::new(page_size);
    let mut screen_mode = ScreenMode::Browse;
    let mut reader_entry: Option<ListedEntry> = None;
    let mut reader_page = 0usize;
    browser.set_page(page.info.page_start, page.entries.len(), 0);
    esp_println::println!("Browser render start, entries={}", page.entries.len());
    render_browser_screen(
        &mut display,
        current_path.as_str(),
        &page.entries,
        browser.selected_index(page.entries.len()),
        BrowserRefresh::Full,
    );
    esp_println::println!("Browser render complete");

    let loop_delay = Delay::new();
    let mut current_state = ButtonState::default();
    let mut last_state = ButtonState::default();
    let mut last_debounce_time = Instant::now();
    let mut adc_config = AdcConfig::new();
    let mut adc_pin1 = adc_config.enable_pin(peripherals.GPIO1, Attenuation::_11dB);
    let mut adc_pin2 = adc_config.enable_pin(peripherals.GPIO2, Attenuation::_11dB);
    let mut adc = Adc::new(peripherals.ADC1, adc_config);
    let power_button = Input::new(
        peripherals.GPIO3,
        InputConfig::default().with_pull(Pull::Up),
    );

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

        if power_button.is_low() {
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

        if screen_mode == ScreenMode::Browse && pressed_events.any_pressed() {
            let browser_input = if pressed_events.is_pressed(Button::Left) {
                Some(BrowserInput::Left)
            } else if pressed_events.is_pressed(Button::Right) {
                Some(BrowserInput::Right)
            } else if pressed_events.is_pressed(Button::Up) {
                Some(BrowserInput::Up)
            } else if pressed_events.is_pressed(Button::Down) {
                Some(BrowserInput::Down)
            } else {
                None
            };

            if let Some(input) = browser_input {
                match browser.handle(
                    input,
                    page.entries.len(),
                    page.info.has_prev,
                    page.info.has_next,
                ) {
                    PagedAction::None => {}
                    PagedAction::Redraw => {
                        render_browser_screen(
                            &mut display,
                            current_path.as_str(),
                            &page.entries,
                            browser.selected_index(page.entries.len()),
                            BrowserRefresh::Fast,
                        );
                    }
                    PagedAction::LoadPage { page_start, selected } => {
                        match load_directory_page(&sd, current_path.as_str(), page_start, page_size) {
                            Ok(next_page) => {
                                page = next_page;
                                browser.set_page(page.info.page_start, page.entries.len(), selected);
                                render_browser_screen(
                                    &mut display,
                                    current_path.as_str(),
                                    &page.entries,
                                    browser.selected_index(page.entries.len()),
                                    BrowserRefresh::Fast,
                                );
                            }
                            Err(err) => {
                                esp_println::println!("Directory listing failed: {:?}", err);
                                render_error_screen(&mut display, "Directory listing error");
                            }
                        }
                    }
                    PagedAction::OpenSelected(index) => {
                        let local_index = index.saturating_sub(page.info.page_start);
                        if let Some(entry) = page.entries.get(local_index) {
                            match entry.kind {
                                xteink_browser::EntryKind::Directory => {
                                    match join_child_path(current_path.as_str(), entry.fs_name.as_str()) {
                                        Ok(next_path) => {
                                            current_path = next_path;
                                            browser = PagedBrowser::new(page_size);
                                            match load_directory_page(&sd, current_path.as_str(), 0, page_size) {
                                                Ok(next_page) => {
                                                    page = next_page;
                                                    browser.set_page(page.info.page_start, page.entries.len(), 0);
                                                    render_browser_screen(
                                                        &mut display,
                                                        current_path.as_str(),
                                                        &page.entries,
                                                        browser.selected_index(page.entries.len()),
                                                        BrowserRefresh::Full,
                                                    );
                                                }
                                                Err(err) => {
                                                    esp_println::println!(
                                                        "Directory listing failed: {:?}",
                                                        err
                                                    );
                                                    render_error_screen(
                                                        &mut display,
                                                        "Directory listing error",
                                                    );
                                                }
                                            }
                                        }
                                        Err(_) => {
                                            esp_println::println!("Enter directory failed");
                                            render_error_screen(
                                                &mut display,
                                                "Failed to open directory",
                                            );
                                        }
                                    }
                                }
                                xteink_browser::EntryKind::Epub => {
                                    reader_entry = Some(entry.clone());
                                    reader_page = 0;
                                    if let Err(err) = render_epub_from_entry(
                                        &sd,
                                        &mut display,
                                        current_path.as_str(),
                                        entry,
                                    ) {
                                        esp_println::println!("EPUB render failed: {:?}", err);
                                        render_error_screen(&mut display, "EPUB render error");
                                    } else {
                                        screen_mode = ScreenMode::Reading;
                                    }
                                }
                                xteink_browser::EntryKind::Other => {
                                    render_browser_screen(
                                        &mut display,
                                        current_path.as_str(),
                                        &page.entries,
                                        browser.selected_index(page.entries.len()),
                                        BrowserRefresh::Fast,
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }

        else if screen_mode == ScreenMode::Reading && pressed_events.any_pressed() {
            let reader_input = if pressed_events.is_pressed(Button::Left) {
                Some(BrowserInput::Left)
            } else if pressed_events.is_pressed(Button::Right) {
                Some(BrowserInput::Right)
            } else if pressed_events.is_pressed(Button::Up) || pressed_events.is_pressed(Button::Down) {
                Some(BrowserInput::Down)
            } else {
                None
            };

            if let Some(input) = reader_input {
                match input {
                    BrowserInput::Left => {
                        reader_page = reader_page.saturating_sub(1);
                        if let Some(entry) = reader_entry.as_ref() {
                            if let Err(err) = render_epub_page_from_entry(
                                &sd,
                                &mut display,
                                current_path.as_str(),
                                entry,
                                reader_page,
                                true,
                            ) {
                                esp_println::println!("EPUB page render failed: {:?}", err);
                                render_error_screen(&mut display, "EPUB render error");
                            }
                        }
                    }
                    BrowserInput::Right => {
                        reader_page = reader_page.saturating_add(1);
                        if let Some(entry) = reader_entry.as_ref() {
                            match render_epub_page_from_entry(
                                &sd,
                                &mut display,
                                current_path.as_str(),
                                entry,
                                reader_page,
                                true,
                            ) {
                                Ok(rendered_page) => reader_page = rendered_page,
                                Err(err) => {
                                    esp_println::println!("EPUB page render failed: {:?}", err);
                                    render_error_screen(&mut display, "EPUB render error");
                                }
                            }
                        }
                    }
                    BrowserInput::Down => {
                        screen_mode = ScreenMode::Browse;
                        render_browser_screen(
                            &mut display,
                            current_path.as_str(),
                            &page.entries,
                            browser.selected_index(page.entries.len()),
                            BrowserRefresh::Full,
                        );
                    }
                    BrowserInput::Up => {}
                }
            }
        }

        loop_delay.delay_millis(1);
    }
}

fn browser_page_size() -> usize {
    let line_height = usize::from(bookerly::BOOKERLY.line_height_px());
    let used_top = 4 + line_height * 2;
    let visible = usize::from(DISPLAY_HEIGHT).saturating_sub(used_top) / line_height.max(1);
    visible.clamp(1, sd_ffi::MAX_ENTRIES)
}

fn render_browser_screen<D>(
    display: &mut D,
    title: &str,
    entries: &[ListedEntry],
    selected: Option<usize>,
    refresh: BrowserRefresh,
) where
    D: BrowserScreenDisplay,
{
    display.clear(0xFF);
    display.draw_text(4, 4, title);

    let line_height = bookerly::BOOKERLY.line_height_px();
    let mut cursor_y = 4 + line_height * 2;

    for (index, entry) in entries.iter().enumerate() {
        if cursor_y.saturating_add(line_height) > DISPLAY_HEIGHT {
            break;
        }

        let mut line = String::<96>::new();
        if Some(index) == selected {
            let _ = line.push('>');
        } else {
            let _ = line.push(' ');
        }
        let _ = line.push(' ');
        let prefix = match entry.kind {
            xteink_browser::EntryKind::Directory => "[D] ",
            xteink_browser::EntryKind::Epub => "[E] ",
            xteink_browser::EntryKind::Other => "[ ] ",
        };
        let _ = line.push_str(prefix);
        let _ = line.push_str(entry.label.as_str());
        display.draw_text(4, cursor_y, line.as_str());
        cursor_y = cursor_y.saturating_add(line_height);
    }

    match refresh {
        BrowserRefresh::Full => display.refresh_full(),
        BrowserRefresh::Fast => display.refresh_fast(),
    }
}

fn render_error_screen<SPI, DC, RST, BUSY, DELAY>(
    display: &mut SSD1677Display<SPI, DC, RST, BUSY, DELAY>,
    message: &str,
) where
    SPI: SpiDevice,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin,
    BUSY: embedded_hal::digital::InputPin,
    DELAY: embedded_hal::delay::DelayNs,
{
    display.clear(0xFF);
    display.draw_wrapped_text(4, 4, message, DISPLAY_HEIGHT);
    display.refresh_full();
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

#[cfg(test)]
mod tests {
    use super::{ListedEntry, ScreenMode, render_browser_screen};
    use xteink_browser::EntryKind;

    #[derive(Default)]
    struct BrowserRenderRecorder {
        calls: [&'static str; 8],
        len: usize,
    }

    impl BrowserRenderRecorder {
        fn push(&mut self, call: &'static str) {
            self.calls[self.len] = call;
            self.len += 1;
        }
    }

    impl BrowserScreenDisplay for BrowserRenderRecorder {
        fn clear(&mut self, _color: u8) {
            self.push("clear");
        }

        fn draw_text(&mut self, _x: u16, _y: u16, _text: &str) {
            self.push("draw");
        }

        fn refresh_full(&mut self) {
            self.push("refresh_full");
        }
    }

    #[test]
    fn browser_screen_uses_full_refresh() {
        let mut display = BrowserRenderRecorder::default();
        let mut label = heapless::String::<96>::new();
        label.push_str("book.epub").unwrap();
        let entries = [ListedEntry {
            label,
            kind: EntryKind::Epub,
        }];

        render_browser_screen(&mut display, "/", &entries, Some(0));

        assert!(display.calls[..display.len].contains(&"refresh_full"));
        assert!(!display.calls[..display.len].contains(&"refresh_fast"));
    }
}
