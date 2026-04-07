use std::fs;

use simulator::{runtime::bootstrap_session, storage::HostStorage};

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
