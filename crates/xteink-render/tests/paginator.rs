use xteink_render::{DISPLAY_HEIGHT, Framebuffer, bookerly};

fn render_cached_text_page_with_chunk_size(
    text: &[u8],
    target_page: usize,
    chunk_size: usize,
) -> (Framebuffer, usize) {
    let mut framebuffer = Framebuffer::new();
    let mut offset = 0usize;

    let rendered_page = framebuffer
        .render_cached_text_page(
            &mut |buffer| {
                if offset >= text.len() {
                    return Ok(0);
                }

                let end = (offset + chunk_size)
                    .min(offset + buffer.len())
                    .min(text.len());
                let chunk = &text[offset..end];
                buffer[..chunk.len()].copy_from_slice(chunk);
                offset = end;
                Ok(chunk.len())
            },
            target_page,
        )
        .expect("cached render should succeed");

    (framebuffer, rendered_page)
}

#[test]
fn cached_text_pagination_is_stable_across_input_chunk_boundaries_at_page_breaks() {
    let line_height = usize::from(bookerly::BOOKERLY.line_height_px());
    let lines_per_page = usize::from(DISPLAY_HEIGHT) / line_height;
    let page_text = "café\n".repeat(lines_per_page);
    let text = format!(
        "\u{001c}{page_text}\u{001d}{page_text}",
        page_text = page_text
    );
    let bytes = text.as_bytes();

    let (chunked, chunked_page) = render_cached_text_page_with_chunk_size(bytes, 1, 1);
    let (bulk, bulk_page) = render_cached_text_page_with_chunk_size(bytes, 1, bytes.len());

    assert_eq!(chunked_page, 1);
    assert_eq!(bulk_page, 1);
    assert_eq!(
        chunked.bytes(),
        bulk.bytes(),
        "page output should not change when the reader splits text differently",
    );
}
