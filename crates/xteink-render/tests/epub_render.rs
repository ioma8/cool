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
