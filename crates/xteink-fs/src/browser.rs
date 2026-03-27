use embedded_hal::spi::SpiDevice;
use xteink_display::SSD1677Display;
use xteink_epub::EpubError;
use xteink_epub::EpubSource;

use crate::{
    cache::{cache_paths_for_epub, parse_meta, serialize_meta, CacheMeta, CachePaths, CACHE_VERSION},
    low_level::{self, DirectoryPageInfo, FsError, SdFilesystem, SdFsFile},
    path::join_child_path,
};

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
) -> Result<EpubRenderResult, EpubError>
where
    SD: SdFilesystem,
    SPI: SpiDevice,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin,
    BUSY: embedded_hal::digital::InputPin,
    DELAY: embedded_hal::delay::DelayNs,
{
    let cache_paths = cache_paths_for_epub(current_path, entry.fs_name.as_str());
    let start_page = read_cached_progress(fs, &cache_paths).unwrap_or(0);

    let result = render_epub_page_from_entry(fs, display, current_path, entry, start_page, false)?;
    let _ = write_cached_progress(fs, cache_paths.progress.as_str(), result.rendered_page);
    Ok(result)
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
    let source_path = join_child_path(current_path, entry.fs_name.as_str()).map_err(|_| EpubError::Io)?;
    let paths = cache_paths_for_epub(current_path, entry.fs_name.as_str());

    let source_size = u32::try_from(
        fs.open_epub_source(source_path.as_str())
            .map_err(|_| EpubError::Io)?
            .len(),
    )
    .map_err(|_| EpubError::OutOfSpace)?;

    let rendered_page = if let Some(meta) = read_cache_meta_if_valid(fs, &paths, source_size) {
        let _ = esp_println::println!(
            "EPUB cache hit: {} v{} source_size={} content_length={}",
            source_path.as_str(),
            meta.version,
            meta.source_size,
            meta.content_length
        );
        let mut content = fs
            .open_cache_file_read(paths.content.as_str())
            .map_err(|_| EpubError::Io)?;
        let mut read_text = |buffer: &mut [u8]| -> Result<usize, EpubError> {
            content.read(buffer).map_err(|_| EpubError::Io)
        };
        display.render_cached_text_page(&mut read_text, page_index)?
    } else {
        let _ = esp_println::println!("EPUB cache miss, parsing source: {}", source_path.as_str());
        let source = fs.open_epub_source(source_path.as_str()).map_err(|_| EpubError::Io)?;
        let rendered_page = read_epub_source_to_cache_and_render(
            fs,
            display,
            source,
            source_size,
            source_path.as_str(),
            &paths,
            page_index,
        )?;
        rendered_page
    };

    let _ = write_cached_progress(fs, paths.progress.as_str(), rendered_page);

    Ok(EpubRenderResult {
        rendered_page,
        refresh: if fast_refresh {
            EpubRefreshMode::Fast
        } else {
            EpubRefreshMode::Full
        },
    })
}

fn read_cache_meta_if_valid<SD: SdFilesystem>(
    fs: &SD,
    paths: &CachePaths,
    source_size: u32,
) -> Option<CacheMeta> {
    let meta = read_cache_meta(fs, paths.meta.as_str()).ok()?;
    if meta.version != CACHE_VERSION || meta.source_size != source_size {
        return None;
    }

    let content = fs.open_cache_file_read(paths.content.as_str()).ok()?;
    if content.len() != meta.content_length as usize {
        return None;
    }
    Some(meta)
}

fn read_cache_meta<SD: SdFilesystem>(fs: &SD, path: &str) -> Result<CacheMeta, EpubError> {
    let mut file = fs.open_cache_file_read(path).map_err(|_| EpubError::Io)?;
    let mut raw = [0u8; 256];
    let mut total = 0usize;

    loop {
        if total >= raw.len() {
            return Err(EpubError::OutOfSpace);
        }
        let n = file.read(&mut raw[total..]).map_err(|_| EpubError::Io)?;
        if n == 0 {
            break;
        }
        total += n;
    }

    parse_meta(core::str::from_utf8(&raw[..total]).map_err(|_| EpubError::Utf8)?)
        .ok_or(EpubError::InvalidFormat)
}

fn read_cached_progress<SD: SdFilesystem>(fs: &SD, paths: &CachePaths) -> Option<usize> {
    read_u32_le(fs, paths.progress.as_str())
        .ok()
        .flatten()
        .and_then(|value| usize::try_from(value).ok())
}

fn write_cached_progress<SD: SdFilesystem>(fs: &SD, path: &str, page: usize) -> Result<(), EpubError> {
    write_u32_le(fs, path, u32::try_from(page).map_err(|_| EpubError::OutOfSpace)?)
}

fn read_u32_le<SD: SdFilesystem>(fs: &SD, path: &str) -> Result<Option<u32>, EpubError> {
    let mut file = match fs.open_cache_file_read(path) {
        Ok(file) => file,
        Err(_) => return Ok(None),
    };
    let mut raw = [0u8; 4];
    let mut total = 0usize;
    while total < raw.len() {
        let n = file.read(&mut raw[total..]).map_err(|_| EpubError::Io)?;
        if n == 0 {
            return Ok(None);
        }
        total += n;
    }
    Ok(Some(u32::from_le_bytes(raw)))
}

fn write_u32_le<SD: SdFilesystem>(fs: &SD, path: &str, value: u32) -> Result<(), EpubError> {
    write_bytes(fs, path, &value.to_le_bytes())
}

fn write_bytes<SD: SdFilesystem>(fs: &SD, path: &str, data: &[u8]) -> Result<(), EpubError> {
    let mut file = fs.open_cache_file_write(path).map_err(|_| EpubError::Io)?;
    let mut written = 0usize;
    while written < data.len() {
        let count = file.write(&data[written..]).map_err(|_| EpubError::Io)?;
        if count == 0 {
            return Err(EpubError::Io);
        }
        written = written.saturating_add(count);
    }
    file.flush().map_err(|_| EpubError::Io)?;
    Ok(())
}

fn write_meta<SD: SdFilesystem>(
    fs: &SD,
    paths: &CachePaths,
    source_size: u32,
    content_length: usize,
) -> Result<(), EpubError> {
    let meta = CacheMeta {
        version: CACHE_VERSION,
        source_size,
        content_length: u32::try_from(content_length).map_err(|_| EpubError::OutOfSpace)?,
    };
    let serialized = serialize_meta(&meta, source_size);
    write_bytes(fs, paths.meta.as_str(), serialized.as_bytes())
}

fn read_epub_source_to_cache_and_render<SD, SPI, DC, RST, BUSY, DELAY>(
    fs: &SD,
    display: &mut SSD1677Display<SPI, DC, RST, BUSY, DELAY>,
    source: SD::EpubSource<'_>,
    source_size: u32,
    source_path: &str,
    paths: &CachePaths,
    page_index: usize,
) -> Result<usize, EpubError>
where
    SD: SdFilesystem,
    SPI: SpiDevice,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin,
    BUSY: embedded_hal::digital::InputPin,
    DELAY: embedded_hal::delay::DelayNs,
{
    let _ = fs.ensure_directory(paths.directory.as_str());
    let mut content = fs.open_cache_file_write(paths.content.as_str()).ok();
    let mut content_length = 0usize;

    let mut on_text_chunk = |chunk: &str| -> Result<(), EpubError> {
        if let Some(file) = content.as_mut() {
            if !chunk.is_empty() {
                let written = file.write(chunk.as_bytes()).map_err(|_| EpubError::Io)?;
                if written != chunk.len() {
                    return Err(EpubError::Io);
                }
                content_length = content_length.saturating_add(written);
            }
        }
        Ok(())
    };

    let rendered_page = display.render_epub_page_with_text_sink(
        source,
        page_index,
        &mut on_text_chunk,
        true,
    )?;

    if let Some(mut file) = content {
        let _ = file.flush();
        if write_meta(fs, paths, source_size, content_length).is_ok() {
            let _ = esp_println::println!(
                "EPUB parsed and cached: {} -> bytes={} content_len={}",
                source_path,
                source_size,
                content_length
            );
        }
    }

    Ok(rendered_page)
}
