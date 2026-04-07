#![no_std]

use core::mem::MaybeUninit;

use embedded_hal::{
    delay::DelayNs,
    digital::{InputPin, OutputPin},
    spi::SpiDevice,
};
use critical_section;
#[cfg(target_arch = "riscv32")]
use esp_rtos::CurrentThreadHandle;
use miniz_oxide::inflate::stream::InflateState;
use xteink_epub::{
    Epub,
    EpubArchive,
    EpubEvent,
    EpubSource,
    ReaderBuffers,
    MAX_ARCHIVE_ENTRIES,
    MAX_ARCHIVE_NAME_CAPACITY,
};

pub mod bookerly;
pub mod demo;
pub(crate) mod pagination;
pub(crate) mod text;

use pagination::{CachedPaginationState, CachedTextRenderer};
use text::{TextBuffer, WrappedLine, measure_text_width};
pub use demo::{DemoDisplay, show_embedded_epub_demo};

const PHYSICAL_WIDTH: u16 = 800;
const PHYSICAL_HEIGHT: u16 = 480;
pub const DISPLAY_WIDTH_BYTES: u16 = PHYSICAL_WIDTH / 8;
pub const BUFFER_SIZE: usize = (DISPLAY_WIDTH_BYTES as usize) * (PHYSICAL_HEIGHT as usize);

pub const DISPLAY_WIDTH: u16 = 480;
pub const DISPLAY_HEIGHT: u16 = 800;

const CMD_SOFT_RESET: u8 = 0x12;
const CMD_BOOSTER_SOFT_START: u8 = 0x0C;
const CMD_DRIVER_OUTPUT_CONTROL: u8 = 0x01;
const CMD_BORDER_WAVEFORM: u8 = 0x3C;
const CMD_TEMP_SENSOR_CONTROL: u8 = 0x18;
const CMD_DATA_ENTRY_MODE: u8 = 0x11;
const CMD_SET_RAM_X_RANGE: u8 = 0x44;
const CMD_SET_RAM_Y_RANGE: u8 = 0x45;
const CMD_SET_RAM_X_COUNTER: u8 = 0x4E;
const CMD_SET_RAM_Y_COUNTER: u8 = 0x4F;
const CMD_WRITE_RAM_BW: u8 = 0x24;
const CMD_WRITE_RAM_RED: u8 = 0x26;
const CMD_DISPLAY_UPDATE_CTRL1: u8 = 0x21;
const CMD_DISPLAY_UPDATE_CTRL2: u8 = 0x22;
const CMD_MASTER_ACTIVATION: u8 = 0x20;
const CMD_WRITE_TEMP: u8 = 0x1A;
const CMD_DEEP_SLEEP: u8 = 0x10;

const CTRL1_NORMAL: u8 = 0x00;
const CTRL1_BYPASS_RED: u8 = 0x40;
const DATA_ENTRY_X_INC_Y_DEC: u8 = 0x01;
const TEMP_SENSOR_INTERNAL: u8 = 0x80;
const DISPLAY_BUSY_TIMEOUT_MS: u16 = 250;
const EPUB_WORKSPACE_ZIP_CD: usize = 768;
const EPUB_WORKSPACE_INFLATE: usize = 3 * 1024;
const EPUB_WORKSPACE_STREAM_INPUT: usize = 1024;
const EPUB_WORKSPACE_XML: usize = 1024;
const EPUB_WORKSPACE_CATALOG: usize = 1536;
const EPUB_WORKSPACE_PATH_BUF: usize = 256;
const EPUB_WORKSPACE_BUDGET: usize = 50 * 1024;
const TEXT_LEN: usize = 2048;
const CACHED_TEXT_CHUNK: usize = 1024;
const CACHED_LINE_LEN: usize = 1024;
const EPUB_RENDER_YIELD_INTERVAL: usize = 1;

#[inline]
fn cooperative_yield() {
    #[cfg(target_arch = "riscv32")]
    {
        CurrentThreadHandle::get().delay(esp_hal::time::Duration::from_micros(0));
    }
}

struct EpubRenderWorkspace {
    zip_cd: [u8; EPUB_WORKSPACE_ZIP_CD],
    inflate: [u8; EPUB_WORKSPACE_INFLATE],
    stream_input: [u8; EPUB_WORKSPACE_STREAM_INPUT],
    xml: [u8; EPUB_WORKSPACE_XML],
    catalog: [u8; EPUB_WORKSPACE_CATALOG],
    path_buf: [u8; EPUB_WORKSPACE_PATH_BUF],
    stream_state: InflateState,
    archive: EpubArchive<MAX_ARCHIVE_ENTRIES, MAX_ARCHIVE_NAME_CAPACITY>,
}

impl EpubRenderWorkspace {
    fn new() -> Self {
        Self {
            zip_cd: [0u8; EPUB_WORKSPACE_ZIP_CD],
            inflate: [0u8; EPUB_WORKSPACE_INFLATE],
            stream_input: [0u8; EPUB_WORKSPACE_STREAM_INPUT],
            xml: [0u8; EPUB_WORKSPACE_XML],
            catalog: [0u8; EPUB_WORKSPACE_CATALOG],
            path_buf: [0u8; EPUB_WORKSPACE_PATH_BUF],
            stream_state: InflateState::new(miniz_oxide::DataFormat::Raw),
            archive: EpubArchive::new(),
        }
    }
}

const _: [(); 1] = [(); (core::mem::size_of::<EpubRenderWorkspace>() <= EPUB_WORKSPACE_BUDGET) as usize];

static mut EPUB_RENDER_WORKSPACE: MaybeUninit<EpubRenderWorkspace> = MaybeUninit::uninit();
static mut EPUB_RENDER_WORKSPACE_READY: bool = false;

fn with_epub_render_workspace<R>(f: impl FnOnce(&mut EpubRenderWorkspace) -> R) -> R {
    critical_section::with(|_| {
        // SAFETY: access is serialized by the critical section above.
        let workspace = unsafe {
            if !EPUB_RENDER_WORKSPACE_READY {
                core::ptr::write(core::ptr::addr_of_mut!(EPUB_RENDER_WORKSPACE) as *mut EpubRenderWorkspace, EpubRenderWorkspace::new());
                EPUB_RENDER_WORKSPACE_READY = true;
            }
            &mut *(core::ptr::addr_of_mut!(EPUB_RENDER_WORKSPACE) as *mut EpubRenderWorkspace)
        };
        f(workspace)
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshMode {
    Full,
    Half,
    Fast,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshScheduleError {
    Busy,
}

pub struct SSD1677Display<SPI, DC, RST, BUSY, DELAY>
where
    SPI: SpiDevice,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
    DELAY: DelayNs,
{
    spi: SPI,
    dc: DC,
    rst: RST,
    busy: BUSY,
    delay: DELAY,
    framebuffer: [u8; BUFFER_SIZE],
    is_screen_on: bool,
}

impl<SPI, DC, RST, BUSY, DELAY> SSD1677Display<SPI, DC, RST, BUSY, DELAY>
where
    SPI: SpiDevice,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
    DELAY: DelayNs,
{
    pub fn new(spi: SPI, dc: DC, rst: RST, busy: BUSY, delay: DELAY) -> Self {
        Self {
            spi,
            dc,
            rst,
            busy,
            delay,
            framebuffer: [0xFF; BUFFER_SIZE],
            is_screen_on: false,
        }
    }

    pub fn init(&mut self) {
        self.reset_display();
        self.init_display_controller();
    }

    pub fn clear(&mut self, color: u8) {
        self.framebuffer.fill(color);
    }

    pub fn framebuffer(&self) -> &[u8; BUFFER_SIZE] {
        &self.framebuffer
    }

    pub fn spi(&self) -> &SPI {
        &self.spi
    }

    pub fn set_pixel(&mut self, x: u16, y: u16, black: bool) {
        if x >= DISPLAY_WIDTH || y >= DISPLAY_HEIGHT {
            return;
        }

        let px = y;
        let py = (DISPLAY_WIDTH - 1) - x;
        let idx = (py as usize) * (DISPLAY_WIDTH_BYTES as usize) + (px as usize / 8);
        let bit = 7 - (px % 8);

        if black {
            self.framebuffer[idx] &= !(1 << bit);
        } else {
            self.framebuffer[idx] |= 1 << bit;
        }
    }

    pub fn draw_text(&mut self, x: u16, y: u16, text: &str) {
        let mut cursor_x = i32::from(x);
        let mut cursor_y = i32::from(y);
        let line_height = i32::from(bookerly::BOOKERLY.line_height_px());

        for ch in text.chars() {
            if ch == '\n' {
                cursor_x = i32::from(x);
                cursor_y += line_height;
                continue;
            }

            let glyph = bookerly::BOOKERLY.glyph_for_char(ch);
            let left = cursor_x + i32::from(glyph.left);
            let top = cursor_y + i32::from(glyph.top);
            self.draw_glyph(glyph, left, top);
            cursor_x += i32::from(glyph.advance_x);
        }
    }

    pub fn draw_wrapped_text(&mut self, x: u16, y: u16, text: &str, max_y: u16) -> u16 {
        const LINE_BUF_LEN: usize = 512;

        let mut cursor_y = y;
        let line_height = bookerly::BOOKERLY.line_height_px();
        let available_width = i32::from(DISPLAY_WIDTH.saturating_sub(x));
        let mut line = WrappedLine::<LINE_BUF_LEN>::new();

        for paragraph in text.split('\n') {
            for word in paragraph.split_whitespace() {
                let word_width = self.measure_text_width(word);
                let word_fits = if line.is_empty() {
                    word_width <= available_width
                } else {
                    line.width + self.measure_text_width(" ") + word_width <= available_width
                };

                if !word_fits && !line.is_empty() {
                    if cursor_y.saturating_add(line_height) > max_y {
                        return cursor_y;
                    }
                    self.draw_text(x, cursor_y, line.as_str());
                    cursor_y = cursor_y.saturating_add(line_height);
                    line.clear();
                }

                if !line.is_empty() {
                    line.push_space();
                }
                line.push_str(word);
            }

            if !line.is_empty() {
                if cursor_y.saturating_add(line_height) > max_y {
                    return cursor_y;
                }
                self.draw_text(x, cursor_y, line.as_str());
                cursor_y = cursor_y.saturating_add(line_height);
                line.clear();
            }
        }

        cursor_y
    }

    pub fn render_embedded_epub_first_screen(&mut self) -> Result<(), xteink_epub::EpubError> {
        self.clear(0xFF);
        self.draw_wrapped_text(
            16,
            16,
            "EPUB demo disabled.\nLoad books from storage instead.",
            DISPLAY_HEIGHT.saturating_sub(16),
        );
        Ok(())
    }

    pub fn render_epub_first_screen<S: EpubSource>(
        &mut self,
        source: S,
    ) -> Result<(), xteink_epub::EpubError> {
        let _ = self.render_epub_page(source, 0)?;
        Ok(())
    }

    pub fn render_epub_page<S: EpubSource>(
        &mut self,
        source: S,
        target_page: usize,
    ) -> Result<usize, xteink_epub::EpubError> {
        self.render_epub_page_with_text_sink_and_cancel(source, target_page, |_| Ok(()), false, || false)
    }

    pub fn render_epub_page_with_text_sink_and_cancel<S: EpubSource, F, C>(
        &mut self,
        source: S,
        target_page: usize,
        mut on_text_chunk: F,
        parse_full_book: bool,
        mut should_cancel: C,
    ) -> Result<usize, xteink_epub::EpubError>
    where
        F: FnMut(&str) -> Result<(), xteink_epub::EpubError>,
        C: FnMut() -> bool,
    {
        with_epub_render_workspace(|workspace| {
            let workspace_ptr = core::ptr::addr_of_mut!(*workspace);
            let mut epub = Epub::open(source)?;
            let mut text = TextBuffer::<TEXT_LEN>::new();
            let mut cursor_y = 0u16;
            let mut current_page = 0usize;
            let mut render_enabled = true;
            let mut event_budget = 0usize;
            let mut event_count = 0usize;
            let line_height = bookerly::BOOKERLY.line_height_px();

            self.clear(0xFF);

            loop {
                if should_cancel() {
                    return Err(xteink_epub::EpubError::Cancelled);
                }

                let event = epub.next_event(ReaderBuffers {
                    zip_cd: unsafe { &mut (*workspace_ptr).zip_cd },
                    inflate: unsafe { &mut (*workspace_ptr).inflate },
                    stream_input: unsafe { &mut (*workspace_ptr).stream_input },
                    xml: unsafe { &mut (*workspace_ptr).xml },
                    catalog: unsafe { &mut (*workspace_ptr).catalog },
                    path_buf: unsafe { &mut (*workspace_ptr).path_buf },
                    stream_state: unsafe { &mut (*workspace_ptr).stream_state },
                    archive: unsafe { &mut (*workspace_ptr).archive },
                })?;

                let Some(event) = event else {
                    break;
                };
                event_count = event_count.saturating_add(1);

                match event {
                    EpubEvent::Text(chunk) => {
                        if render_enabled {
                            text.push(chunk);
                        }
                        on_text_chunk(chunk)?;
                    }
                    EpubEvent::LineBreak => {
                        if render_enabled {
                            cursor_y = self.flush_text_buffer(&mut text, cursor_y);
                            cursor_y = cursor_y.saturating_add(line_height);
                        }
                        on_text_chunk("\n")?;
                    }
                    EpubEvent::ParagraphStart | EpubEvent::HeadingStart(_) => {}
                    EpubEvent::ParagraphEnd | EpubEvent::HeadingEnd => {
                        if render_enabled {
                            cursor_y = self.flush_text_buffer(&mut text, cursor_y);
                            cursor_y = cursor_y.saturating_add(line_height / 2);
                        }
                        on_text_chunk("\n")?;
                    }
                    EpubEvent::Image { alt, .. } => {
                        if let Some(alt) = alt {
                            if render_enabled {
                                text.push(alt);
                            }
                            on_text_chunk(alt)?;
                            if render_enabled {
                                cursor_y = self.flush_text_buffer(&mut text, cursor_y);
                            }
                            on_text_chunk("\n")?;
                            if render_enabled {
                                cursor_y = cursor_y.saturating_add(line_height);
                            }
                        }
                    }
                    EpubEvent::UnsupportedTag => {}
                }

                event_budget = event_budget.saturating_add(1);
                if event_budget >= EPUB_RENDER_YIELD_INTERVAL {
                    event_budget = 0;
                    cooperative_yield();
                    if should_cancel() {
                        return Err(xteink_epub::EpubError::Cancelled);
                    }
                }

                if cursor_y >= DISPLAY_HEIGHT {
                    if render_enabled && current_page >= target_page {
                        if parse_full_book {
                            render_enabled = false;
                            cursor_y = 0;
                            text.clear();
                            continue;
                        }
                        break;
                    }
                    current_page = current_page.saturating_add(1);
                    cursor_y = 0;
                    text.clear();
                    if render_enabled {
                        self.clear(0xFF);
                    }
                }
            }

            if !text.is_empty() {
                on_text_chunk(text.as_str())?;
                text.clear();
            }
            if render_enabled {
                let _ = self.flush_text_buffer(&mut text, cursor_y);
            }
            Ok(current_page)
        })
    }

    pub fn render_epub_page_with_text_sink<S: EpubSource, F>(
        &mut self,
        source: S,
        target_page: usize,
        on_text_chunk: F,
        parse_full_book: bool,
    ) -> Result<usize, xteink_epub::EpubError>
    where
        F: FnMut(&str) -> Result<(), xteink_epub::EpubError>,
    {
        self.render_epub_page_with_text_sink_and_cancel(source, target_page, on_text_chunk, parse_full_book, || false)
    }

    pub fn render_cached_text_page<R>(
        &mut self,
        read_text: &mut R,
        target_page: usize,
    ) -> Result<usize, xteink_epub::EpubError>
    where
        R: FnMut(&mut [u8]) -> Result<usize, xteink_epub::EpubError>,
    {
        self.render_cached_text_page_with_cancel(read_text, target_page, || false)
    }

    pub fn render_cached_text_page_with_cancel<R, C>(
        &mut self,
        read_text: &mut R,
        target_page: usize,
        mut should_cancel: C,
    ) -> Result<usize, xteink_epub::EpubError>
    where
        R: FnMut(&mut [u8]) -> Result<usize, xteink_epub::EpubError>,
        C: FnMut() -> bool,
    {
        let mut cursor_y = 0u16;
        let mut current_page = 0usize;
        let line_height = bookerly::BOOKERLY.line_height_px();
        let mut state = CachedPaginationState::<CACHED_LINE_LEN>::new();
        let mut read_buffer = [0u8; CACHED_TEXT_CHUNK];
        let mut done = false;
        let mut chunk_count = 0usize;

        self.clear(0xFF);

        loop {
            if should_cancel() {
                return Err(xteink_epub::EpubError::Cancelled);
            }
            let read_len = read_text(&mut read_buffer)?;
            if read_len == 0 {
                break;
            }
            chunk_count = chunk_count.saturating_add(1);

            #[cfg(target_arch = "riscv32")]
            if chunk_count % 32 == 0 {
                let _ = esp_println::println!(
                    "EPUB cached render chunk: count={} page={} cursor_y={} done={}",
                    chunk_count,
                    current_page,
                    cursor_y,
                    done
                );
            }

            let mut source = &read_buffer[..read_len];
            while !source.is_empty() && !done {
                match core::str::from_utf8(source) {
                    Ok(text) => {
                        state.cursor_y = cursor_y;
                        state.current_page = current_page;
                        done = pagination::render_cached_text_snippet(
                            self,
                            &mut state,
                            target_page,
                            line_height,
                            text,
                        )?;
                        cursor_y = state.cursor_y;
                        current_page = state.current_page;
                        source = &[];
                    }
                    Err(err) => {
                        let valid = err.valid_up_to();
                        if valid > 0 {
                            state.cursor_y = cursor_y;
                            state.current_page = current_page;
                            done = pagination::render_cached_text_snippet(
                                self,
                                &mut state,
                                target_page,
                                line_height,
                                core::str::from_utf8(&source[..valid]).unwrap_or(""),
                            )?;
                            cursor_y = state.cursor_y;
                            current_page = state.current_page;
                            source = &source[valid..];
                        } else {
                            source = &source[1..];
                        }
                    }
                }
            }

            if done {
                break;
            }

            cooperative_yield();
        }

        if !done {
            state.cursor_y = cursor_y;
            state.current_page = current_page;
            pagination::render_cached_text_snippet(self, &mut state, target_page, line_height, "")?;
            current_page = state.current_page;
        }
        Ok(current_page)
    }

    fn draw_glyph(&mut self, glyph: &bookerly::Glyph, x: i32, y: i32) {
        if glyph.width == 0 || glyph.height == 0 || glyph.data_length == 0 {
            return;
        }

        let row_bytes = usize::from(glyph.width).div_ceil(8);
        let start = glyph.data_offset as usize;
        let end = start + glyph.data_length as usize;
        let bitmap = &bookerly::BOOKERLY.bitmap[start..end];

        for row in 0..glyph.height {
            let row_start = usize::from(row) * row_bytes;
            for col in 0..glyph.width {
                let byte = bitmap[row_start + usize::from(col / 8)];
                let mask = 1 << (7 - (col % 8));
                if byte & mask == 0 {
                    continue;
                }

                let px = x + i32::from(col);
                let py = y + i32::from(row);
                if px < 0
                    || py < 0
                    || px >= i32::from(DISPLAY_WIDTH)
                    || py >= i32::from(DISPLAY_HEIGHT)
                {
                    continue;
                }

                self.set_pixel(px as u16, py as u16, true);
            }
        }
    }

    fn measure_text_width(&self, text: &str) -> i32 {
        measure_text_width(text)
    }

    fn flush_text_buffer<const N: usize>(
        &mut self,
        buffer: &mut TextBuffer<N>,
        cursor_y: u16,
    ) -> u16 {
        if buffer.is_empty() {
            return cursor_y;
        }

        let next_y = self.draw_wrapped_text(0, cursor_y, buffer.as_str(), DISPLAY_HEIGHT);
        buffer.clear();
        next_y
    }

    pub fn display_buffer(&mut self, mode: RefreshMode, wait_for_ready: bool) -> Result<(), RefreshScheduleError> {
        if !wait_for_ready && self.is_busy() {
            return Err(RefreshScheduleError::Busy);
        }

        self.set_ram_area(0, 0, PHYSICAL_WIDTH, PHYSICAL_HEIGHT);

        if mode != RefreshMode::Fast {
            self.send_command(CMD_WRITE_RAM_BW);
            self.write_framebuffer();
            self.send_command(CMD_WRITE_RAM_RED);
            self.write_framebuffer();
        } else {
            self.send_command(CMD_WRITE_RAM_BW);
            self.write_framebuffer();
        }

        self.refresh_display(mode, false);

        if mode == RefreshMode::Fast {
            self.set_ram_area(0, 0, PHYSICAL_WIDTH, PHYSICAL_HEIGHT);
            self.send_command(CMD_WRITE_RAM_RED);
            self.write_framebuffer();
        }

        if wait_for_ready {
            self.wait_while_busy();
        }

        Ok(())
    }

    pub fn refresh_full(&mut self) {
        let _ = self.display_buffer(RefreshMode::Full, true);
    }

    pub fn refresh_fast(&mut self) {
        let _ = self.display_buffer(RefreshMode::Fast, true);
    }

    pub fn refresh_full_nonblocking(&mut self) -> Result<(), RefreshScheduleError> {
        self.display_buffer(RefreshMode::Full, false)
    }

    pub fn refresh_fast_nonblocking(&mut self) -> Result<(), RefreshScheduleError> {
        self.display_buffer(RefreshMode::Fast, false)
    }

    pub fn is_busy(&mut self) -> bool {
        self.busy.is_high().unwrap_or(false)
    }

    pub fn deep_sleep(&mut self) {
        if self.is_screen_on {
            self.send_command(CMD_DISPLAY_UPDATE_CTRL1);
            self.send_data_byte(CTRL1_BYPASS_RED);
            self.send_command(CMD_DISPLAY_UPDATE_CTRL2);
            self.send_data_byte(0x03);
            self.send_command(CMD_MASTER_ACTIVATION);
            self.wait_while_busy();
            self.is_screen_on = false;
        }

        self.send_command(CMD_DEEP_SLEEP);
        self.send_data_byte(0x01);
    }

    fn reset_display(&mut self) {
        let _ = self.rst.set_high();
        self.delay.delay_ms(20);
        let _ = self.rst.set_low();
        self.delay.delay_ms(2);
        let _ = self.rst.set_high();
        self.delay.delay_ms(20);
    }

    fn init_display_controller(&mut self) {
        self.send_command(CMD_SOFT_RESET);
        self.wait_while_busy();
        self.delay.delay_ms(10);

        self.send_command(CMD_TEMP_SENSOR_CONTROL);
        self.send_data_byte(TEMP_SENSOR_INTERNAL);

        self.send_command(CMD_BOOSTER_SOFT_START);
        self.send_data_byte(0xAE);
        self.send_data_byte(0xC7);
        self.send_data_byte(0xC3);
        self.send_data_byte(0xC0);
        self.send_data_byte(0x40);

        self.send_command(CMD_DRIVER_OUTPUT_CONTROL);
        self.send_data_byte(((PHYSICAL_HEIGHT - 1) & 0xFF) as u8);
        self.send_data_byte((((PHYSICAL_HEIGHT - 1) >> 8) & 0xFF) as u8);
        self.send_data_byte(0x02);

        self.send_command(CMD_BORDER_WAVEFORM);
        self.send_data_byte(0x01);
        self.set_ram_area(0, 0, PHYSICAL_WIDTH, PHYSICAL_HEIGHT);
    }

    fn set_ram_area(&mut self, x: u16, y: u16, w: u16, h: u16) {
        let y = PHYSICAL_HEIGHT - y - h;

        self.send_command(CMD_DATA_ENTRY_MODE);
        self.send_data_byte(DATA_ENTRY_X_INC_Y_DEC);

        self.send_command(CMD_SET_RAM_X_RANGE);
        self.send_data_byte((x & 0xFF) as u8);
        self.send_data_byte(((x >> 8) & 0xFF) as u8);
        self.send_data_byte(((x + w - 1) & 0xFF) as u8);
        self.send_data_byte((((x + w - 1) >> 8) & 0xFF) as u8);

        self.send_command(CMD_SET_RAM_Y_RANGE);
        self.send_data_byte(((y + h - 1) & 0xFF) as u8);
        self.send_data_byte((((y + h - 1) >> 8) & 0xFF) as u8);
        self.send_data_byte((y & 0xFF) as u8);
        self.send_data_byte(((y >> 8) & 0xFF) as u8);

        self.send_command(CMD_SET_RAM_X_COUNTER);
        self.send_data_byte((x & 0xFF) as u8);
        self.send_data_byte(((x >> 8) & 0xFF) as u8);

        self.send_command(CMD_SET_RAM_Y_COUNTER);
        self.send_data_byte(((y + h - 1) & 0xFF) as u8);
        self.send_data_byte((((y + h - 1) >> 8) & 0xFF) as u8);
    }

    fn refresh_display(&mut self, mode: RefreshMode, turn_off_screen: bool) {
        self.send_command(CMD_DISPLAY_UPDATE_CTRL1);
        let ctrl1 = if mode == RefreshMode::Fast {
            CTRL1_NORMAL
        } else {
            CTRL1_BYPASS_RED
        };
        self.send_data_byte(ctrl1);

        let mut display_mode = 0x00;
        if !self.is_screen_on {
            self.is_screen_on = true;
            display_mode |= 0xC0;
        }

        if turn_off_screen {
            self.is_screen_on = false;
            display_mode |= 0x03;
        }

        match mode {
            RefreshMode::Full => display_mode |= 0x34,
            RefreshMode::Half => {
                self.send_command(CMD_WRITE_TEMP);
                self.send_data_byte(0x5A);
                display_mode |= 0xD4;
            }
            RefreshMode::Fast => display_mode |= 0x1C,
        }

        self.send_command(CMD_DISPLAY_UPDATE_CTRL2);
        self.send_data_byte(display_mode);
        self.send_command(CMD_MASTER_ACTIVATION);
        self.delay.delay_ms(10);
        self.wait_while_busy();
    }

    fn wait_while_busy(&mut self) {
        for _ in 0..DISPLAY_BUSY_TIMEOUT_MS {
            if !self.busy.is_high().unwrap_or(false) {
                break;
            }
            self.delay.delay_ms(1);
        }
    }

    fn send_command(&mut self, cmd: u8) {
        let _ = self.dc.set_low();
        for _ in 0..10 {
            core::hint::spin_loop();
        }
        let _ = self.spi.write(&[cmd]);
    }

    fn send_data_byte(&mut self, data: u8) {
        let _ = self.dc.set_high();
        for _ in 0..10 {
            core::hint::spin_loop();
        }
        let _ = self.spi.write(&[data]);
    }

    fn write_framebuffer(&mut self) {
        let _ = self.dc.set_high();
        for _ in 0..10 {
            core::hint::spin_loop();
        }
        let mut offset = 0;
        while offset < BUFFER_SIZE {
            let end = (offset + 4096).min(BUFFER_SIZE);
            let _ = self.spi.write(&self.framebuffer[offset..end]);
            offset = end;
        }
    }
}

impl<SPI, DC, RST, BUSY, DELAY> DemoDisplay for SSD1677Display<SPI, DC, RST, BUSY, DELAY>
where
    SPI: SpiDevice,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
    DELAY: DelayNs,
{
    type Error = xteink_epub::EpubError;

    fn init(&mut self) {
        SSD1677Display::init(self);
    }

    fn clear(&mut self, color: u8) {
        SSD1677Display::clear(self, color);
    }

    fn render_embedded_epub_first_screen(&mut self) -> Result<(), Self::Error> {
        SSD1677Display::render_embedded_epub_first_screen(self)
    }

    fn refresh_full(&mut self) {
        SSD1677Display::refresh_full(self);
    }
}

impl<SPI, DC, RST, BUSY, DELAY> CachedTextRenderer for SSD1677Display<SPI, DC, RST, BUSY, DELAY>
where
    SPI: SpiDevice,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
    DELAY: DelayNs,
{
    fn clear(&mut self) {
        SSD1677Display::clear(self, 0xFF);
    }

    fn draw_text(&mut self, x: u16, y: u16, text: &str) {
        SSD1677Display::draw_text(self, x, y, text);
    }

    fn measure_text_width(&self, text: &str) -> i32 {
        SSD1677Display::measure_text_width(self, text)
    }

    fn display_width(&self) -> u16 {
        DISPLAY_WIDTH
    }

    fn display_height(&self) -> u16 {
        DISPLAY_HEIGHT
    }
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod host_critical_section {
    use core::cell::RefCell;
    use std::sync::{Mutex, MutexGuard};

    use critical_section::{set_impl, Impl, RawRestoreState};

    static GLOBAL_MUTEX: Mutex<()> = Mutex::new(());
    std::thread_local!(static GLOBAL_GUARD: RefCell<Option<MutexGuard<'static, ()>>> = const { RefCell::new(None) });

    struct HostCriticalSection;
    set_impl!(HostCriticalSection);

    unsafe impl Impl for HostCriticalSection {
        unsafe fn acquire() -> RawRestoreState {
            let guard = match GLOBAL_MUTEX.lock() {
                Ok(guard) => guard,
                Err(err) => err.into_inner(),
            };
            GLOBAL_GUARD.with(|slot| {
                *slot.borrow_mut() = Some(guard);
            });
        }

        unsafe fn release(_: RawRestoreState) {
            GLOBAL_GUARD.with(|slot| {
                let _guard = slot.borrow_mut().take();
                drop(_guard);
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::convert::Infallible;
    use embedded_hal::spi::Operation;
    use std::{string::{String, ToString}, vec, vec::Vec};
    use xteink_epub::{EpubError, EpubSource};

    const PAGINATION_EPUB_BYTES: &[u8] =
        include_bytes!("../../../test/epubs/test_kerning_ligature.epub");

    struct SliceSource<'a> {
        bytes: &'a [u8],
    }

    impl<'a> EpubSource for SliceSource<'a> {
        fn len(&self) -> usize {
            self.bytes.len()
        }

        fn read_at(&self, offset: u64, buffer: &mut [u8]) -> Result<usize, EpubError> {
            let offset = usize::try_from(offset).map_err(|_| EpubError::InvalidFormat)?;
            if offset >= self.bytes.len() || buffer.is_empty() {
                return Ok(0);
            }
            let len = (self.bytes.len() - offset).min(buffer.len());
            buffer[..len].copy_from_slice(&self.bytes[offset..offset + len]);
            Ok(len)
        }
    }

    #[derive(Debug, Default)]
    struct FakeSpi {
        writes: Vec<Vec<u8>>,
    }

    impl embedded_hal::spi::ErrorType for FakeSpi {
        type Error = Infallible;
    }

    impl SpiDevice for FakeSpi {
        fn transaction(
            &mut self,
            operations: &mut [Operation<'_, u8>],
        ) -> Result<(), Self::Error> {
            for operation in operations {
                match operation {
                    Operation::Read(words) => {
                        for word in words.iter_mut() {
                            *word = 0;
                        }
                    }
                    Operation::Write(words) => {
                        self.writes.push(words.to_vec());
                    }
                    Operation::Transfer(read, write) => {
                        let len = read.len().min(write.len());
                        read[..len].copy_from_slice(&write[..len]);
                        self.writes.push(write.to_vec());
                    }
                    Operation::TransferInPlace(words) => {
                        self.writes.push(words.to_vec());
                    }
                    Operation::DelayNs(_) => {}
                }
            }
            Ok(())
        }
    }

    #[derive(Debug, Default)]
    struct FakeOutputPin {
        states: Vec<bool>,
    }

    impl embedded_hal::digital::ErrorType for FakeOutputPin {
        type Error = Infallible;
    }

    impl OutputPin for FakeOutputPin {
        fn set_low(&mut self) -> Result<(), Self::Error> {
            self.states.push(false);
            Ok(())
        }

        fn set_high(&mut self) -> Result<(), Self::Error> {
            self.states.push(true);
            Ok(())
        }
    }

    #[derive(Debug, Default)]
    struct FakeInputPin {
        states: Vec<bool>,
        cursor: usize,
    }

    impl embedded_hal::digital::ErrorType for FakeInputPin {
        type Error = Infallible;
    }

    impl InputPin for FakeInputPin {
        fn is_high(&mut self) -> Result<bool, Self::Error> {
            let value = self.states.get(self.cursor).copied().unwrap_or(false);
            if self.cursor < self.states.len() {
                self.cursor += 1;
            }
            Ok(value)
        }

        fn is_low(&mut self) -> Result<bool, Self::Error> {
            self.is_high().map(|value| !value)
        }
    }

    #[derive(Debug, Default)]
    struct FakeDelay {
        ms: Vec<u32>,
        us: Vec<u32>,
        ns: Vec<u32>,
    }

    impl DelayNs for FakeDelay {
        fn delay_ns(&mut self, ns: u32) {
            self.ns.push(ns);
        }

        fn delay_us(&mut self, us: u32) {
            self.us.push(us);
        }

        fn delay_ms(&mut self, ms: u32) {
            self.ms.push(ms);
        }
    }

    #[test]
    fn clear_fills_the_framebuffer() {
        let mut display = new_display();

        display.clear(0x00);

        assert!(display.framebuffer().iter().all(|&byte| byte == 0x00));
    }

    #[test]
    fn text_buffer_inserts_separator_only_between_non_whitespace_chunks() {
        let mut buffer = crate::text::TextBuffer::<32>::new();

        buffer.push("Hello");
        buffer.push("world");
        buffer.push("\nnext");

        assert_eq!(buffer.as_str(), "Hello world\nnext");
    }

    #[test]
    fn cached_pagination_snippet_wraps_words_into_multiple_draw_calls() {
        struct FakeRenderer {
            draws: Vec<(u16, String)>,
            clears: usize,
        }

        impl crate::pagination::CachedTextRenderer for FakeRenderer {
            fn clear(&mut self) {
                self.clears += 1;
            }

            fn draw_text(&mut self, _x: u16, y: u16, text: &str) {
                self.draws.push((y, text.to_string()));
            }

            fn measure_text_width(&self, text: &str) -> i32 {
                (text.len() as i32) * 10
            }

            fn display_width(&self) -> u16 {
                30
            }

            fn display_height(&self) -> u16 {
                100
            }
        }

        let mut renderer = FakeRenderer {
            draws: Vec::new(),
            clears: 0,
        };
        let mut state = crate::pagination::CachedPaginationState::<64>::new();

        let done = crate::pagination::render_cached_text_snippet(
            &mut renderer,
            &mut state,
            0,
            10,
            "one two",
        )
        .unwrap();

        assert!(!done);
        assert_eq!(renderer.draws, vec![(0, "one".to_string()), (10, "two".to_string())]);
    }

    #[test]
    fn set_pixel_uses_the_rotated_physical_buffer_coordinates() {
        let mut display = new_display();

        display.clear(0xFF);
        display.set_pixel(0, 0, true);
        display.set_pixel(479, 799, true);

        let first_idx = logical_pixel_index(0, 0);
        let last_idx = logical_pixel_index(479, 799);
        assert_eq!(display.framebuffer()[first_idx], 0x7F);
        assert_eq!(display.framebuffer()[last_idx], 0xFE);
    }

    #[test]
    fn draw_text_renders_bookerly_glyphs_with_spacing() {
        let mut display = new_display();

        display.clear(0xFF);
        display.draw_text(0, 0, "A");

        assert!(display.framebuffer().iter().any(|&byte| byte != 0xFF));
    }

    #[test]
    fn draw_text_accepts_utf8_text() {
        let mut display = new_display();

        display.clear(0xFF);
        display.draw_text(0, 0, "é");

        assert!(display.framebuffer().iter().any(|&byte| byte != 0xFF));
    }

    #[test]
    fn draw_wrapped_text_wraps_words_and_stops_at_the_requested_height() {
        let mut display = new_display();
        let line_height = bookerly::BOOKERLY.line_height_px();

        display.clear(0xFF);
        let next_y = display.draw_wrapped_text(
            0,
            0,
            "ALPHA BETA GAMMA DELTA EPSILON ZETA ETA THETA IOTA KAPPA ".repeat(20).as_str(),
            line_height * 2,
        );

        assert!(band_has_ink(&display, 0, line_height));
        assert!(band_has_ink(&display, line_height, line_height * 2));
        assert!(!band_has_ink(&display, line_height * 2, line_height * 3));
        assert_eq!(next_y, line_height * 2);
    }

    #[test]
    fn render_demo_splash_draws_some_text() {
        let mut display = new_display();

        display.clear(0xFF);
        display.render_embedded_epub_first_screen().unwrap();

        assert!(display.framebuffer().iter().any(|&byte| byte != 0xFF));
    }

    #[test]
    fn render_epub_page_can_target_later_pages() {
        let mut display = new_display();

        let rendered_page = display
            .render_epub_page(SliceSource { bytes: PAGINATION_EPUB_BYTES }, 1)
            .unwrap();

        assert!(rendered_page <= 1);
        assert!(display.framebuffer().iter().any(|&byte| byte != 0xFF));
    }

    #[test]
    fn demo_initializes_clears_renders_and_refreshes() {
        let mut display = DemoRecorder::default();

        show_embedded_epub_demo(&mut display).unwrap();

        assert_eq!(
            display.calls.as_slice(),
            &["init", "clear", "epub", "refresh"]
        );
    }

    #[test]
    fn init_performs_the_expected_reset_and_controller_sequence() {
        let mut display = new_display();

        display.init();

        assert_eq!(
            display.spi().writes,
            vec![
                vec![0x12],
                vec![0x18],
                vec![0x80],
                vec![0x0C],
                vec![0xAE],
                vec![0xC7],
                vec![0xC3],
                vec![0xC0],
                vec![0x40],
                vec![0x01],
                vec![0xDF],
                vec![0x01],
                vec![0x02],
                vec![0x3C],
                vec![0x01],
                vec![0x11],
                vec![0x01],
                vec![0x44],
                vec![0x00],
                vec![0x00],
                vec![0x1F],
                vec![0x03],
                vec![0x45],
                vec![0xDF],
                vec![0x01],
                vec![0x00],
                vec![0x00],
                vec![0x4E],
                vec![0x00],
                vec![0x00],
                vec![0x4F],
                vec![0xDF],
                vec![0x01],
            ]
        );
    }

    #[test]
    fn full_refresh_writes_both_rams_and_activates_the_panel() {
        let mut display = new_display();

        let _ = display.display_buffer(RefreshMode::Full, true);

        assert_eq!(
            display
                .spi()
                .writes
                .iter()
                .filter(|write| write.as_slice() == [0x24])
                .count(),
            1
        );
        assert_eq!(
            display
                .spi()
                .writes
                .iter()
                .filter(|write| write.as_slice() == [0x26])
                .count(),
            1
        );
        assert_eq!(
            display
                .spi()
                .writes
                .iter()
                .filter(|write| write.as_slice() == [0x21])
                .count(),
            1
        );
        assert_eq!(
            display
                .spi()
                .writes
                .iter()
                .filter(|write| write.as_slice() == [0x22])
                .count(),
            1
        );
        assert_eq!(
            display
                .spi()
                .writes
                .iter()
                .filter(|write| write.as_slice() == [0x20])
                .count(),
            1
        );
    }

    #[test]
    fn fast_refresh_updates_red_ram_after_refresh() {
        let mut display = new_display();

        let _ = display.display_buffer(RefreshMode::Fast, true);

        assert_eq!(
            display
                .spi()
                .writes
                .iter()
                .filter(|write| write.as_slice() == [0x24])
                .count(),
            1
        );
        assert_eq!(
            display
                .spi()
                .writes
                .iter()
                .filter(|write| write.as_slice() == [0x26])
                .count(),
            1
        );
    }

    #[test]
    fn deep_sleep_powers_down_an_awake_screen() {
        let mut display = new_display();

        let _ = display.display_buffer(RefreshMode::Full, true);
        display.deep_sleep();

        assert!(
            display
                .spi()
                .writes
                .iter()
                .any(|write| write.as_slice() == [0x10])
        );
    }

    fn new_display()
    -> SSD1677Display<FakeSpi, FakeOutputPin, FakeOutputPin, FakeInputPin, FakeDelay>
    {
        SSD1677Display::new(
            FakeSpi::default(),
            FakeOutputPin::default(),
            FakeOutputPin::default(),
            FakeInputPin::default(),
            FakeDelay::default(),
        )
    }

    fn logical_pixel_index(x: u16, y: u16) -> usize {
        let physical_y = DISPLAY_WIDTH - 1 - x;
        let physical_x = y;
        (physical_y as usize) * (DISPLAY_WIDTH_BYTES as usize) + (physical_x as usize / 8)
    }

    fn band_has_ink(
        display: &SSD1677Display<FakeSpi, FakeOutputPin, FakeOutputPin, FakeInputPin, FakeDelay>,
        y_start: u16,
        y_end: u16,
    ) -> bool {
        for y in y_start..y_end {
            for x in 0..DISPLAY_WIDTH {
                if display.framebuffer()[logical_pixel_index(x, y)] != 0xFF {
                    return true;
                }
            }
        }

        false
    }

    #[derive(Default)]
    struct DemoRecorder {
        calls: [&'static str; 4],
        len: usize,
    }

    impl DemoDisplay for DemoRecorder {
        type Error = core::convert::Infallible;

        fn init(&mut self) {
            self.calls[self.len] = "init";
            self.len += 1;
        }

        fn clear(&mut self, _color: u8) {
            self.calls[self.len] = "clear";
            self.len += 1;
        }

        fn render_embedded_epub_first_screen(&mut self) -> Result<(), Self::Error> {
            self.calls[self.len] = "epub";
            self.len += 1;
            Ok(())
        }

        fn refresh_full(&mut self) {
            self.calls[self.len] = "refresh";
            self.len += 1;
        }
    }
}
