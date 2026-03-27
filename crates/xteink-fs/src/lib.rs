#![cfg_attr(not(test), no_std)]

#[cfg(test)]
extern crate std;

#[cfg(feature = "embedded")]
mod browser;
#[cfg(feature = "embedded")]
mod cache;
#[cfg(feature = "embedded")]
mod hal;
#[cfg(feature = "embedded")]
mod low_level;
#[cfg(feature = "embedded")]
mod path;

#[cfg(feature = "embedded")]
pub use browser::{
    load_directory_page,
    render_epub_from_entry,
    render_epub_from_entry_with_cancel,
    render_epub_page_from_entry,
    render_epub_page_from_entry_with_cancel,
    DirectoryPage,
    ListedEntry,
    EpubRefreshMode,
    EpubRenderResult,
};
#[cfg(feature = "embedded")]
pub use hal::{RawGpioOutput, SD_CS_PIN, SD_POWER_PIN};
#[cfg(feature = "embedded")]
pub use low_level::{init_sd, DirectoryPageInfo, FsError, SdFilesystem};
#[cfg(feature = "embedded")]
pub use cache::*;
#[cfg(feature = "embedded")]
pub use path::{join_child_path, normalize_path, PathError, PATH_CAPACITY};
#[cfg(feature = "embedded")]
pub use low_level::MAX_ENTRIES;
