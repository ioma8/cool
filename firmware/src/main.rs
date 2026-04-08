#![no_std]
#![no_main]

use core::cell::RefCell;
use core::mem::{size_of, size_of_val};
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
use xteink_app::{AppStorage, ListedEntry as AppListedEntry, Session};
use xteink_buttons::{
    Button as RawButton, ButtonState, get_button_from_adc_1, get_button_from_adc_2,
};
use xteink_controller::BrowserRefresh;
use xteink_display::{DISPLAY_HEIGHT, SSD1677Display};
use xteink_fs::{
    FsError, ListedEntry, MAX_ENTRIES, SdFilesystem, init_sd, render_epub_from_entry,
    render_epub_page_from_entry,
};
use xteink_memory::{
    DEVICE_PERSISTENT_BUDGET_BYTES, DEVICE_STACK_RESERVE_BYTES, DEVICE_TOTAL_RAM_BYTES,
    DEVICE_TRANSIENT_HEADROOM_BYTES, DeviceMemoryFootprint,
};
use xteink_render::{Framebuffer, bookerly};

use embedded_hal::spi::{SpiBus, SpiDevice};

esp_bootloader_esp_idf::esp_app_desc!();
const BUTTON_EVENT_CHANNEL_CAPACITY: usize = 8;
const STACK_REPORT_GRANULARITY_BYTES: usize = 256;

static BUTTON_EVENT_CHANNEL: Channel<
    CriticalSectionRawMutex,
    RawButton,
    BUTTON_EVENT_CHANNEL_CAPACITY,
> = Channel::new();
static UI_TASK_STACK_PROBE: Mutex<CriticalSectionRawMutex, StackProbe> =
    Mutex::new(StackProbe::new());
static INPUT_TASK_STACK_PROBE: Mutex<CriticalSectionRawMutex, StackProbe> =
    Mutex::new(StackProbe::new());

#[derive(Clone, Copy)]
struct StackProbe {
    top: usize,
    low: usize,
    reported_usage: usize,
    has_reported: bool,
}

impl StackProbe {
    const fn new() -> Self {
        Self {
            top: 0,
            low: usize::MAX,
            reported_usage: 0,
            has_reported: false,
        }
    }
}
const FIRMWARE_STATIC_DEVICE_BYTES: usize = size_of::<
    Channel<CriticalSectionRawMutex, RawButton, BUTTON_EVENT_CHANNEL_CAPACITY>,
>() + xteink_render::EPUB_RENDER_WORKSPACE_BYTES;

const _: [(); 1] = [(); (FIRMWARE_STATIC_DEVICE_BYTES <= DEVICE_PERSISTENT_BUDGET_BYTES) as usize];

#[derive(Debug)]
enum FirmwareStorageError {
    Fs(FsError),
    Epub(xteink_epub::EpubError),
}

impl core::fmt::Display for FirmwareStorageError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Fs(error) => write!(f, "filesystem error: {:?}", error),
            Self::Epub(error) => write!(f, "epub error: {:?}", error),
        }
    }
}

impl From<FsError> for FirmwareStorageError {
    fn from(error: FsError) -> Self {
        Self::Fs(error)
    }
}

impl From<xteink_epub::EpubError> for FirmwareStorageError {
    fn from(error: xteink_epub::EpubError) -> Self {
        Self::Epub(error)
    }
}

struct FirmwareStorage<'a, SD> {
    sd: &'a SD,
}

impl<'a, SD> FirmwareStorage<'a, SD> {
    fn new(sd: &'a SD) -> Self {
        Self { sd }
    }
}

fn fs_entry_to_app_entry(entry: &ListedEntry) -> AppListedEntry {
    let mut label = heapless::String::new();
    let mut fs_name = heapless::String::new();
    let _ = label.push_str(entry.label.as_str());
    let _ = fs_name.push_str(entry.fs_name.as_str());
    AppListedEntry {
        label,
        fs_name,
        kind: entry.kind,
    }
}

fn app_entry_to_fs_entry(entry: &AppListedEntry) -> ListedEntry {
    let mut label = heapless::String::new();
    let mut fs_name = heapless::String::new();
    let _ = label.push_str(entry.label.as_str());
    let _ = fs_name.push_str(entry.fs_name.as_str());
    ListedEntry {
        label,
        fs_name,
        kind: entry.kind,
    }
}

impl<'a, SD> AppStorage<Framebuffer> for FirmwareStorage<'a, SD>
where
    SD: SdFilesystem,
{
    type Error = FirmwareStorageError;

    fn list_directory_page(
        &self,
        path: &str,
        page_start: usize,
        page_size: usize,
    ) -> Result<xteink_app::DirectoryPage, Self::Error> {
        let mut entries = heapless::Vec::new();
        let info = self
            .sd
            .list_directory_page(path, page_start, page_size, &mut entries)?;
        let mut app_entries = heapless::Vec::new();
        for entry in entries.iter() {
            let _ = app_entries.push(fs_entry_to_app_entry(entry));
        }
        Ok(xteink_app::DirectoryPage {
            entries: app_entries,
            info: xteink_app::DirectoryPageInfo {
                page_start: info.page_start,
                has_prev: info.has_prev,
                has_next: info.has_next,
            },
        })
    }

    fn render_epub_from_entry(
        &self,
        renderer: &mut Framebuffer,
        current_path: &str,
        entry: &AppListedEntry,
    ) -> Result<usize, Self::Error> {
        let rendered = render_epub_from_entry(
            self.sd,
            renderer,
            current_path,
            &app_entry_to_fs_entry(entry),
        )?;
        Ok(rendered.rendered_page)
    }

    fn render_epub_page_from_entry(
        &self,
        renderer: &mut Framebuffer,
        current_path: &str,
        entry: &AppListedEntry,
        target_page: usize,
    ) -> Result<usize, Self::Error> {
        let rendered = render_epub_page_from_entry(
            self.sd,
            renderer,
            current_path,
            &app_entry_to_fs_entry(entry),
            target_page,
            true,
        )?;
        Ok(rendered.rendered_page)
    }
}

fn firmware_memory_footprint<SD, SPIBUS, SPI, DC, RST, BUSY, DELAY>(
    spi_bus: &Mutex<NoopRawMutex, RefCell<SPIBUS>>,
    session: &Session<FirmwareStorage<'_, SD>, Framebuffer>,
    display: &SSD1677Display<SPI, DC, RST, BUSY, DELAY>,
) -> DeviceMemoryFootprint
where
    SPIBUS: SpiBus,
    SPI: SpiDevice,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin,
    BUSY: embedded_hal::digital::InputPin,
    DELAY: embedded_hal::delay::DelayNs,
{
    DeviceMemoryFootprint::new(
        FIRMWARE_STATIC_DEVICE_BYTES
            + size_of_val(spi_bus)
            + size_of_val(session)
            + size_of_val(display),
    )
}

fn enforce_firmware_memory_budget<SD, SPIBUS, SPI, DC, RST, BUSY, DELAY>(
    spi_bus: &Mutex<NoopRawMutex, RefCell<SPIBUS>>,
    session: &Session<FirmwareStorage<'_, SD>, Framebuffer>,
    display: &SSD1677Display<SPI, DC, RST, BUSY, DELAY>,
) where
    SPIBUS: SpiBus,
    SPI: SpiDevice,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin,
    BUSY: embedded_hal::digital::InputPin,
    DELAY: embedded_hal::delay::DelayNs,
{
    let footprint = firmware_memory_footprint(spi_bus, session, display);
    assert!(
        footprint.fits_device_budget(),
        "firmware device memory footprint {} exceeds budget {}",
        footprint.device_bytes,
        DEVICE_PERSISTENT_BUDGET_BYTES
    );
}

fn log_firmware_memory_report(footprint: DeviceMemoryFootprint) {
    let used_permille = footprint.used_device_permille();
    esp_println::println!(
        "Memory footprint: device={}B/{}B ({}.{}%), heap={}B, non_heap={}B, remaining={}B, total_ram={}B, stack_reserve={}B, transient_headroom={}B",
        footprint.device_bytes,
        DEVICE_PERSISTENT_BUDGET_BYTES,
        used_permille / 10,
        used_permille % 10,
        footprint.device_heap_bytes,
        footprint.device_bytes.saturating_sub(footprint.device_heap_bytes),
        footprint.remaining_device_bytes(),
        DEVICE_TOTAL_RAM_BYTES,
        DEVICE_STACK_RESERVE_BYTES,
        DEVICE_TRANSIENT_HEADROOM_BYTES,
    );
}

fn apply_refresh<SPI, DC, RST, BUSY, DELAY>(
    display: &mut SSD1677Display<SPI, DC, RST, BUSY, DELAY>,
    framebuffer: &[u8; xteink_display::BUFFER_SIZE],
    refresh: BrowserRefresh,
) where
    SPI: SpiDevice,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin,
    BUSY: embedded_hal::digital::InputPin,
    DELAY: embedded_hal::delay::DelayNs,
{
    match refresh {
        BrowserRefresh::Full => display.refresh_full(framebuffer),
        BrowserRefresh::Fast => display.refresh_fast(framebuffer),
    }
}

#[inline]
fn current_stack_pointer() -> usize {
    let sp: usize;
    unsafe {
        core::arch::asm!("mv {}, sp", out(reg) sp);
    }
    sp
}

fn observe_task_stack(task_name: &str, probe: &'static Mutex<CriticalSectionRawMutex, StackProbe>) {
    let sp = current_stack_pointer();
    unsafe {
        probe.lock_mut(|state| {
        if state.top == 0 {
            state.top = sp;
        }
        if sp < state.low {
            state.low = sp;
        }

        let used = state.top.saturating_sub(state.low);
        let should_report = if !state.has_reported {
            true
        } else {
            used >= state.reported_usage.saturating_add(STACK_REPORT_GRANULARITY_BYTES)
        };

        if should_report {
            state.reported_usage = used;
            state.has_reported = true;
            esp_println::println!(
                "Stack usage: task={} observed_used={}B",
                task_name,
                used,
            );
        }
    })
    };
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
    let page_size = browser_page_size();

    let Some(sd) = sd else {
        let mut display = display;
        let mut framebuffer = Framebuffer::new();
        let mut pending_display_refresh = PendingDisplayRefresh::None;
        display.init();
        render_error_screen(&mut framebuffer, "SD init failed", &mut pending_display_refresh);
        service_display_refresh(&mut display, framebuffer.bytes(), &mut pending_display_refresh);
        loop {
            yield_now().await;
        }
    };

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

    let mut display = display;
    display.init();
    let mut session = Session::new(FirmwareStorage::new(&sd), Framebuffer::new(), page_size);
    match session.bootstrap() {
        Ok(refresh) => {
            let footprint = firmware_memory_footprint(&spi_bus, &session, &display);
            log_firmware_memory_report(footprint);
            enforce_firmware_memory_budget(&spi_bus, &session, &display);
            apply_refresh(&mut display, session.renderer().bytes(), refresh);
        }
        Err(err) => {
            esp_println::println!("Session bootstrap failed: {:?}", err);
            let mut pending_display_refresh = PendingDisplayRefresh::None;
            render_error_screen(
                session.renderer_mut(),
                "Directory listing error",
                &mut pending_display_refresh,
            );
            service_display_refresh(
                &mut display,
                session.renderer().bytes(),
                &mut pending_display_refresh,
            );
            loop {
                yield_now().await;
            }
        }
    }

    spawner.must_spawn(input_task(sender, power_button));
    ui_task(&mut session, &mut display, receiver).await;
    loop {}
}

#[embassy_executor::task]
async fn input_task(
    sender: Sender<'static, CriticalSectionRawMutex, RawButton, BUTTON_EVENT_CHANNEL_CAPACITY>,
    power_button: Input<'static>,
) {
    observe_task_stack("input_task", &INPUT_TASK_STACK_PROBE);
    let mut last_raw_state = ButtonState::default();
    let mut pressed_lock: Option<RawButton> = None;
    let mut no_button_streak: u8 = RELEASE_STREAK_TO_REARM_PRESS;
    let delay = Delay::new();
    loop {
        observe_task_stack("input_task", &INPUT_TASK_STACK_PROBE);
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
    session: &mut Session<FirmwareStorage<'_, SD>, Framebuffer>,
    display: &mut SSD1677Display<SPI, DC, RST, BUSY, DELAY>,
    receiver: Receiver<'static, CriticalSectionRawMutex, RawButton, BUTTON_EVENT_CHANNEL_CAPACITY>,
) where
    SD: xteink_fs::SdFilesystem,
    SPI: SpiDevice,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin,
    BUSY: embedded_hal::digital::InputPin,
    DELAY: embedded_hal::delay::DelayNs,
{
    observe_task_stack("ui_task", &UI_TASK_STACK_PROBE);
    loop {
        observe_task_stack("ui_task", &UI_TASK_STACK_PROBE);
        let button = receiver.receive().await;
        match session.handle_button(button) {
            Ok(Some(refresh)) => {
                apply_refresh(display, session.renderer().bytes(), refresh);
            }
            Ok(None) => {}
            Err(err) => {
                esp_println::println!("Session button handling failed: {:?}", err);
                let mut pending_display_refresh = PendingDisplayRefresh::None;
                render_error_screen(
                    session.renderer_mut(),
                    "Session error",
                    &mut pending_display_refresh,
                );
                service_display_refresh(
                    display,
                    session.renderer().bytes(),
                    &mut pending_display_refresh,
                );
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

fn render_error_screen(
    display: &mut Framebuffer,
    message: &str,
    pending_display_refresh: &mut PendingDisplayRefresh,
) {
    display.clear(0xFF);
    display.draw_wrapped_text(4, 4, message, DISPLAY_HEIGHT);
    pending_display_refresh.request(BrowserRefresh::Full);
}

fn service_display_refresh<SPI, DC, RST, BUSY, DELAY>(
    display: &mut SSD1677Display<SPI, DC, RST, BUSY, DELAY>,
    framebuffer: &[u8; xteink_display::BUFFER_SIZE],
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
        BrowserRefresh::Full => display.refresh_full_nonblocking(framebuffer),
        BrowserRefresh::Fast => display.refresh_fast_nonblocking(framebuffer),
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
