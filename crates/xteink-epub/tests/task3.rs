use std::path::PathBuf;
use std::time::Instant;

use miniz_oxide::{DataFormat, inflate::stream::InflateState};
use xteink_epub::{
    Epub, EpubArchive, EpubError, EpubEvent, EpubSource, MAX_ARCHIVE_ENTRIES,
    MAX_ARCHIVE_NAME_CAPACITY, ReaderBuffers,
};

mod reference_text;
mod reference_text_1024;

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
    stream_input: Vec<u8>,
    xml: Vec<u8>,
    catalog: Vec<u8>,
    path_buf: Vec<u8>,
    stream_state: InflateState,
    archive: EpubArchive<MAX_ARCHIVE_ENTRIES, MAX_ARCHIVE_NAME_CAPACITY>,
}

impl Scratch {
    fn new(
        zip_cd: usize,
        inflate: usize,
        stream_input: usize,
        xml: usize,
        catalog: usize,
        path_buf: usize,
    ) -> Self {
        Self {
            zip_cd: vec![0; zip_cd],
            inflate: vec![0; inflate],
            stream_input: vec![0; stream_input],
            xml: vec![0; xml],
            catalog: vec![0; catalog],
            path_buf: vec![0; path_buf],
            stream_state: InflateState::new(DataFormat::Raw),
            archive: EpubArchive::new(),
        }
    }

    fn buffers<'a>(&'a mut self) -> ReaderBuffers<'a> {
        ReaderBuffers {
            zip_cd: self.zip_cd.as_mut_slice(),
            inflate: self.inflate.as_mut_slice(),
            stream_input: self.stream_input.as_mut_slice(),
            xml: self.xml.as_mut_slice(),
            catalog: self.catalog.as_mut_slice(),
            path_buf: self.path_buf.as_mut_slice(),
            stream_state: &mut self.stream_state,
            archive: &mut self.archive,
        }
    }

    fn tiny_out_of_space() -> Self {
        Self::new(64, 64, 32, 64, 8, 32)
    }
}

impl Default for Scratch {
    fn default() -> Self {
        Self::new(16 * 1024, 48 * 1024, 2048, 32 * 1024, 8192, 256)
    }
}

impl Scratch {
    fn large_for_smoke() -> Self {
        Self::new(131072, 1_048_576, 8192, 65536, 131072, 2048)
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

fn fixtures_dir() -> Option<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("test")
        .join("epubs");
    if dir.exists() {
        Some(dir)
    } else if fixtures_required() {
        panic!(
            "EPUB fixtures required but missing. Set up test/epubs or unset REQUIRE_EPUB_FIXTURES."
        );
    } else {
        None
    }
}

fn fixtures_required() -> bool {
    matches!(
        std::env::var("REQUIRE_EPUB_FIXTURES").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE") | Ok("yes") | Ok("YES")
    )
}

fn load_fixture(name: &str) -> Option<Vec<u8>> {
    let path = fixtures_dir()?.join(name);
    std::fs::read(path).ok()
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

fn collect_events_from_epub(
    epub: &mut Epub<MemorySource>,
    scratch: &mut Scratch,
) -> Result<Vec<OwnedEvent>, EpubError> {
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

fn collect_readable_prefix(
    data: Vec<u8>,
    scratch: &mut Scratch,
    normalized_prefix_len: usize,
) -> Result<String, EpubError> {
    let source = MemorySource::new(data);
    let mut epub = Epub::open(source)?;
    let mut out = String::new();
    let mut pending_space = false;
    let mut pending_initial = false;

    loop {
        match epub.next_event(scratch.buffers())? {
            Some(event) => match to_owned_event(event) {
                OwnedEvent::Text(text) => {
                    push_readable_segment(
                        &mut out,
                        &text,
                        &mut pending_space,
                        &mut pending_initial,
                    );
                }
                OwnedEvent::ParagraphStart
                | OwnedEvent::ParagraphEnd
                | OwnedEvent::HeadingStart(_)
                | OwnedEvent::HeadingEnd
                | OwnedEvent::LineBreak => {
                    pending_space = !out.is_empty();
                    pending_initial = false;
                }
                OwnedEvent::UnsupportedTag | OwnedEvent::Image { .. } => {}
            },
            None => break,
        }

        if normalize_whitespace(&out).chars().count() >= normalized_prefix_len {
            break;
        }
    }

    Ok(normalize_whitespace(&out))
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

fn push_readable_segment(
    out: &mut String,
    text: &str,
    pending_space: &mut bool,
    pending_initial: &mut bool,
) {
    if fixtures_dir().is_none() {
        return;
    }
    if text.is_empty() {
        return;
    }

    let trimmed = text.trim_start();
    let starts_initial_fragment = matches!(
        trimmed.as_bytes().get(0..2),
        Some([first, b'.']) if first.is_ascii_uppercase()
    );
    let is_single_initial = trimmed.len() == 1 && trimmed.as_bytes()[0].is_ascii_uppercase();
    let ends_with_initial = text
        .split_whitespace()
        .last()
        .is_some_and(|token| token.len() == 1 && token.as_bytes()[0].is_ascii_uppercase());

    let needs_text_boundary = if out.is_empty() {
        false
    } else {
        let prev = out.chars().rev().find(|ch| !ch.is_whitespace());
        let first = text.chars().find(|ch| !ch.is_whitespace());
        matches!(prev, Some(ch) if ch.is_alphanumeric() || matches!(ch, '.' | '!' | '?' | ':' | ';' | ',' | ')' | ']' | '}'))
            && matches!(first, Some(ch) if ch.is_alphanumeric() || matches!(ch, '‘' | '“' | '(' | '[' | '{'))
    };

    let should_join_initial = *pending_initial
        && (is_single_initial || starts_initial_fragment || trimmed.starts_with('.'));

    if (*pending_space || (needs_text_boundary && !should_join_initial))
        && !out.ends_with(' ')
        && !out.is_empty()
    {
        out.push(' ');
    }

    out.push_str(text);
    *pending_space = false;
    *pending_initial = ends_with_initial || should_join_initial;
}

fn prefix_chars(text: &str, len: usize) -> String {
    text.chars().take(len).collect()
}

fn normalize_whitespace(text: &str) -> String {
    text.replace('Â', "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn assert_contains_all(haystack: &str, needles: &[&str]) {
    for needle in needles {
        assert!(
            haystack.contains(needle),
            "expected `{haystack}` to contain `{needle}`"
        );
    }
}

fn find_heading_index(headings: &[String], expected_substrings: &str, start_at: usize) -> usize {
    headings
        .iter()
        .enumerate()
        .skip(start_at)
        .find_map(|(idx, heading)| heading.contains(expected_substrings).then_some(idx))
        .unwrap_or_else(|| panic!("expected heading containing `{expected_substrings}`"))
}

fn replace_all_exact(src: &mut [u8], pattern: &[u8], replacement: &[u8]) {
    assert_eq!(
        pattern.len(),
        replacement.len(),
        "mutated fixture keeps length constant"
    );
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
    if fixtures_dir().is_none() {
        return;
    }
    let fixtures = [
        (
            "test_display_none.epub",
            ["CSS display:none Test EPUB", "Test: Class Selector"],
        ),
        (
            "test_png_images.epub",
            ["PNG Image Tests", "PNG Format Test"],
        ),
        (
            "test_jpeg_images.epub",
            ["JPEG Image Tests", "JPEG Format Test"],
        ),
        (
            "test_mixed_images.epub",
            ["Mixed Image Format Tests", "JPEG in Mixed EPUB"],
        ),
        ("test_tables.epub", ["Some small tables", "Some big tables"]),
        ("test_kerning_ligature.epub", ["Chapter 1", "Chapter 2"]),
    ];

    for (fixture, expected) in &fixtures {
        let mut scratch = Scratch::default();
        let events = collect_events(load_fixture(fixture).expect("fixture"), &mut scratch).unwrap();
        let headings = collect_level1_headings(&events);

        assert!(
            headings.len() >= 2,
            "expected at least two level-1 headings for {fixture}"
        );

        let mut search_from = 0usize;
        for expected_substrings in expected.iter() {
            let idx = find_heading_index(&headings, expected_substrings, search_from);
            let heading = headings.get(idx).unwrap();
            assert_contains_all(heading, &[expected_substrings]);
            search_from = idx + 1;
        }
    }
}

#[test]
fn heading_markup_and_line_breaks_are_preserved() {
    if fixtures_dir().is_none() {
        return;
    }
    let mut scratch = Scratch::default();
    let events = collect_events(
        load_fixture("test_kerning_ligature.epub").expect("fixture"),
        &mut scratch,
    )
    .unwrap();

    let headings = collect_level1_headings(&events);
    let chapter_one = headings
        .iter()
        .find(|heading| heading.contains("Chapter 1"))
        .expect("expected Chapter 1 heading");
    assert_contains_all(chapter_one, &["Chapter 1", "Typographer"]);

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
    if fixtures_dir().is_none() {
        return;
    }
    let mut scratch = Scratch::default();
    let events = collect_events(
        load_fixture("test_kerning_ligature.epub").expect("fixture"),
        &mut scratch,
    )
    .unwrap();

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
            "Watt & Yardley, Fine Typography",
        ],
    );
}

#[test]
fn image_events_include_src_and_alt() {
    if fixtures_dir().is_none() {
        return;
    }
    let mut scratch = Scratch::default();
    let events = collect_events(
        load_fixture("test_png_images.epub").expect("fixture"),
        &mut scratch,
    )
    .unwrap();

    assert!(events.iter().any(|event| match event {
        OwnedEvent::Image { src, alt } => {
            src.ends_with("images/png_format.png") && alt.as_deref() == Some("PNG format test")
        }
        _ => false,
    }));
}

#[test]
fn table_markup_yields_unsupported_tag_events() {
    if fixtures_dir().is_none() {
        return;
    }
    let mut scratch = Scratch::default();
    let events = collect_events(
        load_fixture("test_tables.epub").expect("fixture"),
        &mut scratch,
    )
    .unwrap();

    assert!(
        events
            .iter()
            .any(|event| matches!(event, OwnedEvent::UnsupportedTag)),
        "expected tables to generate UnsupportedTag events"
    );
}

#[test]
fn malformed_or_missing_epub_inputs_are_rejected() {
    if fixtures_dir().is_none() {
        return;
    }
    let mut scratch = Scratch::default();

    assert!(
        collect_events(vec![0x50, 0x4B, 0x03], &mut scratch).is_err(),
        "short/non-ZIP payload should be rejected"
    );

    let mut truncated = load_fixture("test_png_images.epub").expect("fixture");
    truncated.truncate(truncated.len() / 8);
    assert!(
        collect_events(truncated, &mut scratch).is_err(),
        "truncated payload should be rejected"
    );

    let missing_container =
        with_missing_container_entry(load_fixture("test_png_images.epub").expect("fixture"));
    assert!(collect_events(missing_container, &mut scratch).is_err());

    let missing_content =
        with_missing_opf_reference(load_fixture("test_png_images.epub").expect("fixture"));
    assert!(collect_events(missing_content, &mut scratch).is_err());
}

#[test]
fn out_of_space_is_reported() {
    if fixtures_dir().is_none() {
        return;
    }
    let data = load_fixture("test_png_images.epub").expect("fixture");
    let mut tiny = Scratch::tiny_out_of_space();
    let result = collect_events(data, &mut tiny);

    assert!(
        matches!(result, Err(EpubError::OutOfSpace)),
        "expected tiny buffers to cause OutOfSpace"
    );
}

#[test]
fn every_epub_fixture_matches_reference_text_prefix() {
    if fixtures_dir().is_none() {
        return;
    }
    assert!(
        !reference_text::EPUB_REFERENCE_CASES.is_empty(),
        "expected pandoc-handled epub fixtures"
    );

    for (name, expected) in reference_text::EPUB_REFERENCE_CASES {
        let name = *name;

        let mut scratch = Scratch::large_for_smoke();
        let actual = collect_readable_prefix(
            load_fixture(name).expect("fixture"),
            &mut scratch,
            expected.chars().count(),
        )
        .unwrap_or_else(|err| panic!("failed to parse {name}: {err:?}"));

        assert!(
            actual.starts_with(expected),
            "fixture {name} did not match reference text prefix\nactual: {actual}\nexpected: {expected}"
        );
    }
}

#[test]
fn zero_to_production_prefix_matches_reference_text() {
    if fixtures_dir().is_none() {
        return;
    }
    let name = "Zero To Production In Rust - Luca Palmieri.epub";
    let expected = reference_text::EPUB_REFERENCE_CASES
        .iter()
        .find_map(|(fixture, expected)| (*fixture == name).then_some(*expected))
        .expect("expected Zero To Production reference text");

    let mut scratch = Scratch::large_for_smoke();
    let actual = collect_readable_prefix(
        load_fixture(name).expect("fixture"),
        &mut scratch,
        expected.chars().count(),
    )
    .unwrap_or_else(|err| panic!("failed to parse {name}: {err:?}"));

    assert!(
        actual.starts_with(expected),
        "fixture {name} did not match reference text prefix\nactual: {actual}\nexpected: {expected}"
    );
}

#[test]
fn every_epub_fixture_matches_reference_text_1024_prefix() {
    if fixtures_dir().is_none() {
        return;
    }
    assert!(
        !reference_text_1024::EPUB_REFERENCE_CASES_1024.is_empty(),
        "expected pandoc-handled epub fixtures"
    );

    for (name, expected) in reference_text_1024::EPUB_REFERENCE_CASES_1024 {
        let name = *name;

        let mut scratch = Scratch::large_for_smoke();
        let actual = prefix_chars(
            &collect_readable_prefix(load_fixture(name).expect("fixture"), &mut scratch, 1024)
                .unwrap_or_else(|err| panic!("failed to parse {name}: {err:?}")),
            1024,
        );

        assert_eq!(
            actual, *expected,
            "fixture {name} did not match reference text 1024-char prefix"
        );
    }
}

#[test]
fn resume_from_spine_index_allows_continued_parsing() {
    if fixtures_dir().is_none() {
        return;
    }
    let data = load_fixture("Happiness Trap Pocketbook, The - Russ Harris.epub").expect("fixture");
    let mut resumed = Epub::open(MemorySource::new(data)).expect("fixture should reopen");
    let mut resume_scratch = Scratch::default();
    resumed
        .resume_from_spine_index(resume_scratch.buffers(), 1)
        .expect("resume should prepare parser at a later spine entry");
    let actual = collect_events_from_epub(&mut resumed, &mut resume_scratch)
        .expect("resumed parse should succeed");

    assert!(!actual.is_empty(), "resume should still yield events");
}

#[test]
fn prints_large_fixture_parse_baselines() {
    if fixtures_dir().is_none() {
        return;
    }
    let data = load_fixture("Happiness Trap Pocketbook, The - Russ Harris.epub").expect("fixture");

    let started = Instant::now();
    let mut first_page_scratch = Scratch::default();
    let prefix = collect_readable_prefix(data.clone(), &mut first_page_scratch, 1024)
        .expect("first-page prefix parse should succeed");
    let first_page_elapsed = started.elapsed();

    let started = Instant::now();
    let mut full_scratch = Scratch::default();
    let events = collect_events(data, &mut full_scratch).expect("full parse should succeed");
    let full_parse_elapsed = started.elapsed();

    println!(
        "large fixture baselines: first_prefix={:?} full_parse={:?} events={}",
        first_page_elapsed,
        full_parse_elapsed,
        events.len()
    );
    assert!(!prefix.is_empty());
    assert!(!events.is_empty());
}
