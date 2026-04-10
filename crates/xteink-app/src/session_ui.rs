use heapless::String;
use xteink_browser::EntryKind;
use xteink_controller::UiEntry;

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
        self.render_menu(renderer, current_path, entries, selected_index, true);
    }

    pub fn render_toc<R: AppRenderer>(
        &self,
        renderer: &mut R,
        entries: &[ListedEntry],
        selected_index: Option<usize>,
    ) {
        self.render_menu(
            renderer,
            "Table of contents",
            entries,
            selected_index,
            false,
        );
    }

    pub fn render_reader_footer<R: AppRenderer>(
        &self,
        renderer: &mut R,
        _chapter_number: Option<usize>,
        chapter_title: Option<&str>,
        progress_percent: u8,
    ) {
        let footer_height = xteink_render::reader_footer_height();
        let footer_y = xteink_render::reader_content_height();
        renderer.fill_rect(
            0,
            footer_y,
            xteink_render::DISPLAY_WIDTH,
            footer_height,
            0xFF,
        );

        let mut right = String::<16>::new();
        let _ = core::fmt::write(&mut right, format_args!("{}%", progress_percent));
        let layout = xteink_render::layout_reader_footer(chapter_title, None, right.as_str());

        if let Some(text) = layout.left_text {
            renderer.draw_footer_text(layout.left_x, footer_y.saturating_add(4), text);
        }
        renderer.draw_footer_text(
            layout.right_x,
            footer_y.saturating_add(4),
            layout.right_text,
        );
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

    fn render_menu<R: AppRenderer>(
        &self,
        renderer: &mut R,
        title: &str,
        entries: &[ListedEntry],
        selected_index: Option<usize>,
        show_kind: bool,
    ) {
        renderer.clear(0xFF);
        renderer.draw_heading_text(4, 4, title);

        let line_height = xteink_render::body_line_height_px();
        let mut cursor_y = 4 + xteink_render::heading_line_height_px() + line_height;
        for (index, entry) in entries.iter().enumerate() {
            if cursor_y.saturating_add(line_height) > xteink_render::DISPLAY_HEIGHT {
                break;
            }

            let mut line = String::<96>::new();
            let _ = line.push(if selected_index == Some(index) {
                '>'
            } else {
                ' '
            });
            let _ = line.push(' ');
            if show_kind {
                let _ = line.push_str(match entry.kind {
                    EntryKind::Directory => "[D] ",
                    EntryKind::Epub => "[E] ",
                    EntryKind::Other => "[ ] ",
                });
            }
            let _ = line.push_str(entry.label.as_str());
            renderer.draw_text(4, cursor_y, line.as_str());
            cursor_y = cursor_y.saturating_add(line_height);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use heapless::Vec;
    use xteink_browser::EntryKind;

    #[derive(Default)]
    struct RecordingRenderer {
        clears: Vec<u8, 4>,
        fills: Vec<(u16, u16, u16, u16, u8), 4>,
        texts: Vec<(u16, u16, String<128>), 8>,
        heading_texts: Vec<(u16, u16, String<128>), 4>,
        footer_texts: Vec<(u16, u16, String<128>), 4>,
    }

    impl AppRenderer for RecordingRenderer {
        fn clear(&mut self, color: u8) {
            let _ = self.clears.push(color);
        }

        fn fill_rect(&mut self, x: u16, y: u16, width: u16, height: u16, color: u8) {
            let _ = self.fills.push((x, y, width, height, color));
        }

        fn draw_text(&mut self, x: u16, y: u16, text: &str) {
            let mut rendered_text = String::new();
            let _ = rendered_text.push_str(text);
            let _ = self.texts.push((x, y, rendered_text));
        }

        fn draw_heading_text(&mut self, x: u16, y: u16, text: &str) {
            let mut rendered_text = String::new();
            let _ = rendered_text.push_str(text);
            let _ = self.heading_texts.push((x, y, rendered_text));
        }

        fn draw_footer_text(&mut self, x: u16, y: u16, text: &str) {
            let mut rendered_text = String::new();
            let _ = rendered_text.push_str(text);
            let _ = self.footer_texts.push((x, y, rendered_text));
        }
    }

    #[test]
    fn entry_conversion_uses_fs_name_for_controller_entries() {
        let ui = SessionUi::new();
        let mut label = String::new();
        let _ = label.push_str("Book");
        let mut fs_name = String::new();
        let _ = fs_name.push_str("book.epub");
        let entry = ListedEntry {
            label,
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
        let entries = [
            ListedEntry::directory("Books"),
            ListedEntry::epub("book.epub"),
        ];

        ui.render_browser(&mut renderer, "/", &entries, Some(1));

        assert_eq!(
            renderer.clears,
            heapless::Vec::<u8, 4>::from_slice(&[0xFF]).unwrap()
        );
        assert!(
            renderer
                .heading_texts
                .iter()
                .any(|(_, _, text)| text.as_str() == "/")
        );
        assert!(
            renderer
                .texts
                .iter()
                .any(|(_, _, text)| text.as_str().contains("[D] Books"))
        );
        assert!(
            renderer
                .texts
                .iter()
                .any(|(_, _, text)| text.as_str().contains("> [E] book.epub"))
        );
    }

    #[test]
    fn render_reader_footer_draws_title_and_progress() {
        let ui = SessionUi::new();
        let mut renderer = RecordingRenderer::default();

        ui.render_reader_footer(
            &mut renderer,
            Some(12),
            Some("A Very Long Chapter Title For Footer Layout"),
            42,
        );

        assert_eq!(renderer.fills.len(), 1);
        assert!(
            renderer
                .footer_texts
                .iter()
                .any(|(_, _, text)| text.as_str() == "42%")
        );
        let left = renderer
            .footer_texts
            .iter()
            .find(|(_, _, text)| text.as_str().starts_with("A Very Long"))
            .expect("left chapter title");
        let right = renderer
            .footer_texts
            .iter()
            .find(|(_, _, text)| text.as_str() == "42%")
            .expect("right progress");

        assert_eq!(left.0, 4);
        assert!(right.0 > left.0);
    }

    #[test]
    fn render_reader_footer_renders_title_when_progress_leaves_room() {
        let ui = SessionUi::new();
        let mut renderer = RecordingRenderer::default();

        ui.render_reader_footer(&mut renderer, Some(123_456_789), Some("Introduction"), 100);

        assert!(
            renderer
                .footer_texts
                .iter()
                .any(|(_, _, text)| text.as_str() == "100%")
        );
        assert!(
            renderer
                .footer_texts
                .iter()
                .any(|(_, _, text)| text.as_str() == "Introduction")
        );
    }

    #[test]
    fn render_toc_draws_header_and_selected_chapter_title() {
        let ui = SessionUi::new();
        let mut renderer = RecordingRenderer::default();
        let entries = [
            ListedEntry::epub("Cover"),
            ListedEntry::epub("Introduction"),
        ];

        ui.render_toc(&mut renderer, &entries, Some(1));

        assert!(
            renderer
                .heading_texts
                .iter()
                .any(|(_, _, text)| text.as_str() == "Table of contents")
        );
        assert!(
            renderer
                .texts
                .iter()
                .any(|(_, _, text)| text.as_str() == "> Introduction")
        );
    }
}
