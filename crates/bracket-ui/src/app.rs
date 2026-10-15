//! The application shell: one document, file loading and saving on worker threads, window
//! title, status bar and the unsaved-changes dialog.

use std::io;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use bracket_core::document::{self, Document, LineEnding};
use bracket_core::movement::{self, Motion};
use eframe::egui::{self, Id, ViewportCommand};

use crate::editor_view::EditorView;
use crate::i18n::{self, Language, Texts};
use crate::measure::Probe;

/// What to do once the user has dealt with unsaved changes.
#[derive(Debug, Clone)]
enum PendingAction {
    Close,
    Open(PathBuf),
}

pub struct BracketApp {
    texts: &'static Texts,
    doc: Document,
    view: EditorView,
    loading: Option<Receiver<io::Result<Document>>>,
    saving: Option<Receiver<(u64, io::Result<()>)>>,
    status: Option<String>,
    pending: Option<PendingAction>,
    allow_close: bool,
    title: String,
    probe: Option<Probe>,
    frames_ready: u32,
}

impl BracketApp {
    pub fn new(ctx: &egui::Context, path: Option<PathBuf>, probe: Option<Probe>) -> Self {
        let mut app = Self {
            texts: i18n::texts(Language::from_env()),
            doc: Document::new_empty(),
            view: EditorView::new(),
            loading: None,
            saving: None,
            status: None,
            pending: None,
            allow_close: false,
            title: String::new(),
            probe,
            frames_ready: 0,
        };
        ctx.memory_mut(|m| m.request_focus(app.view.id()));
        if let Some(path) = path {
            app.start_loading(ctx, path);
        }
        app
    }

    /// Loads a file on a worker thread; the UI keeps running and shows a loading message.
    fn start_loading(&mut self, ctx: &egui::Context, path: PathBuf) {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result =
                document::load_text(&path).map(|loaded| Document::from_loaded(path, loaded));
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
                self.status = Some((self.texts.open_failed)(&err.to_string()));
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
            self.status = Some(self.texts.save_read_only.to_owned());
            return;
        }
        let Some(request) = self.doc.save_request() else {
            self.status = Some(self.texts.save_needs_path.to_owned());
            return;
        };
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let _ = tx.send((request.version, request.write()));
            ctx.request_repaint();
        });
        self.saving = Some(rx);
    }

    fn poll_saving(&mut self) {
        let Some(rx) = &self.saving else { return };
        match rx.try_recv() {
            Ok((version, Ok(()))) => {
                self.doc.mark_saved(version);
                self.status = Some(self.texts.saved.to_owned());
                self.saving = None;
            }
            Ok((_, Err(err))) => {
                self.status = Some((self.texts.save_failed)(&err.to_string()));
                self.saving = None;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => self.saving = None,
        }
    }

    /// Saves on the UI thread. Only used from the unsaved-changes dialog, where the next
    /// action must wait for the result anyway.
    fn save_now(&mut self) -> bool {
        match self.doc.save_request() {
            Some(request) => match request.write() {
                Ok(()) => {
                    self.doc.mark_saved(request.version);
                    true
                }
                Err(err) => {
                    self.status = Some((self.texts.save_failed)(&err.to_string()));
                    false
                }
            },
            None => {
                self.status = Some(self.texts.save_needs_path.to_owned());
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
            PendingAction::Open(path) => self.start_loading(ctx, path),
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
                self.start_loading(ctx, path);
            }
        }
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let name = self
            .doc
            .path()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.texts.untitled.to_owned());
        let dirty = if self.doc.is_dirty() { " ●" } else { "" };
        let title = format!("{name}{dirty} — Bracket");
        if title != self.title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    fn status_bar(&self, ui: &mut egui::Ui) {
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                let text = self.doc.buffer.text();
                let head = self.doc.buffer.selections().primary().head;
                let line = movement::line_of(text, head) + 1;
                let column = movement::visual_column(text, head, self.doc.buffer.tab_width()) + 1;
                ui.label((self.texts.position)(line, column));
                ui.separator();
                ui.label(match self.doc.line_ending() {
                    LineEnding::CrLf => "CRLF",
                    LineEnding::Lf => "LF",
                    LineEnding::Cr => "CR",
                });
                ui.separator();
                ui.label(if self.doc.has_bom() {
                    "UTF-8 BOM"
                } else {
                    "UTF-8"
                });
                if self.doc.is_read_only() {
                    ui.separator();
                    ui.label(self.texts.read_only);
                }
                ui.separator();
                match &self.status {
                    Some(message) => ui.label(message.as_str()),
                    None if self.doc.path().is_none() => ui.weak(self.texts.drop_hint),
                    None => ui.label(""),
                };
            });
        });
    }

    fn unsaved_dialog(&mut self, ctx: &egui::Context) {
        let Some(action) = self.pending.clone() else {
            return;
        };
        let texts = self.texts;
        let mut choice = None;
        egui::Modal::new(Id::new("unsaved-changes")).show(ctx, |ui| {
            ui.heading(texts.unsaved_title);
            ui.label(texts.unsaved_message);
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button(texts.save).clicked() {
                    choice = Some(true);
                }
                if ui.button(texts.discard).clicked() {
                    choice = Some(false);
                }
                if ui.button(texts.cancel).clicked() {
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
        self.handle_window_events(&ctx);

        self.status_bar(ui);
        egui::CentralPanel::default().show(ui, |ui| {
            if self.loading.is_some() {
                ui.centered_and_justified(|ui| ui.label(self.texts.loading));
            } else {
                let output = self.view.show(ui, &mut self.doc);
                if output.save_requested {
                    self.start_saving(&ctx);
                }
            }
        });
        self.unsaved_dialog(&ctx);
        self.update_title(&ctx);

        self.run_probe(&ctx);
    }
}
