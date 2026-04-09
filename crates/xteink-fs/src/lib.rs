#![cfg_attr(not(test), no_std)]

#[cfg(test)]
extern crate std;

mod cache;
#[cfg(feature = "embedded")]
mod directory;
#[cfg(feature = "embedded")]
mod hal;
#[cfg(feature = "embedded")]
mod log;
#[cfg(feature = "embedded")]
mod low_level;
#[cfg(feature = "embedded")]
mod path;
#[cfg(feature = "embedded")]
mod reader;

pub use cache::*;
#[cfg(feature = "embedded")]
pub use directory::{DirectoryPage, ListedEntry, load_directory_page};
#[cfg(feature = "embedded")]
pub use hal::{RawGpioOutput, SD_CS_PIN, SD_POWER_PIN};
#[cfg(feature = "embedded")]
pub use low_level::MAX_ENTRIES;
#[cfg(feature = "embedded")]
pub use low_level::{DirectoryPageInfo, FsError, SdFilesystem, init_sd};
#[cfg(feature = "embedded")]
pub use path::{PATH_CAPACITY, PathError, join_child_path, normalize_path};
#[cfg(feature = "embedded")]
pub use reader::{
    EpubRefreshMode, EpubRenderResult, render_epub_from_entry, render_epub_from_entry_with_cancel,
    render_epub_page_from_entry, render_epub_page_from_entry_with_cancel,
};
