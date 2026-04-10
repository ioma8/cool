#![cfg_attr(not(test), no_std)]

#[cfg(test)]
extern crate std;

mod cache;
mod directory;
mod filesystem;
#[cfg(feature = "embedded")]
mod hal;
mod log;
#[cfg(feature = "embedded")]
mod low_level;
mod path;
mod reader;

pub use cache::*;
pub use directory::{DirectoryPage, load_directory_page};
pub use filesystem::{
    DirectoryPageInfo, FsError, ListedEntry, MAX_ENTRIES, SdFilesystem, SdFsFile, is_epub_label,
    listed_entry_from_parts,
};
#[cfg(feature = "embedded")]
pub use hal::{RawGpioOutput, SD_CS_PIN, SD_POWER_PIN};
#[cfg(feature = "embedded")]
pub use low_level::init_sd;
pub use path::{PATH_CAPACITY, PathError, join_child_path, normalize_path};
pub use reader::{
    EpubRefreshMode, EpubRenderResult, list_epub_chapter_page, render_epub_chapter_from_entry,
    render_epub_from_entry, render_epub_from_entry_with_cancel, render_epub_page_from_entry,
    render_epub_page_from_entry_with_cancel,
};
