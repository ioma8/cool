use std::cell::RefCell;
use xteink_app::{
    AppStorage, DirectoryPage, DirectoryPageInfo, EpubRenderResult, ListedEntry, Session,
};
use xteink_browser::EntryKind;
use xteink_buttons::Button;
use xteink_render::Framebuffer;

struct FakeStorage;

impl AppStorage<Framebuffer> for FakeStorage {
    type Error = ();

    fn list_directory_page(
        &self,
        _path: &str,
        _page_start: usize,
        _page_size: usize,
    ) -> Result<DirectoryPage, Self::Error> {
        Ok(DirectoryPage {
            entries: heapless::Vec::from_slice(&[ListedEntry::epub("book.epub")]).expect("entry"),
            info: DirectoryPageInfo {
                page_start: 0,
                has_prev: false,
                has_next: false,
            },
        })
    }

    fn render_epub_from_entry(
        &self,
        _framebuffer: &mut xteink_render::Framebuffer,
        _current_path: &str,
        _entry: &ListedEntry,
    ) -> Result<EpubRenderResult, Self::Error> {
        Ok(EpubRenderResult {
            rendered_page: 0,
            progress_percent: 25,
            chapter_number: None,
            chapter_title: None,
        })
    }

    fn render_epub_page_from_entry(
        &self,
        _framebuffer: &mut xteink_render::Framebuffer,
        _current_path: &str,
        _entry: &ListedEntry,
        target_page: usize,
    ) -> Result<EpubRenderResult, Self::Error> {
        Ok(EpubRenderResult {
            rendered_page: target_page,
            progress_percent: 25,
            chapter_number: None,
            chapter_title: None,
        })
    }

    fn render_epub_next_page_from_entry(
        &self,
        framebuffer: &mut xteink_render::Framebuffer,
        current_path: &str,
        entry: &ListedEntry,
        target_page: usize,
    ) -> Result<EpubRenderResult, Self::Error> {
        self.render_epub_page_from_entry(framebuffer, current_path, entry, target_page)
    }

    fn render_epub_previous_page_from_entry(
        &self,
        framebuffer: &mut xteink_render::Framebuffer,
        current_path: &str,
        entry: &ListedEntry,
        target_page: usize,
    ) -> Result<EpubRenderResult, Self::Error> {
        self.render_epub_page_from_entry(framebuffer, current_path, entry, target_page)
    }

    fn list_epub_chapter_page(
        &self,
        _current_path: &str,
        _entry: &ListedEntry,
        _page_start: usize,
        _page_size: usize,
    ) -> Result<DirectoryPage, Self::Error> {
        Ok(DirectoryPage {
            entries: heapless::Vec::from_slice(&[
                ListedEntry::epub("Cover"),
                ListedEntry::epub("Introduction"),
            ])
            .expect("chapters"),
            info: DirectoryPageInfo {
                page_start: 0,
                has_prev: false,
                has_next: false,
            },
        })
    }

    fn render_epub_chapter_from_entry(
        &self,
        _framebuffer: &mut xteink_render::Framebuffer,
        _current_path: &str,
        _entry: &ListedEntry,
        chapter_index: usize,
    ) -> Result<EpubRenderResult, Self::Error> {
        Ok(EpubRenderResult {
            rendered_page: chapter_index.saturating_mul(10),
            progress_percent: 25,
            chapter_number: Some(chapter_index.saturating_add(1)),
            chapter_title: None,
        })
    }
}

struct MultiStorage;

impl AppStorage<Framebuffer> for MultiStorage {
    type Error = ();

    fn list_directory_page(
        &self,
        path: &str,
        _page_start: usize,
        page_size: usize,
    ) -> Result<DirectoryPage, Self::Error> {
        let entries = if path == "/" {
            [
                ListedEntry::directory("Books"),
                ListedEntry::epub("a.epub"),
                ListedEntry::epub("b.epub"),
            ]
        } else {
            [
                ListedEntry::epub("inside.epub"),
                ListedEntry::epub("tail.epub"),
                ListedEntry::epub("extra.epub"),
            ]
        };
        let mut listed = heapless::Vec::new();
        for entry in entries.into_iter().take(page_size) {
            let _ = listed.push(entry);
        }
        Ok(DirectoryPage {
            entries: listed,
            info: DirectoryPageInfo {
                page_start: 0,
                has_prev: false,
                has_next: false,
            },
        })
    }

    fn render_epub_from_entry(
        &self,
        framebuffer: &mut xteink_render::Framebuffer,
        _current_path: &str,
        _entry: &ListedEntry,
    ) -> Result<EpubRenderResult, Self::Error> {
        framebuffer.draw_text(4, 4, "epub");
        Ok(EpubRenderResult {
            rendered_page: 0,
            progress_percent: 33,
            chapter_number: None,
            chapter_title: None,
        })
    }

    fn render_epub_page_from_entry(
        &self,
        _framebuffer: &mut xteink_render::Framebuffer,
        _current_path: &str,
        _entry: &ListedEntry,
        target_page: usize,
    ) -> Result<EpubRenderResult, Self::Error> {
        Ok(EpubRenderResult {
            rendered_page: target_page,
            progress_percent: 33,
            chapter_number: None,
            chapter_title: None,
        })
    }

    fn render_epub_next_page_from_entry(
        &self,
        framebuffer: &mut xteink_render::Framebuffer,
        current_path: &str,
        entry: &ListedEntry,
        target_page: usize,
    ) -> Result<EpubRenderResult, Self::Error> {
        self.render_epub_page_from_entry(framebuffer, current_path, entry, target_page)
    }

    fn render_epub_previous_page_from_entry(
        &self,
        framebuffer: &mut xteink_render::Framebuffer,
        current_path: &str,
        entry: &ListedEntry,
        target_page: usize,
    ) -> Result<EpubRenderResult, Self::Error> {
        self.render_epub_page_from_entry(framebuffer, current_path, entry, target_page)
    }

    fn list_epub_chapter_page(
        &self,
        _current_path: &str,
        _entry: &ListedEntry,
        _page_start: usize,
        page_size: usize,
    ) -> Result<DirectoryPage, Self::Error> {
        let entries = [
            ListedEntry::epub("Cover"),
            ListedEntry::epub("Introduction"),
            ListedEntry::epub("First chapter"),
        ];
        let mut listed = heapless::Vec::new();
        for entry in entries.into_iter().take(page_size) {
            let _ = listed.push(entry);
        }
        Ok(DirectoryPage {
            entries: listed,
            info: DirectoryPageInfo {
                page_start: 0,
                has_prev: false,
                has_next: false,
            },
        })
    }

    fn render_epub_chapter_from_entry(
        &self,
        framebuffer: &mut xteink_render::Framebuffer,
        _current_path: &str,
        _entry: &ListedEntry,
        chapter_index: usize,
    ) -> Result<EpubRenderResult, Self::Error> {
        framebuffer.draw_text(4, 24, "chapter jump");
        Ok(EpubRenderResult {
            rendered_page: chapter_index.saturating_mul(10),
            progress_percent: 33,
            chapter_number: Some(chapter_index.saturating_add(1)),
            chapter_title: None,
        })
    }
}

#[derive(Default)]
struct PagingStorage {
    full_page_calls: RefCell<Vec<usize>>,
    next_calls: RefCell<Vec<usize>>,
    previous_calls: RefCell<Vec<usize>>,
}

impl AppStorage<Framebuffer> for PagingStorage {
    type Error = ();

    fn list_directory_page(
        &self,
        _path: &str,
        _page_start: usize,
        _page_size: usize,
    ) -> Result<DirectoryPage, Self::Error> {
        Ok(DirectoryPage {
            entries: heapless::Vec::from_slice(&[ListedEntry::epub("book.epub")]).expect("entry"),
            info: DirectoryPageInfo {
                page_start: 0,
                has_prev: false,
                has_next: false,
            },
        })
    }

    fn render_epub_from_entry(
        &self,
        _framebuffer: &mut Framebuffer,
        _current_path: &str,
        _entry: &ListedEntry,
    ) -> Result<EpubRenderResult, Self::Error> {
        Ok(EpubRenderResult {
            rendered_page: 0,
            progress_percent: 0,
            chapter_number: None,
            chapter_title: None,
        })
    }

    fn render_epub_page_from_entry(
        &self,
        _framebuffer: &mut Framebuffer,
        _current_path: &str,
        _entry: &ListedEntry,
        target_page: usize,
    ) -> Result<EpubRenderResult, Self::Error> {
        self.full_page_calls.borrow_mut().push(target_page);
        Ok(EpubRenderResult {
            rendered_page: target_page,
            progress_percent: 0,
            chapter_number: None,
            chapter_title: None,
        })
    }

    fn render_epub_next_page_from_entry(
        &self,
        _framebuffer: &mut Framebuffer,
        _current_path: &str,
        _entry: &ListedEntry,
        target_page: usize,
    ) -> Result<EpubRenderResult, Self::Error> {
        self.next_calls.borrow_mut().push(target_page);
        Ok(EpubRenderResult {
            rendered_page: target_page,
            progress_percent: 0,
            chapter_number: None,
            chapter_title: None,
        })
    }

    fn render_epub_previous_page_from_entry(
        &self,
        _framebuffer: &mut Framebuffer,
        _current_path: &str,
        _entry: &ListedEntry,
        target_page: usize,
    ) -> Result<EpubRenderResult, Self::Error> {
        self.previous_calls.borrow_mut().push(target_page);
        Ok(EpubRenderResult {
            rendered_page: target_page,
            progress_percent: 0,
            chapter_number: None,
            chapter_title: None,
        })
    }

    fn list_epub_chapter_page(
        &self,
        _current_path: &str,
        _entry: &ListedEntry,
        _page_start: usize,
        _page_size: usize,
    ) -> Result<DirectoryPage, Self::Error> {
        Ok(DirectoryPage {
            entries: heapless::Vec::new(),
            info: DirectoryPageInfo {
                page_start: 0,
                has_prev: false,
                has_next: false,
            },
        })
    }

    fn render_epub_chapter_from_entry(
        &self,
        _framebuffer: &mut Framebuffer,
        _current_path: &str,
        _entry: &ListedEntry,
        chapter_index: usize,
    ) -> Result<EpubRenderResult, Self::Error> {
        Ok(EpubRenderResult {
            rendered_page: chapter_index,
            progress_percent: 0,
            chapter_number: None,
            chapter_title: None,
        })
    }
}

#[test]
fn opening_an_epub_from_loaded_directory_transitions_to_reader_page_zero() {
    let mut session = Session::new(FakeStorage, Framebuffer::new(), 8);
    session.bootstrap().expect("bootstrap should work");

    session
        .handle_button(Button::Back)
        .expect("open should work");

    assert_eq!(
        session.screen_mode(),
        xteink_controller::ScreenMode::Reading
    );
    assert_eq!(session.reader_page(), 0);
    assert!(session.renderer().bytes().iter().any(|byte| *byte != 0xFF));
    assert_eq!(session.current_entries()[0].kind, EntryKind::Epub);
}

#[test]
fn opening_an_epub_draws_reader_progress_footer() {
    let mut session = Session::new(FakeStorage, Framebuffer::new(), 8);
    session.bootstrap().expect("bootstrap should work");

    session
        .handle_button(Button::Back)
        .expect("open should work");

    let non_white = session
        .renderer()
        .bytes()
        .iter()
        .filter(|byte| **byte != 0xFF)
        .count();
    assert!(non_white > 0);
}

#[test]
fn bootstrap_uses_configured_page_size_instead_of_single_entry() {
    let mut session = Session::new(MultiStorage, Framebuffer::new(), 8);

    session.bootstrap().expect("bootstrap should work");

    assert_eq!(session.current_entries().len(), 3);
}

#[test]
fn opening_directory_updates_current_path_and_loads_child_entries() {
    let mut session = Session::new(MultiStorage, Framebuffer::new(), 8);
    session.bootstrap().expect("bootstrap should work");

    session
        .handle_button(Button::Back)
        .expect("directory open should work");

    assert_eq!(session.current_path(), "/Books");
    assert_eq!(session.current_entries()[0].label.as_str(), "inside.epub");
}

#[test]
fn confirm_in_reader_opens_toc_entries() {
    let mut session = Session::new(FakeStorage, Framebuffer::new(), 8);
    session.bootstrap().expect("bootstrap should work");
    session
        .handle_button(Button::Back)
        .expect("open should work");

    session
        .handle_button(Button::Confirm)
        .expect("toc should open");

    assert_eq!(session.screen_mode(), xteink_controller::ScreenMode::Toc);
    assert_eq!(session.current_entries()[0].label.as_str(), "Cover");
    assert_eq!(session.current_entries()[1].label.as_str(), "Introduction");
}

#[test]
fn confirm_in_toc_jumps_to_selected_chapter_and_returns_to_reader() {
    let mut session = Session::new(FakeStorage, Framebuffer::new(), 8);
    session.bootstrap().expect("bootstrap should work");
    session
        .handle_button(Button::Back)
        .expect("open should work");
    session
        .handle_button(Button::Confirm)
        .expect("toc should open");

    session
        .handle_button(Button::Right)
        .expect("toc selection should move");
    session
        .handle_button(Button::Confirm)
        .expect("toc confirm should jump");

    assert_eq!(
        session.screen_mode(),
        xteink_controller::ScreenMode::Reading
    );
    assert_eq!(session.reader_page(), 10);
}

#[test]
fn down_in_reader_uses_next_page_storage_path() {
    let storage = PagingStorage::default();
    let mut session = Session::new(storage, Framebuffer::new(), 8);
    session.bootstrap().expect("bootstrap should work");
    session
        .handle_button(Button::Back)
        .expect("open should work");

    session
        .handle_button(Button::Down)
        .expect("next page should render");

    assert_eq!(session.reader_page(), 1);
    assert_eq!(session.storage().next_calls.borrow().as_slice(), &[1]);
    assert!(session.storage().full_page_calls.borrow().is_empty());
}

#[test]
fn opening_an_epub_draws_footer_with_footer_variant() {
    let mut session = Session::new(FakeStorage, Framebuffer::new(), 8);
    session.bootstrap().expect("bootstrap should work");

    session
        .handle_button(Button::Back)
        .expect("open should work");

    assert!(session.renderer().bytes().iter().any(|byte| *byte != 0xFF));
}
