//! Incremental, line-based syntax highlighting on a worker thread.
//!
//! The grammars are compiled at build time (see `build.rs`) and loaded the first time a file
//! needs them, on the worker thread. Each highlighted document has its own worker: the UI asks
//! for the lines it is about to show and gets back the colour spans of those lines.
//!
//! Highlighting a line needs the parser state at its start, which depends on every line above.
//! The engine keeps that state every [`CHECKPOINT_EVERY`] lines; after an edit only the
//! checkpoints below the first changed line are recomputed, and only as far as the visible
//! lines.

use std::ops::Range;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use bracket_core::Rope;
use syntect::highlighting::{
    Color, HighlightIterator, HighlightState, Highlighter, Theme, ThemeSet,
};
use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxReference, SyntaxSet};

use crate::packages::{self, PackageWarning};

/// Larger documents are not highlighted (the checkpoints and the first pass would cost too
/// much memory and time).
pub const MAX_HIGHLIGHT_BYTES: usize = 16 * 1024 * 1024;
/// Parser state is kept at the start of every this many lines.
pub const CHECKPOINT_EVERY: usize = 128;
/// Longer lines (in bytes) are shown without colours: regex grammars get very slow on them (minified
/// files), and the parser state carries over unchanged.
pub const MAX_LINE_BYTES: usize = 20_000;
/// The theme until themes can be chosen.
const THEME: &str = "InspiredGitHub";

static GRAMMARS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/grammars.packdump"));

/// Grammar files from language packages, set once at startup.
static EXTERNAL: OnceLock<Vec<PathBuf>> = OnceLock::new();
/// Problems found while compiling the external grammars.
static EXTERNAL_WARNINGS: OnceLock<Vec<PackageWarning>> = OnceLock::new();

/// Adds the grammars of language packages. Call before anything is highlighted; later calls
/// are ignored.
pub fn set_external_grammars(files: Vec<PathBuf>) {
    let _ = EXTERNAL.set(files);
}

/// Problems with the external grammars, once they have been compiled (`None` before).
pub fn external_grammar_warnings() -> Option<&'static [PackageWarning]> {
    EXTERNAL_WARNINGS.get().map(Vec::as_slice)
}

/// The compiled grammars, loaded on first use (about a millisecond; regexes compile lazily).
/// With external grammars they are compiled together, so a package can use or replace the
/// built-in ones (for one scope, the last grammar added wins).
pub fn grammars() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(|| {
        let builtin: SyntaxSet = syntect::dumps::from_binary(GRAMMARS);
        let files = EXTERNAL.get().map(Vec::as_slice).unwrap_or_default();
        let mut warnings = Vec::new();
        let set = if files.is_empty() {
            builtin
        } else {
            let mut builder = builtin.into_builder();
            for file in files {
                match packages::parse_grammar(file) {
                    Ok(syntax) => builder.add(syntax),
                    Err(warning) => warnings.push(warning),
                }
            }
            builder.build()
        };
        let _ = EXTERNAL_WARNINGS.set(warnings);
        set
    })
}

fn theme() -> &'static Theme {
    static THEME_CELL: OnceLock<Theme> = OnceLock::new();
    THEME_CELL.get_or_init(|| {
        ThemeSet::load_defaults()
            .themes
            .remove(THEME)
            .expect("syntect bundles the InspiredGitHub theme")
    })
}

/// A run of characters of one colour within a line (char offsets, end exclusive).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub color: [u8; 3],
}

/// Highlighting of a range of lines, for one version of the text.
#[derive(Clone, Debug)]
pub struct Highlights {
    pub version: u64,
    pub first_line: usize,
    /// Spans of each line from `first_line`. Text in the default colour has no span.
    pub lines: Vec<Vec<Span>>,
    /// The text that was highlighted (a cheap rope clone).
    text: Rope,
}

impl Highlights {
    /// The spans for `line` of `text` (at `version`). Results for an older version are still
    /// used for lines whose text did not change, so editing one line does not make the others
    /// flicker while the new result is computed.
    pub fn spans(&self, line: usize, text: &Rope, version: u64) -> Option<&[Span]> {
        let spans = self.lines.get(line.checked_sub(self.first_line)?)?;
        let same = version == self.version
            || (line < self.text.len_lines()
                && line < text.len_lines()
                && self.text.line(line) == text.line(line));
        same.then_some(spans.as_slice())
    }
}

/// The highlighting state of one document. Synchronous; [`HighlightWorker`] runs it on a thread.
pub struct Engine {
    syntax: &'static SyntaxReference,
    highlighter: Highlighter<'static>,
    default_color: Color,
    /// `checkpoints[i]` is the state at the start of line `i * CHECKPOINT_EVERY`.
    checkpoints: Vec<(ParseState, HighlightState)>,
    /// How many checkpoints are known to be right. The ones after them come from before an
    /// edit: if parsing reaches one with the same state, it and all below are right again.
    valid: usize,
    /// Old checkpoints from this index on are below every line changed since they were made,
    /// so they can match. 0 when there are no old checkpoints.
    reusable_from: usize,
    text: Rope,
}

impl Engine {
    /// `None` if no grammar has this scope.
    pub fn new(scope: &str) -> Option<Self> {
        let set = grammars();
        let syntax = set.find_syntax_by_scope(Scope::new(scope).ok()?)?;
        let theme = theme();
        let highlighter = Highlighter::new(theme);
        let initial = (
            ParseState::new(syntax),
            HighlightState::new(&highlighter, ScopeStack::new()),
        );
        Some(Self {
            syntax,
            default_color: theme.settings.foreground.unwrap_or(Color::BLACK),
            highlighter,
            checkpoints: vec![initial],
            valid: 1,
            reusable_from: 0,
            text: Rope::new(),
        })
    }

    /// Takes a new version of the text. Checkpoints above its first changed line stay right.
    /// If the number of lines is the same (typing within a line), the ones below are kept in
    /// case the parser state comes back to them; otherwise lines moved and they are dropped.
    pub fn set_text(&mut self, text: &Rope) {
        let Some(changed) = first_changed_line(&self.text, text) else {
            return;
        };
        let still_valid = changed / CHECKPOINT_EVERY + 1;
        if self.text.len_lines() == text.len_lines() {
            let last_changed = last_changed_line(&self.text, text, changed);
            self.reusable_from = self.reusable_from.max(last_changed / CHECKPOINT_EVERY + 1);
        } else {
            // Every checkpoint left is right, so there is nothing old to come back to.
            self.checkpoints.truncate(still_valid);
            self.reusable_from = 0;
        }
        self.valid = self.valid.min(still_valid).min(self.checkpoints.len());
        self.text = text.clone();
    }

    /// The spans of `lines`. Parses from the nearest right checkpoint above, adding or
    /// correcting checkpoints on the way. Every few hundred lines `cancelled` is asked whether
    /// to give up (a newer request arrived); the checkpoints made so far are kept.
    pub fn highlight(
        &mut self,
        lines: Range<usize>,
        mut cancelled: impl FnMut() -> bool,
    ) -> Option<Vec<Vec<Span>>> {
        let end = lines.end.min(self.text.len_lines());
        let mut line = (lines.start / CHECKPOINT_EVERY).min(self.valid - 1) * CHECKPOINT_EVERY;
        let (mut parse, mut highlight) = self.checkpoints[line / CHECKPOINT_EVERY].clone();
        let mut out = Vec::with_capacity(lines.len());
        let mut line_text = String::new();
        while line < end {
            if line.is_multiple_of(CHECKPOINT_EVERY) {
                let index = line / CHECKPOINT_EVERY;
                if index >= self.valid {
                    let same = index >= self.reusable_from
                        && self
                            .checkpoints
                            .get(index)
                            .is_some_and(|(p, h)| *p == parse && *h == highlight);
                    if same {
                        // Back to the state from before the edits: everything below is right,
                        // so continue from the checkpoint nearest to the requested lines.
                        self.valid = self.checkpoints.len();
                        self.reusable_from = 0;
                        let nearest = (lines.start / CHECKPOINT_EVERY).min(self.valid - 1);
                        if nearest > index {
                            line = nearest * CHECKPOINT_EVERY;
                            (parse, highlight) = self.checkpoints[nearest].clone();
                            continue;
                        }
                    } else {
                        // Different: correct this one; the old ones below may still match
                        // further down (a comment opened and closed again).
                        let state = (parse.clone(), highlight.clone());
                        match self.checkpoints.get_mut(index) {
                            Some(old) => *old = state,
                            None => self.checkpoints.push(state),
                        }
                        self.valid = index + 1;
                    }
                }
                if line.is_multiple_of(CHECKPOINT_EVERY * 4) && cancelled() {
                    return None;
                }
            }
            line_text.clear();
            line_text.extend(self.text.line(line).chunks());
            let spans = self.line(&mut parse, &mut highlight, &line_text);
            if line >= lines.start {
                out.push(spans);
            }
            line += 1;
        }
        Some(out)
    }

    fn line(
        &self,
        parse: &mut ParseState,
        highlight: &mut HighlightState,
        text: &str,
    ) -> Vec<Span> {
        if text.len() > MAX_LINE_BYTES {
            return Vec::new();
        }
        let Ok(ops) = parse.parse_line(text, grammars()) else {
            return Vec::new();
        };
        let mut spans: Vec<Span> = Vec::new();
        let mut column = 0;
        for (style, piece) in HighlightIterator::new(highlight, &ops, text, &self.highlighter) {
            let len = piece.chars().count();
            let color = style.foreground;
            if color != self.default_color && !piece.trim_end_matches(['\r', '\n']).is_empty() {
                let color = [color.r, color.g, color.b];
                match spans.last_mut() {
                    Some(last) if last.end == column && last.color == color => last.end += len,
                    _ => spans.push(Span {
                        start: column,
                        end: column + len,
                        color,
                    }),
                }
            }
            column += len;
        }
        spans
    }

    pub fn syntax_name(&self) -> &str {
        &self.syntax.name
    }
}

/// The last line where `a` and `b` differ, for texts with the same number of lines that first
/// differ at `first`.
fn last_changed_line(a: &Rope, b: &Rope, first: usize) -> usize {
    (first..a.len_lines())
        .rev()
        .find(|&line| a.line(line) != b.line(line))
        .unwrap_or(first)
}

/// The first line where `a` and `b` differ (or where the shorter one ends); `None` if they
/// are the same.
fn first_changed_line(a: &Rope, b: &Rope) -> Option<usize> {
    let mut line = 0;
    for (x, y) in a.lines().zip(b.lines()) {
        if x != y {
            return Some(line);
        }
        line += 1;
    }
    (a.len_lines() != b.len_lines()).then(|| line.saturating_sub(1))
}

struct Request {
    version: u64,
    text: Rope,
    lines: Range<usize>,
}

/// Highlights one document on its own thread. The thread ends when the worker is dropped.
pub struct HighlightWorker {
    requests: Sender<Request>,
    results: Receiver<Highlights>,
}

impl HighlightWorker {
    /// `notify` runs on the worker thread after each result (to wake the UI).
    pub fn start(scope: String, notify: impl Fn() + Send + 'static) -> Self {
        let (requests, request_rx) = mpsc::channel::<Request>();
        let (result_tx, results) = mpsc::channel();
        thread::spawn(move || {
            let Some(mut engine) = Engine::new(&scope) else {
                return;
            };
            let mut pending = None;
            loop {
                let first = match pending.take() {
                    Some(request) => request,
                    None => match request_rx.recv() {
                        Ok(request) => request,
                        Err(_) => return,
                    },
                };
                // Only the newest request matters.
                let request = request_rx.try_iter().last().unwrap_or(first);
                engine.set_text(&request.text);
                let mut newer = None;
                let lines = engine.highlight(request.lines.clone(), || {
                    newer = request_rx.try_iter().last();
                    newer.is_some()
                });
                match lines {
                    Some(lines) => {
                        let result = Highlights {
                            version: request.version,
                            first_line: request.lines.start,
                            lines,
                            text: request.text,
                        };
                        if result_tx.send(result).is_err() {
                            return;
                        }
                        notify();
                    }
                    None => pending = newer,
                }
            }
        });
        Self { requests, results }
    }

    /// Asks for the spans of `lines` of `text`, which is at `version`.
    pub fn request(&self, version: u64, text: Rope, lines: Range<usize>) {
        let _ = self.requests.send(Request {
            version,
            text,
            lines,
        });
    }

    /// The newest result, if one arrived since the last call.
    pub fn latest(&self) -> Option<Highlights> {
        self.results.try_iter().last()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn highlight_all(scope: &str, text: &str) -> Vec<Vec<Span>> {
        let mut engine = Engine::new(scope).unwrap();
        let rope = Rope::from_str(text);
        engine.set_text(&rope);
        engine.highlight(0..rope.len_lines(), || false).unwrap()
    }

    /// The scopes at the start of `needle` in `text`, as one string.
    fn scopes_at(scope: &str, text: &str, needle: &str) -> String {
        let set = grammars();
        let syntax = set
            .find_syntax_by_scope(Scope::new(scope).unwrap())
            .unwrap();
        let target = text.find(needle).unwrap();
        let mut parse = ParseState::new(syntax);
        let mut stack = ScopeStack::new();
        let mut offset = 0;
        for line in text.split_inclusive('\n') {
            let ops = parse.parse_line(line, set).unwrap();
            for (at, op) in ops {
                if offset + at > target {
                    break;
                }
                stack.apply(&op).unwrap();
            }
            if offset + line.len() > target {
                break;
            }
            offset += line.len();
        }
        format!("{stack:?}")
    }

    #[test]
    fn every_builtin_language_has_a_grammar() {
        for scope in [
            "source.cs",
            "text.html.razor",
            "source.css",
            "source.tsql",
            "source.json",
            "text.xml",
        ] {
            assert!(Engine::new(scope).is_some(), "no grammar for {scope}");
        }
        assert!(Engine::new("source.cobol").is_none());
    }

    #[test]
    fn keywords_get_a_colour_and_plain_text_does_not() {
        let lines = highlight_all("source.cs", "public class Foo { }\n");
        let spans = &lines[0];
        assert!(
            spans.iter().any(|s| s.start == 0 && s.end == 6),
            "{spans:?}"
        );
        assert!(spans.iter().all(|s| s.end <= "public class Foo { }".len()));
    }

    #[test]
    fn edits_give_the_same_result_as_starting_over() {
        let before: String = (0..600)
            .map(|i| format!("int x{i} = {i}; // n\n"))
            .collect();
        let mut engine = Engine::new("source.cs").unwrap();
        engine.set_text(&Rope::from_str(&before));
        engine.highlight(0..600, || false).unwrap();

        // Open a comment in the middle: every line below changes colour.
        let after = before.replacen("int x300", "/* int x300", 1);
        let rope = Rope::from_str(&after);
        engine.set_text(&rope);
        let incremental = engine.highlight(500..520, || false).unwrap();
        let fresh = highlight_all("source.cs", &after)[500..520].to_vec();
        assert_eq!(incremental, fresh);
        assert_ne!(
            incremental,
            highlight_all("source.cs", &before)[500..520].to_vec()
        );
    }

    #[test]
    fn typing_within_a_line_reuses_the_checkpoints_below() {
        let before: String = (0..2000).map(|i| format!("int x{i} = {i};\n")).collect();
        let mut engine = Engine::new("source.cs").unwrap();
        engine.set_text(&Rope::from_str(&before));
        engine.highlight(0..2000, || false).unwrap();
        let checkpoints = engine.checkpoints.len();

        let after = before.replacen("int x10 =", "int x10a =", 1);
        let rope = Rope::from_str(&after);
        engine.set_text(&rope);
        assert_eq!(engine.valid, 1);
        let page = engine.highlight(1900..1950, || false).unwrap();
        assert_eq!(
            engine.valid, checkpoints,
            "the old checkpoints were right again"
        );
        assert_eq!(
            page,
            highlight_all("source.cs", &after)[1900..1950].to_vec()
        );
    }

    #[test]
    fn edits_in_two_places_do_not_reuse_stale_checkpoints() {
        let before: String = (0..2000).map(|i| format!("int x{i} = {i};\n")).collect();
        let mut engine = Engine::new("source.cs").unwrap();
        engine.set_text(&Rope::from_str(&before));
        engine.highlight(0..2000, || false).unwrap();

        // A small change near the top and a comment opened lower down, in one step.
        let after = before.replacen("int x10 =", "int x10a =", 1).replacen(
            "int x1500 =",
            "/* int x1500 =",
            1,
        );
        engine.set_text(&Rope::from_str(&after));
        let page = engine.highlight(1900..1950, || false).unwrap();
        assert_eq!(
            page,
            highlight_all("source.cs", &after)[1900..1950].to_vec()
        );
    }

    #[test]
    fn cancelling_keeps_the_work_done() {
        let text: String = (0..3000).map(|i| format!("int a{i};\n")).collect();
        let mut engine = Engine::new("source.cs").unwrap();
        engine.set_text(&Rope::from_str(&text));
        let mut asked = 0;
        let result = engine.highlight(2900..2910, || {
            asked += 1;
            asked > 2
        });
        assert!(result.is_none());
        assert!(
            engine.checkpoints.len() > 1,
            "checkpoints made before cancelling stay"
        );
    }

    #[test]
    fn stale_results_are_used_for_unchanged_lines() {
        let old = Rope::from_str("int a;\nint b;\n");
        let highlights = Highlights {
            version: 1,
            first_line: 0,
            lines: vec![vec![], vec![]],
            text: old,
        };
        let new = Rope::from_str("int a;\nint bb;\n");
        assert!(highlights.spans(0, &new, 2).is_some());
        assert!(highlights.spans(1, &new, 2).is_none());
        assert!(highlights.spans(1, &new, 1).is_some(), "same version");
        assert!(highlights.spans(5, &new, 1).is_none());
    }

    #[test]
    fn the_worker_answers_requests() {
        let worker = HighlightWorker::start("source.json".to_owned(), || {});
        let text = Rope::from_str("{\"a\": 1}\n");
        worker.request(7, text, 0..1);
        let mut result = None;
        for _ in 0..200 {
            result = worker.latest();
            if result.is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        let result = result.expect("a result");
        assert_eq!(
            (result.version, result.first_line, result.lines.len()),
            (7, 0, 1)
        );
        assert!(!result.lines[0].is_empty());
    }

    #[test]
    fn tsql_grammar() {
        let text = "SELECT [Name], @id, @@ROWCOUNT FROM #tmp WHERE x = N'it''s' -- note\nGO\n/* a /* b */ c */ SELECT 1\n";
        let at = |needle| scopes_at("source.tsql", text, needle);
        assert!(at("SELECT").contains("keyword.other.tsql"));
        assert!(at("Name").contains("variable.other.quoted.tsql"));
        assert!(at("@id").contains("variable.parameter.tsql"));
        assert!(at("@@ROW").contains("support.variable.system.tsql"));
        assert!(at("#tmp").contains("variable.other.temporary-table.tsql"));
        assert!(at("s' --").contains("string.quoted.single.tsql"));
        assert!(at("note").contains("comment.line.double-dash.tsql"));
        assert!(at("GO").contains("keyword.control.batch.tsql"));
        assert!(
            at(" c */").contains("comment.block.tsql"),
            "nested comments"
        );
        assert!(!at("1\n").contains("comment"));
    }

    #[test]
    fn razor_grammar() {
        let text = "@page \"/counter\"\n@using System.Linq\n<p class=\"x\">@Model.Items[0].Name and a@b.com</p>\n@* note *@\n@if (count > 1)\n<b>@(count + 1)</b>\n@code {\n    private int count = 0;\n    void Inc() { if (count < 9) { count++; } }\n}\n<i>after</i>\n";
        let at = |needle| scopes_at("text.html.razor", text, needle);
        assert!(at("page").contains("keyword.control.razor"));
        assert!(at("/counter").contains("string.quoted.double.razor"));
        assert!(at("System").contains("support.type.razor"));
        assert!(at("class=").contains("text.html.basic"));
        assert!(at("Model").contains("variable.other.razor"));
        assert!(
            !at("b.com").contains("variable.other.razor"),
            "e-mail is text"
        );
        assert!(at("note").contains("comment.block.razor"));
        assert!(at("count >").contains("source.cs.embedded.razor"));
        assert!(at("count + 1").contains("source.cs.embedded.razor"));
        assert!(at("private").contains("source.cs.embedded.razor"));
        assert!(at("private").contains("storage.modifier"));
        assert!(at("int count").contains("storage.type"));
        assert!(
            at("count++").contains("source.cs.embedded.razor"),
            "nested braces"
        );
        assert!(
            !at("after").contains("source.cs"),
            "markup again after @code"
        );
    }
}
