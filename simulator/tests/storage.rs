use std::fs;
use std::sync::{Mutex, OnceLock};

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
fn host_storage_renders_decisive_fixture_for_first_two_pages() {
    let _guard = render_test_mutex().lock().expect("render mutex poisoned");
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .join("test/epubs/Decisive - Chip Heath.epub");
    let tmp = tempfile::tempdir().expect("tempdir");
    fs::copy(&fixture, tmp.path().join("Decisive - Chip Heath.epub")).expect("copy fixture");
    let storage = HostStorage::new(tmp.path());
    let entry = xteink_app::ListedEntry::epub("Decisive - Chip Heath.epub");

    let mut saw_non_blank = false;
    for page in 0..6 {
        let mut framebuffer = Framebuffer::new();
        let rendered = storage
            .render_epub_page_from_entry(&mut framebuffer, "/", &entry, page)
            .unwrap_or_else(|err| panic!("page {page} should render: {err:?}"));
        assert_eq!(rendered.rendered_page, page);
        assert!(rendered.progress_percent > 0);
        saw_non_blank |= framebuffer.bytes().iter().any(|byte| *byte != 0xFF);
    }
    assert!(saw_non_blank, "expected at least one early decisive page to contain visible text");
}

#[test]
fn host_storage_progress_is_non_decreasing_between_consecutive_pages_in_same_chapter() {
    let _guard = render_test_mutex().lock().expect("render mutex poisoned");
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .join("test/epubs/Decisive - Chip Heath.epub");
    let tmp = tempfile::tempdir().expect("tempdir");
    fs::copy(&fixture, tmp.path().join("Decisive - Chip Heath.epub")).expect("copy fixture");
    let storage = HostStorage::new(tmp.path());
    let entry = xteink_app::ListedEntry::epub("Decisive - Chip Heath.epub");

    let mut framebuffer = Framebuffer::new();
    let page_zero = storage
        .render_epub_page_from_entry(&mut framebuffer, "/", &entry, 0)
        .expect("page zero should render");
    let mut framebuffer = Framebuffer::new();
    let page_one = storage
        .render_epub_page_from_entry(&mut framebuffer, "/", &entry, 1)
        .expect("page one should render");

    assert!(
        page_one.progress_percent >= page_zero.progress_percent,
        "progress should not go backwards inside the same chapter: {} -> {}",
        page_zero.progress_percent,
        page_one.progress_percent
    );
}

#[test]
fn host_storage_uses_shared_cache_reader_and_writes_cache_artifacts() {
    let _guard = render_test_mutex().lock().expect("render mutex poisoned");
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .join("test/epubs/Decisive - Chip Heath.epub");
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
    let progress = tmp.path().join(cache_paths.progress.trim_start_matches('/'));

    assert!(meta.is_file(), "expected shared reader to write cache meta");
    assert!(content.is_file(), "expected shared reader to write cached content");
    assert!(progress.is_file(), "expected shared reader to write progress");
}

#[test]
fn host_storage_reopens_listed_entry_from_saved_progress() {
    let _guard = render_test_mutex().lock().expect("render mutex poisoned");
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .join("test/epubs/Decisive - Chip Heath.epub");
    let tmp = tempfile::tempdir().expect("tempdir");
    fs::copy(&fixture, tmp.path().join("Decisive - Chip Heath.epub")).expect("copy fixture");
    let storage = HostStorage::new(tmp.path());
    let page = storage.list_directory_page("/", 0, 10).expect("listing");
    let entry = page.entries[0].clone();
    let mut framebuffer = Framebuffer::new();

    let opened = storage
        .render_epub_from_entry(&mut framebuffer, "/", &entry)
        .expect("open should succeed");
    assert_eq!(opened.rendered_page, 0);

    let page_one = storage
        .render_epub_page_from_entry(&mut framebuffer, "/", &entry, 1)
        .expect("page one should render");
    let page_two = storage
        .render_epub_page_from_entry(&mut framebuffer, "/", &entry, 2)
        .expect("page two should render");

    assert_eq!(page_one.rendered_page, 1);
    assert_eq!(page_two.rendered_page, 2);

    let cache_paths = xteink_fs::cache_paths_for_epub("/", entry.fs_name.as_str());
    let progress_path = tmp.path().join(cache_paths.progress.trim_start_matches('/'));
    let progress = fs::read(progress_path).expect("progress file should exist");
    assert_eq!(u32::from_le_bytes([progress[0], progress[1], progress[2], progress[3]]), 2);

    let reopened = storage
        .render_epub_from_entry(&mut framebuffer, "/", &entry)
        .expect("reopen should resume");
    assert_eq!(reopened.rendered_page, 2);
}

#[test]
fn host_storage_ignores_saved_progress_when_cache_meta_is_stale() {
    let _guard = render_test_mutex().lock().expect("render mutex poisoned");
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .join("test/epubs/Decisive - Chip Heath.epub");
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

#[test]
fn host_storage_progress_does_not_spike_to_ninety_nine_on_early_cached_pages() {
    let _guard = render_test_mutex().lock().expect("render mutex poisoned");
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .join("test/epubs/Decisive - Chip Heath.epub");
    let tmp = tempfile::tempdir().expect("tempdir");
    fs::copy(&fixture, tmp.path().join("Decisive - Chip Heath.epub")).expect("copy fixture");
    let storage = HostStorage::new(tmp.path());
    let entry = xteink_app::ListedEntry::epub("Decisive - Chip Heath.epub");
    let mut framebuffer = Framebuffer::new();

    let page_zero = storage
        .render_epub_from_entry(&mut framebuffer, "/", &entry)
        .expect("page zero should render");
    let page_one = storage
        .render_epub_page_from_entry(&mut framebuffer, "/", &entry, 1)
        .expect("page one should render");

    assert!(page_zero.progress_percent < 50);
    assert!(page_one.progress_percent < 50);
    assert!(page_one.progress_percent >= page_zero.progress_percent);
}

#[test]
fn host_storage_progress_is_monotonic_across_early_decisive_pages() {
    let _guard = render_test_mutex().lock().expect("render mutex poisoned");
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .join("test/epubs/Decisive - Chip Heath.epub");
    let tmp = tempfile::tempdir().expect("tempdir");
    fs::copy(&fixture, tmp.path().join("Decisive - Chip Heath.epub")).expect("copy fixture");
    let storage = HostStorage::new(tmp.path());
    let entry = xteink_app::ListedEntry::epub("Decisive - Chip Heath.epub");
    let mut framebuffer = Framebuffer::new();

    let mut last_progress = 0u8;
    for page in 0..30 {
        let rendered = if page == 0 {
            storage
                .render_epub_from_entry(&mut framebuffer, "/", &entry)
                .expect("page zero should render")
        } else {
            storage
                .render_epub_page_from_entry(&mut framebuffer, "/", &entry, page)
                .expect("page should render")
        };
        assert!(
            rendered.progress_percent >= last_progress,
            "page {} progress regressed: {} -> {}",
            page,
            last_progress,
            rendered.progress_percent
        );
        last_progress = rendered.progress_percent;
    }
    assert!(last_progress < 20, "page 29 progress too high: {}", last_progress);
}
