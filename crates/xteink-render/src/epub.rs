use core::mem::MaybeUninit;

use miniz_oxide::inflate::stream::InflateState;
use xteink_epub::{
    Epub, EpubArchive, EpubError, EpubEvent, EpubSource, MAX_ARCHIVE_ENTRIES,
    MAX_ARCHIVE_NAME_CAPACITY, ReaderBuffers,
};
use xteink_memory::SHARED_RENDER_EPUB_WORKSPACE_LIMIT_BYTES;

use crate::{DISPLAY_HEIGHT, Framebuffer, bookerly, text::TextBuffer};

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

fn with_epub_render_workspace<R>(f: impl FnOnce(&mut EpubRenderWorkspace) -> R) -> R {
    #[cfg(target_arch = "riscv32")]
    {
        critical_section::with(|_| {
            let workspace = init_epub_render_workspace();
            f(workspace)
        })
    }

    #[cfg(not(target_arch = "riscv32"))]
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
        with_epub_render_workspace(|workspace| {
            let workspace_ptr = core::ptr::addr_of_mut!(*workspace);
            let mut epub = Epub::open(source)?;
            let mut text = TextBuffer::<TEXT_LEN>::new();
            let mut cursor_y = 0u16;
            let mut current_page = 0usize;
            let mut render_enabled = true;
            let line_height = bookerly::BOOKERLY.line_height_px();

            self.clear(0xFF);

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
                match event {
                    EpubEvent::Text(chunk) => {
                        if render_enabled {
                            text.push(chunk);
                        }
                        on_text_chunk(chunk)?;
                    }
                    EpubEvent::LineBreak => {
                        if render_enabled {
                            cursor_y = self.flush_text_buffer(&mut text, cursor_y);
                            cursor_y = cursor_y.saturating_add(line_height);
                        }
                        on_text_chunk("\n")?;
                    }
                    EpubEvent::ParagraphStart | EpubEvent::HeadingStart(_) => {}
                    EpubEvent::ParagraphEnd | EpubEvent::HeadingEnd => {
                        if render_enabled {
                            cursor_y = self.flush_text_buffer(&mut text, cursor_y);
                            cursor_y = cursor_y.saturating_add(line_height / 2);
                        }
                        on_text_chunk("\n")?;
                    }
                    EpubEvent::Image { alt, .. } => {
                        if let Some(alt) = alt {
                            if render_enabled {
                                text.push(alt);
                                cursor_y = self.flush_text_buffer(&mut text, cursor_y);
                                cursor_y = cursor_y.saturating_add(line_height);
                            }
                            on_text_chunk(alt)?;
                            on_text_chunk("\n")?;
                        }
                    }
                    EpubEvent::UnsupportedTag => {}
                }

                if cursor_y >= DISPLAY_HEIGHT {
                    if render_enabled && current_page >= target_page {
                        if parse_full_book {
                            render_enabled = false;
                            cursor_y = 0;
                            text.clear();
                            continue;
                        }
                        break;
                    }
                    current_page = current_page.saturating_add(1);
                    cursor_y = 0;
                    text.clear();
                    if render_enabled {
                        self.clear(0xFF);
                    }
                }
            }

            if !text.is_empty() {
                on_text_chunk(text.as_str())?;
                text.clear();
            }
            if render_enabled {
                let _ = self.flush_text_buffer(&mut text, cursor_y);
            }
            Ok(current_page)
        })
    }

    fn flush_text_buffer<const N: usize>(
        &mut self,
        buffer: &mut TextBuffer<N>,
        cursor_y: u16,
    ) -> u16 {
        if buffer.is_empty() {
            return cursor_y;
        }
        let next_y = self.draw_wrapped_text(0, cursor_y, buffer.as_str(), DISPLAY_HEIGHT);
        buffer.clear();
        next_y
    }
}
