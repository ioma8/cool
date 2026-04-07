use std::{
    fs,
    path::{Path, PathBuf},
};

use xteink_app::{AppStorage, DirectoryPage, DirectoryPageInfo, ListedEntry};
use xteink_browser::EntryKind;
use xteink_render::Framebuffer;

pub struct HostStorage {
    root: PathBuf,
}

impl HostStorage {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
        }
    }

    fn resolve(&self, path: &str) -> PathBuf {
        let trimmed = path.trim_start_matches('/');
        if trimmed.is_empty() {
            self.root.clone()
        } else {
            self.root.join(trimmed)
        }
    }

    fn entry_kind(path: &Path) -> EntryKind {
        if path.is_dir() {
            EntryKind::Directory
        } else if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("epub"))
        {
            EntryKind::Epub
        } else {
            EntryKind::Other
        }
    }

    fn listed_entry(path: &Path) -> ListedEntry {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        let mut label = heapless::String::new();
        let mut fs_name = heapless::String::new();
        let _ = label.push_str(name);
        let _ = fs_name.push_str(name);
        ListedEntry {
            label,
            fs_name,
            kind: Self::entry_kind(path),
        }
    }
}

#[derive(Debug)]
pub enum StorageError {
    Io(std::io::Error),
    Render(xteink_epub::EpubError),
}

impl core::fmt::Display for StorageError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "io error: {error}"),
            Self::Render(error) => write!(f, "render error: {error:?}"),
        }
    }
}

impl std::error::Error for StorageError {}

impl From<std::io::Error> for StorageError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

struct FileSource {
    bytes: Vec<u8>,
}

impl xteink_epub::EpubSource for FileSource {
    fn len(&self) -> usize {
        self.bytes.len()
    }

    fn read_at(&self, offset: u64, buffer: &mut [u8]) -> Result<usize, xteink_epub::EpubError> {
        let start = offset as usize;
        if start >= self.bytes.len() {
            return Ok(0);
        }
        let end = (start + buffer.len()).min(self.bytes.len());
        let chunk = &self.bytes[start..end];
        buffer[..chunk.len()].copy_from_slice(chunk);
        Ok(chunk.len())
    }
}

impl AppStorage for HostStorage {
    type Error = StorageError;

    fn list_directory_page(
        &self,
        path: &str,
        page_start: usize,
        page_size: usize,
    ) -> Result<DirectoryPage, Self::Error> {
        let resolved = self.resolve(path);
        let mut entries: Vec<_> = fs::read_dir(&resolved)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect();
        entries.sort_by(|left, right| {
            let left_kind = Self::entry_kind(left);
            let right_kind = Self::entry_kind(right);
            let left_rank = if left_kind == EntryKind::Directory {
                0
            } else {
                1
            };
            let right_rank = if right_kind == EntryKind::Directory {
                0
            } else {
                1
            };
            left_rank
                .cmp(&right_rank)
                .then_with(|| left.file_name().cmp(&right.file_name()))
        });

        let total = entries.len();
        let start = page_start.min(total);
        let end = (start + page_size).min(total);
        let mut page_entries = heapless::Vec::new();
        for path in entries[start..end].iter() {
            let _ = page_entries.push(Self::listed_entry(path));
        }

        Ok(DirectoryPage {
            entries: page_entries,
            info: DirectoryPageInfo {
                page_start: start,
                has_prev: start > 0,
                has_next: end < total,
            },
        })
    }

    fn render_epub_from_entry(
        &self,
        framebuffer: &mut Framebuffer,
        current_path: &str,
        entry: &ListedEntry,
    ) -> Result<usize, Self::Error> {
        self.render_epub_page_from_entry(framebuffer, current_path, entry, 0)
    }

    fn render_epub_page_from_entry(
        &self,
        framebuffer: &mut Framebuffer,
        current_path: &str,
        entry: &ListedEntry,
        target_page: usize,
    ) -> Result<usize, Self::Error> {
        let mut full_path = self.resolve(current_path);
        full_path.push(entry.fs_name.as_str());
        let bytes = fs::read(full_path)?;
        framebuffer
            .render_epub_page(FileSource { bytes }, target_page)
            .map_err(StorageError::Render)
    }
}
