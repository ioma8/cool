#[path = "../src/cache.rs"]
mod cache;

use cache::{cache_paths_for_epub, cache_paths_for_epub_candidates, sanitize_cache_name};

#[test]
fn cache_paths_use_logical_dot_cool_root() {
    let paths = cache_paths_for_epub("/MYBOOKS", "PET_JA~1.EPU");

    assert!(paths.directory.as_str().starts_with("/.cool/"));
    assert!(paths.meta.as_str().starts_with("/.cool/"));
    assert!(paths.content.as_str().starts_with("/.cool/"));
    assert!(paths.progress.as_str().starts_with("/.cool/"));
}

#[test]
fn cache_path_candidates_use_dot_cool_root() {
    let candidates = cache_paths_for_epub_candidates("/MYBOOKS", "WHEN_I~1.EPU");

    for paths in candidates {
        assert!(paths.directory.as_str().starts_with("/.cool/"));
        assert!(paths.meta.as_str().starts_with("/.cool/"));
        assert!(paths.content.as_str().starts_with("/.cool/"));
        assert!(paths.progress.as_str().starts_with("/.cool/"));
    }
}

#[test]
fn sanitized_cache_name_respects_component_limit() {
    let name = sanitize_cache_name("/MYBOOKS/WHEN_I WRITE/BOOK.EPU");

    assert_eq!(name.len(), 8);
    assert!(
        name.chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    );
}
