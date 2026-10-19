//! The outline of the active document, in the side panel: its symbols as a tree, with the one at
//! the caret marked. Symbols are extracted on a worker thread, a moment after the text stops
//! changing; the worker keeps a parser per recent document, so each update reuses the previous
//! tree-sitter tree.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use bracket_core::{Document, Rope};
use bracket_index::rules::RuleSet;
use bracket_index::symbols::{Symbol, SymbolKind, SymbolParser};
use bracket_syntax::Badge;
use eframe::egui::{self, Color32, FontId, Rect, Sense, Shape, Stroke, TextStyle, Ui, pos2, vec2};

use crate::i18n::Texts;
use crate::tabs;

/// Typing waits this long before the outline is worked out again.
const DEBOUNCE: Duration = Duration::from_millis(300);
/// Documents larger than this (in bytes) have no outline.
pub const MAX_BYTES: usize = 10 << 20;
/// Documents whose parser the worker keeps.
const PARSERS: usize = 8;
const ROW_HEIGHT: f32 = 22.0;
const INDENT: f32 = 14.0;

struct Request {
    tab: u64,
    version: u64,
    language: String,
    rules: Option<Arc<RuleSet>>,
    text: Rope,
}

/// What the worker found for a version of a document.
enum Found {
    Symbols(Vec<Symbol>),
    Unsupported,
    TooLarge,
}

struct Response {
    tab: u64,
    version: u64,
    found: Found,
}

/// The thread that extracts symbols. Only the newest request waiting is worked on.
struct Worker {
    requests: Sender<Request>,
    responses: Receiver<Response>,
}

impl Worker {
    fn start(ctx: &egui::Context) -> Self {
        let (requests, request_rx) = mpsc::channel::<Request>();
        let (response_tx, responses) = mpsc::channel();
        let ctx = ctx.clone();
        thread::Builder::new()
            .name("outline".to_owned())
            .spawn(move || {
                // The parsers of recent documents, newest last.
                let mut parsers: Vec<((u64, String), SymbolParser)> = Vec::new();
                while let Ok(first) = request_rx.recv() {
                    let request = request_rx.try_iter().last().unwrap_or(first);
                    let found = if request.text.len_bytes() > MAX_BYTES {
                        Found::TooLarge
                    } else {
                        let key = (request.tab, request.language.clone());
                        let parser = match parsers.iter().position(|(k, _)| *k == key) {
                            Some(i) => Some(parsers.remove(i).1),
                            None => SymbolParser::new(&request.language, request.rules.clone()),
                        };
                        match parser {
                            Some(mut parser) => {
                                let symbols = parser.symbols(request.text.to_string());
                                parsers.push((key, parser));
                                if parsers.len() > PARSERS {
                                    parsers.remove(0);
                                }
                                Found::Symbols(symbols)
                            }
                            None => Found::Unsupported,
                        }
                    };
                    let response = Response {
                        tab: request.tab,
                        version: request.version,
                        found,
                    };
                    if response_tx.send(response).is_err() {
                        return;
                    }
                    ctx.request_repaint();
                }
            })
            .expect("the outline thread starts");
        Self {
            requests,
            responses,
        }
    }
}

/// A symbol as Ctrl+Shift+O lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaletteSymbol {
    pub name: String,
    pub kind: SymbolKind,
    /// The symbol it is declared in, if any.
    pub container: Option<String>,
    /// 1-based.
    pub line: usize,
    pub name_range: Range<usize>,
}

/// The active document's symbols for the palette, or why there are none.
pub enum SymbolList<'a> {
    Ready {
        symbols: &'a [PaletteSymbol],
        /// Their names, for the fuzzy search.
        names: &'a [String],
    },
    /// Still being worked out.
    Pending,
    Unsupported,
    TooLarge,
}

/// The palette's list, built once per version of the outline.
struct PaletteCache {
    tab: u64,
    version: u64,
    symbols: Vec<PaletteSymbol>,
    names: Vec<String>,
}

/// The outline shown, and for which version of which tab.
struct Shown {
    tab: u64,
    version: u64,
    found: Found,
}

#[derive(Default)]
pub struct Outline {
    worker: Option<Worker>,
    shown: Option<Shown>,
    /// The tab and version last asked for, and the language.
    requested: Option<(u64, u64, Option<String>)>,
    /// When the active document last changed, for the debounce.
    changed_at: Option<Instant>,
    /// Collapsed symbols, by name, kind and depth, so they stay collapsed as the text changes.
    collapsed: HashSet<(String, SymbolKind, usize)>,
    /// Indices of the visible symbols.
    rows: Vec<usize>,
    rows_stale: bool,
    /// The symbol at the caret last frame; when it changes, it is scrolled into view.
    current: Option<usize>,
    scroll: (f32, f32),
    palette: Option<PaletteCache>,
}

impl Outline {
    /// The symbols of `tab` for Ctrl+Shift+O; `doc` gives their line numbers.
    pub fn palette_list(&mut self, tab: u64, doc: &Document) -> SymbolList<'_> {
        let Some(shown) = self.shown.as_ref().filter(|s| s.tab == tab) else {
            return SymbolList::Pending;
        };
        let symbols = match &shown.found {
            Found::Symbols(symbols) => symbols,
            Found::Unsupported => return SymbolList::Unsupported,
            Found::TooLarge => return SymbolList::TooLarge,
        };
        let fresh = self
            .palette
            .as_ref()
            .is_some_and(|c| c.tab == tab && c.version == shown.version);
        if !fresh {
            let text = doc.buffer.text();
            // The outline may be a little older than the text: positions are clamped.
            let line_of = |at: usize| text.char_to_line(at.min(text.len_chars())) + 1;
            let list: Vec<PaletteSymbol> = symbols
                .iter()
                .map(|s| PaletteSymbol {
                    name: s.name.clone(),
                    kind: s.kind,
                    container: s.parent.map(|p| symbols[p].name.clone()),
                    line: line_of(s.name_range.start),
                    name_range: s.name_range.clone(),
                })
                .collect();
            let names = list.iter().map(|s| s.name.clone()).collect();
            self.palette = Some(PaletteCache {
                tab,
                version: shown.version,
                symbols: list,
                names,
            });
        }
        let cache = self.palette.as_ref().expect("just built");
        SymbolList::Ready {
            symbols: &cache.symbols,
            names: &cache.names,
        }
    }

    /// Keeps the outline in step with the active tab. Call every frame while it is visible.
    pub fn update(
        &mut self,
        ctx: &egui::Context,
        tab: u64,
        doc: &Document,
        language: Option<&str>,
        rules: Option<Arc<RuleSet>>,
    ) {
        if let Some(worker) = &self.worker {
            for response in worker.responses.try_iter().collect::<Vec<_>>() {
                if response.tab != tab {
                    continue;
                }
                if self.shown.as_ref().is_none_or(|s| s.tab != tab) {
                    self.collapsed.clear();
                    self.current = None;
                }
                self.shown = Some(Shown {
                    tab: response.tab,
                    version: response.version,
                    found: response.found,
                });
                self.rows_stale = true;
            }
        }
        let version = doc.buffer.version();
        let wanted = (tab, version, language.map(str::to_owned));
        if self.requested.as_ref() == Some(&wanted) {
            return;
        }
        let Some(language) = language else {
            self.requested = Some(wanted);
            self.shown = Some(Shown {
                tab,
                version,
                found: Found::Unsupported,
            });
            self.rows_stale = true;
            return;
        };
        // A new tab (or language) is worked out at once; edits wait until typing stops.
        let same_document = self
            .requested
            .as_ref()
            .is_some_and(|(t, _, l)| *t == tab && l.as_deref() == Some(language));
        if same_document {
            let changed_at = *self.changed_at.get_or_insert_with(Instant::now);
            let waited = changed_at.elapsed();
            if waited < DEBOUNCE {
                ctx.request_repaint_after(DEBOUNCE - waited);
                return;
            }
        }
        self.changed_at = None;
        let worker = self.worker.get_or_insert_with(|| Worker::start(ctx));
        let _ = worker.requests.send(Request {
            tab,
            version,
            language: language.to_owned(),
            rules,
            text: doc.buffer.text().clone(),
        });
        self.requested = Some(wanted);
    }

    fn symbols(&self) -> &[Symbol] {
        match &self.shown {
            Some(Shown {
                found: Found::Symbols(symbols),
                ..
            }) => symbols,
            _ => &[],
        }
    }

    fn key(symbol: &Symbol) -> (String, SymbolKind, usize) {
        (symbol.name.clone(), symbol.kind, symbol.depth)
    }

    fn rebuild_rows(&mut self) {
        let symbols = self.symbols();
        let mut rows = Vec::with_capacity(symbols.len());
        // Depth below which symbols are hidden by a collapsed parent.
        let mut hidden_below: Option<usize> = None;
        for (index, symbol) in symbols.iter().enumerate() {
            if let Some(depth) = hidden_below {
                if symbol.depth > depth {
                    continue;
                }
                hidden_below = None;
            }
            rows.push(index);
            if self.collapsed.contains(&Self::key(symbol)) {
                hidden_below = Some(symbol.depth);
            }
        }
        self.rows = rows;
        self.rows_stale = false;
    }

    /// The deepest symbol whose range holds `caret`.
    fn at_caret(symbols: &[Symbol], caret: usize) -> Option<usize> {
        let mut best: Option<usize> = None;
        for (index, symbol) in symbols.iter().enumerate() {
            if symbol.range.start > caret {
                break;
            }
            if symbol.range.contains(&caret) || symbol.range.end == caret {
                best = Some(index);
            }
        }
        best
    }

    /// Draws the outline. Returns the name range of the symbol clicked, to go to it.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        tab: u64,
        doc: &Document,
        texts: &Texts,
    ) -> Option<Range<usize>> {
        let message = match &self.shown {
            Some(shown) if shown.tab == tab => match &shown.found {
                Found::Unsupported => Some(texts.outline_unsupported()),
                Found::TooLarge => Some(texts.outline_too_large()),
                Found::Symbols(symbols) if symbols.is_empty() => Some(texts.outline_none()),
                Found::Symbols(_) => None,
            },
            // Still being worked out.
            _ => return None,
        };
        if let Some(message) = message {
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                ui.weak(message);
            });
            return None;
        }
        if self.rows_stale {
            self.rebuild_rows();
        }
        let caret = doc.buffer.selections().primary().head;
        // While the outline is for an older version (typing), its ranges are a little off: the
        // symbol at the caret is not followed until it catches up.
        let stale = self
            .shown
            .as_ref()
            .is_some_and(|s| s.version != doc.buffer.version());
        let current = if stale {
            self.current
        } else {
            Self::at_caret(self.symbols(), caret)
        };
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt("outline-rows")
            .auto_shrink(false);
        if current != self.current {
            self.current = current;
            if let Some(row) = current.and_then(|c| self.rows.iter().position(|&r| r == c)) {
                let (offset, height) = self.scroll;
                let top = row as f32 * ROW_HEIGHT;
                if top < offset {
                    scroll = scroll.vertical_scroll_offset(top);
                } else if top + ROW_HEIGHT > offset + height && height > 0.0 {
                    scroll = scroll.vertical_scroll_offset(top + ROW_HEIGHT - height);
                }
            }
        }
        let mut go_to = None;
        let mut toggled = None;
        let symbols = self.symbols();
        let output = scroll.show_rows(ui, ROW_HEIGHT, self.rows.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for row in range {
                let index = self.rows[row];
                let symbol = &symbols[index];
                let has_children = symbols
                    .get(index + 1)
                    .is_some_and(|next| next.parent == Some(index));
                let collapsed = self.collapsed.contains(&Self::key(symbol));
                let expander = has_children.then_some(!collapsed);
                let (response, chevron) = symbol_row(ui, symbol, Some(index) == current, expander);
                if chevron {
                    toggled = Some(Self::key(symbol));
                } else if response.clicked() {
                    go_to = Some(symbol.name_range.clone());
                }
            }
        });
        self.scroll = (output.state.offset.y, output.inner_rect.height());
        if let Some(key) = toggled {
            if !self.collapsed.remove(&key) {
                self.collapsed.insert(key);
            }
            self.rows_stale = true;
        }
        go_to
    }
}

/// The letter and colour that tell a symbol's kind at a glance.
pub fn kind_badge(kind: SymbolKind) -> Badge {
    use SymbolKind::*;
    let (text, color) = match kind {
        Namespace => ("N", [0x6E, 0x76, 0x81]),
        Class => ("C", [0xC0, 0x6A, 0x1F]),
        Struct => ("S", [0xC0, 0x6A, 0x1F]),
        Interface => ("I", [0x2F, 0x6F, 0xD6]),
        Record => ("R", [0xC0, 0x6A, 0x1F]),
        Enum => ("E", [0xC0, 0x6A, 0x1F]),
        EnumMember => ("e", [0x2F, 0x6F, 0xD6]),
        Delegate => ("D", [0xC0, 0x6A, 0x1F]),
        Method | Constructor => ("M", [0x7C, 0x4D, 0xD8]),
        Property => ("P", [0x1E, 0x88, 0xC0]),
        Field => ("F", [0x1E, 0x88, 0xC0]),
        Event => ("Ev", [0xB8, 0x8A, 0x10]),
        Indexer => ("[]", [0x1E, 0x88, 0xC0]),
        Selector => ("#", [0x0F, 0x8A, 0x7A]),
        AtRule => ("@", [0x6E, 0x76, 0x81]),
        Keyframes => ("K", [0x0F, 0x8A, 0x7A]),
        Variable => ("V", [0x1E, 0x88, 0xC0]),
        Key => ("K", [0x1E, 0x88, 0xC0]),
        Element => ("<>", [0xC0, 0x6A, 0x1F]),
        Procedure => ("P", [0x7C, 0x4D, 0xD8]),
        Function => ("F", [0x7C, 0x4D, 0xD8]),
        View => ("V", [0xC0, 0x6A, 0x1F]),
        Table => ("T", [0xC0, 0x6A, 0x1F]),
        Trigger => ("Tr", [0x7C, 0x4D, 0xD8]),
        Type => ("T", [0xC0, 0x6A, 0x1F]),
        Directive => ("@", [0x6E, 0x76, 0x81]),
    };
    Badge {
        text: text.to_owned(),
        color,
    }
}

/// One symbol: indentation, an arrow if it has children, its kind and its name. Returns the
/// row and whether the arrow was clicked.
fn symbol_row(
    ui: &mut Ui,
    symbol: &Symbol,
    current: bool,
    expander: Option<bool>,
) -> (egui::Response, bool) {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            true,
            current,
            symbol.name.clone(),
        )
    });
    let left = rect.left() + 6.0 + symbol.depth as f32 * INDENT;
    let arrow_rect = Rect::from_min_size(pos2(left, rect.top()), vec2(16.0, ROW_HEIGHT));
    let arrow_clicked = expander.is_some()
        && ui
            .interact(
                arrow_rect,
                ui.id()
                    .with(("outline-arrow", &symbol.name, symbol.range.start)),
                Sense::click(),
            )
            .clicked();
    if !ui.is_rect_visible(rect) {
        return (response, arrow_clicked);
    }
    let visuals = ui.visuals();
    let painter = ui.painter();
    if current {
        painter.rect_filled(rect, 0.0, visuals.widgets.inactive.weak_bg_fill);
    } else if response.hovered() {
        painter.rect_filled(rect, 0.0, visuals.widgets.hovered.weak_bg_fill);
    }
    let text_color = visuals.text_color();
    let center_y = rect.center().y;
    if let Some(open) = expander {
        let center = pos2(left + 8.0, center_y);
        let s = 3.5;
        let points = if open {
            vec![
                center + vec2(-s, -s / 2.0),
                center + vec2(s, -s / 2.0),
                center + vec2(0.0, s),
            ]
        } else {
            vec![
                center + vec2(-s / 2.0, -s),
                center + vec2(s, 0.0),
                center + vec2(-s / 2.0, s),
            ]
        };
        painter.add(Shape::convex_polygon(points, text_color, Stroke::NONE));
    }
    let font = TextStyle::Body.resolve(ui.style());
    let badge_font = FontId::proportional((font.size * 0.7).round());
    let mut x = left + 16.0;
    x += tabs::paint_badge(
        painter,
        &kind_badge(symbol.kind),
        badge_font,
        pos2(x, center_y),
    ) + 6.0;
    let name = painter.layout_no_wrap(symbol.name.clone(), font, text_color);
    painter
        .with_clip_rect(Rect::from_min_max(pos2(x, rect.top()), rect.max))
        .galley(
            pos2(x, center_y - name.size().y / 2.0),
            name,
            Color32::WHITE,
        );
    (response, arrow_clicked)
}

/// Compiles the `symbols.toml` of the language packages that have one: by language id, and
/// the problems found, with the file each one is in.
pub fn package_rules(
    languages: &[bracket_syntax::Language],
) -> (
    HashMap<String, Arc<RuleSet>>,
    Vec<bracket_syntax::PackageWarning>,
) {
    let mut rules = HashMap::new();
    let mut warnings = Vec::new();
    for language in languages {
        let (Some(text), Some(folder)) = (&language.symbols, &language.package) else {
            continue;
        };
        match RuleSet::parse(text) {
            Ok(set) => {
                rules.insert(language.id.clone(), Arc::new(set));
            }
            Err(error) => warnings.push(bracket_syntax::PackageWarning::Syntax {
                path: folder.join(bracket_syntax::packages::SYMBOLS),
                error,
            }),
        }
    }
    (rules, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bracket_core::LineEnding;
    use bracket_core::encoding::TextEncoding;

    fn doc(text: &str) -> Document {
        let mut doc = Document::new_empty(LineEnding::Lf, TextEncoding::UTF8);
        doc.buffer.insert(text);
        doc
    }

    fn wait(outline: &mut Outline, ctx: &egui::Context, doc: &Document, language: Option<&str>) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            outline.update(ctx, 1, doc, language, None);
            if outline
                .shown
                .as_ref()
                .is_some_and(|s| s.version == doc.buffer.version())
            {
                return;
            }
            assert!(Instant::now() < deadline, "the outline never arrived");
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn the_outline_follows_the_document() {
        let ctx = egui::Context::default();
        let mut outline = Outline::default();
        let mut doc = doc("class A { void M() { } }\n");
        wait(&mut outline, &ctx, &doc, Some("csharp"));
        let names: Vec<&str> = outline.symbols().iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["A", "M"]);

        doc.buffer.insert("class B { }\n");
        wait(&mut outline, &ctx, &doc, Some("csharp"));
        assert_eq!(outline.symbols().len(), 3);
    }

    #[test]
    fn documents_without_symbols_say_why() {
        let ctx = egui::Context::default();
        let mut outline = Outline::default();
        let doc = doc("fn main() {}");
        outline.update(&ctx, 1, &doc, Some("rust"), None);
        wait(&mut outline, &ctx, &doc, Some("rust"));
        assert!(matches!(
            outline.shown.as_ref().unwrap().found,
            Found::Unsupported
        ));
        outline.update(&ctx, 1, &doc, None, None);
        assert!(matches!(
            outline.shown.as_ref().unwrap().found,
            Found::Unsupported
        ));
    }

    #[test]
    fn collapsed_symbols_hide_their_children_and_the_caret_symbol_is_the_deepest() {
        let ctx = egui::Context::default();
        let mut outline = Outline::default();
        let text = "class A {\n  void M() { }\n  int x;\n}\nclass B { }\n";
        let doc = doc(text);
        wait(&mut outline, &ctx, &doc, Some("csharp"));
        outline.rebuild_rows();
        assert_eq!(outline.rows.len(), 4);
        let a = Outline::key(&outline.symbols()[0]);
        outline.collapsed.insert(a);
        outline.rebuild_rows();
        assert_eq!(outline.rows, [0, 3], "A's members are hidden");
        let caret = text.find("M()").unwrap();
        assert_eq!(Outline::at_caret(outline.symbols(), caret), Some(1));
        let caret = text.find("class B").unwrap() + 3;
        assert_eq!(Outline::at_caret(outline.symbols(), caret), Some(3));
    }

    #[test]
    fn the_palette_list_has_lines_and_containers() {
        let ctx = egui::Context::default();
        let mut outline = Outline::default();
        let doc = doc("class A\n{\n  void M() { }\n}\n");
        assert!(matches!(outline.palette_list(1, &doc), SymbolList::Pending));
        wait(&mut outline, &ctx, &doc, Some("csharp"));
        let SymbolList::Ready { symbols, names } = outline.palette_list(1, &doc) else {
            panic!("the symbols are ready");
        };
        assert_eq!(names, ["A", "M"]);
        assert_eq!(symbols[1].line, 3);
        assert_eq!(symbols[1].container.as_deref(), Some("A"));
        assert!(matches!(outline.palette_list(2, &doc), SymbolList::Pending));
    }

    #[test]
    fn broken_package_rules_become_warnings() {
        let mut good = bracket_syntax::LanguageRegistry::builtin().languages()[0].clone();
        good.id = "python".into();
        good.package = Some("languages/python".into());
        good.symbols =
            Some("format_version = 1\n[[symbol]]\nkind = \"function\"\npattern = '^def'\n".into());
        let mut bad = good.clone();
        bad.id = "ini".into();
        bad.package = Some("languages/ini".into());
        bad.symbols = Some("format_version = 1\n".into());
        let (rules, warnings) = package_rules(&[good, bad]);
        assert!(rules.contains_key("python") && !rules.contains_key("ini"));
        assert_eq!(warnings.len(), 1);
    }
}
