use xteink_render::{BUFFER_SIZE, DISPLAY_HEIGHT, DISPLAY_WIDTH, Framebuffer};

#[test]
fn framebuffer_starts_white_and_sets_black_pixels_in_device_layout() {
    let mut framebuffer = Framebuffer::new();

    assert_eq!(framebuffer.bytes().len(), BUFFER_SIZE);
    assert!(framebuffer.bytes().iter().all(|byte| *byte == 0xFF));

    framebuffer.set_pixel(0, 0, true);
    framebuffer.set_pixel(DISPLAY_WIDTH - 1, 0, true);
    framebuffer.set_pixel(0, DISPLAY_HEIGHT - 1, true);

    let bytes = framebuffer.bytes();
    assert_eq!(bytes[47900], 0x7F);
    assert_eq!(bytes[0], 0x7F);
    assert_eq!(bytes[47999], 0xFE);
}
