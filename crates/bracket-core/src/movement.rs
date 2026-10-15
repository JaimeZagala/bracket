//! Cursor positions and boundaries over a rope. Pure functions, independent of any UI.
//!
//! A CRLF pair is treated as a single unit: the caret never stops between `\r` and `\n`.
//! Grapheme clusters (for example `e` + combining accent) are not handled yet.

use ropey::Rope;

/// True if `pos` falls between the `\r` and `\n` of a CRLF pair.
pub fn is_inside_crlf(text: &Rope, pos: usize) -> bool {
    pos > 0 && pos < text.len_chars() && text.char(pos - 1) == '\r' && text.char(pos) == '\n'
}

/// Position one step to the left of `pos`, or 0.
pub fn prev_boundary(text: &Rope, pos: usize) -> usize {
    if pos == 0 {
        return 0;
    }
    let prev = pos - 1;
    if is_inside_crlf(text, prev) {
        prev - 1
    } else {
        prev
    }
}

/// Position one step to the right of `pos`, or the end of the text.
pub fn next_boundary(text: &Rope, pos: usize) -> usize {
    let len = text.len_chars();
    if pos >= len {
        return len;
    }
    let next = pos + 1;
    if is_inside_crlf(text, next) {
        next + 1
    } else {
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boundaries_skip_crlf_as_a_unit() {
        let text = Rope::from_str("a\r\nb");
        assert_eq!(next_boundary(&text, 1), 3);
        assert_eq!(prev_boundary(&text, 3), 1);
        assert!(is_inside_crlf(&text, 2));
    }

    #[test]
    fn lone_cr_and_lf_are_single_steps() {
        let text = Rope::from_str("a\rb\nc");
        assert_eq!(next_boundary(&text, 1), 2);
        assert_eq!(prev_boundary(&text, 4), 3);
    }

    #[test]
    fn boundaries_stop_at_text_ends() {
        let text = Rope::from_str("ab");
        assert_eq!(prev_boundary(&text, 0), 0);
        assert_eq!(next_boundary(&text, 2), 2);
    }
}
