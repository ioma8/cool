use std::{fs, path::PathBuf};

use xteink_epub::{Epub, EpubError, EpubEvent, EpubSource, ReaderBuffers};

#[derive(Clone)]
struct MemorySource {
    data: Vec<u8>,
}

impl MemorySource {
    fn new(data: Vec<u8>) -> Self {
        Self { data }
    }
}

impl EpubSource for MemorySource {
    fn len(&self) -> usize {
        self.data.len()
    }

    fn read_at(&self, offset: u64, dst: &mut [u8]) -> Result<usize, EpubError> {
        let offset = match usize::try_from(offset) {
            Ok(v) => v,
            Err(_) => return Ok(0),
        };

        if dst.is_empty() || offset >= self.data.len() {
            return Ok(0);
        }

        let read_len = (self.data.len() - offset).min(dst.len());
        dst[..read_len].copy_from_slice(&self.data[offset..offset + read_len]);
        Ok(read_len)
    }
}

struct Scratch {
    zip_cd: Vec<u8>,
    inflate: Vec<u8>,
    xml: Vec<u8>,
    catalog: Vec<u8>,
    path_buf: Vec<u8>,
}

impl Scratch {
    fn new(zip_cd: usize, inflate: usize, xml: usize, catalog: usize, path_buf: usize) -> Self {
        Self {
            zip_cd: vec![0; zip_cd],
            inflate: vec![0; inflate],
            xml: vec![0; xml],
            catalog: vec![0; catalog],
            path_buf: vec![0; path_buf],
        }
    }

    fn buffers<'a>(&'a mut self) -> ReaderBuffers<'a> {
        ReaderBuffers {
            zip_cd: self.zip_cd.as_mut_slice(),
            inflate: self.inflate.as_mut_slice(),
            xml: self.xml.as_mut_slice(),
            catalog: self.catalog.as_mut_slice(),
            path_buf: self.path_buf.as_mut_slice(),
        }
    }

    fn tiny_out_of_space() -> Self {
        Self::new(64, 64, 64, 8, 32)
    }
}

impl Default for Scratch {
    fn default() -> Self {
        Self::new(16384, 32768, 32768, 2048, 1024)
    }
}

impl Scratch {
    fn large_for_smoke() -> Self {
        Self::new(131072, 1_048_576, 65536, 131072, 2048)
    }
}

#[derive(Debug, PartialEq)]
enum OwnedEvent {
    Text(String),
    ParagraphStart,
    ParagraphEnd,
    HeadingStart(u8),
    HeadingEnd,
    LineBreak,
    Image { src: String, alt: Option<String> },
    UnsupportedTag,
}

fn to_owned_event(event: EpubEvent<'_>) -> OwnedEvent {
    match event {
        EpubEvent::Text(text) => OwnedEvent::Text(text.to_string()),
        EpubEvent::ParagraphStart => OwnedEvent::ParagraphStart,
        EpubEvent::ParagraphEnd => OwnedEvent::ParagraphEnd,
        EpubEvent::HeadingStart(level) => OwnedEvent::HeadingStart(level),
        EpubEvent::HeadingEnd => OwnedEvent::HeadingEnd,
        EpubEvent::LineBreak => OwnedEvent::LineBreak,
        EpubEvent::Image { src, alt } => OwnedEvent::Image {
            src: src.to_string(),
            alt: alt.map(std::string::ToString::to_string),
        },
        EpubEvent::UnsupportedTag => OwnedEvent::UnsupportedTag,
    }
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("test")
        .join("epubs")
}

fn load_fixture(name: &str) -> Vec<u8> {
    let path = fixtures_dir().join(name);
    std::fs::read(path).expect("fixture should be readable")
}

fn fixture_names() -> Vec<String> {
    let mut names = fs::read_dir(fixtures_dir())
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let path = entry.path();
            let is_epub = path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("epub"));
            if is_epub {
                entry.file_name().into_string().ok()
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    names.sort();
    names
}

fn collect_events(data: Vec<u8>, scratch: &mut Scratch) -> Result<Vec<OwnedEvent>, EpubError> {
    let source = MemorySource::new(data);
    let mut epub = Epub::open(source)?;
    let mut out = Vec::new();

    loop {
        let event = epub.next_event(scratch.buffers())?;
        match event {
            Some(event) => out.push(to_owned_event(event)),
            None => break,
        }
    }

    Ok(out)
}

fn collect_level1_headings(events: &[OwnedEvent]) -> Vec<String> {
    let mut headings = Vec::new();
    let mut inside_heading = false;
    let mut heading_level = 0u8;
    let mut current = String::new();

    for event in events {
        match event {
            OwnedEvent::HeadingStart(level) => {
                heading_level = *level;
                inside_heading = true;
                current.clear();
            }
            OwnedEvent::Text(text) if inside_heading => current.push_str(text),
            OwnedEvent::LineBreak if inside_heading => current.push(' '),
            OwnedEvent::HeadingEnd if inside_heading => {
                if heading_level == 1 {
                    headings.push(current.trim().to_string());
                }
                inside_heading = false;
                heading_level = 0;
                current.clear();
            }
            _ => {}
        }
    }

    headings
}

fn assert_contains_all(haystack: &str, needles: &[&str]) {
    for needle in needles {
        assert!(
            haystack.contains(needle),
            "expected `{haystack}` to contain `{needle}`"
        );
    }
}

fn replace_all_exact(src: &mut [u8], pattern: &[u8], replacement: &[u8]) {
    assert_eq!(pattern.len(), replacement.len(), "mutated fixture keeps length constant");
    let mut i = 0usize;
    while i + pattern.len() <= src.len() {
        if &src[i..i + pattern.len()] == pattern {
            src[i..i + pattern.len()].copy_from_slice(replacement);
            i += pattern.len();
        } else {
            i += 1;
        }
    }
}

fn with_missing_container_entry(mut data: Vec<u8>) -> Vec<u8> {
    replace_all_exact(
        data.as_mut_slice(),
        b"META-INF/container.xml",
        b"META-INF/container.bad",
    );
    data
}

fn with_missing_opf_reference(mut data: Vec<u8>) -> Vec<u8> {
    replace_all_exact(data.as_mut_slice(), b"/content.opf", b"/content.bad");
    data
}

#[test]
fn container_discovery_and_opf_parsing_respect_sample_ordering() {
    let fixtures = [
        ("test_display_none.epub", ["CSS display:none Test EPUB", "Test: Class Selector"]),
        ("test_png_images.epub", ["PNG Image Tests", "PNG Format Test"]),
        ("test_jpeg_images.epub", ["JPEG Image Tests", "JPEG Format Test"]),
        ("test_mixed_images.epub", ["Mixed Image Format Tests", "JPEG in Mixed EPUB"]),
        ("test_tables.epub", ["Tables? In CrossPoint?", "Some small tables"]),
        ("test_kerning_ligature.epub", ["Chapter 1", "Chapter 2"]),
    ];

    for (fixture, expected) in &fixtures {
        let mut scratch = Scratch::default();
        let events = collect_events(load_fixture(fixture), &mut scratch).unwrap();
        let headings = collect_level1_headings(&events);

        assert!(
            headings.len() >= 2,
            "expected at least two level-1 headings for {fixture}"
        );

        for (idx, expected_substrings) in expected.iter().enumerate() {
            let heading = headings.get(idx).unwrap();
            assert_contains_all(heading, &[expected_substrings]);
        }
    }
}

#[test]
fn heading_markup_and_line_breaks_are_preserved() {
    let mut scratch = Scratch::default();
    let events = collect_events(load_fixture("test_kerning_ligature.epub"), &mut scratch).unwrap();

    let headings = collect_level1_headings(&events);
    assert_contains_all(&headings[0], &["Chapter 1", "Typographer"]);

    let saw_line_break_in_title = events
        .iter()
        .take(30)
        .any(|event| matches!(event, OwnedEvent::LineBreak));
    assert!(
        saw_line_break_in_title,
        "expected <br/> in first chapter title to become LineBreak"
    );
}

#[test]
fn chapter_text_is_extracted_from_real_fixture_content() {
    let mut scratch = Scratch::default();
    let events = collect_events(load_fixture("test_kerning_ligature.epub"), &mut scratch).unwrap();

    let full_text = events
        .iter()
        .filter_map(|event| match event {
            OwnedEvent::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ");

    assert_contains_all(
        &full_text,
        &[
            "Chapter 1",
            "The Typographer",
            "AVERY WATT always wanted to be a typographer.",
            "Watt &Yardley, Fine Typography",
        ],
    );
}

#[test]
fn image_events_include_src_and_alt() {
    let mut scratch = Scratch::default();
    let events = collect_events(load_fixture("test_png_images.epub"), &mut scratch).unwrap();

    assert!(events.iter().any(|event| match event {
        OwnedEvent::Image { src, alt } => {
            src.ends_with("images/png_format.png")
                && alt.as_deref() == Some("PNG format test")
        }
        _ => false,
    }));
}

#[test]
fn table_markup_yields_unsupported_tag_events() {
    let mut scratch = Scratch::default();
    let events = collect_events(load_fixture("test_tables.epub"), &mut scratch).unwrap();

    assert!(
        events.iter().any(|event| matches!(event, OwnedEvent::UnsupportedTag)),
        "expected tables to generate UnsupportedTag events"
    );
}

#[test]
fn malformed_or_missing_epub_inputs_are_rejected() {
    let mut scratch = Scratch::default();

    assert!(
        collect_events(vec![0x50, 0x4B, 0x03], &mut scratch).is_err(),
        "short/non-ZIP payload should be rejected"
    );

    let mut truncated = load_fixture("test_png_images.epub");
    truncated.truncate(truncated.len() / 8);
    assert!(collect_events(truncated, &mut scratch).is_err(), "truncated payload should be rejected");

    let missing_container = with_missing_container_entry(load_fixture("test_png_images.epub"));
    assert!(collect_events(missing_container, &mut scratch).is_err());

    let missing_content = with_missing_opf_reference(load_fixture("test_png_images.epub"));
    assert!(collect_events(missing_content, &mut scratch).is_err());
}

#[test]
fn out_of_space_is_reported() {
    let data = load_fixture("test_png_images.epub");
    let mut tiny = Scratch::tiny_out_of_space();
    let result = collect_events(data, &mut tiny);

    assert!(
        matches!(result, Err(EpubError::OutOfSpace)),
        "expected tiny buffers to cause OutOfSpace"
    );
}

#[test]
fn every_epub_fixture_parses_successfully() {
    let names = fixture_names();
    assert!(!names.is_empty(), "expected epub fixtures in test/epubs");

    for name in names {
        let mut scratch = Scratch::large_for_smoke();
        let events = collect_events(load_fixture(&name), &mut scratch)
            .unwrap_or_else(|err| panic!("failed to parse {name}: {err:?}"));
        assert!(
            !events.is_empty(),
            "expected at least one event from fixture {name}"
        );
    }
}
