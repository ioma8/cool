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
    let cache_paths = cache_paths_for_epub(current_path, entry.fs_name.as_str());
    let source_size = u32::try_from(
        fs.open_epub_source(source_path.as_str())
            .map_err(|_| EpubError::Io)?
            .len(),
    )
    .map_err(|_| EpubError::OutOfSpace)?;
    let saved = read_cached_progress(fs, cache_paths.progress.as_str());
    let start_page = saved.map(|p| p.current_page_hint as usize).unwrap_or(0);

    render_epub_page_from_entry_with_cancel(
        fs,
        display,
        current_path,
        entry,
        source_size,
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
    let source_path =
        join_child_path(current_path, entry.fs_name.as_str()).map_err(|_| EpubError::Io)?;
    let source_size = u32::try_from(
        fs.open_epub_source(source_path.as_str())
            .map_err(|_| EpubError::Io)?
            .len(),
    )
    .map_err(|_| EpubError::OutOfSpace)?;
    render_epub_page_from_entry_with_cancel(
        fs,
        display,
        current_path,
        entry,
        source_size,
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
        let source = fs
            .open_epub_source(source_path.as_str())
            .map_err(|_| EpubError::Io)?;
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
        // initialize sidecars
        write_offset_record(fs, cache_paths.pages.as_str(), 0, true)?;
        write_offset_record(fs, cache_paths.chapters.as_str(), 0, true)?;
        meta = Some(new_meta);
    }

    let meta = meta.ok_or(EpubError::Io)?;
    let saved = read_cached_progress(fs, cache_paths.progress.as_str()).unwrap_or(ProgressState {
        current_byte_offset: 0,
        current_page_hint: 0,
    });

    let (rendered_page, page_start_offset, next_page_offset) = render_cached_page(
        fs,
        display,
        cache_paths.content.as_str(),
        page_index,
        saved,
        &mut should_cancel,
    )?;

    let progress_percent =
        compute_progress_percent(page_start_offset, meta.content_length, meta.build_complete);

    write_cached_progress(
        fs,
        cache_paths.progress.as_str(),
        ProgressState {
            current_byte_offset: page_start_offset,
            current_page_hint: u32::try_from(rendered_page).unwrap_or(u32::MAX),
        },
    )?;

    // best-effort page index update
    let _ = write_page_index_record(
        fs,
        cache_paths.pages.as_str(),
        rendered_page,
        page_start_offset,
    );
    let _ = write_page_index_record(
        fs,
        cache_paths.pages.as_str(),
        rendered_page + 1,
        next_page_offset,
    );

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

fn compute_progress_percent(current_offset: u64, content_length: u64, build_complete: bool) -> u8 {
    if content_length == 0 {
        return 0;
    }
    let raw = ((current_offset.saturating_mul(100)) / content_length).clamp(0, 100);
    let pct = u8::try_from(raw).unwrap_or(100);
    if build_complete {
        pct.max(1)
    } else {
        pct.min(99)
    }
}

fn render_cached_page<SD, C>(
    fs: &SD,
    display: &mut Framebuffer,
    content_path: &str,
    target_page: usize,
    saved: ProgressState,
    should_cancel: &mut C,
) -> Result<(usize, u64, u64), EpubError>
where
    SD: SdFilesystem,
    C: FnMut() -> bool,
{
    let (mut page, mut offset) = if target_page >= saved.current_page_hint as usize {
        (saved.current_page_hint as usize, saved.current_byte_offset)
    } else {
        (0usize, 0u64)
    };

    loop {
        let mut file = fs
            .open_cache_file_read(content_path)
            .map_err(|_| EpubError::Io)?;
        file.seek_from_start(0).map_err(|_| EpubError::Io)?;
        let mut read_text = |buffer: &mut [u8]| -> Result<usize, EpubError> {
            file.read(buffer).map_err(|_| EpubError::Io)
        };
        let rendered = display.render_cached_text_page_from_offset_with_progress(
            &mut read_text,
            usize::try_from(offset).map_err(|_| EpubError::OutOfSpace)?,
            &mut *should_cancel,
        )?;

        let next =
            u64::try_from(rendered.next_page_start_byte).map_err(|_| EpubError::OutOfSpace)?;
        if page >= target_page || next <= offset {
            return Ok((page, offset, next));
        }

        page = page.saturating_add(1);
        offset = next;
    }
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
    let mut content_buffer = CacheWriteBuffer::new();

    let mut on_text_chunk = |chunk: &str| -> Result<(), EpubError> {
        if !chunk.is_empty() {
            content_buffer.push(&mut content, chunk.as_bytes())?;
        }
        Ok(())
    };

    let build = display.build_epub_cache_prefix_with_text_sink_and_cancel(
        source,
        page_index,
        &mut on_text_chunk,
        should_cancel,
    )?;

    content_buffer.flush(&mut content)?;
    content.flush().map_err(|_| EpubError::Io)?;

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
    let mut raw = [0u8; 12];
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

fn write_offset_record<SD: SdFilesystem>(
    fs: &SD,
    path: &str,
    offset: u64,
    truncate: bool,
) -> Result<(), EpubError> {
    let mut file = if truncate {
        fs.open_cache_file_write(path)
    } else {
        fs.open_cache_file_append(path)
    }
    .map_err(|_| EpubError::Io)?;
    let raw = encode_offset(offset);
    let mut written = 0usize;
    while written < raw.len() {
        let count = file.write(&raw[written..]).map_err(|_| EpubError::Io)?;
        if count == 0 {
            return Err(EpubError::Io);
        }
        written += count;
    }
    file.flush().map_err(|_| EpubError::Io)?;
    Ok(())
}

fn write_page_index_record<SD: SdFilesystem>(
    fs: &SD,
    path: &str,
    page: usize,
    offset: u64,
) -> Result<(), EpubError> {
    let mut file = match fs.open_cache_file_read(path) {
        Ok(file) => {
            let existing_records = file.len() / 8;
            if page < existing_records {
                return Ok(());
            }
            fs.open_cache_file_append(path).map_err(|_| EpubError::Io)?
        }
        Err(_) => fs.open_cache_file_write(path).map_err(|_| EpubError::Io)?,
    };

    let raw = encode_offset(offset);
    let mut written = 0usize;
    while written < raw.len() {
        let count = file.write(&raw[written..]).map_err(|_| EpubError::Io)?;
        if count == 0 {
            return Err(EpubError::Io);
        }
        written += count;
    }
    file.flush().map_err(|_| EpubError::Io)?;
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
