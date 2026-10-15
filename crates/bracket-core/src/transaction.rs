//! Changes and transactions: the unit of editing and of undo/redo.

use ropey::Rope;

use crate::selection::Selections;

/// One replacement: at char position `start`, the text `removed` is replaced by `inserted`.
///
/// Storing the removed text (not just its length) makes every change invertible.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub start: usize,
    pub removed: String,
    pub inserted: String,
}

impl Change {
    /// End of the removed range, in chars, before the change is applied.
    pub fn removed_end(&self) -> usize {
        self.start + self.removed.chars().count()
    }

    /// The change that undoes this one.
    pub fn inverted(&self) -> Self {
        Self {
            start: self.start,
            removed: self.inserted.clone(),
            inserted: self.removed.clone(),
        }
    }

    pub(crate) fn apply(&self, text: &mut Rope) {
        let end = self.removed_end();
        debug_assert_eq!(
            text.slice(self.start..end).to_string(),
            self.removed,
            "change does not match the text it removes"
        );
        if end > self.start {
            text.remove(self.start..end);
        }
        if !self.inserted.is_empty() {
            text.insert(self.start, &self.inserted);
        }
    }
}

/// A group of changes applied as a single editing step, plus the selections before and after.
///
/// Changes are applied in order, and the position of each one refers to the text left by the
/// previous change. Undo applies the inverted changes in reverse order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transaction {
    pub changes: Vec<Change>,
    pub before: Selections,
    pub after: Selections,
}

impl Transaction {
    pub(crate) fn apply(&self, text: &mut Rope) {
        for change in &self.changes {
            change.apply(text);
        }
    }

    pub(crate) fn revert(&self, text: &mut Rope) {
        for change in self.changes.iter().rev() {
            change.inverted().apply(text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(start: usize, removed: &str, inserted: &str) -> Change {
        Change {
            start,
            removed: removed.to_owned(),
            inserted: inserted.to_owned(),
        }
    }

    #[test]
    fn apply_and_revert_sequential_changes() {
        let mut text = Rope::from_str("hello world");
        let tx = Transaction {
            changes: vec![change(0, "hello", "hi"), change(3, "world", "there")],
            before: Selections::caret(0),
            after: Selections::caret(0),
        };
        tx.apply(&mut text);
        assert_eq!(text.to_string(), "hi there");
        tx.revert(&mut text);
        assert_eq!(text.to_string(), "hello world");
    }

    #[test]
    fn positions_are_chars_not_bytes() {
        let mut text = Rope::from_str("ñandú€");
        change(4, "ú", "u").apply(&mut text);
        assert_eq!(text.to_string(), "ñandu€");
    }
}
