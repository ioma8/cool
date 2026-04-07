use std::{char, env, fs, path::PathBuf};

use fontdue::{Font, FontSettings};

const FONT_SIZE: f32 = 32.0;
const FONT_PATH: &str = "../xteink-display/assets/Bookerly-Regular.ttf";
const OUTPUT_FILE: &str = "bookerly_generated.rs";

#[derive(Clone, Copy)]
struct GlyphMeta {
    codepoint: u32,
    width: u8,
    height: u8,
    advance_x: u16,
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

fn main() {
    println!("cargo:rerun-if-changed={FONT_PATH}");

    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("missing CARGO_MANIFEST_DIR"));
    let font_path = manifest_dir.join(FONT_PATH);
    let font_bytes = fs::read(&font_path).unwrap_or_else(|err| {
        panic!("failed to read {}: {err}", font_path.display());
    });

    let font = Font::from_bytes(font_bytes, FontSettings::default()).unwrap_or_else(|err| {
        panic!("failed to parse {}: {err}", font_path.display());
    });

    let line_metrics = font
        .horizontal_line_metrics(FONT_SIZE)
        .expect("font does not expose horizontal line metrics");

    let ascender = line_metrics.ascent.ceil() as i16;
    let descender = line_metrics.descent.ceil() as i16;
    let line_height = line_metrics.new_line_size.ceil().max(1.0) as u16;
    let baseline_offset = f32::from(ascender);

    let glyph_top = |m: &fontdue::Metrics| {
        (baseline_offset + (-m.bounds.height - m.bounds.ymin).floor()) as i16
    };

    let mut bitmaps = Vec::new();
    let mut glyphs = Vec::new();
    let mut intervals = Vec::new();
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

        let (metrics, bitmap) = font.rasterize(ch, FONT_SIZE);
        let packed = pack_bitmap(&bitmap, metrics.width, metrics.height);
        let data_offset = bitmaps.len() as u32;
        bitmaps.extend_from_slice(&packed);

        let glyph = GlyphMeta {
            codepoint,
            width: metrics.width as u8,
            height: metrics.height as u8,
            advance_x: metrics.advance_width.round().max(0.0) as u16,
            left: metrics.xmin as i16,
            top: glyph_top(&metrics),
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

    let replacement_index = replacement_index.unwrap_or(0);
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("missing OUT_DIR"));
    let output_path = out_dir.join(OUTPUT_FILE);
    fs::write(
        &output_path,
        render_generated_file(
            &bitmaps,
            &glyphs,
            &intervals,
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
            if bitmap[y * width + x] > 0 {
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
            "    Glyph {{ codepoint: 0x{:X}, width: {}, height: {}, advance_x: {}, left: {}, top: {}, data_offset: {}, data_length: {} }},\n",
            glyph.codepoint,
            glyph.width,
            glyph.height,
            glyph.advance_x,
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

    out.push_str(&format!(
        "pub static BOOKERLY: Font = Font {{ bitmap: BOOKERLY_BITMAPS, glyphs: BOOKERLY_GLYPHS, intervals: BOOKERLY_INTERVALS, ascender: {ascender}, descender: {descender}, line_height: {line_height}, replacement_index: {replacement_index} }};\n"
    ));

    out
}
