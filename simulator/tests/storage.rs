use std::fs;
use std::sync::{Mutex, OnceLock};

fn decisive_fixture_path() -> Option<std::path::PathBuf> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .join("test/epubs/Decisive - Chip Heath.epub");
    if path.exists() { Some(path) } else { None }
}

use simulator::storage::HostStorage;
use xteink_app::AppStorage;
use xteink_browser::EntryKind;
use xteink_render::Framebuffer;

fn render_test_mutex() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

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
fn host_storage_lists_long_epub_names_without_too_many_entries() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let long_name = "This is a deliberately very long EPUB filename that exceeds the shared fs_name storage limit for simulator parity testing.epub";
    fs::write(tmp.path().join(long_name), b"epub").expect("epub");

    let storage = HostStorage::new(tmp.path());
    let page = storage
        .list_directory_page("/", 0, 10)
        .expect("listing should succeed");

    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.entries[0].kind, EntryKind::Epub);
    assert!(long_name.starts_with(page.entries[0].label.as_str()));
    assert!(page.entries[0].fs_name.len() <= 96);
}

#[test]
fn host_storage_uses_shared_cache_reader_and_writes_cache_artifacts() {
    let _guard = render_test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let Some(fixture) = decisive_fixture_path() else {
        return;
    };
    let tmp = tempfile::tempdir().expect("tempdir");
    fs::copy(&fixture, tmp.path().join("Decisive - Chip Heath.epub")).expect("copy fixture");
    let storage = HostStorage::new(tmp.path());
    let entry = xteink_app::ListedEntry::epub("Decisive - Chip Heath.epub");
    let mut framebuffer = Framebuffer::new();

    let _ = storage
        .render_epub_from_entry(&mut framebuffer, "/", &entry)
        .expect("render should succeed");

    let cache_paths = xteink_fs::cache_paths_for_epub("/", "Decisive - Chip Heath.epub");
    let meta = tmp.path().join(cache_paths.meta.trim_start_matches('/'));
    let content = tmp.path().join(cache_paths.content.trim_start_matches('/'));
    let progress = tmp
        .path()
        .join(cache_paths.progress.trim_start_matches('/'));

    assert!(meta.is_file(), "expected shared reader to write cache meta");
    assert!(
        content.is_file(),
        "expected shared reader to write cached content"
    );
    assert!(
        progress.is_file(),
        "expected shared reader to write progress"
    );
}

#[test]
fn host_storage_reopens_from_saved_current_page_offset() {
    let _guard = render_test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let Some(fixture) = decisive_fixture_path() else {
        return;
    };
    let tmp = tempfile::tempdir().expect("tempdir");
    fs::copy(&fixture, tmp.path().join("Decisive - Chip Heath.epub")).expect("copy fixture");
    let storage = HostStorage::new(tmp.path());
    let entry = xteink_app::ListedEntry::epub("Decisive - Chip Heath.epub");

    let mut first = Framebuffer::new();
    let opened = storage
        .render_epub_from_entry(&mut first, "/", &entry)
        .expect("open should succeed");
    assert_eq!(opened.rendered_page, 0);

    let mut second = Framebuffer::new();
    let page_one = storage
        .render_epub_page_from_entry(&mut second, "/", &entry, 1)
        .expect("page one should render");
    assert_eq!(page_one.rendered_page, 1);

    let mut reopened = Framebuffer::new();
    let resumed = storage
        .render_epub_from_entry(&mut reopened, "/", &entry)
        .expect("reopen should resume from current offset");

    assert_eq!(resumed.rendered_page, 1);
    assert_eq!(reopened.bytes(), second.bytes());
}

#[test]
fn host_storage_ignores_saved_progress_when_cache_meta_is_stale() {
    let _guard = render_test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let Some(fixture) = decisive_fixture_path() else {
        return;
    };
    let tmp = tempfile::tempdir().expect("tempdir");
    fs::copy(&fixture, tmp.path().join("Decisive - Chip Heath.epub")).expect("copy fixture");
    let storage = HostStorage::new(tmp.path());
    let entry = xteink_app::ListedEntry::epub("Decisive - Chip Heath.epub");
    let paths = xteink_fs::cache_paths_for_epub("/", "Decisive - Chip Heath.epub");
    let cache_dir = tmp.path().join(paths.directory.trim_start_matches('/'));
    fs::create_dir_all(&cache_dir).expect("cache dir");
    fs::write(
        cache_dir.join("meta.txt"),
        "version=0\nsource_size=2584345\ncontent_length=10\ncached_pages=5\nnext_spine_index=0\nresume_page=4\nresume_cursor_y=0\ncomplete=1\nsource_len=2584345\n",
    )
    .expect("stale meta");
    fs::write(cache_dir.join("progress.bin"), [4u8, 0, 0, 0, 80]).expect("stale progress");

    let mut framebuffer = Framebuffer::new();
    let reopened = storage
        .render_epub_from_entry(&mut framebuffer, "/", &entry)
        .expect("open should ignore stale progress");

    assert_eq!(reopened.rendered_page, 0);
}
