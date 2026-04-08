use xteink_render::{Framebuffer, bookerly};

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
    let mut framebuffer = Framebuffer::new();

    let result = framebuffer.render_epub_page(EmptySource, 0);

    assert!(matches!(result, Err(xteink_epub::EpubError::InvalidFormat)));
}

#[test]
fn real_fixture_first_page_does_not_fail_with_out_of_space() {
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
            "fixture should not exhaust workspace on first-page render: {name}"
        );
    }
}

#[test]
fn large_fixture_cold_parse_to_cache_without_out_of_space() {
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
