#![cfg_attr(not(test), no_std)]

#[cfg(test)]
extern crate std;

mod browser;
mod cache;
mod hal;
mod low_level;
mod path;

pub use browser::{
    load_directory_page,
    render_epub_from_entry,
    render_epub_page_from_entry,
    DirectoryPage,
    ListedEntry,
    EpubRefreshMode,
    EpubRenderResult,
};
pub use hal::{RawGpioOutput, SD_CS_PIN, SD_POWER_PIN};
pub use low_level::{init_sd, DirectoryPageInfo, FsError, SdFilesystem};
pub use cache::*;
pub use path::{join_child_path, normalize_path, PathError, PATH_CAPACITY};
pub use low_level::MAX_ENTRIES;
