use crate::bookerly;

pub(crate) struct WrappedLine<const N: usize> {
    pub(crate) buf: [u8; N],
    pub(crate) len: usize,
    pub(crate) width: i32,
}

pub(crate) struct TextBuffer<const N: usize> {
    pub(crate) buf: [u8; N],
    pub(crate) len: usize,
}

pub(crate) struct WrappedTextLayoutResult {
    pub(crate) next_y: u16,
    pub(crate) consumed: usize,
}

impl<const N: usize> TextBuffer<N> {
    pub(crate) const fn new() -> Self {
        Self {
            buf: [0; N],
            len: 0,
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub(crate) fn clear(&mut self) {
        self.len = 0;
    }

    pub(crate) fn push_exact(&mut self, text: &str) {
        let bytes = text.as_bytes();
        let remaining = self.buf.len().saturating_sub(self.len);
        let copy_len = core::cmp::min(remaining, bytes.len());
        self.buf[self.len..self.len + copy_len].copy_from_slice(&bytes[..copy_len]);
        self.len += copy_len;
    }

    pub(crate) fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }

    pub(crate) fn advance(&mut self, count: usize) {
        if count == 0 {
            return;
        }
        if count >= self.len {
            self.clear();
            return;
        }
        self.buf.copy_within(count..self.len, 0);
        self.len -= count;
    }
}

impl<const N: usize> WrappedLine<N> {
    pub(crate) const fn new() -> Self {
        Self {
            buf: [0; N],
            len: 0,
            width: 0,
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub(crate) fn clear(&mut self) {
        self.len = 0;
        self.width = 0;
    }

    pub(crate) fn push_space(&mut self) {
        if self.len < self.buf.len() {
            self.buf[self.len] = b' ';
            self.len += 1;
            self.width += measure_text_width(" ");
        }
    }

    pub(crate) fn push_str(&mut self, text: &str) {
        let bytes = text.as_bytes();
        let remaining = self.buf.len().saturating_sub(self.len);
        let copy_len = core::cmp::min(remaining, bytes.len());
        self.buf[self.len..self.len + copy_len].copy_from_slice(&bytes[..copy_len]);
        self.len += copy_len;
        self.width += measure_text_width(text);
    }

    pub(crate) fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }
}

pub(crate) fn measure_text_width(text: &str) -> i32 {
    bookerly::BOOKERLY.shape_text(text, |_, _, _| {})
}

pub(crate) fn layout_wrapped_text_page<const N: usize>(
    line: &mut WrappedLine<N>,
    x: u16,
    y: u16,
    text: &str,
    max_y: u16,
    mut draw_line: impl FnMut(u16, u16, &str),
) -> WrappedTextLayoutResult {
    let mut cursor_y = y;
    let line_height = bookerly::BOOKERLY.line_height_px();
    let available_width = i32::from(crate::DISPLAY_WIDTH.saturating_sub(x));
    let mut consumed = 0usize;
    let mut word_start: Option<usize> = None;
    let mut after_word = 0usize;
    let mut pending_space = false;

    line.clear();

    let flush_line = |line: &mut WrappedLine<N>,
                      cursor_y: &mut u16,
                      consumed: &mut usize,
                      flush_up_to: usize,
                      draw_line: &mut dyn FnMut(u16, u16, &str)|
     -> bool {
        if line.is_empty() {
            *consumed = flush_up_to;
            return false;
        }
        if cursor_y.saturating_add(line_height) > max_y {
            return true;
        }
        draw_line(x, *cursor_y, line.as_str());
        *cursor_y = cursor_y.saturating_add(line_height);
        *consumed = flush_up_to;
        line.clear();
        false
    };

    let mut iter = text.char_indices().peekable();
    while let Some((idx, ch)) = iter.next() {
        if ch == '\n' {
            if let Some(start) = word_start.take() {
                let word = &text[start..after_word];
                let word_width = measure_text_width(word);
                let fits = if line.is_empty() {
                    word_width <= available_width
                } else {
                    line.width + measure_text_width(" ") + word_width <= available_width
                };
                if !fits && !line.is_empty() {
                    if flush_line(line, &mut cursor_y, &mut consumed, start, &mut draw_line) {
                        return WrappedTextLayoutResult {
                            next_y: cursor_y,
                            consumed,
                        };
                    }
                }
                if !line.is_empty() && pending_space {
                    line.push_space();
                }
                line.push_str(word);
            }
            pending_space = false;
            word_start = None;
            after_word = idx + ch.len_utf8();
            if flush_line(
                line,
                &mut cursor_y,
                &mut consumed,
                idx + ch.len_utf8(),
                &mut draw_line,
            ) {
                return WrappedTextLayoutResult {
                    next_y: cursor_y,
                    consumed,
                };
            }
            continue;
        }

        if ch.is_whitespace() {
            if let Some(start) = word_start.take() {
                let word = &text[start..after_word];
                let word_width = measure_text_width(word);
                let fits = if line.is_empty() {
                    word_width <= available_width
                } else {
                    line.width + measure_text_width(" ") + word_width <= available_width
                };
                if !fits && !line.is_empty() {
                    if flush_line(line, &mut cursor_y, &mut consumed, start, &mut draw_line) {
                        return WrappedTextLayoutResult {
                            next_y: cursor_y,
                            consumed,
                        };
                    }
                }
                if !line.is_empty() && pending_space {
                    line.push_space();
                }
                line.push_str(word);
            } else if line.is_empty() {
                consumed = idx + ch.len_utf8();
            }
            pending_space = true;
            word_start = None;
            after_word = idx + ch.len_utf8();
            continue;
        }

        if word_start.is_none() {
            word_start = Some(idx);
        }
        after_word = idx + ch.len_utf8();

        if iter.peek().is_none() {
            let start = word_start.take().unwrap_or(idx);
            let word = &text[start..after_word];
            let word_width = measure_text_width(word);
            let fits = if line.is_empty() {
                word_width <= available_width
            } else {
                line.width + measure_text_width(" ") + word_width <= available_width
            };
            if !fits && !line.is_empty() {
                if flush_line(line, &mut cursor_y, &mut consumed, start, &mut draw_line) {
                    return WrappedTextLayoutResult {
                        next_y: cursor_y,
                        consumed,
                    };
                }
            }
            if !line.is_empty() && pending_space {
                line.push_space();
            }
            line.push_str(word);
        }
    }

    if !line.is_empty()
        && flush_line(
            line,
            &mut cursor_y,
            &mut consumed,
            text.len(),
            &mut draw_line,
        )
    {
        return WrappedTextLayoutResult {
            next_y: cursor_y,
            consumed,
        };
    }

    WrappedTextLayoutResult {
        next_y: cursor_y,
        consumed,
    }
}
