use xteink_render::{Framebuffer, bookerly};

#[test]
fn draw_text_uses_bookerly_glyphs_and_changes_expected_pixels() {
    let mut framebuffer = Framebuffer::new();

    assert!(bookerly::BOOKERLY.line_height_px() > 0);

    framebuffer.draw_text(4, 4, "Hi");

    assert!(framebuffer.bytes().iter().any(|byte| *byte != 0xFF));
}

#[test]
fn wrapped_text_advances_to_multiple_lines_when_width_is_tight() {
    let mut framebuffer = Framebuffer::new();
    let start_y = 4;

    let end_y = framebuffer.draw_wrapped_text(400, start_y, "alpha beta gamma", 200);

    assert!(end_y > start_y + bookerly::BOOKERLY.line_height_px());
}
