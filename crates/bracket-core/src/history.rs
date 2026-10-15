//! Undo/redo history made of transactions.
//!
//! Every undo step has a unique id, and [`History::state`] is the id of the last step applied
//! (0 before any edit). Undo and redo bring back earlier ids, so a document can tell exactly
//! whether its text matches what was saved, even after undoing back to it.

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
    id: u64,
}

#[derive(Clone, Debug, Default)]
pub struct History {
    undo: Vec<Entry>,
    redo: Vec<Entry>,
    /// When true, the next edit starts a new undo step even if it could be merged.
    sealed: bool,
    /// Last id handed out; ids are never reused.
    last_id: u64,
}

impl History {
    fn new_id(&mut self) -> u64 {
        self.last_id += 1;
        self.last_id
    }

    /// Records a transaction that has just been applied. Clears the redo stack.
    pub fn push(&mut self, transaction: Transaction, kind: EditKind) {
        self.redo.clear();
        let id = self.new_id();
        if let Some(last) = self.undo.last_mut() {
            let mergeable = !self.sealed
                && kind != EditKind::Other
                && last.kind == kind
                && last.transaction.after == transaction.before;
            if mergeable {
                last.transaction.changes.extend(transaction.changes);
                last.transaction.after = transaction.after;
                // The text is different now, so the step gets a new identity.
                last.id = id;
                return;
            }
        }
        self.sealed = false;
        self.undo.push(Entry {
            transaction,
            kind,
            id,
        });
    }

    /// Makes the next edit start a new undo step.
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// Identifies the current text state (see the module docs).
    pub fn state(&self) -> u64 {
        self.undo.last().map_or(0, |entry| entry.id)
    }

    /// Takes the most recent step to undo; the caller reverts it.
    pub fn pop_undo(&mut self) -> Option<Transaction> {
        let entry = self.undo.pop()?;
        let transaction = entry.transaction.clone();
        self.redo.push(entry);
        self.sealed = true;
        Some(transaction)
    }

    /// Takes the most recent undone step to redo; the caller applies it again.
    pub fn pop_redo(&mut self) -> Option<Transaction> {
        let entry = self.redo.pop()?;
        let transaction = entry.transaction.clone();
        self.undo.push(Entry {
            kind: EditKind::Other,
            ..entry
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
