//! Ctrl+P: going to a file by typing part of its path.
//!
//! The project's file index lives on its own thread, which builds it when the project opens,
//! keeps it up to date with the project watcher's changes and answers queries (only the newest
//! one when several are waiting). The palette shows recent files while the query is empty and
//! understands `file:line`, `file:line:column` and `:line`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use bracket_core::CancelToken;
use bracket_index::fuzzy::{self, Query};
use bracket_index::project_symbols::FoundSymbol;
use bracket_index::symbols::SymbolKind;
use bracket_index::{FileIndex, WalkOptions, walk};
use bracket_session::ProjectChanges;
use bracket_syntax::LanguageRegistry;
use eframe::egui::{
    self, Align2, Color32, FontId, Id, Key, Modifiers, Order, Rect, Sense, TextStyle, Ui, pos2,
    text::{CCursor, CCursorRange, LayoutJob, TextFormat},
    vec2,
};

use crate::commands::Command;
use crate::i18n::Texts;
use crate::outline::{PaletteSymbol, SymbolList, kind_badge};
use crate::symbol_index::FilesChanged;
use crate::symbol_index::SymbolWorker;
use crate::tabs;

/// Results kept for one query.
const LIMIT: usize = 200;
const ROW_HEIGHT: f32 = 24.0;
const VISIBLE_ROWS: f32 = 12.0;

/// A file found for a query: its path relative to the project and the matched characters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub relative: String,
    pub positions: Vec<usize>,
}

enum IndexCommand {
    Changes(ProjectChanges),
    Query { text: String, generation: u64 },
}

pub enum IndexEvent {
    /// The index is built (or changed) and holds this many files.
    Ready(usize),
    Results {
        generation: u64,
        found: Vec<Found>,
    },
}

/// The thread that owns the project's file index.
pub struct IndexWorker {
    commands: Sender<IndexCommand>,
    events: Receiver<IndexEvent>,
    cancel: CancelToken,
}

impl IndexWorker {
    /// Starts building the index of `root`. `notify` is called whenever an event is ready;
    /// `on_files` with the list once built and with every change after that.
    pub fn start(
        root: PathBuf,
        options: WalkOptions,
        notify: impl Fn() + Send + 'static,
        on_files: impl Fn(FilesChanged) + Send + 'static,
    ) -> Self {
        let (commands, command_rx) = mpsc::channel();
        let (event_tx, events) = mpsc::channel();
        let cancel = CancelToken::new();
        let worker_cancel = cancel.clone();
        thread::Builder::new()
            .name("file-index".to_owned())
            .spawn(move || {
                let send = |event| {
                    let sent = event_tx.send(event).is_ok();
                    notify();
                    sent
                };
                let Some(mut index) = build(&root, &options, &worker_cancel) else {
                    return;
                };
                if !send(IndexEvent::Ready(index.files.len())) {
                    return;
                }
                let mut files = index.files.clone();
                on_files(FilesChanged {
                    files: files.clone(),
                    changed: Vec::new(),
                    full: true,
                });
                while let Ok(first) = command_rx.recv() {
                    let mut query = None;
                    // Whether the list changed, and the paths to read again.
                    let mut changed = false;
                    let mut touched = None::<FilesChanged>;
                    for command in std::iter::once(first).chain(command_rx.try_iter()) {
                        match command {
                            IndexCommand::Changes(changes) => {
                                let full = changes.changes_listing()
                                    && apply(&mut index, &options, &changes, &worker_cancel);
                                changed |= changes.changes_listing();
                                let touched = touched.get_or_insert_with(|| FilesChanged {
                                    files: files.clone(),
                                    changed: Vec::new(),
                                    full: false,
                                });
                                touched.full |= full;
                                touched
                                    .changed
                                    .extend(changes.paths.into_iter().chain(changes.modified));
                            }
                            IndexCommand::Query { text, generation } => {
                                query = Some((text, generation));
                            }
                        }
                    }
                    if worker_cancel.is_cancelled() {
                        return;
                    }
                    if changed {
                        files = index.files.clone();
                        if !send(IndexEvent::Ready(index.files.len())) {
                            return;
                        }
                    }
                    if let Some(mut touched) = touched {
                        touched.files = files.clone();
                        on_files(touched);
                    }
                    if let Some((text, generation)) = query {
                        let found = search(&index.files, &text);
                        if !send(IndexEvent::Results { generation, found }) {
                            return;
                        }
                    }
                }
            })
            .expect("the file index thread starts");
        Self {
            commands,
            events,
            cancel,
        }
    }

    pub fn changes(&self, changes: ProjectChanges) {
        let _ = self.commands.send(IndexCommand::Changes(changes));
    }

    pub fn query(&self, text: String, generation: u64) {
        let _ = self.commands.send(IndexCommand::Query { text, generation });
    }

    pub fn events(&self) -> Vec<IndexEvent> {
        self.events.try_iter().collect()
    }
}

impl Drop for IndexWorker {
    /// A walk still running stops.
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

/// Walks the project; invalid exclusions are dropped (the tree reports them). `None` if
/// cancelled.
fn build(root: &Path, options: &WalkOptions, cancel: &CancelToken) -> Option<FileIndex> {
    let result = match walk(root, options, cancel) {
        Ok(result) => result,
        Err(_) => walk(root, &WalkOptions::default(), cancel).ok()?,
    };
    result.ok()
}

/// Brings the index up to date: folder by folder, or entirely again when the ignore rules
/// changed or notifications were lost. Returns whether it was built again.
fn apply(
    index: &mut FileIndex,
    options: &WalkOptions,
    changes: &ProjectChanges,
    cancel: &CancelToken,
) -> bool {
    let rules_changed = changes.paths.iter().any(|path| {
        path.file_name().is_some_and(|n| {
            n.eq_ignore_ascii_case(".gitignore") || n.eq_ignore_ascii_case(".ignore")
        })
    });
    if changes.rescan || rules_changed {
        if let Some(fresh) = build(&index.root, options, cancel) {
            *index = fresh;
        }
        return true;
    }
    // Parents first: a new folder is walked from its parent, and then its own changes are
    // already in.
    let dirs: BTreeSet<(usize, PathBuf)> = changes
        .paths
        .iter()
        .filter_map(|path| path.parent())
        .map(|dir| (dir.components().count(), dir.to_path_buf()))
        .collect();
    for (_, dir) in dirs {
        let _ = index
            .resync_dir(&dir, options)
            .or_else(|_| index.resync_dir(&dir, &WalkOptions::default()));
    }
    false
}

fn search(files: &[String], text: &str) -> Vec<Found> {
    fuzzy::search(files, &Query::new(text), LIMIT)
        .into_iter()
        .map(|m| Found {
            relative: files[m.index].clone(),
            positions: m.positions,
        })
        .collect()
}

/// A line, and optionally a column, typed after the file name.
pub type LinePosition = (usize, Option<usize>);

/// Splits `file:line:column`, `file:line` or `:line` into the file part and the position.
pub fn parse_query(text: &str) -> (&str, Option<LinePosition>) {
    fn split_number(text: &str) -> Option<(&str, usize)> {
        let (head, tail) = text.rsplit_once(':')?;
        let number = tail.trim().parse().ok().filter(|n| *n > 0)?;
        Some((head, number))
    }
    let text = text.trim();
    match split_number(text) {
        Some((head, second)) => match split_number(head) {
            Some((file, first)) => (file.trim(), Some((first, Some(second)))),
            None => (head.trim(), Some((second, None))),
        },
        None => (text, None),
    }
}

/// What the user chose in the palette.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteAction {
    Open {
        path: PathBuf,
        line: Option<LinePosition>,
    },
    /// `:line` in the active document.
    GoToLine(LinePosition),
    /// `@symbol`: select this range (a symbol's name) in the active document.
    GoTo(std::ops::Range<usize>),
    /// `#symbol`: open a file with the caret at a symbol's name: line and column, 1-based.
    GoToSymbol {
        path: PathBuf,
        position: (usize, usize),
    },
    Run(Command),
    Close,
}

/// One entry of the list.
struct Item {
    path: PathBuf,
    /// Shown in the list: relative to the project when inside it.
    shown: String,
    positions: Vec<usize>,
}

/// The Ctrl+P palette while it is open.
pub struct QuickOpen {
    text: String,
    /// Bumped on every change of the query; results for older ones are dropped.
    generation: u64,
    found: Vec<Found>,
    found_generation: u64,
    /// The project's symbols for `#`, and the query they answer.
    symbols_found: Vec<FoundSymbol>,
    symbols_generation: u64,
    recent: Vec<PathBuf>,
    selected: usize,
    first_frame: bool,
}

impl QuickOpen {
    /// `recent`: recently opened files, newest first. `prefix`: what the box starts with:
    /// nothing for files, `>` for commands, `:` for a line.
    pub fn new(recent: Vec<PathBuf>, prefix: &str) -> Self {
        Self {
            text: prefix.to_owned(),
            generation: 0,
            found: Vec::new(),
            found_generation: 0,
            symbols_found: Vec::new(),
            symbols_generation: 0,
            recent: recent.into_iter().filter(|p| p.is_file()).collect(),
            selected: 0,
            first_frame: true,
        }
    }

    /// Takes the results of a query; old ones are dropped.
    pub fn results(&mut self, generation: u64, found: Vec<Found>) {
        if generation == self.generation {
            self.found = found;
            self.found_generation = generation;
            self.selected = 0;
        }
    }

    /// Takes the results of a project symbol query; old ones are dropped.
    pub fn symbol_results(&mut self, generation: u64, found: Vec<FoundSymbol>) {
        if generation == self.generation {
            self.symbols_found = found;
            self.symbols_generation = generation;
            self.selected = 0;
        }
    }

    /// The query for the file or symbol index, if what it searches for changed and is not
    /// empty.
    fn query_changed(&mut self, index: Option<&IndexWorker>, symbol_index: Option<&SymbolWorker>) {
        self.generation += 1;
        self.selected = 0;
        let (file, _) = parse_query(&self.text);
        match Mode::of(&self.text) {
            Mode::Files if !file.is_empty() => {
                if let Some(index) = index {
                    index.query(file.to_owned(), self.generation);
                }
            }
            Mode::ProjectSymbols => {
                let name = self.text[1..].trim();
                if let Some(symbol_index) = symbol_index
                    && !name.is_empty()
                {
                    symbol_index.query(name.to_owned(), self.generation);
                }
            }
            _ => {}
        }
    }

    /// The entries to list now.
    fn items(&self, root: Option<&Path>, has_index: bool) -> Vec<Item> {
        let shown = |path: &Path| match root.and_then(|r| path.strip_prefix(r).ok()) {
            Some(relative) => relative.display().to_string(),
            None => path.display().to_string(),
        };
        let (file, _) = parse_query(&self.text);
        if file.is_empty() {
            return self
                .recent
                .iter()
                .map(|path| Item {
                    shown: shown(path),
                    path: path.clone(),
                    positions: Vec::new(),
                })
                .collect();
        }
        match root {
            Some(root) if has_index => self
                .found
                .iter()
                .map(|found| Item {
                    path: root.join(&found.relative),
                    shown: found.relative.clone(),
                    positions: found.positions.clone(),
                })
                .collect(),
            // No project: the recent files are all there is to search.
            _ => {
                let shown_recent: Vec<String> = self.recent.iter().map(|p| shown(p)).collect();
                fuzzy::search(&shown_recent, &Query::new(file), LIMIT)
                    .into_iter()
                    .map(|m| Item {
                        path: self.recent[m.index].clone(),
                        shown: shown_recent[m.index].clone(),
                        positions: m.positions,
                    })
                    .collect()
            }
        }
    }

    /// The project symbols found for the current query; none while the box asks for nothing.
    fn shown_symbols(&self) -> &[FoundSymbol] {
        if self.text[1..].trim().is_empty() {
            &[]
        } else {
            &self.symbols_found
        }
    }

    /// Whether the palette lists the active document's symbols (`@`), so they must be ready.
    pub fn wants_symbols(&self) -> bool {
        Mode::of(&self.text) == Mode::Symbols
    }

    /// Draws the palette. `indexed`: the number of files in the index, once it is built;
    /// `commands`: the commands available now; `lines`: the active document's line count;
    /// `symbols`: its symbols, for `@`.
    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        root: Option<&Path>,
        index: Option<&IndexWorker>,
        indexed: Option<usize>,
        symbol_index: Option<&SymbolWorker>,
        symbols_indexed: Option<usize>,
        commands: &[PaletteCommand],
        lines: Option<usize>,
        symbols: SymbolList<'_>,
        registry: &LanguageRegistry,
        texts: &Texts,
    ) -> Option<PaletteAction> {
        let mode = Mode::of(&self.text);
        let items = match mode {
            Mode::Files => self.items(root, index.is_some() && indexed.is_some()),
            _ => Vec::new(),
        };
        let matches = match mode {
            Mode::Commands => command_matches(commands, self.text[1..].trim()),
            _ => Vec::new(),
        };
        let symbol_list: &[PaletteSymbol] = match &symbols {
            SymbolList::Ready { symbols, .. } => symbols,
            _ => &[],
        };
        let symbol_matches = match (&symbols, mode) {
            (SymbolList::Ready { names, .. }, Mode::Symbols) => {
                symbol_matches(names, self.text[1..].trim())
            }
            _ => Vec::new(),
        };
        let (file, line) = parse_query(&self.text);
        let file = file.to_owned();
        let go_to_line = mode == Mode::Line && line.is_some();

        // List keys, before the text field sees them.
        let count = match mode {
            Mode::Files => items.len(),
            Mode::Commands => matches.len(),
            Mode::Symbols => symbol_matches.len(),
            Mode::ProjectSymbols => self.shown_symbols().len(),
            Mode::Line => usize::from(go_to_line),
        };
        let pressed = |key| ctx.input_mut(|i| i.consume_key(Modifiers::NONE, key));
        let page = VISIBLE_ROWS as usize - 1;
        if count > 0 {
            if pressed(Key::ArrowDown) {
                self.selected = (self.selected + 1) % count;
            }
            if pressed(Key::ArrowUp) {
                self.selected = (self.selected + count - 1) % count;
            }
            if pressed(Key::PageDown) {
                self.selected = (self.selected + page).min(count - 1);
            }
            if pressed(Key::PageUp) {
                self.selected = self.selected.saturating_sub(page);
            }
        }
        self.selected = self.selected.min(count.saturating_sub(1));
        if pressed(Key::Escape) {
            return Some(PaletteAction::Close);
        }
        if pressed(Key::Enter) {
            let chosen = match mode {
                Mode::Line => line.map(PaletteAction::GoToLine),
                Mode::Commands => matches
                    .get(self.selected)
                    .map(|(i, _)| PaletteAction::Run(commands[*i].command)),
                Mode::Symbols => symbol_matches
                    .get(self.selected)
                    .map(|(i, _)| PaletteAction::GoTo(symbol_list[*i].name_range.clone())),
                Mode::ProjectSymbols => root.and_then(|root| {
                    let found = self.shown_symbols().get(self.selected)?;
                    Some(go_to_symbol(root, found))
                }),
                Mode::Files => items.get(self.selected).map(|item| PaletteAction::Open {
                    path: item.path.clone(),
                    line,
                }),
            };
            if chosen.is_some() {
                return chosen;
            }
        }

        let mut action = None;
        let screen = ctx.content_rect();
        let width = (screen.width() - 32.0).clamp(200.0, 640.0);
        let area = egui::Area::new(Id::new("quick-open"))
            .order(Order::Foreground)
            .anchor(Align2::CENTER_TOP, vec2(0.0, 48.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_width(width);
                    let edit_id = Id::new("quick-open-text");
                    let mut edit = egui::TextEdit::singleline(&mut self.text)
                        .id(edit_id)
                        .hint_text(texts.quick_open_hint())
                        .desired_width(f32::INFINITY)
                        .show(ui);
                    if self.first_frame {
                        // After a prefix typed for the user (">" or ":"), ready to type on.
                        edit.response.request_focus();
                        let end = CCursor::new(self.text.chars().count());
                        edit.state
                            .cursor
                            .set_char_range(Some(CCursorRange::one(end)));
                        edit.state.store(ui.ctx(), edit_id);
                        self.first_frame = false;
                    }
                    if edit.response.changed() {
                        self.query_changed(index, symbol_index);
                    }
                    ui.add_space(4.0);
                    match mode {
                        Mode::Line => {
                            let label = match (line, lines) {
                                (Some((number, _)), _) => {
                                    texts.quick_open_go_to_line(&number.to_string())
                                }
                                (None, Some(lines)) => texts.go_to_line_hint(&lines.to_string()),
                                (None, None) => texts.go_to_line_no_document().to_owned(),
                            };
                            if list_row(ui, &label, &[], go_to_line, None).clicked() {
                                action = line.map(PaletteAction::GoToLine);
                            }
                        }
                        Mode::Symbols => {
                            let message = match &symbols {
                                SymbolList::Pending => Some(texts.symbols_pending()),
                                SymbolList::Unsupported => Some(texts.outline_unsupported()),
                                SymbolList::TooLarge => Some(texts.outline_too_large()),
                                SymbolList::Ready { .. } if symbol_list.is_empty() => {
                                    Some(texts.outline_none())
                                }
                                SymbolList::Ready { .. } if symbol_matches.is_empty() => {
                                    Some(texts.symbols_no_results())
                                }
                                SymbolList::Ready { .. } => None,
                            };
                            if let Some(message) = message {
                                ui.weak(message);
                                return;
                            }
                            egui::ScrollArea::vertical()
                                .max_height(ROW_HEIGHT * VISIBLE_ROWS)
                                .auto_shrink([false, true])
                                .show_rows(ui, ROW_HEIGHT, symbol_matches.len(), |ui, range| {
                                    ui.spacing_mut().item_spacing.y = 0.0;
                                    for i in range {
                                        let (index, positions) = &symbol_matches[i];
                                        let symbol = &symbol_list[*index];
                                        let selected = i == self.selected;
                                        let line = texts.symbol_line(&symbol.line.to_string());
                                        let response = symbol_row(
                                            ui,
                                            symbol.kind,
                                            &symbol.name,
                                            symbol.container.as_deref(),
                                            positions,
                                            &line,
                                            selected,
                                        );
                                        if selected {
                                            response.scroll_to_me(None);
                                        }
                                        if response.clicked() {
                                            action = Some(PaletteAction::GoTo(
                                                symbol.name_range.clone(),
                                            ));
                                        }
                                    }
                                });
                        }
                        Mode::ProjectSymbols => {
                            let name = self.text[1..].trim();
                            let message = match (root, symbols_indexed) {
                                (None, _) => Some(texts.project_symbols_no_project().to_owned()),
                                (Some(_), None) => {
                                    Some(texts.project_symbols_indexing().to_owned())
                                }
                                (Some(_), Some(count)) if name.is_empty() => {
                                    Some(texts.project_symbols_hint(&count.to_string()))
                                }
                                (Some(_), Some(_)) => (self.symbols_generation == self.generation
                                    && self.symbols_found.is_empty())
                                .then(|| texts.symbols_no_results().to_owned()),
                            };
                            if let Some(message) = message {
                                ui.weak(message);
                                return;
                            }
                            let Some(root) = root else { return };
                            let shown = self.shown_symbols();
                            egui::ScrollArea::vertical()
                                .max_height(ROW_HEIGHT * VISIBLE_ROWS)
                                .auto_shrink([false, true])
                                .show_rows(ui, ROW_HEIGHT, shown.len(), |ui, range| {
                                    ui.spacing_mut().item_spacing.y = 0.0;
                                    for i in range {
                                        let found = &shown[i];
                                        let selected = i == self.selected;
                                        let file = found
                                            .relative
                                            .rsplit('\\')
                                            .next()
                                            .unwrap_or(&found.relative);
                                        let place = format!("{file}:{}", found.line + 1);
                                        let response = symbol_row(
                                            ui,
                                            found.kind,
                                            &found.name,
                                            found.container.as_deref(),
                                            &found.positions,
                                            &place,
                                            selected,
                                        )
                                        .on_hover_text(&found.relative);
                                        if selected {
                                            response.scroll_to_me(None);
                                        }
                                        if response.clicked() {
                                            action = Some(go_to_symbol(root, found));
                                        }
                                    }
                                });
                        }
                        Mode::Commands => {
                            if matches.is_empty() {
                                ui.weak(texts.commands_no_results());
                                return;
                            }
                            egui::ScrollArea::vertical()
                                .max_height(ROW_HEIGHT * VISIBLE_ROWS)
                                .auto_shrink([false, true])
                                .show_rows(ui, ROW_HEIGHT, matches.len(), |ui, range| {
                                    ui.spacing_mut().item_spacing.y = 0.0;
                                    for i in range {
                                        let (index, positions) = &matches[i];
                                        let command = &commands[*index];
                                        let selected = i == self.selected;
                                        let response =
                                            command_row(ui, command, positions, selected);
                                        if selected {
                                            response.scroll_to_me(None);
                                        }
                                        if response.clicked() {
                                            action = Some(PaletteAction::Run(command.command));
                                        }
                                    }
                                });
                        }
                        Mode::Files => {
                            if !file.is_empty() && root.is_some() && indexed.is_none() {
                                ui.weak(texts.quick_open_indexing());
                                return;
                            }
                            if items.is_empty() {
                                if !file.is_empty() && self.found_generation == self.generation {
                                    ui.weak(texts.quick_open_no_results());
                                }
                                return;
                            }
                            egui::ScrollArea::vertical()
                                .max_height(ROW_HEIGHT * VISIBLE_ROWS)
                                .auto_shrink([false, true])
                                .show_rows(ui, ROW_HEIGHT, items.len(), |ui, range| {
                                    ui.spacing_mut().item_spacing.y = 0.0;
                                    for i in range {
                                        let item = &items[i];
                                        let badge = registry.for_path(&item.path).map(|l| &l.badge);
                                        let selected = i == self.selected;
                                        let response = list_row(
                                            ui,
                                            &item.shown,
                                            &item.positions,
                                            selected,
                                            badge,
                                        );
                                        if selected {
                                            response.scroll_to_me(None);
                                        }
                                        if response.clicked() {
                                            action = Some(PaletteAction::Open {
                                                path: item.path.clone(),
                                                line,
                                            });
                                        }
                                    }
                                });
                            if let (Some(count), true) = (indexed, root.is_some()) {
                                ui.add_space(2.0);
                                ui.weak(match symbols_indexed {
                                    Some(symbols) => texts.quick_open_count_symbols(
                                        &count.to_string(),
                                        &symbols.to_string(),
                                    ),
                                    None => texts.quick_open_count(&count.to_string()),
                                });
                            }
                        }
                    }
                });
            });
        if action.is_none() && area.response.clicked_elsewhere() {
            action = Some(PaletteAction::Close);
        }
        action
    }
}

/// What the palette is doing, from the first character typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Files,
    /// `>`: run a command.
    Commands,
    /// `@`: go to a symbol of the active document.
    Symbols,
    /// `#`: go to a symbol of the project.
    ProjectSymbols,
    /// `:`: go to a line of the active document.
    Line,
}

impl Mode {
    fn of(text: &str) -> Self {
        if text.starts_with('>') {
            Self::Commands
        } else if text.starts_with('@') {
            Self::Symbols
        } else if text.starts_with('#') {
            Self::ProjectSymbols
        } else if text.trim() == ":"
            || (text.trim_start().starts_with(':') && parse_query(text).0.is_empty())
        {
            Self::Line
        } else {
            Self::Files
        }
    }
}

/// A command as the palette lists it.
#[derive(Debug, Clone)]
pub struct PaletteCommand {
    pub command: Command,
    /// "File: Save".
    pub label: String,
    /// Shown when hovering over it, to know how to refer to it.
    pub id: &'static str,
    pub shortcut: Option<String>,
}

/// The commands matching `query` (all of them, in order, when it is empty), with the matched
/// characters of each label.
fn command_matches(commands: &[PaletteCommand], query: &str) -> Vec<(usize, Vec<usize>)> {
    if query.is_empty() {
        return (0..commands.len()).map(|i| (i, Vec::new())).collect();
    }
    let labels: Vec<String> = commands.iter().map(|c| c.label.clone()).collect();
    fuzzy::search(&labels, &Query::new(query), LIMIT)
        .into_iter()
        .map(|m| (m.index, m.positions))
        .collect()
}

/// The symbols matching `query` (all of them, in document order, when it is empty), with the
/// matched characters of each name.
fn symbol_matches(names: &[String], query: &str) -> Vec<(usize, Vec<usize>)> {
    if query.is_empty() {
        return (0..names.len()).map(|i| (i, Vec::new())).collect();
    }
    fuzzy::search(names, &Query::new(query), LIMIT)
        .into_iter()
        .map(|m| (m.index, m.positions))
        .collect()
}

/// Opens the file of a project symbol with the caret at its name.
fn go_to_symbol(root: &Path, found: &FoundSymbol) -> PaletteAction {
    PaletteAction::GoToSymbol {
        path: root.join(&found.relative),
        position: (found.line as usize + 1, found.column as usize + 1),
    }
}

/// One symbol: its kind, its name with the matched characters marked, the symbol it is in and,
/// on the right, where it is (`place`).
fn symbol_row(
    ui: &mut Ui,
    kind: SymbolKind,
    name: &str,
    container: Option<&str>,
    positions: &[usize],
    place: &str,
    selected: bool,
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let visuals = ui.visuals().clone();
    let painter = ui.painter();
    let (text, weak, accent) = row_colors(&visuals, selected);
    if selected {
        painter.rect_filled(rect, 3.0, visuals.selection.bg_fill);
    } else if response.hovered() {
        painter.rect_filled(rect, 3.0, visuals.widgets.hovered.weak_bg_fill);
    }
    let font = TextStyle::Body.resolve(ui.style());
    let center_y = rect.center().y;
    let mut x = rect.left() + 8.0;
    let badge_font = FontId::proportional((font.size * 0.7).round());
    x += tabs::paint_badge(painter, &kind_badge(kind), badge_font, pos2(x, center_y)) + 6.0;
    let line_galley = painter.layout_no_wrap(
        place.to_owned(),
        FontId::proportional(font.size * 0.85),
        weak,
    );
    let right = rect.right() - 8.0 - line_galley.size().x;
    painter.galley(
        pos2(right, center_y - line_galley.size().y / 2.0),
        line_galley,
        weak,
    );
    let chars: Vec<char> = name.chars().collect();
    let name = painter.layout_job(highlighted(
        &chars,
        0..chars.len(),
        positions,
        text,
        accent,
        font.size,
    ));
    let clip = painter.with_clip_rect(Rect::from_min_max(
        pos2(x, rect.top()),
        pos2(right - 8.0, rect.bottom()),
    ));
    let name_width = name.size().x;
    clip.galley(pos2(x, center_y - name.size().y / 2.0), name, text);
    if let Some(container) = container {
        let container = painter.layout_no_wrap(
            container.to_owned(),
            FontId::proportional(font.size * 0.85),
            weak,
        );
        clip.galley(
            pos2(x + name_width + 10.0, center_y - container.size().y / 2.0),
            container,
            weak,
        );
    }
    response
}

/// One command: its label with the matched characters marked, and its shortcut on the right.
fn command_row(
    ui: &mut Ui,
    command: &PaletteCommand,
    positions: &[usize],
    selected: bool,
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
    let response = response.on_hover_text(command.id);
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let visuals = ui.visuals().clone();
    let painter = ui.painter();
    let (text, weak, accent) = row_colors(&visuals, selected);
    if selected {
        painter.rect_filled(rect, 3.0, visuals.selection.bg_fill);
    } else if response.hovered() {
        painter.rect_filled(rect, 3.0, visuals.widgets.hovered.weak_bg_fill);
    }
    let font = TextStyle::Body.resolve(ui.style());
    let chars: Vec<char> = command.label.chars().collect();
    let label = painter.layout_job(highlighted(
        &chars,
        0..chars.len(),
        positions,
        text,
        accent,
        font.size,
    ));
    let center_y = rect.center().y;
    let mut right = rect.right() - 8.0;
    if let Some(shortcut) = &command.shortcut {
        let keys = painter.layout_no_wrap(
            shortcut.clone(),
            FontId::proportional(font.size * 0.9),
            weak,
        );
        right -= keys.size().x;
        painter.galley(pos2(right, center_y - keys.size().y / 2.0), keys, weak);
        right -= 12.0;
    }
    let left = rect.left() + 8.0;
    painter
        .with_clip_rect(Rect::from_min_max(
            pos2(left, rect.top()),
            pos2(right, rect.bottom()),
        ))
        .galley(pos2(left, center_y - label.size().y / 2.0), label, text);
    response
}

/// Text, weak text and accent colours of a row.
fn row_colors(visuals: &egui::Visuals, selected: bool) -> (Color32, Color32, Color32) {
    if selected {
        let strong = visuals.selection.stroke.color;
        (strong, strong.gamma_multiply(0.8), strong)
    } else {
        (
            visuals.text_color(),
            visuals.weak_text_color(),
            visuals.hyperlink_color,
        )
    }
}

/// `chars[range]` with the characters at `positions` in the accent colour, underlined.
fn highlighted(
    chars: &[char],
    range: std::ops::Range<usize>,
    positions: &[usize],
    color: Color32,
    accent: Color32,
    size: f32,
) -> LayoutJob {
    let mut job = LayoutJob::default();
    for i in range {
        let matched = positions.contains(&i);
        job.append(
            &chars[i].to_string(),
            0.0,
            TextFormat {
                font_id: FontId::proportional(size),
                color: if matched { accent } else { color },
                underline: if matched {
                    egui::Stroke::new(1.0, accent)
                } else {
                    egui::Stroke::NONE
                },
                ..TextFormat::default()
            },
        );
    }
    job
}

/// One entry: badge, file name and the folder it is in, with the matched characters in the
/// accent colour.
fn list_row(
    ui: &mut Ui,
    shown: &str,
    positions: &[usize],
    selected: bool,
    badge: Option<&bracket_syntax::Badge>,
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let visuals = ui.visuals().clone();
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, 3.0, visuals.selection.bg_fill);
    } else if response.hovered() {
        painter.rect_filled(rect, 3.0, visuals.widgets.hovered.weak_bg_fill);
    }
    let font = TextStyle::Body.resolve(ui.style());
    let (text, weak, accent) = row_colors(&visuals, selected);
    let mut x = rect.left() + 8.0;
    let center_y = rect.center().y;
    if let Some(badge) = badge {
        let badge_font = FontId::proportional((font.size * 0.75).round());
        x += tabs::paint_badge(painter, badge, badge_font, pos2(x, center_y)) + 6.0;
    }
    let chars: Vec<char> = shown.chars().collect();
    let name_start = chars
        .iter()
        .rposition(|&c| c == '\\' || c == '/')
        .map_or(0, |i| i + 1);
    let job = |range: std::ops::Range<usize>, color: Color32, size: f32| {
        highlighted(&chars, range, positions, color, accent, size)
    };
    let name = painter.layout_job(job(name_start..chars.len(), text, font.size));
    let folder = painter.layout_job(job(0..name_start.saturating_sub(1), weak, font.size * 0.9));
    let clip = painter.with_clip_rect(Rect::from_min_max(pos2(x, rect.top()), rect.max));
    let name_width = name.size().x;
    clip.galley(pos2(x, center_y - name.size().y / 2.0), name, text);
    clip.galley(
        pos2(x + name_width + 10.0, center_y - folder.size().y / 2.0),
        folder,
        weak,
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{Duration, Instant};

    #[test]
    fn line_and_column_are_split_off() {
        assert_eq!(parse_query("Program.cs"), ("Program.cs", None));
        assert_eq!(
            parse_query("Program.cs:42"),
            ("Program.cs", Some((42, None)))
        );
        assert_eq!(parse_query("prog:42:7"), ("prog", Some((42, Some(7)))));
        assert_eq!(parse_query(":120"), ("", Some((120, None))));
        assert_eq!(parse_query(" user :3 "), ("user", Some((3, None))));
        assert_eq!(parse_query("a:0"), ("a:0", None), "lines start at 1");
        assert_eq!(parse_query(r"C:\src"), (r"C:\src", None));
        assert_eq!(parse_query("prog:"), ("prog:", None));
    }

    fn wait_for(worker: &IndexWorker, mut found: impl FnMut(&IndexEvent) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if worker.events().iter().any(&mut found) {
                return;
            }
            assert!(Instant::now() < deadline, "the index never answered");
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn the_index_answers_queries_and_follows_changes() {
        let root = std::env::temp_dir().join(format!("bracket-quick-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join(r"src\Program.cs"), "x").unwrap();
        fs::write(root.join("README.md"), "x").unwrap();

        let (files_tx, files_rx) = mpsc::channel();
        let worker = IndexWorker::start(
            root.clone(),
            WalkOptions::default(),
            || {},
            move |c| {
                let _ = files_tx.send(c);
            },
        );
        wait_for(&worker, |e| matches!(e, IndexEvent::Ready(2)));
        worker.query("prog".into(), 1);
        wait_for(&worker, |e| match e {
            IndexEvent::Results { generation, found } => {
                *generation == 1 && found.len() == 1 && found[0].relative == r"src\Program.cs"
            }
            _ => false,
        });

        fs::write(root.join(r"src\Startup.cs"), "x").unwrap();
        worker.changes(ProjectChanges {
            paths: [root.join(r"src\Startup.cs")].into(),
            ..ProjectChanges::default()
        });
        worker.query("startup".into(), 2);
        wait_for(&worker, |e| match e {
            IndexEvent::Results { generation, found } => *generation == 2 && found.len() == 1,
            _ => false,
        });

        // The symbol index gets the list, then each change with the list as it is after it.
        let built = files_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(built.full && built.files.len() == 2);
        let added = files_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(!added.full && added.files.len() == 3);
        assert_eq!(added.changed, [root.join(r"src\Startup.cs")]);
        worker.changes(ProjectChanges {
            modified: [root.join(r"src\Program.cs")].into(),
            ..ProjectChanges::default()
        });
        let modified = files_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(modified.changed, [root.join(r"src\Program.cs")]);
        assert!(
            std::sync::Arc::ptr_eq(&modified.files, &added.files),
            "the list is not copied again"
        );
        drop(worker);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_first_character_picks_the_mode() {
        assert_eq!(Mode::of("prog"), Mode::Files);
        assert_eq!(Mode::of("prog:12"), Mode::Files);
        assert_eq!(Mode::of(">save"), Mode::Commands);
        assert_eq!(Mode::of(":"), Mode::Line);
        assert_eq!(Mode::of(":42"), Mode::Line);
        assert_eq!(Mode::of(":42:3"), Mode::Line);
    }

    #[test]
    fn commands_are_found_by_their_label() {
        let commands: Vec<PaletteCommand> = [
            (Command::Save, "File: Save"),
            (Command::GoToLine, "Go: Go to line"),
        ]
        .into_iter()
        .map(|(command, label)| PaletteCommand {
            command,
            label: label.to_owned(),
            id: command.id(),
            shortcut: None,
        })
        .collect();
        assert_eq!(
            command_matches(&commands, "").len(),
            2,
            "all, with no query"
        );
        let found = command_matches(&commands, "gtl");
        assert_eq!(found.len(), 1);
        assert_eq!(commands[found[0].0].command, Command::GoToLine);
    }

    #[test]
    fn symbols_are_found_by_name_or_all_listed() {
        let names = vec![
            "Invoice".to_owned(),
            "Compute".to_owned(),
            "Total".to_owned(),
        ];
        assert_eq!(
            symbol_matches(&names, "").len(),
            3,
            "all, in order, with no query"
        );
        let found = symbol_matches(&names, "cmp");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, 1);
        assert_eq!(Mode::of("@total"), Mode::Symbols);
        assert!(QuickOpen::new(Vec::new(), "@").wants_symbols());
        assert!(!QuickOpen::new(Vec::new(), "").wants_symbols());
    }

    #[test]
    fn project_symbols_follow_the_newest_query() {
        assert_eq!(Mode::of("#invoice"), Mode::ProjectSymbols);
        let mut palette = QuickOpen::new(Vec::new(), "#");
        assert!(palette.shown_symbols().is_empty());
        palette.text = "#inv".to_owned();
        palette.query_changed(None, None);
        let found = |name: &str| FoundSymbol {
            name: name.to_owned(),
            kind: SymbolKind::Class,
            container: None,
            relative: format!(r"src\{name}.cs"),
            line: 4,
            column: 6,
            positions: Vec::new(),
        };
        palette.symbol_results(palette.generation - 1, vec![found("Old")]);
        assert!(
            palette.shown_symbols().is_empty(),
            "an older query is dropped"
        );
        palette.symbol_results(palette.generation, vec![found("Invoice")]);
        assert_eq!(palette.shown_symbols()[0].name, "Invoice");
        assert_eq!(
            go_to_symbol(Path::new(r"C:\p"), &palette.shown_symbols()[0]),
            PaletteAction::GoToSymbol {
                path: PathBuf::from(r"C:\p\src\Invoice.cs"),
                position: (5, 7),
            }
        );
    }

    #[test]
    fn older_results_are_dropped() {
        let mut palette = QuickOpen::new(Vec::new(), "");
        palette.text = "pro".into();
        palette.query_changed(None, None);
        palette.text = "prog".into();
        palette.query_changed(None, None);
        let found = vec![Found {
            relative: "old".into(),
            positions: vec![],
        }];
        palette.results(1, found);
        assert!(
            palette.found.is_empty(),
            "results for \"pro\" arrive too late"
        );
        palette.results(
            2,
            vec![Found {
                relative: r"src\Program.cs".into(),
                positions: vec![4, 5, 6, 7],
            }],
        );
        assert_eq!(palette.found.len(), 1);
    }
}
