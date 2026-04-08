use fontdue::{Font, FontSettings};
use freetype::Library;
use rustybuzz::{Face, Feature, UnicodeBuffer};
use rustybuzz::ttf_parser::Tag;
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

#[test]
fn generated_bookerly_metrics_match_font_bitmap_baseline_layout() {
    const FONT_SIZE: f32 = 32.0;
    let font = Font::from_bytes(
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../xteink-display/assets/Bookerly-Regular.ttf"
        )) as &[u8],
        FontSettings::default(),
    )
    .expect("Bookerly should parse");
    let line_metrics = font
        .horizontal_line_metrics(FONT_SIZE)
        .expect("Bookerly should expose line metrics");
    let ascender = line_metrics.ascent.ceil() as i16;

    for ch in ['A', 'g', 'j', 'Q', 'y', 'p'] {
        let (metrics, _) = font.rasterize(ch, FONT_SIZE);
        let glyph = bookerly::BOOKERLY.glyph_for_char(ch);

        assert_eq!(glyph.left, metrics.xmin as i16, "left mismatch for {ch}");
        assert_eq!(
            glyph.top,
            ascender - (metrics.height as i16) - (metrics.ymin as i16),
            "top mismatch for {ch}"
        );
    }
}

#[test]
fn generated_bookerly_pair_positioning_matches_gpos_for_common_pairs() {
    const FONT_SIZE: f32 = 32.0;
    let font_bytes = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../xteink-display/assets/Bookerly-Regular.ttf"
    )) as &[u8];
    let face = Face::from_slice(font_bytes, 0).expect("Bookerly shaping face should parse");
    let scale = FONT_SIZE / face.units_per_em() as f32;
    let samples = [('A', 'V'), ('T', 'o'), ('W', 'a'), ('Y', 'o'), ('L', 'y')];
    let mut matched_any = false;

    for (left, right) in samples {
        let Some(expected) = shape_pair_positioning(&face, scale, left, right) else {
            continue;
        };
        let actual = bookerly::BOOKERLY.positioning_for_pair(left, right);
        assert_eq!(actual.pen_adjust, expected.pen_adjust, "pen adjust mismatch for {left}{right}");
        assert_eq!(actual.x_offset, expected.x_offset, "x offset mismatch for {left}{right}");
        assert_eq!(actual.y_offset, expected.y_offset, "y offset mismatch for {left}{right}");
        assert_eq!(
            actual.advance_adjust,
            expected.advance_adjust,
            "advance adjust mismatch for {left}{right}"
        );
        matched_any = true;
    }

    assert!(matched_any, "expected at least one common pair to use GPOS positioning");
}

#[test]
fn renderer_positions_match_full_shaping_for_office() {
    const FONT_SIZE: f32 = 32.0;
    let font_bytes = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../xteink-display/assets/Bookerly-Regular.ttf"
    )) as &[u8];
    let face = Face::from_slice(font_bytes, 0).expect("Bookerly shaping face should parse");
    let scale = FONT_SIZE / face.units_per_em() as f32;
    let text = "office";

    assert_eq!(
        renderer_positions(text),
        shaped_positions(&face, scale, text),
        "renderer should match full shaping positions"
    );
}

#[test]
fn generated_bookerly_metrics_match_hinted_freetype_metrics() {
    let library = Library::init().expect("freetype should init");
    let face = library
        .new_face(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../xteink-display/assets/Bookerly-Regular.ttf"
            ),
            0,
        )
        .expect("Bookerly should load in freetype");
    face.set_pixel_sizes(0, 32).expect("pixel size should set");

    for ch in ['A', 'H', 'x', 'g', 'p', 'j', 'Q', 'y'] {
        face.load_char(ch as usize, freetype::face::LoadFlag::RENDER)
            .expect("glyph should render");
        let glyph_slot = face.glyph();
        let bitmap = glyph_slot.bitmap();
        let glyph = bookerly::BOOKERLY.glyph_for_char(ch);
        let ascender = bookerly::BOOKERLY.ascender_px();
        let expected_top = ascender - glyph_slot.bitmap_top() as i16;

        assert_eq!(glyph.left, glyph_slot.bitmap_left() as i16, "left mismatch for {ch}");
        assert_eq!(glyph.top, expected_top, "top mismatch for {ch}");
        assert_eq!(glyph.width, bitmap.width() as u8, "width mismatch for {ch}");
        assert_eq!(glyph.height, bitmap.rows() as u8, "height mismatch for {ch}");
        assert_eq!(
            glyph.advance_x,
            (glyph_slot.advance().x >> 6) as u16,
            "advance mismatch for {ch}"
        );
    }
}

#[test]
fn generated_bookerly_bitmaps_match_hinted_freetype_rasterization() {
    let library = Library::init().expect("freetype should init");
    let face = library
        .new_face(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../xteink-display/assets/Bookerly-Regular.ttf"
            ),
            0,
        )
        .expect("Bookerly should load in freetype");
    face.set_pixel_sizes(0, 32).expect("pixel size should set");

    for ch in ['A', 'e', 'g', 'j', 'Q', 'y'] {
        face.load_char(ch as usize, freetype::face::LoadFlag::RENDER)
            .expect("glyph should render");
        let slot = face.glyph();
        let bitmap = slot.bitmap();
        let glyph = bookerly::BOOKERLY.glyph_for_char(ch);
        let actual = unpack_bookerly_bitmap(glyph);
        let expected = bitmap
            .buffer()
            .iter()
            .map(|value| *value > 0)
            .collect::<Vec<_>>();
        assert_eq!(actual, expected, "bitmap mismatch for {ch}");
    }
}

fn shape_pair_positioning(
    face: &Face<'_>,
    scale: f32,
    left: char,
    right: char,
) -> Option<bookerly::PairPositioning> {
    let left_glyph = bookerly::BOOKERLY.glyph_for_char(left);
    let right_glyph = bookerly::BOOKERLY.glyph_for_char(right);
    let mut text = String::new();
    text.push(left);
    text.push(right);

    let mut buffer = UnicodeBuffer::new();
    buffer.push_str(&text);
    let output = rustybuzz::shape(face, &[], buffer);
    let positions = output.glyph_positions();
    if positions.len() != 2 {
        return None;
    }

    let pen_adjust =
        (positions[0].x_advance as f32 * scale).round() as i16 - left_glyph.advance_x as i16;
    let x_offset = (positions[1].x_offset as f32 * scale).round() as i16;
    let y_offset = -((positions[1].y_offset as f32 * scale).round() as i16);
    let advance_adjust =
        (positions[1].x_advance as f32 * scale).round() as i16 - right_glyph.advance_x as i16;

    if pen_adjust == 0 && x_offset == 0 && y_offset == 0 && advance_adjust == 0 {
        return None;
    }

    Some(bookerly::PairPositioning {
        key: ((left as u64) << 32) | (right as u64),
        pen_adjust,
        x_offset,
        y_offset,
        advance_adjust,
    })
}

fn renderer_positions(text: &str) -> Vec<i32> {
    let mut positions = Vec::new();
    bookerly::BOOKERLY.shape_text(text, |glyph, glyph_x, _| {
        positions.push(glyph_x + i32::from(glyph.left));
    });
    positions
}

fn shaped_positions(face: &Face<'_>, scale: f32, text: &str) -> Vec<i32> {
    let features = [
        Feature::new(Tag::from_bytes(b"liga"), 0, ..),
        Feature::new(Tag::from_bytes(b"clig"), 0, ..),
        Feature::new(Tag::from_bytes(b"calt"), 0, ..),
    ];
    let mut buffer = UnicodeBuffer::new();
    buffer.push_str(text);
    let output = rustybuzz::shape(face, &features, buffer);
    let mut pen = 0i32;
    let mut positions = Vec::with_capacity(output.glyph_positions().len());
    for (ch, position) in text.chars().zip(output.glyph_positions()) {
        let glyph = bookerly::BOOKERLY.glyph_for_char(ch);
        positions.push(pen + (position.x_offset as f32 * scale).round() as i32 + i32::from(glyph.left));
        pen += (position.x_advance as f32 * scale).round() as i32;
    }

    positions
}

fn unpack_bookerly_bitmap(glyph: &bookerly::Glyph) -> Vec<bool> {
    let bitmap = &bookerly::BOOKERLY.bitmap
        [glyph.data_offset as usize..(glyph.data_offset + glyph.data_length) as usize];
    let row_bytes = usize::from(glyph.width).div_ceil(8);
    let mut unpacked = Vec::with_capacity(usize::from(glyph.width) * usize::from(glyph.height));

    for row in 0..glyph.height {
        let row_start = usize::from(row) * row_bytes;
        for col in 0..glyph.width {
            let byte = bitmap[row_start + usize::from(col / 8)];
            let mask = 1 << (7 - (col % 8));
            unpacked.push(byte & mask != 0);
        }
    }

    unpacked
}
