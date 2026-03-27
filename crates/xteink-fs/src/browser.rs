use embedded_hal::spi::SpiDevice;
use xteink_display::SSD1677Display;
use xteink_epub::EpubError;

use crate::{low_level::{self, DirectoryPageInfo, FsError, SdFilesystem}, path::join_child_path};

pub const MAX_ENTRIES: usize = low_level::MAX_ENTRIES;
pub use crate::low_level::ListedEntry;

#[derive(Debug)]
pub struct DirectoryPage {
    pub entries: heapless::Vec<ListedEntry, MAX_ENTRIES>,
    pub info: DirectoryPageInfo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EpubRefreshMode {
    Full,
    Fast,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EpubRenderResult {
    pub rendered_page: usize,
    pub refresh: EpubRefreshMode,
}

pub fn load_directory_page<SD: SdFilesystem>(
    fs: &SD,
    current_path: &str,
    page_start: usize,
    page_size: usize,
) -> Result<DirectoryPage, FsError> {
    let mut entries: heapless::Vec<ListedEntry, MAX_ENTRIES> = heapless::Vec::new();
    let info = fs.list_directory_page(current_path, page_start, page_size, &mut entries)?;
    Ok(DirectoryPage { entries, info })
}

pub fn render_epub_from_entry<SD, SPI, DC, RST, BUSY, DELAY>(
    fs: &SD,
    display: &mut SSD1677Display<SPI, DC, RST, BUSY, DELAY>,
    current_path: &str,
    entry: &ListedEntry,
) -> Result<EpubRefreshMode, EpubError>
where
    SD: SdFilesystem,
    SPI: SpiDevice,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin,
    BUSY: embedded_hal::digital::InputPin,
    DELAY: embedded_hal::delay::DelayNs,
{
    render_epub_page_from_entry(fs, display, current_path, entry, 0, false).map(|result| result.refresh)
}

pub fn render_epub_page_from_entry<SD, SPI, DC, RST, BUSY, DELAY>(
    fs: &SD,
    display: &mut SSD1677Display<SPI, DC, RST, BUSY, DELAY>,
    current_path: &str,
    entry: &ListedEntry,
    page_index: usize,
    fast_refresh: bool,
) -> Result<EpubRenderResult, EpubError>
where
    SD: SdFilesystem,
    SPI: SpiDevice,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin,
    BUSY: embedded_hal::digital::InputPin,
    DELAY: embedded_hal::delay::DelayNs,
{
    let path = join_child_path(current_path, entry.fs_name.as_str()).map_err(|_| EpubError::Io)?;
    esp_println::println!("EPUB open start: {}", path.as_str());
    let source = fs.open_epub_source(path.as_str()).map_err(|_| EpubError::Io)?;
    esp_println::println!("EPUB source opened: {}", path.as_str());

    esp_println::println!(
        "EPUB render page start: {} target_page={} fast_refresh={}",
        path.as_str(),
        page_index,
        fast_refresh
    );
    let rendered_page = display.render_epub_page(source, page_index)?;
    esp_println::println!(
        "EPUB render page complete: {} rendered_page={}",
        path.as_str(),
        rendered_page
    );
    let refresh = if fast_refresh {
        EpubRefreshMode::Fast
    } else {
        EpubRefreshMode::Full
    };
    Ok(EpubRenderResult {
        rendered_page,
        refresh,
    })
}
