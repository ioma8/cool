use xteink_epub::{EpubError, EpubSource};
use xteink_render::{DISPLAY_HEIGHT, DISPLAY_WIDTH, Framebuffer, reader_content_height};

use crate::{
    ListedEntry, SdFilesystem, SdFsFile,
    cache::{
        CACHE_VERSION, CacheMeta, CachePaths, ProgressState, cache_paths_for_epub, decode_progress,
        encode_offset, encode_progress, parse_meta, serialize_meta,
    },
    log::logln,
    path::join_child_path,
};

const CACHED_TEXT_WRITE_BUFFER: usize = 1024;
const LAYOUT_SIG_VERSION: u16 = 1;
const LAYOUT_SIG_FONT: u32 = 1;
const LAYOUT_SIG_PAGINATOR: u32 = 2;

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
    let source_path =
        join_child_path(current_path, entry.fs_name.as_str()).map_err(|_| EpubError::Io)?;
    let source = fs
        .open_epub_source(source_path.as_str())
        .map_err(|_| EpubError::Io)?;
    let source_size = u32::try_from(source.len()).map_err(|_| EpubError::OutOfSpace)?;
    let cache_paths = cache_paths_for_epub(current_path, entry.fs_name.as_str());
    let saved = read_cached_progress(fs, cache_paths.progress.as_str());

    render_epub_page_from_entry_with_source_and_cancel(
        fs,
        display,
        current_path,
        entry,
        source_size,
        Some(source),
        saved
            .and_then(|progress| usize::try_from(progress.current_page_start_offset).ok())
            .unwrap_or(0),
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
    let source_path =
        join_child_path(current_path, entry.fs_name.as_str()).map_err(|_| EpubError::Io)?;
    let source_size = u32::try_from(
        fs.open_epub_source(source_path.as_str())
            .map_err(|_| EpubError::Io)?
            .len(),
    )
    .map_err(|_| EpubError::OutOfSpace)?;
    render_epub_page_from_entry_with_source_and_cancel(
        fs,
        display,
        current_path,
        entry,
        source_size,
        None,
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
    source_size: u32,
    page_index: usize,
    fast_refresh: bool,
    should_cancel: C,
) -> Result<EpubRenderResult, EpubError>
where
    SD: SdFilesystem,
    C: FnMut() -> bool,
{
    render_epub_page_from_entry_with_source_and_cancel(
        fs,
        display,
        current_path,
        entry,
        source_size,
        None,
        page_index,
        fast_refresh,
        should_cancel,
    )
}

fn render_epub_page_from_entry_with_source_and_cancel<'a, SD, C>(
    fs: &'a SD,
    display: &mut Framebuffer,
    current_path: &str,
    entry: &ListedEntry,
    source_size: u32,
    mut source_for_build: Option<SD::EpubSource<'a>>,
    page_index: usize,
    fast_refresh: bool,
    mut should_cancel: C,
) -> Result<EpubRenderResult, EpubError>
where
    SD: SdFilesystem,
    C: FnMut() -> bool,
{
    let source_path =
        join_child_path(current_path, entry.fs_name.as_str()).map_err(|_| EpubError::Io)?;
    let cache_paths = cache_paths_for_epub(current_path, entry.fs_name.as_str());

    let mut meta = read_cache_meta_if_valid(fs, &cache_paths, source_size).ok();
    if meta.is_none() {
        logln!("EPUB cache miss, parsing source: {}", source_path.as_str());
        let source = if let Some(source) = source_for_build.take() {
            source
        } else {
            fs.open_epub_source(source_path.as_str())
                .map_err(|_| EpubError::Io)?
        };
        let build = build_content_cache(
            fs,
            display,
            source,
            source_size,
            &cache_paths,
            page_index,
            &mut should_cancel,
        )?;
        let new_meta = CacheMeta {
            version: CACHE_VERSION,
            source_size,
            content_length: u64::try_from(build.content_length)
                .map_err(|_| EpubError::OutOfSpace)?,
            build_complete: build.complete,
            next_chapter_index: build.next_chapter_index,
            layout_sig_version: LAYOUT_SIG_VERSION,
            layout_sig_width: DISPLAY_WIDTH,
            layout_sig_height: DISPLAY_HEIGHT,
            layout_sig_content_height: reader_content_height(),
            layout_sig_font: LAYOUT_SIG_FONT,
            layout_sig_paginator: LAYOUT_SIG_PAGINATOR,
        };
        write_meta(fs, cache_paths.meta.as_str(), &new_meta)?;
        meta = Some(new_meta);
    }

    let meta = meta.ok_or(EpubError::Io)?;
    let saved = read_cached_progress(fs, cache_paths.progress.as_str());

    let saved_offset = saved
        .map(|progress| progress.current_page_start_offset)
        .filter(|offset| *offset <= meta.content_length)
        .unwrap_or(0);

    let requested_offset = u64::try_from(page_index).unwrap_or(u64::MAX);
    let use_saved_offset = !fast_refresh && requested_offset == saved_offset;

    let (rendered_page, page_start_offset, next_page_offset, previous_page_offset) =
        if use_saved_offset {
            let rendered_page = count_pages_before_offset(
                fs,
                cache_paths.content.as_str(),
                saved_offset,
                &mut should_cancel,
            )?;
            let (_, next_page_offset) = render_cached_page_at_offset(
                fs,
                display,
                cache_paths.content.as_str(),
                saved_offset,
                &mut should_cancel,
            )?;
            (
                rendered_page,
                saved_offset,
                next_page_offset,
                saved
                    .map(|progress| progress.previous_page_start_offset)
                    .filter(|offset| *offset <= saved_offset)
                    .unwrap_or(0),
            )
        } else {
            let (rendered_page, page_start_offset, next_page_offset) = render_cached_page(
                fs,
                display,
                cache_paths.content.as_str(),
                page_index,
                &mut should_cancel,
            )?;
            let previous_page_offset = page_start_offset_for_page(
                fs,
                cache_paths.content.as_str(),
                rendered_page.saturating_sub(1),
                &mut should_cancel,
            )?;
            (
                rendered_page,
                page_start_offset,
                next_page_offset,
                previous_page_offset,
            )
        };

    let progress_percent =
        compute_progress_percent(page_start_offset, next_page_offset, meta.content_length);

    write_cached_progress(
        fs,
        cache_paths.progress.as_str(),
        ProgressState {
            previous_page_start_offset: previous_page_offset,
            current_page_start_offset: page_start_offset,
            next_page_start_offset: next_page_offset,
        },
    )?;

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

fn compute_progress_percent(current_offset: u64, next_page_offset: u64, content_length: u64) -> u8 {
    if content_length == 0 {
        return 0;
    }

    if current_offset == 0 {
        return 0;
    }

    if next_page_offset >= content_length {
        return 100;
    }

    let raw = ((current_offset.saturating_mul(100)) / content_length).clamp(0, 99);
    u8::try_from(raw).unwrap_or(99)
}

fn page_start_offset_for_page<SD, C>(
    fs: &SD,
    content_path: &str,
    target_page: usize,
    should_cancel: &mut C,
) -> Result<u64, EpubError>
where
    SD: SdFilesystem,
    C: FnMut() -> bool,
{
    if target_page == 0 {
        return Ok(0);
    }
    let mut scratch = Framebuffer::new();
    let (_, offset, _) =
        render_cached_page(fs, &mut scratch, content_path, target_page, should_cancel)?;
    Ok(offset)
}

fn render_cached_page<SD, C>(
    fs: &SD,
    display: &mut Framebuffer,
    content_path: &str,
    target_page: usize,
    should_cancel: &mut C,
) -> Result<(usize, u64, u64), EpubError>
where
    SD: SdFilesystem,
    C: FnMut() -> bool,
{
    let (mut page, mut offset) = (0usize, 0u64);
    let mut scratch = Framebuffer::new();

    loop {
        let (_, next) = if page >= target_page {
            render_cached_page_at_offset(fs, display, content_path, offset, should_cancel)?
        } else {
            render_cached_page_at_offset(fs, &mut scratch, content_path, offset, should_cancel)?
        };
        if page >= target_page || next <= offset {
            return Ok((page, offset, next));
        }

        page = page.saturating_add(1);
        offset = next;
    }
}

fn count_pages_before_offset<SD, C>(
    fs: &SD,
    content_path: &str,
    target_offset: u64,
    should_cancel: &mut C,
) -> Result<usize, EpubError>
where
    SD: SdFilesystem,
    C: FnMut() -> bool,
{
    if target_offset == 0 {
        return Ok(0);
    }

    let mut page = 0usize;
    let mut offset = 0u64;
    loop {
        let next = next_page_offset_for_offset(fs, content_path, offset, should_cancel)?;
        if next <= offset || next >= target_offset {
            return Ok(page.saturating_add(1));
        }
        page = page.saturating_add(1);
        offset = next;
    }
}

fn next_page_offset_for_offset<SD, C>(
    fs: &SD,
    content_path: &str,
    page_start_offset: u64,
    should_cancel: &mut C,
) -> Result<u64, EpubError>
where
    SD: SdFilesystem,
    C: FnMut() -> bool,
{
    let mut scratch = Framebuffer::new();
    let (_, next_page_offset) = render_cached_page_at_offset(
        fs,
        &mut scratch,
        content_path,
        page_start_offset,
        should_cancel,
    )?;
    Ok(next_page_offset)
}

fn render_cached_page_at_offset<SD, C>(
    fs: &SD,
    display: &mut Framebuffer,
    content_path: &str,
    page_start_offset: u64,
    should_cancel: &mut C,
) -> Result<(u64, u64), EpubError>
where
    SD: SdFilesystem,
    C: FnMut() -> bool,
{
    let mut file = fs
        .open_cache_file_read(content_path)
        .map_err(|_| EpubError::Io)?;
    file.seek_from_start(u32::try_from(page_start_offset).map_err(|_| EpubError::OutOfSpace)?)
        .map_err(|_| EpubError::Io)?;
    let mut read_text = |buffer: &mut [u8]| -> Result<usize, EpubError> {
        file.read(buffer).map_err(|_| EpubError::Io)
    };
    let rendered = display.render_cached_text_page_from_offset_with_progress(
        &mut read_text,
        0,
        &mut *should_cancel,
    )?;
    let next_page_offset = page_start_offset.saturating_add(
        u64::try_from(rendered.next_page_start_byte).map_err(|_| EpubError::OutOfSpace)?,
    );
    Ok((page_start_offset, next_page_offset))
}

struct BuildResult {
    content_length: usize,
    complete: bool,
    next_chapter_index: u16,
}

fn build_content_cache<SD>(
    fs: &SD,
    display: &mut Framebuffer,
    source: SD::EpubSource<'_>,
    _source_size: u32,
    paths: &CachePaths,
    page_index: usize,
    should_cancel: &mut impl FnMut() -> bool,
) -> Result<BuildResult, EpubError>
where
    SD: SdFilesystem,
{
    fs.ensure_directory(paths.directory.as_str())
        .map_err(|_| EpubError::Io)?;
    let mut content = fs
        .open_cache_file_write(paths.content.as_str())
        .map_err(|_| EpubError::Io)?;
    let mut chapters = fs
        .open_cache_file_write(paths.chapters.as_str())
        .map_err(|_| EpubError::Io)?;
    let mut content_buffer = CacheWriteBuffer::new();

    let mut on_text_chunk = |chunk: &str| -> Result<(), EpubError> {
        if !chunk.is_empty() {
            content_buffer.push(&mut content, chunk.as_bytes())?;
        }
        Ok(())
    };
    let mut on_chapter_start = |_: u16, offset: usize| -> Result<(), EpubError> {
        write_offset_bytes(
            &mut chapters,
            u64::try_from(offset).map_err(|_| EpubError::OutOfSpace)?,
        )
    };

    let build = display.build_epub_cache_prefix_with_callbacks_and_cancel(
        source,
        page_index,
        &mut on_text_chunk,
        &mut on_chapter_start,
        should_cancel,
    )?;

    content_buffer.flush(&mut content)?;
    content.flush().map_err(|_| EpubError::Io)?;
    chapters.flush().map_err(|_| EpubError::Io)?;

    Ok(BuildResult {
        content_length: content_buffer.total_written(),
        complete: build.complete,
        next_chapter_index: build.next_spine_index,
    })
}

fn read_cache_meta_if_valid<SD: SdFilesystem>(
    fs: &SD,
    paths: &CachePaths,
    source_size: u32,
) -> Result<CacheMeta, EpubError> {
    let meta = read_cache_meta(fs, paths.meta.as_str())?;
    if meta.version != CACHE_VERSION || meta.source_size != source_size {
        return Err(EpubError::InvalidFormat);
    }
    if meta.layout_sig_version != LAYOUT_SIG_VERSION
        || meta.layout_sig_width != DISPLAY_WIDTH
        || meta.layout_sig_height != DISPLAY_HEIGHT
        || meta.layout_sig_content_height != reader_content_height()
        || meta.layout_sig_font != LAYOUT_SIG_FONT
        || meta.layout_sig_paginator != LAYOUT_SIG_PAGINATOR
    {
        return Err(EpubError::InvalidFormat);
    }

    let content = fs
        .open_cache_file_read(paths.content.as_str())
        .map_err(|_| EpubError::Io)?;
    if content.len() as u64 != meta.content_length {
        return Err(EpubError::InvalidFormat);
    }
    Ok(meta)
}

fn read_cache_meta<SD: SdFilesystem>(fs: &SD, path: &str) -> Result<CacheMeta, EpubError> {
    let mut file = fs.open_cache_file_read(path).map_err(|_| EpubError::Io)?;
    let mut raw = [0u8; 384];
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

fn read_cached_progress<SD: SdFilesystem>(fs: &SD, path: &str) -> Option<ProgressState> {
    let mut file = fs.open_cache_file_read(path).ok()?;
    let mut raw = [0u8; 24];
    let mut total = 0usize;
    while total < raw.len() {
        let n = file.read(&mut raw[total..]).ok()?;
        if n == 0 {
            break;
        }
        total += n;
    }
    decode_progress(&raw[..total])
}

fn write_cached_progress<SD: SdFilesystem>(
    fs: &SD,
    path: &str,
    progress: ProgressState,
) -> Result<(), EpubError> {
    write_bytes(fs, path, &encode_progress(progress))
}

fn write_meta<SD: SdFilesystem>(fs: &SD, path: &str, meta: &CacheMeta) -> Result<(), EpubError> {
    let serialized = serialize_meta(meta);
    write_bytes(fs, path, serialized.as_bytes())
}

fn write_offset_bytes<F: SdFsFile>(file: &mut F, offset: u64) -> Result<(), EpubError> {
    let raw = encode_offset(offset);
    let mut written = 0usize;
    while written < raw.len() {
        let count = file.write(&raw[written..]).map_err(|_| EpubError::Io)?;
        if count == 0 {
            return Err(EpubError::Io);
        }
        written += count;
    }
    Ok(())
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
