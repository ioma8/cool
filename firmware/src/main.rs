#![no_std]
#![no_main]

use core::cell::RefCell;
use embassy_embedded_hal::shared_bus::blocking::spi::SpiDeviceWithConfig;
use embassy_executor::Spawner;
use embassy_futures::yield_now;
use embassy_sync::blocking_mutex::{Mutex, raw::CriticalSectionRawMutex, raw::NoopRawMutex};
use embassy_sync::channel::{Channel, Receiver, Sender};
use esp_backtrace as _;
use esp_hal::{
    analog::adc::{Adc, AdcConfig, Attenuation},
    clock::CpuClock,
    delay::Delay,
    gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull, RtcPinWithResistors},
    peripherals::APB_SARADC,
    spi::master::{Config as SpiConfig, Spi},
    time::Rate,
};
use heapless::String;
use xteink_buttons::{
    Button as RawButton, ButtonState, get_button_from_adc_1, get_button_from_adc_2,
};
use xteink_controller::{AppController, BrowserRefresh, ControllerCommand, UiEntry};
use xteink_display::{DISPLAY_HEIGHT, DISPLAY_WIDTH, SSD1677Display, bookerly};
use xteink_fs::{
    EpubRefreshMode, ListedEntry, MAX_ENTRIES, SdFilesystem, init_sd, render_epub_from_entry,
    render_epub_page_from_entry,
};

use embedded_hal::spi::{SpiBus, SpiDevice};

esp_bootloader_esp_idf::esp_app_desc!();
const BUTTON_EVENT_CHANNEL_CAPACITY: usize = 8;

static BUTTON_EVENT_CHANNEL: Channel<
    CriticalSectionRawMutex,
    RawButton,
    BUTTON_EVENT_CHANNEL_CAPACITY,
> = Channel::new();
static APP_DIRECTORY_PAGE: Mutex<CriticalSectionRawMutex, xteink_fs::DirectoryPage> =
    Mutex::new(xteink_fs::DirectoryPage {
        entries: heapless::Vec::new(),
        info: xteink_fs::DirectoryPageInfo {
            page_start: 0,
            has_prev: false,
            has_next: false,
        },
    });
#[inline]
fn app_directory_page_with<R>(f: impl FnOnce(&xteink_fs::DirectoryPage) -> R) -> R {
    APP_DIRECTORY_PAGE.lock(|page| f(page))
}

#[inline]
fn app_directory_page_with_mut<R>(f: impl FnOnce(&mut xteink_fs::DirectoryPage) -> R) -> R {
    unsafe { APP_DIRECTORY_PAGE.lock_mut(|page| f(page)) }
}

fn load_browser_directory_page<SD: SdFilesystem>(
    sd: &SD,
    current_path: &str,
    page_start: usize,
    page_size: usize,
) -> Result<(), xteink_fs::FsError> {
    app_directory_page_with_mut(|page| {
        page.entries.clear();
        let info =
            sd.list_directory_page(current_path, page_start, page_size, &mut page.entries)?;
        page.info = info;
        Ok(())
    })
}

#[inline]
fn render_current_browser_screen<SPI, DC, RST, BUSY, DELAY>(
    display: &mut SSD1677Display<SPI, DC, RST, BUSY, DELAY>,
    current_path: &str,
    controller: &AppController,
    refresh: BrowserRefresh,
    pending_display_refresh: &mut PendingDisplayRefresh,
) where
    SPI: SpiDevice,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin,
    BUSY: embedded_hal::digital::InputPin,
    DELAY: embedded_hal::delay::DelayNs,
{
    app_directory_page_with(|page| {
        render_browser_screen(
            display,
            current_path,
            &page.entries,
            controller.browser().selected_index(page.entries.len()),
            refresh,
            pending_display_refresh,
        );
    });
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingDisplayRefresh {
    None,
    Full,
    Fast,
}

impl PendingDisplayRefresh {
    fn request(&mut self, refresh: BrowserRefresh) {
        *self = match (*self, refresh) {
            (_, BrowserRefresh::Full) => Self::Full,
            (Self::None, BrowserRefresh::Fast) => Self::Fast,
            (Self::Full, BrowserRefresh::Fast) => Self::Full,
            (Self::Fast, BrowserRefresh::Fast) => Self::Fast,
        };
    }

    fn as_refresh(&self) -> Option<BrowserRefresh> {
        match self {
            Self::None => None,
            Self::Full => Some(BrowserRefresh::Full),
            Self::Fast => Some(BrowserRefresh::Fast),
        }
    }
}

const ADC_ATTEN_BITS_12DB: u8 = 0x03;
const BUTTON_SCAN_ATTEMPTS: usize = 6;
const BUTTON_SCAN_DELAY_US: u32 = 150;
const RELEASE_STREAK_TO_REARM_PRESS: u8 = 2;

#[inline]
fn read_adc1_oneshot_raw(channel: u8, attenuation_bits: u8) -> u16 {
    let masked = attenuation_bits & 0x03;

    APB_SARADC::regs().onetime_sample().modify(|_, w| unsafe {
        w.saradc1_onetime_sample().set_bit();
        w.onetime_channel().bits(channel);
        w.onetime_atten().bits(masked)
    });
    APB_SARADC::regs()
        .onetime_sample()
        .modify(|_, w| w.onetime_start().set_bit());

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

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
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
    let display = SSD1677Display::new(
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
        let mut display = display;
        let mut pending_display_refresh = PendingDisplayRefresh::None;
        display.init();
        render_error_screen(&mut display, "SD init failed", &mut pending_display_refresh);
        service_display_refresh(&mut display, &mut pending_display_refresh);
        loop {
            yield_now().await;
        }
    };

    if let Err(err) = load_browser_directory_page(&sd, current_path.as_str(), 0, page_size) {
        let mut display = display;
        let mut pending_display_refresh = PendingDisplayRefresh::None;
        esp_println::println!("Directory listing failed: {:?}", err);
        display.init();
        render_error_screen(
            &mut display,
            "Directory listing error",
            &mut pending_display_refresh,
        );
        service_display_refresh(&mut display, &mut pending_display_refresh);
        loop {
            yield_now().await;
        }
    }

    let mut controller = AppController::new(page_size);
    app_directory_page_with(|page| {
        controller.apply_directory_loaded(page.info.page_start, page.entries.len(), 0);
    });

    let sender = BUTTON_EVENT_CHANNEL.sender();
    let receiver = BUTTON_EVENT_CHANNEL.receiver();

    let mut adc_config = AdcConfig::new();
    peripherals.GPIO1.rtcio_pullup(false);
    peripherals.GPIO1.rtcio_pulldown(true);
    peripherals.GPIO2.rtcio_pullup(false);
    peripherals.GPIO2.rtcio_pulldown(true);
    peripherals.GPIO3.rtcio_pullup(false);
    peripherals.GPIO3.rtcio_pulldown(true);
    let _adc_pin1 = adc_config.enable_pin(peripherals.GPIO1, Attenuation::_11dB);
    let _adc_pin2 = adc_config.enable_pin(peripherals.GPIO2, Attenuation::_11dB);
    let _adc = Adc::new(peripherals.ADC1, adc_config);
    let _ = _adc;
    let power_button = Input::new(
        peripherals.GPIO3,
        InputConfig::default().with_pull(Pull::Down),
    );

    let mut init_display = display;
    init_display.init();
    spawner.must_spawn(input_task(sender, power_button));
    ui_task(sd, init_display, page_size, controller, receiver).await;
    loop {}
}

#[embassy_executor::task]
async fn input_task(
    sender: Sender<'static, CriticalSectionRawMutex, RawButton, BUTTON_EVENT_CHANNEL_CAPACITY>,
    power_button: Input<'static>,
) {
    let mut last_raw_state = ButtonState::default();
    let mut pressed_lock: Option<RawButton> = None;
    let mut no_button_streak: u8 = RELEASE_STREAK_TO_REARM_PRESS;
    let delay = Delay::new();
    loop {
        let mut raw_state = ButtonState::default();
        let mut decoded_pin1 = None;
        let mut decoded_pin2 = None;
        let mut power_button_pressed = false;

        for _ in 0..BUTTON_SCAN_ATTEMPTS {
            let adc1_value = read_adc1_oneshot_raw(1, ADC_ATTEN_BITS_12DB);
            delay.delay_micros(BUTTON_SCAN_DELAY_US);
            let adc2_value = read_adc1_oneshot_raw(2, ADC_ATTEN_BITS_12DB);
            let sample_pin1 = get_button_from_adc_1(adc1_value);
            let sample_pin2 = get_button_from_adc_2(adc2_value);
            let sample_power = power_button.is_low();

            if sample_pin1.is_some() || sample_pin2.is_some() || sample_power {
                decoded_pin1 = sample_pin1;
                decoded_pin2 = sample_pin2;
                power_button_pressed = sample_power;
                break;
            }

            delay.delay_micros(BUTTON_SCAN_DELAY_US);
            yield_now().await;
        }

        if let Some(raw_button) = decoded_pin1 {
            raw_state = raw_state.with_button(raw_button);
        } else if let Some(raw_button) = decoded_pin2 {
            raw_state = raw_state.with_button(raw_button);
        }

        if power_button_pressed {
            raw_state = raw_state.with_button(RawButton::Power);
        }

        let pressed = pressed_button_from_state(last_raw_state, raw_state);
        last_raw_state = raw_state;
        if let Some(button) = pressed {
            if pressed_lock.is_none() && no_button_streak >= RELEASE_STREAK_TO_REARM_PRESS {
                let _ = sender.send(button).await;
                pressed_lock = Some(button);
                no_button_streak = 0;
            }
        }

        if raw_state.state == 0 {
            no_button_streak = no_button_streak.saturating_add(1);
            if no_button_streak >= RELEASE_STREAK_TO_REARM_PRESS {
                pressed_lock = None;
            }
        } else {
            no_button_streak = 0;
        }

        yield_now().await;
    }
}

async fn ui_task<SD, SPI, DC, RST, BUSY, DELAY>(
    sd: SD,
    mut display: SSD1677Display<SPI, DC, RST, BUSY, DELAY>,
    page_size: usize,
    mut controller: AppController,
    receiver: Receiver<'static, CriticalSectionRawMutex, RawButton, BUTTON_EVENT_CHANNEL_CAPACITY>,
) where
    SD: xteink_fs::SdFilesystem,
    SPI: SpiDevice,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin,
    BUSY: embedded_hal::digital::InputPin,
    DELAY: embedded_hal::delay::DelayNs,
{
    let mut pending_display_refresh = PendingDisplayRefresh::None;
    app_directory_page_with(|page| {
        render_browser_screen(
            &mut display,
            controller.current_path(),
            &page.entries,
            controller.browser().selected_index(page.entries.len()),
            BrowserRefresh::Full,
            &mut pending_display_refresh,
        );
    });
    service_display_refresh(&mut display, &mut pending_display_refresh);

    loop {
        let button = receiver.receive().await;
        let command = app_directory_page_with(|page| {
            let mut ui_entries = heapless::Vec::<UiEntry, MAX_ENTRIES>::new();
            for entry in page.entries.iter() {
                let _ = ui_entries.push(listed_entry_to_ui_entry(entry));
            }
            controller.handle_button(
                button,
                ui_entries.as_slice(),
                controller_page_info(page.info),
            )
        });

        handle_controller_command(
            &sd,
            &mut display,
            page_size,
            &mut controller,
            command,
            &mut pending_display_refresh,
        );

        service_display_refresh(&mut display, &mut pending_display_refresh);
    }
}

fn browser_page_size() -> usize {
    let line_height = usize::from(bookerly::BOOKERLY.line_height_px());
    let used_top = 4 + line_height * 2;
    let visible = usize::from(DISPLAY_HEIGHT).saturating_sub(used_top) / line_height.max(1);
    visible.clamp(1, MAX_ENTRIES)
}

fn handle_controller_command<SD, SPI, DC, RST, BUSY, DELAY>(
    sd: &SD,
    display: &mut SSD1677Display<SPI, DC, RST, BUSY, DELAY>,
    page_size: usize,
    controller: &mut AppController,
    command: ControllerCommand,
    pending_display_refresh: &mut PendingDisplayRefresh,
) where
    SD: xteink_fs::SdFilesystem,
    SPI: SpiDevice,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin,
    BUSY: embedded_hal::digital::InputPin,
    DELAY: embedded_hal::delay::DelayNs,
{
    match command {
        ControllerCommand::None => {}
        ControllerCommand::RenderBrowser { refresh } => {
            render_current_browser_screen(
                display,
                controller.current_path(),
                controller,
                refresh,
                pending_display_refresh,
            );
        }
        ControllerCommand::LoadDirectory {
            path,
            page_start,
            selected,
            refresh,
        } => match load_browser_directory_page(sd, path.as_str(), page_start, page_size) {
            Ok(()) => {
                let next_len = app_directory_page_with(|page| page.entries.len());
                controller.apply_directory_loaded(page_start, next_len, selected);
                render_current_browser_screen(
                    display,
                    controller.current_path(),
                    controller,
                    refresh,
                    pending_display_refresh,
                );
            }
            Err(err) => {
                esp_println::println!("Directory listing failed: {:?}", err);
                render_error_screen(display, "Directory listing error", pending_display_refresh);
            }
        },
        ControllerCommand::OpenEpub { path, entry } => {
            render_loading_popover(
                display,
                "Loading book...",
                "Please wait",
                pending_display_refresh,
            );
            service_display_refresh(display, pending_display_refresh);
            let listed_entry = ui_entry_to_listed_entry(&entry);
            match render_epub_from_entry(sd, display, path.as_str(), &listed_entry) {
                Ok(result) => {
                    controller.apply_epub_opened(result.rendered_page);
                    match result.refresh {
                        EpubRefreshMode::Full => {
                            pending_display_refresh.request(BrowserRefresh::Full)
                        }
                        EpubRefreshMode::Fast => {
                            pending_display_refresh.request(BrowserRefresh::Fast)
                        }
                    }
                }
                Err(err) => {
                    esp_println::println!("EPUB render failed: {:?}", err);
                    render_error_screen(display, "EPUB render error", pending_display_refresh);
                }
            }
        }
        ControllerCommand::RenderReaderPage {
            path,
            entry,
            target_page,
            fast,
        } => {
            let listed_entry = ui_entry_to_listed_entry(&entry);
            match render_epub_page_from_entry(
                sd,
                display,
                path.as_str(),
                &listed_entry,
                target_page,
                fast,
            ) {
                Ok(result) => {
                    controller.apply_reader_page_rendered(result.rendered_page);
                    match result.refresh {
                        EpubRefreshMode::Full => {
                            pending_display_refresh.request(BrowserRefresh::Full)
                        }
                        EpubRefreshMode::Fast => {
                            pending_display_refresh.request(BrowserRefresh::Fast)
                        }
                    }
                }
                Err(err) => {
                    esp_println::println!("EPUB page render failed: {:?}", err);
                    render_error_screen(display, "EPUB render error", pending_display_refresh);
                }
            }
        }
    }
}

fn listed_entry_to_ui_entry(entry: &ListedEntry) -> UiEntry {
    let mut name = heapless::String::new();
    let _ = name.push_str(entry.fs_name.as_str());
    UiEntry {
        name,
        kind: entry.kind,
    }
}

fn ui_entry_to_listed_entry(entry: &UiEntry) -> ListedEntry {
    let mut label = heapless::String::new();
    let mut fs_name = heapless::String::new();
    let _ = label.push_str(entry.name.as_str());
    let _ = fs_name.push_str(entry.name.as_str());
    ListedEntry {
        label,
        fs_name,
        kind: entry.kind,
    }
}

fn controller_page_info(
    info: xteink_fs::DirectoryPageInfo,
) -> xteink_controller::DirectoryPageInfo {
    xteink_controller::DirectoryPageInfo {
        page_start: info.page_start,
        has_prev: info.has_prev,
        has_next: info.has_next,
    }
}

fn render_browser_screen<SPI, DC, RST, BUSY, DELAY>(
    display: &mut SSD1677Display<SPI, DC, RST, BUSY, DELAY>,
    title: &str,
    entries: &[ListedEntry],
    selected: Option<usize>,
    refresh: BrowserRefresh,
    pending_display_refresh: &mut PendingDisplayRefresh,
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
    pending_display_refresh.request(refresh);
}

fn render_error_screen<SPI, DC, RST, BUSY, DELAY>(
    display: &mut SSD1677Display<SPI, DC, RST, BUSY, DELAY>,
    message: &str,
    pending_display_refresh: &mut PendingDisplayRefresh,
) where
    SPI: SpiDevice,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin,
    BUSY: embedded_hal::digital::InputPin,
    DELAY: embedded_hal::delay::DelayNs,
{
    display.clear(0xFF);
    display.draw_wrapped_text(4, 4, message, DISPLAY_HEIGHT);
    pending_display_refresh.request(BrowserRefresh::Full);
}

fn render_loading_popover<SPI, DC, RST, BUSY, DELAY>(
    display: &mut SSD1677Display<SPI, DC, RST, BUSY, DELAY>,
    title: &str,
    subtitle: &str,
    pending_display_refresh: &mut PendingDisplayRefresh,
) where
    SPI: SpiDevice,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin,
    BUSY: embedded_hal::digital::InputPin,
    DELAY: embedded_hal::delay::DelayNs,
{
    let box_width = 240u16;
    let box_height = 96u16;
    let left = (DISPLAY_WIDTH.saturating_sub(box_width)) / 2;
    let top = (DISPLAY_HEIGHT.saturating_sub(box_height)) / 2;
    let right = left.saturating_add(box_width).min(DISPLAY_WIDTH);
    let bottom = top.saturating_add(box_height).min(DISPLAY_HEIGHT);

    for y in top..bottom {
        for x in left..right {
            display.set_pixel(x, y, false);
        }
    }

    for x in left..right {
        display.set_pixel(x, top, true);
        display.set_pixel(x, bottom.saturating_sub(1), true);
    }
    for y in top..bottom {
        display.set_pixel(left, y, true);
        display.set_pixel(right.saturating_sub(1), y, true);
    }

    display.draw_text(left + 16, top + 16, title);
    display.draw_text(
        left + 16,
        top + 16 + bookerly::BOOKERLY.line_height_px() + 8,
        subtitle,
    );
    pending_display_refresh.request(BrowserRefresh::Fast);
}

fn service_display_refresh<SPI, DC, RST, BUSY, DELAY>(
    display: &mut SSD1677Display<SPI, DC, RST, BUSY, DELAY>,
    pending_display_refresh: &mut PendingDisplayRefresh,
) where
    SPI: SpiDevice,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin,
    BUSY: embedded_hal::digital::InputPin,
    DELAY: embedded_hal::delay::DelayNs,
{
    let Some(refresh) = pending_display_refresh.as_refresh() else {
        return;
    };

    let schedule = match refresh {
        BrowserRefresh::Full => display.refresh_full_nonblocking(),
        BrowserRefresh::Fast => display.refresh_fast_nonblocking(),
    };

    if schedule.is_ok() {
        *pending_display_refresh = PendingDisplayRefresh::None;
    }
}

fn pressed_button_from_state(
    previous_state: ButtonState,
    current_state: ButtonState,
) -> Option<RawButton> {
    let new_press = ButtonState {
        state: current_state.state & !previous_state.state,
    };
    if let Some(button) = new_press.first_pressed() {
        return Some(button);
    }
    None
}
