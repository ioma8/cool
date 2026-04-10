use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;
use xteink_render::{Framebuffer, bookerly};

static EPUB_RENDER_TEST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock_render_mutex() -> std::sync::MutexGuard<'static, ()> {
    EPUB_RENDER_TEST_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

struct EmptySource;

impl xteink_epub::EpubSource for EmptySource {
    fn len(&self) -> usize {
        0
    }

    fn read_at(&self, _offset: u64, _buffer: &mut [u8]) -> Result<usize, xteink_epub::EpubError> {
        Ok(0)
    }
}

struct VecSource {
    bytes: Vec<u8>,
}

impl xteink_epub::EpubSource for VecSource {
    fn len(&self) -> usize {
        self.bytes.len()
    }

    fn read_at(&self, offset: u64, buffer: &mut [u8]) -> Result<usize, xteink_epub::EpubError> {
        let start = offset as usize;
        if start >= self.bytes.len() {
            return Ok(0);
        }
        let end = (start + buffer.len()).min(self.bytes.len());
        let chunk = &self.bytes[start..end];
        buffer[..chunk.len()].copy_from_slice(chunk);
        Ok(chunk.len())
    }
}

struct CountingSource {
    bytes: Vec<u8>,
    reads: AtomicUsize,
    bytes_read: AtomicUsize,
}

impl CountingSource {
    fn new(bytes: Vec<u8>) -> Self {
        Self {
            bytes,
            reads: AtomicUsize::new(0),
            bytes_read: AtomicUsize::new(0),
        }
    }

    fn counts(&self) -> (usize, usize) {
        (
            self.reads.load(Ordering::Relaxed),
            self.bytes_read.load(Ordering::Relaxed),
        )
    }
}

impl xteink_epub::EpubSource for CountingSource {
    fn len(&self) -> usize {
        self.bytes.len()
    }

    fn read_at(&self, offset: u64, buffer: &mut [u8]) -> Result<usize, xteink_epub::EpubError> {
        let start = offset as usize;
        if start >= self.bytes.len() {
            return Ok(0);
        }
        let end = (start + buffer.len()).min(self.bytes.len());
        let chunk = &self.bytes[start..end];
        buffer[..chunk.len()].copy_from_slice(chunk);
        self.reads.fetch_add(1, Ordering::Relaxed);
        self.bytes_read.fetch_add(chunk.len(), Ordering::Relaxed);
        Ok(chunk.len())
    }
}

impl xteink_epub::EpubSource for &CountingSource {
    fn len(&self) -> usize {
        self.bytes.len()
    }

    fn read_at(&self, offset: u64, buffer: &mut [u8]) -> Result<usize, xteink_epub::EpubError> {
        (*self).read_at(offset, buffer)
    }
}

#[test]
fn cached_text_pagination_advances_to_requested_page() {
    let _guard = lock_render_mutex();
    let mut framebuffer = Framebuffer::new();
    let paragraph = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu\n";
    let text = paragraph.repeat(80);
    let mut offset = 0usize;

    let rendered_page = framebuffer
        .render_cached_text_page(
            &mut |buffer| {
                if offset >= text.len() {
                    return Ok(0);
                }
                let end = (offset + buffer.len()).min(text.len());
                let chunk = &text.as_bytes()[offset..end];
                buffer[..chunk.len()].copy_from_slice(chunk);
                offset = end;
                Ok(chunk.len())
            },
            1,
        )
        .expect("cached render should succeed");

    assert_eq!(rendered_page, 1);
    assert!(framebuffer.bytes().iter().any(|byte| *byte != 0xFF));
    assert!(bookerly::BOOKERLY.line_height_px() > 0);
}

#[test]
fn cached_render_from_offset_matches_page_api() {
    let _guard = lock_render_mutex();
    let paragraph = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu ";
    let text = paragraph.repeat(180);
    let bytes = text.as_bytes().to_vec();

    let mut direct0 = Framebuffer::new();
    let mut direct0_offset = 0usize;
    let direct0_rendered = direct0
        .render_cached_text_page(
            &mut |buffer| {
                if direct0_offset >= bytes.len() {
                    return Ok(0);
                }
                let end = (direct0_offset + buffer.len()).min(bytes.len());
                let chunk = &bytes[direct0_offset..end];
                buffer[..chunk.len()].copy_from_slice(chunk);
                direct0_offset = end;
                Ok(chunk.len())
            },
            0,
        )
        .expect("direct cached render should succeed");
    assert_eq!(direct0_rendered, 0);

    let mut direct1 = Framebuffer::new();
    let mut direct1_offset = 0usize;
    let direct1_rendered = direct1
        .render_cached_text_page(
            &mut |buffer| {
                if direct1_offset >= bytes.len() {
                    return Ok(0);
                }
                let end = (direct1_offset + buffer.len()).min(bytes.len());
                let chunk = &bytes[direct1_offset..end];
                buffer[..chunk.len()].copy_from_slice(chunk);
                direct1_offset = end;
                Ok(chunk.len())
            },
            1,
        )
        .expect("direct cached page 1 render should succeed");
    assert_eq!(direct1_rendered, 1);

    let mut page0_offset = 0usize;
    let mut offset_reader0 = |buffer: &mut [u8]| -> Result<usize, xteink_epub::EpubError> {
        if page0_offset >= bytes.len() {
            return Ok(0);
        }
        let end = (page0_offset + buffer.len()).min(bytes.len());
        let chunk = &bytes[page0_offset..end];
        buffer[..chunk.len()].copy_from_slice(chunk);
        page0_offset = end;
        Ok(chunk.len())
    };
    let mut framebuffer0 = Framebuffer::new();
    let page0 = framebuffer0
        .render_cached_text_page_from_offset_with_progress(&mut offset_reader0, 0usize, || false)
        .expect("offset cached render should succeed");
    assert_eq!(page0.rendered_page, 0);
    assert!(page0.next_page_start_byte > 0);
    assert!(page0.consumed_bytes > 0);
    assert_eq!(framebuffer0.bytes(), direct0.bytes());

    let mut page1_offset = 0usize;
    let mut offset_reader1 = move |buffer: &mut [u8]| -> Result<usize, xteink_epub::EpubError> {
        if page1_offset >= bytes.len() {
            return Ok(0);
        }
        let end = (page1_offset + buffer.len()).min(bytes.len());
        let chunk = &bytes[page1_offset..end];
        buffer[..chunk.len()].copy_from_slice(chunk);
        page1_offset = end;
        Ok(chunk.len())
    };
    let mut second_page = Framebuffer::new();
    let page1 = second_page
        .render_cached_text_page_from_offset_with_progress(
            &mut offset_reader1,
            page0.next_page_start_byte,
            || false,
        )
        .expect("second page from offset should succeed");
    assert_eq!(page1.rendered_page, 0);
    assert!(page1.next_page_start_byte > page0.next_page_start_byte);
    assert!(
        second_page.bytes().iter().any(|byte| *byte != 0xFF),
        "offset render should produce visible content"
    );
}

#[test]
fn epub_render_api_returns_error_for_invalid_empty_source() {
    let _guard = lock_render_mutex();
    let mut framebuffer = Framebuffer::new();

    let result = framebuffer.render_epub_page(EmptySource, 0);

    assert!(matches!(result, Err(xteink_epub::EpubError::InvalidFormat)));
}

#[test]
fn real_fixture_first_page_does_not_fail_with_out_of_space() {
    let _guard = lock_render_mutex();
    let Some(fixture_root) = fixture_dir() else {
        return;
    };
    let fixture = fixture_root.join("Decisive - Chip Heath.epub");
    let bytes = std::fs::read(fixture).expect("fixture should be readable");
    let mut framebuffer = Framebuffer::new();

    let result = framebuffer.render_epub_page(VecSource { bytes }, 0);

    assert!(!matches!(result, Err(xteink_epub::EpubError::OutOfSpace)));
    assert_eq!(result.expect("render should succeed"), 0);
    assert!(
        framebuffer.bytes().iter().any(|byte| *byte != 0xFF),
        "first page should not be blank"
    );
}

fn fixture_dir() -> Option<std::path::PathBuf> {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
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

fn epub_fixture_paths() -> Vec<std::path::PathBuf> {
    let Some(dir) = fixture_dir() else {
        return Vec::new();
    };
    let mut fixtures = std::fs::read_dir(dir)
        .expect("fixture directory should be readable")
        .map(|entry| entry.expect("fixture entry should be readable").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("epub"))
        })
        .collect::<Vec<_>>();
    fixtures.sort();
    fixtures
}

#[test]
fn all_epub_fixtures_first_page_render_without_out_of_space() {
    let _guard = lock_render_mutex();
    let fixtures = epub_fixture_paths();
    if fixtures.is_empty() {
        assert!(
            !fixtures_required(),
            "REQUIRE_EPUB_FIXTURES is set but no .epub files were found under test/epubs"
        );
        return;
    }

    for fixture in fixtures {
        let name = fixture
            .file_name()
            .and_then(|name| name.to_str())
            .expect("fixture file name should be valid utf-8")
            .to_owned();
        let bytes = std::fs::read(&fixture).expect("fixture should be readable");
        let mut framebuffer = Framebuffer::new();
        let result = framebuffer.render_epub_page(VecSource { bytes }, 0);

        assert!(
            !matches!(result, Err(xteink_epub::EpubError::OutOfSpace)),
            "fixture should not exhaust workspace on first-page render: {name}, got {result:?}"
        );
    }
}

#[test]
fn large_fixture_cold_parse_to_cache_without_out_of_space() {
    let _guard = lock_render_mutex();
    let Some(fixture_root) = fixture_dir() else {
        return;
    };
    let fixture = fixture_root.join("Happiness Trap Pocketbook, The - Russ Harris.epub");
    let bytes = std::fs::read(&fixture).expect("fixture should be readable");
    let mut framebuffer = Framebuffer::new();
    let mut emitted_text_bytes = 0usize;

    let result = framebuffer.render_epub_page_with_text_sink_and_cancel(
        VecSource { bytes },
        0,
        |chunk| {
            emitted_text_bytes = emitted_text_bytes.saturating_add(chunk.len());
            Ok(())
        },
        true,
        || false,
    );

    assert!(!matches!(result, Err(xteink_epub::EpubError::OutOfSpace)));
    assert!(
        result.is_ok(),
        "large fixture cold parse should succeed: {result:?}"
    );
    assert!(emitted_text_bytes > 0);
}

#[test]
fn full_cache_build_matches_full_book_parse_for_large_fixture() {
    let _guard = lock_render_mutex();
    let Some(fixture_root) = fixture_dir() else {
        return;
    };
    let fixture = fixture_root.join("Happiness Trap Pocketbook, The - Russ Harris.epub");
    let bytes = std::fs::read(&fixture).expect("fixture should be readable");
    let mut direct = Framebuffer::new();
    let mut full_parse = Vec::<u8>::new();
    let direct_result = direct.render_epub_page_with_text_sink_and_cancel(
        VecSource {
            bytes: bytes.clone(),
        },
        0,
        |chunk| {
            full_parse.extend_from_slice(chunk.as_bytes());
            Ok(())
        },
        true,
        || false,
    );
    assert_eq!(direct_result.expect("full parse should succeed"), 0);

    let mut framebuffer = Framebuffer::new();
    let mut cached_text = Vec::<u8>::new();
    let result = framebuffer.build_epub_cache_prefix_with_text_sink_and_cancel(
        VecSource { bytes },
        0,
        |chunk| {
            cached_text.extend_from_slice(chunk.as_bytes());
            Ok(())
        },
        || false,
    );

    let result = result.expect("cache prefix build should succeed");
    assert_eq!(result.rendered_page, 0);
    assert!(result.complete, "cache build should linearize the full book");
    assert_eq!(cached_text, full_parse);
}

#[test]
fn prints_cold_first_page_baselines_for_large_fixture() {
    let _guard = lock_render_mutex();
    let Some(fixture_root) = fixture_dir() else {
        return;
    };
    let fixture = fixture_root.join("Happiness Trap Pocketbook, The - Russ Harris.epub");
    let bytes = std::fs::read(&fixture).expect("fixture should be readable");

    let mut direct = Framebuffer::new();
    let direct_start = Instant::now();
    let direct_result = direct.render_epub_page(
        VecSource {
            bytes: bytes.clone(),
        },
        0,
    );
    let direct_elapsed = direct_start.elapsed();
    assert_eq!(direct_result.expect("direct render should succeed"), 0);

    let mut cached = Framebuffer::new();
    let mut emitted_text_bytes = 0usize;
    let cached_start = Instant::now();
    let cached_result = cached.build_epub_cache_prefix_with_text_sink_and_cancel(
        VecSource { bytes },
        0,
        |chunk| {
            emitted_text_bytes = emitted_text_bytes.saturating_add(chunk.len());
            Ok(())
        },
        || false,
    );
    let cached_elapsed = cached_start.elapsed();
    assert_eq!(
        cached_result
            .expect("cache prefix build should succeed")
            .rendered_page,
        0
    );
    assert!(emitted_text_bytes > 0);

    eprintln!(
        "cold first-page baselines: direct={:?} cached_prefix={:?} emitted_text_bytes={}",
        direct_elapsed, cached_elapsed, emitted_text_bytes
    );
}

#[test]
fn prints_cold_first_page_io_profile_for_large_fixture() {
    let _guard = lock_render_mutex();
    let Some(fixture_root) = fixture_dir() else {
        return;
    };
    let fixture = fixture_root.join("Happiness Trap Pocketbook, The - Russ Harris.epub");
    let source = CountingSource::new(std::fs::read(&fixture).expect("fixture should be readable"));
    let mut framebuffer = Framebuffer::new();

    let result = framebuffer.render_epub_page(&source, 0);
    assert_eq!(result.expect("render should succeed"), 0);
    let (reads, bytes_read) = source.counts();
    eprintln!(
        "cold first-page io profile: reads={} bytes_read={}",
        reads, bytes_read
    );
}

#[test]
fn cached_page_matches_direct_epub_render_for_following_page() {
    let _guard = lock_render_mutex();
    let Some(fixture_root) = fixture_dir() else {
        return;
    };
    let fixture = fixture_root.join("Decisive - Chip Heath.epub");
    let bytes = std::fs::read(&fixture).expect("fixture should be readable");
    let target_page = 1usize;

    let mut direct = Framebuffer::new();
    let direct_result = direct.render_epub_page(
        VecSource {
            bytes: bytes.clone(),
        },
        target_page,
    );
    assert_eq!(
        direct_result.expect("direct render should succeed"),
        target_page
    );
    assert!(
        direct.bytes().iter().any(|byte| *byte != 0xFF),
        "direct target page should not be blank"
    );

    let mut cached_text = Vec::<u8>::new();
    let mut builder = Framebuffer::new();
    let build_result = builder.render_epub_page_with_text_sink_and_cancel(
        VecSource { bytes },
        target_page,
        |chunk| {
            cached_text.extend_from_slice(chunk.as_bytes());
            Ok(())
        },
        true,
        || false,
    );
    assert_eq!(
        build_result.expect("cache build should succeed"),
        target_page
    );
    assert!(
        !cached_text.is_empty(),
        "expected cached text to be emitted"
    );

    let mut cached = Framebuffer::new();
    let mut offset = 0usize;
    let cached_result = cached.render_cached_text_page(
        &mut |buffer| {
            if offset >= cached_text.len() {
                return Ok(0);
            }
            let end = (offset + buffer.len()).min(cached_text.len());
            let chunk = &cached_text[offset..end];
            buffer[..chunk.len()].copy_from_slice(chunk);
            offset = end;
            Ok(chunk.len())
        },
        target_page,
    );
    assert_eq!(
        cached_result.expect("cached render should succeed"),
        target_page
    );
    assert!(
        cached.bytes().iter().any(|byte| *byte != 0xFF),
        "cached target page should not be blank"
    );
    if cached.bytes() != direct.bytes() {
        let first_diff = cached
            .bytes()
            .iter()
            .zip(direct.bytes().iter())
            .position(|(left, right)| left != right)
            .unwrap_or(0);
        let cached_nonwhite = cached.bytes().iter().filter(|&&b| b != 0xFF).count();
        let direct_nonwhite = direct.bytes().iter().filter(|&&b| b != 0xFF).count();
        panic!(
            "cached page rendering must match direct EPUB rendering for the same page: first_diff_byte={} cached_nonwhite={} direct_nonwhite={}",
            first_diff, cached_nonwhite, direct_nonwhite,
        );
    }
    assert_eq!(
        cached.bytes(),
        direct.bytes(),
        "cached page rendering must match direct EPUB rendering for the same page"
    );
}

#[test]
fn cold_open_prefix_render_matches_direct_first_page() {
    let _guard = lock_render_mutex();
    let Some(fixture_root) = fixture_dir() else {
        return;
    };
    let fixture = fixture_root.join("Decisive - Chip Heath.epub");
    let bytes = std::fs::read(&fixture).expect("fixture should be readable");

    let mut direct = Framebuffer::new();
    let direct_result = direct.render_epub_page(
        VecSource {
            bytes: bytes.clone(),
        },
        0,
    );
    assert_eq!(direct_result.expect("direct render should succeed"), 0);

    let mut cached_text = Vec::<u8>::new();
    let mut cold_open = Framebuffer::new();
    let build_result = cold_open.build_epub_cache_prefix_with_text_sink_and_cancel(
        VecSource { bytes },
        0,
        |chunk| {
            cached_text.extend_from_slice(chunk.as_bytes());
            Ok(())
        },
        || false,
    );
    let build = build_result.expect("cold-open prefix build should succeed");
    assert_eq!(build.rendered_page, 0);
    assert!(
        !cached_text.is_empty(),
        "expected cached text to be emitted"
    );
    assert_eq!(
        cold_open.bytes(),
        direct.bytes(),
        "cold-open prefix rendering must match direct first-page rendering"
    );
}
