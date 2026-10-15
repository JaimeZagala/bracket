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

pub fn is_line_break(c: char) -> bool {
    c == '\n' || c == '\r'
}

/// Index of the line that contains `pos`.
pub fn line_of(text: &Rope, pos: usize) -> usize {
    text.char_to_line(pos)
}

/// Position of the first char of `line`.
pub fn line_start(text: &Rope, line: usize) -> usize {
    text.line_to_char(line)
}

/// Position right before the line break of `line` (or the end of the text on the last line).
pub fn line_end(text: &Rope, line: usize) -> usize {
    let start = text.line_to_char(line);
    let slice = text.line(line);
    let mut len = slice.len_chars();
    // Strip the line break: CRLF, LF or CR.
    if len > 0 && slice.char(len - 1) == '\n' {
        len -= 1;
    }
    if len > 0 && slice.char(len - 1) == '\r' {
        len -= 1;
    }
    start + len
}

/// Visual column of `pos` within its line: tabs advance to the next multiple of `tab_width`,
/// every other char takes one column.
pub fn visual_column(text: &Rope, pos: usize, tab_width: usize) -> usize {
    let start = line_start(text, line_of(text, pos));
    text.slice(start..pos)
        .chars()
        .fold(0, |col, c| advance_column(col, c, tab_width))
}

/// Position in `line` closest to `column` without going past it, never beyond the line end.
pub fn pos_at_visual_column(text: &Rope, line: usize, column: usize, tab_width: usize) -> usize {
    let start = line_start(text, line);
    let end = line_end(text, line);
    let mut col = 0;
    let mut pos = start;
    for c in text.slice(start..end).chars() {
        let next = advance_column(col, c, tab_width);
        if next > column {
            // Snap to whichever side of the char is nearer.
            if column - col > next - column {
                pos += 1;
            }
            break;
        }
        col = next;
        pos += 1;
    }
    pos
}

fn advance_column(col: usize, c: char, tab_width: usize) -> usize {
    if c == '\t' && tab_width > 0 {
        (col / tab_width + 1) * tab_width
    } else {
        col + 1
    }
}

/// Position of the first non-blank char of `line`, or its end if the line is blank.
pub fn first_non_blank(text: &Rope, line: usize) -> usize {
    let start = line_start(text, line);
    let end = line_end(text, line);
    let blanks = text
        .slice(start..end)
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .count();
    start + blanks
}

/// Character classes used for word-wise movement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CharClass {
    Word,
    Space,
    LineBreak,
    Punctuation,
}

fn class_of(c: char) -> CharClass {
    if is_line_break(c) {
        CharClass::LineBreak
    } else if c.is_whitespace() {
        CharClass::Space
    } else if c.is_alphanumeric() || c == '_' {
        CharClass::Word
    } else {
        CharClass::Punctuation
    }
}

/// Ctrl+Right: skips blanks, then moves to the end of the following word or punctuation run.
/// A line break is a stop of its own.
pub fn next_word_boundary(text: &Rope, pos: usize) -> usize {
    let len = text.len_chars();
    if pos >= len {
        return len;
    }
    if is_line_break(text.char(pos)) {
        return next_boundary(text, pos);
    }
    let mut p = pos;
    while p < len && class_of(text.char(p)) == CharClass::Space {
        p += 1;
    }
    if p < len {
        let class = class_of(text.char(p));
        if class != CharClass::LineBreak {
            while p < len && class_of(text.char(p)) == class {
                p += 1;
            }
        }
    }
    p
}

/// Ctrl+Left: skips blanks backwards, then moves to the start of the preceding word or
/// punctuation run. A line break is a stop of its own.
pub fn prev_word_boundary(text: &Rope, pos: usize) -> usize {
    if pos == 0 {
        return 0;
    }
    if is_line_break(text.char(pos - 1)) {
        return prev_boundary(text, pos);
    }
    let mut p = pos;
    while p > 0 && class_of(text.char(p - 1)) == CharClass::Space {
        p -= 1;
    }
    if p > 0 {
        let class = class_of(text.char(p - 1));
        if class != CharClass::LineBreak {
            while p > 0 && class_of(text.char(p - 1)) == class {
                p -= 1;
            }
        }
    }
    p
}

/// A cursor movement, applied to every selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    Left,
    Right,
    WordLeft,
    WordRight,
    /// Home: first non-blank char, or the line start if already there.
    LineStart,
    LineEnd,
    /// Up by the given number of lines (1 for the arrow key, a page for Page Up).
    Up(usize),
    Down(usize),
    DocumentStart,
    DocumentEnd,
}

/// New head position and goal column for `motion` starting from a selection whose head is
/// `head` and whose goal column is `goal_column`. Vertical motions keep the goal column so the
/// caret returns to it after crossing shorter lines.
pub fn apply_motion(
    text: &Rope,
    head: usize,
    goal_column: Option<usize>,
    motion: Motion,
    tab_width: usize,
) -> (usize, Option<usize>) {
    let line = line_of(text, head);
    match motion {
        Motion::Left => (prev_boundary(text, head), None),
        Motion::Right => (next_boundary(text, head), None),
        Motion::WordLeft => (prev_word_boundary(text, head), None),
        Motion::WordRight => (next_word_boundary(text, head), None),
        Motion::LineStart => {
            let first = first_non_blank(text, line);
            let pos = if head == first {
                line_start(text, line)
            } else {
                first
            };
            (pos, None)
        }
        Motion::LineEnd => (line_end(text, line), None),
        Motion::Up(lines) => {
            if line == 0 {
                return (0, None);
            }
            let goal = goal_column.unwrap_or_else(|| visual_column(text, head, tab_width));
            let target = line.saturating_sub(lines);
            (
                pos_at_visual_column(text, target, goal, tab_width),
                Some(goal),
            )
        }
        Motion::Down(lines) => {
            let last = text.len_lines() - 1;
            if line == last {
                return (text.len_chars(), None);
            }
            let goal = goal_column.unwrap_or_else(|| visual_column(text, head, tab_width));
            let target = (line + lines).min(last);
            (
                pos_at_visual_column(text, target, goal, tab_width),
                Some(goal),
            )
        }
        Motion::DocumentStart => (0, None),
        Motion::DocumentEnd => (text.len_chars(), None),
    }
}

/// Range of the word (or punctuation or blank run) around `pos`, for double-click selection.
pub fn word_range_at(text: &Rope, pos: usize) -> (usize, usize) {
    let len = text.len_chars();
    // Prefer the char after the caret; fall back to the one before it at a line end.
    let probe = if pos < len && !is_line_break(text.char(pos)) {
        pos
    } else if pos > 0 && !is_line_break(text.char(pos - 1)) {
        pos - 1
    } else {
        return (pos, pos);
    };
    let class = class_of(text.char(probe));
    let mut start = probe;
    while start > 0 && class_of(text.char(start - 1)) == class {
        start -= 1;
    }
    let mut end = probe + 1;
    while end < len && class_of(text.char(end)) == class {
        end += 1;
    }
    (start, end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_end_excludes_every_kind_of_line_break() {
        let text = Rope::from_str("ab\r\ncd\nef\rgh");
        assert_eq!(line_end(&text, 0), 2);
        assert_eq!(line_end(&text, 1), 6);
        assert_eq!(line_end(&text, 2), 9);
        assert_eq!(line_end(&text, 3), 12);
    }

    #[test]
    fn visual_column_expands_tabs() {
        let text = Rope::from_str("\tab\tc");
        assert_eq!(visual_column(&text, 1, 4), 4);
        assert_eq!(visual_column(&text, 3, 4), 6);
        assert_eq!(visual_column(&text, 4, 4), 8);
    }

    #[test]
    fn pos_at_visual_column_snaps_and_clamps() {
        let text = Rope::from_str("\tab\nx");
        assert_eq!(
            pos_at_visual_column(&text, 0, 1, 4),
            0,
            "inside the tab, nearer its start"
        );
        assert_eq!(
            pos_at_visual_column(&text, 0, 3, 4),
            1,
            "inside the tab, nearer its end"
        );
        assert_eq!(pos_at_visual_column(&text, 0, 5, 4), 2);
        assert_eq!(
            pos_at_visual_column(&text, 0, 99, 4),
            3,
            "clamped to the line end"
        );
        assert_eq!(pos_at_visual_column(&text, 1, 99, 4), 5);
    }

    #[test]
    fn word_boundaries_follow_char_classes() {
        let text = Rope::from_str("foo.bar  baz\nqux");
        assert_eq!(next_word_boundary(&text, 0), 3);
        assert_eq!(next_word_boundary(&text, 3), 4);
        assert_eq!(next_word_boundary(&text, 7), 12);
        assert_eq!(
            next_word_boundary(&text, 12),
            13,
            "line break is its own stop"
        );
        assert_eq!(prev_word_boundary(&text, 12), 9);
        assert_eq!(prev_word_boundary(&text, 13), 12);
        assert_eq!(prev_word_boundary(&text, 4), 3);
    }

    #[test]
    fn word_range_at_selects_runs() {
        let text = Rope::from_str("let año_2 = 1;\n");
        assert_eq!(word_range_at(&text, 5), (4, 9));
        assert_eq!(
            word_range_at(&text, 9),
            (9, 10),
            "the blank run after the word"
        );
        assert_eq!(
            word_range_at(&text, 14),
            (13, 14),
            "at line end, the char before"
        );
    }

    #[test]
    fn vertical_motion_keeps_goal_column() {
        let text = Rope::from_str("abcdef\nab\nabcdef");
        let (pos, goal) = apply_motion(&text, 5, None, Motion::Down(1), 4);
        assert_eq!(
            (pos, goal),
            (9, Some(5)),
            "clamped to the short line, goal kept"
        );
        let (pos, goal) = apply_motion(&text, pos, goal, Motion::Down(1), 4);
        assert_eq!((pos, goal), (15, Some(5)), "back to the goal column");
    }

    #[test]
    fn vertical_motion_at_edges_goes_to_document_ends() {
        let text = Rope::from_str("abc\ndef");
        assert_eq!(apply_motion(&text, 2, None, Motion::Up(1), 4), (0, None));
        assert_eq!(apply_motion(&text, 5, None, Motion::Down(1), 4), (7, None));
        assert_eq!(
            apply_motion(&text, 1, None, Motion::Down(10), 4),
            (5, Some(1))
        );
    }

    #[test]
    fn line_start_toggles_between_indent_and_column_zero() {
        let text = Rope::from_str("    code");
        assert_eq!(apply_motion(&text, 6, None, Motion::LineStart, 4).0, 4);
        assert_eq!(apply_motion(&text, 4, None, Motion::LineStart, 4).0, 0);
        assert_eq!(apply_motion(&text, 0, None, Motion::LineStart, 4).0, 4);
    }

    #[test]
    fn first_non_blank_skips_indentation() {
        let text = Rope::from_str("  \tx\n   \n");
        assert_eq!(first_non_blank(&text, 0), 3);
        assert_eq!(first_non_blank(&text, 1), 8);
    }

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
