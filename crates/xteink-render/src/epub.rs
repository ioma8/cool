use core::mem::MaybeUninit;

use heapless::{String, Vec};
use miniz_oxide::inflate::stream::InflateState;
use xteink_epub::{
    Epub, EpubArchive, EpubError, EpubEvent, EpubSource, MAX_ARCHIVE_ENTRIES,
    MAX_ARCHIVE_NAME_CAPACITY, ReaderBuffers,
};
use xteink_memory::SHARED_RENDER_EPUB_WORKSPACE_LIMIT_BYTES;

use crate::{
    CACHE_BOLD_END_MARKER, CACHE_BOLD_START_MARKER, CACHE_HEADING_END_MARKER,
    CACHE_HEADING_START_MARKER, CACHE_ITALIC_END_MARKER, CACHE_ITALIC_START_MARKER,
    CACHE_LAYOUT_STREAM_MARKER, CACHE_LINE_BREAK_MARKER, CACHE_PAGE_BREAK_MARKER,
    CACHE_PARAGRAPH_BREAK_MARKER, CACHE_QUOTE_END_MARKER, CACHE_QUOTE_START_MARKER, Framebuffer,
    paginator::{PaginationConfig, PaginationEvent, PaginationObserver, PaginatorState},
};

const EPUB_WORKSPACE_ZIP_CD: usize = 8 * 1024;
const EPUB_WORKSPACE_INFLATE: usize = 40 * 1024;
const EPUB_WORKSPACE_STREAM_INPUT: usize = 2048;
const EPUB_WORKSPACE_XML: usize = 16 * 1024;
const EPUB_WORKSPACE_CATALOG: usize = 8192;
const EPUB_WORKSPACE_PATH_BUF: usize = 256;
const TEXT_LEN: usize = 2048;
const CHAPTER_TITLE_LEN: usize = 64;
const DUPLICATE_TITLE_PROBE_TEXT_LEN: usize = 96;
const DUPLICATE_TITLE_PROBE_EVENTS: usize = 12;

struct EpubRenderWorkspace {
    zip_cd: [u8; EPUB_WORKSPACE_ZIP_CD],
    inflate: [u8; EPUB_WORKSPACE_INFLATE],
    stream_input: [u8; EPUB_WORKSPACE_STREAM_INPUT],
    xml: [u8; EPUB_WORKSPACE_XML],
    catalog: [u8; EPUB_WORKSPACE_CATALOG],
    path_buf: [u8; EPUB_WORKSPACE_PATH_BUF],
    stream_state: InflateState,
    archive: EpubArchive<MAX_ARCHIVE_ENTRIES, MAX_ARCHIVE_NAME_CAPACITY>,
}

pub const EPUB_RENDER_WORKSPACE_BYTES: usize = core::mem::size_of::<EpubRenderWorkspace>();

const _: [(); 1] =
    [(); (EPUB_RENDER_WORKSPACE_BYTES <= SHARED_RENDER_EPUB_WORKSPACE_LIMIT_BYTES) as usize];

static mut EPUB_RENDER_WORKSPACE: MaybeUninit<EpubRenderWorkspace> = MaybeUninit::uninit();
static mut EPUB_RENDER_WORKSPACE_READY: bool = false;

#[cfg(test)]
static EPUB_RENDER_WORKSPACE_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheBuildResult {
    pub rendered_page: usize,
    pub rendered_progress_percent: u8,
    pub cached_pages: usize,
    pub cached_progress_percent: u8,
    pub next_spine_index: u16,
    pub spine_count: u16,
    pub resume_page: usize,
    pub resume_cursor_y: u16,
    pub progress_percent: u8,
    pub complete: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RenderMode {
    TargetPageOnly,
    FullBook,
    FullBookPreserveTargetPage,
    ThroughChapterBoundaryAfterTarget,
    LayoutOnlyThroughChapterBoundaryAfterTarget,
}

fn mode_draws_page(mode: RenderMode, current_page: usize, target_page: usize) -> bool {
    match mode {
        RenderMode::TargetPageOnly
        | RenderMode::FullBook
        | RenderMode::ThroughChapterBoundaryAfterTarget => true,
        RenderMode::FullBookPreserveTargetPage => current_page <= target_page,
        RenderMode::LayoutOnlyThroughChapterBoundaryAfterTarget => false,
    }
}

fn mode_preserves_target_framebuffer(mode: RenderMode) -> bool {
    matches!(
        mode,
        RenderMode::FullBookPreserveTargetPage
            | RenderMode::ThroughChapterBoundaryAfterTarget
            | RenderMode::LayoutOnlyThroughChapterBoundaryAfterTarget
    )
}

fn mode_preserves_target_page_only(mode: RenderMode) -> bool {
    matches!(mode, RenderMode::FullBookPreserveTargetPage)
}

fn should_draw_current_page(mode: RenderMode, current_page: usize, target_page: usize) -> bool {
    if mode_preserves_target_page_only(mode) && current_page > target_page {
        return false;
    }
    mode_draws_page(mode, current_page, target_page)
}

#[derive(Clone, Copy)]
struct ResumeCheckpoint {
    page: usize,
    cursor_y: u16,
    spine_index: u16,
}

struct CacheTextObserver<'a, F> {
    on_text_chunk: &'a mut F,
    emitted_cache_bytes: usize,
}

impl<F> PaginationObserver for CacheTextObserver<'_, F>
where
    F: FnMut(&str) -> Result<(), EpubError>,
{
    fn on_text(&mut self, text: &str) -> Result<(), EpubError> {
        self.emit_cache_chunk(text)
    }

    fn on_line_break(&mut self) -> Result<(), EpubError> {
        self.emit_cache_chunk(CACHE_LINE_BREAK_MARKER.encode_utf8(&mut [0; 4]))
    }

    fn on_paragraph_break(&mut self) -> Result<(), EpubError> {
        self.emit_cache_chunk(CACHE_PARAGRAPH_BREAK_MARKER.encode_utf8(&mut [0; 4]))
    }

    fn on_page_break(&mut self) -> Result<(), EpubError> {
        self.emit_cache_chunk(CACHE_PAGE_BREAK_MARKER.encode_utf8(&mut [0; 4]))
    }

    fn on_heading_start(&mut self) -> Result<(), EpubError> {
        self.emit_cache_chunk(CACHE_HEADING_START_MARKER.encode_utf8(&mut [0; 4]))
    }

    fn on_heading_end(&mut self) -> Result<(), EpubError> {
        self.emit_cache_chunk(CACHE_HEADING_END_MARKER.encode_utf8(&mut [0; 4]))
    }

    fn on_bold_start(&mut self) -> Result<(), EpubError> {
        self.emit_cache_chunk(CACHE_BOLD_START_MARKER.encode_utf8(&mut [0; 4]))
    }

    fn on_bold_end(&mut self) -> Result<(), EpubError> {
        self.emit_cache_chunk(CACHE_BOLD_END_MARKER.encode_utf8(&mut [0; 4]))
    }

    fn on_italic_start(&mut self) -> Result<(), EpubError> {
        self.emit_cache_chunk(CACHE_ITALIC_START_MARKER.encode_utf8(&mut [0; 4]))
    }

    fn on_italic_end(&mut self) -> Result<(), EpubError> {
        self.emit_cache_chunk(CACHE_ITALIC_END_MARKER.encode_utf8(&mut [0; 4]))
    }

    fn on_quote_start(&mut self) -> Result<(), EpubError> {
        self.emit_cache_chunk(CACHE_QUOTE_START_MARKER.encode_utf8(&mut [0; 4]))
    }

    fn on_quote_end(&mut self) -> Result<(), EpubError> {
        self.emit_cache_chunk(CACHE_QUOTE_END_MARKER.encode_utf8(&mut [0; 4]))
    }
}

impl<F> CacheTextObserver<'_, F> {
    fn emit_cache_chunk(&mut self, chunk: &str) -> Result<(), EpubError>
    where
        F: FnMut(&str) -> Result<(), EpubError>,
    {
        self.emitted_cache_bytes = self.emitted_cache_bytes.saturating_add(chunk.len());
        (self.on_text_chunk)(chunk)
    }

    fn emitted_cache_bytes(&self) -> usize {
        self.emitted_cache_bytes
    }
}

enum BufferedChapterEvent {
    Text(String<DUPLICATE_TITLE_PROBE_TEXT_LEN>),
    LineBreak,
    ParagraphBreak,
}

struct DuplicateTitleProbe {
    normalized_title: String<DUPLICATE_TITLE_PROBE_TEXT_LEN>,
    candidate: String<DUPLICATE_TITLE_PROBE_TEXT_LEN>,
    buffered: Vec<BufferedChapterEvent, DUPLICATE_TITLE_PROBE_EVENTS>,
}

impl DuplicateTitleProbe {
    fn new(title: &str) -> Self {
        Self {
            normalized_title: normalize_title_text(title),
            candidate: String::new(),
            buffered: Vec::new(),
        }
    }

    fn is_empty(&self) -> bool {
        self.candidate.is_empty()
    }

    fn push_text(&mut self, text: &str) -> bool {
        let mut chunk = String::<DUPLICATE_TITLE_PROBE_TEXT_LEN>::new();
        let _ = chunk.push_str(text);
        let _ = self.buffered.push(BufferedChapterEvent::Text(chunk));
        append_normalized_title_text(&mut self.candidate, text);
        self.matches_prefix()
    }

    fn push_break(&mut self, paragraph: bool) -> bool {
        let _ = self.buffered.push(if paragraph {
            BufferedChapterEvent::ParagraphBreak
        } else {
            BufferedChapterEvent::LineBreak
        });
        if !self.candidate.is_empty() && !self.candidate.ends_with(' ') {
            let _ = self.candidate.push(' ');
        }
        self.matches_prefix()
    }

    fn is_complete(&self) -> bool {
        self.candidate.as_str().trim_end() == self.normalized_title.as_str()
    }

    fn matches_prefix(&self) -> bool {
        self.normalized_title
            .starts_with(self.candidate.as_str().trim_end())
    }
}

fn normalize_title_text<const N: usize>(text: &str) -> String<N> {
    let mut normalized = String::<N>::new();
    append_normalized_title_text(&mut normalized, text);
    normalized
}

fn append_normalized_title_text<const N: usize>(out: &mut String<N>, text: &str) {
    let mut pending_space = false;
    for ch in text.chars() {
        if ch.is_alphanumeric() {
            if pending_space && !out.is_empty() {
                let _ = out.push(' ');
            }
            let _ = out.push(ch.to_ascii_lowercase());
            pending_space = false;
        } else if (ch.is_whitespace() || ch.is_ascii_punctuation()) && !out.is_empty() {
            pending_space = true;
        }
    }
}

fn with_epub_render_workspace<R>(f: impl FnOnce(&mut EpubRenderWorkspace) -> R) -> R {
    #[cfg(target_arch = "riscv32")]
    {
        critical_section::with(|_| {
            let workspace = init_epub_render_workspace();
            f(workspace)
        })
    }

    #[cfg(all(not(target_arch = "riscv32"), test))]
    {
        let _guard = EPUB_RENDER_WORKSPACE_MUTEX
            .lock()
            .expect("workspace mutex poisoned");
        let workspace = init_epub_render_workspace();
        f(workspace)
    }

    #[cfg(all(not(target_arch = "riscv32"), not(test)))]
    {
        let workspace = init_epub_render_workspace();
        f(workspace)
    }
}

fn init_epub_render_workspace() -> &'static mut EpubRenderWorkspace {
    unsafe {
        if !EPUB_RENDER_WORKSPACE_READY {
            let workspace_ptr =
                core::ptr::addr_of_mut!(EPUB_RENDER_WORKSPACE) as *mut EpubRenderWorkspace;
            core::ptr::write_bytes(
                core::ptr::addr_of_mut!((*workspace_ptr).zip_cd).cast::<u8>(),
                0,
                EPUB_WORKSPACE_ZIP_CD,
            );
            core::ptr::write_bytes(
                core::ptr::addr_of_mut!((*workspace_ptr).inflate).cast::<u8>(),
                0,
                EPUB_WORKSPACE_INFLATE,
            );
            core::ptr::write_bytes(
                core::ptr::addr_of_mut!((*workspace_ptr).stream_input).cast::<u8>(),
                0,
                EPUB_WORKSPACE_STREAM_INPUT,
            );
            core::ptr::write_bytes(
                core::ptr::addr_of_mut!((*workspace_ptr).xml).cast::<u8>(),
                0,
                EPUB_WORKSPACE_XML,
            );
            core::ptr::write_bytes(
                core::ptr::addr_of_mut!((*workspace_ptr).catalog).cast::<u8>(),
                0,
                EPUB_WORKSPACE_CATALOG,
            );
            core::ptr::write_bytes(
                core::ptr::addr_of_mut!((*workspace_ptr).path_buf).cast::<u8>(),
                0,
                EPUB_WORKSPACE_PATH_BUF,
            );
            core::ptr::write(
                core::ptr::addr_of_mut!((*workspace_ptr).stream_state),
                InflateState::new(miniz_oxide::DataFormat::Raw),
            );
            core::ptr::write(
                core::ptr::addr_of_mut!((*workspace_ptr).archive),
                EpubArchive::new(),
            );
            EPUB_RENDER_WORKSPACE_READY = true;
        }
        &mut *(core::ptr::addr_of_mut!(EPUB_RENDER_WORKSPACE) as *mut EpubRenderWorkspace)
    }
}

impl Framebuffer {
    pub fn render_epub_page_with_progress<S: EpubSource>(
        &mut self,
        source: S,
        target_page: usize,
    ) -> Result<CacheBuildResult, EpubError> {
        self.render_epub_with_mode(
            source,
            target_page,
            &mut |_| Ok(()),
            &mut |_, _| Ok(()),
            &mut |_| None,
            RenderMode::ThroughChapterBoundaryAfterTarget,
            None,
            &mut || false,
        )
    }

    pub fn render_epub_page<S: EpubSource>(
        &mut self,
        source: S,
        target_page: usize,
    ) -> Result<usize, EpubError> {
        self.render_epub_page_with_text_sink_and_cancel(
            source,
            target_page,
            |_| Ok(()),
            false,
            || false,
        )
    }

    pub fn render_epub_page_with_text_sink_and_cancel<S: EpubSource, F, C>(
        &mut self,
        source: S,
        target_page: usize,
        mut on_text_chunk: F,
        parse_full_book: bool,
        mut should_cancel: C,
    ) -> Result<usize, EpubError>
    where
        F: FnMut(&str) -> Result<(), EpubError>,
        C: FnMut() -> bool,
    {
        let mode = if parse_full_book {
            RenderMode::FullBook
        } else {
            RenderMode::TargetPageOnly
        };
        let result = self.render_epub_with_mode(
            source,
            target_page,
            &mut on_text_chunk,
            &mut |_, _| Ok(()),
            &mut |_| None,
            mode,
            None,
            &mut should_cancel,
        )?;
        Ok(result.rendered_page)
    }

    pub fn build_epub_cache_prefix_with_text_sink_and_cancel<S: EpubSource, F, C>(
        &mut self,
        source: S,
        target_page: usize,
        mut on_text_chunk: F,
        mut should_cancel: C,
    ) -> Result<CacheBuildResult, EpubError>
    where
        F: FnMut(&str) -> Result<(), EpubError>,
        C: FnMut() -> bool,
    {
        self.build_epub_cache_prefix_with_callbacks_and_cancel(
            source,
            target_page,
            &mut on_text_chunk,
            &mut |_, _| Ok(()),
            &mut |_| None,
            &mut should_cancel,
        )
    }

    pub fn build_epub_cache_prefix_with_callbacks_and_cancel<S: EpubSource, F, G, H, C>(
        &mut self,
        source: S,
        target_page: usize,
        mut on_text_chunk: F,
        mut on_chapter_start: G,
        mut chapter_title_for_index: H,
        mut should_cancel: C,
    ) -> Result<CacheBuildResult, EpubError>
    where
        F: FnMut(&str) -> Result<(), EpubError>,
        G: FnMut(u16, usize) -> Result<(), EpubError>,
        H: FnMut(u16) -> Option<String<CHAPTER_TITLE_LEN>>,
        C: FnMut() -> bool,
    {
        self.render_epub_with_mode(
            source,
            target_page,
            &mut on_text_chunk,
            &mut on_chapter_start,
            &mut chapter_title_for_index,
            RenderMode::FullBookPreserveTargetPage,
            None,
            &mut should_cancel,
        )
    }

    pub fn extend_epub_cache_prefix_with_text_sink_and_cancel<S: EpubSource, F, C>(
        &mut self,
        source: S,
        target_page: usize,
        resume_page: usize,
        resume_cursor_y: u16,
        resume_spine_index: u16,
        mut on_text_chunk: F,
        mut should_cancel: C,
    ) -> Result<CacheBuildResult, EpubError>
    where
        F: FnMut(&str) -> Result<(), EpubError>,
        C: FnMut() -> bool,
    {
        self.render_epub_with_mode(
            source,
            target_page,
            &mut on_text_chunk,
            &mut |_, _| Ok(()),
            &mut |_| None,
            RenderMode::LayoutOnlyThroughChapterBoundaryAfterTarget,
            Some(ResumeCheckpoint {
                page: resume_page,
                cursor_y: resume_cursor_y,
                spine_index: resume_spine_index,
            }),
            &mut should_cancel,
        )
    }

    fn render_epub_with_mode<S: EpubSource, F, G, H, C>(
        &mut self,
        source: S,
        target_page: usize,
        mut on_text_chunk: &mut F,
        on_chapter_start: &mut G,
        chapter_title_for_index: &mut H,
        mut mode: RenderMode,
        resume: Option<ResumeCheckpoint>,
        should_cancel: &mut C,
    ) -> Result<CacheBuildResult, EpubError>
    where
        F: FnMut(&str) -> Result<(), EpubError>,
        G: FnMut(u16, usize) -> Result<(), EpubError>,
        H: FnMut(u16) -> Option<String<CHAPTER_TITLE_LEN>>,
        C: FnMut() -> bool,
    {
        with_epub_render_workspace(|workspace| {
            let workspace_ptr = core::ptr::addr_of_mut!(*workspace);
            unsafe {
                (*workspace_ptr).zip_cd.fill(0);
                (*workspace_ptr).inflate.fill(0);
                (*workspace_ptr).stream_input.fill(0);
                (*workspace_ptr).xml.fill(0);
                (*workspace_ptr).catalog.fill(0);
                (*workspace_ptr).path_buf.fill(0);
                (*workspace_ptr).stream_state = InflateState::new(miniz_oxide::DataFormat::Raw);
                (*workspace_ptr).archive = EpubArchive::new();
            }
            let mut epub = Epub::open(source)?;
            let mut paginator = PaginatorState::<TEXT_LEN>::new(PaginationConfig {
                target_page,
                draw_target_page: should_draw_current_page(
                    mode,
                    resume.map_or(0, |checkpoint| checkpoint.page),
                    target_page,
                ),
                stop_after_target_page: matches!(mode, RenderMode::TargetPageOnly),
                preserve_target_page_framebuffer: mode_preserves_target_framebuffer(mode),
                start_page: resume.map_or(0, |checkpoint| checkpoint.page),
                start_cursor_y: resume.map_or(0, |checkpoint| checkpoint.cursor_y),
            });
            if resume.is_none() {
                // The layout marker is part of the serialized cache stream.
            }
            let mut observer = CacheTextObserver {
                on_text_chunk: &mut on_text_chunk,
                emitted_cache_bytes: 0,
            };
            if resume.is_none() {
                observer.emit_cache_chunk(CACHE_LAYOUT_STREAM_MARKER.encode_utf8(&mut [0; 4]))?;
            }
            let mut stop_after_spine_index: Option<u16> = None;
            let mut chapter_end_override: Option<(usize, u16)> = None;
            let mut rendered_prefix_text_bytes: Option<usize> = None;
            let mut reported_next_spine_index = 0u16;
            let mut duplicate_title_probe: Option<DuplicateTitleProbe> = None;

            if mode != RenderMode::LayoutOnlyThroughChapterBoundaryAfterTarget {
                self.clear(0xFF);
            }

            if let Some(checkpoint) = resume {
                epub.resume_from_spine_index(
                    ReaderBuffers {
                        zip_cd: unsafe { &mut (*workspace_ptr).zip_cd },
                        inflate: unsafe { &mut (*workspace_ptr).inflate },
                        stream_input: unsafe { &mut (*workspace_ptr).stream_input },
                        xml: unsafe { &mut (*workspace_ptr).xml },
                        catalog: unsafe { &mut (*workspace_ptr).catalog },
                        path_buf: unsafe { &mut (*workspace_ptr).path_buf },
                        stream_state: unsafe { &mut (*workspace_ptr).stream_state },
                        archive: unsafe { &mut (*workspace_ptr).archive },
                    },
                    checkpoint.spine_index,
                )?;
            }

            loop {
                if should_cancel() {
                    return Err(EpubError::Cancelled);
                }
                let event = epub.next_event_with_spine_index(ReaderBuffers {
                    zip_cd: unsafe { &mut (*workspace_ptr).zip_cd },
                    inflate: unsafe { &mut (*workspace_ptr).inflate },
                    stream_input: unsafe { &mut (*workspace_ptr).stream_input },
                    xml: unsafe { &mut (*workspace_ptr).xml },
                    catalog: unsafe { &mut (*workspace_ptr).catalog },
                    path_buf: unsafe { &mut (*workspace_ptr).path_buf },
                    stream_state: unsafe { &mut (*workspace_ptr).stream_state },
                    archive: unsafe { &mut (*workspace_ptr).archive },
                })?;

                let Some((next_spine_index, event)) = event else {
                    break;
                };
                let page_before_event = paginator.current_page();
                let cursor_before_event = paginator.cursor_y();
                let config = PaginationConfig {
                    target_page,
                    draw_target_page: should_draw_current_page(
                        mode,
                        paginator.current_page(),
                        target_page,
                    ),
                    stop_after_target_page: matches!(mode, RenderMode::TargetPageOnly),
                    preserve_target_page_framebuffer: mode_preserves_target_framebuffer(mode),
                    start_page: 0,
                    start_cursor_y: 0,
                };
                if next_spine_index > reported_next_spine_index {
                    let chapter_title_index = reported_next_spine_index;
                    if paginator.has_visible_page_content_or_pending_output() {
                        let chapter_break = paginator.feed(
                            self,
                            &mut observer,
                            config,
                            PaginationEvent::ExplicitPageBreak,
                        )?;
                        if chapter_break.target_complete {
                            rendered_prefix_text_bytes
                                .get_or_insert(observer.emitted_cache_bytes());
                            match mode {
                                RenderMode::TargetPageOnly => break,
                                RenderMode::ThroughChapterBoundaryAfterTarget => {
                                    mode = RenderMode::LayoutOnlyThroughChapterBoundaryAfterTarget;
                                    stop_after_spine_index.get_or_insert(next_spine_index);
                                }
                                RenderMode::LayoutOnlyThroughChapterBoundaryAfterTarget => {
                                    stop_after_spine_index.get_or_insert(next_spine_index);
                                }
                                RenderMode::FullBook | RenderMode::FullBookPreserveTargetPage => {}
                            }
                        }
                    }
                    let chapter_start_offset = observer
                        .emitted_cache_bytes()
                        .saturating_add(paginator.pending_output_bytes());
                    report_chapter_starts(
                        &mut reported_next_spine_index,
                        next_spine_index,
                        chapter_start_offset,
                        on_chapter_start,
                    )?;
                    if let Some(title) = chapter_title_for_index(chapter_title_index)
                        && !title.is_empty()
                    {
                        let heading_start = paginator.feed(
                            self,
                            &mut observer,
                            config,
                            PaginationEvent::HeadingStart,
                        )?;
                        if heading_start.target_complete {
                            rendered_prefix_text_bytes
                                .get_or_insert(observer.emitted_cache_bytes());
                        }
                        let title_progress = paginator.feed(
                            self,
                            &mut observer,
                            config,
                            PaginationEvent::Text(title.as_str()),
                        )?;
                        if title_progress.target_complete {
                            rendered_prefix_text_bytes
                                .get_or_insert(observer.emitted_cache_bytes());
                            match mode {
                                RenderMode::TargetPageOnly => break,
                                RenderMode::ThroughChapterBoundaryAfterTarget => {
                                    mode = RenderMode::LayoutOnlyThroughChapterBoundaryAfterTarget;
                                    stop_after_spine_index.get_or_insert(next_spine_index);
                                }
                                RenderMode::LayoutOnlyThroughChapterBoundaryAfterTarget => {
                                    stop_after_spine_index.get_or_insert(next_spine_index);
                                }
                                RenderMode::FullBook | RenderMode::FullBookPreserveTargetPage => {}
                            }
                        } else {
                            let heading_end = paginator.feed(
                                self,
                                &mut observer,
                                config,
                                PaginationEvent::HeadingEnd,
                            )?;
                            if heading_end.target_complete {
                                rendered_prefix_text_bytes
                                    .get_or_insert(observer.emitted_cache_bytes());
                            }
                            for _ in 0..2 {
                                let spacing_progress = paginator.feed(
                                    self,
                                    &mut observer,
                                    config,
                                    PaginationEvent::LineBreak,
                                )?;
                                if spacing_progress.target_complete {
                                    rendered_prefix_text_bytes
                                        .get_or_insert(observer.emitted_cache_bytes());
                                    match mode {
                                        RenderMode::TargetPageOnly => break,
                                        RenderMode::ThroughChapterBoundaryAfterTarget => {
                                            mode = RenderMode::LayoutOnlyThroughChapterBoundaryAfterTarget;
                                            stop_after_spine_index.get_or_insert(next_spine_index);
                                        }
                                        RenderMode::LayoutOnlyThroughChapterBoundaryAfterTarget => {
                                            stop_after_spine_index.get_or_insert(next_spine_index);
                                        }
                                        RenderMode::FullBook
                                        | RenderMode::FullBookPreserveTargetPage => {}
                                    }
                                }
                            }
                        }
                        duplicate_title_probe = Some(DuplicateTitleProbe::new(title.as_str()));
                    }
                }
                if let Some(probe) = duplicate_title_probe.as_mut() {
                    let probe_result = match &event {
                        EpubEvent::Text(chunk) => Some((probe.push_text(chunk), false)),
                        EpubEvent::LineBreak => Some((probe.push_break(false), false)),
                        EpubEvent::ParagraphEnd | EpubEvent::HeadingEnd => {
                            Some((probe.push_break(true), probe.is_complete()))
                        }
                        EpubEvent::ParagraphStart
                        | EpubEvent::HeadingStart(_)
                        | EpubEvent::BoldStart
                        | EpubEvent::BoldEnd
                        | EpubEvent::ItalicStart
                        | EpubEvent::ItalicEnd
                        | EpubEvent::QuoteStart
                        | EpubEvent::QuoteEnd => None,
                        EpubEvent::Image { .. } | EpubEvent::UnsupportedTag => {
                            Some((probe.is_empty(), false))
                        }
                    };
                    if let Some((matches_prefix, complete)) = probe_result {
                        if complete {
                            duplicate_title_probe = None;
                            continue;
                        }
                        if !matches_prefix {
                            let buffered = core::mem::replace(&mut probe.buffered, Vec::new());
                            duplicate_title_probe = None;
                            if let Some(progress) = feed_buffered_chapter_events(
                                &mut paginator,
                                self,
                                &mut observer,
                                config,
                                buffered,
                            )? {
                                if progress.target_complete {
                                    rendered_prefix_text_bytes
                                        .get_or_insert(observer.emitted_cache_bytes());
                                    match mode {
                                        RenderMode::TargetPageOnly => break,
                                        RenderMode::ThroughChapterBoundaryAfterTarget => {
                                            mode = RenderMode::LayoutOnlyThroughChapterBoundaryAfterTarget;
                                            stop_after_spine_index.get_or_insert(next_spine_index);
                                        }
                                        RenderMode::LayoutOnlyThroughChapterBoundaryAfterTarget => {
                                            stop_after_spine_index.get_or_insert(next_spine_index);
                                        }
                                        RenderMode::FullBook
                                        | RenderMode::FullBookPreserveTargetPage => {}
                                    }
                                    continue;
                                }
                            }
                            if matches!(
                                event,
                                EpubEvent::Text(_)
                                    | EpubEvent::LineBreak
                                    | EpubEvent::ParagraphEnd
                                    | EpubEvent::HeadingEnd
                            ) {
                                continue;
                            }
                        } else if matches!(
                            event,
                            EpubEvent::Text(_)
                                | EpubEvent::LineBreak
                                | EpubEvent::ParagraphEnd
                                | EpubEvent::HeadingEnd
                        ) {
                            continue;
                        }
                    }
                }
                let progress = match event {
                    EpubEvent::Text(chunk) => Some(paginator.feed(
                        self,
                        &mut observer,
                        config,
                        PaginationEvent::Text(chunk),
                    )?),
                    EpubEvent::LineBreak => Some(paginator.feed(
                        self,
                        &mut observer,
                        config,
                        PaginationEvent::LineBreak,
                    )?),
                    EpubEvent::ParagraphStart => None,
                    EpubEvent::HeadingStart(_) => Some(paginator.feed(
                        self,
                        &mut observer,
                        config,
                        PaginationEvent::HeadingStart,
                    )?),
                    EpubEvent::ParagraphEnd => Some(paginator.feed(
                        self,
                        &mut observer,
                        config,
                        PaginationEvent::ParagraphBreak,
                    )?),
                    EpubEvent::HeadingEnd => Some(paginator.feed(
                        self,
                        &mut observer,
                        config,
                        PaginationEvent::HeadingEnd,
                    )?),
                    EpubEvent::BoldStart => Some(paginator.feed(
                        self,
                        &mut observer,
                        config,
                        PaginationEvent::BoldStart,
                    )?),
                    EpubEvent::BoldEnd => Some(paginator.feed(
                        self,
                        &mut observer,
                        config,
                        PaginationEvent::BoldEnd,
                    )?),
                    EpubEvent::ItalicStart => Some(paginator.feed(
                        self,
                        &mut observer,
                        config,
                        PaginationEvent::ItalicStart,
                    )?),
                    EpubEvent::ItalicEnd => Some(paginator.feed(
                        self,
                        &mut observer,
                        config,
                        PaginationEvent::ItalicEnd,
                    )?),
                    EpubEvent::QuoteStart => Some(paginator.feed(
                        self,
                        &mut observer,
                        config,
                        PaginationEvent::QuoteStart,
                    )?),
                    EpubEvent::QuoteEnd => Some(paginator.feed(
                        self,
                        &mut observer,
                        config,
                        PaginationEvent::QuoteEnd,
                    )?),
                    EpubEvent::Image { alt, .. } => alt
                        .map(|alt| {
                            let progress = paginator.feed(
                                self,
                                &mut observer,
                                config,
                                PaginationEvent::Text(alt),
                            )?;
                            if progress.target_complete {
                                Ok(progress)
                            } else {
                                paginator.feed(
                                    self,
                                    &mut observer,
                                    config,
                                    PaginationEvent::LineBreak,
                                )
                            }
                        })
                        .transpose()?,
                    EpubEvent::UnsupportedTag => None,
                };
                let Some(progress) = progress else {
                    continue;
                };
                if progress.target_complete {
                    rendered_prefix_text_bytes.get_or_insert(observer.emitted_cache_bytes());
                    match mode {
                        RenderMode::TargetPageOnly => break,
                        RenderMode::ThroughChapterBoundaryAfterTarget => {
                            mode = RenderMode::LayoutOnlyThroughChapterBoundaryAfterTarget;
                            stop_after_spine_index.get_or_insert(next_spine_index);
                        }
                        RenderMode::LayoutOnlyThroughChapterBoundaryAfterTarget => {
                            stop_after_spine_index.get_or_insert(next_spine_index);
                        }
                        RenderMode::FullBook | RenderMode::FullBookPreserveTargetPage => {}
                    }
                }
                if matches!(
                    mode,
                    RenderMode::LayoutOnlyThroughChapterBoundaryAfterTarget
                ) && stop_after_spine_index
                    .map(|stop| next_spine_index > stop)
                    .unwrap_or(false)
                {
                    chapter_end_override = Some((page_before_event, cursor_before_event));
                    break;
                }
            }

            report_chapter_starts(
                &mut reported_next_spine_index,
                epub.next_spine_index(),
                observer
                    .emitted_cache_bytes()
                    .saturating_add(paginator.pending_output_bytes()),
                on_chapter_start,
            )?;

            let finish_config = PaginationConfig {
                target_page,
                draw_target_page: should_draw_current_page(
                    mode,
                    paginator.current_page(),
                    target_page,
                ),
                stop_after_target_page: matches!(mode, RenderMode::TargetPageOnly),
                preserve_target_page_framebuffer: mode_preserves_target_framebuffer(mode),
                start_page: 0,
                start_cursor_y: 0,
            };
            let finish =
                paginator.feed(self, &mut observer, finish_config, PaginationEvent::End)?;
            let rendered_page = finish.current_page.min(target_page);
            let (current_page, cursor_y) =
                chapter_end_override.unwrap_or((paginator.current_page(), paginator.cursor_y()));
            let cached_pages = if epub.is_complete() {
                current_page.saturating_add(1)
            } else if cursor_y > 0 {
                current_page.saturating_add(1)
            } else {
                current_page.max(1)
            };
            let rendered_page = rendered_page.min(target_page);
            let (consumed_book_bytes, total_book_bytes) =
                epub.progress_bytes(unsafe { &(*workspace_ptr).catalog })?;
            let cached_progress_percent =
                percent_from_book_bytes(consumed_book_bytes, total_book_bytes, epub.is_complete());
            let rendered_progress_percent = if epub.is_complete() {
                percent_from_pages(rendered_page, cached_pages)
            } else {
                percent_from_cached_prefix_bytes(
                    rendered_prefix_text_bytes.unwrap_or_else(|| observer.emitted_cache_bytes()),
                    observer.emitted_cache_bytes(),
                    cached_progress_percent,
                )
            };
            Ok(CacheBuildResult {
                rendered_page,
                rendered_progress_percent,
                cached_pages,
                cached_progress_percent,
                next_spine_index: epub.next_spine_index(),
                spine_count: epub.spine_count(),
                resume_page: current_page,
                resume_cursor_y: cursor_y,
                progress_percent: rendered_progress_percent,
                complete: epub.is_complete(),
            })
        })
    }
}

fn report_chapter_starts<G>(
    reported_next_spine_index: &mut u16,
    current_next_spine_index: u16,
    current_offset: usize,
    on_chapter_start: &mut G,
) -> Result<(), EpubError>
where
    G: FnMut(u16, usize) -> Result<(), EpubError>,
{
    if current_next_spine_index <= *reported_next_spine_index {
        return Ok(());
    }

    for chapter_index in *reported_next_spine_index..current_next_spine_index {
        on_chapter_start(chapter_index, current_offset)?;
    }
    *reported_next_spine_index = current_next_spine_index;
    Ok(())
}

fn feed_buffered_chapter_events<const N: usize, R, O>(
    paginator: &mut PaginatorState<N>,
    renderer: &mut R,
    observer: &mut O,
    config: PaginationConfig,
    buffered: Vec<BufferedChapterEvent, DUPLICATE_TITLE_PROBE_EVENTS>,
) -> Result<Option<crate::paginator::PaginationProgress>, EpubError>
where
    R: crate::paginator::PaginationRenderer,
    O: PaginationObserver,
{
    let mut last = None;
    for event in buffered {
        let progress = match event {
            BufferedChapterEvent::Text(text) => paginator.feed(
                renderer,
                observer,
                config,
                PaginationEvent::Text(text.as_str()),
            )?,
            BufferedChapterEvent::LineBreak => {
                paginator.feed(renderer, observer, config, PaginationEvent::LineBreak)?
            }
            BufferedChapterEvent::ParagraphBreak => {
                paginator.feed(renderer, observer, config, PaginationEvent::ParagraphBreak)?
            }
        };
        let target_complete = progress.target_complete;
        last = Some(progress);
        if target_complete {
            break;
        }
    }
    Ok(last)
}

fn percent_from_pages(rendered_page: usize, total_pages: usize) -> u8 {
    if total_pages == 0 {
        return 0;
    }
    let progress = ((rendered_page.saturating_add(1)) * 100) / total_pages.max(1);
    progress.clamp(1, 100) as u8
}

fn percent_from_cached_prefix_bytes(
    rendered_text_bytes: usize,
    cached_text_bytes: usize,
    cached_progress_percent: u8,
) -> u8 {
    if cached_text_bytes == 0 {
        return 0;
    }
    let progress = rendered_text_bytes
        .min(cached_text_bytes)
        .saturating_mul(usize::from(cached_progress_percent))
        / cached_text_bytes.max(1);
    progress.clamp(1, usize::from(cached_progress_percent).max(1)) as u8
}

fn percent_from_book_bytes(
    consumed_book_bytes: usize,
    total_book_bytes: usize,
    complete: bool,
) -> u8 {
    if total_book_bytes == 0 {
        return 0;
    }
    let max = if complete { 100 } else { 99 };
    let progress = consumed_book_bytes
        .min(total_book_bytes)
        .saturating_mul(100)
        / total_book_bytes.max(1);
    progress.clamp(1, max) as u8
}
