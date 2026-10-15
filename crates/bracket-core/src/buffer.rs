//! The text buffer: a rope with a set of selections.

use std::io;

use ropey::Rope;

use crate::selection::Selections;

/// Text plus the current selections. Text is stored exactly as given: line endings
/// and any other characters are never normalized.
#[derive(Clone, Debug)]
pub struct Buffer {
    text: Rope,
    selections: Selections,
}

impl Default for Buffer {
    fn default() -> Self {
        Self::new()
    }
}

impl Buffer {
    /// An empty buffer with a caret at the start.
    pub fn new() -> Self {
        Self::from_text("")
    }

    pub fn from_text(text: &str) -> Self {
        Self {
            text: Rope::from_str(text),
            selections: Selections::caret(0),
        }
    }

    /// Reads UTF-8 text. Fails with `InvalidData` if the input is not valid UTF-8.
    pub fn from_reader(reader: impl io::Read) -> io::Result<Self> {
        Ok(Self {
            text: Rope::from_reader(reader)?,
            selections: Selections::caret(0),
        })
    }

    pub fn text(&self) -> &Rope {
        &self.text
    }

    pub fn len_chars(&self) -> usize {
        self.text.len_chars()
    }

    /// Number of lines. A trailing line break starts a new, empty last line.
    pub fn len_lines(&self) -> usize {
        self.text.len_lines()
    }

    pub fn selections(&self) -> &Selections {
        &self.selections
    }

    /// Replaces the selections (cursor movement, mouse clicks). Positions beyond the end are clamped.
    pub fn set_selections(&mut self, selections: Selections) {
        self.selections = selections.clamped(self.text.len_chars());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selection::Selection;

    #[test]
    fn text_is_kept_byte_for_byte() {
        let original = "a\r\nb\nc\rd\u{2028}e\u{feff}";
        let buffer = Buffer::from_text(original);
        assert_eq!(buffer.text().to_string(), original);
    }

    #[test]
    fn only_lf_crlf_and_cr_break_lines() {
        let buffer = Buffer::from_text("a\r\nb\nc\rd\u{2028}e");
        assert_eq!(buffer.len_lines(), 4);
    }

    #[test]
    fn from_reader_rejects_invalid_utf8() {
        let bytes: &[u8] = &[b'a', 0xff, b'b'];
        let err = Buffer::from_reader(bytes).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn set_selections_clamps_to_text() {
        let mut buffer = Buffer::from_text("hello");
        buffer.set_selections(Selections::single(Selection::new(1, 99)));
        assert_eq!(buffer.selections().primary(), &Selection::new(1, 5));
    }
}
