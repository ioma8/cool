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
    gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull, RtcPinWithResistors},
    main,
    spi::master::{Config as SpiConfig, Spi},
    peripherals::APB_SARADC,
    time::Rate,
};
use heapless::String;
use xteink_fs::{
    init_sd, join_child_path, load_directory_page, render_epub_from_entry,
    render_epub_page_from_entry, ListedEntry, MAX_ENTRIES,
};
use xteink_browser::{Input as BrowserInput, PagedAction, PagedBrowser};
use xteink_display::{DISPLAY_HEIGHT, SSD1677Display, bookerly};
use xteink_buttons::{
    Button as RawButton, ButtonState, get_button_from_adc_1, get_button_from_adc_2,
};
use xteink_input::InputManager;

use embedded_hal::spi::{SpiBus, SpiDevice};

esp_bootloader_esp_idf::esp_app_desc!();
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

const ADC_ATTEN_BITS_12DB: u8 = 0x03;

#[inline]
fn read_adc1_oneshot_raw(channel: u8, attenuation_bits: u8) -> u16 {
    let masked = attenuation_bits & 0x03;

    APB_SARADC::regs().onetime_sample().modify(|_, w| unsafe {
        w.saradc1_onetime_sample().set_bit();
        w.onetime_channel().bits(channel);
        w.onetime_atten().bits(masked)
    });
    APB_SARADC::regs().onetime_sample().modify(|_, w| {
        w.onetime_start().set_bit()
    });

    while !APB_SARADC::regs().int_raw().read().adc1_done().bit() {}

    let value = APB_SARADC::regs()
        .sar1data_status()
        .read()
        .saradc1_data()
        .bits() as u16;

    APB_SARADC::regs()
        .int_clr()
        .write(|w| w.adc1_done().clear_bit_by_one());
    APB_SARADC::regs()
        .onetime_sample()
        .modify(|_, w| w.onetime_start().clear_bit());

    value
}

#[main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));

    let boot_delay = Delay::new();
    let usb_detect = Input::new(
        peripherals.GPIO20,
        InputConfig::default().with_pull(Pull::Down),
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
    let mut input_manager = InputManager::new();
    let mut now_ms = 0u32;
    let mut adc_config = AdcConfig::new();
    let mut debug_frame: u32 = 0;
    peripherals.GPIO1.rtcio_pullup(false);
    peripherals.GPIO1.rtcio_pulldown(true);
    peripherals.GPIO2.rtcio_pullup(false);
    peripherals.GPIO2.rtcio_pulldown(true);
    peripherals.GPIO3.rtcio_pullup(false);
    peripherals.GPIO3.rtcio_pulldown(true);
    let _adc_pin1 = adc_config.enable_pin(peripherals.GPIO1, Attenuation::_11dB);
    let _adc_pin2 = adc_config.enable_pin(peripherals.GPIO2, Attenuation::_11dB);
    let _adc = Adc::new(peripherals.ADC1, adc_config);
    let power_button = Input::new(
        peripherals.GPIO3,
        InputConfig::default().with_pull(Pull::Down),
    );

    loop {
        let mut raw_state = ButtonState::default();
        let adc1_value = read_adc1_oneshot_raw(1, ADC_ATTEN_BITS_12DB);
        loop_delay.delay_millis(1);
        let adc2_value = read_adc1_oneshot_raw(2, ADC_ATTEN_BITS_12DB);
        let power_button_pressed = power_button.is_low();
        let decoded_pin1 = get_button_from_adc_1(adc1_value);
        let decoded_pin2 = get_button_from_adc_2(adc2_value);

        if let Some(raw_button) = decoded_pin1 {
            raw_state = raw_state.with_button(raw_button);
        } else if let Some(raw_button) = decoded_pin2 {
            raw_state = raw_state.with_button(raw_button);
        }

        if power_button_pressed {
            raw_state = raw_state.with_button(RawButton::Power);
        }

        if (debug_frame & 0x0F) == 0 {
            esp_println::println!(
                "adc1={} adc2={} pin1={:?} pin2={:?} power={} state={:08b}",
                adc1_value,
                adc2_value,
                decoded_pin1,
                decoded_pin2,
                power_button_pressed,
                raw_state.state,
            );
        }
        debug_frame = debug_frame.wrapping_add(1);

        input_manager.update(raw_state, now_ms);
        let pressed = pressed_button(&input_manager);

        now_ms = now_ms.saturating_add(1);

        if screen_mode == ScreenMode::Browse {
            if let Some(button) = pressed {
                match button {
                    RawButton::Confirm => {
                        if let Some(entry) = page
                            .entries
                            .get(browser.selected_index(page.entries.len()).unwrap_or(0))
                        {
                            match entry.kind {
                                xteink_browser::EntryKind::Directory => {
                                    match join_child_path(current_path.as_str(), entry.fs_name.as_str()) {
                                        Ok(next_path) => {
                                            current_path = next_path;
                                            browser = PagedBrowser::new(page_size);
                                            match load_directory_page(&sd, current_path.as_str(), 0, page_size) {
                                                Ok(next_page) => {
                                                    page = next_page;
                                                    browser.set_page(
                                                        page.info.page_start,
                                                        page.entries.len(),
                                                        0,
                                                    );
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
                                            render_error_screen(&mut display, "Failed to open directory");
                                        }
                                    }
                                }
                                xteink_browser::EntryKind::Epub => {
                                    reader_entry = Some(entry.clone());
                                    reader_page = 0;
                                    if let Err(err) =
                                        render_epub_from_entry(&sd, &mut display, current_path.as_str(), entry)
                                    {
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
                    RawButton::Back => {
                        if current_path.as_str() != "/" {
                            if let Some(parent) = current_path.as_str().rsplit_once('/') {
                                let mut next_path: String<256> = String::new();
                                let _ = next_path
                                    .push_str(if parent.0.is_empty() { "/" } else { parent.0 });
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
                                        esp_println::println!("Directory listing failed: {:?}", err);
                                        render_error_screen(&mut display, "Directory listing error");
                                    }
                                }
                            }
                        }
                    }
                    RawButton::Left | RawButton::Up => {
                        if let Some(selected) = browser.selected_index(page.entries.len()) {
                            match browser.handle(
                                BrowserInput::Left,
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
                                        Some(selected),
                                        BrowserRefresh::Fast,
                                    );
                                }
                                PagedAction::LoadPage {
                                    page_start,
                                    selected,
                                } => {
                                    match load_directory_page(
                                        &sd,
                                        current_path.as_str(),
                                        page_start,
                                        page_size,
                                    ) {
                                        Ok(next_page) => {
                                            page = next_page;
                                            browser.set_page(
                                                page.info.page_start,
                                                page.entries.len(),
                                                selected,
                                            );
                                            render_browser_screen(
                                                &mut display,
                                                current_path.as_str(),
                                                &page.entries,
                                                browser.selected_index(page.entries.len()),
                                                BrowserRefresh::Fast,
                                            );
                                        }
                                        Err(err) => {
                                            esp_println::println!(
                                                "Directory listing failed: {:?}",
                                                err
                                            );
                                            render_error_screen(&mut display, "Directory listing error");
                                        }
                                    }
                                }
                                PagedAction::OpenSelected(_) => {}
                            }
                        }
                    }
                    RawButton::Right | RawButton::Down => {
                        match browser.handle(
                            BrowserInput::Right,
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
                            PagedAction::LoadPage {
                                page_start,
                                selected,
                            } => match load_directory_page(&sd, current_path.as_str(), page_start, page_size) {
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
                                    esp_println::println!(
                                        "Directory listing failed: {:?}",
                                        err
                                    );
                                    render_error_screen(&mut display, "Directory listing error");
                                }
                            },
                            PagedAction::OpenSelected(_) => {}
                        }
                    }
                    _ => {}
                }
            }
        } else if screen_mode == ScreenMode::Reading && pressed.is_some() {
            let reader_input = match pressed {
                Some(RawButton::Left) | Some(RawButton::Up) => Some(BrowserInput::Left),
                Some(RawButton::Right) | Some(RawButton::Down) => Some(BrowserInput::Right),
                Some(RawButton::Back) => Some(BrowserInput::Down),
                _ => None,
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

    }
}

fn browser_page_size() -> usize {
    let line_height = usize::from(bookerly::BOOKERLY.line_height_px());
    let used_top = 4 + line_height * 2;
    let visible = usize::from(DISPLAY_HEIGHT).saturating_sub(used_top) / line_height.max(1);
    visible.clamp(1, MAX_ENTRIES)
}

fn render_browser_screen<SPI, DC, RST, BUSY, DELAY>(
    display: &mut SSD1677Display<SPI, DC, RST, BUSY, DELAY>,
    title: &str,
    entries: &[ListedEntry],
    selected: Option<usize>,
    refresh: BrowserRefresh,
) where
    SPI: SpiDevice,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin,
    BUSY: embedded_hal::digital::InputPin,
    DELAY: embedded_hal::delay::DelayNs,
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

fn pressed_button(input_manager: &InputManager) -> Option<RawButton> {
    if input_manager.was_pressed(RawButton::Confirm) {
        Some(RawButton::Confirm)
    } else if input_manager.was_pressed(RawButton::Back) {
        Some(RawButton::Back)
    } else if input_manager.was_pressed(RawButton::Left) {
        Some(RawButton::Left)
    } else if input_manager.was_pressed(RawButton::Right) {
        Some(RawButton::Right)
    } else if input_manager.was_pressed(RawButton::Up) {
        Some(RawButton::Up)
    } else if input_manager.was_pressed(RawButton::Down) {
        Some(RawButton::Down)
    } else if input_manager.was_pressed(RawButton::Power) {
        Some(RawButton::Power)
    } else {
        None
    }
}
