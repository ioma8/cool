use xteink_render::{
    DISPLAY_HEIGHT, DISPLAY_WIDTH, Framebuffer, GRAY_LEVELS, SHADE_BLACK, SHADE_BUFFER_SIZE,
    SHADE_WHITE,
};

#[test]
fn framebuffer_starts_white_and_reports_shades() {
    let framebuffer = Framebuffer::new();

    assert_eq!(framebuffer.shade_storage().len(), SHADE_BUFFER_SIZE);
    assert!(
        framebuffer.shade_storage().iter().all(|byte| *byte == 0),
        "packed 2-bit white pixels should initialize to zero"
    );
    assert_eq!(framebuffer.shade_at(0, 0), SHADE_WHITE);
    assert_eq!(
        framebuffer.shade_at(DISPLAY_WIDTH - 1, DISPLAY_HEIGHT - 1),
        SHADE_WHITE
    );
}

#[test]
fn framebuffer_sets_and_reads_individual_shades() {
    let mut framebuffer = Framebuffer::new();

    framebuffer.set_shade(0, 0, 1);
    framebuffer.set_shade(1, 0, 2);
    framebuffer.set_shade(2, 0, SHADE_BLACK);
    framebuffer.set_shade(DISPLAY_WIDTH - 1, DISPLAY_HEIGHT - 1, 1);

    assert_eq!(framebuffer.shade_at(0, 0), 1);
    assert_eq!(framebuffer.shade_at(1, 0), 2);
    assert_eq!(framebuffer.shade_at(2, 0), SHADE_BLACK);
    assert_eq!(
        framebuffer.shade_at(DISPLAY_WIDTH - 1, DISPLAY_HEIGHT - 1),
        1
    );
}

#[test]
fn framebuffer_clear_and_binary_compatibility_use_grayscale_endpoints() {
    let mut framebuffer = Framebuffer::new();

    framebuffer.clear(0x00);
    assert_eq!(framebuffer.shade_at(0, 0), SHADE_BLACK);
    assert_eq!(
        framebuffer.shade_at(DISPLAY_WIDTH - 1, DISPLAY_HEIGHT - 1),
        SHADE_BLACK
    );

    framebuffer.clear(0xFF);
    assert_eq!(framebuffer.shade_at(0, 0), SHADE_WHITE);

    framebuffer.set_pixel(5, 7, true);
    framebuffer.set_pixel(6, 7, false);
    assert_eq!(framebuffer.shade_at(5, 7), SHADE_BLACK);
    assert_eq!(framebuffer.shade_at(6, 7), SHADE_WHITE);
}

#[test]
fn framebuffer_clamps_out_of_range_shade_values() {
    let mut framebuffer = Framebuffer::new();

    framebuffer.set_shade(0, 0, GRAY_LEVELS + 4);

    assert_eq!(framebuffer.shade_at(0, 0), SHADE_BLACK);
}
