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

    pub(crate) fn push(&mut self, text: &str) {
        let bytes = text.as_bytes();
        let needs_space = self.len > 0
            && !self.buf[self.len - 1].is_ascii_whitespace()
            && !bytes.first().copied().unwrap_or(b' ').is_ascii_whitespace();

        if needs_space && self.len < self.buf.len() {
            self.buf[self.len] = b' ';
            self.len += 1;
        }

        let remaining = self.buf.len().saturating_sub(self.len);
        let copy_len = core::cmp::min(remaining, bytes.len());
        self.buf[self.len..self.len + copy_len].copy_from_slice(&bytes[..copy_len]);
        self.len += copy_len;
    }

    pub(crate) fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
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
