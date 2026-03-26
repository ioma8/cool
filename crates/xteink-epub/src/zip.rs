const EOCD_SIGNATURE: u32 = 0x0605_4B50;
const CENTRAL_DIRECTORY_SIGNATURE: u32 = 0x0201_4B50;
const LOCAL_FILE_HEADER_SIGNATURE: u32 = 0x0403_4B50;

const EOCD_FIXED_SIZE: usize = 22;
const CENTRAL_DIR_FILE_HEADER_SIZE: usize = 46;
const LOCAL_FILE_HEADER_SIZE: usize = 30;
const MAX_EOCD_LOOKBACK: usize = EOCD_FIXED_SIZE + 0xFFFF;

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
    pub name_offset: u16,
    pub name_len: u16,
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
    entries: [Option<EpubEntryMetadata>; MAX_ENTRIES],
    names: [u8; NAME_CAPACITY],
    names_len: usize,
}

impl<const MAX_ENTRIES: usize, const NAME_CAPACITY: usize> EpubArchive<
    MAX_ENTRIES,
    NAME_CAPACITY,
> {
    pub const fn new() -> Self {
        Self {
            eocd: EndOfCentralDirectory::empty(),
            entry_count: 0,
            entries: [None; MAX_ENTRIES],
            names: [0u8; NAME_CAPACITY],
            names_len: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.entry_count
    }

    pub const fn eocd(&self) -> &EndOfCentralDirectory {
        &self.eocd
    }

    pub fn clear(&mut self) {
        self.entry_count = 0;
        self.names_len = 0;
        self.entries.iter_mut().for_each(|entry| *entry = None);
    }

    pub fn parse<S: ZipSource>(&mut self, source: &S) -> Result<(), Error> {
        self.clear();

        let file_size = source.len() as u64;
        let eocd = find_eocd(source, file_size)?;

        if eocd.disk_number != 0 || eocd.cd_entries_on_disk != eocd.cd_entries_total {
            return Err(Error::InvalidArchive("multi-disk archives are not supported"));
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
            return Err(Error::InvalidArchive("central directory exceeds source length"));
        }

        let mut cursor = cd_start;
        for index in 0..entry_count {
            let entry = parse_central_directory_entry(source, cursor)?;

            let name_len = usize::from(entry.name_len);
            let name_start = self.names_len;
            if name_len > NAME_CAPACITY.saturating_sub(name_start) {
                return Err(Error::NameBufferTooSmall);
            }

            let name_end = name_start
                .checked_add(name_len)
                .ok_or(Error::ArithmeticOverflow)?;

            read_exact_at(source, cursor + CENTRAL_DIR_FILE_HEADER_SIZE as u64, &mut self.names[name_start..name_end])?;
            self.entries[index] = Some(EpubEntryMetadata {
                name_offset: u16::try_from(name_start).map_err(|_| Error::ArithmeticOverflow)?,
                name_len: u16::try_from(name_len).map_err(|_| Error::ArithmeticOverflow)?,
                compression: entry.compression,
                crc32: entry.crc32,
                compressed_size: entry.compressed_size,
                uncompressed_size: entry.uncompressed_size,
                local_header_offset: entry.local_header_offset,
                data_offset: entry.data_offset,
            });

            self.names_len = name_end;
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

    pub fn entry_by_index(&self, index: usize) -> Option<&EpubEntryMetadata> {
        self.entries.get(index).and_then(Option::as_ref)
    }

    pub fn entry_by_name(&self, name: &str) -> Option<&EpubEntryMetadata> {
        let needle = name.as_bytes();
        self.entry_by_name_bytes(needle)
    }

    pub fn entry_by_name_bytes(&self, needle: &[u8]) -> Option<&EpubEntryMetadata> {
        self.entries[..self.entry_count]
            .iter()
            .flatten()
            .find(|entry| self.entry_name_bytes(entry) == needle)
    }

    pub fn entry_name_bytes(&self, entry: &EpubEntryMetadata) -> &[u8] {
        &self.names[usize::from(entry.name_offset)..usize::from(entry.name_offset + entry.name_len)]
    }

    pub fn entry_name_utf8(&self, entry: &EpubEntryMetadata) -> Option<&str> {
        core::str::from_utf8(self.entry_name_bytes(entry)).ok()
    }
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
    data_offset: u32,
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
    let mut scan_window = [0u8; MAX_EOCD_LOOKBACK];
    let search = &mut scan_window[..search_size];
    read_exact_at(source, scan_start, search)?;

    if search.len() < EOCD_FIXED_SIZE {
        return Err(Error::InvalidArchive("source shorter than EOCD minimum"));
    }

    for pos in (0..=search.len() - EOCD_FIXED_SIZE).rev() {
        if u32_from(&search[pos..pos + 4]) != EOCD_SIGNATURE {
            continue;
        }

        let comment_len = u16_from(&search[pos + 20..pos + 22]);
        let absolute = scan_start + pos as u64;
        let record_length = EOCD_FIXED_SIZE + usize::from(comment_len);
        if absolute.checked_add(record_length as u64) != Some(file_size) {
            continue;
        }

        return Ok(EndOfCentralDirectory {
            disk_number: u16_from(&search[pos + 4..pos + 6]),
            cd_entries_on_disk: u16_from(&search[pos + 8..pos + 10]),
            cd_entries_total: u16_from(&search[pos + 10..pos + 12]),
            cd_size: u32_from(&search[pos + 12..pos + 16]),
            cd_offset: u32_from(&search[pos + 16..pos + 20]),
            comment_length: comment_len,
        });
    }

    Err(Error::EocdNotFound)
}

fn parse_central_directory_entry<S: ZipSource>(
    source: &S,
    cursor: u64,
) -> Result<ParsedCentralDirectoryEntry, Error> {
    let mut header = [0u8; CENTRAL_DIR_FILE_HEADER_SIZE];
    read_exact_at(source, cursor, &mut header)?;
    if u32_from(&header[0..4]) != CENTRAL_DIRECTORY_SIGNATURE {
        return Err(Error::InvalidArchive("invalid central directory signature"));
    }

    let name_len = u16_from(&header[28..30]);
    let extra_length = u16_from(&header[30..32]);
    let comment_length = u16_from(&header[32..34]);
    let local_header_offset = u32_from(&header[42..46]);
    let data_offset = compute_local_data_offset(source, u64::from(local_header_offset))?;

    Ok(ParsedCentralDirectoryEntry {
        compression: CompressionMethod::from_u16(u16_from(&header[10..12])),
        crc32: u32_from(&header[16..20]),
        compressed_size: u32_from(&header[20..24]),
        uncompressed_size: u32_from(&header[24..28]),
        name_len,
        extra_length,
        comment_length,
        local_header_offset,
        data_offset,
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
