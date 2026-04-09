use std::fs;
use std::sync::{Mutex, OnceLock};

fn decisive_fixture_path() -> Option<std::path::PathBuf> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .join("test/epubs/Decisive - Chip Heath.epub");
    if path.exists() {
        Some(path)
    } else if fixtures_required() {
        panic!(
            "EPUB fixtures required but missing. Set up test/epubs or unset REQUIRE_EPUB_FIXTURES."
        );
    } else {
        None
    }
}

fn fixtures_required() -> bool {
    matches!(
        std::env::var("REQUIRE_EPUB_FIXTURES").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE") | Ok("yes") | Ok("YES")
    )
}

use simulator::{
    runtime::{bootstrap_session, simulator_device_memory_footprint},
    storage::HostStorage,
};
use xteink_app::Session;
use xteink_buttons::Button;
use xteink_memory::DEVICE_PERSISTENT_BUDGET_BYTES;
use xteink_render::Framebuffer;

fn render_test_mutex() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[test]
fn runtime_boots_root_directory_and_renders_first_browser_screen() {
    let tmp = tempfile::tempdir().expect("tempdir");
    fs::write(tmp.path().join("story.epub"), b"epub").expect("epub");

    let session = bootstrap_session(HostStorage::new(tmp.path()), 8).expect("bootstrap");

    assert!(session.renderer().bytes().iter().any(|byte| *byte != 0xFF));
    assert_eq!(session.current_entries().len(), 1);
}

#[test]
fn simulator_device_footprint_stays_within_budget() {
    let footprint = simulator_device_memory_footprint(1);
    assert!(footprint.fits_device_budget());
    assert!(footprint.device_bytes <= DEVICE_PERSISTENT_BUDGET_BYTES);
    assert!(footprint.host_only_bytes > 0);
}

#[test]
fn runtime_opens_decisive_fixture_to_non_blank_reader_page() {
    let _guard = render_test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let Some(fixture) = decisive_fixture_path() else {
        return;
    };
    let tmp = tempfile::tempdir().expect("tempdir");
    fs::copy(&fixture, tmp.path().join("Decisive - Chip Heath.epub")).expect("copy fixture");
    let storage = HostStorage::new(tmp.path());
    let mut session = Session::new(storage, Framebuffer::new(), 8);

    session.bootstrap().expect("bootstrap");
    let refresh = session
        .handle_button(Button::Back)
        .expect("open selected epub should succeed");

    assert!(refresh.is_some());
    assert!(
        session.renderer().bytes().iter().any(|byte| *byte != 0xFF),
        "opening a real epub through the simulator session should render a non-blank page"
    );

    let refresh = session
        .handle_button(Button::Down)
        .expect("next page should render");
    assert!(refresh.is_some());
    assert!(
        session.renderer().bytes().iter().any(|byte| *byte != 0xFF),
        "turning to the next page should also render a non-blank page"
    );
}

#[test]
fn runtime_reopens_epub_from_saved_progress() {
    let _guard = render_test_mutex()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let Some(fixture) = decisive_fixture_path() else {
        return;
    };
    let tmp = tempfile::tempdir().expect("tempdir");
    fs::copy(&fixture, tmp.path().join("Decisive - Chip Heath.epub")).expect("copy fixture");

    let storage = HostStorage::new(tmp.path());
    let mut session = Session::new(storage, Framebuffer::new(), 8);

    session.bootstrap().expect("bootstrap");
    session
        .handle_button(Button::Back)
        .expect("open selected epub");
    session
        .handle_button(Button::Down)
        .expect("page 1 should render");
    session
        .handle_button(Button::Down)
        .expect("page 2 should render");
    assert_eq!(session.reader_page(), 2);

    session
        .handle_button(Button::Back)
        .expect("return to browser");
    assert_eq!(session.current_path(), "/");

    session
        .handle_button(Button::Back)
        .expect("reopen selected epub");
    assert_eq!(
        session.reader_page(),
        2,
        "reopening an epub should resume from the last saved reading position"
    );
}
