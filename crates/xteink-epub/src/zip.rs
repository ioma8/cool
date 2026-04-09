const EOCD_SIGNATURE: u32 = 0x0605_4B50;
const CENTRAL_DIRECTORY_SIGNATURE: u32 = 0x0201_4B50;
const LOCAL_FILE_HEADER_SIGNATURE: u32 = 0x0403_4B50;

const EOCD_FIXED_SIZE: usize = 22;
const CENTRAL_DIR_FILE_HEADER_SIZE: usize = 46;
const LOCAL_FILE_HEADER_SIZE: usize = 30;
const MAX_EOCD_LOOKBACK: usize = EOCD_FIXED_SIZE + 0xFFFF;
const EOCD_SCAN_CHUNK: usize = 1024;
const EOCD_SIGNATURE_LEN: usize = 4;

#[derive(Clone, Copy)]
pub struct MemorySource<'a> {
    data: &'a [u8],
}

impl<'a> MemorySource<'a> {
    pub const fn new(data: &'a [u8]) -> Self {
        Self { data }
    }
}

/// A source abstraction used to parse ZIP data without owning backing storage.
pub trait ZipSource {
    fn len(&self) -> usize;

    /// Reads bytes at an absolute offset into `buffer` and returns the number of bytes
    /// copied. `read_at` may legally return a short read.
    fn read_at(&self, offset: u64, buffer: &mut [u8]) -> Result<usize, ()>;
}

impl<'a> ZipSource for MemorySource<'a> {
    fn len(&self) -> usize {
        self.data.len()
    }

    fn read_at(&self, offset: u64, buffer: &mut [u8]) -> Result<usize, ()> {
        let offset = match usize::try_from(offset) {
            Ok(value) => value,
            Err(_) => return Ok(0),
        };

        if buffer.is_empty() || offset >= self.data.len() {
            return Ok(0);
        }

        let to_copy = (self.data.len() - offset).min(buffer.len());
        buffer[..to_copy].copy_from_slice(&self.data[offset..offset + to_copy]);
        Ok(to_copy)
    }
}

/// Re-exported alias for a slice-backed source, useful for unit tests and firmware mirrors.
pub type SliceSource<'a> = MemorySource<'a>;

#[derive(Debug)]
pub enum Error {
    /// A read from the backing source failed.
    Source,
    /// The EOCD record could not be found in the searchable trailer window.
    EocdNotFound,
    /// The source ended before the parser could satisfy a read request.
    ShortRead,
    /// The ZIP data is structurally invalid or unsupported for this parser.
    InvalidArchive(&'static str),
    /// The file used more records than the fixed array permits.
    TooManyEntries,
    /// The file name buffer is too small for fixed-size metadata storage.
    NameBufferTooSmall,
    /// Arithmetic overflow was detected while calculating offsets.
    ArithmeticOverflow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionMethod {
    Stored,
    Deflate,
    Other(u16),
}

impl CompressionMethod {
    fn from_u16(raw: u16) -> Self {
        match raw {
            0 => Self::Stored,
            8 => Self::Deflate,
            other => Self::Other(other),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct EndOfCentralDirectory {
    pub disk_number: u16,
    pub cd_entries_on_disk: u16,
    pub cd_entries_total: u16,
    pub cd_size: u32,
    pub cd_offset: u32,
    pub comment_length: u16,
}

impl EndOfCentralDirectory {
    pub const fn empty() -> Self {
        Self {
            disk_number: 0,
            cd_entries_on_disk: 0,
            cd_entries_total: 0,
            cd_size: 0,
            cd_offset: 0,
            comment_length: 0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct EpubEntryMetadata {
    pub compression: CompressionMethod,
    pub crc32: u32,
    pub compressed_size: u32,
    pub uncompressed_size: u32,
    pub local_header_offset: u32,
    pub data_offset: u32,
}

#[derive(Debug)]
pub struct EpubArchive<const MAX_ENTRIES: usize, const NAME_CAPACITY: usize> {
    eocd: EndOfCentralDirectory,
    entry_count: usize,
}

impl<const MAX_ENTRIES: usize, const NAME_CAPACITY: usize> EpubArchive<MAX_ENTRIES, NAME_CAPACITY> {
    pub const fn new() -> Self {
        Self {
            eocd: EndOfCentralDirectory::empty(),
            entry_count: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.entry_count
    }

    pub const fn eocd(&self) -> &EndOfCentralDirectory {
        &self.eocd
    }

    pub fn central_directory_size(&self) -> usize {
        usize::try_from(self.eocd.cd_size).unwrap_or(usize::MAX)
    }

    pub fn clear(&mut self) {
        self.entry_count = 0;
    }

    pub fn parse<S: ZipSource>(&mut self, source: &S) -> Result<(), Error> {
        self.clear();

        let file_size = source.len() as u64;
        let eocd = find_eocd(source, file_size)?;

        if eocd.disk_number != 0 || eocd.cd_entries_on_disk != eocd.cd_entries_total {
            return Err(Error::InvalidArchive(
                "multi-disk archives are not supported",
            ));
        }

        let entry_count = usize::from(eocd.cd_entries_total);
        if entry_count > MAX_ENTRIES {
            return Err(Error::TooManyEntries);
        }

        let cd_start = u64::from(eocd.cd_offset);
        let cd_end = cd_start
            .checked_add(u64::from(eocd.cd_size))
            .ok_or(Error::ArithmeticOverflow)?;
        if cd_end > file_size {
            return Err(Error::InvalidArchive(
                "central directory exceeds source length",
            ));
        }

        let mut cursor = cd_start;
        for _ in 0..entry_count {
            let entry = parse_central_directory_entry(source, cursor)?;
            self.entry_count = self.entry_count.saturating_add(1);
            cursor = cursor
                .checked_add(CENTRAL_DIR_FILE_HEADER_SIZE as u64)
                .and_then(|value| value.checked_add(u64::from(entry.name_len)))
                .and_then(|value| value.checked_add(u64::from(entry.extra_length)))
                .and_then(|value| value.checked_add(u64::from(entry.comment_length)))
                .ok_or(Error::ArithmeticOverflow)?;
        }

        if cursor != cd_end {
            return Err(Error::InvalidArchive(
                "central directory size did not match parsed entry headers",
            ));
        }

        self.eocd = eocd;
        Ok(())
    }

    pub fn entry_by_name<S: ZipSource>(
        &self,
        source: &S,
        name: &str,
        scratch: &mut [u8],
    ) -> Result<Option<EpubEntryMetadata>, Error> {
        let needle = name.as_bytes();
        self.entry_by_name_bytes(source, needle, scratch)
    }

    pub fn entry_by_name_bytes<S: ZipSource>(
        &self,
        source: &S,
        needle: &[u8],
        scratch: &mut [u8],
    ) -> Result<Option<EpubEntryMetadata>, Error> {
        let cd_size = self.central_directory_size();
        if cd_size <= scratch.len() {
            let cd_start = u64::from(self.eocd.cd_offset);
            read_exact_at(source, cd_start, &mut scratch[..cd_size])?;
            return self.entry_by_name_bytes_in_cd(source, needle, &scratch[..cd_size]);
        }

        if let Some(found) = self.entry_by_name_bytes_chunked(source, needle, scratch)? {
            return Ok(Some(found));
        }

        let cd_start = u64::from(self.eocd.cd_offset);
        let cd_end = cd_start
            .checked_add(u64::from(self.eocd.cd_size))
            .ok_or(Error::ArithmeticOverflow)?;
        let mut cursor = cd_start;

        while cursor < cd_end {
            let entry = parse_central_directory_entry(source, cursor)?;
            let name_len = usize::from(entry.name_len);
            if name_len > scratch.len() {
                return Err(Error::NameBufferTooSmall);
            }

            read_exact_at(
                source,
                cursor + CENTRAL_DIR_FILE_HEADER_SIZE as u64,
                &mut scratch[..name_len],
            )?;

            if &scratch[..name_len] == needle {
                let data_offset =
                    compute_local_data_offset(source, u64::from(entry.local_header_offset))?;
                return Ok(Some(EpubEntryMetadata {
                    compression: entry.compression,
                    crc32: entry.crc32,
                    compressed_size: entry.compressed_size,
                    uncompressed_size: entry.uncompressed_size,
                    local_header_offset: entry.local_header_offset,
                    data_offset,
                }));
            }

            cursor = cursor
                .checked_add(CENTRAL_DIR_FILE_HEADER_SIZE as u64)
                .and_then(|value| value.checked_add(u64::from(entry.name_len)))
                .and_then(|value| value.checked_add(u64::from(entry.extra_length)))
                .and_then(|value| value.checked_add(u64::from(entry.comment_length)))
                .ok_or(Error::ArithmeticOverflow)?;
        }

        Ok(None)
    }

    fn entry_by_name_bytes_chunked<S: ZipSource>(
        &self,
        source: &S,
        needle: &[u8],
        scratch: &mut [u8],
    ) -> Result<Option<EpubEntryMetadata>, Error> {
        let cd_start = u64::from(self.eocd.cd_offset);
        let cd_end = cd_start
            .checked_add(u64::from(self.eocd.cd_size))
            .ok_or(Error::ArithmeticOverflow)?;
        let mut cursor = cd_start;

        while cursor < cd_end {
            let read_len = usize::try_from((cd_end - cursor).min(scratch.len() as u64))
                .map_err(|_| Error::ArithmeticOverflow)?;
            read_exact_at(source, cursor, &mut scratch[..read_len])?;

            let mut local_cursor = 0usize;
            while local_cursor < read_len {
                let remaining = read_len - local_cursor;
                if remaining < CENTRAL_DIR_FILE_HEADER_SIZE {
                    break;
                }
                let entry =
                    parse_central_directory_entry_header(&scratch[local_cursor..local_cursor + CENTRAL_DIR_FILE_HEADER_SIZE])?;
                let total_len = CENTRAL_DIR_FILE_HEADER_SIZE
                    .checked_add(usize::from(entry.name_len))
                    .and_then(|value| value.checked_add(usize::from(entry.extra_length)))
                    .and_then(|value| value.checked_add(usize::from(entry.comment_length)))
                    .ok_or(Error::ArithmeticOverflow)?;

                if total_len > scratch.len() {
                    break;
                }
                if local_cursor + total_len > read_len {
                    break;
                }

                let name_start = local_cursor + CENTRAL_DIR_FILE_HEADER_SIZE;
                let name_end = name_start + usize::from(entry.name_len);
                if &scratch[name_start..name_end] == needle {
                    let data_offset =
                        compute_local_data_offset(source, u64::from(entry.local_header_offset))?;
                    return Ok(Some(EpubEntryMetadata {
                        compression: entry.compression,
                        crc32: entry.crc32,
                        compressed_size: entry.compressed_size,
                        uncompressed_size: entry.uncompressed_size,
                        local_header_offset: entry.local_header_offset,
                        data_offset,
                    }));
                }

                local_cursor = local_cursor
                    .checked_add(total_len)
                    .ok_or(Error::ArithmeticOverflow)?;
            }

            if local_cursor == 0 {
                break;
            }

            cursor = cursor
                .checked_add(u64::try_from(local_cursor).map_err(|_| Error::ArithmeticOverflow)?)
                .ok_or(Error::ArithmeticOverflow)?;
        }

        Ok(None)
    }

    pub fn load_central_directory<'a, S: ZipSource>(
        &self,
        source: &S,
        scratch: &'a mut [u8],
    ) -> Result<Option<&'a [u8]>, Error> {
        let cd_size = self.central_directory_size();
        if cd_size > scratch.len() {
            return Ok(None);
        }
        read_exact_at(source, u64::from(self.eocd.cd_offset), &mut scratch[..cd_size])?;
        Ok(Some(&scratch[..cd_size]))
    }

    pub fn entry_by_name_bytes_in_cd<S: ZipSource>(
        &self,
        source: &S,
        needle: &[u8],
        cd: &[u8],
    ) -> Result<Option<EpubEntryMetadata>, Error> {
        let mut cursor = 0usize;
        while cursor < cd.len() {
            let entry = parse_central_directory_entry_from_slice(cd, cursor)?;
            let name_start = cursor + CENTRAL_DIR_FILE_HEADER_SIZE;
            let name_end = name_start + usize::from(entry.name_len);
            if name_end > cd.len() {
                return Err(Error::InvalidArchive("entry name exceeds central directory"));
            }
            if &cd[name_start..name_end] == needle {
                let data_offset =
                    compute_local_data_offset(source, u64::from(entry.local_header_offset))?;
                return Ok(Some(EpubEntryMetadata {
                    compression: entry.compression,
                    crc32: entry.crc32,
                    compressed_size: entry.compressed_size,
                    uncompressed_size: entry.uncompressed_size,
                    local_header_offset: entry.local_header_offset,
                    data_offset,
                }));
            }
            cursor = cursor
                .checked_add(CENTRAL_DIR_FILE_HEADER_SIZE)
                .and_then(|value| value.checked_add(usize::from(entry.name_len)))
                .and_then(|value| value.checked_add(usize::from(entry.extra_length)))
                .and_then(|value| value.checked_add(usize::from(entry.comment_length)))
                .ok_or(Error::ArithmeticOverflow)?;
        }
        Ok(None)
    }

    pub fn for_each_entry<S: ZipSource, F>(
        &self,
        source: &S,
        scratch: &mut [u8],
        mut visitor: F,
    ) -> Result<(), Error>
    where
        F: FnMut(&[u8], EpubEntryMetadata) -> Result<(), Error>,
    {
        let cd_size = self.central_directory_size();
        if cd_size <= scratch.len() {
            let cd_start = u64::from(self.eocd.cd_offset);
            read_exact_at(source, cd_start, &mut scratch[..cd_size])?;
            return for_each_entry_in_cd(source, &scratch[..cd_size], &mut visitor);
        }

        let cd_start = u64::from(self.eocd.cd_offset);
        let cd_end = cd_start
            .checked_add(u64::from(self.eocd.cd_size))
            .ok_or(Error::ArithmeticOverflow)?;
        let mut cursor = cd_start;

        while cursor < cd_end {
            let read_len = usize::try_from((cd_end - cursor).min(scratch.len() as u64))
                .map_err(|_| Error::ArithmeticOverflow)?;
            read_exact_at(source, cursor, &mut scratch[..read_len])?;

            let mut local_cursor = 0usize;
            while local_cursor < read_len {
                let remaining = read_len - local_cursor;
                if remaining < CENTRAL_DIR_FILE_HEADER_SIZE {
                    break;
                }
                let entry =
                    parse_central_directory_entry_header(&scratch[local_cursor..local_cursor + CENTRAL_DIR_FILE_HEADER_SIZE])?;
                let total_len = CENTRAL_DIR_FILE_HEADER_SIZE
                    .checked_add(usize::from(entry.name_len))
                    .and_then(|value| value.checked_add(usize::from(entry.extra_length)))
                    .and_then(|value| value.checked_add(usize::from(entry.comment_length)))
                    .ok_or(Error::ArithmeticOverflow)?;

                if total_len > scratch.len() || local_cursor + total_len > read_len {
                    break;
                }

                let name_start = local_cursor + CENTRAL_DIR_FILE_HEADER_SIZE;
                let name_end = name_start + usize::from(entry.name_len);
                let data_offset =
                    compute_local_data_offset(source, u64::from(entry.local_header_offset))?;
                visitor(
                    &scratch[name_start..name_end],
                    EpubEntryMetadata {
                        compression: entry.compression,
                        crc32: entry.crc32,
                        compressed_size: entry.compressed_size,
                        uncompressed_size: entry.uncompressed_size,
                        local_header_offset: entry.local_header_offset,
                        data_offset,
                    },
                )?;

                local_cursor = local_cursor
                    .checked_add(total_len)
                    .ok_or(Error::ArithmeticOverflow)?;
            }

            if local_cursor == 0 {
                return Err(Error::NameBufferTooSmall);
            }

            cursor = cursor
                .checked_add(u64::try_from(local_cursor).map_err(|_| Error::ArithmeticOverflow)?)
                .ok_or(Error::ArithmeticOverflow)?;
        }

        Ok(())
    }
}

fn for_each_entry_in_cd<S: ZipSource, F>(
    source: &S,
    cd: &[u8],
    visitor: &mut F,
) -> Result<(), Error>
where
    F: FnMut(&[u8], EpubEntryMetadata) -> Result<(), Error>,
{
    let mut cursor = 0usize;
    while cursor < cd.len() {
        let entry = parse_central_directory_entry_from_slice(cd, cursor)?;
        let name_start = cursor + CENTRAL_DIR_FILE_HEADER_SIZE;
        let name_end = name_start + usize::from(entry.name_len);
        if name_end > cd.len() {
            return Err(Error::InvalidArchive("entry name exceeds central directory"));
        }
        let data_offset = compute_local_data_offset(source, u64::from(entry.local_header_offset))?;
        visitor(
            &cd[name_start..name_end],
            EpubEntryMetadata {
                compression: entry.compression,
                crc32: entry.crc32,
                compressed_size: entry.compressed_size,
                uncompressed_size: entry.uncompressed_size,
                local_header_offset: entry.local_header_offset,
                data_offset,
            },
        )?;
        cursor = cursor
            .checked_add(CENTRAL_DIR_FILE_HEADER_SIZE)
            .and_then(|value| value.checked_add(usize::from(entry.name_len)))
            .and_then(|value| value.checked_add(usize::from(entry.extra_length)))
            .and_then(|value| value.checked_add(usize::from(entry.comment_length)))
            .ok_or(Error::ArithmeticOverflow)?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct ParsedCentralDirectoryEntry {
    compression: CompressionMethod,
    crc32: u32,
    compressed_size: u32,
    uncompressed_size: u32,
    name_len: u16,
    extra_length: u16,
    comment_length: u16,
    local_header_offset: u32,
}

fn find_eocd<S: ZipSource>(source: &S, file_size: u64) -> Result<EndOfCentralDirectory, Error> {
    if file_size == 0 || file_size < EOCD_FIXED_SIZE as u64 {
        return Err(Error::EocdNotFound);
    }

    let search_size = if file_size > MAX_EOCD_LOOKBACK as u64 {
        MAX_EOCD_LOOKBACK
    } else {
        file_size as usize
    };

    let scan_start = file_size.saturating_sub(search_size as u64);

    let mut chunk = [0u8; EOCD_SCAN_CHUNK];
    let mut carry = [0u8; EOCD_SIGNATURE_LEN - 1];
    let mut carry_len = 0usize;
    let mut cursor = scan_start;
    let mut window = [0u8; EOCD_SCAN_CHUNK + EOCD_SIGNATURE_LEN - 1];

    while cursor < file_size {
        let remaining = file_size - cursor;
        let read_size = (remaining as usize).min(EOCD_SCAN_CHUNK);
        read_exact_at(source, cursor, &mut chunk[..read_size])?;

        let window_len = carry_len + read_size;
        window[..carry_len].copy_from_slice(&carry[..carry_len]);
        window[carry_len..window_len].copy_from_slice(&chunk[..read_size]);

        let window_base = cursor
            .checked_sub(u64::try_from(carry_len).map_err(|_| Error::ArithmeticOverflow)?)
            .ok_or(Error::ArithmeticOverflow)?;

        if window_len >= EOCD_FIXED_SIZE {
            for pos in 0..=(window_len - EOCD_FIXED_SIZE) {
                if u32_from(&window[pos..pos + 4]) != EOCD_SIGNATURE {
                    continue;
                }

                let comment_len = u16_from(&window[pos + 20..pos + 22]);
                let record_length = EOCD_FIXED_SIZE + usize::from(comment_len);
                let absolute = window_base + pos as u64;
                if absolute < scan_start {
                    continue;
                }

                if let Some(value) = absolute.checked_add(record_length as u64) {
                    if value != file_size {
                        continue;
                    }
                } else {
                    continue;
                }

                return Ok(EndOfCentralDirectory {
                    disk_number: u16_from(&window[pos + 4..pos + 6]),
                    cd_entries_on_disk: u16_from(&window[pos + 8..pos + 10]),
                    cd_entries_total: u16_from(&window[pos + 10..pos + 12]),
                    cd_size: u32_from(&window[pos + 12..pos + 16]),
                    cd_offset: u32_from(&window[pos + 16..pos + 20]),
                    comment_length: comment_len,
                });
            }
        }

        let carry_keep = core::cmp::min(EOCD_SIGNATURE_LEN - 1, window_len);
        let carry_start = window_len - carry_keep;
        carry[..carry_keep].copy_from_slice(&window[carry_start..window_len]);
        carry_len = carry_keep;

        cursor = cursor
            .checked_add(read_size as u64)
            .ok_or(Error::ArithmeticOverflow)?;
    }

    Err(Error::EocdNotFound)
}

fn parse_central_directory_entry<S: ZipSource>(
    source: &S,
    cursor: u64,
) -> Result<ParsedCentralDirectoryEntry, Error> {
    let mut header = [0u8; CENTRAL_DIR_FILE_HEADER_SIZE];
    read_exact_at(source, cursor, &mut header)?;
    parse_central_directory_entry_header(&header)
}

fn parse_central_directory_entry_from_slice(
    cd: &[u8],
    cursor: usize,
) -> Result<ParsedCentralDirectoryEntry, Error> {
    let end = cursor
        .checked_add(CENTRAL_DIR_FILE_HEADER_SIZE)
        .ok_or(Error::ArithmeticOverflow)?;
    if end > cd.len() {
        return Err(Error::ShortRead);
    }
    parse_central_directory_entry_header(&cd[cursor..end])
}

fn parse_central_directory_entry_header(
    header: &[u8],
) -> Result<ParsedCentralDirectoryEntry, Error> {
    if u32_from(&header[0..4]) != CENTRAL_DIRECTORY_SIGNATURE {
        return Err(Error::InvalidArchive("invalid central directory signature"));
    }

    let name_len = u16_from(&header[28..30]);
    let extra_length = u16_from(&header[30..32]);
    let comment_length = u16_from(&header[32..34]);
    let local_header_offset = u32_from(&header[42..46]);
    Ok(ParsedCentralDirectoryEntry {
        compression: CompressionMethod::from_u16(u16_from(&header[10..12])),
        crc32: u32_from(&header[16..20]),
        compressed_size: u32_from(&header[20..24]),
        uncompressed_size: u32_from(&header[24..28]),
        name_len,
        extra_length,
        comment_length,
        local_header_offset,
    })
}

fn compute_local_data_offset<S: ZipSource>(source: &S, local_offset: u64) -> Result<u32, Error> {
    let mut local_header = [0u8; LOCAL_FILE_HEADER_SIZE];
    read_exact_at(source, local_offset, &mut local_header)?;
    if u32_from(&local_header[0..4]) != LOCAL_FILE_HEADER_SIGNATURE {
        return Err(Error::InvalidArchive("invalid local header signature"));
    }

    let local_name_len = u16_from(&local_header[26..28]);
    let local_extra_len = u16_from(&local_header[28..30]);

    local_offset
        .checked_add(30)
        .and_then(|value| value.checked_add(u64::from(local_name_len)))
        .and_then(|value| value.checked_add(u64::from(local_extra_len)))
        .and_then(|value| u32::try_from(value).ok())
        .ok_or(Error::ArithmeticOverflow)
}

fn read_exact_at<S: ZipSource>(source: &S, offset: u64, buffer: &mut [u8]) -> Result<(), Error> {
    let mut consumed = 0usize;
    while consumed < buffer.len() {
        let n = source
            .read_at(offset + consumed as u64, &mut buffer[consumed..])
            .map_err(|_| Error::Source)?;
        if n == 0 {
            return Err(Error::ShortRead);
        }
        consumed = consumed.saturating_add(n);
    }
    Ok(())
}

#[inline]
fn u16_from(data: &[u8]) -> u16 {
    u16::from_le_bytes([data[0], data[1]])
}

#[inline]
fn u32_from(data: &[u8]) -> u32 {
    u32::from_le_bytes([data[0], data[1], data[2], data[3]])
}
