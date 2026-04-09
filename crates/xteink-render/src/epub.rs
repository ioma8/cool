use core::mem::MaybeUninit;

use miniz_oxide::inflate::stream::InflateState;
use xteink_epub::{
    Epub, EpubArchive, EpubError, EpubEvent, EpubSource, MAX_ARCHIVE_ENTRIES,
    MAX_ARCHIVE_NAME_CAPACITY, ReaderBuffers,
};
use xteink_memory::SHARED_RENDER_EPUB_WORKSPACE_LIMIT_BYTES;

use crate::{
    CACHE_LAYOUT_STREAM_MARKER, CACHE_LINE_BREAK_MARKER, CACHE_PAGE_BREAK_MARKER, CACHE_PARAGRAPH_BREAK_MARKER,
    Framebuffer,
    paginator::{PaginationConfig, PaginationEvent, PaginationObserver, PaginatorState},
};

const EPUB_WORKSPACE_ZIP_CD: usize = 16 * 1024;
const EPUB_WORKSPACE_INFLATE: usize = 48 * 1024;
const EPUB_WORKSPACE_STREAM_INPUT: usize = 2048;
const EPUB_WORKSPACE_XML: usize = 32 * 1024;
const EPUB_WORKSPACE_CATALOG: usize = 8192;
const EPUB_WORKSPACE_PATH_BUF: usize = 256;
const TEXT_LEN: usize = 2048;

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
    pub cached_pages: usize,
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
    ThroughChapterBoundaryAfterTarget,
    LayoutOnlyThroughChapterBoundaryAfterTarget,
}

fn mode_draws_page(mode: RenderMode, _current_page: usize, _target_page: usize) -> bool {
    match mode {
        RenderMode::TargetPageOnly | RenderMode::FullBook | RenderMode::ThroughChapterBoundaryAfterTarget => true,
        RenderMode::LayoutOnlyThroughChapterBoundaryAfterTarget => false,
    }
}

fn mode_preserves_target_framebuffer(mode: RenderMode) -> bool {
    matches!(
        mode,
        RenderMode::ThroughChapterBoundaryAfterTarget
            | RenderMode::LayoutOnlyThroughChapterBoundaryAfterTarget
    )
}

#[derive(Clone, Copy)]
struct ResumeCheckpoint {
    page: usize,
    cursor_y: u16,
    spine_index: u16,
}

struct CacheTextObserver<'a, F> {
    on_text_chunk: &'a mut F,
}

impl<F> PaginationObserver for CacheTextObserver<'_, F>
where
    F: FnMut(&str) -> Result<(), EpubError>,
{
    fn on_text(&mut self, text: &str) -> Result<(), EpubError> {
        (self.on_text_chunk)(text)
    }

    fn on_line_break(&mut self) -> Result<(), EpubError> {
        (self.on_text_chunk)(CACHE_LINE_BREAK_MARKER.encode_utf8(&mut [0; 4]))
    }

    fn on_paragraph_break(&mut self) -> Result<(), EpubError> {
        (self.on_text_chunk)(CACHE_PARAGRAPH_BREAK_MARKER.encode_utf8(&mut [0; 4]))
    }

    fn on_page_break(&mut self) -> Result<(), EpubError> {
        (self.on_text_chunk)(CACHE_PAGE_BREAK_MARKER.encode_utf8(&mut [0; 4]))
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
        let _guard = EPUB_RENDER_WORKSPACE_MUTEX.lock().expect("workspace mutex poisoned");
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
            core::ptr::write_bytes(core::ptr::addr_of_mut!((*workspace_ptr).zip_cd).cast::<u8>(), 0, EPUB_WORKSPACE_ZIP_CD);
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
            core::ptr::write_bytes(core::ptr::addr_of_mut!((*workspace_ptr).xml).cast::<u8>(), 0, EPUB_WORKSPACE_XML);
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
        self.render_epub_with_mode(
            source,
            target_page,
            &mut on_text_chunk,
            RenderMode::ThroughChapterBoundaryAfterTarget,
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
            RenderMode::LayoutOnlyThroughChapterBoundaryAfterTarget,
            Some(ResumeCheckpoint {
                page: resume_page,
                cursor_y: resume_cursor_y,
                spine_index: resume_spine_index,
            }),
            &mut should_cancel,
        )
    }

    fn render_epub_with_mode<S: EpubSource, F, C>(
        &mut self,
        source: S,
        target_page: usize,
        mut on_text_chunk: &mut F,
        mut mode: RenderMode,
        resume: Option<ResumeCheckpoint>,
        should_cancel: &mut C,
    ) -> Result<CacheBuildResult, EpubError>
    where
        F: FnMut(&str) -> Result<(), EpubError>,
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
                draw_target_page: mode_draws_page(
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
                on_text_chunk(CACHE_LAYOUT_STREAM_MARKER.encode_utf8(&mut [0; 4]))?;
            }
            let mut observer = CacheTextObserver { on_text_chunk: &mut on_text_chunk };
            let mut stop_after_spine_index: Option<u16> = None;
            let mut active_chapter_index: Option<u16> = None;
            let mut active_chapter_start_page = paginator.current_page();
            let mut target_chapter_index: Option<u16> = None;
            let mut target_chapter_start_page: Option<usize> = None;
            let mut chapter_end_override: Option<(usize, u16)> = None;

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
                let event = epub.next_event(ReaderBuffers {
                    zip_cd: unsafe { &mut (*workspace_ptr).zip_cd },
                    inflate: unsafe { &mut (*workspace_ptr).inflate },
                    stream_input: unsafe { &mut (*workspace_ptr).stream_input },
                    xml: unsafe { &mut (*workspace_ptr).xml },
                    catalog: unsafe { &mut (*workspace_ptr).catalog },
                    path_buf: unsafe { &mut (*workspace_ptr).path_buf },
                    stream_state: unsafe { &mut (*workspace_ptr).stream_state },
                    archive: unsafe { &mut (*workspace_ptr).archive },
                })?;

                let Some(event) = event else { break };
                let page_before_event = paginator.current_page();
                let cursor_before_event = paginator.cursor_y();
                let config = PaginationConfig {
                    target_page,
                    draw_target_page: mode_draws_page(mode, paginator.current_page(), target_page),
                    stop_after_target_page: matches!(mode, RenderMode::TargetPageOnly),
                    preserve_target_page_framebuffer: mode_preserves_target_framebuffer(mode),
                    start_page: 0,
                    start_cursor_y: 0,
                };
                let progress = match event {
                    EpubEvent::Text(chunk) => {
                        paginator.feed(self, &mut observer, config, PaginationEvent::Text(chunk))?
                    }
                    EpubEvent::LineBreak => {
                        paginator.feed(self, &mut observer, config, PaginationEvent::LineBreak)?
                    }
                    EpubEvent::ParagraphStart | EpubEvent::HeadingStart(_) => continue,
                    EpubEvent::ParagraphEnd | EpubEvent::HeadingEnd => paginator.feed(
                        self,
                        &mut observer,
                        config,
                        PaginationEvent::ParagraphBreak,
                    )?,
                    EpubEvent::Image { alt, .. } => {
                        if let Some(alt) = alt {
                            let progress =
                                paginator.feed(self, &mut observer, config, PaginationEvent::Text(alt))?;
                            if progress.target_complete {
                                progress
                            } else {
                                paginator.feed(self, &mut observer, config, PaginationEvent::LineBreak)?
                            }
                        } else {
                            continue;
                        }
                    }
                    EpubEvent::UnsupportedTag => continue,
                };
                let event_chapter_index = epub.next_spine_index().saturating_sub(1);
                if active_chapter_index != Some(event_chapter_index) {
                    active_chapter_index = Some(event_chapter_index);
                    active_chapter_start_page = page_before_event;
                }

                if progress.target_complete {
                    match mode {
                        RenderMode::TargetPageOnly => break,
                        RenderMode::ThroughChapterBoundaryAfterTarget => {
                            target_chapter_index.get_or_insert(event_chapter_index);
                            target_chapter_start_page.get_or_insert(active_chapter_start_page);
                            mode = RenderMode::LayoutOnlyThroughChapterBoundaryAfterTarget;
                            stop_after_spine_index.get_or_insert(epub.next_spine_index());
                        }
                        RenderMode::LayoutOnlyThroughChapterBoundaryAfterTarget => {
                            stop_after_spine_index.get_or_insert(epub.next_spine_index());
                        }
                        RenderMode::FullBook => {}
                    }
                }
                if matches!(mode, RenderMode::LayoutOnlyThroughChapterBoundaryAfterTarget)
                    && stop_after_spine_index
                        .map(|stop| epub.next_spine_index() > stop)
                        .unwrap_or(false)
                {
                    chapter_end_override = Some((page_before_event, cursor_before_event));
                    break;
                }
            }

            let finish_config = PaginationConfig {
                target_page,
                draw_target_page: mode_draws_page(mode, paginator.current_page(), target_page),
                stop_after_target_page: matches!(mode, RenderMode::TargetPageOnly),
                preserve_target_page_framebuffer: mode_preserves_target_framebuffer(mode),
                start_page: 0,
                start_cursor_y: 0,
            };
            let finish = paginator.feed(self, &mut observer, finish_config, PaginationEvent::End)?;
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
            let progress_percent = if epub.is_complete() {
                percent_from_pages(rendered_page, cached_pages)
            } else if let (Some(chapter_index), Some(chapter_start_page)) =
                (target_chapter_index, target_chapter_start_page)
            {
                percent_from_chapter_pages(
                    rendered_page,
                    chapter_start_page,
                    current_page,
                    cursor_y,
                    chapter_index,
                    epub.spine_count(),
                )
            } else {
                percent_from_spine(epub.next_spine_index(), epub.spine_count())
            };
            Ok(CacheBuildResult {
                rendered_page,
                cached_pages,
                next_spine_index: epub.next_spine_index(),
                spine_count: epub.spine_count(),
                resume_page: current_page,
                resume_cursor_y: cursor_y,
                progress_percent,
                complete: epub.is_complete(),
            })
        })
    }
}

fn percent_from_pages(rendered_page: usize, total_pages: usize) -> u8 {
    if total_pages == 0 {
        return 0;
    }
    let progress = ((rendered_page.saturating_add(1)) * 100) / total_pages.max(1);
    progress.clamp(1, 100) as u8
}

fn percent_from_spine(next_spine_index: u16, spine_count: u16) -> u8 {
    if spine_count == 0 {
        return 0;
    }
    let progress = (u32::from(next_spine_index).saturating_mul(99)) / u32::from(spine_count);
    progress.clamp(1, 99) as u8
}

fn percent_from_chapter_pages(
    rendered_page: usize,
    chapter_start_page: usize,
    current_page: usize,
    cursor_y: u16,
    chapter_index: u16,
    spine_count: u16,
) -> u8 {
    if spine_count == 0 {
        return 0;
    }
    let chapter_pages = if cursor_y > 0 {
        current_page.saturating_sub(chapter_start_page).saturating_add(1)
    } else {
        current_page.saturating_sub(chapter_start_page).max(1)
    };
    let page_in_chapter = rendered_page
        .saturating_sub(chapter_start_page)
        .saturating_add(1)
        .min(chapter_pages);
    let numerator = usize::from(chapter_index)
        .saturating_mul(chapter_pages)
        .saturating_add(page_in_chapter);
    let denominator = usize::from(spine_count).saturating_mul(chapter_pages).max(1);
    let progress = numerator.saturating_mul(100) / denominator;
    progress.clamp(1, 99) as u8
}
