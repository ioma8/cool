use std::fs;

use simulator::{
    runtime::{bootstrap_session, simulator_device_memory_footprint},
    storage::HostStorage,
};
use xteink_app::Session;
use xteink_buttons::Button;
use xteink_memory::DEVICE_PERSISTENT_BUDGET_BYTES;
use xteink_render::Framebuffer;

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
    let fixture_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .join("test/epubs");
    let storage = HostStorage::new(&fixture_root);
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
