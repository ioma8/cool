#![cfg_attr(not(test), no_std)]

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Directory,
    Epub,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry<'a> {
    pub name: &'a str,
    pub kind: EntryKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    None,
    Selected(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PagedAction {
    None,
    Redraw,
    LoadPage { page_start: usize, selected: usize },
    OpenSelected(usize),
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PagedBrowser {
    page_start: usize,
    selected: usize,
    page_size: usize,
}

impl PagedBrowser {
    pub const fn new(page_size: usize) -> Self {
        Self {
            page_start: 0,
            selected: 0,
            page_size,
        }
    }

    pub fn selected_index(&self, len: usize) -> Option<usize> {
        if len == 0 {
            None
        } else {
            Some(self.selected.min(len - 1))
        }
    }

    pub fn page_start(&self) -> usize {
        self.page_start
    }

    pub fn set_page(&mut self, page_start: usize, page_len: usize, selected: usize) {
        self.page_start = page_start;
        self.selected = selected.min(page_len.saturating_sub(1));
    }

    pub fn handle(&mut self, input: Input, page_len: usize, has_prev: bool, has_next: bool) -> PagedAction {
        if page_len == 0 {
            return PagedAction::None;
        }

        match input {
            Input::Left => {
                if self.selected > 0 {
                    self.selected -= 1;
                    PagedAction::Redraw
                } else if has_prev {
                    self.page_start = self.page_start.saturating_sub(self.page_size);
                    self.selected = self.page_size.saturating_sub(1);
                    PagedAction::LoadPage {
                        page_start: self.page_start,
                        selected: self.selected,
                    }
                } else {
                    PagedAction::None
                }
            }
            Input::Right => {
                if self.selected + 1 < page_len {
                    self.selected += 1;
                    PagedAction::Redraw
                } else if has_next {
                    self.page_start = self.page_start.saturating_add(self.page_size);
                    self.selected = 0;
                    PagedAction::LoadPage {
                        page_start: self.page_start,
                        selected: self.selected,
                    }
                } else {
                    PagedAction::None
                }
            }
            Input::Up => {
                if has_prev {
                    self.page_start = self.page_start.saturating_sub(self.page_size);
                    self.selected = self.page_size.saturating_sub(1);
                    PagedAction::LoadPage {
                        page_start: self.page_start,
                        selected: self.selected,
                    }
                } else {
                    PagedAction::None
                }
            }
            Input::Down => PagedAction::OpenSelected(self.page_start + self.selected),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Browser {
    selected: usize,
}

impl Browser {
    pub const fn new() -> Self {
        Self { selected: 0 }
    }

    pub fn selected_index(&self, len: usize) -> Option<usize> {
        if len == 0 {
            None
        } else {
            Some(self.selected.min(len - 1))
        }
    }

    pub fn selected_entry<'a>(&self, entries: &'a [Entry<'a>]) -> Option<&'a Entry<'a>> {
        self.selected_index(entries.len()).and_then(|index| entries.get(index))
    }

    pub fn handle(&mut self, input: Input, len: usize) -> Action {
        match input {
            Input::Left => {
                self.move_left(len);
                Action::None
            }
            Input::Right => {
                self.move_right(len);
                Action::None
            }
            Input::Up => Action::None,
            Input::Down => self
                .selected_index(len)
                .map(Action::Selected)
                .unwrap_or(Action::None),
        }
    }

    fn move_left(&mut self, len: usize) {
        if len == 0 || self.selected == 0 {
            return;
        }
        self.selected -= 1;
    }

    fn move_right(&mut self, len: usize) {
        if len == 0 || self.selected + 1 >= len {
            return;
        }
        self.selected += 1;
    }
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use super::*;

    const ENTRIES: &[Entry<'_>] = &[
        Entry {
            name: "books",
            kind: EntryKind::Directory,
        },
        Entry {
            name: "novel.epub",
            kind: EntryKind::Epub,
        },
        Entry {
            name: "notes.txt",
            kind: EntryKind::Other,
        },
    ];

    #[test]
    fn left_and_right_stay_within_bounds() {
        let mut browser = Browser::new();

        assert_eq!(browser.selected_index(ENTRIES.len()), Some(0));
        assert_eq!(browser.handle(Input::Left, ENTRIES.len()), Action::None);
        assert_eq!(browser.selected_index(ENTRIES.len()), Some(0));

        assert_eq!(browser.handle(Input::Right, ENTRIES.len()), Action::None);
        assert_eq!(browser.selected_index(ENTRIES.len()), Some(1));
        assert_eq!(browser.handle(Input::Right, ENTRIES.len()), Action::None);
        assert_eq!(browser.selected_index(ENTRIES.len()), Some(2));
        assert_eq!(browser.handle(Input::Right, ENTRIES.len()), Action::None);
        assert_eq!(browser.selected_index(ENTRIES.len()), Some(2));
    }

    #[test]
    fn down_returns_the_selected_entry() {
        let mut browser = Browser::new();

        assert_eq!(browser.handle(Input::Down, ENTRIES.len()), Action::Selected(0));
        assert_eq!(browser.handle(Input::Right, ENTRIES.len()), Action::None);
        assert_eq!(browser.handle(Input::Down, ENTRIES.len()), Action::Selected(1));
    }

    #[test]
    fn empty_entries_are_safe() {
        let mut browser = Browser::new();

        assert_eq!(browser.selected_index(0), None);
        assert_eq!(browser.selected_entry(&[]), None);
        assert_eq!(browser.handle(Input::Left, 0), Action::None);
        assert_eq!(browser.handle(Input::Right, 0), Action::None);
        assert_eq!(browser.handle(Input::Down, 0), Action::None);
    }

    #[test]
    fn paged_browser_moves_within_page_and_pages_when_needed() {
        let mut browser = PagedBrowser::new(3);
        browser.set_page(0, 3, 0);

        assert_eq!(browser.handle(Input::Right, 3, false, true), PagedAction::Redraw);
        assert_eq!(browser.selected_index(3), Some(1));
        assert_eq!(browser.handle(Input::Right, 3, false, true), PagedAction::Redraw);
        assert_eq!(browser.selected_index(3), Some(2));
        assert_eq!(
            browser.handle(Input::Right, 3, false, true),
            PagedAction::LoadPage {
                page_start: 3,
                selected: 0
            }
        );
    }

    #[test]
    fn paged_browser_pages_back_when_moving_left_from_top() {
        let mut browser = PagedBrowser::new(3);
        browser.set_page(3, 3, 0);

        assert_eq!(
            browser.handle(Input::Left, 3, true, true),
            PagedAction::LoadPage {
                page_start: 0,
                selected: 2
            }
        );
    }

    #[test]
    fn paged_browser_pages_up_with_up_button() {
        let mut browser = PagedBrowser::new(3);
        browser.set_page(3, 3, 1);

        assert_eq!(
            browser.handle(Input::Up, 3, true, true),
            PagedAction::LoadPage {
                page_start: 0,
                selected: 2
            }
        );
    }

    #[test]
    fn paged_browser_opens_selected_global_index() {
        let mut browser = PagedBrowser::new(3);
        browser.set_page(6, 3, 1);

        assert_eq!(browser.handle(Input::Down, 3, true, true), PagedAction::OpenSelected(7));
    }

    #[test]
    fn entry_kinds_are_distinguishable() {
        assert_eq!(ENTRIES[0].kind, EntryKind::Directory);
        assert_eq!(ENTRIES[1].kind, EntryKind::Epub);
        assert_eq!(ENTRIES[2].kind, EntryKind::Other);
    }
}
