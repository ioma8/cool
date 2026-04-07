use crate::text::WrappedLine;

pub(crate) trait CachedTextRenderer {
    fn clear_to_white(&mut self);
    fn draw_text(&mut self, x: u16, y: u16, text: &str);
    fn measure_text_width(&self, text: &str) -> i32;
    fn display_width(&self) -> u16;
    fn display_height(&self) -> u16;
}

pub(crate) struct CachedPaginationState<const N: usize> {
    pub(crate) line: WrappedLine<N>,
    pub(crate) cursor_y: u16,
    pub(crate) current_page: usize,
}

impl<const N: usize> CachedPaginationState<N> {
    pub(crate) const fn new() -> Self {
        Self {
            line: WrappedLine::new(),
            cursor_y: 0,
            current_page: 0,
        }
    }
}

pub(crate) fn render_cached_text_snippet<R, const N: usize>(
    renderer: &mut R,
    state: &mut CachedPaginationState<N>,
    target_page: usize,
    line_height: u16,
    text: &str,
) -> Result<bool, xteink_epub::EpubError>
where
    R: CachedTextRenderer,
{
    let finish_line = |renderer: &mut R,
                       state: &mut CachedPaginationState<N>|
     -> Result<bool, xteink_epub::EpubError> {
        if state.line.is_empty() {
            return Ok(false);
        }
        if state.cursor_y.saturating_add(line_height) > renderer.display_height() {
            if state.current_page >= target_page {
                return Ok(true);
            }
            state.current_page = state.current_page.saturating_add(1);
            state.line.clear();
            state.cursor_y = 0;
            renderer.clear_to_white();
        }
        renderer.draw_text(0, state.cursor_y, state.line.as_str());
        state.cursor_y = state.cursor_y.saturating_add(line_height);
        state.line.clear();
        Ok(false)
    };

    let finish_vertical_gap = |renderer: &mut R,
                               state: &mut CachedPaginationState<N>|
     -> Result<bool, xteink_epub::EpubError> {
        if state.cursor_y.saturating_add(line_height) > renderer.display_height() {
            if state.current_page >= target_page {
                return Ok(true);
            }
            state.current_page = state.current_page.saturating_add(1);
            state.cursor_y = 0;
            renderer.clear_to_white();
        }
        state.cursor_y = state.cursor_y.saturating_add(line_height);
        Ok(false)
    };

    for (segment_index, segment) in text.split('\n').enumerate() {
        if segment_index > 0 {
            if finish_line(renderer, state)? {
                return Ok(true);
            }
            if finish_vertical_gap(renderer, state)? {
                return Ok(true);
            }
        }

        for word in segment.split_whitespace() {
            let word_width = renderer.measure_text_width(word);
            if !state.line.is_empty()
                && state.line.width + renderer.measure_text_width(" ") + word_width
                    > i32::from(renderer.display_width())
            {
                if state.cursor_y.saturating_add(line_height) > renderer.display_height() {
                    if state.current_page >= target_page {
                        return Ok(true);
                    }
                    state.current_page = state.current_page.saturating_add(1);
                    state.line.clear();
                    state.cursor_y = 0;
                    renderer.clear_to_white();
                }
                renderer.draw_text(0, state.cursor_y, state.line.as_str());
                state.cursor_y = state.cursor_y.saturating_add(line_height);
                state.line.clear();
            }
            if !state.line.is_empty() {
                state.line.push_space();
            }
            state.line.push_str(word);
        }

        if !segment.is_empty() && !state.line.is_empty() && finish_line(renderer, state)? {
            return Ok(true);
        }

        if state.current_page > target_page && state.cursor_y >= renderer.display_height() {
            return Ok(true);
        }
    }
    Ok(false)
}
