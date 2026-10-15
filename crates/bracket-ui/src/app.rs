//! The application shell: one document, file loading and saving on worker threads, window
//! title, status bar and the unsaved-changes dialog.

use std::collections::HashSet;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;

use bracket_core::document::{self, Document, LineEnding, SaveError};
use bracket_core::encoding::{self, TextEncoding};
use bracket_core::movement::{self, Motion};
use bracket_session::{Config, ConfigWarning, Language, LoadedConfig, config};
use eframe::egui::{self, Id, Key, Modifiers, ViewportCommand};

use crate::Launch;
use crate::editor_view::EditorView;
use crate::fonts::{self, Fallback, FontFile, FontSource};
use crate::i18n::{self, LANGUAGES, Texts};
use crate::measure::Probe;

/// Result of a background save: the version and encoding written, and whether it worked.
type SaveOutcome = (u64, TextEncoding, Result<(), SaveError>);

/// Something picked in a status bar menu.
#[derive(Debug, Clone, Copy)]
enum StatusAction {
    Language(Language),
    ConvertLineEndings(LineEnding),
    ReopenWith(TextEncoding),
    SaveWith(TextEncoding),
}

/// What to do once the user has dealt with unsaved changes.
#[derive(Debug, Clone)]
enum PendingAction {
    Close,
    Open(PathBuf),
}

pub struct BracketApp {
    texts: &'static Texts,
    config: Config,
    doc: Document,
    view: EditorView,
    loading: Option<Receiver<io::Result<Document>>>,
    saving: Option<Receiver<SaveOutcome>>,
    /// Latest result of a user action (open, save...).
    status: Option<String>,
    /// Configuration warnings, shown (translated) while there is no other status message.
    config_warnings: Vec<ConfigWarning>,
    config_path: PathBuf,
    config_saving: Option<Receiver<Result<(), String>>>,
    pending: Option<PendingAction>,
    allow_close: bool,
    title: String,
    /// Configured editor font that is not installed, and the font used instead.
    missing_font: Option<(String, String)>,
    font_source: Option<Arc<dyn FontSource>>,
    fallbacks_requested: HashSet<Fallback>,
    fallback_tx: Sender<FontFile>,
    fallback_rx: Receiver<FontFile>,
    probe: Option<Probe>,
    frames_ready: u32,
}

impl BracketApp {
    pub fn new(ctx: &egui::Context, launch: Launch, mut probe: Option<Probe>) -> Self {
        if let Some(Probe::Startup(startup)) = &mut probe {
            startup.mark_app_created();
        }
        let Launch {
            file,
            settings,
            config_path,
            fonts,
        } = launch;
        let LoadedConfig { config, warnings } = settings;

        // The font thread started at the beginning of main() and has normally finished by now.
        let setup = fonts.join().ok();
        let missing_font = setup.as_ref().and_then(|s| {
            let family = s.startup.missing_family.clone()?;
            let used = s.startup.monospace.as_ref().map(|f| f.family.clone())?;
            Some((family, used))
        });
        let (font_source, startup) = match setup {
            Some(s) => (Some(s.source), Some(s.startup)),
            None => (None, None),
        };
        let defs = startup
            .and_then(fonts::definitions)
            .expect("no usable font found: Consolas and Segoe UI ship with Windows");
        ctx.set_fonts(defs);
        let (fallback_tx, fallback_rx) = mpsc::channel();

        let texts = i18n::texts(config.language);
        let mut doc = Document::new_empty(config.line_ending, config.encoding);
        doc.buffer.set_tab_width(config.tab_width);
        let mut app = Self {
            texts,
            doc,
            view: EditorView::with_font_size(config.font_size),
            config,
            loading: None,
            saving: None,
            status: None,
            config_warnings: warnings,
            config_path,
            config_saving: None,
            missing_font,
            font_source,
            fallbacks_requested: HashSet::new(),
            fallback_tx,
            fallback_rx,
            pending: None,
            allow_close: false,
            title: String::new(),
            probe,
            frames_ready: 0,
        };
        ctx.memory_mut(|m| m.request_focus(app.view.id()));
        if let Some(path) = file {
            app.start_loading(ctx, path, None);
        }
        app
    }

    /// Loads a fallback font for `c` on a worker thread, once per kind of fallback.
    fn request_fallback(&mut self, ctx: &egui::Context, c: char) {
        let kind = Fallback::for_char(c);
        // Whatever happens next, the view stops looking for this kind of character.
        self.view.fallback_requested(kind);
        let Some(source) = self.font_source.clone() else {
            return;
        };
        if !self.fallbacks_requested.insert(kind) {
            return;
        }
        let tx = self.fallback_tx.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            if let Some(file) = fonts::load_fallback(source.as_ref(), kind) {
                let _ = tx.send(file);
                ctx.request_repaint();
            }
        });
    }

    fn poll_fallbacks(&mut self, ctx: &egui::Context) {
        while let Ok(file) = self.fallback_rx.try_recv() {
            ctx.add_font(fonts::fallback_insert(file));
        }
    }

    /// Loads a file on a worker thread; the UI keeps running and shows a loading message.
    /// `encoding` forces how the bytes are decoded ("reopen with encoding"); `None` detects it.
    fn start_loading(
        &mut self,
        ctx: &egui::Context,
        path: PathBuf,
        encoding: Option<TextEncoding>,
    ) {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let (line_ending, tab_width) = (self.config.line_ending, self.config.tab_width);
        thread::spawn(move || {
            let loaded = match encoding {
                Some(encoding) => document::load_text_as(&path, encoding),
                None => document::load_text(&path),
            };
            let result = loaded.map(|loaded| {
                let mut doc = Document::from_loaded(path, loaded, line_ending);
                doc.buffer.set_tab_width(tab_width);
                doc
            });
            // The receiver may be gone if the window closed; nothing to do then.
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        self.loading = Some(rx);
    }

    fn poll_loading(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.loading else { return };
        match rx.try_recv() {
            Ok(Ok(doc)) => {
                self.doc = doc;
                self.view.reset();
                self.status = None;
                self.loading = None;
                ctx.memory_mut(|m| m.request_focus(self.view.id()));
            }
            Ok(Err(err)) => {
                self.status = Some(self.texts.open_failed(&err.to_string()));
                self.loading = None;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => self.loading = None,
        }
    }

    /// Saves on a worker thread. The text is a cheap snapshot, so editing can continue.
    fn start_saving(&mut self, ctx: &egui::Context) {
        if self.saving.is_some() {
            return;
        }
        if self.doc.is_read_only() {
            self.status = Some(self.texts.save_read_only().to_owned());
            return;
        }
        let Some(request) = self.doc.save_request() else {
            self.status = Some(self.texts.save_needs_path().to_owned());
            return;
        };
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result = request.write();
            let _ = tx.send((request.version, request.encoding, result));
            ctx.request_repaint();
        });
        self.saving = Some(rx);
    }

    fn poll_saving(&mut self) {
        let Some(rx) = &self.saving else { return };
        match rx.try_recv() {
            Ok((version, encoding, Ok(()))) => {
                self.doc.mark_saved(version, encoding);
                self.status = Some(self.texts.saved().to_owned());
                self.saving = None;
            }
            Ok((_, _, Err(err))) => {
                self.status = Some(self.save_error_text(&err));
                self.saving = None;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => self.saving = None,
        }
    }

    fn save_error_text(&self, err: &SaveError) -> String {
        match err {
            SaveError::Encode(err) => {
                let character = err.character.map(String::from).unwrap_or_default();
                self.texts
                    .save_encode_failed(&err.encoding.label(), &character)
            }
            SaveError::Io(err) => self.texts.save_failed(&err.to_string()),
        }
    }

    /// Saves on the UI thread. Only used from the unsaved-changes dialog, where the next
    /// action must wait for the result anyway.
    fn save_now(&mut self) -> bool {
        match self.doc.save_request() {
            Some(request) => match request.write() {
                Ok(()) => {
                    self.doc.mark_saved(request.version, request.encoding);
                    true
                }
                Err(err) => {
                    self.status = Some(self.save_error_text(&err));
                    false
                }
            },
            None => {
                self.status = Some(self.texts.save_needs_path().to_owned());
                false
            }
        }
    }

    fn run_pending(&mut self, ctx: &egui::Context, action: PendingAction) {
        match action {
            PendingAction::Close => {
                self.allow_close = true;
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
            PendingAction::Open(path) => self.start_loading(ctx, path, None),
        }
    }

    fn handle_window_events(&mut self, ctx: &egui::Context) {
        let close_requested = ctx.input(|i| i.viewport().close_requested());
        if close_requested && self.doc.is_dirty() && !self.allow_close {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.pending = Some(PendingAction::Close);
        }

        let dropped = ctx.input(|i| i.raw.dropped_files.first().map(|f| f.path().to_path_buf()));
        if let Some(path) = dropped {
            if self.doc.is_dirty() {
                self.pending = Some(PendingAction::Open(path));
            } else {
                self.start_loading(ctx, path, None);
            }
        }
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let name = self
            .doc
            .path()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.texts.untitled().to_owned());
        let dirty = if self.doc.is_dirty() { " ●" } else { "" };
        let title = format!("{name}{dirty} — Bracket");
        if title != self.title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    /// Draws the status bar. Returns what was picked in one of its menus, if anything.
    fn status_bar(&self, ui: &mut egui::Ui) -> Option<StatusAction> {
        let mut picked = None;
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                let text = self.doc.buffer.text();
                let head = self.doc.buffer.selections().primary().head;
                let line = movement::line_of(text, head) + 1;
                let column = movement::visual_column(text, head, self.doc.buffer.tab_width()) + 1;
                ui.label(self.texts.position(line, column));
                ui.separator();
                let current_line_ending = self.doc.line_ending();
                ui.menu_button(current_line_ending.label(), |ui| {
                    for line_ending in LineEnding::ALL {
                        let label = self.texts.convert_line_endings(line_ending.label());
                        if ui
                            .selectable_label(line_ending == current_line_ending, label)
                            .clicked()
                        {
                            picked = Some(StatusAction::ConvertLineEndings(line_ending));
                            ui.close();
                        }
                    }
                })
                .response
                .on_hover_text(self.texts.line_ending_tooltip());
                ui.separator();
                let current_encoding = self.doc.encoding();
                ui.menu_button(current_encoding.label(), |ui| {
                    ui.menu_button(self.texts.reopen_with_encoding(), |ui| {
                        for encoding in encoding::selectable() {
                            if ui.button(encoding.label()).clicked() {
                                picked = Some(StatusAction::ReopenWith(encoding));
                                ui.close();
                            }
                        }
                    });
                    ui.menu_button(self.texts.save_with_encoding(), |ui| {
                        for encoding in encoding::selectable() {
                            if ui
                                .selectable_label(encoding == current_encoding, encoding.label())
                                .clicked()
                            {
                                picked = Some(StatusAction::SaveWith(encoding));
                                ui.close();
                            }
                        }
                    });
                })
                .response
                .on_hover_text(self.texts.encoding_tooltip());
                if self.doc.is_read_only() {
                    ui.separator();
                    ui.label(self.texts.read_only());
                }
                ui.separator();
                let notice = warnings_summary(self.texts, &self.config_warnings).or_else(|| {
                    let (family, used) = self.missing_font.as_ref()?;
                    Some(self.texts.font_missing(family, used))
                });
                match self.status.as_ref().or(notice.as_ref()) {
                    Some(message) => ui.label(message.as_str()),
                    None if self.doc.path().is_none() => ui.weak(self.texts.drop_hint()),
                    None => ui.label(""),
                };
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.menu_button(self.texts.language_name(), |ui| {
                        for language in LANGUAGES {
                            let name = i18n::texts(language).language_name();
                            if ui
                                .selectable_label(language == self.config.language, name)
                                .clicked()
                            {
                                picked = Some(StatusAction::Language(language));
                                ui.close();
                            }
                        }
                    })
                    .response
                    .on_hover_text(self.texts.language_tooltip());
                });
            });
        });
        picked
    }

    fn run_status_action(&mut self, ctx: &egui::Context, action: StatusAction) {
        match action {
            StatusAction::Language(language) => self.set_language(ctx, language),
            StatusAction::ConvertLineEndings(line_ending) => {
                if !self.doc.is_read_only() {
                    self.doc.convert_line_endings(line_ending);
                }
            }
            StatusAction::SaveWith(encoding) => {
                if !self.doc.is_read_only() {
                    self.doc.set_encoding(encoding);
                }
            }
            StatusAction::ReopenWith(encoding) => {
                let Some(path) = self.doc.path().map(PathBuf::from) else {
                    return;
                };
                if self.doc.is_dirty() {
                    self.status = Some(self.texts.reopen_needs_clean().to_owned());
                } else {
                    self.start_loading(ctx, path, Some(encoding));
                }
            }
        }
    }

    /// Switches the interface language at once and saves it in the configuration.
    fn set_language(&mut self, ctx: &egui::Context, language: Language) {
        if language == self.config.language {
            return;
        }
        self.config.language = language;
        self.texts = i18n::texts(language);
        self.start_config_save(ctx);
    }

    /// Writes the configuration on a worker thread (comments in the file are kept).
    fn start_config_save(&mut self, ctx: &egui::Context) {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let (config, path) = (self.config.clone(), self.config_path.clone());
        thread::spawn(move || {
            let _ = tx.send(config::save(&config, &path).map_err(|e| e.to_string()));
            ctx.request_repaint();
        });
        self.config_saving = Some(rx);
    }

    fn poll_config_saving(&mut self) {
        let Some(rx) = &self.config_saving else {
            return;
        };
        match rx.try_recv() {
            Ok(result) => {
                if let Err(error) = result {
                    self.status = Some(self.texts.config_save_failed(&error));
                }
                self.config_saving = None;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => self.config_saving = None,
        }
    }

    fn unsaved_dialog(&mut self, ctx: &egui::Context) {
        let Some(action) = self.pending.clone() else {
            return;
        };
        let texts = self.texts;
        let mut choice = None;
        egui::Modal::new(Id::new("unsaved-changes")).show(ctx, |ui| {
            ui.heading(texts.unsaved_title());
            ui.label(texts.unsaved_message());
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button(texts.save()).clicked() {
                    choice = Some(true);
                }
                if ui.button(texts.discard()).clicked() {
                    choice = Some(false);
                }
                if ui.button(texts.cancel()).clicked() {
                    self.pending = None;
                }
            });
        });
        match choice {
            Some(true) => {
                self.pending = None;
                if self.save_now() {
                    self.run_pending(ctx, action);
                }
            }
            Some(false) => {
                self.pending = None;
                self.run_pending(ctx, action);
            }
            None => {}
        }
    }
}

impl BracketApp {
    /// Drives the measurement probe, if any (see `measure.rs`).
    fn run_probe(&mut self, ctx: &egui::Context) {
        let Some(probe) = &mut self.probe else { return };
        if let Probe::Startup(startup) = probe {
            startup.mark_first_ui();
        }
        ctx.request_repaint();
        // The second frame with the final content means the first one has been presented.
        // Count frames, not passes: egui may run extra layout passes within one frame.
        if self.loading.is_some() || ctx.current_pass_index() != 0 {
            return;
        }
        self.frames_ready += 1;
        if self.frames_ready < 2 {
            return;
        }
        match probe {
            Probe::Startup(startup) => startup.finish(),
            Probe::Scroll(scroll) => {
                // One page further every frame, wrapping at the end: each frame lays out new lines.
                let buffer = &mut self.doc.buffer;
                if buffer.selections().primary().head == buffer.len_chars() {
                    buffer.set_selections(bracket_core::Selections::caret(0));
                } else {
                    buffer.move_selections(Motion::Down(self.view.page_lines()), false);
                }
                self.view.request_scroll_to_caret();
                scroll.frame();
            }
        }
    }
}

impl eframe::App for BracketApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_loading(&ctx);
        self.poll_saving();
        self.poll_config_saving();
        self.poll_fallbacks(&ctx);
        self.handle_window_events(&ctx);

        // Application shortcuts work wherever the keyboard focus is (for example after using a
        // status bar menu); consuming them keeps the editor from seeing them too.
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::S)) {
            self.start_saving(&ctx);
        }
        if let Some(action) = self.status_bar(ui) {
            self.run_status_action(&ctx, action);
            // Give the keyboard back to the editor after using a menu.
            ctx.memory_mut(|m| m.request_focus(self.view.id()));
        }
        egui::CentralPanel::default().show(ui, |ui| {
            if self.loading.is_some() {
                ui.centered_and_justified(|ui| ui.label(self.texts.loading()));
            } else {
                let output = self.view.show(ui, &mut self.doc);
                if let Some(c) = output.missing_glyph {
                    self.request_fallback(&ctx, c);
                }
            }
        });
        self.unsaved_dialog(&ctx);
        self.update_title(&ctx);

        self.run_probe(&ctx);
    }
}

/// The first configuration warning, translated, plus how many more there are.
fn warnings_summary(texts: &Texts, warnings: &[ConfigWarning]) -> Option<String> {
    let first = texts.config_warning(warnings.first()?);
    Some(match warnings.len() - 1 {
        0 => first,
        more => format!("{first} {}", texts.more_warnings(more)),
    })
}
