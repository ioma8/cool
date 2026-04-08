#![cfg_attr(not(test), no_std)]

pub mod zip;

use core::str;

use miniz_oxide::inflate::{
    TINFLStatus, decompress_slice_iter_to_slice,
    stream::{InflateState, inflate},
};
use miniz_oxide::{DataFormat, MZError, MZFlush, MZStatus};

pub use zip::{CompressionMethod, EpubArchive, EpubEntryMetadata, Error as ZipError};

pub const MAX_CHAPTER_DIR_BYTES: usize = 1024;
pub const MAX_ARCHIVE_ENTRIES: usize = 512;
pub const MAX_ARCHIVE_NAME_CAPACITY: usize = 16 * 1024;
const CHAPTER_TAIL_RESERVE: usize = 1024;
const NEXT_EVENT_PROGRESS_INTERVAL: usize = 256;

macro_rules! epub_logln {
    ($($arg:tt)*) => {{
        #[cfg(target_arch = "riscv32")]
        {
            let _ = esp_println::println!($($arg)*);
        }
    }};
}

#[derive(Debug)]
pub enum EpubError {
    Io,
    Zip,
    Utf8,
    InvalidFormat,
    Compression,
    OutOfSpace,
    Unsupported,
    Cancelled,
}

impl From<ZipError> for EpubError {
    fn from(error: ZipError) -> Self {
        match error {
            ZipError::Source => Self::Zip,
            ZipError::EocdNotFound => Self::InvalidFormat,
            ZipError::ShortRead => Self::Io,
            ZipError::InvalidArchive(_) => Self::InvalidFormat,
            ZipError::TooManyEntries => Self::InvalidFormat,
            ZipError::NameBufferTooSmall => Self::OutOfSpace,
            ZipError::ArithmeticOverflow => Self::InvalidFormat,
        }
    }
}

/// Source abstraction for random-access reads from EPUB bytes.
pub trait EpubSource {
    fn len(&self) -> usize;
    fn read_at(&self, offset: u64, buffer: &mut [u8]) -> Result<usize, EpubError>;
}

impl<T> zip::ZipSource for T
where
    T: EpubSource,
{
    fn len(&self) -> usize {
        T::len(self)
    }

    fn read_at(&self, offset: u64, buffer: &mut [u8]) -> Result<usize, ()> {
        T::read_at(self, offset, buffer).map_err(|_| ())
    }
}

pub struct ReaderBuffers<'a> {
    pub zip_cd: &'a mut [u8],
    pub inflate: &'a mut [u8],
    pub stream_input: &'a mut [u8],
    pub xml: &'a mut [u8],
    pub catalog: &'a mut [u8],
    pub path_buf: &'a mut [u8],
    pub stream_state: &'a mut InflateState,
    pub archive: &'a mut EpubArchive<MAX_ARCHIVE_ENTRIES, MAX_ARCHIVE_NAME_CAPACITY>,
}

struct ReaderScratch<'a> {
    zip_cd: *mut [u8],
    inflate: *mut [u8],
    stream_input: *mut [u8],
    xml: *mut [u8],
    catalog: *mut [u8],
    path_buf: *mut [u8],
    stream_state: *mut InflateState,
    archive: *mut EpubArchive<MAX_ARCHIVE_ENTRIES, MAX_ARCHIVE_NAME_CAPACITY>,
    _marker: core::marker::PhantomData<&'a mut ()>,
}

impl<'a> ReaderScratch<'a> {
    fn new(buffers: ReaderBuffers<'a>) -> Self {
        Self {
            zip_cd: buffers.zip_cd as *mut [u8],
            inflate: buffers.inflate as *mut [u8],
            stream_input: buffers.stream_input as *mut [u8],
            xml: buffers.xml as *mut [u8],
            catalog: buffers.catalog as *mut [u8],
            path_buf: buffers.path_buf as *mut [u8],
            stream_state: buffers.stream_state as *mut InflateState,
            archive: buffers.archive
                as *mut EpubArchive<MAX_ARCHIVE_ENTRIES, MAX_ARCHIVE_NAME_CAPACITY>,
            _marker: core::marker::PhantomData,
        }
    }

    fn zip_cd_ptr(&self) -> *mut [u8] {
        self.zip_cd
    }

    fn inflate_ptr(&self) -> *mut [u8] {
        self.inflate
    }

    fn stream_input_ptr(&self) -> *mut [u8] {
        self.stream_input
    }

    fn xml_ptr(&self) -> *mut [u8] {
        self.xml
    }

    fn catalog_ptr(&self) -> *mut [u8] {
        self.catalog
    }

    fn path_buf_ptr(&self) -> *mut [u8] {
        self.path_buf
    }

    fn stream_state_ptr(&self) -> *mut InflateState {
        self.stream_state
    }

    fn archive_ptr(&self) -> *mut EpubArchive<MAX_ARCHIVE_ENTRIES, MAX_ARCHIVE_NAME_CAPACITY> {
        self.archive
    }
}

#[derive(Debug)]
pub enum EpubEvent<'a> {
    Text(&'a str),
    ParagraphStart,
    ParagraphEnd,
    HeadingStart(u8),
    HeadingEnd,
    LineBreak,
    Image { src: &'a str, alt: Option<&'a str> },
    UnsupportedTag,
}

#[derive(Debug)]
struct ParserState {
    catalog_ready: bool,
    spine_count: u16,
    spine_index: u16,
    chapter_loaded: bool,
    chapter_finished: bool,
    chapter_entry: Option<EpubEntryMetadata>,
    chapter_len: usize,
    cursor: usize,
    input_len: usize,
    input_cursor: usize,
    compressed_read: usize,
    in_paragraph: bool,
    in_heading: u8,
    in_pre: bool,
    chapter_is_nav_doc: bool,
    nav_suppress_outside: bool,
    nav_visible_depth: u8,
    prev_space: bool,
    last_nonspace_char: Option<char>,
    ignore_depth: u8,
    skip_tag_depth: u16,
    skip_tag_name_len: u8,
    skip_tag_name: [u8; 8],
    list_depth: u8,
    list_ordered: [bool; 8],
    list_items: [u16; 8],
    table_in_head: bool,
    table_has_head: bool,
    table_row_cols: u8,
    chapter_dir_len: usize,
    chapter_dir: [u8; MAX_CHAPTER_DIR_BYTES],
    done: bool,
}

impl Default for ParserState {
    fn default() -> Self {
        Self {
            catalog_ready: false,
            spine_count: 0,
            spine_index: 0,
            chapter_loaded: false,
            chapter_finished: false,
            chapter_entry: None,
            chapter_len: 0,
            cursor: 0,
            input_len: 0,
            input_cursor: 0,
            compressed_read: 0,
            in_paragraph: false,
            in_heading: 0,
            in_pre: false,
            chapter_is_nav_doc: false,
            nav_suppress_outside: false,
            nav_visible_depth: 0,
            prev_space: false,
            last_nonspace_char: None,
            ignore_depth: 0,
            skip_tag_depth: 0,
            skip_tag_name_len: 0,
            skip_tag_name: [0u8; 8],
            list_depth: 0,
            list_ordered: [false; 8],
            list_items: [0u16; 8],
            table_in_head: false,
            table_has_head: false,
            table_row_cols: 0,
            chapter_dir_len: 0,
            chapter_dir: [0u8; MAX_CHAPTER_DIR_BYTES],
            done: false,
        }
    }
}

pub struct Epub<S: EpubSource> {
    source: S,
    state: ParserState,
}

impl<S: EpubSource> Epub<S> {
    pub fn open(source: S) -> Result<Self, EpubError> {
        Ok(Self {
            source,
            state: ParserState::default(),
        })
    }

    pub fn next_event<'a>(
        &'a mut self,
        workspace: ReaderBuffers<'a>,
    ) -> Result<Option<EpubEvent<'a>>, EpubError> {
        let scratch = ReaderScratch::new(workspace);
        let inflate_ptr = scratch.inflate_ptr();
        let stream_input_ptr = scratch.stream_input_ptr();
        let xml_ptr = scratch.xml_ptr();
        let catalog_ptr = scratch.catalog_ptr();
        let path_buf_ptr = scratch.path_buf_ptr();
        let zip_cd_ptr = scratch.zip_cd_ptr();
        let stream_state_ptr = scratch.stream_state_ptr();
        let archive_ptr = scratch.archive_ptr();

        if self.state.done {
            return Ok(None);
        }

        if !self.state.catalog_ready {
            epub_logln!("EPUB parser: prepare_catalog begin");
            let (catalog, inflate, zip_cd, path_buf, archive) = unsafe {
                (
                    &mut *catalog_ptr,
                    &mut *inflate_ptr,
                    &mut *zip_cd_ptr,
                    &mut *path_buf_ptr,
                    &mut *archive_ptr,
                )
            };
            self.prepare_catalog(catalog, inflate, zip_cd, path_buf, archive)?;
            self.state.catalog_ready = true;
            epub_logln!(
                "EPUB parser: prepare_catalog end spine_count={}",
                self.state.spine_count
            );
        }

        let mut loop_iterations = 0usize;
        loop {
            loop_iterations = loop_iterations.saturating_add(1);
            if loop_iterations % NEXT_EVENT_PROGRESS_INTERVAL == 0 {
                epub_logln!(
                    "EPUB parser: progress spine_index={} cursor={} chapter_len={} chapter_finished={} compressed_read={} input_cursor={} input_len={}",
                    self.state.spine_index,
                    self.state.cursor,
                    self.state.chapter_len,
                    self.state.chapter_finished,
                    self.state.compressed_read,
                    self.state.input_cursor,
                    self.state.input_len
                );
            }
            if self.state.done {
                return Ok(None);
            }

            if !self.state.chapter_loaded {
                if self.state.spine_index >= self.state.spine_count {
                    self.state.done = true;
                    return Ok(None);
                }
                epub_logln!(
                    "EPUB parser: load_current_chapter begin spine_index={}",
                    self.state.spine_index
                );
                let (catalog, zip_cd, path_buf, stream_state, archive) = unsafe {
                    (
                        &mut *catalog_ptr,
                        &mut *zip_cd_ptr,
                        &mut *path_buf_ptr,
                        &mut *stream_state_ptr,
                        &mut *archive_ptr,
                    )
                };
                self.load_current_chapter(catalog, zip_cd, path_buf, stream_state, archive)?;
                if let Some(_entry) = self.state.chapter_entry {
                    epub_logln!(
                        "EPUB parser: load_current_chapter end spine_index={} chapter_dir_len={} compression={:?} compressed_size={} uncompressed_size={}",
                        self.state.spine_index.saturating_sub(1),
                        self.state.chapter_dir_len,
                        _entry.compression,
                        _entry.compressed_size,
                        _entry.uncompressed_size
                    );
                }
            }

            if self.state.cursor >= self.state.chapter_len
                || (!self.state.chapter_finished
                    && self.state.chapter_len.saturating_sub(self.state.cursor)
                        <= CHAPTER_TAIL_RESERVE)
            {
                epub_logln!(
                    "EPUB parser: refill_current_chapter begin cursor={} chapter_len={} chapter_finished={} compressed_read={} input_cursor={} input_len={}",
                    self.state.cursor,
                    self.state.chapter_len,
                    self.state.chapter_finished,
                    self.state.compressed_read,
                    self.state.input_cursor,
                    self.state.input_len
                );
                if !self.state.chapter_finished && self.state.cursor >= self.state.chapter_len {
                    self.state.cursor = self
                        .state
                        .chapter_len
                        .saturating_sub(CHAPTER_TAIL_RESERVE.min(self.state.chapter_len));
                }
                let (inflate, stream_input, stream_state) = unsafe {
                    (
                        &mut *inflate_ptr,
                        &mut *stream_input_ptr,
                        &mut *stream_state_ptr,
                    )
                };
                self.refill_current_chapter(inflate, stream_input, stream_state)?;
                epub_logln!(
                    "EPUB parser: refill_current_chapter end cursor={} chapter_len={} chapter_finished={} compressed_read={} input_cursor={} input_len={}",
                    self.state.cursor,
                    self.state.chapter_len,
                    self.state.chapter_finished,
                    self.state.compressed_read,
                    self.state.input_cursor,
                    self.state.input_len
                );
            }

            if self.state.cursor >= self.state.chapter_len {
                self.state.chapter_loaded = false;
                continue;
            }

            let chapter = unsafe {
                core::slice::from_raw_parts((*inflate_ptr).as_ptr(), self.state.chapter_len)
            };
            let dir_len = self.state.chapter_dir_len;
            let mut chapter_dir = [0u8; MAX_CHAPTER_DIR_BYTES];
            chapter_dir[..dir_len].copy_from_slice(&self.state.chapter_dir[..dir_len]);

            let cursor_before = self.state.cursor;
            if let Some(event) =
                parse_next_xhtml_event(chapter, &mut self.state, &chapter_dir[..dir_len], unsafe {
                    &mut *xml_ptr
                })?
            {
                return Ok(Some(event));
            }

            if self.state.cursor == cursor_before && !self.state.chapter_finished {
                if self.state.cursor >= self.state.chapter_len {
                    self.state.cursor = self
                        .state
                        .chapter_len
                        .saturating_sub(CHAPTER_TAIL_RESERVE.min(self.state.chapter_len));
                }
                let (inflate, stream_input, stream_state) = unsafe {
                    (
                        &mut *inflate_ptr,
                        &mut *stream_input_ptr,
                        &mut *stream_state_ptr,
                    )
                };
                self.refill_current_chapter(inflate, stream_input, stream_state)?;
            }

            if self.state.cursor >= self.state.chapter_len && self.state.chapter_finished {
                self.state.chapter_loaded = false;
            }
        }
    }

    fn prepare_catalog(
        &mut self,
        catalog: &mut [u8],
        inflate: &mut [u8],
        zip_cd: &mut [u8],
        _path_buf: &mut [u8],
        archive: &mut EpubArchive<MAX_ARCHIVE_ENTRIES, MAX_ARCHIVE_NAME_CAPACITY>,
    ) -> Result<(), EpubError> {
        epub_logln!("EPUB parser: archive.parse begin [catalog]");
        archive.parse(&self.source)?;
        epub_logln!("EPUB parser: archive.parse end [catalog]");

        let container_entry = archive
            .entry_by_name(&self.source, "META-INF/container.xml", zip_cd)?
            .ok_or(EpubError::InvalidFormat)?;
        epub_logln!(
            "EPUB parser: read container begin compressed_size={} uncompressed_size={}",
            container_entry.compressed_size,
            container_entry.uncompressed_size
        );
        let container = read_entry(&self.source, &container_entry, inflate, zip_cd)?;
        epub_logln!("EPUB parser: read container end bytes={}", container.len());

        let (opf_path_start, opf_path_len) = parse_container_root(container)?;
        let mut opf_path = [0u8; 512];
        if opf_path_len > opf_path.len() {
            return Err(EpubError::OutOfSpace);
        }
        opf_path[..opf_path_len]
            .copy_from_slice(&container[opf_path_start..opf_path_start + opf_path_len]);

        let opf_entry = archive
            .entry_by_name_bytes(&self.source, &opf_path[..opf_path_len], zip_cd)?
            .ok_or(EpubError::InvalidFormat)?;
        epub_logln!(
            "EPUB parser: read opf begin path_len={} compressed_size={} uncompressed_size={}",
            opf_path_len,
            opf_entry.compressed_size,
            opf_entry.uncompressed_size
        );
        let opf = read_entry(&self.source, &opf_entry, inflate, zip_cd)?;
        epub_logln!("EPUB parser: read opf end bytes={}", opf.len());

        epub_logln!("EPUB parser: parse_opf begin");
        let count = parse_opf(opf, &opf_path[..opf_path_len], catalog)?;
        epub_logln!("EPUB parser: parse_opf end spine_count={}", count);
        self.state.spine_count = count;
        Ok(())
    }

    fn load_current_chapter(
        &mut self,
        catalog: &mut [u8],
        zip_cd: &mut [u8],
        _path_buf: &mut [u8],
        stream_state: &mut InflateState,
        archive: &mut EpubArchive<MAX_ARCHIVE_ENTRIES, MAX_ARCHIVE_NAME_CAPACITY>,
    ) -> Result<(), EpubError> {
        let index = self.state.spine_index;
        let (entry_start, entry_len) = read_spine_entry(catalog, index)?;
        let chapter_path = &catalog[entry_start..entry_start + entry_len];

        let entry = archive
            .entry_by_name_bytes(&self.source, chapter_path, zip_cd)?
            .ok_or(EpubError::InvalidFormat)?;
        epub_logln!(
            "EPUB parser: chapter entry resolved path_len={} compression={:?} compressed_size={} uncompressed_size={}",
            chapter_path.len(),
            entry.compression,
            entry.compressed_size,
            entry.uncompressed_size
        );

        let base = path_parent(chapter_path);
        if base.len() > self.state.chapter_dir.len() {
            return Err(EpubError::OutOfSpace);
        }
        self.state.chapter_dir[..base.len()].copy_from_slice(base);
        self.state.chapter_dir_len = base.len();
        self.state.chapter_entry = Some(entry);
        self.state.chapter_len = 0;
        self.state.cursor = 0;
        self.state.input_len = 0;
        self.state.input_cursor = 0;
        self.state.compressed_read = 0;
        self.state.chapter_finished =
            matches!(entry.compression, CompressionMethod::Stored) && entry.uncompressed_size == 0;
        self.state.in_paragraph = false;
        self.state.in_heading = 0;
        self.state.in_pre = false;
        self.state.chapter_is_nav_doc =
            chapter_path.ends_with(b"toc.xhtml") || chapter_path.ends_with(b"nav.xhtml");
        self.state.nav_suppress_outside = false;
        self.state.nav_visible_depth = 0;
        self.state.prev_space = false;
        self.state.last_nonspace_char = None;
        self.state.ignore_depth = 0;
        self.state.skip_tag_depth = 0;
        self.state.skip_tag_name_len = 0;
        self.state.skip_tag_name = [0u8; 8];
        self.state.list_depth = 0;
        self.state.list_ordered = [false; 8];
        self.state.list_items = [0u16; 8];
        self.state.table_in_head = false;
        self.state.table_has_head = false;
        self.state.table_row_cols = 0;
        self.state.chapter_loaded = true;
        self.state.spine_index = self.state.spine_index.saturating_add(1);
        stream_state.reset(DataFormat::Raw);
        Ok(())
    }

    fn refill_current_chapter(
        &mut self,
        chapter_buf: &mut [u8],
        stream_input: &mut [u8],
        stream_state: &mut InflateState,
    ) -> Result<(), EpubError> {
        let preserved = self.state.chapter_len.saturating_sub(self.state.cursor);
        if preserved == chapter_buf.len() {
            return Err(EpubError::OutOfSpace);
        }
        if preserved > 0 && self.state.cursor > 0 {
            chapter_buf.copy_within(self.state.cursor..self.state.chapter_len, 0);
        }
        self.state.chapter_len = preserved;
        self.state.cursor = 0;

        let Some(entry) = self.state.chapter_entry else {
            return Err(EpubError::InvalidFormat);
        };

        match entry.compression {
            CompressionMethod::Stored => {
                let total = usize::try_from(entry.uncompressed_size)
                    .map_err(|_| EpubError::InvalidFormat)?;
                if self.state.compressed_read >= total {
                    self.state.chapter_finished = true;
                    return Ok(());
                }
                let remaining = total - self.state.compressed_read;
                let to_read =
                    remaining.min(chapter_buf.len().saturating_sub(self.state.chapter_len));
                read_exact(
                    &self.source,
                    u64::from(entry.data_offset)
                        + u64::try_from(self.state.compressed_read)
                            .map_err(|_| EpubError::InvalidFormat)?,
                    &mut chapter_buf[self.state.chapter_len..self.state.chapter_len + to_read],
                )?;
                self.state.compressed_read = self.state.compressed_read.saturating_add(to_read);
                self.state.chapter_len = self.state.chapter_len.saturating_add(to_read);
                self.state.chapter_finished = self.state.compressed_read >= total;
                Ok(())
            }
            CompressionMethod::Deflate => {
                let compressed_total =
                    usize::try_from(entry.compressed_size).map_err(|_| EpubError::InvalidFormat)?;
                loop {
                    if self.state.chapter_len >= chapter_buf.len() {
                        return Ok(());
                    }

                    if self.state.input_cursor >= self.state.input_len
                        && self.state.compressed_read < compressed_total
                    {
                        let remaining = compressed_total - self.state.compressed_read;
                        let to_read = remaining.min(stream_input.len());
                        read_exact(
                            &self.source,
                            u64::from(entry.data_offset)
                                + u64::try_from(self.state.compressed_read)
                                    .map_err(|_| EpubError::InvalidFormat)?,
                            &mut stream_input[..to_read],
                        )?;
                        self.state.compressed_read =
                            self.state.compressed_read.saturating_add(to_read);
                        self.state.input_len = to_read;
                        self.state.input_cursor = 0;
                    }

                    let input = &stream_input[self.state.input_cursor..self.state.input_len];
                    let flush = if self.state.compressed_read >= compressed_total {
                        MZFlush::Finish
                    } else {
                        MZFlush::None
                    };
                    let result = inflate(
                        stream_state,
                        input,
                        &mut chapter_buf[self.state.chapter_len..],
                        flush,
                    );
                    if result.bytes_consumed == 0
                        && result.bytes_written == 0
                        && !matches!(result.status, Ok(MZStatus::StreamEnd))
                    {
                        epub_logln!(
                            "EPUB parser: refill stall compressed_read={} compressed_total={} chapter_len={} input_cursor={} input_len={} flush={:?} status={:?}",
                            self.state.compressed_read,
                            compressed_total,
                            self.state.chapter_len,
                            self.state.input_cursor,
                            self.state.input_len,
                            flush,
                            result.status
                        );
                    }
                    self.state.input_cursor = self
                        .state
                        .input_cursor
                        .saturating_add(result.bytes_consumed);
                    self.state.chapter_len =
                        self.state.chapter_len.saturating_add(result.bytes_written);

                    match result.status {
                        Ok(MZStatus::Ok) => {
                            if result.bytes_written > 0 {
                                return Ok(());
                            }
                            if self.state.input_cursor >= self.state.input_len
                                && self.state.compressed_read >= compressed_total
                            {
                                return Err(EpubError::Compression);
                            }
                        }
                        Ok(MZStatus::StreamEnd) => {
                            self.state.chapter_finished = true;
                            return Ok(());
                        }
                        Ok(MZStatus::NeedDict) => return Err(EpubError::Unsupported),
                        Err(MZError::Buf) => {
                            if result.bytes_written > 0
                                || self.state.chapter_len >= chapter_buf.len()
                            {
                                return Ok(());
                            }
                            if self.state.input_cursor >= self.state.input_len
                                && self.state.compressed_read < compressed_total
                            {
                                continue;
                            }
                            return Err(EpubError::OutOfSpace);
                        }
                        Err(MZError::Data) => return Err(EpubError::Compression),
                        Err(MZError::Stream | MZError::Param) => {
                            return Err(EpubError::InvalidFormat);
                        }
                        Err(_) => return Err(EpubError::Compression),
                    }
                }
            }
            CompressionMethod::Other(_) => Err(EpubError::Unsupported),
        }
    }
}

#[derive(Clone, Copy)]
struct ManifestItem {
    href_len: u16,
    href_start: usize,
    media_start: usize,
    media_end: usize,
}

fn read_spine_entry(catalog: &[u8], index: u16) -> Result<(usize, usize), EpubError> {
    if catalog.len() < 2 {
        return Err(EpubError::InvalidFormat);
    }
    let count = u16::from_le_bytes([catalog[0], catalog[1]]);
    if index >= count {
        return Err(EpubError::InvalidFormat);
    }

    let mut cursor = 2usize;
    for _ in 0..index {
        if cursor + 2 > catalog.len() {
            return Err(EpubError::InvalidFormat);
        }
        let len = usize::from(u16::from_le_bytes([catalog[cursor], catalog[cursor + 1]]));
        cursor = cursor
            .checked_add(2)
            .and_then(|v| v.checked_add(len))
            .ok_or(EpubError::InvalidFormat)?;
        if cursor > catalog.len() {
            return Err(EpubError::InvalidFormat);
        }
    }

    if cursor + 2 > catalog.len() {
        return Err(EpubError::InvalidFormat);
    }
    let len = usize::from(u16::from_le_bytes([catalog[cursor], catalog[cursor + 1]]));
    let start = cursor + 2;
    let end = start + len;
    if end > catalog.len() {
        return Err(EpubError::InvalidFormat);
    }
    Ok((start, len))
}

fn parse_container_root(container: &[u8]) -> Result<(usize, usize), EpubError> {
    let mut cursor = 0usize;
    while cursor < container.len() {
        if container[cursor] != b'<' {
            cursor += 1;
            continue;
        }
        if let Some(tag) = parse_xml_tag(container, &mut cursor)? {
            if !tag.is_end && tag.name_is("rootfile") {
                if let Some(full_path) = tag.attr(b"full-path") {
                    let start = container.as_ptr() as usize;
                    let path_start = full_path.as_ptr() as usize - start;
                    return Ok((path_start, full_path.len()));
                }
            }
        }
    }
    Err(EpubError::InvalidFormat)
}

fn parse_opf(opf: &[u8], opf_path: &[u8], catalog: &mut [u8]) -> Result<u16, EpubError> {
    if catalog.len() < 2 {
        return Err(EpubError::OutOfSpace);
    }
    let mut write = 2usize;
    let mut count: u16 = 0;
    let opf_base = path_parent(opf_path);

    let mut cursor = 0usize;
    while cursor < opf.len() {
        if opf[cursor] != b'<' {
            cursor += 1;
            continue;
        }
        if let Some(tag) = parse_xml_tag(opf, &mut cursor)? {
            if !tag.is_end && tag.name_is("itemref") {
                if matches!(tag.attr(b"linear"), Some(value) if value == b"no") {
                    continue;
                }
                let idref = tag.attr(b"idref").ok_or(EpubError::InvalidFormat)?;
                let item = manifest_item_for_id(opf, idref)?;
                let media_type = &opf[item.media_start..item.media_end];
                if !attr_eq(media_type, b"application/xhtml+xml") {
                    continue;
                }

                let href = &opf[item.href_start..item.href_start + usize::from(item.href_len)];
                let mut tmp = [0u8; MAX_CHAPTER_DIR_BYTES];
                let resolved_len = resolve_reference(opf_base, href, &mut tmp)?;
                if write + 2 + resolved_len > catalog.len() {
                    return Err(EpubError::OutOfSpace);
                }
                catalog[write..write + 2].copy_from_slice(
                    &(u16::try_from(resolved_len).map_err(|_| EpubError::OutOfSpace)?)
                        .to_le_bytes(),
                );
                write += 2;
                catalog[write..write + resolved_len].copy_from_slice(&tmp[..resolved_len]);
                write += resolved_len;
                count = count.saturating_add(1);
            }
        }
    }
    catalog[..2].copy_from_slice(&count.to_le_bytes());
    Ok(count)
}

fn manifest_item_for_id(opf: &[u8], idref: &[u8]) -> Result<ManifestItem, EpubError> {
    let opf_start = opf.as_ptr() as usize;
    let mut cursor = 0usize;
    while cursor < opf.len() {
        if opf[cursor] != b'<' {
            cursor += 1;
            continue;
        }
        if let Some(tag) = parse_xml_tag(opf, &mut cursor)? {
            if !tag.is_end && tag.name_is("item") {
                if let Some(id) = tag.attr(b"id") {
                    if attr_eq(id, idref) {
                        let media = tag.attr(b"media-type").unwrap_or(b"");
                        let href = tag.attr(b"href").ok_or(EpubError::InvalidFormat)?;
                        let href_start = href.as_ptr() as usize - opf_start;
                        let media_start = if media.is_empty() {
                            href_start
                        } else {
                            media.as_ptr() as usize - opf_start
                        };
                        return Ok(ManifestItem {
                            href_len: u16::try_from(href.len())
                                .map_err(|_| EpubError::OutOfSpace)?,
                            href_start,
                            media_start,
                            media_end: media_start + media.len(),
                        });
                    }
                }
            }
        }
    }
    Err(EpubError::InvalidFormat)
}

fn attr_eq(lhs: &[u8], rhs: &[u8]) -> bool {
    if lhs.len() != rhs.len() {
        return false;
    }
    lhs.iter().zip(rhs).all(|(a, b)| a == b)
}

fn attr_contains_token(value: &[u8], token: &[u8]) -> bool {
    let mut cursor = 0usize;
    while cursor < value.len() {
        while cursor < value.len() && value[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        let start = cursor;
        while cursor < value.len() && !value[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if start < cursor && &value[start..cursor] == token {
            return true;
        }
    }
    false
}

fn attr_contains_bytes(value: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || needle.len() > value.len() {
        return false;
    }
    value.windows(needle.len()).any(|window| window == needle)
}

fn tag_has_class(tag: &Tag<'_>, class_name: &[u8]) -> bool {
    tag.attr(b"class")
        .is_some_and(|value| attr_contains_token(value, class_name))
}

fn tag_has_style(tag: &Tag<'_>, needle: &[u8]) -> bool {
    tag.attr(b"style")
        .is_some_and(|value| attr_contains_bytes(value, needle))
}

fn tag_is_css_hidden(tag: &Tag<'_>) -> bool {
    if tag_has_style(tag, b"display:none") || tag_has_style(tag, b"display: none") {
        return true;
    }
    if tag_has_style(tag, b"display:block") || tag_has_style(tag, b"display: block") {
        return false;
    }
    if tag.name_is("aside") {
        return true;
    }
    if tag.name_is("div") && tag_has_class(tag, b"removeme") {
        return true;
    }
    if tag.name_is("span") && tag_has_class(tag, b"secret") {
        return true;
    }
    if tag.name_is("img") && tag_has_class(tag, b"suppressed") {
        return true;
    }
    if tag_has_class(tag, b"hidden")
        || tag_has_class(tag, b"invisible")
        || tag_has_class(tag, b"hidden-container")
    {
        return !((tag.name_is("p") && tag_has_class(tag, b"force-show"))
            || tag_has_style(tag, b"display:block")
            || tag_has_style(tag, b"display: block"));
    }
    false
}

fn table_small_separator<'a>(cols: u8, text_buf: &'a mut [u8]) -> Result<&'a str, EpubError> {
    let cols = usize::from(cols);
    if cols == 0 {
        return Err(EpubError::InvalidFormat);
    }
    let needed = cols * 7 + cols.saturating_sub(1);
    if needed > text_buf.len() {
        return Err(EpubError::OutOfSpace);
    }
    let mut write = 0usize;
    for idx in 0..cols {
        if idx > 0 {
            text_buf[write] = b' ';
            write += 1;
        }
        for _ in 0..7 {
            text_buf[write] = b'-';
            write += 1;
        }
    }
    if write < text_buf.len() {
        text_buf[write] = b' ';
        write += 1;
    }
    str::from_utf8(&text_buf[..write]).map_err(|_| EpubError::Utf8)
}

fn table_big_border<'a>(text_buf: &'a mut [u8]) -> Result<&'a str, EpubError> {
    const LEFT: usize = 95;
    const RIGHT: usize = 664;
    let needed = LEFT + 1 + RIGHT;
    if needed > text_buf.len() {
        return Err(EpubError::OutOfSpace);
    }
    let mut write = 0usize;
    for _ in 0..LEFT {
        text_buf[write] = b'-';
        write += 1;
    }
    text_buf[write] = b' ';
    write += 1;
    for _ in 0..RIGHT {
        text_buf[write] = b'-';
        write += 1;
    }
    if write < text_buf.len() {
        text_buf[write] = b' ';
        write += 1;
    }
    str::from_utf8(&text_buf[..write]).map_err(|_| EpubError::Utf8)
}

fn parse_next_xhtml_event<'a>(
    data: &'a [u8],
    state: &mut ParserState,
    chapter_dir: &[u8],
    text_buf: &'a mut [u8],
) -> Result<Option<EpubEvent<'a>>, EpubError> {
    let text_buf_ptr = text_buf as *mut [u8];
    loop {
        if state.skip_tag_depth > 0 {
            let name_len = usize::from(state.skip_tag_name_len);
            let cursor_before = state.cursor;
            state.skip_tag_depth = skip_container_fast(
                data,
                &mut state.cursor,
                &state.skip_tag_name[..name_len],
                state.skip_tag_depth,
            )?;
            if state.skip_tag_depth == 0 {
                state.skip_tag_name_len = 0;
                continue;
            }
            if state.cursor == cursor_before {
                return Ok(None);
            }
            continue;
        }

        if state.cursor >= data.len() {
            return Ok(None);
        }

        if data[state.cursor] == b'<' {
            let Some(tag) = parse_xml_tag(data, &mut state.cursor)? else {
                continue;
            };
            if tag.is_end {
                if let Some(event) = unsafe { match_tag_end(tag, state, &mut *text_buf_ptr) }? {
                    return Ok(Some(event));
                }
                continue;
            }
            if should_fast_skip_container(&tag) {
                let local_name = trim_namespace(tag.name);
                if local_name.len() > state.skip_tag_name.len() {
                    return Err(EpubError::OutOfSpace);
                }
                state.skip_tag_name[..local_name.len()].copy_from_slice(local_name);
                state.skip_tag_name_len =
                    u8::try_from(local_name.len()).map_err(|_| EpubError::OutOfSpace)?;
                state.skip_tag_depth = 1;
                continue;
            }
            if let Some(event) =
                unsafe { parse_tag_start(tag, state, chapter_dir, &mut *text_buf_ptr) }?
            {
                return Ok(Some(event));
            }
            continue;
        }

        if let Some(text) = unsafe { parse_text(data, state, &mut *text_buf_ptr) }? {
            return Ok(Some(EpubEvent::Text(text)));
        }
    }
}

fn should_fast_skip_container(tag: &Tag<'_>) -> bool {
    (tag.name_is("nav")
        && tag
            .attr(b"type")
            .is_some_and(|value| attr_contains_token(value, b"toc")))
        || ((tag.name_is("body") || tag.name_is("section") || tag.name_is("div"))
            && tag.attr(b"type").is_some_and(|value| {
                attr_contains_token(value, b"titlepage") || attr_contains_token(value, b"cover")
            }))
}

fn skip_container_fast(
    data: &[u8],
    cursor: &mut usize,
    tag_name: &[u8],
    mut depth: u16,
) -> Result<u16, EpubError> {
    let local_name = trim_namespace(tag_name);
    while *cursor < data.len() {
        while *cursor < data.len() && data[*cursor] != b'<' {
            *cursor += 1;
        }
        if *cursor >= data.len() {
            return Ok(depth);
        }
        let before = *cursor;
        let Some(tag) = parse_xml_tag(data, cursor)? else {
            if *cursor == before {
                return Ok(depth);
            }
            continue;
        };
        if eq_ascii_case(trim_namespace(tag.name), local_name) {
            if tag.is_end {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Ok(0);
                }
            } else if !tag.is_self_closing {
                depth = depth.saturating_add(1);
            }
        }
    }
    Ok(depth)
}

fn parse_text<'a>(
    data: &[u8],
    state: &mut ParserState,
    out: &'a mut [u8],
) -> Result<Option<&'a str>, EpubError> {
    if state.ignore_depth > 0 {
        while state.cursor < data.len() && data[state.cursor] != b'<' {
            state.cursor += 1;
        }
        return Ok(None);
    }
    if state.chapter_is_nav_doc && state.nav_suppress_outside && state.nav_visible_depth == 0 {
        while state.cursor < data.len() && data[state.cursor] != b'<' {
            state.cursor += 1;
        }
        return Ok(None);
    }

    let mut out_len = 0usize;
    let mut did_write = false;

    while state.cursor < data.len() {
        let current = data[state.cursor];
        if current == b'<' {
            break;
        }
        let remaining = data.len() - state.cursor;
        if remaining < b"[HIDDEN]".len() && b"[HIDDEN]".starts_with(&data[state.cursor..]) {
            break;
        }
        if data[state.cursor..].starts_with(b"[HIDDEN]") {
            state.cursor += b"[HIDDEN]".len();
            continue;
        }
        if current == b'&' {
            let mut end = state.cursor + 1;
            while end < data.len() && data[end] != b';' {
                end += 1;
            }
            if end >= data.len() {
                break;
            }
            if end < data.len() {
                let entity_bytes = &data[state.cursor..=end];
                let mut tmp = [0u8; 4];
                let mut tmp_len = 0usize;
                if decode_entity(entity_bytes, &mut tmp, &mut tmp_len)? {
                    if out_len + tmp_len > out.len() {
                        return Err(EpubError::OutOfSpace);
                    }
                    out[out_len..out_len + tmp_len].copy_from_slice(&tmp[..tmp_len]);
                    out_len += tmp_len;
                    state.cursor = end + 1;
                    state.prev_space = matches!(
                        entity_bytes,
                        b"&nbsp;" | b"&#160;" | b"&#xA0;" | b"&#xa0;" | b"&#x00A0;" | b"&#x00a0;"
                    );
                    did_write = true;
                    continue;
                }
            }
        }

        state.cursor += 1;
        let mut ch = current;
        if !state.in_pre && is_space(ch) {
            let mut lookahead = state.cursor;
            while lookahead < data.len() && is_space(data[lookahead]) {
                lookahead += 1;
            }
            if let Some(prev) = state.last_nonspace_char
                && prev.is_ascii_uppercase()
                && matches!(data.get(lookahead..), Some(next) if next.len() >= 2
                    && next[0].is_ascii_uppercase()
                    && next[1] == b'.')
            {
                state.cursor = lookahead;
                continue;
            }
            if state.prev_space {
                continue;
            }
            ch = b' ';
            state.prev_space = true;
        } else {
            state.prev_space = false;
        }

        if out_len >= out.len() {
            return Err(EpubError::OutOfSpace);
        }
        out[out_len] = ch;
        out_len += 1;
        if !is_space(ch) {
            state.last_nonspace_char = Some(char::from(ch));
        }
        did_write = true;
    }

    if !did_write {
        return Ok(None);
    }

    Ok(Some(
        str::from_utf8(&out[..out_len]).map_err(|_| EpubError::Utf8)?,
    ))
}

fn match_tag_end<'a>(
    tag: Tag<'a>,
    state: &mut ParserState,
    text_buf: &'a mut [u8],
) -> Result<Option<EpubEvent<'a>>, EpubError> {
    if state.ignore_depth > 0 {
        state.ignore_depth = state.ignore_depth.saturating_sub(1);
        return Ok(None);
    }
    if state.chapter_is_nav_doc && state.nav_suppress_outside {
        if tag.name_is("nav") && state.nav_visible_depth > 0 {
            state.nav_visible_depth = state.nav_visible_depth.saturating_sub(1);
            return Ok(None);
        }
        if state.nav_visible_depth == 0 {
            return Ok(None);
        }
    }
    if tag.name_is("ol") || tag.name_is("ul") {
        if state.list_depth > 0 {
            state.list_depth -= 1;
        }
        return Ok(None);
    }
    if tag.name_is("p") {
        if state.in_paragraph {
            state.in_paragraph = false;
            return Ok(Some(EpubEvent::ParagraphEnd));
        }
        if tag.name_is("pre") {
            state.in_pre = false;
        }
        return Ok(None);
    }
    if tag.name_is("pre") {
        state.in_pre = false;
    }
    if tag.name_is("tr") {
        if state.table_in_head && state.table_row_cols > 0 {
            let cols = state.table_row_cols;
            state.table_row_cols = 0;
            return Ok(Some(EpubEvent::Text(table_small_separator(
                cols, text_buf,
            )?)));
        }
        state.table_row_cols = 0;
        return Ok(None);
    }
    if tag.name_is("tbody") {
        if !state.table_has_head {
            let border = table_big_border(text_buf)?;
            return Ok(Some(EpubEvent::Text(border)));
        }
        return Ok(None);
    }
    if tag.name_is("thead") {
        state.table_in_head = false;
        return Ok(None);
    }
    if tag.name_starts_with(b"h") {
        if let Some(level) = tag.heading_level() {
            if state.in_heading == level {
                state.in_heading = 0;
                return Ok(Some(EpubEvent::HeadingEnd));
            }
        }
        return Ok(None);
    }
    Ok(None)
}

fn parse_tag_start<'a>(
    tag: Tag<'a>,
    state: &mut ParserState,
    chapter_dir: &[u8],
    text_buf: &'a mut [u8],
) -> Result<Option<EpubEvent<'a>>, EpubError> {
    let is_ignored_container = tag.name_is("head")
        || tag.name_is("style")
        || tag.name_is("script")
        || (tag.name_is("nav")
            && tag
                .attr(b"type")
                .is_some_and(|value| attr_contains_token(value, b"toc")))
        || ((tag.name_is("body") || tag.name_is("section") || tag.name_is("div"))
            && tag.attr(b"type").is_some_and(|value| {
                attr_contains_token(value, b"titlepage") || attr_contains_token(value, b"cover")
            }));

    if state.ignore_depth > 0 {
        if !tag.is_self_closing {
            state.ignore_depth = state.ignore_depth.saturating_add(1);
        }
        return Ok(None);
    }

    if is_ignored_container || tag_is_css_hidden(&tag) {
        state.ignore_depth = state.ignore_depth.saturating_add(1);
        return Ok(None);
    }
    if state.chapter_is_nav_doc && tag.name_is("body") {
        state.nav_suppress_outside = tag
            .attr(b"type")
            .is_some_and(|value| attr_contains_token(value, b"frontmatter"));
        return Ok(None);
    }
    if state.chapter_is_nav_doc && state.nav_suppress_outside {
        if tag.name_is("nav") && !tag.is_self_closing {
            state.nav_visible_depth = state.nav_visible_depth.saturating_add(1);
            return Ok(None);
        }
        if state.nav_visible_depth == 0 {
            return Ok(None);
        }
    }
    if tag.name_is("ol") || tag.name_is("ul") {
        let depth = usize::from(state.list_depth);
        if depth < state.list_ordered.len() {
            state.list_ordered[depth] = tag.name_is("ol");
            state.list_items[depth] = 0;
            state.list_depth = state.list_depth.saturating_add(1);
        }
        return Ok(None);
    }
    if tag.name_is("li") {
        if state.list_depth > 0 {
            let depth = usize::from(state.list_depth - 1);
            if state.list_ordered[depth] {
                state.list_items[depth] = state.list_items[depth].saturating_add(1);
                let number = state.list_items[depth];
                let mut write = 0usize;
                if number >= 10 {
                    let tens = number / 10;
                    let ones = number % 10;
                    if write < text_buf.len() {
                        text_buf[write] = b'0' + u8::try_from(tens).unwrap_or(0);
                        write += 1;
                    }
                    if write < text_buf.len() {
                        text_buf[write] = b'0' + u8::try_from(ones).unwrap_or(0);
                        write += 1;
                    }
                } else if write < text_buf.len() {
                    text_buf[write] = b'0' + u8::try_from(number).unwrap_or(0);
                    write += 1;
                }
                if write + 1 <= text_buf.len() {
                    text_buf[write] = b'.';
                    write += 1;
                }
                if write < text_buf.len() {
                    text_buf[write] = b' ';
                    write += 1;
                }
                let number_text =
                    str::from_utf8(&text_buf[..write]).map_err(|_| EpubError::Utf8)?;
                return Ok(Some(EpubEvent::Text(number_text)));
            }
            if !state.list_ordered[depth] {
                if text_buf.len() < 2 {
                    return Err(EpubError::OutOfSpace);
                }
                text_buf[0] = b'-';
                text_buf[1] = b' ';
                let bullet = str::from_utf8(&text_buf[..2]).map_err(|_| EpubError::Utf8)?;
                return Ok(Some(EpubEvent::Text(bullet)));
            }
        }
        return Ok(None);
    }
    if tag.name_is("p") && !tag.is_self_closing {
        if !state.in_paragraph {
            state.in_paragraph = true;
            return Ok(Some(EpubEvent::ParagraphStart));
        }
        return Ok(None);
    }
    if tag.name_is("hr") {
        const HORIZONTAL_RULE: &[u8] =
            b"------------------------------------------------------------------------";
        if HORIZONTAL_RULE.len() > text_buf.len() {
            return Err(EpubError::OutOfSpace);
        }
        text_buf[..HORIZONTAL_RULE.len()].copy_from_slice(HORIZONTAL_RULE);
        let rule =
            str::from_utf8(&text_buf[..HORIZONTAL_RULE.len()]).map_err(|_| EpubError::Utf8)?;
        return Ok(Some(EpubEvent::Text(rule)));
    }
    if tag.name_starts_with(b"h") {
        if let Some(level) = tag.heading_level() {
            state.in_heading = level;
            return Ok(Some(EpubEvent::HeadingStart(level)));
        }
    }
    if tag.name_is("pre") {
        state.in_pre = true;
        return Ok(None);
    }
    if tag.name_is("br") {
        return Ok(Some(EpubEvent::LineBreak));
    }
    if tag.name_is("img") {
        let src_raw = tag.attr(b"src").ok_or(EpubError::InvalidFormat)?;
        if tag
            .attr(b"alt")
            .is_some_and(|value| attr_eq(value, b"hline"))
            || src_raw.ends_with(b"hline.jpg")
        {
            return Ok(None);
        }
        let mut resolved = [0u8; MAX_CHAPTER_DIR_BYTES];
        let resolved_len = match resolve_reference(chapter_dir, src_raw, &mut resolved) {
            Ok(value) => value,
            Err(EpubError::OutOfSpace) => return Err(EpubError::OutOfSpace),
            Err(error) => return Err(error),
        };
        let src = str::from_utf8(&resolved[..resolved_len]).map_err(|_| EpubError::Utf8)?;

        if src.len() > text_buf.len() {
            return Err(EpubError::OutOfSpace);
        }
        text_buf[..src.len()].copy_from_slice(src.as_bytes());
        let mut write = src.len();
        let alt = if let Some(alt_raw) = tag.attr(b"alt") {
            let alt = str::from_utf8(alt_raw).map_err(|_| EpubError::Utf8)?;
            if write >= text_buf.len() || write + alt.len() > text_buf.len() {
                return Err(EpubError::OutOfSpace);
            }
            text_buf[write..write + alt.len()].copy_from_slice(alt.as_bytes());
            let alt_start = write;
            write += alt.len();
            Some(str::from_utf8(&text_buf[alt_start..write]).map_err(|_| EpubError::Utf8)?)
        } else {
            None
        };

        let src_ref = str::from_utf8(&text_buf[..src.len()]).map_err(|_| EpubError::Utf8)?;
        return Ok(Some(EpubEvent::Image { src: src_ref, alt }));
    }
    if tag.name_is("table") {
        state.table_in_head = false;
        state.table_has_head = false;
        state.table_row_cols = 0;
        return Ok(Some(EpubEvent::UnsupportedTag));
    }
    if tag.name_is("thead") {
        state.table_in_head = true;
        state.table_has_head = true;
        state.table_row_cols = 0;
        return Ok(None);
    }
    if tag.name_is("tbody") {
        state.table_in_head = false;
        if !state.table_has_head {
            let border = table_big_border(text_buf)?;
            return Ok(Some(EpubEvent::Text(border)));
        }
        return Ok(None);
    }
    if tag.name_is("tr") {
        state.table_row_cols = 0;
        return Ok(None);
    }
    if tag.name_is("th") || tag.name_is("td") {
        state.table_row_cols = state.table_row_cols.saturating_add(1);
        return Ok(None);
    }
    if tag.is_unsupported() {
        return Ok(Some(EpubEvent::UnsupportedTag));
    }
    Ok(None)
}

fn resolve_reference(base: &[u8], reference: &[u8], out: &mut [u8]) -> Result<usize, EpubError> {
    let mut sanitized = reference;
    for idx in 0..reference.len() {
        if reference[idx] == b'#' || reference[idx] == b'?' {
            sanitized = &reference[..idx];
            break;
        }
    }

    if is_absolute_uri(sanitized) {
        if sanitized.len() > out.len() {
            return Err(EpubError::OutOfSpace);
        }
        out[..sanitized.len()].copy_from_slice(sanitized);
        return Ok(sanitized.len());
    }

    let mut decoded = [0u8; MAX_CHAPTER_DIR_BYTES];
    let decoded_len = percent_decode(sanitized, &mut decoded)?;

    let mut segment_end = [0usize; 24];
    let mut segment_count = 0usize;
    let mut out_len = 0usize;

    for segment in split_segments(base) {
        if apply_segment(
            segment,
            out,
            &mut out_len,
            &mut segment_end,
            &mut segment_count,
        )
        .is_err()
        {
            return Err(EpubError::OutOfSpace);
        }
    }
    for segment in split_segments(&decoded[..decoded_len]) {
        if segment == b"." || segment.is_empty() {
            continue;
        }
        if segment == b".." {
            if segment_count > 0 {
                segment_count -= 1;
                out_len = if segment_count == 0 {
                    0
                } else {
                    segment_end[segment_count - 1]
                };
            }
            continue;
        }
        apply_segment(
            segment,
            out,
            &mut out_len,
            &mut segment_end,
            &mut segment_count,
        )?;
    }

    Ok(out_len)
}

fn percent_decode(input: &[u8], out: &mut [u8]) -> Result<usize, EpubError> {
    let mut out_len = 0usize;
    let mut i = 0usize;
    while i < input.len() {
        if input[i] == b'%' && i + 2 < input.len() {
            if let (Some(high), Some(low)) = (from_hex(input[i + 1]), from_hex(input[i + 2])) {
                if out_len >= out.len() {
                    return Err(EpubError::OutOfSpace);
                }
                out[out_len] = (high << 4) | low;
                out_len += 1;
                i += 3;
                continue;
            }
        }
        if out_len >= out.len() {
            return Err(EpubError::OutOfSpace);
        }
        out[out_len] = input[i];
        out_len += 1;
        i += 1;
    }
    Ok(out_len)
}

fn from_hex(ch: u8) -> Option<u8> {
    match ch {
        b'0'..=b'9' => Some(ch - b'0'),
        b'a'..=b'f' => Some(ch - b'a' + 10),
        b'A'..=b'F' => Some(ch - b'A' + 10),
        _ => None,
    }
}

fn apply_segment(
    segment: &[u8],
    out: &mut [u8],
    out_len: &mut usize,
    segment_end: &mut [usize; 24],
    segment_count: &mut usize,
) -> Result<(), EpubError> {
    if segment.is_empty() || segment == b"." {
        return Ok(());
    }
    if segment == b".." {
        if *segment_count > 0 {
            *segment_count -= 1;
            *out_len = if *segment_count == 0 {
                0
            } else {
                segment_end[*segment_count - 1]
            };
        }
        return Ok(());
    }

    if *out_len > 0 {
        if *out_len + 1 > out.len() {
            return Err(EpubError::OutOfSpace);
        }
        out[*out_len] = b'/';
        *out_len += 1;
    }
    if *out_len + segment.len() > out.len() {
        return Err(EpubError::OutOfSpace);
    }
    out[*out_len..*out_len + segment.len()].copy_from_slice(segment);
    *out_len += segment.len();
    if *segment_count < segment_end.len() {
        segment_end[*segment_count] = *out_len;
        *segment_count += 1;
    } else {
        return Err(EpubError::OutOfSpace);
    }
    Ok(())
}

fn is_absolute_uri(value: &[u8]) -> bool {
    for i in 0..value.len().saturating_sub(2) {
        if value[i] == b':' && value[i + 1] == b'/' && value[i + 2] == b'/' {
            return true;
        }
    }
    false
}

fn path_parent(path: &[u8]) -> &[u8] {
    let mut last = None;
    for idx in (0..path.len()).rev() {
        if path[idx] == b'/' {
            last = Some(idx);
            break;
        }
    }
    match last {
        Some(pos) if pos > 0 => &path[..pos],
        _ => &[],
    }
}

fn decode_entity(bytes: &[u8], out: &mut [u8], out_len: &mut usize) -> Result<bool, EpubError> {
    if bytes.first() != Some(&b'&') || bytes.last() != Some(&b';') || bytes.len() < 3 {
        return Ok(false);
    }
    let entity = &bytes[1..bytes.len() - 1];

    let cp = match entity {
        b"lt" => Some('<' as u32),
        b"gt" => Some('>' as u32),
        b"amp" => Some('&' as u32),
        b"copy" => Some('©' as u32),
        b"quot" => Some('"' as u32),
        b"apos" => Some('\'' as u32),
        b"nbsp" => Some(0x00A0),
        _ if entity.starts_with(b"#x") => {
            Some(parse_num_radix(&entity[2..], 16).ok_or(EpubError::InvalidFormat)?)
        }
        _ if entity.starts_with(b"#") => {
            Some(parse_num_radix(&entity[1..], 10).ok_or(EpubError::InvalidFormat)?)
        }
        _ => return Ok(false),
    };

    let cp = cp.ok_or(EpubError::InvalidFormat)?;
    let ch = core::char::from_u32(cp).ok_or(EpubError::InvalidFormat)?;
    let mut buf = [0u8; 4];
    let written = ch.encode_utf8(&mut buf).len();
    if *out_len + written > out.len() {
        return Err(EpubError::OutOfSpace);
    }
    out[*out_len..*out_len + written].copy_from_slice(&buf[..written]);
    *out_len += written;
    Ok(true)
}

fn parse_num_radix(input: &[u8], radix: u32) -> Option<u32> {
    let mut value = 0u32;
    for ch in input {
        let v = match ch {
            b'0'..=b'9' => u32::from(ch - b'0'),
            b'a'..=b'f' if radix == 16 => u32::from(ch - b'a' + 10),
            b'A'..=b'F' if radix == 16 => u32::from(ch - b'A' + 10),
            _ => return None,
        };
        value = value.checked_mul(radix)?.checked_add(v)?;
    }
    Some(value)
}

fn is_space(ch: u8) -> bool {
    ch == b' ' || ch == b'\n' || ch == b'\r' || ch == b'\t'
}

fn split_segments(path: &[u8]) -> Segments<'_> {
    Segments { path, idx: 0 }
}

struct Segments<'a> {
    path: &'a [u8],
    idx: usize,
}

impl<'a> Iterator for Segments<'a> {
    type Item = &'a [u8];
    fn next(&mut self) -> Option<Self::Item> {
        while self.idx < self.path.len() && self.path[self.idx] == b'/' {
            self.idx += 1;
        }
        if self.idx >= self.path.len() {
            return None;
        }
        let start = self.idx;
        while self.idx < self.path.len() && self.path[self.idx] != b'/' {
            self.idx += 1;
        }
        Some(&self.path[start..self.path.len().min(self.idx)])
    }
}

#[derive(Debug, Clone, Copy)]
struct Tag<'a> {
    name: &'a [u8],
    attrs: &'a [u8],
    is_end: bool,
    is_self_closing: bool,
}

impl<'a> Tag<'a> {
    fn is_unsupported(self) -> bool {
        self.name_is("table")
            || self.name_is("thead")
            || self.name_is("tbody")
            || self.name_is("tfoot")
            || self.name_is("tr")
            || self.name_is("td")
            || self.name_is("th")
    }

    fn name_is(&self, expected: &str) -> bool {
        eq_ascii_case(trim_namespace(self.name), expected.as_bytes())
    }

    fn name_starts_with(&self, expected: &[u8]) -> bool {
        let name = trim_namespace(self.name);
        if name.len() < expected.len() {
            return false;
        }
        &name[..expected.len()] == expected
    }

    fn heading_level(&self) -> Option<u8> {
        let name = trim_namespace(self.name);
        if name.len() != 2 || (name[0] != b'h' && name[0] != b'H') {
            return None;
        }
        let level = name[1].wrapping_sub(b'0');
        if (1..=6).contains(&level) {
            Some(level)
        } else {
            None
        }
    }

    fn attr(&self, key: &[u8]) -> Option<&'a [u8]> {
        find_attr(self.attrs, key)
    }
}

fn parse_xml_tag<'a>(data: &'a [u8], cursor: &mut usize) -> Result<Option<Tag<'a>>, EpubError> {
    let tag_start = *cursor;
    while *cursor < data.len() && data[*cursor].is_ascii_whitespace() {
        *cursor += 1;
    }
    if *cursor >= data.len() || data[*cursor] != b'<' {
        return Ok(None);
    }

    *cursor += 1;
    if *cursor >= data.len() {
        *cursor = tag_start;
        return Ok(None);
    }

    if data[*cursor] == b'!' {
        skip_special(data, cursor)?;
        return Ok(None);
    }
    if data[*cursor] == b'?' {
        while *cursor < data.len() && data[*cursor] != b'>' {
            *cursor += 1;
        }
        if *cursor < data.len() {
            *cursor += 1;
        }
        return Ok(None);
    }

    let mut is_end = false;
    if data[*cursor] == b'/' {
        is_end = true;
        *cursor += 1;
    }

    let name_start = *cursor;
    while *cursor < data.len() {
        let ch = data[*cursor];
        if ch.is_ascii_whitespace() || ch == b'/' || ch == b'>' {
            break;
        }
        *cursor += 1;
    }
    if name_start == *cursor {
        return Err(EpubError::InvalidFormat);
    }

    while *cursor < data.len() && data[*cursor] != b'>' {
        if data[*cursor] == b'\'' || data[*cursor] == b'"' {
            let quote = data[*cursor];
            *cursor += 1;
            while *cursor < data.len() && data[*cursor] != quote {
                *cursor += 1;
            }
            if *cursor >= data.len() {
                *cursor = tag_start;
                return Ok(None);
            }
            *cursor += 1;
            continue;
        }
        *cursor += 1;
    }
    if *cursor >= data.len() {
        *cursor = tag_start;
        return Ok(None);
    }

    let is_self_closing = *cursor > name_start && data[*cursor - 1] == b'/';
    let tag_end = *cursor;
    *cursor += 1;
    let name_len = tag_name_len(data, name_start, tag_end)?;
    let attrs_start = name_start + name_len;
    let attrs_end = if is_self_closing {
        tag_end - 1
    } else {
        tag_end
    };
    let attrs = if attrs_start <= attrs_end && attrs_end <= data.len() {
        &data[attrs_start..attrs_end]
    } else {
        &[]
    };

    Ok(Some(Tag {
        name: &data[name_start..name_start + name_len],
        attrs,
        is_end,
        is_self_closing,
    }))
}

fn tag_name_len(data: &[u8], start: usize, end: usize) -> Result<usize, EpubError> {
    for idx in start..end {
        let ch = data[idx];
        if ch.is_ascii_whitespace() || ch == b'/' || ch == b'>' {
            return Ok(idx - start);
        }
    }
    Ok(end - start)
}

fn skip_special(data: &[u8], cursor: &mut usize) -> Result<(), EpubError> {
    if *cursor + 2 >= data.len() {
        return Err(EpubError::InvalidFormat);
    }
    if data.get(*cursor..*cursor + 3) == Some(b"!--") {
        *cursor += 3;
        while *cursor + 3 <= data.len() {
            if &data[*cursor..*cursor + 3] == b"-->" {
                *cursor += 3;
                return Ok(());
            }
            *cursor += 1;
        }
        return Err(EpubError::InvalidFormat);
    }
    while *cursor < data.len() && data[*cursor] != b'>' {
        *cursor += 1;
    }
    if *cursor < data.len() {
        *cursor += 1;
        return Ok(());
    }
    Err(EpubError::InvalidFormat)
}

fn find_attr<'a>(attrs: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    let mut cursor = 0usize;
    while cursor < attrs.len() {
        while cursor < attrs.len() && attrs[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor >= attrs.len() {
            return None;
        }

        let name_start = cursor;
        while cursor < attrs.len() {
            let ch = attrs[cursor];
            if ch == b'=' || ch.is_ascii_whitespace() {
                break;
            }
            cursor += 1;
        }
        if cursor >= attrs.len() || name_start == cursor {
            return None;
        }
        let raw_name = &attrs[name_start..cursor];
        while cursor < attrs.len() && attrs[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor >= attrs.len() || attrs[cursor] != b'=' {
            while cursor < attrs.len() && attrs[cursor] != b' ' {
                cursor += 1;
            }
            continue;
        }
        cursor += 1;
        while cursor < attrs.len() && attrs[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor >= attrs.len() {
            return None;
        }
        let quote = attrs[cursor];
        if quote != b'"' && quote != b'\'' {
            return None;
        }
        cursor += 1;
        let value_start = cursor;
        while cursor < attrs.len() && attrs[cursor] != quote {
            cursor += 1;
        }
        if cursor >= attrs.len() {
            return None;
        }
        let value = &attrs[value_start..cursor];
        if eq_ascii_case(trim_namespace(raw_name), key) {
            return Some(value);
        }
        cursor += 1;
    }
    None
}

fn trim_namespace(name: &[u8]) -> &[u8] {
    for idx in (0..name.len()).rev() {
        if name[idx] == b':' {
            return &name[idx + 1..];
        }
    }
    name
}

fn eq_ascii_case(lhs: &[u8], rhs: &[u8]) -> bool {
    lhs.len() == rhs.len()
        && lhs
            .iter()
            .zip(rhs.iter())
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
}

fn read_entry<'a>(
    source: &impl EpubSource,
    entry: &EpubEntryMetadata,
    output: &'a mut [u8],
    compressed: &'a mut [u8],
) -> Result<&'a [u8], EpubError> {
    let compressed_size =
        usize::try_from(entry.compressed_size).map_err(|_| EpubError::InvalidFormat)?;
    let uncompressed_size =
        usize::try_from(entry.uncompressed_size).map_err(|_| EpubError::InvalidFormat)?;
    let data_offset = u64::from(entry.data_offset);

    match entry.compression {
        CompressionMethod::Stored => {
            if uncompressed_size > output.len() {
                return Err(EpubError::OutOfSpace);
            }
            read_exact(source, data_offset, &mut output[..uncompressed_size])?;
            Ok(&output[..uncompressed_size])
        }
        CompressionMethod::Deflate => {
            if compressed_size > compressed.len() || uncompressed_size > output.len() {
                return Err(EpubError::OutOfSpace);
            }
            if compressed_size == 0 {
                return Ok(&[]);
            }
            read_exact(source, data_offset, &mut compressed[..compressed_size])?;
            let written = decompress_slice_iter_to_slice(
                &mut output[..uncompressed_size],
                core::iter::once(&compressed[..compressed_size]),
                false,
                false,
            )
            .map_err(map_inflate_error)?;
            Ok(&output[..written])
        }
        CompressionMethod::Other(_) => Err(EpubError::Unsupported),
    }
}

fn map_inflate_error(error: TINFLStatus) -> EpubError {
    match error {
        TINFLStatus::HasMoreOutput => EpubError::OutOfSpace,
        TINFLStatus::BadParam => EpubError::InvalidFormat,
        TINFLStatus::Adler32Mismatch => EpubError::Compression,
        TINFLStatus::FailedCannotMakeProgress
        | TINFLStatus::Failed
        | TINFLStatus::NeedsMoreInput => EpubError::Compression,
        _ => EpubError::Compression,
    }
}

fn read_exact(source: &impl EpubSource, offset: u64, buffer: &mut [u8]) -> Result<(), EpubError> {
    let mut cursor = 0usize;
    while cursor < buffer.len() {
        let read = source
            .read_at(offset + cursor as u64, &mut buffer[cursor..])
            .map_err(|_| EpubError::Io)?;
        if read == 0 {
            return Err(EpubError::Io);
        }
        cursor = cursor.saturating_add(read);
    }
    Ok(())
}
