use std::fs;

use simulator::storage::HostStorage;
use xteink_app::AppStorage;
use xteink_browser::EntryKind;
use xteink_render::Framebuffer;

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

#[test]
fn host_storage_renders_decisive_fixture_for_first_two_pages() {
    let fixture_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .join("test/epubs");
    let storage = HostStorage::new(&fixture_root);
    let entry = xteink_app::ListedEntry::epub("Decisive - Chip Heath.epub");

    let mut saw_non_blank = false;
    for page in 0..6 {
        let mut framebuffer = Framebuffer::new();
        let rendered = storage
            .render_epub_page_from_entry(&mut framebuffer, "/", &entry, page)
            .expect("page should render");
        assert_eq!(rendered, page);
        saw_non_blank |= framebuffer.bytes().iter().any(|byte| *byte != 0xFF);
    }
    assert!(saw_non_blank, "expected at least one early decisive page to contain visible text");
}
