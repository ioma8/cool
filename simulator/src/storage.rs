use std::{
    cell::RefCell,
    fs,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use xteink_app::{
    AppStorage, DirectoryPage, DirectoryPageInfo as AppDirectoryPageInfo, EpubRenderResult,
    ListedEntry,
};
use xteink_fs::{
    DirectoryPage as FsDirectoryPage, DirectoryPageInfo as FsDirectoryPageInfo, FsError,
    ListedEntry as FsListedEntry, SdFilesystem, SdFsFile, listed_entry_from_parts,
    load_directory_page, render_epub_from_entry, render_epub_page_from_entry,
};
use xteink_render::Framebuffer;

pub struct HostStorage {
    root: PathBuf,
}

impl HostStorage {
    const MAX_ENTRY_TEXT: usize = 96;

    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
        }
    }

    fn host_error(message: &str) -> FsError {
        let mut error = heapless::String::<64>::new();
        let _ = error.push_str(message);
        FsError::OpenFailed(error)
    }

    fn host_path_for(&self, path: &str) -> Result<PathBuf, FsError> {
        let normalized =
            xteink_fs::normalize_path(path).map_err(|_| Self::host_error("invalid path"))?;
        let mut resolved = self.root.clone();
        for component in normalized
            .as_str()
            .split('/')
            .filter(|component| !component.is_empty())
        {
            resolved = Self::resolve_component(&resolved, component)?;
        }
        Ok(resolved)
    }

    fn host_parent_and_leaf_for(&self, path: &str) -> Result<(PathBuf, String), FsError> {
        let normalized =
            xteink_fs::normalize_path(path).map_err(|_| Self::host_error("invalid path"))?;
        let trimmed = normalized.as_str().trim_end_matches('/');
        let Some((parent, leaf)) = trimmed.rsplit_once('/') else {
            return Err(Self::host_error("missing path component"));
        };
        if leaf.is_empty() {
            return Err(Self::host_error("empty file name"));
        }
        let parent = if parent.is_empty() { "/" } else { parent };
        Ok((self.host_path_for(parent)?, leaf.to_string()))
    }

    fn host_directory_path_for(&self, path: &str) -> Result<PathBuf, FsError> {
        let normalized =
            xteink_fs::normalize_path(path).map_err(|_| Self::host_error("invalid path"))?;
        let mut resolved = self.root.clone();
        for component in normalized
            .as_str()
            .split('/')
            .filter(|component| !component.is_empty())
        {
            let direct = resolved.join(component);
            if direct.exists() {
                resolved = direct;
                continue;
            }
            match Self::resolve_component(&resolved, component) {
                Ok(existing) => resolved = existing,
                Err(_) => {
                    resolved.push(component);
                }
            }
        }
        Ok(resolved)
    }

    fn app_entry(entry: &FsListedEntry) -> ListedEntry {
        let mut label = heapless::String::new();
        let mut fs_name = heapless::String::new();
        let _ = label.push_str(entry.label.as_str());
        let _ = fs_name.push_str(entry.fs_name.as_str());
        ListedEntry {
            label,
            fs_name,
            kind: entry.kind,
        }
    }

    fn file_name_str(path: &Path) -> Result<&str, FsError> {
        path.file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| Self::host_error("invalid filename"))
    }

    fn short_component_name(name: &str, is_directory: bool) -> String {
        let hash = stable_name_hash(name) & 0x0FFF_FFFF;

        let extension = if is_directory {
            None
        } else {
            name.rsplit_once('.').and_then(|(_, ext)| {
                let mut short = String::new();
                for ch in ext.chars() {
                    if ch.is_ascii_alphanumeric() {
                        short.push(ch.to_ascii_uppercase());
                        if short.len() == 8 {
                            break;
                        }
                    }
                }
                if short.is_empty() { None } else { Some(short) }
            })
        };

        match extension {
            Some(extension) => format!("F{hash:07X}.{extension}"),
            None if is_directory => format!("D{hash:07X}"),
            None => format!("F{hash:07X}"),
        }
    }

    fn resolve_component(parent: &Path, component: &str) -> Result<PathBuf, FsError> {
        let direct = parent.join(component);
        if direct.exists() {
            return Ok(direct);
        }

        let entries = fs::read_dir(parent).map_err(|_| Self::host_error("read dir"))?;
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            let name = Self::file_name_str(&path)?;
            if Self::short_component_name(name, path.is_dir()) == component {
                return Ok(path);
            }
        }

        Err(Self::host_error("component not found"))
    }

    fn display_label(name: &str) -> String {
        let mut label = String::new();
        for ch in name.chars() {
            if label.len() + ch.len_utf8() > Self::MAX_ENTRY_TEXT {
                break;
            }
            label.push(ch);
        }
        label
    }
}

fn stable_name_hash(name: &str) -> u32 {
    let mut hash = 0x811C9DC5u32;
    for byte in name.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x01000193);
    }
    hash
}

pub struct HostFile {
    file: fs::File,
    len: usize,
}

impl HostFile {
    fn open(path: &Path) -> Result<Self, FsError> {
        let file = fs::File::open(path).map_err(|_| HostStorage::host_error("open read"))?;
        let len = usize::try_from(
            file.metadata()
                .map_err(|_| HostStorage::host_error("metadata"))?
                .len(),
        )
        .unwrap_or(usize::MAX);
        Ok(Self { file, len })
    }

    fn open_with_options(path: &Path, options: &fs::OpenOptions) -> Result<Self, FsError> {
        let file = options
            .open(path)
            .map_err(|_| HostStorage::host_error("open write"))?;
        let len = usize::try_from(
            file.metadata()
                .map_err(|_| HostStorage::host_error("metadata"))?
                .len(),
        )
        .unwrap_or(usize::MAX);
        Ok(Self { file, len })
    }
}

impl SdFsFile for HostFile {
    fn len(&self) -> usize {
        self.len
    }

    fn seek_from_start(&mut self, offset: u32) -> Result<(), FsError> {
        self.file
            .seek(SeekFrom::Start(u64::from(offset)))
            .map_err(|_| HostStorage::host_error("seek"))?;
        Ok(())
    }

    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, FsError> {
        self.file
            .read(buffer)
            .map_err(|_| HostStorage::host_error("read"))
    }

    fn write(&mut self, buffer: &[u8]) -> Result<usize, FsError> {
        let written = self
            .file
            .write(buffer)
            .map_err(|_| HostStorage::host_error("write"))?;
        self.len = self.len.max(
            usize::try_from(
                self.file
                    .stream_position()
                    .map_err(|_| HostStorage::host_error("position"))?,
            )
            .unwrap_or(self.len),
        );
        Ok(written)
    }

    fn flush(&mut self) -> Result<(), FsError> {
        self.file
            .flush()
            .map_err(|_| HostStorage::host_error("flush"))
    }
}

pub struct HostEpubSource {
    len: usize,
    file: RefCell<fs::File>,
}

impl HostEpubSource {
    fn open(path: &Path) -> Result<Self, FsError> {
        let file = fs::File::open(path).map_err(|_| HostStorage::host_error("open epub"))?;
        let len = usize::try_from(
            file.metadata()
                .map_err(|_| HostStorage::host_error("metadata"))?
                .len(),
        )
        .unwrap_or(usize::MAX);
        Ok(Self {
            len,
            file: RefCell::new(file),
        })
    }
}

impl xteink_epub::EpubSource for HostEpubSource {
    fn len(&self) -> usize {
        self.len
    }

    fn read_at(&self, offset: u64, buffer: &mut [u8]) -> Result<usize, xteink_epub::EpubError> {
        if offset >= self.len as u64 {
            return Ok(0);
        }
        let mut file = self.file.borrow_mut();
        file.seek(SeekFrom::Start(offset))
            .map_err(|_| xteink_epub::EpubError::Io)?;
        file.read(buffer).map_err(|_| xteink_epub::EpubError::Io)
    }
}

impl SdFilesystem for HostStorage {
    type EpubSource<'a>
        = HostEpubSource
    where
        Self: 'a;
    type File<'a>
        = HostFile
    where
        Self: 'a;

    fn list_directory_page(
        &self,
        path: &str,
        page_start: usize,
        page_size: usize,
        entries: &mut heapless::Vec<FsListedEntry, { xteink_fs::MAX_ENTRIES }>,
    ) -> Result<FsDirectoryPageInfo, FsError> {
        entries.clear();
        let resolved = self.host_path_for(path)?;
        let mut paths: Vec<_> = fs::read_dir(&resolved)
            .map_err(|_| Self::host_error("read dir"))?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| !name.starts_with('.'))
            })
            .collect();
        paths.sort_by(|left, right| {
            let left_rank = if left.is_dir() { 0 } else { 1 };
            let right_rank = if right.is_dir() { 0 } else { 1 };
            left_rank
                .cmp(&right_rank)
                .then_with(|| left.file_name().cmp(&right.file_name()))
        });

        let total = paths.len();
        let start = page_start.min(total);
        let end = (start + page_size.min(xteink_fs::MAX_ENTRIES).max(1)).min(total);
        for path in &paths[start..end] {
            let name = Self::file_name_str(path)?;
            let short_name = Self::short_component_name(name, path.is_dir());
            let label = Self::display_label(name);
            let entry =
                listed_entry_from_parts(label.as_str(), short_name.as_str(), path.is_dir())?;
            let _ = entries.push(entry);
        }
        Ok(FsDirectoryPageInfo {
            page_start: start,
            has_prev: start > 0,
            has_next: end < total,
        })
    }

    fn open_epub_source<'a>(&'a self, path: &str) -> Result<Self::EpubSource<'a>, FsError> {
        HostEpubSource::open(&self.host_path_for(path)?)
    }

    fn open_cache_file_read<'a>(&'a self, path: &str) -> Result<Self::File<'a>, FsError> {
        HostFile::open(&self.host_path_for(path)?)
    }

    fn open_cache_file_write<'a>(&'a self, path: &str) -> Result<Self::File<'a>, FsError> {
        let (parent, leaf) = self.host_parent_and_leaf_for(path)?;
        fs::create_dir_all(&parent).map_err(|_| Self::host_error("mkdirs"))?;
        let path = parent.join(leaf);
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        HostFile::open_with_options(&path, &options)
    }

    fn open_cache_file_append<'a>(&'a self, path: &str) -> Result<Self::File<'a>, FsError> {
        let (parent, leaf) = self.host_parent_and_leaf_for(path)?;
        fs::create_dir_all(&parent).map_err(|_| Self::host_error("mkdirs"))?;
        let path = parent.join(leaf);
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).append(true);
        HostFile::open_with_options(&path, &options)
    }

    fn ensure_directory(&self, path: &str) -> Result<(), FsError> {
        fs::create_dir_all(self.host_directory_path_for(path)?)
            .map_err(|_| Self::host_error("mkdir"))?;
        Ok(())
    }
}

#[derive(Debug)]
pub enum StorageError {
    Fs(FsError),
    Render(xteink_epub::EpubError),
}

impl core::fmt::Display for StorageError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Fs(error) => write!(f, "filesystem error: {error:?}"),
            Self::Render(error) => write!(f, "render error: {error:?}"),
        }
    }
}

impl std::error::Error for StorageError {}

impl From<FsError> for StorageError {
    fn from(error: FsError) -> Self {
        Self::Fs(error)
    }
}

impl From<xteink_epub::EpubError> for StorageError {
    fn from(error: xteink_epub::EpubError) -> Self {
        Self::Render(error)
    }
}

impl AppStorage<Framebuffer> for HostStorage {
    type Error = StorageError;

    fn list_directory_page(
        &self,
        path: &str,
        page_start: usize,
        page_size: usize,
    ) -> Result<DirectoryPage, Self::Error> {
        let FsDirectoryPage { entries, info } =
            load_directory_page(self, path, page_start, page_size)?;
        let mut app_entries = heapless::Vec::new();
        for entry in entries.iter() {
            let _ = app_entries.push(Self::app_entry(entry));
        }
        Ok(DirectoryPage {
            entries: app_entries,
            info: AppDirectoryPageInfo {
                page_start: info.page_start,
                has_prev: info.has_prev,
                has_next: info.has_next,
            },
        })
    }

    fn render_epub_from_entry(
        &self,
        renderer: &mut Framebuffer,
        current_path: &str,
        entry: &ListedEntry,
    ) -> Result<EpubRenderResult, Self::Error> {
        let fs_entry =
            listed_entry_from_parts(entry.label.as_str(), entry.fs_name.as_str(), false)?;
        let rendered = render_epub_from_entry(self, renderer, current_path, &fs_entry)?;
        Ok(EpubRenderResult {
            rendered_page: rendered.rendered_page,
            progress_percent: rendered.progress_percent,
        })
    }

    fn render_epub_page_from_entry(
        &self,
        renderer: &mut Framebuffer,
        current_path: &str,
        entry: &ListedEntry,
        target_page: usize,
    ) -> Result<EpubRenderResult, Self::Error> {
        let fs_entry =
            listed_entry_from_parts(entry.label.as_str(), entry.fs_name.as_str(), false)?;
        let rendered = render_epub_page_from_entry(
            self,
            renderer,
            current_path,
            &fs_entry,
            target_page,
            true,
        )?;
        Ok(EpubRenderResult {
            rendered_page: rendered.rendered_page,
            progress_percent: rendered.progress_percent,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::HostEpubSource;
    use std::fs;
    use xteink_epub::EpubSource;

    #[test]
    fn file_source_reads_epub_bytes_on_demand() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("story.epub");
        fs::write(&path, b"abcdefghijklmnopqrstuvwxyz").expect("epub");

        let source = HostEpubSource::open(&path).expect("open");
        let mut buffer = [0u8; 5];

        assert_eq!(source.len(), 26);
        assert_eq!(source.read_at(2, &mut buffer).expect("read"), 5);
        assert_eq!(&buffer, b"cdefg");
        assert_eq!(source.read_at(30, &mut buffer).expect("eof"), 0);
    }
}
