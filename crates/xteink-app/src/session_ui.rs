use heapless::String;
use xteink_browser::EntryKind;
use xteink_controller::UiEntry;
use xteink_render::bookerly;

use super::{AppRenderer, ListedEntry};

pub struct SessionUi;

impl SessionUi {
    pub const fn new() -> Self {
        Self
    }

    pub fn render_browser<R: AppRenderer>(
        &self,
        renderer: &mut R,
        current_path: &str,
        entries: &[ListedEntry],
        selected_index: Option<usize>,
    ) {
        renderer.clear(0xFF);
        renderer.draw_text(4, 4, current_path);

        let line_height = bookerly::BOOKERLY.line_height_px();
        let mut cursor_y = 4 + line_height * 2;
        for (index, entry) in entries.iter().enumerate() {
            if cursor_y.saturating_add(line_height) > xteink_render::DISPLAY_HEIGHT {
                break;
            }

            let mut line = String::<96>::new();
            let _ = line.push(if selected_index == Some(index) { '>' } else { ' ' });
            let _ = line.push(' ');
            let _ = line.push_str(match entry.kind {
                EntryKind::Directory => "[D] ",
                EntryKind::Epub => "[E] ",
                EntryKind::Other => "[ ] ",
            });
            let _ = line.push_str(entry.label.as_str());
            renderer.draw_text(4, cursor_y, line.as_str());
            cursor_y = cursor_y.saturating_add(line_height);
        }
    }

    pub fn ui_entry_to_listed(&self, entry: &UiEntry) -> ListedEntry {
        let mut label = String::new();
        let mut fs_name = String::new();
        let _ = label.push_str(entry.name.as_str());
        let _ = fs_name.push_str(entry.name.as_str());
        ListedEntry {
            label,
            fs_name,
            kind: entry.kind,
        }
    }

    pub fn listed_entry_to_ui(&self, entry: &ListedEntry) -> UiEntry {
        let mut name = String::new();
        let _ = name.push_str(entry.fs_name.as_str());
        UiEntry {
            name,
            kind: entry.kind,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xteink_browser::EntryKind;

    #[derive(Default)]
    struct RecordingRenderer {
        clears: Vec<u8, 4>,
        texts: Vec<(u16, u16, String<128>), 8>,
    }

    impl AppRenderer for RecordingRenderer {
        fn clear(&mut self, color: u8) {
            let _ = self.clears.push(color);
        }

        fn draw_text(&mut self, x: u16, y: u16, text: &str) {
            let mut text = String::new();
            let _ = text.push_str(text);
            let _ = self.texts.push((x, y, text));
        }
    }

    #[test]
    fn entry_conversion_uses_fs_name_for_controller_entries() {
        let ui = SessionUi::new();
        let mut fs_name = String::new();
        let _ = fs_name.push_str("book.epub");
        let entry = ListedEntry {
            label: String::from("Book"),
            fs_name,
            kind: EntryKind::Epub,
        };

        let converted = ui.listed_entry_to_ui(&entry);

        assert_eq!(converted.name.as_str(), "book.epub");
        assert_eq!(converted.kind, EntryKind::Epub);
    }

    #[test]
    fn render_browser_draws_selection_marker_and_entry_label() {
        let ui = SessionUi::new();
        let mut renderer = RecordingRenderer::default();
        let entries = [ListedEntry::directory("Books"), ListedEntry::epub("book.epub")];

        ui.render_browser(&mut renderer, "/", &entries, Some(1));

        assert_eq!(renderer.clears, heapless::Vec::from_slice(&[0xFF]).unwrap());
        assert!(renderer
            .texts
            .iter()
            .any(|(_, _, text)| text.as_str().contains("[D] Books")));
        assert!(renderer
            .texts
            .iter()
            .any(|(_, _, text)| text.as_str().contains("> [E] book.epub")));
    }
}
