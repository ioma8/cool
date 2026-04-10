use crate::text::{TextBuffer, WrappedTextLayoutResult};
use xteink_epub::EpubError;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TextStyle {
    pub(crate) heading: bool,
    pub(crate) bold: bool,
    pub(crate) italic: bool,
    pub(crate) quote: bool,
}

pub(crate) trait PaginationRenderer {
    fn clear_to_white(&mut self);
    fn draw_wrapped_text_block(
        &mut self,
        x: u16,
        y: u16,
        text: &str,
        max_y: u16,
        style: TextStyle,
    ) -> WrappedTextLayoutResult;
    fn measure_wrapped_text_block(
        &mut self,
        x: u16,
        y: u16,
        text: &str,
        max_y: u16,
        style: TextStyle,
    ) -> WrappedTextLayoutResult;
    fn display_height(&self) -> u16;
    fn line_height(&self, style: TextStyle) -> u16;
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

    fn on_heading_start(&mut self) -> Result<(), EpubError> {
        Ok(())
    }

    fn on_heading_end(&mut self) -> Result<(), EpubError> {
        Ok(())
    }

    fn on_bold_start(&mut self) -> Result<(), EpubError> {
        Ok(())
    }

    fn on_bold_end(&mut self) -> Result<(), EpubError> {
        Ok(())
    }

    fn on_italic_start(&mut self) -> Result<(), EpubError> {
        Ok(())
    }

    fn on_italic_end(&mut self) -> Result<(), EpubError> {
        Ok(())
    }

    fn on_quote_start(&mut self) -> Result<(), EpubError> {
        Ok(())
    }

    fn on_quote_end(&mut self) -> Result<(), EpubError> {
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
    HeadingStart,
    HeadingEnd,
    BoldStart,
    BoldEnd,
    ItalicStart,
    ItalicEnd,
    QuoteStart,
    QuoteEnd,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PaginationState {
    current_page: usize,
    cursor_y: u16,
    page_has_content: bool,
    pending_action: PendingAction,
    explicit_page_breaks: bool,
    style: TextStyle,
}

impl PaginationState {
    pub(crate) const fn new(config: PaginationConfig) -> Self {
        Self {
            current_page: config.start_page,
            cursor_y: config.start_cursor_y,
            page_has_content: config.start_cursor_y > 0,
            pending_action: PendingAction::None,
            explicit_page_breaks: false,
            style: TextStyle {
                heading: false,
                bold: false,
                italic: false,
                quote: false,
            },
        }
    }

    pub(crate) fn current_page(&self) -> usize {
        self.current_page
    }

    pub(crate) fn cursor_y(&self) -> u16 {
        self.cursor_y
    }

    pub(crate) fn page_has_content(&self) -> bool {
        self.page_has_content
    }

    pub(crate) fn enable_explicit_page_breaks(&mut self) {
        self.explicit_page_breaks = true;
    }

    pub(crate) fn style(&self) -> TextStyle {
        self.style
    }

    pub(crate) fn set_pending_action(&mut self, pending_action: PendingAction) {
        self.pending_action = pending_action;
    }

    pub(crate) fn pending_action(&self) -> PendingAction {
        self.pending_action
    }

    pub(crate) fn clear_pending_action(&mut self) {
        self.pending_action = PendingAction::None;
    }

    pub(crate) fn set_style(&mut self, style: TextStyle) {
        self.style = style;
    }

    pub(crate) fn advance_line_break(&mut self, line_height: u16) {
        self.cursor_y = self.cursor_y.saturating_add(line_height);
    }

    pub(crate) fn advance_paragraph_break(&mut self, line_height: u16) {
        self.cursor_y = self.cursor_y.saturating_add(line_height / 2);
    }

    pub(crate) fn set_cursor_y(&mut self, cursor_y: u16) {
        self.cursor_y = cursor_y;
    }

    pub(crate) fn mark_page_has_content(&mut self) {
        self.page_has_content = true;
    }

    pub(crate) fn current_page_is_target_or_later(&self, config: &PaginationConfig) -> bool {
        self.current_page >= config.target_page
    }

    pub(crate) fn should_advance_for_height(&self, renderer_height: u16) -> bool {
        !self.explicit_page_breaks && self.cursor_y >= renderer_height
    }

    pub(crate) fn advance_page(&mut self, config: PaginationConfig) -> PaginationAdvanceOutcome {
        if self.current_page >= config.target_page {
            if config.stop_after_target_page {
                return PaginationAdvanceOutcome {
                    target_complete: true,
                    clear_framebuffer: false,
                };
            }
            self.current_page = self.current_page.saturating_add(1);
            self.cursor_y = 0;
            self.page_has_content = false;
            return PaginationAdvanceOutcome {
                target_complete: true,
                clear_framebuffer: !config.preserve_target_page_framebuffer,
            };
        }

        self.current_page = self.current_page.saturating_add(1);
        self.cursor_y = 0;
        self.page_has_content = false;
        PaginationAdvanceOutcome {
            target_complete: false,
            clear_framebuffer: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PaginationAdvanceOutcome {
    pub(crate) target_complete: bool,
    pub(crate) clear_framebuffer: bool,
}

pub(crate) struct PaginatorState<const N: usize> {
    text: TextBuffer<N>,
    state: PaginationState,
}

impl<const N: usize> PaginatorState<N> {
    pub(crate) const fn new(config: PaginationConfig) -> Self {
        Self {
            text: TextBuffer::new(),
            state: PaginationState::new(config),
        }
    }

    pub(crate) fn current_page(&self) -> usize {
        self.state.current_page()
    }

    pub(crate) fn cursor_y(&self) -> u16 {
        self.state.cursor_y()
    }

    pub(crate) fn pending_output_bytes(&self) -> usize {
        self.text.len
            + match self.state.pending_action() {
                PendingAction::None => 0,
                PendingAction::LineBreak | PendingAction::ParagraphBreak => 1,
            }
    }

    pub(crate) fn has_visible_page_content_or_pending_output(&self) -> bool {
        self.state.page_has_content() || self.pending_output_bytes() > 0
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
                self.state.enable_explicit_page_breaks();
            }
            PaginationEvent::Text(text) => {
                target_complete = self.push_text(renderer, observer, config, text)?;
            }
            PaginationEvent::LineBreak => {
                let page_before_flush = self.state.current_page();
                target_complete = self.flush_text(renderer, observer, config)?;
                let break_absorbed_by_page_advance =
                    self.state.current_page() != page_before_flush && self.state.cursor_y() == 0;
                if !target_complete && !break_absorbed_by_page_advance {
                    self.state.set_pending_action(PendingAction::LineBreak);
                    target_complete = self.apply_pending_action(renderer, observer, config)?;
                }
            }
            PaginationEvent::ParagraphBreak => {
                let page_before_flush = self.state.current_page();
                target_complete = self.flush_text(renderer, observer, config)?;
                let break_absorbed_by_page_advance =
                    self.state.current_page() != page_before_flush && self.state.cursor_y() == 0;
                if !target_complete && !break_absorbed_by_page_advance {
                    self.state.set_pending_action(PendingAction::ParagraphBreak);
                    target_complete = self.apply_pending_action(renderer, observer, config)?;
                }
            }
            PaginationEvent::HeadingStart => {
                target_complete = self.transition_style(
                    renderer,
                    observer,
                    config,
                    |style| style.heading = true,
                    |observer| observer.on_heading_start(),
                )?;
            }
            PaginationEvent::HeadingEnd => {
                target_complete = self.transition_style(
                    renderer,
                    observer,
                    config,
                    |style| style.heading = false,
                    |observer| observer.on_heading_end(),
                )?;
            }
            PaginationEvent::BoldStart => {
                target_complete = self.transition_style(
                    renderer,
                    observer,
                    config,
                    |style| style.bold = true,
                    |observer| observer.on_bold_start(),
                )?;
            }
            PaginationEvent::BoldEnd => {
                target_complete = self.transition_style(
                    renderer,
                    observer,
                    config,
                    |style| style.bold = false,
                    |observer| observer.on_bold_end(),
                )?;
            }
            PaginationEvent::ItalicStart => {
                target_complete = self.transition_style(
                    renderer,
                    observer,
                    config,
                    |style| style.italic = true,
                    |observer| observer.on_italic_start(),
                )?;
            }
            PaginationEvent::ItalicEnd => {
                target_complete = self.transition_style(
                    renderer,
                    observer,
                    config,
                    |style| style.italic = false,
                    |observer| observer.on_italic_end(),
                )?;
            }
            PaginationEvent::QuoteStart => {
                target_complete = self.transition_style(
                    renderer,
                    observer,
                    config,
                    |style| style.quote = true,
                    |observer| observer.on_quote_start(),
                )?;
            }
            PaginationEvent::QuoteEnd => {
                target_complete = self.transition_style(
                    renderer,
                    observer,
                    config,
                    |style| style.quote = false,
                    |observer| observer.on_quote_end(),
                )?;
            }
            PaginationEvent::ExplicitPageBreak => {
                target_complete = self.flush_text(renderer, observer, config)?;
                if !target_complete {
                    self.state.enable_explicit_page_breaks();
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
            current_page: self.state.current_page(),
            cursor_y: self.state.cursor_y(),
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
            self.text
                .push_exact(core::str::from_utf8(&bytes[start..start + take]).unwrap_or(""));
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
            let cursor_before = self.state.cursor_y();
            let result =
                if self.state.current_page() >= config.target_page && config.draw_target_page {
                    renderer.draw_wrapped_text_block(
                        0,
                        self.state.cursor_y(),
                        current_text,
                        renderer.display_height(),
                        self.state.style(),
                    )
                } else {
                    renderer.measure_wrapped_text_block(
                        0,
                        self.state.cursor_y(),
                        current_text,
                        renderer.display_height(),
                        self.state.style(),
                    )
                };
            if result.consumed > 0 {
                observer.on_text(&current_text[..result.consumed])?;
            }
            self.text.advance(result.consumed);
            self.state.set_cursor_y(result.next_y);
            if self.state.cursor_y() > cursor_before {
                self.state.mark_page_has_content();
            }
            if !self.text.is_empty() {
                if self.advance_page(renderer, observer, config)? {
                    return Ok(true);
                }
            } else if self
                .state
                .should_advance_for_height(renderer.display_height())
            {
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
        if !self.state.page_has_content() {
            self.state.clear_pending_action();
            return Ok(false);
        }
        match self.state.pending_action() {
            PendingAction::None => {}
            PendingAction::LineBreak => {
                observer.on_line_break()?;
                self.state
                    .advance_line_break(renderer.line_height(self.state.style()));
            }
            PendingAction::ParagraphBreak => {
                observer.on_paragraph_break()?;
                self.state
                    .advance_paragraph_break(renderer.line_height(self.state.style()));
            }
        }
        self.state.clear_pending_action();

        if self
            .state
            .should_advance_for_height(renderer.display_height())
        {
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
        if self.state.current_page_is_target_or_later(&config) {
            if config.stop_after_target_page {
                return Ok(true);
            }
            observer.on_page_break()?;
            let outcome = self.state.advance_page(config);
            if !config.preserve_target_page_framebuffer {
                renderer.clear_to_white();
            }
            return Ok(outcome.target_complete);
        }
        observer.on_page_break()?;
        let outcome = self.state.advance_page(config);
        renderer.clear_to_white();
        Ok(outcome.target_complete)
    }

    fn transition_style<R, O>(
        &mut self,
        renderer: &mut R,
        observer: &mut O,
        config: PaginationConfig,
        mut update_style: impl FnMut(&mut TextStyle),
        notify: impl FnOnce(&mut O) -> Result<(), EpubError>,
    ) -> Result<bool, EpubError>
    where
        R: PaginationRenderer,
        O: PaginationObserver,
    {
        let target_complete = self.flush_text(renderer, observer, config)?;
        notify(observer)?;
        let mut style = self.state.style();
        update_style(&mut style);
        self.state.set_style(style);
        Ok(target_complete)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bookerly;
    use crate::text::WrappedTextLayoutResult;
    use std::vec;

    #[derive(Default)]
    struct FakeRenderer {
        cleared: usize,
        drawn_styles: std::vec::Vec<TextStyle>,
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
            style: TextStyle,
        ) -> WrappedTextLayoutResult {
            self.drawn_styles.push(style);
            WrappedTextLayoutResult {
                next_y: y.saturating_add(self.line_height(style)),
                consumed: text.len(),
            }
        }

        fn measure_wrapped_text_block(
            &mut self,
            _x: u16,
            y: u16,
            text: &str,
            _max_y: u16,
            style: TextStyle,
        ) -> WrappedTextLayoutResult {
            WrappedTextLayoutResult {
                next_y: y.saturating_add(self.line_height(style)),
                consumed: text.len(),
            }
        }

        fn display_height(&self) -> u16 {
            bookerly::BOOKERLY.line_height_px()
        }

        fn line_height(&self, style: TextStyle) -> u16 {
            if style.heading {
                bookerly::BOOKERLY_HEADING.line_height_px()
            } else {
                bookerly::BOOKERLY_BODY.line_height_px()
            }
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

        fn on_heading_start(&mut self) -> Result<(), EpubError> {
            self.events.push("heading-start");
            Ok(())
        }

        fn on_heading_end(&mut self) -> Result<(), EpubError> {
            self.events.push("heading-end");
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
            .feed(
                &mut renderer,
                &mut observer,
                config,
                PaginationEvent::Text("hello"),
            )
            .expect("text should succeed");
        let _ = state
            .feed(
                &mut renderer,
                &mut observer,
                config,
                PaginationEvent::LineBreak,
            )
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

    #[test]
    fn pagination_state_advances_without_renderer_side_effects() {
        let config = PaginationConfig {
            target_page: 1,
            draw_target_page: true,
            stop_after_target_page: true,
            preserve_target_page_framebuffer: false,
            start_page: 0,
            start_cursor_y: 0,
        };
        let mut state = PaginationState::new(config);

        state.mark_page_has_content();
        state.set_pending_action(PendingAction::LineBreak);
        state.advance_line_break(bookerly::BOOKERLY.line_height_px());
        state.clear_pending_action();

        assert_eq!(state.cursor_y(), bookerly::BOOKERLY.line_height_px());
        assert_eq!(state.pending_action(), PendingAction::None);

        let outcome = state.advance_page(config);
        assert!(!outcome.target_complete);
        assert!(outcome.clear_framebuffer);
        assert_eq!(state.current_page(), 1);
        assert_eq!(state.cursor_y(), 0);
    }

    #[test]
    fn heading_events_switch_drawn_text_to_heading_style() {
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

        state
            .feed(
                &mut renderer,
                &mut observer,
                config,
                PaginationEvent::HeadingStart,
            )
            .expect("heading start should succeed");
        state
            .feed(
                &mut renderer,
                &mut observer,
                config,
                PaginationEvent::Text("Chapter"),
            )
            .expect("heading text should succeed");
        state
            .feed(
                &mut renderer,
                &mut observer,
                config,
                PaginationEvent::HeadingEnd,
            )
            .expect("heading end should succeed");
        state
            .feed(
                &mut renderer,
                &mut observer,
                config,
                PaginationEvent::Text("Body"),
            )
            .expect("body text should succeed");
        let _ = state
            .feed(&mut renderer, &mut observer, config, PaginationEvent::End)
            .expect("end should succeed");

        assert_eq!(
            observer.events,
            vec!["heading-start", "text", "heading-end", "text"]
        );
        assert_eq!(renderer.drawn_styles.len(), 2);
        assert!(renderer.drawn_styles[0].heading);
        assert!(!renderer.drawn_styles[1].heading);
    }
}
