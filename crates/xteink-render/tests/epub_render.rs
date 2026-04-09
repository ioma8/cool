use xteink_render::{Framebuffer, bookerly};

static EPUB_RENDER_TEST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct EmptySource;

impl xteink_epub::EpubSource for EmptySource {
    fn len(&self) -> usize {
        0
    }

    fn read_at(&self, _offset: u64, _buffer: &mut [u8]) -> Result<usize, xteink_epub::EpubError> {
        Ok(0)
    }
}

struct VecSource {
    bytes: Vec<u8>,
}

impl xteink_epub::EpubSource for VecSource {
    fn len(&self) -> usize {
        self.bytes.len()
    }

    fn read_at(&self, offset: u64, buffer: &mut [u8]) -> Result<usize, xteink_epub::EpubError> {
        let start = offset as usize;
        if start >= self.bytes.len() {
            return Ok(0);
        }
        let end = (start + buffer.len()).min(self.bytes.len());
        let chunk = &self.bytes[start..end];
        buffer[..chunk.len()].copy_from_slice(chunk);
        Ok(chunk.len())
    }
}

#[test]
fn cached_text_pagination_advances_to_requested_page() {
    let _guard = EPUB_RENDER_TEST_MUTEX
        .lock()
        .expect("render test mutex poisoned");
    let mut framebuffer = Framebuffer::new();
    let paragraph = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu\n";
    let text = paragraph.repeat(80);
    let mut offset = 0usize;

    let rendered_page = framebuffer
        .render_cached_text_page(
            &mut |buffer| {
                if offset >= text.len() {
                    return Ok(0);
                }
                let end = (offset + buffer.len()).min(text.len());
                let chunk = &text.as_bytes()[offset..end];
                buffer[..chunk.len()].copy_from_slice(chunk);
                offset = end;
                Ok(chunk.len())
            },
            1,
        )
        .expect("cached render should succeed");

    assert_eq!(rendered_page, 1);
    assert!(framebuffer.bytes().iter().any(|byte| *byte != 0xFF));
    assert!(bookerly::BOOKERLY.line_height_px() > 0);
}

#[test]
fn epub_render_api_returns_error_for_invalid_empty_source() {
    let _guard = EPUB_RENDER_TEST_MUTEX
        .lock()
        .expect("render test mutex poisoned");
    let mut framebuffer = Framebuffer::new();

    let result = framebuffer.render_epub_page(EmptySource, 0);

    assert!(matches!(result, Err(xteink_epub::EpubError::InvalidFormat)));
}

#[test]
fn real_fixture_first_page_does_not_fail_with_out_of_space() {
    let _guard = EPUB_RENDER_TEST_MUTEX
        .lock()
        .expect("render test mutex poisoned");
    let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("test")
        .join("epubs")
        .join("Decisive - Chip Heath.epub");
    let bytes = std::fs::read(fixture).expect("fixture should be readable");
    let mut framebuffer = Framebuffer::new();

    let result = framebuffer.render_epub_page(VecSource { bytes }, 0);

    assert!(!matches!(result, Err(xteink_epub::EpubError::OutOfSpace)));
    assert_eq!(result.expect("render should succeed"), 0);
    assert!(
        framebuffer.bytes().iter().any(|byte| *byte != 0xFF),
        "first page should not be blank"
    );
}

fn fixture_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("test")
        .join("epubs")
}

fn epub_fixture_paths() -> Vec<std::path::PathBuf> {
    let mut fixtures = std::fs::read_dir(fixture_dir())
        .expect("fixture directory should exist")
        .map(|entry| entry.expect("fixture entry should be readable").path())
        .filter(|path| path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("epub")))
        .collect::<Vec<_>>();
    fixtures.sort();
    fixtures
}

#[test]
fn all_epub_fixtures_first_page_render_without_out_of_space() {
    let _guard = EPUB_RENDER_TEST_MUTEX
        .lock()
        .expect("render test mutex poisoned");
    let fixtures = epub_fixture_paths();
    assert!(!fixtures.is_empty(), "expected at least one epub fixture");

    for fixture in fixtures {
        let name = fixture
            .file_name()
            .and_then(|name| name.to_str())
            .expect("fixture file name should be valid utf-8")
            .to_owned();
        let bytes = std::fs::read(&fixture).expect("fixture should be readable");
        let mut framebuffer = Framebuffer::new();
        let result = framebuffer.render_epub_page(VecSource { bytes }, 0);

        assert!(
            !matches!(result, Err(xteink_epub::EpubError::OutOfSpace)),
            "fixture should not exhaust workspace on first-page render: {name}, got {result:?}"
        );
    }
}

#[test]
fn large_fixture_cold_parse_to_cache_without_out_of_space() {
    let _guard = EPUB_RENDER_TEST_MUTEX
        .lock()
        .expect("render test mutex poisoned");
    let fixture = fixture_dir().join("Happiness Trap Pocketbook, The - Russ Harris.epub");
    let bytes = std::fs::read(&fixture).expect("fixture should be readable");
    let mut framebuffer = Framebuffer::new();
    let mut emitted_text_bytes = 0usize;

    let result = framebuffer.render_epub_page_with_text_sink_and_cancel(
        VecSource { bytes },
        0,
        |chunk| {
            emitted_text_bytes = emitted_text_bytes.saturating_add(chunk.len());
            Ok(())
        },
        true,
        || false,
    );

    assert!(!matches!(result, Err(xteink_epub::EpubError::OutOfSpace)));
    assert!(result.is_ok(), "large fixture cold parse should succeed: {result:?}");
    assert!(emitted_text_bytes > 0);
}

#[test]
fn cache_prefix_build_stops_before_full_book_for_large_fixture() {
    let _guard = EPUB_RENDER_TEST_MUTEX
        .lock()
        .expect("render test mutex poisoned");
    let fixture = fixture_dir().join("Happiness Trap Pocketbook, The - Russ Harris.epub");
    let bytes = std::fs::read(&fixture).expect("fixture should be readable");
    let mut framebuffer = Framebuffer::new();
    let mut emitted_text_bytes = 0usize;

    let result = framebuffer.build_epub_cache_prefix_with_text_sink_and_cancel(
        VecSource { bytes },
        0,
        |chunk| {
            emitted_text_bytes = emitted_text_bytes.saturating_add(chunk.len());
            Ok(())
        },
        || false,
    );

    let result = result.expect("cache prefix build should succeed");
    assert_eq!(result.rendered_page, 0);
    assert!(result.cached_pages >= 1);
    assert!(emitted_text_bytes > 0);
}

#[test]
fn cached_page_matches_direct_epub_render_for_following_page() {
    let _guard = EPUB_RENDER_TEST_MUTEX
        .lock()
        .expect("render test mutex poisoned");
    let fixture = fixture_dir().join("Decisive - Chip Heath.epub");
    let bytes = std::fs::read(&fixture).expect("fixture should be readable");
    let target_page = 1usize;

    let mut direct = Framebuffer::new();
    let direct_result = direct.render_epub_page(VecSource { bytes: bytes.clone() }, target_page);
    assert_eq!(direct_result.expect("direct render should succeed"), target_page);
    assert!(
        direct.bytes().iter().any(|byte| *byte != 0xFF),
        "direct target page should not be blank"
    );

    let mut cached_text = Vec::<u8>::new();
    let mut builder = Framebuffer::new();
    let build_result = builder.render_epub_page_with_text_sink_and_cancel(
        VecSource { bytes },
        target_page,
        |chunk| {
            cached_text.extend_from_slice(chunk.as_bytes());
            Ok(())
        },
        true,
        || false,
    );
    assert_eq!(build_result.expect("cache build should succeed"), target_page);
    assert!(!cached_text.is_empty(), "expected cached text to be emitted");

    let mut cached = Framebuffer::new();
    let mut offset = 0usize;
    let cached_result = cached.render_cached_text_page(
        &mut |buffer| {
            if offset >= cached_text.len() {
                return Ok(0);
            }
            let end = (offset + buffer.len()).min(cached_text.len());
            let chunk = &cached_text[offset..end];
            buffer[..chunk.len()].copy_from_slice(chunk);
            offset = end;
            Ok(chunk.len())
        },
        target_page,
    );
    assert_eq!(cached_result.expect("cached render should succeed"), target_page);
    assert!(
        cached.bytes().iter().any(|byte| *byte != 0xFF),
        "cached target page should not be blank"
    );
    if cached.bytes() != direct.bytes() {
        let first_diff = cached
            .bytes()
            .iter()
            .zip(direct.bytes().iter())
            .position(|(left, right)| left != right)
            .unwrap_or(0);
        let cached_nonwhite = cached.bytes().iter().filter(|&&b| b != 0xFF).count();
        let direct_nonwhite = direct.bytes().iter().filter(|&&b| b != 0xFF).count();
        panic!(
            "cached page rendering must match direct EPUB rendering for the same page: first_diff_byte={} cached_nonwhite={} direct_nonwhite={}",
            first_diff,
            cached_nonwhite,
            direct_nonwhite,
        );
    }
    assert_eq!(
        cached.bytes(),
        direct.bytes(),
        "cached page rendering must match direct EPUB rendering for the same page"
    );
}

#[test]
fn cold_open_prefix_render_matches_direct_first_page() {
    let _guard = EPUB_RENDER_TEST_MUTEX
        .lock()
        .expect("render test mutex poisoned");
    let fixture = fixture_dir().join("Decisive - Chip Heath.epub");
    let bytes = std::fs::read(&fixture).expect("fixture should be readable");

    let mut direct = Framebuffer::new();
    let direct_result = direct.render_epub_page(VecSource { bytes: bytes.clone() }, 0);
    assert_eq!(direct_result.expect("direct render should succeed"), 0);

    let mut cached_text = Vec::<u8>::new();
    let mut cold_open = Framebuffer::new();
    let build_result = cold_open.build_epub_cache_prefix_with_text_sink_and_cancel(
        VecSource { bytes },
        0,
        |chunk| {
            cached_text.extend_from_slice(chunk.as_bytes());
            Ok(())
        },
        || false,
    );
    let build = build_result.expect("cold-open prefix build should succeed");
    assert_eq!(build.rendered_page, 0);
    assert!(!cached_text.is_empty(), "expected cached text to be emitted");
    assert_eq!(
        cold_open.bytes(),
        direct.bytes(),
        "cold-open prefix rendering must match direct first-page rendering"
    );
}
