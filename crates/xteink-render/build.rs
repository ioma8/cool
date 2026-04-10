use std::{
    char,
    collections::{BTreeSet, hash_map::DefaultHasher},
    env, fs,
    hash::{Hash, Hasher},
    path::PathBuf,
};

use fontdue::{Font, FontSettings};
use freetype::Library;
use rustybuzz::{Face, UnicodeBuffer};

const BODY_FONT_SIZE: f32 = 30.0;
const HEADING_FONT_SIZE: f32 = 38.0;
const FOOTER_FONT_SIZE: f32 = 24.0;
const REGULAR_FONT_PATH: &str = "../xteink-display/assets/Bookerly-Regular.ttf";
const ITALIC_FONT_PATH: &str = "../xteink-display/assets/Bookerly Italic.ttf";
const OUTPUT_FILE: &str = "bookerly_generated.rs";
const STAMP_FILE: &str = "bookerly_generated.stamp";
const GENERATOR_VERSION: u32 = 1;

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

struct GeneratedFontVariant {
    symbol: &'static str,
    bitmaps: Vec<u8>,
    glyphs: Vec<GlyphMeta>,
    intervals: Vec<IntervalMeta>,
    pair_positioning: Vec<PairPositioningMeta>,
    ascender: i16,
    descender: i16,
    line_height: u16,
    replacement_index: usize,
}

#[derive(Clone, Copy)]
struct SupportedGlyph {
    codepoint: u32,
    layout_advance_units: i32,
}

#[derive(Clone, Copy)]
struct PairPositioningUnits {
    left: u32,
    right: u32,
    left_x_advance: i32,
    right_x_advance: i32,
    x_offset: i32,
    y_offset: i32,
}

struct FontSource {
    font: Font,
    shape_face: Face<'static>,
    supported_glyphs: Vec<SupportedGlyph>,
    pair_positioning_units: Vec<PairPositioningUnits>,
    library: Library,
    font_path: PathBuf,
}

fn main() {
    println!("cargo:rerun-if-changed={REGULAR_FONT_PATH}");
    println!("cargo:rerun-if-changed={ITALIC_FONT_PATH}");

    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("missing CARGO_MANIFEST_DIR"));
    let italic = manifest_dir.join(ITALIC_FONT_PATH);
    let regular_path = manifest_dir.join(REGULAR_FONT_PATH);
    let regular_bytes = fs::read(&regular_path).unwrap_or_else(|err| {
        panic!("failed to read {}: {err}", regular_path.display());
    });
    let italic_bytes = italic.exists().then(|| {
        fs::read(&italic).unwrap_or_else(|err| panic!("failed to read {}: {err}", italic.display()))
    });
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("missing OUT_DIR"));
    let output_path = out_dir.join(OUTPUT_FILE);
    let stamp_path = out_dir.join(STAMP_FILE);
    let stamp = build_stamp(&regular_bytes, italic_bytes.as_deref());
    if stamp_matches(&stamp_path, &stamp) && output_path.exists() {
        return;
    }

    let regular = load_font_source(&regular_path, regular_bytes);
    let italic = italic_bytes.map(|bytes| load_font_source(&italic, bytes));
    let mut variants = vec![
        build_font_variant("BOOKERLY_BODY", BODY_FONT_SIZE, &regular),
        build_font_variant("BOOKERLY_HEADING", HEADING_FONT_SIZE, &regular),
        build_font_variant("BOOKERLY_FOOTER", FOOTER_FONT_SIZE, &regular),
    ];
    if let Some(italic) = italic.as_ref() {
        variants.push(build_font_variant(
            "BOOKERLY_BODY_ITALIC",
            BODY_FONT_SIZE,
            italic,
        ));
        variants.push(build_font_variant(
            "BOOKERLY_HEADING_ITALIC",
            HEADING_FONT_SIZE,
            italic,
        ));
        variants.push(build_font_variant(
            "BOOKERLY_FOOTER_ITALIC",
            FOOTER_FONT_SIZE,
            italic,
        ));
    }
    fs::write(
        &output_path,
        render_generated_file(&variants, italic.is_some()),
    )
    .unwrap_or_else(|err| panic!("failed to write {}: {err}", output_path.display()));
    fs::write(&stamp_path, stamp)
        .unwrap_or_else(|err| panic!("failed to write {}: {err}", stamp_path.display()));
}

fn load_font_source(font_path: &PathBuf, font_bytes: Vec<u8>) -> FontSource {
    let font =
        Font::from_bytes(font_bytes.clone(), FontSettings::default()).unwrap_or_else(|err| {
            panic!("failed to parse {}: {err}", font_path.display());
        });
    let owned_bytes = Box::leak(font_bytes.into_boxed_slice());
    let shape_face = Face::from_slice(owned_bytes, 0).expect("failed to parse shaping face");
    let parser_face =
        rustybuzz::ttf_parser::Face::parse(owned_bytes, 0).expect("failed to parse ttf face");
    let supported_glyphs = collect_supported_glyphs(&shape_face, &parser_face);
    let pair_positioning_units = collect_pair_positioning_units(&shape_face, &supported_glyphs);
    let library = Library::init().expect("failed to init freetype");
    FontSource {
        font,
        shape_face,
        supported_glyphs,
        pair_positioning_units,
        library,
        font_path: font_path.to_path_buf(),
    }
}

fn build_stamp(regular_bytes: &[u8], italic_bytes: Option<&[u8]>) -> String {
    let mut hasher = DefaultHasher::new();
    GENERATOR_VERSION.hash(&mut hasher);
    BODY_FONT_SIZE.to_bits().hash(&mut hasher);
    HEADING_FONT_SIZE.to_bits().hash(&mut hasher);
    FOOTER_FONT_SIZE.to_bits().hash(&mut hasher);
    regular_bytes.hash(&mut hasher);
    italic_bytes.is_some().hash(&mut hasher);
    if let Some(italic_bytes) = italic_bytes {
        italic_bytes.hash(&mut hasher);
    }
    format!("{:016x}\n", hasher.finish())
}

fn stamp_matches(path: &PathBuf, expected: &str) -> bool {
    fs::read_to_string(path)
        .ok()
        .is_some_and(|existing| existing == expected)
}

fn build_font_variant(
    symbol: &'static str,
    font_size: f32,
    source: &FontSource,
) -> GeneratedFontVariant {
    let face = source
        .library
        .new_face(&source.font_path, 0)
        .unwrap_or_else(|err| {
            panic!(
                "failed to load {} in freetype: {err}",
                source.font_path.display()
            )
        });
    face.set_pixel_sizes(0, font_size as u32)
        .expect("failed to set freetype pixel size");

    let line_metrics = source
        .font
        .horizontal_line_metrics(font_size)
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

    for supported_glyph in &source.supported_glyphs {
        let codepoint = supported_glyph.codepoint;
        let ch = char::from_u32(codepoint).expect("supported codepoint must be valid char");

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
            layout_advance_x: scale_font_units(
                supported_glyph.layout_advance_units,
                font_size,
                &source.shape_face,
            ),
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

    for pair_units in &source.pair_positioning_units {
        let Some(left_glyph) = glyphs
            .iter()
            .find(|glyph| glyph.codepoint == pair_units.left)
        else {
            continue;
        };
        let Some(right_glyph) = glyphs
            .iter()
            .find(|glyph| glyph.codepoint == pair_units.right)
        else {
            continue;
        };

        let pen_adjust =
            scale_font_units_signed(pair_units.left_x_advance, font_size, &source.shape_face)
                as i16
                - left_glyph.advance_x as i16;
        let x_offset =
            scale_font_units_signed(pair_units.x_offset, font_size, &source.shape_face) as i16;
        let y_offset =
            -scale_font_units_signed(pair_units.y_offset, font_size, &source.shape_face) as i16;
        let advance_adjust =
            scale_font_units_signed(pair_units.right_x_advance, font_size, &source.shape_face)
                as i16
                - right_glyph.advance_x as i16;

        if pen_adjust == 0 && x_offset == 0 && y_offset == 0 && advance_adjust == 0 {
            continue;
        }

        pair_positioning.push(PairPositioningMeta {
            left: pair_units.left,
            right: pair_units.right,
            pen_adjust,
            x_offset,
            y_offset,
            advance_adjust,
        });
    }

    GeneratedFontVariant {
        symbol,
        bitmaps,
        glyphs,
        intervals,
        pair_positioning,
        ascender,
        descender,
        line_height,
        replacement_index: replacement_index.unwrap_or(0),
    }
}

fn collect_supported_glyphs(
    shape_face: &Face<'_>,
    face: &rustybuzz::ttf_parser::Face<'_>,
) -> Vec<SupportedGlyph> {
    let mut codepoints = BTreeSet::new();
    let Some(cmap) = face.tables().cmap else {
        return Vec::new();
    };

    for subtable in cmap.subtables {
        if !subtable.is_unicode() {
            continue;
        }

        subtable.codepoints(|codepoint| {
            let Some(ch) = char::from_u32(codepoint) else {
                return;
            };
            if face.glyph_index(ch).is_some() {
                codepoints.insert(codepoint);
            }
        });
    }

    codepoints
        .into_iter()
        .map(|codepoint| SupportedGlyph {
            codepoint,
            layout_advance_units: shape_single_advance_units(
                shape_face,
                char::from_u32(codepoint).expect("supported codepoint must be valid char"),
            ),
        })
        .collect()
}

fn pack_bitmap(bitmap: &[u8], width: usize, height: usize) -> Vec<u8> {
    let row_bytes = width.div_ceil(4);
    let mut packed = vec![0u8; row_bytes * height];

    for y in 0..height {
        for x in 0..width {
            let shade = quantize_coverage(bitmap[y * width + x]);
            let index = y * row_bytes + x / 4;
            let shift = 6 - ((x % 4) * 2);
            packed[index] |= shade << shift;
        }
    }

    packed
}

fn quantize_coverage(value: u8) -> u8 {
    match value {
        0..=63 => 0,
        64..=127 => 1,
        128..=191 => 2,
        _ => 3,
    }
}

fn render_generated_file(variants: &[GeneratedFontVariant], has_real_italic: bool) -> String {
    let mut out = String::new();
    for variant in variants {
        out.push_str(&format!(
            "pub static {}_BITMAPS: &[u8] = &[\n",
            variant.symbol
        ));
        for chunk in variant.bitmaps.chunks(16) {
            out.push_str("    ");
            for byte in chunk {
                out.push_str(&format!("0x{byte:02X}, "));
            }
            out.push('\n');
        }
        out.push_str("];\n\n");

        out.push_str(&format!(
            "pub static {}_GLYPHS: &[Glyph] = &[\n",
            variant.symbol
        ));
        for glyph in &variant.glyphs {
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

        out.push_str(&format!(
            "pub static {}_INTERVALS: &[Interval] = &[\n",
            variant.symbol
        ));
        for interval in &variant.intervals {
            out.push_str(&format!(
                "    Interval {{ first: 0x{:X}, last: 0x{:X}, offset: {} }},\n",
                interval.first, interval.last, interval.offset
            ));
        }
        out.push_str("];\n\n");

        out.push_str(&format!(
            "pub static {}_PAIR_POSITIONING: &[PairPositioning] = &[\n",
            variant.symbol
        ));
        for pair in &variant.pair_positioning {
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
            "pub static {}: Font = Font {{ bitmap: {}_BITMAPS, glyphs: {}_GLYPHS, intervals: {}_INTERVALS, pair_positioning: {}_PAIR_POSITIONING, ascender: {}, descender: {}, line_height: {}, replacement_index: {} }};\n\n",
            variant.symbol,
            variant.symbol,
            variant.symbol,
            variant.symbol,
            variant.symbol,
            variant.ascender,
            variant.descender,
            variant.line_height,
            variant.replacement_index
        ));
    }
    if !has_real_italic {
        out.push_str("pub static BOOKERLY_BODY_ITALIC: Font = BOOKERLY_BODY;\n");
        out.push_str("pub static BOOKERLY_HEADING_ITALIC: Font = BOOKERLY_HEADING;\n");
        out.push_str("pub static BOOKERLY_FOOTER_ITALIC: Font = BOOKERLY_FOOTER;\n");
    }
    out.push_str(&format!(
        "pub const BOOKERLY_HAS_REAL_ITALIC: bool = {};\n",
        if has_real_italic { "true" } else { "false" }
    ));
    out.push_str("pub static BOOKERLY: Font = BOOKERLY_BODY;\n");

    out
}

fn collect_pair_positioning_units(
    shape_face: &Face<'_>,
    supported_glyphs: &[SupportedGlyph],
) -> Vec<PairPositioningUnits> {
    let pair_candidates = supported_glyphs
        .iter()
        .copied()
        .filter(|glyph| supports_pair_positioning(glyph.codepoint))
        .collect::<Vec<_>>();
    let mut pairs = Vec::new();

    for left_glyph in &pair_candidates {
        let left_char = char::from_u32(left_glyph.codepoint).expect("pair codepoint must be char");
        for right_glyph in &pair_candidates {
            let right_char =
                char::from_u32(right_glyph.codepoint).expect("pair codepoint must be char");
            let Some(pair) = shape_pair_positioning_units(
                left_glyph.codepoint,
                right_glyph.codepoint,
                shape_face,
                left_char,
                right_char,
            ) else {
                continue;
            };
            pairs.push(pair);
        }
    }

    pairs
}

fn shape_single_advance_units(face: &Face<'_>, ch: char) -> i32 {
    let mut text = String::new();
    text.push(ch);

    let features = shaping_features();
    let mut buffer = UnicodeBuffer::new();
    buffer.push_str(&text);
    let output = rustybuzz::shape(face, &features, buffer);
    let Some(position) = output.glyph_positions().first() else {
        return 0;
    };

    position.x_advance
}

fn shape_pair_positioning_units(
    left_codepoint: u32,
    right_codepoint: u32,
    face: &Face<'_>,
    left_char: char,
    right_char: char,
) -> Option<PairPositioningUnits> {
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

    if positions[0].x_advance == 0
        && positions[1].x_advance == 0
        && positions[1].x_offset == 0
        && positions[1].y_offset == 0
    {
        return None;
    }

    Some(PairPositioningUnits {
        left: left_codepoint,
        right: right_codepoint,
        left_x_advance: positions[0].x_advance,
        right_x_advance: positions[1].x_advance,
        x_offset: positions[1].x_offset,
        y_offset: positions[1].y_offset,
    })
}

fn scale_font_units(value: i32, font_size: f32, face: &Face<'_>) -> u16 {
    scale_font_units_signed(value, font_size, face).max(0) as u16
}

fn scale_font_units_signed(value: i32, font_size: f32, face: &Face<'_>) -> i32 {
    (value as f32 * (font_size / face.units_per_em() as f32)).round() as i32
}

fn supports_pair_positioning(codepoint: u32) -> bool {
    matches!(codepoint, 0x0020..=0x017F | 0x2010..=0x201F | 0x2026 | 0x20AC)
}
