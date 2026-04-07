use std::fs;

use simulator::{
    runtime::{bootstrap_session, simulator_device_memory_footprint},
    storage::HostStorage,
};
use xteink_memory::DEVICE_PERSISTENT_BUDGET_BYTES;

#[test]
fn runtime_boots_root_directory_and_renders_first_browser_screen() {
    let tmp = tempfile::tempdir().expect("tempdir");
    fs::write(tmp.path().join("story.epub"), b"epub").expect("epub");

    let session = bootstrap_session(HostStorage::new(tmp.path()), 8).expect("bootstrap");

    assert!(
        session
            .framebuffer()
            .bytes()
            .iter()
            .any(|byte| *byte != 0xFF)
    );
    assert_eq!(session.current_entries().len(), 1);
}

#[test]
fn simulator_device_footprint_stays_within_budget() {
    let footprint = simulator_device_memory_footprint(1);
    assert!(footprint.fits_device_budget());
    assert!(footprint.device_bytes <= DEVICE_PERSISTENT_BUDGET_BYTES);
    assert!(footprint.host_only_bytes > 0);
}
