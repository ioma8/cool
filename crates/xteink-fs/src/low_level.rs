use core::{cell::RefCell, convert::Infallible, fmt::Write};

use embassy_embedded_hal::SetConfig;
use embassy_sync::blocking_mutex::{Mutex, raw::NoopRawMutex};
use embedded_hal::delay::DelayNs;
use embedded_hal::spi::{ErrorType as SpiErrorType, SpiBus};
use embedded_sdmmc::{
    DirEntry, File, LfnBuffer, Mode, RawDirectory, RawVolume, ShortFileName, TimeSource, Timestamp,
    VolumeIdx, VolumeManager,
};
use heapless::{String, Vec};
use xteink_browser::EntryKind;
use xteink_epub::{EpubError, EpubSource};
use xteink_sdspi::{SdSpiCard, SdSpiOptions, SpiTransport};

use crate::{hal, path::normalize_path};

pub const MAX_ENTRIES: usize = 64;
const LABEL_CAPACITY: usize = 96;
const LFN_CAPACITY: usize = 256;
const MAX_DIRS: usize = 16;
const MAX_FILES: usize = 4;
const MAX_VOLUMES: usize = 1;

#[derive(Debug, Clone)]
pub struct ListedEntry {
    pub label: String<LABEL_CAPACITY>,
    pub fs_name: String<LABEL_CAPACITY>,
    pub kind: EntryKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsError {
    CardInitFailed(String<64>),
    MountFailed(String<64>),
    TooManyEntries,
    InvalidUtf8,
    OpenFailed(String<64>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectoryPageInfo {
    pub page_start: usize,
    pub has_prev: bool,
    pub has_next: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SdTransportError<E> {
    Spi(E),
    SetConfig,
}

pub struct NoopTimeSource;

impl TimeSource for NoopTimeSource {
    fn get_timestamp(&self) -> Timestamp {
        Timestamp {
            year_since_1970: 0,
            zero_indexed_month: 0,
            zero_indexed_day: 0,
            hours: 0,
            minutes: 0,
            seconds: 0,
        }
    }
}

pub struct SharedSpiTransport<'a, SPI>
where
    SPI: SpiBus + SpiErrorType + SetConfig<Config = esp_hal::spi::master::Config>,
{
    bus: &'a Mutex<NoopRawMutex, RefCell<SPI>>,
    base_config: esp_hal::spi::master::Config,
}

impl<'a, SPI> SharedSpiTransport<'a, SPI>
where
    SPI: SpiBus + SpiErrorType + SetConfig<Config = esp_hal::spi::master::Config>,
{
    pub fn new(
        bus: &'a Mutex<NoopRawMutex, RefCell<SPI>>,
        base_config: esp_hal::spi::master::Config,
    ) -> Self {
        Self { bus, base_config }
    }
}

impl<'a, SPI> SpiTransport for SharedSpiTransport<'a, SPI>
where
    SPI: SpiBus + SpiErrorType + SetConfig<Config = esp_hal::spi::master::Config>,
{
    type Error = SdTransportError<SPI::Error>;

    fn set_clock_hz(&mut self, hz: u32) -> Result<(), Self::Error> {
        let config = self
            .base_config
            .with_frequency(esp_hal::time::Rate::from_hz(hz));
        self.bus.lock(|cell| {
            let mut bus = cell.borrow_mut();
            bus.set_config(&config)
                .map_err(|_| SdTransportError::SetConfig)
        })
    }

    fn transfer_byte(&mut self, byte: u8) -> Result<u8, Self::Error> {
        self.bus.lock(|cell| {
            let mut bus = cell.borrow_mut();
            let mut word = [byte];
            SpiBus::transfer_in_place(&mut *bus, &mut word).map_err(SdTransportError::Spi)?;
            Ok(word[0])
        })
    }
}

type SdCard<'bus, SPI, DELAY> = SdSpiCard<
    SharedSpiTransport<'bus, SPI>,
    hal::RawGpioOutput,
    hal::RawGpioOutput,
    DELAY,
    SdTransportError<<SPI as SpiErrorType>::Error>,
    Infallible,
>;

type SdVolumes<'bus, SPI, DELAY> =
    VolumeManager<SdCard<'bus, SPI, DELAY>, NoopTimeSource, MAX_DIRS, MAX_FILES, MAX_VOLUMES>;

type SdRawFile<'fs, 'bus, SPI, DELAY> =
    File<'fs, SdCard<'bus, SPI, DELAY>, NoopTimeSource, MAX_DIRS, MAX_FILES, MAX_VOLUMES>;

pub struct SdApp<'bus, SPI, DELAY>
where
    SPI: SpiBus + SpiErrorType + SetConfig<Config = esp_hal::spi::master::Config>,
    DELAY: DelayNs,
{
    volume_mgr: SdVolumes<'bus, SPI, DELAY>,
    raw_volume: RawVolume,
}

pub trait SdFsFile {
    fn len(&self) -> usize;
    fn seek_from_start(&mut self, offset: u32) -> Result<(), FsError>;
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, FsError>;
    fn write(&mut self, buffer: &[u8]) -> Result<usize, FsError>;
    fn flush(&mut self) -> Result<(), FsError>;
}

pub trait SdFilesystem {
    type EpubSource<'a>: EpubSource + 'a
    where
        Self: 'a;
    type File<'a>: SdFsFile + 'a
    where
        Self: 'a;

    fn list_directory_page(
        &self,
        path: &str,
        page_start: usize,
        page_size: usize,
        entries: &mut Vec<ListedEntry, MAX_ENTRIES>,
    ) -> Result<DirectoryPageInfo, FsError>;

    fn open_epub_source<'a>(&'a self, path: &str) -> Result<Self::EpubSource<'a>, FsError>;
    fn open_cache_file_read<'a>(&'a self, path: &str) -> Result<Self::File<'a>, FsError>;
    fn open_cache_file_write<'a>(&'a self, path: &str) -> Result<Self::File<'a>, FsError>;
    fn ensure_directory(&self, path: &str) -> Result<(), FsError>;
}

impl<'bus, SPI, DELAY> SdApp<'bus, SPI, DELAY>
where
    SPI: SpiBus + SpiErrorType + SetConfig<Config = esp_hal::spi::master::Config>,
    DELAY: DelayNs + 'bus,
{
    pub fn begin(
        spi_bus: &'bus Mutex<NoopRawMutex, RefCell<SPI>>,
        base_config: esp_hal::spi::master::Config,
        delay: DELAY,
    ) -> Result<Self, FsError> {
        let transport = SharedSpiTransport::new(spi_bus, base_config);
        hal::power_on_sd_card();
        let cs = hal::sd_cs_output();
        let power = hal::RawGpioOutput::new(hal::SD_POWER_PIN, true);
        let card = SdSpiCard::new(transport, cs, power, delay, SdSpiOptions::default());
        let card_type = match card.begin() {
            Ok(card_type) => card_type,
            Err(err) => return Err(FsError::CardInitFailed(error_string(&err))),
        };
        esp_println::println!("SD card type: {:?}", card_type);
        let volume_mgr = VolumeManager::new_with_limits(card, NoopTimeSource, 5000);
        let raw_volume = volume_mgr
            .open_volume(VolumeIdx(0))
            .map_err(|err| FsError::MountFailed(error_string(&err)))?
            .to_raw_volume();
        Ok(Self {
            volume_mgr,
            raw_volume,
        })
    }

    fn open_directory_at_path<'fs>(&'fs self, path: &str) -> Result<RawDirectory, FsError> {
        let path =
            normalize_path(path).map_err(|_| FsError::OpenFailed(error_message("invalid path")))?;
        esp_println::println!("SD open_directory_at_path start: {}", path.as_str());
        let mut raw_directory = self
            .volume_mgr
            .open_root_dir(self.raw_volume)
            .map_err(|err| {
                esp_println::println!(
                    "SD open_directory_at_path root failed: {} -> {:?}",
                    path,
                    err
                );
                FsError::OpenFailed(error_string(&err))
            })?;
        for component in path_components(path.as_str()) {
            esp_println::println!("SD open_directory_at_path component: {}", component);
            raw_directory = self
                .volume_mgr
                .open_dir(raw_directory, component)
                .map_err(|err| {
                    esp_println::println!(
                        "SD open_directory_at_path component failed: {} / {} -> {:?}",
                        path,
                        component,
                        err
                    );
                    FsError::OpenFailed(error_string(&err))
                })?;
        }
        esp_println::println!("SD open_directory_at_path complete: {}", path.as_str());
        Ok(raw_directory)
    }

    fn open_file_at_path_with_mode<'fs>(
        &'fs self,
        path: &str,
        mode: Mode,
    ) -> Result<SdRawFile<'fs, 'bus, SPI, DELAY>, FsError> {
        let path =
            normalize_path(path).map_err(|_| FsError::OpenFailed(error_message("invalid path")))?;
        esp_println::println!("SD open_file_at_path start: {}", path.as_str());
        let (parent_path, file_name) = split_parent_path(path.as_str())?;
        esp_println::println!(
            "SD open_file_at_path split: {} parent={} file={}",
            path.as_str(),
            parent_path,
            file_name
        );
        let raw_directory = self.open_directory_at_path(parent_path)?;
        let file = self
            .volume_mgr
            .open_file_in_dir(raw_directory, file_name, mode)
            .map_err(|err| {
                esp_println::println!(
                    "SD open_file_at_path open_file_in_dir failed: {} -> {:?}",
                    path.as_str(),
                    err
                );
                FsError::OpenFailed(error_string(&err))
            })?;
        esp_println::println!("SD open_file_at_path complete: {}", path.as_str());
        Ok(file.to_file(&self.volume_mgr))
    }

    fn open_file_at_path<'fs>(
        &'fs self,
        path: &str,
    ) -> Result<SdRawFile<'fs, 'bus, SPI, DELAY>, FsError> {
        self.open_file_at_path_with_mode(path, Mode::ReadOnly)
    }

    pub fn ensure_directory_internal(&self, path: &str) -> Result<(), FsError> {
        let path =
            normalize_path(path).map_err(|_| FsError::OpenFailed(error_message("invalid path")))?;
        if path.as_str() == "/" {
            return Ok(());
        }
        let mut current = self
            .volume_mgr
            .open_root_dir(self.raw_volume)
            .map_err(|_| FsError::OpenFailed(error_message("open root")))?;
        for component in path_components(path.as_str()) {
            match self.volume_mgr.open_dir(current, component) {
                Ok(next) => {
                    current = next;
                }
                Err(open_err) => {
                    if !matches!(open_err, embedded_sdmmc::Error::NotFound) {
                        return Err(FsError::OpenFailed(error_string(&open_err)));
                    }
                    self.volume_mgr
                        .make_dir_in_dir(current, component)
                        .map_err(|err| FsError::OpenFailed(error_string(&err)))?;
                    current = self
                        .volume_mgr
                        .open_dir(current, component)
                        .map_err(|err| FsError::OpenFailed(error_string(&err)))?;
                }
            }
        }
        Ok(())
    }
}

impl<'bus, SPI, DELAY> SdFilesystem for SdApp<'bus, SPI, DELAY>
where
    SPI: SpiBus + SpiErrorType + SetConfig<Config = esp_hal::spi::master::Config>,
    DELAY: DelayNs + 'bus,
{
    type EpubSource<'a>
        = SdEpubSource<'a, 'bus, SPI, DELAY>
    where
        Self: 'a;
    type File<'a>
        = SdRawFile<'a, 'bus, SPI, DELAY>
    where
        Self: 'a;

    fn list_directory_page(
        &self,
        path: &str,
        page_start: usize,
        page_size: usize,
        entries: &mut Vec<ListedEntry, MAX_ENTRIES>,
    ) -> Result<DirectoryPageInfo, FsError> {
        entries.clear();
        esp_println::println!(
            "SD list_directory_page start: {} page_start={} page_size={}",
            path,
            page_start,
            page_size
        );
        let raw_directory = self.open_directory_at_path(path)?;
        esp_println::println!("SD directory opened: {}", path);
        let directory = raw_directory.to_directory(&self.volume_mgr);
        let mut lfn_storage = [0u8; LFN_CAPACITY];
        let mut lfn_buffer = LfnBuffer::new(&mut lfn_storage);
        let mut skipped = 0usize;
        let mut collected = 0usize;
        let page_size = page_size.min(MAX_ENTRIES).max(1);
        let mut has_next = false;

        esp_println::println!("SD iterating directory: {}", path);
        directory
            .iterate_dir_lfn(&mut lfn_buffer, |entry, long_name| {
                if entry.attributes.is_lfn() {
                    return;
                }
                if skipped < page_start {
                    skipped += 1;
                    return;
                }
                if collected >= page_size {
                    has_next = true;
                    return;
                }
                match listed_entry_from_dir_entry(entry, long_name) {
                    Ok(listed) => {
                        let _ = push_listed_entry(entries, listed);
                        collected += 1;
                    }
                    Err(_) => {}
                }
            })
            .map_err(|err| FsError::OpenFailed(error_string(&err)))?;
        esp_println::println!(
            "SD directory iteration complete: {}{}",
            path,
            if has_next { " (more entries)" } else { "" }
        );

        Ok(DirectoryPageInfo {
            page_start,
            has_prev: page_start > 0,
            has_next,
        })
    }

    fn open_epub_source<'b>(&'b self, path: &str) -> Result<Self::EpubSource<'b>, FsError> {
        let file = self.open_file_at_path(path)?;
        Ok(SdEpubSource { file })
    }

    fn open_cache_file_read<'b>(&'b self, path: &str) -> Result<Self::File<'b>, FsError> {
        self.open_file_at_path(path)
    }

    fn open_cache_file_write<'b>(&'b self, path: &str) -> Result<Self::File<'b>, FsError> {
        self.open_file_at_path_with_mode(path, Mode::ReadWriteCreateOrTruncate)
    }

    fn ensure_directory(&self, path: &str) -> Result<(), FsError> {
        self.ensure_directory_internal(path)
    }
}

impl<'fs, 'bus, SPI, DELAY> SdFsFile for SdRawFile<'fs, 'bus, SPI, DELAY>
where
    SPI: SpiBus + SpiErrorType + SetConfig<Config = esp_hal::spi::master::Config>,
    DELAY: DelayNs + 'bus,
{
    fn len(&self) -> usize {
        self.length() as usize
    }

    fn seek_from_start(&mut self, offset: u32) -> Result<(), FsError> {
        File::seek_from_start(self, offset).map_err(|err| FsError::OpenFailed(error_string(&err)))
    }

    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, FsError> {
        File::read(self, buffer).map_err(|err| FsError::OpenFailed(error_string(&err)))
    }

    fn write(&mut self, buffer: &[u8]) -> Result<usize, FsError> {
        File::write(self, buffer).map_err(|err| FsError::OpenFailed(error_string(&err)))?;
        Ok(buffer.len())
    }

    fn flush(&mut self) -> Result<(), FsError> {
        File::flush(self).map_err(|err| FsError::OpenFailed(error_string(&err)))
    }
}

pub struct SdEpubSource<'fs, 'bus, SPI, DELAY>
where
    SPI: SpiBus + SpiErrorType + SetConfig<Config = esp_hal::spi::master::Config>,
    DELAY: DelayNs,
{
    file: SdRawFile<'fs, 'bus, SPI, DELAY>,
}

impl<'fs, 'bus, SPI, DELAY> EpubSource for SdEpubSource<'fs, 'bus, SPI, DELAY>
where
    SPI: SpiBus + SetConfig<Config = esp_hal::spi::master::Config>,
    DELAY: DelayNs + 'bus,
{
    fn len(&self) -> usize {
        self.file.length() as usize
    }

    fn read_at(&self, offset: u64, buffer: &mut [u8]) -> Result<usize, EpubError> {
        let offset = u32::try_from(offset).map_err(|_| EpubError::InvalidFormat)?;
        self.file
            .seek_from_start(offset)
            .map_err(|_| EpubError::Io)?;
        self.file.read(buffer).map_err(|_| EpubError::Io)
    }
}

pub fn init_sd<'a, SPI, DELAY>(
    spi_bus: &'a Mutex<NoopRawMutex, RefCell<SPI>>,
    base_config: esp_hal::spi::master::Config,
    delay: DELAY,
) -> Result<SdApp<'a, SPI, DELAY>, FsError>
where
    SPI: SpiBus + SetConfig<Config = esp_hal::spi::master::Config>,
    DELAY: DelayNs + 'a,
{
    SdApp::begin(spi_bus, base_config, delay)
}

pub fn is_epub_label(label: &str) -> bool {
    label
        .rsplit_once('.')
        .map(|(_, extension)| extension.eq_ignore_ascii_case("epub"))
        .unwrap_or(false)
}

pub(crate) fn listed_entry_from_dir_entry(
    entry: &DirEntry,
    long_name: Option<&str>,
) -> Result<ListedEntry, FsError> {
    let fs_name = short_file_name_to_string(&entry.name)?;
    let label = match long_name {
        Some(name) => heapless_string(name)?,
        None => fs_name.clone(),
    };
    listed_entry_from_parts(
        label.as_str(),
        fs_name.as_str(),
        entry.attributes.is_directory(),
    )
}

pub(crate) fn listed_entry_from_parts(
    label: &str,
    fs_name: &str,
    is_directory: bool,
) -> Result<ListedEntry, FsError> {
    let mut out = String::<LABEL_CAPACITY>::new();
    out.push_str(label).map_err(|_| FsError::TooManyEntries)?;
    let mut fs_out = String::<LABEL_CAPACITY>::new();
    fs_out
        .push_str(fs_name)
        .map_err(|_| FsError::TooManyEntries)?;
    let kind = if is_directory {
        EntryKind::Directory
    } else if is_epub_label(label) {
        EntryKind::Epub
    } else {
        EntryKind::Other
    };
    Ok(ListedEntry {
        label: out,
        fs_name: fs_out,
        kind,
    })
}

fn path_components(path: &str) -> impl Iterator<Item = &str> {
    path.split('/').filter(|component| !component.is_empty())
}

fn split_parent_path(path: &str) -> Result<(&str, &str), FsError> {
    let trimmed = path.trim_end_matches('/');
    let Some((parent, name)) = trimmed.rsplit_once('/') else {
        return Err(FsError::OpenFailed(error_message("missing path component")));
    };

    if name.is_empty() {
        return Err(FsError::OpenFailed(error_message("empty file name")));
    }

    let parent = if parent.is_empty() { "/" } else { parent };
    Ok((parent, name))
}

fn short_file_name_to_string(name: &ShortFileName) -> Result<String<LABEL_CAPACITY>, FsError> {
    let base = core::str::from_utf8(name.base_name()).map_err(|_| FsError::InvalidUtf8)?;
    let ext = core::str::from_utf8(name.extension()).map_err(|_| FsError::InvalidUtf8)?;

    let mut out = String::<LABEL_CAPACITY>::new();
    out.push_str(base).map_err(|_| FsError::TooManyEntries)?;
    if !ext.is_empty() {
        out.push('.').map_err(|_| FsError::TooManyEntries)?;
        out.push_str(ext).map_err(|_| FsError::TooManyEntries)?;
    }
    Ok(out)
}

fn heapless_string(label: &str) -> Result<String<LABEL_CAPACITY>, FsError> {
    let mut out = String::<LABEL_CAPACITY>::new();
    out.push_str(label).map_err(|_| FsError::TooManyEntries)?;
    Ok(out)
}

fn push_listed_entry(
    entries: &mut Vec<ListedEntry, MAX_ENTRIES>,
    entry: ListedEntry,
) -> Option<()> {
    entries.push(entry).ok()
}

fn error_string<E: core::fmt::Debug>(err: &E) -> String<64> {
    let mut out = String::<64>::new();
    let _ = write!(&mut out, "{err:?}");
    out
}

fn error_message(msg: &str) -> String<64> {
    let mut out = String::<64>::new();
    let _ = out.push_str(msg);
    out
}

#[cfg(test)]
mod tests {
    use super::{
        error_string, is_epub_label, listed_entry_from_parts, path_components, push_listed_entry,
    };

    #[derive(Debug)]
    struct TestError;

    #[test]
    fn classifies_directories_from_name_and_flag() {
        let listed = listed_entry_from_parts("BOOKS", "BOOKS", true).unwrap();

        assert_eq!(listed.kind, xteink_browser::EntryKind::Directory);
        assert_eq!(listed.label.as_str(), "BOOKS");
        assert_eq!(listed.fs_name.as_str(), "BOOKS");
    }

    #[test]
    fn classifies_epubs_by_extension() {
        let listed = listed_entry_from_parts("story.epub", "STORY.EPUB", false).unwrap();

        assert_eq!(listed.kind, xteink_browser::EntryKind::Epub);
        assert_eq!(listed.label.as_str(), "story.epub");
        assert_eq!(listed.fs_name.as_str(), "STORY.EPUB");
    }

    #[test]
    fn recognizes_epub_labels_case_insensitively() {
        assert!(is_epub_label("BOOK.EPUB"));
        assert!(is_epub_label("book.EpUb"));
        assert!(!is_epub_label("book.txt"));
    }

    #[test]
    fn formats_debug_errors() {
        let rendered = error_string(&TestError);
        assert_eq!(rendered.as_str(), "TestError");
    }

    #[test]
    fn push_listed_entry_truncates_when_full() {
        let mut entries: Vec<ListedEntry, 1> = Vec::new();
        let first = listed_entry_from_parts("one", "ONE", false).unwrap();
        let second = listed_entry_from_parts("two", "TWO", false).unwrap();

        assert!(entries.push(first).is_ok());
        assert!(push_listed_entry(&mut entries, second).is_none());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].label.as_str(), "one");
        assert_eq!(entries[0].fs_name.as_str(), "ONE");
    }

    #[test]
    fn path_components_ignore_current_directory_segments() {
        let components: heapless::Vec<&str, 4> =
            path_components("/MYBOOKS/./WHEN_I~1.EPU").collect();

        assert_eq!(components.as_slice(), &["MYBOOKS", "WHEN_I~1.EPU"]);
    }

    #[test]
    fn normalizes_current_directory_segments_before_opening() {
        use crate::path::normalize_path;
        let normalized = normalize_path("/MYBOOKS/./WHEN_I~1.EPU").unwrap();

        assert_eq!(normalized.as_str(), "/MYBOOKS/WHEN_I~1.EPU");
    }
}
