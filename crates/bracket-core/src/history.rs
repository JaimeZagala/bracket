//! Undo/redo history made of transactions.

use crate::transaction::Transaction;

/// What kind of edit produced a transaction. Consecutive edits of the same groupable kind
/// are merged into a single undo step, like typing a word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditKind {
    /// Typing a single character.
    Typing,
    DeleteBackward,
    DeleteForward,
    /// Anything that must be its own undo step (paste, replace all, multi-line input...).
    Other,
}

#[derive(Clone, Debug)]
struct Entry {
    transaction: Transaction,
    kind: EditKind,
}

#[derive(Clone, Debug, Default)]
pub struct History {
    undo: Vec<Entry>,
    redo: Vec<Transaction>,
    /// When true, the next edit starts a new undo step even if it could be merged.
    sealed: bool,
}

impl History {
    /// Records a transaction that has just been applied. Clears the redo stack.
    pub fn push(&mut self, transaction: Transaction, kind: EditKind) {
        self.redo.clear();
        if let Some(last) = self.undo.last_mut() {
            let mergeable = !self.sealed
                && kind != EditKind::Other
                && last.kind == kind
                && last.transaction.after == transaction.before;
            if mergeable {
                last.transaction.changes.extend(transaction.changes);
                last.transaction.after = transaction.after;
                return;
            }
        }
        self.sealed = false;
        self.undo.push(Entry { transaction, kind });
    }

    /// Makes the next edit start a new undo step.
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// Takes the most recent step to undo; the caller reverts it.
    pub fn pop_undo(&mut self) -> Option<Transaction> {
        let entry = self.undo.pop()?;
        self.redo.push(entry.transaction.clone());
        self.sealed = true;
        Some(entry.transaction)
    }

    /// Takes the most recent undone step to redo; the caller applies it again.
    pub fn pop_redo(&mut self) -> Option<Transaction> {
        let transaction = self.redo.pop()?;
        self.undo.push(Entry {
            transaction: transaction.clone(),
            kind: EditKind::Other,
        });
        self.sealed = true;
        Some(transaction)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}
