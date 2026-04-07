use embedded_hal::spi::SpiDevice;
use xteink_display::SSD1677Display;
use xteink_epub::EpubError;
use xteink_epub::EpubSource;

use crate::{
    cache::{
        cache_paths_for_epub_candidates, parse_meta, serialize_meta, CacheMeta, CachePaths,
        CACHE_VERSION,
    },
    directory::ListedEntry,
    log::logln,
    low_level::{SdFilesystem, SdFsFile},
    path::join_child_path,
};

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
    render_epub_from_entry_with_cancel(fs, display, current_path, entry, || false)
}

pub fn render_epub_from_entry_with_cancel<SD, SPI, DC, RST, BUSY, DELAY, C>(
    fs: &SD,
    display: &mut SSD1677Display<SPI, DC, RST, BUSY, DELAY>,
    current_path: &str,
    entry: &ListedEntry,
    should_cancel: C,
) -> Result<EpubRenderResult, EpubError>
where
    SD: SdFilesystem,
    SPI: SpiDevice,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin,
    BUSY: embedded_hal::digital::InputPin,
    DELAY: embedded_hal::delay::DelayNs,
    C: FnMut() -> bool,
{
    logln!(
        "EPUB render start: path={} entry={} kind={:?}",
        current_path,
        entry.fs_name.as_str(),
        entry.kind
    );
    let cache_candidates = cache_paths_for_epub_candidates(current_path, entry.fs_name.as_str());
    logln!(
        "EPUB cache probe begin: {} | {}",
        cache_candidates[0].progress.as_str(),
        cache_candidates[1].progress.as_str()
    );
    let start_page = cache_candidates
        .iter()
        .find_map(|paths| {
            logln!("EPUB cache probe read begin: {}", paths.progress.as_str());
            let result = read_cached_progress(fs, paths);
            logln!(
                "EPUB cache probe read end: {} -> {:?}",
                paths.progress.as_str(),
                result
            );
            result
        })
        .unwrap_or(0);
    logln!(
        "EPUB render progress: cached_start_page={} cache_root={}",
        start_page,
        cache_candidates[0].directory.as_str()
    );

    render_epub_page_from_entry_with_cancel(
        fs,
        display,
        current_path,
        entry,
        start_page,
        false,
        should_cancel,
    )
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
    render_epub_page_from_entry_with_cancel(fs, display, current_path, entry, page_index, fast_refresh, || false)
}

pub fn render_epub_page_from_entry_with_cancel<SD, SPI, DC, RST, BUSY, DELAY, C>(
    fs: &SD,
    display: &mut SSD1677Display<SPI, DC, RST, BUSY, DELAY>,
    current_path: &str,
    entry: &ListedEntry,
    page_index: usize,
    fast_refresh: bool,
    should_cancel: C,
) -> Result<EpubRenderResult, EpubError>
where
    SD: SdFilesystem,
    SPI: SpiDevice,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin,
    BUSY: embedded_hal::digital::InputPin,
    DELAY: embedded_hal::delay::DelayNs,
    C: FnMut() -> bool,
{
    let source_path = join_child_path(current_path, entry.fs_name.as_str()).map_err(|_| EpubError::Io)?;
    let cache_paths = cache_paths_for_epub_candidates(current_path, entry.fs_name.as_str());
    let mut should_cancel = should_cancel;
    logln!("EPUB source open begin: {}", source_path.as_str());

    let source_size = u32::try_from(
        fs.open_epub_source(source_path.as_str())
            .map_err(|_| EpubError::Io)?
            .len(),
    )
    .map_err(|_| EpubError::OutOfSpace)?;
    logln!(
        "EPUB source opened: {} bytes={} target_page={} fast_refresh={}",
        source_path.as_str(),
        source_size,
        page_index,
        fast_refresh
    );

    let mut cache_paths_for_work: Option<CachePaths> = None;
    let rendered_page = if let Some((paths, meta)) = select_valid_cache(fs, &cache_paths, source_size) {
        cache_paths_for_work = Some(paths);
        logln!(
            "EPUB cache hit: {} v{} source_size={} content_length={}",
            source_path.as_str(),
            meta.version,
            meta.source_size,
            meta.content_length
        );
        let paths = cache_paths_for_work
            .as_ref()
            .cloned()
            .unwrap_or_else(|| cache_paths[0].clone());
        let mut content = fs
            .open_cache_file_read(paths.content.as_str())
            .map_err(|_| EpubError::Io)?;
        let mut read_text = |buffer: &mut [u8]| -> Result<usize, EpubError> {
            content.read(buffer).map_err(|_| EpubError::Io)
        };
        display.render_cached_text_page_with_cancel(&mut read_text, page_index, &mut should_cancel)?
    } else {
        logln!("EPUB cache miss, parsing source: {}", source_path.as_str());
        let source = fs.open_epub_source(source_path.as_str()).map_err(|_| EpubError::Io)?;
        let chosen = choose_cache_root(fs, &cache_paths).or_else(|| cache_paths.first().cloned());
        if let Some(chosen) = chosen {
            cache_paths_for_work = Some(chosen);
        }
        let cache_paths_for_render = cache_paths_for_work
            .as_ref()
            .cloned()
            .unwrap_or_else(|| cache_paths[0].clone());
        logln!(
            "EPUB parse start: source={} cache_dir={}",
            source_path.as_str(),
            cache_paths_for_render.directory.as_str()
        );
        let rendered_page = read_epub_source_to_cache_and_render(
            fs,
            display,
            source,
            source_size,
            source_path.as_str(),
            &cache_paths_for_render,
            page_index,
            &mut should_cancel,
        )?;
        logln!(
            "EPUB parse complete: source={} rendered_page={}",
            source_path.as_str(),
            rendered_page
        );
        rendered_page
    };

    if let Some(paths) = cache_paths_for_work {
        let _ = write_cached_progress(fs, paths.progress.as_str(), rendered_page);
    }

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

fn select_valid_cache<SD: SdFilesystem>(
    fs: &SD,
    candidates: &[CachePaths; 2],
    source_size: u32,
) -> Option<(CachePaths, CacheMeta)> {
    for paths in candidates.iter() {
        if let Some(meta) = read_cache_meta_if_valid(fs, paths, source_size) {
            return Some((paths.clone(), meta));
        }
    }
    None
}

fn choose_cache_root<SD: SdFilesystem>(fs: &SD, candidates: &[CachePaths; 2]) -> Option<CachePaths> {
    candidates
        .iter()
        .find(|paths| fs.ensure_directory(paths.directory.as_str()).is_ok())
        .cloned()
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
    should_cancel: &mut impl FnMut() -> bool,
) -> Result<usize, EpubError>
where
    SD: SdFilesystem,
    SPI: SpiDevice,
    DC: embedded_hal::digital::OutputPin,
    RST: embedded_hal::digital::OutputPin,
    BUSY: embedded_hal::digital::InputPin,
    DELAY: embedded_hal::delay::DelayNs,
{
    logln!(
        "EPUB cache write start: directory={} content={}",
        paths.directory.as_str(),
        paths.content.as_str()
    );
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

    let rendered_page = display.render_epub_page_with_text_sink_and_cancel(
        source,
        page_index,
        &mut on_text_chunk,
        true,
        should_cancel,
    )?;

    if let Some(mut file) = content {
        let _ = file.flush();
        if write_meta(fs, paths, source_size, content_length).is_ok() {
            logln!(
                "EPUB parsed and cached: {} -> bytes={} content_len={}",
                source_path,
                source_size,
                content_length
            );
        }
    }

    Ok(rendered_page)
}
