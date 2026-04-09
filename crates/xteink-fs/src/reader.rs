use xteink_epub::EpubError;
use xteink_epub::EpubSource;
use xteink_render::{CacheBuildResult, Framebuffer};

use crate::{
    cache::{
        cache_paths_for_epub, parse_meta, serialize_meta, CacheMeta, CachePaths, CACHE_VERSION,
    },
    directory::ListedEntry,
    log::logln,
    low_level::{SdFilesystem, SdFsFile},
    path::join_child_path,
};

const CACHED_TEXT_WRITE_BUFFER: usize = 1024;

struct CacheWriteBuffer {
    bytes: [u8; CACHED_TEXT_WRITE_BUFFER],
    len: usize,
    total_written: usize,
}

impl CacheWriteBuffer {
    fn new() -> Self {
        Self {
            bytes: [0; CACHED_TEXT_WRITE_BUFFER],
            len: 0,
            total_written: 0,
        }
    }

    fn total_written(&self) -> usize {
        self.total_written
    }

    fn push<F: SdFsFile>(&mut self, file: &mut F, chunk: &[u8]) -> Result<(), EpubError> {
        let mut remaining = chunk;
        while !remaining.is_empty() {
            if self.len == self.bytes.len() {
                self.flush(file)?;
            }
            let capacity = self.bytes.len().saturating_sub(self.len);
            let copy_len = capacity.min(remaining.len());
            self.bytes[self.len..self.len + copy_len].copy_from_slice(&remaining[..copy_len]);
            self.len += copy_len;
            remaining = &remaining[copy_len..];
        }
        Ok(())
    }

    fn flush<F: SdFsFile>(&mut self, file: &mut F) -> Result<(), EpubError> {
        if self.len == 0 {
            return Ok(());
        }
        let mut written = 0usize;
        while written < self.len {
            let count = file
                .write(&self.bytes[written..self.len])
                .map_err(|_| EpubError::Io)?;
            if count == 0 {
                return Err(EpubError::Io);
            }
            written = written.saturating_add(count);
        }
        self.total_written = self.total_written.saturating_add(self.len);
        self.len = 0;
        Ok(())
    }
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
    pub progress_percent: u8,
}

pub fn render_epub_from_entry<SD>(
    fs: &SD,
    display: &mut Framebuffer,
    current_path: &str,
    entry: &ListedEntry,
) -> Result<EpubRenderResult, EpubError>
where
    SD: SdFilesystem,
{
    render_epub_from_entry_with_cancel(fs, display, current_path, entry, || false)
}

pub fn render_epub_from_entry_with_cancel<SD, C>(
    fs: &SD,
    display: &mut Framebuffer,
    current_path: &str,
    entry: &ListedEntry,
    should_cancel: C,
) -> Result<EpubRenderResult, EpubError>
where
    SD: SdFilesystem,
    C: FnMut() -> bool,
{
    logln!(
        "EPUB render start: path={} entry={} kind={:?}",
        current_path,
        entry.fs_name.as_str(),
        entry.kind
    );
    logln!("EPUB render step: building cache candidate paths");
    let cache_paths = cache_paths_for_epub(current_path, entry.fs_name.as_str());
    logln!("EPUB render step: built cache candidate paths");
    logln!(
        "EPUB render step: cache candidate lengths {} | {}",
        cache_paths.progress.len(),
        cache_paths.progress.len()
    );
    logln!(
        "EPUB cache probe begin: {}",
        cache_paths.progress.as_str()
    );
    let cache_state = resolve_cache_state(fs, &cache_paths, 0, None);
    let saved_progress = cache_state.start_page;
    let start_page = saved_progress.map(|progress| progress.page).unwrap_or(0);
    logln!(
        "EPUB render progress: cached_start_page={} cache_root={}",
        start_page,
        cache_paths.directory.as_str()
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

pub fn render_epub_page_from_entry<SD>(
    fs: &SD,
    display: &mut Framebuffer,
    current_path: &str,
    entry: &ListedEntry,
    page_index: usize,
    fast_refresh: bool,
) -> Result<EpubRenderResult, EpubError>
where
    SD: SdFilesystem,
{
    render_epub_page_from_entry_with_cancel(
        fs,
        display,
        current_path,
        entry,
        page_index,
        fast_refresh,
        || false,
    )
}

pub fn render_epub_page_from_entry_with_cancel<SD, C>(
    fs: &SD,
    display: &mut Framebuffer,
    current_path: &str,
    entry: &ListedEntry,
    page_index: usize,
    fast_refresh: bool,
    should_cancel: C,
) -> Result<EpubRenderResult, EpubError>
where
    SD: SdFilesystem,
    C: FnMut() -> bool,
{
    let source_path =
        join_child_path(current_path, entry.fs_name.as_str()).map_err(|_| EpubError::Io)?;
    let cache_paths = cache_paths_for_epub(current_path, entry.fs_name.as_str());
    let mut should_cancel = should_cancel;
    logln!("EPUB source open begin: {}", source_path.as_str());

    let mut source = Some(
        fs.open_epub_source(source_path.as_str())
            .map_err(|_| EpubError::Io)?,
    );
    let source_size =
        u32::try_from(source.as_ref().ok_or(EpubError::Io)?.len()).map_err(|_| EpubError::OutOfSpace)?;
    logln!(
        "EPUB source opened: {} bytes={} target_page={} fast_refresh={}",
        source_path.as_str(),
        source_size,
        page_index,
        fast_refresh
    );

    let saved_progress = read_cached_progress(fs, &cache_paths);
    let cache_state = resolve_cache_state(fs, &cache_paths, source_size, Some(page_index));
    let mut cache_paths_for_work = cache_state.work_paths;
    let (rendered_page, progress_percent) = match cache_state.selection {
        CacheSelection::Hit { paths, meta: _meta } => {
            cache_paths_for_work = Some(paths.clone());
            logln!(
                "EPUB cache hit: {} v{} source_size={} content_length={}",
                source_path.as_str(),
                _meta.version,
                _meta.source_size,
                _meta.content_length
            );
            let mut content = fs
                .open_cache_file_read(paths.content.as_str())
                .map_err(|_| EpubError::Io)?;
            let mut read_text = |buffer: &mut [u8]| -> Result<usize, EpubError> {
                content.read(buffer).map_err(|_| EpubError::Io)
            };
            let rendered_page = display.render_cached_text_page_with_cancel(
                &mut read_text,
                page_index,
                &mut should_cancel,
            )?;
            let progress_percent = if _meta.complete {
                percent_from_pages(rendered_page, _meta.cached_pages)
            } else if let Some(saved) = saved_progress {
                estimate_percent_with_saved_progress(rendered_page, saved)
            } else {
                estimate_percent_from_cached_prefix(rendered_page, _meta.cached_pages)
            };
            (rendered_page, progress_percent)
        }
        CacheSelection::Resume { paths, meta: _meta } => {
            cache_paths_for_work = Some(paths.clone());
            logln!(
                "EPUB partial cache rebuild: {} v{} cached_pages={} next_spine_index={} resume_page={} resume_cursor_y={}",
                source_path.as_str(),
                _meta.version,
                _meta.cached_pages,
                _meta.next_spine_index,
                _meta.resume_page,
                _meta.resume_cursor_y
            );
            let source = source.take().ok_or(EpubError::Io)?;
            let build = read_epub_source_to_cache_and_render(
                fs,
                display,
                source,
                source_size,
                source_path.as_str(),
                &paths,
                page_index,
                &mut should_cancel,
            )?;
            (build.rendered_page, build.progress_percent)
        }
        CacheSelection::Miss { paths } => {
            logln!("EPUB cache miss, parsing source: {}", source_path.as_str());
            let source = source.take().ok_or(EpubError::Io)?;
            if let Some(paths) = paths {
                cache_paths_for_work = Some(paths);
            }
            let cache_paths_for_render = cache_paths_for_work
                .as_ref()
                .cloned()
                .unwrap_or_else(|| cache_paths.clone());
            logln!(
                "EPUB parse start: source={} cache_dir={}",
                source_path.as_str(),
                cache_paths_for_render.directory.as_str()
            );
            let build = read_epub_source_to_cache_and_render(
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
                build.rendered_page
            );
            (build.rendered_page, build.progress_percent)
        }
    };

    if let Some(paths) = cache_paths_for_work {
        if let Err(_err) = write_cached_progress(
            fs,
            paths.progress.as_str(),
            rendered_page,
            progress_percent,
        ) {
            logln!(
                "EPUB progress write failed: {} -> {:?}",
                paths.progress.as_str(),
                _err
            );
        }
    }

    Ok(EpubRenderResult {
        rendered_page,
        refresh: if fast_refresh {
            EpubRefreshMode::Fast
        } else {
            EpubRefreshMode::Full
        },
        progress_percent,
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

#[derive(Debug, Clone)]
struct CacheProbe {
    start_page: Option<SavedProgress>,
    selection: CacheSelection,
    work_paths: Option<CachePaths>,
}

#[derive(Debug, Clone)]
enum CacheSelection {
    Hit { paths: CachePaths, meta: CacheMeta },
    Resume { paths: CachePaths, meta: CacheMeta },
    Miss { paths: Option<CachePaths> },
}

fn resolve_cache_state<SD: SdFilesystem>(
    fs: &SD,
    paths: &CachePaths,
    source_size: u32,
    page_index: Option<usize>,
) -> CacheProbe {
    if page_index.is_none() {
        return CacheProbe {
            start_page: read_cached_progress(fs, paths),
            selection: CacheSelection::Miss { paths: None },
            work_paths: None,
        };
    }

    let selection = if let Some(meta) = read_cache_meta_if_valid(fs, paths, source_size) {
        let cached_pages = usize::try_from(meta.cached_pages).ok();
        match (page_index, cached_pages) {
            (Some(page_index), Some(cached_pages)) if meta.complete || page_index < cached_pages => {
                CacheSelection::Hit {
                    paths: paths.clone(),
                    meta,
                }
            }
            (Some(_), Some(_)) if !meta.complete => CacheSelection::Resume {
                paths: paths.clone(),
                meta,
            },
            _ => CacheSelection::Miss {
                paths: Some(paths.clone()),
            },
        }
    } else {
        CacheSelection::Miss {
            paths: Some(paths.clone()),
        }
    };

    let work_paths = match &selection {
        CacheSelection::Hit { paths, .. } | CacheSelection::Resume { paths, .. } => {
            Some(paths.clone())
        }
        CacheSelection::Miss { paths } => paths.clone(),
    };

    CacheProbe {
        start_page: None,
        selection,
        work_paths,
    }
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

#[derive(Debug, Clone, Copy)]
struct SavedProgress {
    page: usize,
    progress_percent: u8,
}

fn read_cached_progress<SD: SdFilesystem>(fs: &SD, paths: &CachePaths) -> Option<SavedProgress> {
    let mut file = fs.open_cache_file_read(paths.progress.as_str()).ok()?;
    let mut raw = [0u8; 5];
    let mut total = 0usize;
    while total < raw.len() {
        let n = file.read(&mut raw[total..]).ok()?;
        if n == 0 {
            break;
        }
        total += n;
    }
    if total < 4 {
        return None;
    }
    let page = usize::try_from(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]])).ok()?;
    let progress_percent = if total >= 5 { raw[4] } else { 0 };
    Some(SavedProgress {
        page,
        progress_percent,
    })
}

fn write_cached_progress<SD: SdFilesystem>(
    fs: &SD,
    path: &str,
    page: usize,
    progress_percent: u8,
) -> Result<(), EpubError> {
    let page = u32::try_from(page).map_err(|_| EpubError::OutOfSpace)?;
    let mut raw = [0u8; 5];
    raw[..4].copy_from_slice(&page.to_le_bytes());
    raw[4] = progress_percent;
    write_bytes(fs, path, &raw)
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
    build: CacheBuildResult,
) -> Result<(), EpubError> {
    let meta = CacheMeta {
        version: CACHE_VERSION,
        source_size,
        content_length: u32::try_from(content_length).map_err(|_| EpubError::OutOfSpace)?,
        cached_pages: u32::try_from(build.cached_pages).map_err(|_| EpubError::OutOfSpace)?,
        next_spine_index: build.next_spine_index,
        resume_page: u32::try_from(build.resume_page).map_err(|_| EpubError::OutOfSpace)?,
        resume_cursor_y: build.resume_cursor_y,
        complete: build.complete,
    };
    let serialized = serialize_meta(&meta, source_size);
    write_bytes(fs, paths.meta.as_str(), serialized.as_bytes())
}

fn read_epub_source_to_cache_and_render<SD>(
    fs: &SD,
    display: &mut Framebuffer,
    source: SD::EpubSource<'_>,
    source_size: u32,
    _source_path: &str,
    paths: &CachePaths,
    page_index: usize,
    should_cancel: &mut impl FnMut() -> bool,
) -> Result<CacheBuildResult, EpubError>
where
    SD: SdFilesystem,
{
    logln!(
        "EPUB cache write start: directory={} content={}",
        paths.directory.as_str(),
        paths.content.as_str()
    );
    if let Err(_err) = fs.ensure_directory(paths.directory.as_str()) {
        logln!(
            "EPUB cache ensure_directory failed: {} -> {:?}",
            paths.directory.as_str(),
            _err
        );
    }
    let mut content = match fs.open_cache_file_write(paths.content.as_str()) {
        Ok(file) => {
            logln!("EPUB cache content open ok: {}", paths.content.as_str());
            Some(file)
        }
        Err(_err) => {
            logln!(
                "EPUB cache content open failed: {} -> {:?}",
                paths.content.as_str(),
                _err
            );
            None
        }
    };
    let mut content_buffer = CacheWriteBuffer::new();

    let mut on_text_chunk = |chunk: &str| -> Result<(), EpubError> {
        if let Some(file) = content.as_mut() {
            if !chunk.is_empty() {
                content_buffer.push(file, chunk.as_bytes())?;
            }
        }
        Ok(())
    };

    let build = display.build_epub_cache_prefix_with_text_sink_and_cancel(
        source,
        page_index,
        &mut on_text_chunk,
        should_cancel,
    )?;

    if let Some(mut file) = content {
        content_buffer.flush(&mut file)?;
        let _ = file.flush();
        let content_length = content_buffer.total_written();
        if write_meta(fs, paths, source_size, content_length, build).is_ok() {
            logln!(
                "EPUB parsed and cached: {} -> bytes={} content_len={} cached_pages={} next_spine_index={} complete={}",
                source_path,
                source_size,
                content_length,
                build.cached_pages,
                build.next_spine_index,
                build.complete
            );
        } else {
            logln!(
                "EPUB cache meta write failed: {} content_len={}",
                paths.meta.as_str(),
                content_length
            );
        }
    }

    Ok(build)
}

fn percent_from_pages(rendered_page: usize, total_pages: u32) -> u8 {
    if total_pages == 0 {
        return 0;
    }
    let progress = ((rendered_page.saturating_add(1)) * 100) / usize::try_from(total_pages).unwrap_or(1);
    progress.clamp(1, 100) as u8
}

fn estimate_percent_from_cached_prefix(rendered_page: usize, cached_pages: u32) -> u8 {
    if cached_pages == 0 {
        return 0;
    }
    let progress = ((rendered_page.saturating_add(1)) * 99) / usize::try_from(cached_pages).unwrap_or(1);
    progress.clamp(1, 99) as u8
}

fn estimate_percent_with_saved_progress(rendered_page: usize, saved: SavedProgress) -> u8 {
    if saved.progress_percent == 0 {
        return 0;
    }
    if rendered_page >= saved.page {
        return saved.progress_percent;
    }
    let scaled = ((rendered_page.saturating_add(1)) * usize::from(saved.progress_percent))
        / saved.page.saturating_add(1);
    scaled.clamp(1, usize::from(saved.progress_percent)) as u8
}
