#[path = "../src/cache.rs"]
mod cache;

use cache::{
    CACHE_VERSION, CacheMeta, ProgressState, cache_paths_for_epub, decode_offset, decode_progress,
    encode_offset, encode_progress, parse_meta, sanitize_cache_name, serialize_meta,
};

fn sample_meta() -> CacheMeta {
    CacheMeta {
        version: CACHE_VERSION,
        source_size: 2584345,
        content_length: 281307,
        build_complete: false,
        next_chapter_index: 17,
        layout_sig_version: 1,
        layout_sig_width: 600,
        layout_sig_height: 800,
        layout_sig_content_height: 749,
        layout_sig_font: 0xDEAD_BEEF,
        layout_sig_paginator: 0x1234_5678,
    }
}

#[test]
fn cache_paths_use_logical_dot_cool_root() {
    let paths = cache_paths_for_epub("/MYBOOKS", "PET_JA~1.EPU");

    assert!(paths.directory.as_str().starts_with("/.cool/"));
    assert!(paths.meta.as_str().starts_with("/.cool/"));
    assert!(paths.content.as_str().starts_with("/.cool/"));
    assert!(paths.chapters.as_str().starts_with("/.cool/"));
    assert!(paths.pages.as_str().starts_with("/.cool/"));
    assert!(paths.progress.as_str().starts_with("/.cool/"));
    assert!(paths.chapters.ends_with("chapters.idx"));
    assert!(paths.pages.ends_with("pages.idx"));
}

#[test]
fn cache_meta_roundtrip() {
    let raw = serialize_meta(&sample_meta());
    assert_eq!(parse_meta(raw.as_str()), Some(sample_meta()));
}

#[test]
fn progress_roundtrip() {
    let progress = ProgressState {
        current_byte_offset: 123_456,
        current_page_hint: 33,
    };
    assert_eq!(decode_progress(&encode_progress(progress)), Some(progress));
}

#[test]
fn idx_offset_roundtrip() {
    let offset = 0x1122_3344_5566_7788;
    assert_eq!(decode_offset(&encode_offset(offset)), Some(offset));
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
