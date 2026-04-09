use std::{char, env, fs, path::PathBuf};

use fontdue::{Font, FontSettings};
use freetype::Library;
use rustybuzz::{Face, UnicodeBuffer};

const FONT_SIZE: f32 = 32.0;
const FONT_PATH: &str = "../xteink-display/assets/Bookerly-Regular.ttf";
const OUTPUT_FILE: &str = "bookerly_generated.rs";

fn shaping_features() -> [rustybuzz::Feature; 3] {
    [
        rustybuzz::Feature::new(rustybuzz::ttf_parser::Tag::from_bytes(b"liga"), 0, ..),
        rustybuzz::Feature::new(rustybuzz::ttf_parser::Tag::from_bytes(b"clig"), 0, ..),
        rustybuzz::Feature::new(rustybuzz::ttf_parser::Tag::from_bytes(b"calt"), 0, ..),
    ]
}

#[derive(Clone, Copy)]
struct GlyphMeta {
    codepoint: u32,
    width: u8,
    height: u8,
    advance_x: u16,
    layout_advance_x: u16,
    left: i16,
    top: i16,
    data_offset: u32,
    data_length: u32,
}

#[derive(Clone, Copy)]
struct IntervalMeta {
    first: u32,
    last: u32,
    offset: u32,
}

#[derive(Clone, Copy)]
struct PairPositioningMeta {
    left: u32,
    right: u32,
    pen_adjust: i16,
    x_offset: i16,
    y_offset: i16,
    advance_adjust: i16,
}

fn main() {
    println!("cargo:rerun-if-changed={FONT_PATH}");

    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("missing CARGO_MANIFEST_DIR"));
    let font_path = manifest_dir.join(FONT_PATH);
    let font_bytes = fs::read(&font_path).unwrap_or_else(|err| {
        panic!("failed to read {}: {err}", font_path.display());
    });

    let font =
        Font::from_bytes(font_bytes.clone(), FontSettings::default()).unwrap_or_else(|err| {
            panic!("failed to parse {}: {err}", font_path.display());
        });
    let shape_face = Face::from_slice(&font_bytes, 0).expect("failed to parse shaping face");
    let library = Library::init().expect("failed to init freetype");
    let face = library
        .new_face(&font_path, 0)
        .unwrap_or_else(|err| panic!("failed to load {} in freetype: {err}", font_path.display()));
    face.set_pixel_sizes(0, FONT_SIZE as u32)
        .expect("failed to set freetype pixel size");

    let line_metrics = font
        .horizontal_line_metrics(FONT_SIZE)
        .expect("font does not expose horizontal line metrics");

    let ascender = line_metrics.ascent.ceil() as i16;
    let descender = line_metrics.descent.ceil() as i16;
    let line_height = line_metrics.new_line_size.ceil().max(1.0) as u16;

    let mut bitmaps = Vec::new();
    let mut glyphs = Vec::new();
    let mut intervals = Vec::new();
    let mut pair_positioning = Vec::new();
    let mut interval_start = None::<(u32, u32)>;
    let mut last_codepoint = None::<u32>;
    let mut replacement_index = None::<usize>;

    for codepoint in 0..=0x10FFFF {
        let Some(ch) = char::from_u32(codepoint) else {
            continue;
        };
        if font.lookup_glyph_index(ch) == 0 {
            continue;
        }

        face.load_char(ch as usize, freetype::face::LoadFlag::RENDER)
            .unwrap_or_else(|err| panic!("failed to render {ch:?} in freetype: {err}"));
        let glyph_slot = face.glyph();
        let bitmap = glyph_slot.bitmap();
        let packed = pack_bitmap(
            bitmap.buffer(),
            bitmap.width() as usize,
            bitmap.rows() as usize,
        );
        let data_offset = bitmaps.len() as u32;
        bitmaps.extend_from_slice(&packed);

        let glyph = GlyphMeta {
            codepoint,
            width: bitmap.width() as u8,
            height: bitmap.rows() as u8,
            advance_x: (glyph_slot.advance().x >> 6).max(0) as u16,
            layout_advance_x: shape_single_advance(&shape_face, ch),
            left: glyph_slot.bitmap_left() as i16,
            top: ascender - glyph_slot.bitmap_top() as i16,
            data_offset,
            data_length: packed.len() as u32,
        };

        if codepoint == 0xFFFD {
            replacement_index = Some(glyphs.len());
        }

        if let Some(previous) = last_codepoint {
            if previous + 1 != codepoint {
                if let Some((first, start)) = interval_start.take() {
                    intervals.push(IntervalMeta {
                        first,
                        last: previous,
                        offset: start,
                    });
                }
                interval_start = Some((codepoint, glyphs.len() as u32));
            }
        } else {
            interval_start = Some((codepoint, 0));
        }

        if interval_start.is_none() {
            interval_start = Some((codepoint, glyphs.len() as u32));
        }

        last_codepoint = Some(codepoint);
        glyphs.push(glyph);
    }

    if let Some((first, start)) = interval_start.take() {
        if let Some(last) = last_codepoint {
            intervals.push(IntervalMeta {
                first,
                last,
                offset: start,
            });
        }
    }

    for left_glyph in &glyphs {
        if !supports_pair_positioning(left_glyph.codepoint) {
            continue;
        }
        let Some(left_char) = char::from_u32(left_glyph.codepoint) else {
            continue;
        };

        for right_glyph in &glyphs {
            if !supports_pair_positioning(right_glyph.codepoint) {
                continue;
            }
            let Some(right_char) = char::from_u32(right_glyph.codepoint) else {
                continue;
            };

            let Some(pair) = shape_pair_positioning(
                left_glyph.codepoint,
                right_glyph.codepoint,
                &shape_face,
                left_char,
                right_char,
                left_glyph,
                right_glyph,
            ) else {
                continue;
            };

            pair_positioning.push(pair);
        }
    }

    let replacement_index = replacement_index.unwrap_or(0);
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("missing OUT_DIR"));
    let output_path = out_dir.join(OUTPUT_FILE);
    fs::write(
        &output_path,
        render_generated_file(
            &bitmaps,
            &glyphs,
            &intervals,
            &pair_positioning,
            ascender,
            descender,
            line_height,
            replacement_index,
        ),
    )
    .unwrap_or_else(|err| panic!("failed to write {}: {err}", output_path.display()));
}

fn pack_bitmap(bitmap: &[u8], width: usize, height: usize) -> Vec<u8> {
    let row_bytes = width.div_ceil(8);
    let mut packed = vec![0u8; row_bytes * height];

    for y in 0..height {
        for x in 0..width {
            if bitmap[y * width + x] >= 0x80 {
                let index = y * row_bytes + x / 8;
                packed[index] |= 1 << (7 - (x % 8));
            }
        }
    }

    packed
}

fn render_generated_file(
    bitmaps: &[u8],
    glyphs: &[GlyphMeta],
    intervals: &[IntervalMeta],
    pair_positioning: &[PairPositioningMeta],
    ascender: i16,
    descender: i16,
    line_height: u16,
    replacement_index: usize,
) -> String {
    let mut out = String::new();
    out.push_str("pub static BOOKERLY_BITMAPS: &[u8] = &[\n");
    for chunk in bitmaps.chunks(16) {
        out.push_str("    ");
        for byte in chunk {
            out.push_str(&format!("0x{byte:02X}, "));
        }
        out.push('\n');
    }
    out.push_str("];\n\n");

    out.push_str("pub static BOOKERLY_GLYPHS: &[Glyph] = &[\n");
    for glyph in glyphs {
        out.push_str(&format!(
            "    Glyph {{ codepoint: 0x{:X}, width: {}, height: {}, advance_x: {}, layout_advance_x: {}, left: {}, top: {}, data_offset: {}, data_length: {} }},\n",
            glyph.codepoint,
            glyph.width,
            glyph.height,
            glyph.advance_x,
            glyph.layout_advance_x,
            glyph.left,
            glyph.top,
            glyph.data_offset,
            glyph.data_length
        ));
    }
    out.push_str("];\n\n");

    out.push_str("pub static BOOKERLY_INTERVALS: &[Interval] = &[\n");
    for interval in intervals {
        out.push_str(&format!(
            "    Interval {{ first: 0x{:X}, last: 0x{:X}, offset: {} }},\n",
            interval.first, interval.last, interval.offset
        ));
    }
    out.push_str("];\n\n");

    out.push_str("pub static BOOKERLY_PAIR_POSITIONING: &[PairPositioning] = &[\n");
    for pair in pair_positioning {
        out.push_str(&format!(
            "    PairPositioning {{ key: 0x{:016X}, pen_adjust: {}, x_offset: {}, y_offset: {}, advance_adjust: {} }},\n",
            ((pair.left as u64) << 32) | (pair.right as u64),
            pair.pen_adjust,
            pair.x_offset,
            pair.y_offset,
            pair.advance_adjust
        ));
    }
    out.push_str("];\n\n");

    out.push_str(&format!(
        "pub static BOOKERLY: Font = Font {{ bitmap: BOOKERLY_BITMAPS, glyphs: BOOKERLY_GLYPHS, intervals: BOOKERLY_INTERVALS, pair_positioning: BOOKERLY_PAIR_POSITIONING, ascender: {ascender}, descender: {descender}, line_height: {line_height}, replacement_index: {replacement_index} }};\n"
    ));

    out
}

fn shape_single_advance(face: &Face<'_>, ch: char) -> u16 {
    let mut text = String::new();
    text.push(ch);

    let features = shaping_features();
    let mut buffer = UnicodeBuffer::new();
    buffer.push_str(&text);
    let output = rustybuzz::shape(face, &features, buffer);
    let Some(position) = output.glyph_positions().first() else {
        return 0;
    };

    let scale = FONT_SIZE / face.units_per_em() as f32;
    ((position.x_advance as f32 * scale).round() as i32).max(0) as u16
}

fn shape_pair_positioning(
    left_codepoint: u32,
    right_codepoint: u32,
    face: &Face<'_>,
    left_char: char,
    right_char: char,
    left_glyph: &GlyphMeta,
    right_glyph: &GlyphMeta,
) -> Option<PairPositioningMeta> {
    let mut text = String::new();
    text.push(left_char);
    text.push(right_char);

    let mut buffer = UnicodeBuffer::new();
    buffer.push_str(&text);
    let features = shaping_features();
    let output = rustybuzz::shape(face, &features, buffer);
    let positions = output.glyph_positions();
    if positions.len() != 2 {
        return None;
    }

    let scale = FONT_SIZE / face.units_per_em() as f32;
    let pen_adjust =
        (positions[0].x_advance as f32 * scale).round() as i16 - left_glyph.advance_x as i16;
    let x_offset = (positions[1].x_offset as f32 * scale).round() as i16;
    let y_offset = -((positions[1].y_offset as f32 * scale).round() as i16);
    let advance_adjust =
        (positions[1].x_advance as f32 * scale).round() as i16 - right_glyph.advance_x as i16;

    if pen_adjust == 0 && x_offset == 0 && y_offset == 0 && advance_adjust == 0 {
        return None;
    }

    Some(PairPositioningMeta {
        left: left_codepoint,
        right: right_codepoint,
        pen_adjust,
        x_offset,
        y_offset,
        advance_adjust,
    })
}

fn supports_pair_positioning(codepoint: u32) -> bool {
    matches!(codepoint, 0x0020..=0x017F | 0x2010..=0x201F | 0x2026 | 0x20AC)
}
