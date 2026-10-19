//! The find bar (Ctrl+F) above the editor: the query, match case / whole word / regex, a
//! counter and previous / next. Matches are found again whenever the query, its options, the
//! active tab or its text change; documents over [`THREAD_ABOVE`] are searched on a worker
//! thread, cancelled when a newer search starts.

use std::ops::Range;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use bracket_core::{CancelToken, Document, Generations};
use bracket_search::{FindError, FindOptions, Finder, Matches};
use eframe::egui::{
    self, Id, Key, Modifiers, RichText, Ui,
    text::{CCursor, CCursorRange},
};

use crate::i18n::Texts;

/// Documents larger than this (in bytes) are searched on a worker thread.
pub const THREAD_ABOVE: usize = 1 << 20;
/// Matches kept for one search; the counter shows "100000+" beyond it.
pub const LIMIT: usize = 100_000;

/// What the results were found for.
#[derive(Clone, Debug, PartialEq, Eq)]
struct SearchKey {
    tab: u64,
    version: u64,
    query: String,
    options: FindOptions,
}

/// What the user asked for in the bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindAction {
    Next,
    Previous,
    Close,
}

#[derive(Default)]
pub struct FindBar {
    open: bool,
    query: String,
    options: FindOptions,
    /// Give the field the keyboard (with its text selected) on the next frame.
    focus: bool,
    error: Option<String>,
    results: Option<(SearchKey, Matches)>,
    pending: Option<(SearchKey, Receiver<Option<Matches>>)>,
    searches: Generations,
    /// Select the first match from here once the results for a new query arrive.
    jump_from: Option<usize>,
}

impl FindBar {
    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn has_query(&self) -> bool {
        !self.query.is_empty()
    }

    /// Opens the bar, with `initial` as the query if given; the first match from `caret` is
    /// selected as soon as it is found.
    pub fn open(&mut self, initial: Option<String>, caret: usize) {
        if let Some(text) = initial {
            self.query = text;
        }
        self.open = true;
        self.focus = true;
        self.jump_from = Some(caret);
    }

    pub fn close(&mut self) {
        self.open = false;
        self.searches.cancel();
        self.pending = None;
        self.jump_from = None;
    }

    fn key(&self, tab: u64, doc: &Document) -> SearchKey {
        SearchKey {
            tab,
            version: doc.buffer.version(),
            query: self.query.clone(),
            options: self.options,
        }
    }

    /// Keeps the matches current for the active tab. Call every frame while the bar is open.
    pub fn update(&mut self, ctx: &egui::Context, tab: u64, doc: &Document) {
        if !self.open {
            return;
        }
        let key = self.key(tab, doc);
        if let Some((pending_key, rx)) = &self.pending {
            match rx.try_recv() {
                Ok(found) => {
                    let pending_key = pending_key.clone();
                    self.pending = None;
                    self.searches.cancel();
                    if let Some(matches) = found {
                        self.results = Some((pending_key, matches));
                    }
                }
                Err(TryRecvError::Disconnected) => self.pending = None,
                Err(TryRecvError::Empty) => {}
            }
        }
        let current = |k: Option<&SearchKey>| k == Some(&key);
        if current(self.results.as_ref().map(|(k, _)| k))
            || current(self.pending.as_ref().map(|(k, _)| k))
        {
            return;
        }
        let finder = match Finder::new(&self.query, self.options) {
            Ok(finder) => finder,
            Err(err) => {
                self.error = match err {
                    FindError::Empty => None,
                    FindError::Invalid(message) => Some(message),
                };
                self.searches.cancel();
                self.pending = None;
                self.results = Some((key, Matches::default()));
                return;
            }
        };
        self.error = None;
        let rope = doc.buffer.text();
        if rope.len_bytes() <= THREAD_ABOVE {
            let text = rope.to_string();
            let matches = finder
                .find_all(&text, LIMIT, &CancelToken::new())
                .unwrap_or_default();
            self.searches.cancel();
            self.pending = None;
            self.results = Some((key, matches));
            return;
        }
        // Cloning a rope is cheap: the text is shared until one of them changes.
        let rope = rope.clone();
        let task = self.searches.start();
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let text = rope.to_string();
            let _ = tx.send(finder.find_all(&text, LIMIT, &task.token));
            ctx.request_repaint();
        });
        self.pending = Some((key, rx));
    }

    /// The matches to mark in the tab: only while the bar is open and they are current.
    pub fn marks(&self, tab: u64, doc: &Document) -> &[Range<usize>] {
        match &self.results {
            Some((key, matches)) if self.open && *key == self.key(tab, doc) => &matches.ranges,
            _ => &[],
        }
    }

    /// The match to select now that the results for a new query arrived, if one is waiting.
    pub fn take_jump(&mut self, tab: u64, doc: &Document) -> Option<Range<usize>> {
        let from = self.jump_from?;
        let ranges = self.marks(tab, doc);
        if self.results.as_ref().map(|(k, _)| k) != Some(&self.key(tab, doc)) {
            return None;
        }
        let found = nearest(ranges, from);
        self.jump_from = None;
        found
    }

    /// The next (or previous) match from the selection, wrapping around the document.
    pub fn step(&self, tab: u64, doc: &Document, forward: bool) -> Option<Range<usize>> {
        let ranges = self.marks(tab, doc);
        let selection = doc.buffer.selections().primary();
        let index = step(ranges, selection.start()..selection.end(), forward)?;
        Some(ranges[index].clone())
    }

    /// Draws the bar. `caret`: where a new query starts looking from.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        texts: &Texts,
        tab: u64,
        doc: &Document,
    ) -> Option<FindAction> {
        let mut action = None;
        let id = Id::new("find-query");
        let ctx = ui.ctx().clone();
        let caret = doc.buffer.selections().primary().start();
        if ui.memory(|m| m.has_focus(id)) {
            let consume = |modifiers, key| ctx.input_mut(|i| i.consume_key(modifiers, key));
            if consume(Modifiers::SHIFT, Key::Enter) {
                action = Some(FindAction::Previous);
            } else if consume(Modifiers::NONE, Key::Enter) {
                action = Some(FindAction::Next);
            } else if consume(Modifiers::NONE, Key::Escape) {
                action = Some(FindAction::Close);
            }
        }
        // Alt+C, Alt+W and Alt+R toggle the options wherever the keyboard is.
        for (key, option) in [
            (Key::C, &mut self.options.match_case),
            (Key::W, &mut self.options.whole_word),
            (Key::R, &mut self.options.regex),
        ] {
            if ctx.input_mut(|i| i.consume_key(Modifiers::ALT, key)) {
                *option = !*option;
                self.jump_from = Some(caret);
            }
        }

        ui.horizontal(|ui| {
            let mut output = egui::TextEdit::singleline(&mut self.query)
                .id(id)
                .hint_text(texts.find_hint())
                .desired_width(260.0)
                .show(ui);
            if self.focus {
                output.response.request_focus();
                let all =
                    CCursorRange::two(CCursor::new(0), CCursor::new(self.query.chars().count()));
                output.state.cursor.set_char_range(Some(all));
                output.state.store(ui.ctx(), id);
                self.focus = false;
            }
            if output.response.changed() {
                self.jump_from = Some(caret);
            }
            for (label, tip, option) in [
                ("Aa", texts.find_match_case(), &mut self.options.match_case),
                ("ab", texts.find_whole_word(), &mut self.options.whole_word),
                (".*", texts.find_regex(), &mut self.options.regex),
            ] {
                let text = RichText::new(label).monospace();
                if ui
                    .selectable_label(*option, text)
                    .on_hover_text(tip)
                    .clicked()
                {
                    *option = !*option;
                    self.jump_from = Some(caret);
                }
            }
            ui.add_space(6.0);
            let marks = self.marks(tab, doc);
            let selection = doc.buffer.selections().primary();
            let current = marks
                .binary_search_by_key(&selection.start(), |r| r.start)
                .ok()
                .filter(|&i| marks[i].end == selection.end());
            let truncated = self
                .results
                .as_ref()
                .is_some_and(|(k, m)| m.truncated && *k == self.key(tab, doc));
            let total = if truncated {
                format!("{}+", marks.len())
            } else {
                marks.len().to_string()
            };
            let counter = if let Some(error) = &self.error {
                ui.colored_label(ui.visuals().error_fg_color, texts.find_invalid())
                    .on_hover_text(error.as_str())
            } else if self.pending.is_some() && marks.is_empty() {
                ui.weak(texts.find_searching())
            } else if self.query.is_empty() {
                ui.weak("")
            } else if marks.is_empty() {
                ui.weak(texts.find_none())
            } else if let Some(i) = current {
                ui.label(texts.find_position(&(i + 1).to_string(), &total))
            } else {
                ui.label(texts.find_total(&total))
            };
            let _ = counter;
            ui.add_space(6.0);
            if ui
                .small_button("↑")
                .on_hover_text(texts.find_previous_tip())
                .clicked()
            {
                action = Some(FindAction::Previous);
            }
            if ui
                .small_button("↓")
                .on_hover_text(texts.find_next_tip())
                .clicked()
            {
                action = Some(FindAction::Next);
            }
            if ui
                .small_button("×")
                .on_hover_text(texts.find_close_tip())
                .clicked()
            {
                action = Some(FindAction::Close);
            }
        });
        action
    }
}

/// The first match starting at or after `from`, or the first one (wrapping around).
fn nearest(ranges: &[Range<usize>], from: usize) -> Option<Range<usize>> {
    let index = ranges.partition_point(|r| r.start < from);
    ranges.get(index).or_else(|| ranges.first()).cloned()
}

/// The match after (or before) `selection`: if the selection is a match, its neighbour;
/// otherwise the first one after (or the last one before) it. Wraps around.
fn step(ranges: &[Range<usize>], selection: Range<usize>, forward: bool) -> Option<usize> {
    if ranges.is_empty() {
        return None;
    }
    let count = ranges.len();
    let exact = ranges
        .binary_search_by_key(&selection.start, |r| r.start)
        .ok()
        .filter(|&i| ranges[i].end == selection.end);
    Some(match (exact, forward) {
        (Some(i), true) => (i + 1) % count,
        (Some(i), false) => (i + count - 1) % count,
        (None, true) => {
            let after = ranges.partition_point(|r| r.start < selection.start);
            after % count
        }
        (None, false) => {
            let before = ranges.partition_point(|r| r.start < selection.start);
            (before + count - 1) % count
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bracket_core::LineEnding;
    use bracket_core::encoding::TextEncoding;

    #[test]
    fn stepping_wraps_and_starts_from_the_selection() {
        let ranges = [2..4, 10..12, 20..22];
        assert_eq!(
            step(&ranges, 10..12, true),
            Some(2),
            "from a match to the next"
        );
        assert_eq!(step(&ranges, 20..22, true), Some(0), "wraps forward");
        assert_eq!(step(&ranges, 2..4, false), Some(2), "wraps backward");
        assert_eq!(step(&ranges, 5..5, true), Some(1), "first after the caret");
        assert_eq!(step(&ranges, 5..5, false), Some(0), "last before the caret");
        assert_eq!(step(&ranges, 30..30, true), Some(0));
        assert_eq!(step(&[], 0..0, true), None);
        assert_eq!(nearest(&ranges, 10), Some(10..12));
        assert_eq!(nearest(&ranges, 25), Some(2..4));
    }

    fn doc(text: &str) -> Document {
        let mut doc = Document::new_empty(LineEnding::Lf, TextEncoding::UTF8);
        doc.buffer.insert(text);
        doc
    }

    #[test]
    fn small_documents_are_searched_at_once_and_follow_edits() {
        let ctx = egui::Context::default();
        let mut bar = FindBar::default();
        let mut doc = doc("let x = 1;\nlet y = x;\n");
        bar.open(Some("x".into()), 0);
        bar.update(&ctx, 1, &doc);
        assert_eq!(bar.marks(1, &doc), [4..5, 19..20]);
        assert_eq!(bar.take_jump(1, &doc), Some(4..5));
        assert!(
            bar.marks(2, &doc).is_empty(),
            "another tab has its own matches"
        );

        doc.buffer.insert("x");
        assert!(bar.marks(1, &doc).is_empty(), "stale after an edit");
        bar.update(&ctx, 1, &doc);
        assert_eq!(bar.marks(1, &doc).len(), 3);

        bar.close();
        assert!(
            bar.marks(1, &doc).is_empty(),
            "nothing is marked while closed"
        );
    }

    #[test]
    fn large_documents_are_searched_on_a_thread() {
        let ctx = egui::Context::default();
        let mut bar = FindBar::default();
        let line = "public class Customer { }\n";
        let doc = doc(&line.repeat(THREAD_ABOVE / line.len() + 10));
        bar.open(Some("customer".into()), 0);
        bar.update(&ctx, 1, &doc);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while bar.marks(1, &doc).is_empty() {
            assert!(
                std::time::Instant::now() < deadline,
                "the search never finished"
            );
            thread::sleep(std::time::Duration::from_millis(5));
            bar.update(&ctx, 1, &doc);
        }
        assert_eq!(bar.marks(1, &doc).len(), THREAD_ABOVE / line.len() + 10);
    }

    #[test]
    fn an_invalid_regex_is_reported_and_marks_nothing() {
        let ctx = egui::Context::default();
        let mut bar = FindBar::default();
        let doc = doc("abc");
        bar.options.regex = true;
        bar.open(Some("(".into()), 0);
        bar.update(&ctx, 1, &doc);
        assert!(bar.error.is_some());
        assert!(bar.marks(1, &doc).is_empty());
    }
}
