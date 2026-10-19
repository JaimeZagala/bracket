//! The path to the caret in XML and JSON documents, for the status bar. It is worked out on a
//! thread of its own a moment after the caret or the text stop changing; the thread keeps a
//! parser per recent document, so a move costs no parse and an edit only a partial one.

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use bracket_core::{Document, Rope};
use bracket_index::caret_path::{self, PathParser};
use eframe::egui;

/// The caret and the text must stay put this long.
const DEBOUNCE: Duration = Duration::from_millis(100);
/// Documents larger than this (in bytes) have no caret path.
const MAX_BYTES: usize = 10 << 20;
/// Documents whose parser the worker keeps.
const PARSERS: usize = 8;

/// The document, its version and the caret: what a path answers.
type Key = (u64, u64, usize);

struct Request {
    key: Key,
    language: String,
    text: Rope,
}

struct Worker {
    requests: Sender<Request>,
    responses: Receiver<(Key, Option<String>)>,
}

impl Worker {
    fn start(ctx: &egui::Context) -> Self {
        let (requests, request_rx) = mpsc::channel::<Request>();
        let (response_tx, responses) = mpsc::channel();
        let ctx = ctx.clone();
        thread::Builder::new()
            .name("caret-path".to_owned())
            .spawn(move || {
                // The parsers of recent documents, newest last.
                let mut parsers: Vec<((u64, String), PathParser)> = Vec::new();
                while let Ok(first) = request_rx.recv() {
                    // Only the newest request waiting is worked on.
                    let request = request_rx.try_iter().last().unwrap_or(first);
                    let (tab, _, caret) = request.key;
                    let key = (tab, request.language.clone());
                    let parser = match parsers.iter().position(|(k, _)| *k == key) {
                        Some(i) => Some(parsers.remove(i).1),
                        None => PathParser::new(&request.language),
                    };
                    let path = parser.and_then(|mut parser| {
                        let path = parser.path_at(request.text.to_string(), caret);
                        parsers.push((key, parser));
                        if parsers.len() > PARSERS {
                            parsers.remove(0);
                        }
                        path
                    });
                    if response_tx.send((request.key, path)).is_err() {
                        return;
                    }
                    ctx.request_repaint();
                }
            })
            .expect("the caret path thread starts");
        Self {
            requests,
            responses,
        }
    }
}

/// The caret path of the active document.
#[derive(Default)]
pub struct CaretPath {
    worker: Option<Worker>,
    requested: Option<Key>,
    /// When the document or caret last changed, while waiting to ask.
    changed_at: Option<(Key, Instant)>,
    /// The newest path, and the document it belongs to.
    shown: Option<(u64, String)>,
}

impl CaretPath {
    /// Keeps the path in step with the active document. Call every frame.
    pub fn update(
        &mut self,
        ctx: &egui::Context,
        tab: u64,
        doc: &Document,
        language: Option<&str>,
    ) {
        if let Some(worker) = &self.worker {
            for ((t, _, _), path) in worker.responses.try_iter() {
                self.shown = path.map(|p| (t, p));
            }
        }
        let Some(language) = language.filter(|l| caret_path::has_path(l)) else {
            self.shown = None;
            return;
        };
        if doc.buffer.text().len_bytes() > MAX_BYTES {
            self.shown = None;
            return;
        }
        let key = (
            tab,
            doc.buffer.version(),
            doc.buffer.selections().primary().head,
        );
        if self.requested == Some(key) {
            return;
        }
        let since = match self.changed_at {
            Some((k, since)) if k == key => since,
            _ => {
                let now = Instant::now();
                self.changed_at = Some((key, now));
                now
            }
        };
        let waited = since.elapsed();
        if waited < DEBOUNCE {
            ctx.request_repaint_after(DEBOUNCE - waited);
            return;
        }
        self.changed_at = None;
        self.requested = Some(key);
        let worker = self.worker.get_or_insert_with(|| Worker::start(ctx));
        let _ = worker.requests.send(Request {
            key,
            language: language.to_owned(),
            text: doc.buffer.text().clone(),
        });
    }

    /// The path to show for `tab`, if any.
    pub fn path(&self, tab: u64) -> Option<&str> {
        self.shown
            .as_ref()
            .filter(|(t, _)| *t == tab)
            .map(|(_, p)| p.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bracket_core::LineEnding;
    use bracket_core::encoding::TextEncoding;

    #[test]
    fn the_path_follows_the_caret_after_a_pause() {
        let ctx = egui::Context::default();
        let mut path = CaretPath::default();
        let mut doc = Document::new_empty(LineEnding::Lf, TextEncoding::UTF8);
        doc.buffer.insert("{\"a\": {\"b\": 1}}");
        doc.buffer
            .set_selections(bracket_core::Selections::caret(12));
        let started = Instant::now();
        while path.path(1).is_none() {
            assert!(started.elapsed() < Duration::from_secs(10), "no path");
            path.update(&ctx, 1, &doc, Some("json"));
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(path.path(1), Some("$.a.b"));
        assert_eq!(path.path(2), None, "another document");
        path.update(&ctx, 1, &doc, Some("csharp"));
        assert_eq!(path.path(1), None, "no path for other languages");
    }
}
