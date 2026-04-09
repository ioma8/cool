use crate::{DirectoryPageInfo, FsError, ListedEntry, MAX_ENTRIES, SdFilesystem};

#[derive(Debug)]
pub struct DirectoryPage {
    pub entries: heapless::Vec<ListedEntry, MAX_ENTRIES>,
    pub info: DirectoryPageInfo,
}

pub fn load_directory_page<SD: SdFilesystem>(
    fs: &SD,
    current_path: &str,
    page_start: usize,
    page_size: usize,
) -> Result<DirectoryPage, FsError> {
    let mut entries: heapless::Vec<ListedEntry, MAX_ENTRIES> = heapless::Vec::new();
    let info = fs.list_directory_page(current_path, page_start, page_size, &mut entries)?;
    Ok(DirectoryPage { entries, info })
}
