use crate::{
    bookerly,
    text::{TextBuffer, WrappedTextLayoutResult},
};
use xteink_epub::EpubError;

pub(crate) trait PaginationRenderer {
    fn clear_to_white(&mut self);
    fn draw_wrapped_text_block(
        &mut self,
        x: u16,
        y: u16,
        text: &str,
        max_y: u16,
    ) -> WrappedTextLayoutResult;
    fn measure_wrapped_text_block(
        &mut self,
        x: u16,
        y: u16,
        text: &str,
        max_y: u16,
    ) -> WrappedTextLayoutResult;
    fn display_height(&self) -> u16;
}

pub(crate) trait PaginationObserver {
    fn on_text(&mut self, _text: &str) -> Result<(), EpubError> {
        Ok(())
    }

    fn on_line_break(&mut self) -> Result<(), EpubError> {
        Ok(())
    }

    fn on_paragraph_break(&mut self) -> Result<(), EpubError> {
        Ok(())
    }

    fn on_page_break(&mut self) -> Result<(), EpubError> {
        Ok(())
    }
}

pub(crate) struct NoopPaginationObserver;

impl PaginationObserver for NoopPaginationObserver {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PaginationEvent<'a> {
    Text(&'a str),
    LineBreak,
    ParagraphBreak,
    ExplicitPageBreak,
    EnableExplicitBreaks,
    End,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PendingAction {
    None,
    LineBreak,
    ParagraphBreak,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PaginationConfig {
    pub(crate) target_page: usize,
    pub(crate) draw_target_page: bool,
    pub(crate) stop_after_target_page: bool,
    pub(crate) preserve_target_page_framebuffer: bool,
    pub(crate) start_page: usize,
    pub(crate) start_cursor_y: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PaginationProgress {
    pub(crate) current_page: usize,
    pub(crate) cursor_y: u16,
    pub(crate) target_complete: bool,
}

pub(crate) struct PaginatorState<const N: usize> {
    text: TextBuffer<N>,
    current_page: usize,
    cursor_y: u16,
    page_has_content: bool,
    pending_action: PendingAction,
    explicit_page_breaks: bool,
}

impl<const N: usize> PaginatorState<N> {
    pub(crate) const fn new(config: PaginationConfig) -> Self {
        Self {
            text: TextBuffer::new(),
            current_page: config.start_page,
            cursor_y: config.start_cursor_y,
            page_has_content: config.start_cursor_y > 0,
            pending_action: PendingAction::None,
            explicit_page_breaks: false,
        }
    }

    pub(crate) fn current_page(&self) -> usize {
        self.current_page
    }

    pub(crate) fn cursor_y(&self) -> u16 {
        self.cursor_y
    }

    pub(crate) fn feed<R, O>(
        &mut self,
        renderer: &mut R,
        observer: &mut O,
        config: PaginationConfig,
        event: PaginationEvent<'_>,
    ) -> Result<PaginationProgress, EpubError>
    where
        R: PaginationRenderer,
        O: PaginationObserver,
    {
        let mut target_complete = false;
        match event {
            PaginationEvent::EnableExplicitBreaks => {
                self.explicit_page_breaks = true;
            }
            PaginationEvent::Text(text) => {
                target_complete = self.push_text(renderer, observer, config, text)?;
            }
            PaginationEvent::LineBreak => {
                let page_before_flush = self.current_page;
                target_complete = self.flush_text(renderer, observer, config)?;
                let break_absorbed_by_page_advance =
                    self.current_page != page_before_flush && self.cursor_y == 0;
                if !target_complete && !break_absorbed_by_page_advance {
                    self.pending_action = PendingAction::LineBreak;
                    target_complete = self.apply_pending_action(renderer, observer, config)?;
                }
            }
            PaginationEvent::ParagraphBreak => {
                let page_before_flush = self.current_page;
                target_complete = self.flush_text(renderer, observer, config)?;
                let break_absorbed_by_page_advance =
                    self.current_page != page_before_flush && self.cursor_y == 0;
                if !target_complete && !break_absorbed_by_page_advance {
                    self.pending_action = PendingAction::ParagraphBreak;
                    target_complete = self.apply_pending_action(renderer, observer, config)?;
                }
            }
            PaginationEvent::ExplicitPageBreak => {
                target_complete = self.flush_text(renderer, observer, config)?;
                if !target_complete {
                    self.explicit_page_breaks = true;
                    target_complete = self.advance_page(renderer, observer, config)?;
                }
            }
            PaginationEvent::End => {
                target_complete = self.flush_text(renderer, observer, config)?;
                if !target_complete {
                    target_complete = self.apply_pending_action(renderer, observer, config)?;
                }
            }
        }

        Ok(PaginationProgress {
            current_page: self.current_page,
            cursor_y: self.cursor_y,
            target_complete,
        })
    }

    fn push_text<R, O>(
        &mut self,
        renderer: &mut R,
        observer: &mut O,
        config: PaginationConfig,
        text: &str,
    ) -> Result<bool, EpubError>
    where
        R: PaginationRenderer,
        O: PaginationObserver,
    {
        let bytes = text.as_bytes();
        let mut start = 0usize;
        while start < bytes.len() {
            let remaining = self.text.buf.len().saturating_sub(self.text.len);
            if remaining == 0 {
                if self.flush_text(renderer, observer, config)? {
                    return Ok(true);
                }
                continue;
            }
            let take = remaining.min(bytes.len() - start);
            self.text.push_exact(core::str::from_utf8(&bytes[start..start + take]).unwrap_or(""));
            start += take;
        }
        Ok(false)
    }

    fn flush_text<R, O>(
        &mut self,
        renderer: &mut R,
        observer: &mut O,
        config: PaginationConfig,
    ) -> Result<bool, EpubError>
    where
        R: PaginationRenderer,
        O: PaginationObserver,
    {
        while !self.text.is_empty() {
            let current_text = self.text.as_str();
            let cursor_before = self.cursor_y;
            let result = if self.current_page >= config.target_page && config.draw_target_page {
                renderer.draw_wrapped_text_block(0, self.cursor_y, current_text, renderer.display_height())
            } else {
                renderer.measure_wrapped_text_block(0, self.cursor_y, current_text, renderer.display_height())
            };
            if result.consumed > 0 {
                observer.on_text(&current_text[..result.consumed])?;
            }
            self.text.advance(result.consumed);
            self.cursor_y = result.next_y;
            if self.cursor_y > cursor_before {
                self.page_has_content = true;
            }
            if !self.text.is_empty() {
                if self.advance_page(renderer, observer, config)? {
                    return Ok(true);
                }
            } else if !self.explicit_page_breaks && self.cursor_y >= renderer.display_height() {
                if self.advance_page(renderer, observer, config)? {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    fn apply_pending_action<R, O>(
        &mut self,
        renderer: &mut R,
        observer: &mut O,
        config: PaginationConfig,
    ) -> Result<bool, EpubError>
    where
        R: PaginationRenderer,
        O: PaginationObserver,
    {
        if !self.page_has_content {
            self.pending_action = PendingAction::None;
            return Ok(false);
        }
        match self.pending_action {
            PendingAction::None => {}
            PendingAction::LineBreak => {
                self.cursor_y = self.cursor_y.saturating_add(bookerly::BOOKERLY.line_height_px());
                observer.on_line_break()?;
            }
            PendingAction::ParagraphBreak => {
                self.cursor_y = self
                    .cursor_y
                    .saturating_add(bookerly::BOOKERLY.line_height_px() / 2);
                observer.on_paragraph_break()?;
            }
        }
        self.pending_action = PendingAction::None;

        if !self.explicit_page_breaks && self.cursor_y >= renderer.display_height() {
            return self.advance_page(renderer, observer, config);
        }
        Ok(false)
    }

    fn advance_page<R, O>(
        &mut self,
        renderer: &mut R,
        observer: &mut O,
        config: PaginationConfig,
    ) -> Result<bool, EpubError>
    where
        R: PaginationRenderer,
        O: PaginationObserver,
    {
        if self.current_page >= config.target_page {
            if config.stop_after_target_page {
                return Ok(true);
            }
            observer.on_page_break()?;
            self.current_page = self.current_page.saturating_add(1);
            self.cursor_y = 0;
            self.page_has_content = false;
            if !config.preserve_target_page_framebuffer {
                renderer.clear_to_white();
            }
            return Ok(true);
        }
        observer.on_page_break()?;
        self.current_page = self.current_page.saturating_add(1);
        self.cursor_y = 0;
        self.page_has_content = false;
        renderer.clear_to_white();
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::WrappedTextLayoutResult;
    use std::vec;

    #[derive(Default)]
    struct FakeRenderer {
        cleared: usize,
    }

    impl PaginationRenderer for FakeRenderer {
        fn clear_to_white(&mut self) {
            self.cleared += 1;
        }

        fn draw_wrapped_text_block(
            &mut self,
            _x: u16,
            y: u16,
            text: &str,
            _max_y: u16,
        ) -> WrappedTextLayoutResult {
            WrappedTextLayoutResult {
                next_y: y.saturating_add(bookerly::BOOKERLY.line_height_px()),
                consumed: text.len(),
            }
        }

        fn measure_wrapped_text_block(
            &mut self,
            _x: u16,
            y: u16,
            text: &str,
            _max_y: u16,
        ) -> WrappedTextLayoutResult {
            WrappedTextLayoutResult {
                next_y: y.saturating_add(bookerly::BOOKERLY.line_height_px()),
                consumed: text.len(),
            }
        }

        fn display_height(&self) -> u16 {
            bookerly::BOOKERLY.line_height_px()
        }
    }

    #[derive(Default)]
    struct RecordingObserver {
        events: std::vec::Vec<&'static str>,
    }

    impl PaginationObserver for RecordingObserver {
        fn on_text(&mut self, _text: &str) -> Result<(), EpubError> {
            self.events.push("text");
            Ok(())
        }

        fn on_line_break(&mut self) -> Result<(), EpubError> {
            self.events.push("line");
            Ok(())
        }

        fn on_paragraph_break(&mut self) -> Result<(), EpubError> {
            self.events.push("paragraph");
            Ok(())
        }

        fn on_page_break(&mut self) -> Result<(), EpubError> {
            self.events.push("page");
            Ok(())
        }
    }

    #[test]
    fn line_break_that_overflows_page_does_not_emit_extra_break_marker() {
        let mut renderer = FakeRenderer::default();
        let mut observer = RecordingObserver::default();
        let mut state = PaginatorState::<64>::new(PaginationConfig {
            target_page: 10,
            draw_target_page: false,
            stop_after_target_page: false,
            preserve_target_page_framebuffer: false,
            start_page: 0,
            start_cursor_y: 0,
        });

        let config = PaginationConfig {
            target_page: 10,
            draw_target_page: false,
            stop_after_target_page: false,
            preserve_target_page_framebuffer: false,
            start_page: 0,
            start_cursor_y: 0,
        };

        let _ = state
            .feed(&mut renderer, &mut observer, config, PaginationEvent::Text("hello"))
            .expect("text should succeed");
        let _ = state
            .feed(&mut renderer, &mut observer, config, PaginationEvent::LineBreak)
            .expect("line break should succeed");

        assert_eq!(observer.events, vec!["text", "page"]);
        assert_eq!(state.current_page(), 1);
        assert_eq!(state.cursor_y(), 0);
    }

    #[test]
    fn leading_paragraph_break_does_not_advance_blank_page() {
        let mut renderer = FakeRenderer::default();
        let mut observer = RecordingObserver::default();
        let mut state = PaginatorState::<64>::new(PaginationConfig {
            target_page: 0,
            draw_target_page: true,
            stop_after_target_page: true,
            preserve_target_page_framebuffer: false,
            start_page: 0,
            start_cursor_y: 0,
        });

        let config = PaginationConfig {
            target_page: 0,
            draw_target_page: true,
            stop_after_target_page: true,
            preserve_target_page_framebuffer: false,
            start_page: 0,
            start_cursor_y: 0,
        };

        let progress = state
            .feed(
                &mut renderer,
                &mut observer,
                config,
                PaginationEvent::ParagraphBreak,
            )
            .expect("paragraph break should succeed");

        assert!(!progress.target_complete);
        assert!(observer.events.is_empty());
        assert_eq!(state.current_page(), 0);
        assert_eq!(state.cursor_y(), 0);
    }
}
