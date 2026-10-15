//! The text buffer: a rope with a set of selections.

use std::io;
use std::ops::Range;

use ropey::Rope;

use crate::history::{EditKind, History};
use crate::movement::{
    Motion, apply_motion, is_inside_crlf, line_end, line_of, line_start, next_boundary,
    prev_boundary,
};
use crate::selection::{Selection, Selections};
use crate::transaction::{Change, Transaction};

/// Tab width used until the configuration says otherwise.
pub const DEFAULT_TAB_WIDTH: usize = 4;

/// Text plus the current selections. Text is stored exactly as given: line endings
/// and any other characters are never normalized.
#[derive(Clone, Debug)]
pub struct Buffer {
    text: Rope,
    selections: Selections,
    history: History,
    tab_width: usize,
    /// Incremented on every change to the text (edit, undo or redo).
    version: u64,
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
            history: History::default(),
            tab_width: DEFAULT_TAB_WIDTH,
            version: 0,
        }
    }

    /// Reads UTF-8 text. Fails with `InvalidData` if the input is not valid UTF-8.
    pub fn from_reader(reader: impl io::Read) -> io::Result<Self> {
        Ok(Self {
            text: Rope::from_reader(reader)?,
            selections: Selections::caret(0),
            history: History::default(),
            tab_width: DEFAULT_TAB_WIDTH,
            version: 0,
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

    /// Changes whenever the text changes. Comparing versions tells whether there are unsaved edits.
    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn tab_width(&self) -> usize {
        self.tab_width
    }

    /// Sets the tab width used for visual columns. Values below 1 are treated as 1.
    pub fn set_tab_width(&mut self, tab_width: usize) {
        self.tab_width = tab_width.max(1);
    }

    /// Moves every selection. With `extend` (Shift held) the anchors stay and the selections
    /// grow; without it, each selection collapses to a caret at the new position, except that
    /// Left/Right on a non-empty selection collapse it to its start/end.
    pub fn move_selections(&mut self, motion: Motion, extend: bool) {
        let text = &self.text;
        let tab_width = self.tab_width;
        self.selections = self.selections.map(|sel| {
            if !extend && !sel.is_caret() {
                match motion {
                    Motion::Left => return Selection::caret(sel.start()),
                    Motion::Right => return Selection::caret(sel.end()),
                    _ => {}
                }
            }
            let (head, goal_column) =
                apply_motion(text, sel.head, sel.goal_column, motion, tab_width);
            Selection {
                anchor: if extend { sel.anchor } else { head },
                head,
                goal_column,
            }
        });
    }

    /// Replaces the whole text in one undo step (for example to convert line endings), keeping
    /// each selection end on the same line and column, clamped to the new line length.
    /// Returns false, and records nothing, if the text is already `new_text`.
    pub fn replace_all_keeping_positions(&mut self, new_text: &str) -> bool {
        if self.text == new_text {
            return false;
        }
        let before = self.selections.clone();
        let text = &self.text;
        let to_line_col = |pos: usize| {
            let line = line_of(text, pos);
            (line, pos - line_start(text, line))
        };
        let ends: Vec<_> = before
            .iter()
            .map(|s| (to_line_col(s.anchor), to_line_col(s.head)))
            .collect();

        let change = Change {
            start: 0,
            removed: self.text.to_string(),
            inserted: new_text.to_owned(),
        };
        change.apply(&mut self.text);

        let text = &self.text;
        let from_line_col = |(line, column): (usize, usize)| {
            let line = line.min(text.len_lines() - 1);
            (line_start(text, line) + column).min(line_end(text, line))
        };
        let selections = ends
            .into_iter()
            .map(|(anchor, head)| Selection::new(from_line_col(anchor), from_line_col(head)))
            .collect();
        self.selections = Selections::new(selections, before.primary_index());
        let transaction = Transaction {
            changes: vec![change],
            before,
            after: self.selections.clone(),
        };
        self.history.push(transaction, EditKind::Other);
        self.version += 1;
        true
    }

    /// Selects the whole text as a single selection.
    pub fn select_all(&mut self) {
        self.selections = Selections::single(Selection::new(0, self.text.len_chars()));
    }

    /// Replaces the selections (cursor movement, mouse clicks). Positions beyond the end are
    /// clamped, and positions between the `\r` and `\n` of a CRLF move before the `\r`.
    pub fn set_selections(&mut self, selections: Selections) {
        let text = &self.text;
        let snap = |pos: usize| {
            if is_inside_crlf(text, pos) {
                pos - 1
            } else {
                pos
            }
        };
        self.selections = selections.clamped(text.len_chars()).map(|s| Selection {
            anchor: snap(s.anchor),
            head: snap(s.head),
            goal_column: s.goal_column,
        });
    }

    /// Replaces every selection with `text`. Each caret ends up after its insertion.
    ///
    /// Typing single characters one after another forms one undo step; anything else
    /// (several characters, a line break, replacing a selection) is a step of its own.
    pub fn insert(&mut self, text: &str) {
        let mut chars = text.chars();
        let single_char =
            matches!((chars.next(), chars.next()), (Some(c), None) if c != '\n' && c != '\r');
        let kind = if single_char && self.selections.iter().all(Selection::is_caret) {
            EditKind::Typing
        } else {
            EditKind::Other
        };
        self.edit(kind, |_, sel| (sel.start()..sel.end(), text.to_owned()));
    }

    /// Backspace: deletes each non-empty selection, or the character before each caret.
    pub fn delete_backward(&mut self) {
        let kind = self.deletion_kind(EditKind::DeleteBackward);
        self.edit(kind, |text, sel| {
            let range = if sel.is_caret() {
                prev_boundary(text, sel.head)..sel.head
            } else {
                sel.start()..sel.end()
            };
            (range, String::new())
        });
    }

    /// Delete: deletes each non-empty selection, or the character after each caret.
    pub fn delete_forward(&mut self) {
        let kind = self.deletion_kind(EditKind::DeleteForward);
        self.edit(kind, |text, sel| {
            let range = if sel.is_caret() {
                sel.head..next_boundary(text, sel.head)
            } else {
                sel.start()..sel.end()
            };
            (range, String::new())
        });
    }

    /// Undoes the last step. Returns false if there was nothing to undo.
    pub fn undo(&mut self) -> bool {
        let Some(transaction) = self.history.pop_undo() else {
            return false;
        };
        transaction.revert(&mut self.text);
        self.version += 1;
        self.selections = transaction.before;
        true
    }

    /// Redoes the last undone step. Returns false if there was nothing to redo.
    pub fn redo(&mut self) -> bool {
        let Some(transaction) = self.history.pop_redo() else {
            return false;
        };
        transaction.apply(&mut self.text);
        self.version += 1;
        self.selections = transaction.after;
        true
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// Makes the next edit start a new undo step (for example after a pause in typing).
    pub fn seal_undo_step(&mut self) {
        self.history.seal();
    }

    /// Deleting a selection is a step of its own; deleting char by char groups.
    fn deletion_kind(&self, char_kind: EditKind) -> EditKind {
        if self.selections.iter().all(Selection::is_caret) {
            char_kind
        } else {
            EditKind::Other
        }
    }

    /// Core editing routine shared by every edit command.
    ///
    /// `plan` is called once per selection, in order, against the text as it was before the edit,
    /// and returns the range to replace and its replacement. Ranges that reach into the previous
    /// one are trimmed so they never overlap. All replacements form one transaction, recorded in
    /// the history as a single step, and each selection becomes a caret after its replacement.
    fn edit(
        &mut self,
        kind: EditKind,
        mut plan: impl FnMut(&Rope, &Selection) -> (Range<usize>, String),
    ) {
        let before = self.selections.clone();
        let plans: Vec<(Range<usize>, String)> =
            before.iter().map(|sel| plan(&self.text, sel)).collect();

        let mut changes = Vec::new();
        let mut carets = Vec::with_capacity(plans.len());
        // Chars inserted and removed so far: they shift every later position.
        let mut added = 0;
        let mut removed_total = 0;
        let mut prev_end = 0;

        for (range, inserted) in plans {
            let start = range.start.max(prev_end);
            let end = range.end.max(start);
            prev_end = end;

            let at = start + added - removed_total;
            let removed_len = end - start;
            let inserted_len = inserted.chars().count();
            if removed_len > 0 || inserted_len > 0 {
                let change = Change {
                    start: at,
                    removed: self.text.slice(at..at + removed_len).to_string(),
                    inserted,
                };
                change.apply(&mut self.text);
                changes.push(change);
            }
            added += inserted_len;
            removed_total += removed_len;
            carets.push(Selection::caret(at + inserted_len));
        }

        self.selections = Selections::new(carets, before.primary_index());
        if !changes.is_empty() {
            let transaction = Transaction {
                changes,
                before,
                after: self.selections.clone(),
            };
            self.history.push(transaction, kind);
            self.version += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buffer_with(text: &str, selections: &[(usize, usize)]) -> Buffer {
        let mut buffer = Buffer::from_text(text);
        let ranges = selections
            .iter()
            .map(|&(a, h)| Selection::new(a, h))
            .collect();
        buffer.set_selections(Selections::new(ranges, 0));
        buffer
    }

    fn heads(buffer: &Buffer) -> Vec<usize> {
        buffer.selections().iter().map(|s| s.head).collect()
    }

    #[test]
    fn insert_at_every_caret() {
        let mut b = buffer_with("abc", &[(0, 0), (3, 3)]);
        b.insert("XY");
        assert_eq!(b.text().to_string(), "XYabcXY");
        assert_eq!(heads(&b), vec![2, 7]);
    }

    #[test]
    fn insert_replaces_selections() {
        let mut b = buffer_with("one two three", &[(0, 3), (8, 13)]);
        b.insert("1");
        assert_eq!(b.text().to_string(), "1 two 1");
        assert_eq!(heads(&b), vec![1, 7]);
    }

    #[test]
    fn insert_handles_multibyte_text() {
        let mut b = buffer_with("año", &[(1, 2)]);
        b.insert("ñ€");
        assert_eq!(b.text().to_string(), "añ€o");
        assert_eq!(heads(&b), vec![3]);
    }

    #[test]
    fn delete_backward_at_every_caret() {
        let mut b = buffer_with("abcdef", &[(2, 2), (5, 5)]);
        b.delete_backward();
        assert_eq!(b.text().to_string(), "acdf");
        assert_eq!(heads(&b), vec![1, 3]);
    }

    #[test]
    fn delete_backward_removes_crlf_as_a_unit() {
        let mut b = buffer_with("a\r\nb", &[(3, 3)]);
        b.delete_backward();
        assert_eq!(b.text().to_string(), "ab");
        assert_eq!(heads(&b), vec![1]);
    }

    #[test]
    fn delete_backward_with_touching_ranges_merges_carets() {
        let mut b = buffer_with("abc", &[(1, 1), (2, 2)]);
        b.delete_backward();
        assert_eq!(b.text().to_string(), "c");
        assert_eq!(heads(&b), vec![0]);
    }

    #[test]
    fn delete_forward_at_end_does_nothing() {
        let mut b = buffer_with("ab", &[(2, 2)]);
        b.delete_forward();
        assert_eq!(b.text().to_string(), "ab");
    }

    #[test]
    fn delete_forward_removes_selection_and_crlf() {
        let mut b = buffer_with("x\r\nyz", &[(1, 1), (3, 4)]);
        b.delete_forward();
        assert_eq!(b.text().to_string(), "xz");
        assert_eq!(heads(&b), vec![1]);
    }

    #[test]
    fn set_selections_snaps_out_of_crlf() {
        let b = buffer_with("a\r\nb", &[(2, 2)]);
        assert_eq!(heads(&b), vec![1]);
    }

    #[test]
    fn typing_groups_into_one_undo_step() {
        let mut b = buffer_with("", &[(0, 0)]);
        for c in ["h", "o", "l", "a"] {
            b.insert(c);
        }
        assert!(b.undo());
        assert_eq!(b.text().to_string(), "");
        assert!(!b.undo());
    }

    #[test]
    fn moving_the_caret_or_sealing_starts_a_new_step() {
        let mut b = buffer_with("", &[(0, 0)]);
        b.insert("a");
        b.insert("b");
        b.set_selections(Selections::caret(0));
        b.insert("c");
        b.seal_undo_step();
        b.insert("d");
        assert_eq!(b.text().to_string(), "cdab");
        b.undo();
        assert_eq!(b.text().to_string(), "cab");
        b.undo();
        assert_eq!(b.text().to_string(), "ab");
        b.undo();
        assert_eq!(b.text().to_string(), "");
    }

    #[test]
    fn multi_caret_edit_is_one_step_and_restores_selections() {
        let mut b = buffer_with("a\nb\nc", &[(1, 1), (3, 3), (5, 5)]);
        b.insert(";");
        assert_eq!(b.text().to_string(), "a;\nb;\nc;");
        b.undo();
        assert_eq!(b.text().to_string(), "a\nb\nc");
        assert_eq!(heads(&b), vec![1, 3, 5]);
        b.redo();
        assert_eq!(b.text().to_string(), "a;\nb;\nc;");
        assert_eq!(heads(&b), vec![2, 5, 8]);
    }

    #[test]
    fn line_break_and_paste_are_separate_steps() {
        let mut b = buffer_with("", &[(0, 0)]);
        b.insert("a");
        b.insert("\n");
        b.insert("pasted text");
        b.undo();
        assert_eq!(b.text().to_string(), "a\n");
        b.undo();
        assert_eq!(b.text().to_string(), "a");
    }

    #[test]
    fn new_edit_after_undo_clears_redo() {
        let mut b = buffer_with("", &[(0, 0)]);
        b.insert("x");
        b.undo();
        assert!(b.can_redo());
        b.insert("y");
        assert!(!b.can_redo());
        assert!(!b.redo());
    }

    #[test]
    fn shift_motion_extends_and_plain_motion_collapses() {
        let mut b = buffer_with("hello world", &[(0, 0)]);
        b.move_selections(Motion::WordRight, true);
        assert_eq!(b.selections().primary(), &Selection::new(0, 5));
        b.move_selections(Motion::Left, false);
        assert_eq!(b.selections().primary(), &Selection::caret(0));
        b.move_selections(Motion::LineEnd, false);
        assert_eq!(heads(&b), vec![11]);
    }

    #[test]
    fn carets_that_meet_after_moving_merge() {
        let mut b = buffer_with("ab\ncd", &[(1, 1), (4, 4)]);
        b.move_selections(Motion::DocumentStart, false);
        assert_eq!(b.selections().len(), 1);
    }

    #[test]
    fn motion_skips_crlf_as_a_unit() {
        let mut b = buffer_with("a\r\nb", &[(1, 1)]);
        b.move_selections(Motion::Right, false);
        assert_eq!(heads(&b), vec![3]);
    }

    #[test]
    fn select_all_covers_the_text() {
        let mut b = buffer_with("abc\ndef", &[(1, 1), (5, 5)]);
        b.select_all();
        assert_eq!(b.selections().as_slice(), &[Selection::new(0, 7)]);
    }

    #[test]
    fn replace_all_keeps_line_and_column_and_undoes_in_one_step() {
        let mut b = buffer_with("ab\r\ncd\r\nef", &[(5, 6)]);
        assert!(b.replace_all_keeping_positions("ab\ncd\nef"));
        assert_eq!(b.text().to_string(), "ab\ncd\nef");
        assert_eq!(
            b.selections().primary(),
            &Selection::new(4, 5),
            "line 1, columns 1..2"
        );
        assert!(
            !b.replace_all_keeping_positions("ab\ncd\nef"),
            "no change, no step"
        );
        b.undo();
        assert_eq!(b.text().to_string(), "ab\r\ncd\r\nef");
        assert_eq!(b.selections().primary(), &Selection::new(5, 6));
    }

    #[test]
    fn edit_that_changes_nothing_is_not_recorded() {
        let mut b = buffer_with("ab", &[(0, 0)]);
        b.delete_backward();
        assert!(!b.can_undo());
    }

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
