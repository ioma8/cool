use heapless::{String, Vec};
use xteink_browser::EntryKind;
use xteink_epub::EpubSource;

pub const MAX_ENTRIES: usize = 24;
const LABEL_CAPACITY: usize = 96;

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
    fn open_cache_file_append<'a>(&'a self, path: &str) -> Result<Self::File<'a>, FsError>;
    fn ensure_directory(&self, path: &str) -> Result<(), FsError>;
}

pub fn listed_entry_from_parts(
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
    Ok(ListedEntry {
        label: out,
        fs_name: fs_out,
        kind: if is_directory {
            EntryKind::Directory
        } else if is_epub_label(label) || is_epub_label(fs_name) {
            EntryKind::Epub
        } else {
            EntryKind::Other
        },
    })
}

pub fn is_epub_label(label: &str) -> bool {
    label
        .rsplit_once('.')
        .map(|(_, extension)| extension.eq_ignore_ascii_case("epub"))
        .unwrap_or(false)
}
