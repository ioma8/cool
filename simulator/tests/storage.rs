use std::fs;

use simulator::storage::HostStorage;
use xteink_app::AppStorage;
use xteink_browser::EntryKind;

#[test]
fn host_storage_maps_root_to_simulator_sdcard_and_lists_entries() {
    let tmp = tempfile::tempdir().expect("tempdir");
    fs::create_dir(tmp.path().join("Books")).expect("dir");
    fs::write(tmp.path().join("story.epub"), b"epub").expect("epub");
    fs::write(tmp.path().join("notes.txt"), b"txt").expect("txt");

    let storage = HostStorage::new(tmp.path());
    let page = storage
        .list_directory_page("/", 0, 10)
        .expect("listing should succeed");

    assert_eq!(page.entries.len(), 3);
    assert_eq!(page.entries[0].kind, EntryKind::Directory);
    assert_eq!(page.entries[1].kind, EntryKind::Other);
    assert_eq!(page.entries[2].kind, EntryKind::Epub);
}
