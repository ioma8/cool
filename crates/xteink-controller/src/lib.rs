#![cfg_attr(not(test), no_std)]

#[cfg(test)]
extern crate std;

use heapless::String;
use xteink_browser::{EntryKind, Input as BrowserInput, PagedAction, PagedBrowser};
use xteink_buttons::Button as RawButton;

pub const PATH_CAPACITY: usize = 256;
pub const ENTRY_NAME_CAPACITY: usize = 96;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiEntry {
    pub name: String<ENTRY_NAME_CAPACITY>,
    pub kind: EntryKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectoryPageInfo {
    pub page_start: usize,
    pub has_prev: bool,
    pub has_next: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenMode {
    Browse,
    Reading,
    Toc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserRefresh {
    Full,
    Fast,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControllerCommand {
    None,
    RenderBrowser {
        refresh: BrowserRefresh,
    },
    LoadDirectory {
        path: String<PATH_CAPACITY>,
        page_start: usize,
        selected: usize,
        refresh: BrowserRefresh,
    },
    OpenEpub {
        path: String<PATH_CAPACITY>,
        entry: UiEntry,
    },
    RenderReaderPage {
        path: String<PATH_CAPACITY>,
        entry: UiEntry,
        target_page: usize,
        fast: bool,
    },
    LoadToc {
        path: String<PATH_CAPACITY>,
        entry: UiEntry,
        page_start: usize,
        selected: usize,
        refresh: BrowserRefresh,
    },
    RenderToc {
        refresh: BrowserRefresh,
    },
    JumpToChapter {
        path: String<PATH_CAPACITY>,
        entry: UiEntry,
        chapter_index: usize,
    },
}

#[derive(Debug, Clone)]
pub struct AppController {
    page_size: usize,
    browser: PagedBrowser,
    current_path: String<PATH_CAPACITY>,
    screen_mode: ScreenMode,
    reader_entry: Option<UiEntry>,
    reader_page: usize,
    reader_chapter_index: usize,
    toc_browser: PagedBrowser,
}

impl AppController {
    pub fn new(page_size: usize) -> Self {
        let mut current_path = String::new();
        let _ = current_path.push('/');
        Self {
            page_size,
            browser: PagedBrowser::new(page_size),
            current_path,
            screen_mode: ScreenMode::Browse,
            reader_entry: None,
            reader_page: 0,
            reader_chapter_index: 0,
            toc_browser: PagedBrowser::new(page_size),
        }
    }

    pub fn current_path(&self) -> &str {
        self.current_path.as_str()
    }

    pub fn screen_mode(&self) -> ScreenMode {
        self.screen_mode
    }

    pub fn browser(&self) -> &PagedBrowser {
        &self.browser
    }

    pub fn toc_browser(&self) -> &PagedBrowser {
        &self.toc_browser
    }

    pub fn reader_page(&self) -> usize {
        self.reader_page
    }

    pub fn apply_epub_opened(&mut self, rendered_page: usize) {
        self.screen_mode = ScreenMode::Reading;
        self.reader_page = rendered_page;
    }

    pub fn apply_reader_chapter_changed(&mut self, chapter_index: usize) {
        self.reader_chapter_index = chapter_index;
    }

    pub fn apply_toc_loaded(&mut self, page_start: usize, page_len: usize, selected: usize) {
        self.toc_browser.set_page(page_start, page_len, selected);
    }

    pub fn apply_directory_loaded(&mut self, page_start: usize, page_len: usize, selected: usize) {
        self.browser.set_page(page_start, page_len, selected);
    }

    pub fn apply_reader_page_rendered(&mut self, rendered_page: usize) {
        self.reader_page = rendered_page;
    }

    pub fn handle_button(
        &mut self,
        button: RawButton,
        page_entries: &[UiEntry],
        page_info: DirectoryPageInfo,
    ) -> ControllerCommand {
        let selected_entry = self
            .browser
            .selected_index(page_entries.len())
            .and_then(|index| page_entries.get(index))
            .cloned();
        self.handle_button_with_selected_entry(
            button,
            page_entries.len(),
            page_info,
            selected_entry,
        )
    }

    pub fn handle_button_with_selected_entry(
        &mut self,
        button: RawButton,
        page_len: usize,
        page_info: DirectoryPageInfo,
        selected_entry: Option<UiEntry>,
    ) -> ControllerCommand {
        match (self.screen_mode, button) {
            (ScreenMode::Browse, RawButton::Left | RawButton::Up) => {
                self.handle_browse_navigation(BrowserInput::Left, page_len, page_info)
            }
            (ScreenMode::Browse, RawButton::Right | RawButton::Down) => {
                self.handle_browse_navigation(BrowserInput::Right, page_len, page_info)
            }
            (ScreenMode::Browse, RawButton::Back) => {
                let Some(entry) = selected_entry else {
                    return ControllerCommand::None;
                };

                match entry.kind {
                    EntryKind::Directory => {
                        let Ok(next_path) =
                            join_child_path(self.current_path.as_str(), entry.name.as_str())
                        else {
                            return ControllerCommand::None;
                        };
                        self.current_path = next_path.clone();
                        self.browser = PagedBrowser::new(self.page_size);
                        ControllerCommand::LoadDirectory {
                            path: next_path,
                            page_start: 0,
                            selected: 0,
                            refresh: BrowserRefresh::Fast,
                        }
                    }
                    EntryKind::Epub => {
                        self.reader_entry = Some(entry.clone());
                        ControllerCommand::OpenEpub {
                            path: self.current_path.clone(),
                            entry,
                        }
                    }
                    EntryKind::Other => ControllerCommand::None,
                }
            }
            (ScreenMode::Browse, RawButton::Confirm) => {
                let Some(parent) = parent_path(self.current_path.as_str()) else {
                    return ControllerCommand::None;
                };
                self.current_path = parent.clone();
                self.browser = PagedBrowser::new(self.page_size);
                ControllerCommand::LoadDirectory {
                    path: parent,
                    page_start: 0,
                    selected: 0,
                    refresh: BrowserRefresh::Fast,
                }
            }
            (ScreenMode::Reading, RawButton::Right | RawButton::Down) => {
                let Some(entry) = self.reader_entry.clone() else {
                    return ControllerCommand::None;
                };
                self.reader_page = self.reader_page.saturating_add(1);
                ControllerCommand::RenderReaderPage {
                    path: self.current_path.clone(),
                    entry,
                    target_page: self.reader_page,
                    fast: true,
                }
            }
            (ScreenMode::Reading, RawButton::Left | RawButton::Up) => {
                let Some(entry) = self.reader_entry.clone() else {
                    return ControllerCommand::None;
                };
                self.reader_page = self.reader_page.saturating_sub(1);
                ControllerCommand::RenderReaderPage {
                    path: self.current_path.clone(),
                    entry,
                    target_page: self.reader_page,
                    fast: true,
                }
            }
            (ScreenMode::Reading, RawButton::Back) => {
                self.screen_mode = ScreenMode::Browse;
                ControllerCommand::RenderBrowser {
                    refresh: BrowserRefresh::Fast,
                }
            }
            (ScreenMode::Reading, RawButton::Confirm) => {
                let Some(entry) = self.reader_entry.clone() else {
                    return ControllerCommand::None;
                };
                self.screen_mode = ScreenMode::Toc;
                ControllerCommand::LoadToc {
                    path: self.current_path.clone(),
                    entry,
                    page_start: (self.reader_chapter_index / self.page_size) * self.page_size,
                    selected: self.reader_chapter_index % self.page_size,
                    refresh: BrowserRefresh::Fast,
                }
            }
            (ScreenMode::Toc, RawButton::Left | RawButton::Up) => {
                self.handle_toc_navigation(BrowserInput::Left, page_len, page_info)
            }
            (ScreenMode::Toc, RawButton::Right | RawButton::Down) => {
                self.handle_toc_navigation(BrowserInput::Right, page_len, page_info)
            }
            (ScreenMode::Toc, RawButton::Back) => {
                self.screen_mode = ScreenMode::Reading;
                ControllerCommand::RenderReaderPage {
                    path: self.current_path.clone(),
                    entry: self.reader_entry.clone().unwrap_or_else(|| UiEntry {
                        name: String::new(),
                        kind: EntryKind::Epub,
                    }),
                    target_page: self.reader_page,
                    fast: true,
                }
            }
            (ScreenMode::Toc, RawButton::Confirm) => {
                let Some(entry) = self.reader_entry.clone() else {
                    return ControllerCommand::None;
                };
                let Some(selected) = self.toc_browser.selected_index(page_len) else {
                    return ControllerCommand::None;
                };
                self.screen_mode = ScreenMode::Reading;
                self.reader_chapter_index = page_info.page_start.saturating_add(selected);
                ControllerCommand::JumpToChapter {
                    path: self.current_path.clone(),
                    entry,
                    chapter_index: self.reader_chapter_index,
                }
            }
            _ => ControllerCommand::None,
        }
    }
}

impl AppController {
    fn handle_browse_navigation(
        &mut self,
        input: BrowserInput,
        page_len: usize,
        page_info: DirectoryPageInfo,
    ) -> ControllerCommand {
        match self
            .browser
            .handle(input, page_len, page_info.has_prev, page_info.has_next)
        {
            PagedAction::None => ControllerCommand::None,
            PagedAction::Redraw => ControllerCommand::RenderBrowser {
                refresh: BrowserRefresh::Fast,
            },
            PagedAction::LoadPage {
                page_start,
                selected,
            } => ControllerCommand::LoadDirectory {
                path: self.current_path.clone(),
                page_start,
                selected,
                refresh: BrowserRefresh::Fast,
            },
            PagedAction::OpenSelected(_) => ControllerCommand::None,
        }
    }

    fn handle_toc_navigation(
        &mut self,
        input: BrowserInput,
        page_len: usize,
        page_info: DirectoryPageInfo,
    ) -> ControllerCommand {
        match self
            .toc_browser
            .handle(input, page_len, page_info.has_prev, page_info.has_next)
        {
            PagedAction::None => ControllerCommand::None,
            PagedAction::Redraw => ControllerCommand::RenderToc {
                refresh: BrowserRefresh::Fast,
            },
            PagedAction::LoadPage {
                page_start,
                selected,
            } => ControllerCommand::LoadToc {
                path: self.current_path.clone(),
                entry: self.reader_entry.clone().unwrap_or_else(|| UiEntry {
                    name: String::new(),
                    kind: EntryKind::Epub,
                }),
                page_start,
                selected,
                refresh: BrowserRefresh::Fast,
            },
            PagedAction::OpenSelected(index) => {
                let Some(entry) = self.reader_entry.clone() else {
                    return ControllerCommand::None;
                };
                self.screen_mode = ScreenMode::Reading;
                self.reader_chapter_index = index;
                ControllerCommand::JumpToChapter {
                    path: self.current_path.clone(),
                    entry,
                    chapter_index: index,
                }
            }
        }
    }
}

fn join_child_path(parent: &str, child: &str) -> Result<String<PATH_CAPACITY>, ()> {
    let mut path = String::<PATH_CAPACITY>::new();
    let parent = parent.trim_end_matches('/');

    if parent.is_empty() {
        path.push('/').map_err(|_| ())?;
    } else {
        path.push_str(parent).map_err(|_| ())?;
        path.push('/').map_err(|_| ())?;
    }

    path.push_str(child.trim_start_matches('/'))
        .map_err(|_| ())?;
    Ok(path)
}

fn parent_path(path: &str) -> Option<String<PATH_CAPACITY>> {
    if path == "/" {
        return None;
    }

    let mut next = String::<PATH_CAPACITY>::new();
    if let Some((prefix, _)) = path.rsplit_once('/') {
        if prefix.is_empty() {
            let _ = next.push('/');
        } else {
            let _ = next.push_str(prefix);
        }
        return Some(next);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn directory_entry(name: &str) -> UiEntry {
        entry(name, EntryKind::Directory)
    }

    fn epub_entry(name: &str) -> UiEntry {
        entry(name, EntryKind::Epub)
    }

    fn entry(name: &str, kind: EntryKind) -> UiEntry {
        let mut entry_name = String::new();
        let _ = entry_name.push_str(name);
        UiEntry {
            name: entry_name,
            kind,
        }
    }

    #[test]
    fn browse_back_button_on_directory_requests_full_reload_for_child_path() {
        let mut controller = AppController::new(7);
        let entries = [directory_entry("books")];

        let command = controller.handle_button(
            RawButton::Back,
            &entries,
            DirectoryPageInfo {
                page_start: 0,
                has_prev: false,
                has_next: false,
            },
        );

        assert_eq!(
            command,
            ControllerCommand::LoadDirectory {
                path: String::try_from("/books").unwrap(),
                page_start: 0,
                selected: 0,
                refresh: BrowserRefresh::Fast,
            }
        );
        assert_eq!(controller.current_path(), "/books");
        assert_eq!(controller.screen_mode(), ScreenMode::Browse);
        assert_eq!(controller.browser().page_start(), 0);
        assert_eq!(controller.browser().selected_index(1), Some(0));
    }

    #[test]
    fn confirm_button_moves_to_parent_directory_with_full_reload() {
        let mut controller = AppController::new(7);
        controller.current_path = String::try_from("/books/fiction").unwrap();

        let command = controller.handle_button(
            RawButton::Confirm,
            &[],
            DirectoryPageInfo {
                page_start: 0,
                has_prev: false,
                has_next: false,
            },
        );

        assert_eq!(
            command,
            ControllerCommand::LoadDirectory {
                path: String::try_from("/books").unwrap(),
                page_start: 0,
                selected: 0,
                refresh: BrowserRefresh::Fast,
            }
        );
        assert_eq!(controller.current_path(), "/books");
    }

    #[test]
    fn back_button_on_epub_requests_open_epub_and_preserves_browse_mode_until_result() {
        let mut controller = AppController::new(7);
        let entries = [epub_entry("novel.epub")];

        let command = controller.handle_button(
            RawButton::Back,
            &entries,
            DirectoryPageInfo {
                page_start: 0,
                has_prev: false,
                has_next: false,
            },
        );

        assert_eq!(
            command,
            ControllerCommand::OpenEpub {
                path: String::try_from("/").unwrap(),
                entry: epub_entry("novel.epub"),
            }
        );
        assert_eq!(controller.screen_mode(), ScreenMode::Browse);
    }

    #[test]
    fn apply_epub_opened_switches_to_reading_and_tracks_rendered_page() {
        let mut controller = AppController::new(7);
        let entries = [epub_entry("novel.epub")];
        let _ = controller.handle_button(
            RawButton::Back,
            &entries,
            DirectoryPageInfo {
                page_start: 0,
                has_prev: false,
                has_next: false,
            },
        );

        controller.apply_epub_opened(3);

        assert_eq!(controller.screen_mode(), ScreenMode::Reading);
        assert_eq!(controller.reader_page(), 3);
    }

    #[test]
    fn reading_right_requests_next_page_render() {
        let mut controller = AppController::new(7);
        let entries = [epub_entry("novel.epub")];
        let _ = controller.handle_button(
            RawButton::Back,
            &entries,
            DirectoryPageInfo {
                page_start: 0,
                has_prev: false,
                has_next: false,
            },
        );
        controller.apply_epub_opened(3);

        let command = controller.handle_button(
            RawButton::Right,
            &[],
            DirectoryPageInfo {
                page_start: 0,
                has_prev: false,
                has_next: false,
            },
        );

        assert_eq!(
            command,
            ControllerCommand::RenderReaderPage {
                path: String::try_from("/").unwrap(),
                entry: epub_entry("novel.epub"),
                target_page: 4,
                fast: true,
            }
        );
        assert_eq!(controller.reader_page(), 4);
    }

    #[test]
    fn reading_back_returns_to_browser_with_full_refresh() {
        let mut controller = AppController::new(7);
        let entries = [epub_entry("novel.epub")];
        let _ = controller.handle_button(
            RawButton::Back,
            &entries,
            DirectoryPageInfo {
                page_start: 0,
                has_prev: false,
                has_next: false,
            },
        );
        controller.apply_epub_opened(1);

        let command = controller.handle_button(
            RawButton::Back,
            &[],
            DirectoryPageInfo {
                page_start: 0,
                has_prev: false,
                has_next: false,
            },
        );

        assert_eq!(
            command,
            ControllerCommand::RenderBrowser {
                refresh: BrowserRefresh::Fast,
            }
        );
        assert_eq!(controller.screen_mode(), ScreenMode::Browse);
    }

    #[test]
    fn browse_right_moves_selection_and_requests_fast_render() {
        let mut controller = AppController::new(3);
        let entries = [directory_entry("books"), epub_entry("novel.epub")];

        let command = controller.handle_button(
            RawButton::Right,
            &entries,
            DirectoryPageInfo {
                page_start: 0,
                has_prev: false,
                has_next: false,
            },
        );

        assert_eq!(
            command,
            ControllerCommand::RenderBrowser {
                refresh: BrowserRefresh::Fast,
            }
        );
        assert_eq!(controller.browser().selected_index(entries.len()), Some(1));
    }

    #[test]
    fn browse_right_at_page_end_requests_next_page_load() {
        let mut controller = AppController::new(3);
        let entries = [
            directory_entry("books"),
            epub_entry("novel.epub"),
            directory_entry("notes"),
        ];
        let _ = controller.handle_button(
            RawButton::Right,
            &entries,
            DirectoryPageInfo {
                page_start: 0,
                has_prev: false,
                has_next: true,
            },
        );
        let _ = controller.handle_button(
            RawButton::Right,
            &entries,
            DirectoryPageInfo {
                page_start: 0,
                has_prev: false,
                has_next: true,
            },
        );

        let command = controller.handle_button(
            RawButton::Right,
            &entries,
            DirectoryPageInfo {
                page_start: 0,
                has_prev: false,
                has_next: true,
            },
        );

        assert_eq!(
            command,
            ControllerCommand::LoadDirectory {
                path: String::try_from("/").unwrap(),
                page_start: 3,
                selected: 0,
                refresh: BrowserRefresh::Fast,
            }
        );
    }

    #[test]
    fn reading_confirm_opens_toc_at_current_chapter() {
        let mut controller = AppController::new(3);
        let entries = [epub_entry("novel.epub")];
        let _ = controller.handle_button(
            RawButton::Back,
            &entries,
            DirectoryPageInfo {
                page_start: 0,
                has_prev: false,
                has_next: false,
            },
        );
        controller.apply_epub_opened(5);
        controller.apply_reader_chapter_changed(4);

        let command = controller.handle_button(
            RawButton::Confirm,
            &[],
            DirectoryPageInfo {
                page_start: 0,
                has_prev: false,
                has_next: false,
            },
        );

        assert_eq!(
            command,
            ControllerCommand::LoadToc {
                path: String::try_from("/").unwrap(),
                entry: epub_entry("novel.epub"),
                page_start: 3,
                selected: 1,
                refresh: BrowserRefresh::Fast,
            }
        );
        assert_eq!(controller.screen_mode(), ScreenMode::Toc);
    }

    #[test]
    fn toc_confirm_jumps_to_selected_chapter_and_returns_to_reader() {
        let mut controller = AppController::new(3);
        let entries = [epub_entry("novel.epub")];
        let _ = controller.handle_button(
            RawButton::Back,
            &entries,
            DirectoryPageInfo {
                page_start: 0,
                has_prev: false,
                has_next: false,
            },
        );
        controller.apply_epub_opened(0);
        controller.apply_reader_chapter_changed(0);
        let _ = controller.handle_button(
            RawButton::Confirm,
            &[],
            DirectoryPageInfo {
                page_start: 0,
                has_prev: false,
                has_next: false,
            },
        );
        controller.apply_toc_loaded(0, 3, 0);

        let command = controller.handle_button(
            RawButton::Right,
            &[
                epub_entry("Cover"),
                epub_entry("Introduction"),
                epub_entry("One"),
            ],
            DirectoryPageInfo {
                page_start: 0,
                has_prev: false,
                has_next: false,
            },
        );

        assert_eq!(
            command,
            ControllerCommand::RenderToc {
                refresh: BrowserRefresh::Fast,
            }
        );

        let command = controller.handle_button(
            RawButton::Confirm,
            &[
                epub_entry("Cover"),
                epub_entry("Introduction"),
                epub_entry("One"),
            ],
            DirectoryPageInfo {
                page_start: 0,
                has_prev: false,
                has_next: false,
            },
        );

        assert_eq!(
            command,
            ControllerCommand::JumpToChapter {
                path: String::try_from("/").unwrap(),
                entry: epub_entry("novel.epub"),
                chapter_index: 1,
            }
        );
        assert_eq!(controller.screen_mode(), ScreenMode::Reading);
    }
}
