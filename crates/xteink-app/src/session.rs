#[path = "session_ui.rs"]
mod session_ui;

use heapless::{String, Vec};
use xteink_browser::EntryKind;
use xteink_buttons::Button;
use xteink_controller::{
    AppController, BrowserRefresh, ControllerCommand, DirectoryPageInfo as ControllerPageInfo,
    ScreenMode,
};

use session_ui::SessionUi;

const PATH_CAPACITY: usize = 256;
const MAX_ENTRIES: usize = 24;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedEntry {
    pub label: String<96>,
    pub fs_name: String<96>,
    pub kind: EntryKind,
}

impl ListedEntry {
    pub fn epub(name: &str) -> Self {
        let mut label = String::new();
        let mut fs_name = String::new();
        let _ = label.push_str(name);
        let _ = fs_name.push_str(name);
        Self {
            label,
            fs_name,
            kind: EntryKind::Epub,
        }
    }

    pub fn directory(name: &str) -> Self {
        let mut label = String::new();
        let mut fs_name = String::new();
        let _ = label.push_str(name);
        let _ = fs_name.push_str(name);
        Self {
            label,
            fs_name,
            kind: EntryKind::Directory,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectoryPageInfo {
    pub page_start: usize,
    pub has_prev: bool,
    pub has_next: bool,
}

#[derive(Debug, Clone)]
pub struct DirectoryPage {
    pub entries: Vec<ListedEntry, MAX_ENTRIES>,
    pub info: DirectoryPageInfo,
}

pub trait AppRenderer {
    fn clear(&mut self, color: u8);
    fn draw_text(&mut self, x: u16, y: u16, text: &str);
}

impl AppRenderer for xteink_render::Framebuffer {
    fn clear(&mut self, color: u8) {
        xteink_render::Framebuffer::clear(self, color);
    }

    fn draw_text(&mut self, x: u16, y: u16, text: &str) {
        xteink_render::Framebuffer::draw_text(self, x, y, text);
    }
}

pub trait AppStorage<R: AppRenderer> {
    type Error;

    fn list_directory_page(
        &self,
        path: &str,
        page_start: usize,
        page_size: usize,
    ) -> Result<DirectoryPage, Self::Error>;
    fn render_epub_from_entry(
        &self,
        renderer: &mut R,
        current_path: &str,
        entry: &ListedEntry,
    ) -> Result<usize, Self::Error>;
    fn render_epub_page_from_entry(
        &self,
        renderer: &mut R,
        current_path: &str,
        entry: &ListedEntry,
        target_page: usize,
    ) -> Result<usize, Self::Error>;
}

pub struct Session<S, R> {
    storage: S,
    renderer: R,
    ui: SessionUi,
    controller: AppController,
    current_path: String<PATH_CAPACITY>,
    page: DirectoryPage,
    page_size: usize,
}

impl<S, R> Session<S, R>
where
    S: AppStorage<R>,
    R: AppRenderer,
{
    pub fn new(storage: S, renderer: R, page_size: usize) -> Self {
        let mut current_path = String::new();
        let _ = current_path.push('/');
        Self {
            storage,
            renderer,
            ui: SessionUi::new(),
            controller: AppController::new(page_size),
            current_path,
            page_size,
            page: DirectoryPage {
                entries: Vec::new(),
                info: DirectoryPageInfo {
                    page_start: 0,
                    has_prev: false,
                    has_next: false,
                },
            },
        }
    }

    pub fn bootstrap(&mut self) -> Result<BrowserRefresh, S::Error> {
        self.load_directory(0, 0, BrowserRefresh::Fast)
    }

    pub fn handle_button(&mut self, button: Button) -> Result<Option<BrowserRefresh>, S::Error> {
        let selected_entry = self
            .controller
            .browser()
            .selected_index(self.page.entries.len())
            .and_then(|index| self.page.entries.get(index))
            .map(|entry| self.ui.listed_entry_to_ui(entry));
        let command = self.controller.handle_button_with_selected_entry(
            button,
            self.page.entries.len(),
            self.controller_page_info(),
            selected_entry,
        );
        match command {
            ControllerCommand::None => Ok(None),
            ControllerCommand::RenderBrowser { refresh } => {
                self.ui.render_browser(
                    &mut self.renderer,
                    self.current_path.as_str(),
                    self.page.entries.as_slice(),
                    self.controller
                        .browser()
                        .selected_index(self.page.entries.len()),
                );
                Ok(Some(refresh))
            }
            ControllerCommand::LoadDirectory {
                path,
                page_start,
                selected,
                refresh,
                ..
            } => {
                self.current_path = path;
                self.load_directory(page_start, selected, refresh).map(Some)
            }
            ControllerCommand::OpenEpub { entry, .. } => {
                let listed = self.ui.ui_entry_to_listed(&entry);
                let rendered = self.storage.render_epub_from_entry(
                    &mut self.renderer,
                    self.current_path.as_str(),
                    &listed,
                )?;
                self.controller.apply_epub_opened(rendered);
                Ok(Some(BrowserRefresh::Fast))
            }
            ControllerCommand::RenderReaderPage {
                entry,
                target_page,
                fast,
                ..
            } => {
                let listed = self.ui.ui_entry_to_listed(&entry);
                let rendered = self.storage.render_epub_page_from_entry(
                    &mut self.renderer,
                    self.current_path.as_str(),
                    &listed,
                    target_page,
                )?;
                self.controller.apply_reader_page_rendered(rendered);
                Ok(Some(if fast {
                    BrowserRefresh::Fast
                } else {
                    BrowserRefresh::Full
                }))
            }
        }
    }

    pub fn renderer(&self) -> &R {
        &self.renderer
    }

    pub fn renderer_mut(&mut self) -> &mut R {
        &mut self.renderer
    }

    pub fn screen_mode(&self) -> ScreenMode {
        self.controller.screen_mode()
    }

    pub fn reader_page(&self) -> usize {
        self.controller.reader_page()
    }

    pub fn current_entries(&self) -> &[ListedEntry] {
        self.page.entries.as_slice()
    }

    pub fn current_path(&self) -> &str {
        self.current_path.as_str()
    }

    fn load_directory(
        &mut self,
        page_start: usize,
        selected: usize,
        refresh: BrowserRefresh,
    ) -> Result<BrowserRefresh, S::Error> {
        self.page = self.storage.list_directory_page(
            self.current_path.as_str(),
            page_start,
            self.page_size,
        )?;
        self.controller.apply_directory_loaded(
            self.page.info.page_start,
            self.page.entries.len(),
            selected,
        );
        self.ui.render_browser(
            &mut self.renderer,
            self.current_path.as_str(),
            self.page.entries.as_slice(),
            self.controller
                .browser()
                .selected_index(self.page.entries.len()),
        );
        Ok(refresh)
    }

    fn controller_page_info(&self) -> ControllerPageInfo {
        ControllerPageInfo {
            page_start: self.page.info.page_start,
            has_prev: self.page.info.has_prev,
            has_next: self.page.info.has_next,
        }
    }
}
