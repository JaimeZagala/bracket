//! The application shell: open documents as tabs, file loading and saving on worker threads,
//! window title, status bar and the unsaved-changes dialog.

use std::collections::HashSet;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::Instant;

use bracket_core::Selections;
use bracket_core::document::{self, DiskStamp, Document, FileReplacer, LineEnding, SaveError};
use bracket_core::encoding::{self, TextEncoding};
use bracket_core::movement::{self, Motion};
use bracket_session::config::AUTO_THEME;
use bracket_session::{
    Appearance, Config, ConfigWarning, FileWatcher, Language, LoadedConfig, OpenRequest,
    SessionTab, config, requests,
};
use bracket_syntax::ThemeColors;
use bracket_syntax::highlight::{self, MAX_HIGHLIGHT_BYTES};
use bracket_syntax::{LanguageRegistry, PackageWarning};
use eframe::egui::{self, Id, Key, KeyboardShortcut, Modifiers, ViewportCommand};

use crate::dialogs;
use crate::disk::{self, DiskNotice, DiskState, DiskWatch, NoticeAction};
use crate::editor_view::{EditorStyle, EditorView};
use crate::fonts::{self, Fallback, FontSource, LoadedFallback};
use crate::i18n::{self, LANGUAGES, Texts};
use crate::measure::Probe;
use crate::session::{self, SessionKeeper};
use crate::tabs::{self, Loaded, Loading, Tab, TabAction, TabHighlight};
use crate::{Launch, RestoredSession};

/// How the window starts.
pub struct WindowStart {
    /// The window position and size eframe stored last time (see `BracketApp::save`).
    pub stored: Option<String>,
    /// Maximize the window after its first frame.
    pub maximize: bool,
}

/// eframe's storage key for the window position and size.
pub const WINDOW_KEY: &str = "window";

/// Something picked in a status bar menu.
#[derive(Debug, Clone)]
enum StatusAction {
    InterfaceLanguage(Language),
    /// `None` for plain text.
    DocumentLanguage(Option<String>),
    ConvertLineEndings(LineEnding),
    ReopenWith(TextEncoding),
    SaveWith(TextEncoding),
}

/// Something picked in the View menu.
#[derive(Debug, Clone)]
enum ViewAction {
    Appearance(Appearance),
    EditorTheme(String),
}

/// What waits for the user to deal with unsaved changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingAction {
    /// Close the tab with this id.
    CloseTab(u64),
    CloseWindow,
}

/// Menu commands, also bound to keyboard shortcuts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    New,
    Open,
    Save,
    SaveAs,
    CloseTab,
    NextTab,
    PreviousTab,
    Exit,
}

impl Command {
    const SHORTCUTS: [(Self, KeyboardShortcut); 9] = [
        // Shortcuts with Shift go first: Ctrl+S would also match Ctrl+Shift+S.
        (
            Self::SaveAs,
            KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::S),
        ),
        (
            Self::PreviousTab,
            KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::Tab),
        ),
        (
            Self::Save,
            KeyboardShortcut::new(Modifiers::COMMAND, Key::S),
        ),
        (Self::New, KeyboardShortcut::new(Modifiers::COMMAND, Key::N)),
        (
            Self::Open,
            KeyboardShortcut::new(Modifiers::COMMAND, Key::O),
        ),
        (
            Self::CloseTab,
            KeyboardShortcut::new(Modifiers::COMMAND, Key::W),
        ),
        (
            Self::NextTab,
            KeyboardShortcut::new(Modifiers::COMMAND, Key::Tab),
        ),
        (
            Self::PreviousTab,
            KeyboardShortcut::new(Modifiers::COMMAND, Key::PageUp),
        ),
        (
            Self::NextTab,
            KeyboardShortcut::new(Modifiers::COMMAND, Key::PageDown),
        ),
    ];

    /// The first shortcut bound to the command, as shown in menus.
    fn shortcut(self) -> Option<KeyboardShortcut> {
        Self::SHORTCUTS
            .iter()
            .find(|(command, _)| *command == self)
            .map(|(_, shortcut)| *shortcut)
    }
}

pub struct BracketApp {
    texts: &'static Texts,
    config: Config,
    /// Never empty: closing the last tab opens an untitled one.
    tabs: Vec<Tab>,
    active: usize,
    next_tab_id: u64,
    languages: LanguageRegistry,
    /// Scroll the tab bar to the active tab on the next frame.
    reveal_active_tab: bool,
    /// Latest result of a user action (open, save...).
    status: Option<String>,
    /// Configuration warnings, shown (translated) while there is no other status message.
    config_warnings: Vec<ConfigWarning>,
    /// The editor theme in use (the configured one, or the light or dark default).
    editor_theme: String,
    /// Its editor-area colours.
    theme_colors: ThemeColors,
    /// Problems with language packages; grammar problems are added once they are compiled.
    package_warnings: Vec<PackageWarning>,
    grammar_warnings_seen: bool,
    config_path: PathBuf,
    config_saving: Option<Receiver<Result<(), String>>>,
    pending: Option<PendingAction>,
    allow_close: bool,
    title: String,
    /// Configured editor font that is not installed, and the font used instead.
    missing_font: Option<(String, String)>,
    font_source: Option<Arc<dyn FontSource>>,
    /// Fallback kinds already requested; no view checks their characters again.
    fallbacks_requested: HashSet<Fallback>,
    fallback_tx: Sender<LoadedFallback>,
    fallback_rx: Receiver<LoadedFallback>,
    /// Editor font actually in use; fallbacks are aligned to its baseline.
    editor_font: String,
    replacer: Arc<dyn FileReplacer>,
    /// `None` while measuring: the session is neither restored nor saved.
    session: Option<SessionKeeper>,
    /// Files handed over by other Bracket processes (single instance).
    inbox: Option<Inbox>,
    /// Reports open files changed by other programs (`None` if it could not start).
    watcher: Option<FileWatcher>,
    disk_rx: Receiver<PathBuf>,
    /// Hash of the watched paths, to update the watcher only when they change.
    watched_signature: Option<u64>,
    probe: Option<Probe>,
    frames_ready: u32,
    /// What eframe last stored for the window while it was not minimized.
    stored_window: Option<String>,
    minimized: bool,
    /// The window opened with the maximized size but not maximized (see `lib.rs`): maximize
    /// it once it is on screen.
    maximize_pending: bool,
    frames_shown: u32,
}

impl BracketApp {
    pub fn new(
        ctx: &egui::Context,
        launch: Launch,
        mut probe: Option<Probe>,
        window: WindowStart,
    ) -> Self {
        if let Some(Probe::Startup(startup)) = &mut probe {
            startup.mark_app_created();
        }
        let Launch {
            files,
            requests_dir,
            settings,
            config_path,
            fonts,
            replacer,
            session,
            window_file: _,
            languages,
            window_icon: _,
        } = launch;
        let bracket_syntax::Packages {
            languages: package_languages,
            grammars,
            warnings: package_warnings,
        } = languages;
        bracket_syntax::highlight::set_external_grammars(grammars);
        let LoadedConfig {
            config,
            mut warnings,
        } = settings;
        ctx.set_theme(theme_preference(config.appearance));
        if config.editor_theme != AUTO_THEME
            && !highlight::theme_names().contains(&config.editor_theme.as_str())
        {
            warnings.push(ConfigWarning::InvalidValue {
                key: "editor.theme".to_owned(),
                found: format!("\"{}\"", config.editor_theme),
            });
        }

        // The font thread started at the beginning of main() and has normally finished by now.
        let setup = fonts.join().ok();
        let editor_font = setup
            .as_ref()
            .and_then(|s| s.startup.monospace.as_ref())
            .map_or_else(|| config.font_family.clone(), |f| f.family.clone());
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
        let (disk_tx, disk_rx) = mpsc::channel();
        let watcher_ctx = ctx.clone();
        let watcher = FileWatcher::new(move |path| {
            let _ = disk_tx.send(path);
            watcher_ctx.request_repaint();
        })
        .ok();

        let mut app = Self {
            texts: i18n::texts(config.language),
            config,
            tabs: Vec::new(),
            active: 0,
            next_tab_id: 0,
            languages: LanguageRegistry::with_packages(package_languages),
            package_warnings,
            grammar_warnings_seen: false,
            editor_theme: String::new(),
            theme_colors: ThemeColors::default(),
            reveal_active_tab: false,
            status: None,
            config_warnings: warnings,
            config_path,
            config_saving: None,
            missing_font,
            font_source,
            fallbacks_requested: HashSet::new(),
            fallback_tx,
            fallback_rx,
            editor_font,
            replacer,
            pending: None,
            allow_close: false,
            title: String::new(),
            session: None,
            inbox: None,
            watcher,
            disk_rx,
            watched_signature: None,
            probe,
            frames_ready: 0,
            stored_window: window.stored,
            minimized: false,
            maximize_pending: window.maximize,
            frames_shown: 0,
        };
        if let Some(restored) = session {
            app.restore_session(ctx, restored);
        }
        if app.tabs.is_empty() {
            let doc = app.empty_document();
            app.add_tab(doc);
        }
        app.focus_editor(ctx);
        for request in files {
            app.open_request(ctx, request);
        }
        if let Some(dir) = requests_dir {
            app.inbox = Inbox::start(ctx, dir);
        }
        app
    }

    /// Opens the tabs of the previous session; each one loads on a worker thread.
    fn restore_session(&mut self, ctx: &egui::Context, restored: RestoredSession) {
        let RestoredSession {
            files,
            previous,
            error,
        } = restored;
        if let Some(error) = error {
            self.status = Some(self.texts.session_damaged(&error));
        }
        self.session = Some(SessionKeeper::new(files, &previous));
        // Only the active tab and tabs with unsaved changes are read now; the others wait
        // until they are activated, so startup does not read files nobody is looking at.
        for (position, entry) in previous.tabs.into_iter().enumerate() {
            let doc = self.empty_document();
            let index = self.add_tab(doc);
            if position == previous.active || entry.unsaved.is_some() {
                self.start_restoring(ctx, index, entry);
            } else {
                let tab = &mut self.tabs[index];
                tab.unloaded = Some(entry);
                tab.refresh_language(&self.languages);
            }
        }
        if !self.tabs.is_empty() {
            self.active = previous.active.min(self.tabs.len() - 1);
            self.reveal_active_tab = true;
        }
    }

    /// Writes the session now (the window is closing). Returns false if it could not be
    /// written, and then unsaved changes must be dealt with before closing.
    fn save_session_now(&mut self) -> bool {
        let Some(keeper) = &mut self.session else {
            return false;
        };
        match keeper.save_now(&self.tabs, self.active) {
            Ok(()) => true,
            Err(err) => {
                self.status = Some(self.texts.session_save_failed(&err.to_string()));
                false
            }
        }
    }

    /// Keeps the session up to date while the app runs.
    fn keep_session(&mut self, ctx: &egui::Context) {
        let Some(keeper) = &mut self.session else {
            return;
        };
        if let Some(error) = keeper.tick(ctx, &self.tabs, self.active) {
            self.status = Some(self.texts.session_save_failed(&error));
        }
    }

    /// The first frame is painted while the window is still hidden; the window is maximized in
    /// the next one, when it is already on screen.
    fn maximize_once_shown(&mut self, ctx: &egui::Context) {
        if !self.maximize_pending || ctx.current_pass_index() != 0 {
            return;
        }
        self.frames_shown += 1;
        if self.frames_shown < 2 {
            ctx.request_repaint();
            return;
        }
        self.maximize_pending = false;
        ctx.send_viewport_cmd(ViewportCommand::Maximized(true));
    }

    /// Opens a file from the command line or another Bracket, at its line and column if given.
    fn open_request(&mut self, ctx: &egui::Context, request: OpenRequest) {
        let OpenRequest { path, line, column } = request;
        self.open_file(ctx, path);
        let Some(line) = line else { return };
        let position = (line, column.unwrap_or(1));
        let tab = &mut self.tabs[self.active];
        match &mut tab.loading {
            Some(loading) => loading.goto = Some(position),
            None => go_to(tab, position),
        }
    }

    /// Opens what other Bracket processes handed over, and comes to the front.
    fn poll_inbox(&mut self, ctx: &egui::Context) {
        let Some(inbox) = &mut self.inbox else { return };
        let Some(requests) = inbox.take() else { return };
        for request in requests {
            self.open_request(ctx, request);
        }
        ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(ViewportCommand::Focus);
    }

    /// Keeps the watched files in step with the tabs, and checks the files reported as changed
    /// on disk once their burst of changes is over.
    fn follow_disk(&mut self, ctx: &egui::Context) {
        let loaded = |tab: &&Tab| tab.loading.is_none();
        let mut hasher = DefaultHasher::new();
        for tab in self.tabs.iter().filter(loaded) {
            tab.doc.path().hash(&mut hasher);
        }
        let signature = Some(hasher.finish());
        if let Some(watcher) = &mut self.watcher
            && signature != self.watched_signature
        {
            watcher.set_paths(self.tabs.iter().filter(loaded).filter_map(|t| t.doc.path()));
            self.watched_signature = signature;
        }

        let now = Instant::now();
        while let Ok(path) = self.disk_rx.try_recv() {
            // Matched by file name only: the watcher may spell the folder differently (long
            // names for 8.3 ones, other case). A false match only costs a check of the stamp.
            let Some(changed) = path.file_name() else {
                continue;
            };
            for tab in &mut self.tabs {
                let shows_it = tab
                    .doc
                    .path()
                    .and_then(|p| p.file_name())
                    .is_some_and(|name| name.eq_ignore_ascii_case(changed));
                if tab.loading.is_none() && shows_it {
                    tab.disk.check_at = Some(now + disk::SETTLE);
                }
            }
        }

        let mut next_check: Option<Instant> = None;
        let mut reloaded = None;
        for tab in &mut self.tabs {
            if let Some(rx) = &tab.disk.checking {
                match rx.try_recv() {
                    Ok(result) => {
                        tab.disk.checking = None;
                        if tab.saving.is_some() {
                            // Bracket is writing the file itself: look again afterwards.
                            tab.disk.check_at = Some(now);
                        } else if let Ok(state) = result
                            && apply_disk_state(tab, state)
                        {
                            reloaded = Some(tab.title(self.texts));
                        }
                    }
                    Err(TryRecvError::Empty) => {}
                    Err(TryRecvError::Disconnected) => tab.disk.checking = None,
                }
            }
            let Some(at) = tab.disk.check_at else {
                continue;
            };
            // While loading or saving, the stamp is about to change: check afterwards.
            if tab.loading.is_some() || tab.saving.is_some() || tab.disk.checking.is_some() {
                continue;
            }
            if at > now {
                next_check = Some(next_check.map_or(at, |next| next.min(at)));
                continue;
            }
            tab.disk.check_at = None;
            let Some(path) = tab.doc.path().map(PathBuf::from) else {
                continue;
            };
            let (tx, rx) = mpsc::channel();
            let (read_encoding, known) = (tab.read_encoding, tab.disk.stamp);
            let ctx = ctx.clone();
            thread::spawn(move || {
                let _ = tx.send(disk::check(&path, read_encoding, known));
                ctx.request_repaint();
            });
            tab.disk.checking = Some(rx);
        }
        if let Some(at) = next_check {
            ctx.request_repaint_after(at - now);
        }
        if let Some(name) = reloaded {
            self.status = Some(self.texts.disk_reloaded(&name));
        }
    }

    /// Carries out what was picked in the active tab's notice about its file on disk.
    fn resolve_disk_notice(&mut self, action: NoticeAction) {
        let tab = &mut self.tabs[self.active];
        match (tab.disk.notice.take(), action) {
            (Some(DiskNotice::Changed { loaded, stamp }), NoticeAction::Reload) => {
                tab.doc.reload(loaded);
                tab.disk.stamp = Some(stamp);
            }
            // The user's text stays; the next save overwrites the file.
            (Some(DiskNotice::Changed { stamp, .. }), NoticeAction::Keep) => {
                tab.disk.stamp = Some(stamp);
            }
            _ => {}
        }
    }

    fn active_tab(&self) -> &Tab {
        &self.tabs[self.active]
    }

    fn tab_index(&self, id: u64) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.id == id)
    }

    fn empty_document(&self) -> Document {
        let mut doc = Document::new_empty(self.config.line_ending, self.config.encoding);
        doc.buffer.set_tab_width(self.config.tab_width);
        doc
    }

    /// Adds a tab at the end and returns its index; it does not become active.
    fn add_tab(&mut self, doc: Document) -> usize {
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        let mut view = EditorView::for_tab(id, self.config.font_size);
        for kind in &self.fallbacks_requested {
            view.fallback_requested(*kind);
        }
        let untitled = tabs::free_untitled_number(&self.tabs);
        let mut tab = Tab::new(id, doc, view, untitled);
        tab.refresh_language(&self.languages);
        self.tabs.push(tab);
        self.tabs.len() - 1
    }

    fn activate(&mut self, ctx: &egui::Context, index: usize) {
        self.active = index;
        self.reveal_active_tab = true;
        if let Some(entry) = self.tabs[index].unloaded.take() {
            self.start_restoring(ctx, index, entry);
        }
        self.focus_editor(ctx);
    }

    fn focus_editor(&self, ctx: &egui::Context) {
        let id = self.active_tab().view.id();
        ctx.memory_mut(|m| m.request_focus(id));
    }

    /// Closes a tab, asking first if it has unsaved changes.
    fn close_tab(&mut self, ctx: &egui::Context, index: usize) {
        let tab = &self.tabs[index];
        if tab.doc.is_dirty() {
            self.pending = Some(PendingAction::CloseTab(tab.id));
        } else {
            self.remove_tab(ctx, index);
        }
    }

    /// Removes a tab without asking. The tab to its right becomes active (or the one to its
    /// left at the end); closing the last tab leaves an untitled one.
    fn remove_tab(&mut self, ctx: &egui::Context, index: usize) {
        self.tabs.remove(index);
        if self.tabs.is_empty() {
            let doc = self.empty_document();
            self.add_tab(doc);
        }
        let active = if index < self.active {
            self.active - 1
        } else {
            self.active.min(self.tabs.len() - 1)
        };
        self.activate(ctx, active);
    }

    /// Opens a file in a tab: the one already showing it, the active one if it is blank, or a
    /// new one.
    fn open_file(&mut self, ctx: &egui::Context, path: PathBuf) {
        let open = self.tabs.iter().position(|tab| {
            tab.path()
                .is_some_and(|tab_path| tabs::same_file(tab_path, &path))
        });
        if let Some(index) = open {
            self.activate(ctx, index);
            return;
        }
        let (index, new_tab) = if self.active_tab().is_blank() {
            (self.active, false)
        } else {
            let doc = self.empty_document();
            (self.add_tab(doc), true)
        };
        self.start_loading(ctx, index, path, None, new_tab);
        self.activate(ctx, index);
    }

    /// Loads a fallback font for `c` on a worker thread, once per kind of fallback.
    fn request_fallback(&mut self, ctx: &egui::Context, c: char) {
        let kind = Fallback::for_char(c);
        if !self.fallbacks_requested.insert(kind) {
            return;
        }
        // Whatever happens next, the views stop looking for this kind of character.
        for tab in &mut self.tabs {
            tab.view.fallback_requested(kind);
        }
        let Some(source) = self.font_source.clone() else {
            return;
        };
        let tx = self.fallback_tx.clone();
        let ctx = ctx.clone();
        let (primary, font_size) = (self.editor_font.clone(), self.config.font_size);
        thread::spawn(move || {
            if let Some(fallback) = fonts::load_fallback(source.as_ref(), kind, &primary, font_size)
            {
                let _ = tx.send(fallback);
                ctx.request_repaint();
            }
        });
    }

    fn poll_fallbacks(&mut self, ctx: &egui::Context) {
        while let Ok(fallback) = self.fallback_rx.try_recv() {
            ctx.add_font(fonts::fallback_insert(fallback));
        }
    }

    /// Loads a file into a tab on a worker thread; the UI keeps running and the tab shows a
    /// loading message. `encoding` forces how the bytes are decoded ("reopen with encoding");
    /// `None` detects it.
    fn start_loading(
        &mut self,
        ctx: &egui::Context,
        index: usize,
        path: PathBuf,
        encoding: Option<TextEncoding>,
        new_tab: bool,
    ) {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let (line_ending, tab_width) = (self.config.line_ending, self.config.tab_width);
        let thread_path = path.clone();
        thread::spawn(move || {
            let path = thread_path;
            // Taken before reading: a change while reading shows up as a newer stamp later.
            let stamp = DiskStamp::of(&path).ok();
            let loaded = match encoding {
                Some(encoding) => document::load_text_as(&path, encoding),
                None => document::load_text(&path),
            };
            let result = loaded.map(|loaded| {
                let mut doc = Document::from_loaded(path, loaded, line_ending);
                doc.buffer.set_tab_width(tab_width);
                Loaded { doc, stamp }
            });
            // The receiver may be gone if the tab or the window closed; nothing to do then.
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        self.tabs[index].loading = Some(Loading {
            path: Some(path),
            read_encoding: encoding,
            restore: None,
            rx,
            new_tab,
            goto: None,
            started: Instant::now(),
        });
    }

    /// Rebuilds a tab of the previous session on a worker thread, like [`Self::start_loading`].
    fn start_restoring(&mut self, ctx: &egui::Context, index: usize, entry: SessionTab) {
        let Some(files) = self.session.as_ref().map(|keeper| keeper.files().clone()) else {
            return;
        };
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let fallback = (self.config.line_ending, self.config.encoding);
        let tab_width = self.config.tab_width;
        let thread_entry = entry.clone();
        thread::spawn(move || {
            let stamp = thread_entry
                .path
                .as_deref()
                .and_then(|path| DiskStamp::of(path).ok());
            let result = session::restore_document(&thread_entry, &files, fallback, tab_width)
                .map(|doc| Loaded { doc, stamp });
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        self.tabs[index].loading = Some(Loading {
            path: entry.path.clone(),
            read_encoding: entry.read_encoding,
            restore: Some(entry),
            rx,
            new_tab: true,
            goto: None,
            started: Instant::now(),
        });
    }

    fn poll_loading(&mut self, ctx: &egui::Context) {
        let mut failed = Vec::new();
        let mut loaded_active = false;
        for (index, tab) in self.tabs.iter_mut().enumerate() {
            let Some(loading) = &tab.loading else {
                continue;
            };
            let new_tab = loading.new_tab;
            let read_encoding = loading.read_encoding;
            let top_line = loading.restore.as_ref().map(|entry| entry.top_line);
            let goto = loading.goto;
            let chosen_language = loading
                .restore
                .as_ref()
                .and_then(|entry| entry.language.clone());
            match loading.rx.try_recv() {
                Ok(Ok(Loaded { doc, stamp })) => {
                    tab.doc = doc;
                    tab.highlight = None;
                    tab.disk = DiskWatch::synced(stamp);
                    tab.view.reset();
                    if let Some(line) = top_line {
                        tab.view.scroll_to_line(line);
                    }
                    tab.read_encoding = read_encoding;
                    tab.loading = None;
                    if let Some(position) = goto {
                        go_to(tab, position);
                    }
                    tab.refresh_language(&self.languages);
                    if let Some(id) = chosen_language {
                        // Picked by hand last time; an empty id is plain text.
                        if id.is_empty() || self.languages.get(&id).is_some() {
                            tab.language = (!id.is_empty()).then_some(id);
                            tab.language_chosen = true;
                        }
                    }
                    if let Some(width) = tab
                        .language
                        .as_deref()
                        .and_then(|id| self.languages.get(id))
                        .and_then(|language| language.tab_width)
                    {
                        tab.doc.buffer.set_tab_width(width);
                    }
                    self.status = None;
                    loaded_active |= index == self.active;
                }
                Ok(Err(err)) => {
                    self.status = Some(self.texts.open_failed(&err.to_string()));
                    tab.loading = None;
                    if new_tab {
                        failed.push(tab.id);
                    }
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    tab.loading = None;
                    if new_tab {
                        failed.push(tab.id);
                    }
                }
            }
        }
        for tab in &mut self.tabs {
            if tab.loading.is_none() && tab.unloaded.is_none() && tab.highlight.is_none() {
                start_highlight(tab, &self.languages, &self.editor_theme, ctx);
            }
        }
        // A tab opened for a file that could not be read has nothing to show.
        for id in failed {
            if let Some(index) = self.tab_index(id) {
                self.remove_tab(ctx, index);
            }
        }
        if loaded_active {
            self.focus_editor(ctx);
        }
    }

    /// Saves a tab on a worker thread. The text is a cheap snapshot, so editing can continue.
    fn start_saving(&mut self, ctx: &egui::Context, index: usize) {
        let tab = &mut self.tabs[index];
        if tab.saving.is_some() {
            return;
        }
        if tab.doc.is_read_only() {
            self.status = Some(self.texts.save_read_only().to_owned());
            return;
        }
        // Untitled documents go through Save As first, so there is always a path here.
        let Some(request) = tab.doc.save_request() else {
            return;
        };
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let replacer = Arc::clone(&self.replacer);
        thread::spawn(move || {
            let result = request
                .write(replacer.as_ref())
                .map(|()| DiskStamp::of(&request.path).ok());
            let _ = tx.send((request.version, request.encoding, result));
            ctx.request_repaint();
        });
        tab.saving = Some(rx);
    }

    fn poll_saving(&mut self) {
        for tab in &mut self.tabs {
            let Some(rx) = &tab.saving else { continue };
            match rx.try_recv() {
                Ok((version, encoding, Ok(stamp))) => {
                    tab.doc.mark_saved(version, encoding);
                    tab.disk.stamp = stamp;
                    tab.disk.notice = None;
                    self.status = Some(self.texts.saved().to_owned());
                    tab.saving = None;
                }
                Ok((_, _, Err(err))) => {
                    self.status = Some(save_error_text(self.texts, &err));
                    tab.saving = None;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => tab.saving = None,
            }
        }
    }

    /// Saves a tab on the UI thread. Only used from the unsaved-changes dialog, where the next
    /// action must wait for the result anyway.
    fn save_now(&mut self, frame: &eframe::Frame, index: usize) -> bool {
        if self.tabs[index].doc.path().is_none() {
            // With several untitled tabs to save, the title says which one this is.
            let title = format!(
                "{} — {}",
                self.texts.dialog_save_as_title(),
                self.tabs[index].title(self.texts)
            );
            let Some(path) = dialogs::pick_file_to_save(frame, &title, None) else {
                return false;
            };
            let tab = &mut self.tabs[index];
            tab.doc.set_path(path);
            tab.refresh_language(&self.languages);
            // The language may have changed: highlighting starts again on the next frame.
            tab.highlight = None;
        }
        let tab = &mut self.tabs[index];
        match tab.doc.save_request() {
            Some(request) => match request.write(self.replacer.as_ref()) {
                Ok(()) => {
                    tab.doc.mark_saved(request.version, request.encoding);
                    tab.disk = DiskWatch::synced(DiskStamp::of(&request.path).ok());
                    true
                }
                Err(err) => {
                    self.status = Some(save_error_text(self.texts, &err));
                    false
                }
            },
            None => {
                self.status = Some(self.texts.save_read_only().to_owned());
                false
            }
        }
    }

    fn run_command(&mut self, ctx: &egui::Context, frame: &eframe::Frame, command: Command) {
        let index = self.active;
        let loading = self.active_tab().loading.is_some();
        match command {
            Command::New => {
                let doc = self.empty_document();
                let index = self.add_tab(doc);
                self.activate(ctx, index);
                self.status = None;
            }
            Command::Open => self.open_with_dialog(ctx, frame),
            // Nothing to save until the file has loaded.
            Command::Save | Command::SaveAs if loading => {}
            Command::Save if self.active_tab().doc.path().is_none() => {
                self.save_as(ctx, frame, index);
            }
            Command::Save => self.start_saving(ctx, index),
            Command::SaveAs => self.save_as(ctx, frame, index),
            Command::CloseTab => self.close_tab(ctx, index),
            Command::NextTab => self.activate(ctx, (index + 1) % self.tabs.len()),
            Command::PreviousTab => {
                let count = self.tabs.len();
                self.activate(ctx, (index + count - 1) % count);
            }
            // Closing goes through the unsaved-changes check in handle_window_events.
            Command::Exit => ctx.send_viewport_cmd(ViewportCommand::Close),
        }
        self.focus_editor(ctx);
    }

    fn open_with_dialog(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        let start_dir = self
            .active_tab()
            .path()
            .and_then(|p| p.parent())
            .map(PathBuf::from);
        let title = self.texts.dialog_open_title();
        if let Some(path) = dialogs::pick_file_to_open(frame, title, start_dir.as_deref()) {
            self.open_file(ctx, path);
        }
    }

    fn save_as(&mut self, ctx: &egui::Context, frame: &eframe::Frame, index: usize) {
        if self.tabs[index].doc.is_read_only() {
            self.status = Some(self.texts.save_read_only().to_owned());
            return;
        }
        let title = self.texts.dialog_save_as_title();
        if let Some(path) = dialogs::pick_file_to_save(frame, title, self.tabs[index].doc.path()) {
            let tab = &mut self.tabs[index];
            tab.doc.set_path(path);
            tab.refresh_language(&self.languages);
            // The language may have changed: highlighting starts again on the next frame.
            tab.highlight = None;
            self.start_saving(ctx, index);
        }
    }

    /// Draws the menu bar. Returns the command picked, if any.
    fn menu_bar(&self, ui: &mut egui::Ui) -> (Option<Command>, Option<ViewAction>) {
        let mut picked = None;
        let mut view_pick = None;
        let theme_names = highlight::theme_names();
        let items = [
            (Command::New, self.texts.menu_new()),
            (Command::Open, self.texts.menu_open()),
            (Command::Save, self.texts.menu_save()),
            (Command::SaveAs, self.texts.menu_save_as()),
            (Command::CloseTab, self.texts.menu_close_tab()),
        ];
        egui::Panel::top("menu").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button(self.texts.menu_file(), |ui| {
                    for (command, label) in items {
                        let mut button = egui::Button::new(label);
                        if let Some(shortcut) = command.shortcut() {
                            button = button.shortcut_text(ui.ctx().format_shortcut(&shortcut));
                        }
                        if ui.add(button).clicked() {
                            picked = Some(command);
                            ui.close();
                        }
                    }
                    ui.separator();
                    if ui.button(self.texts.menu_exit()).clicked() {
                        picked = Some(Command::Exit);
                        ui.close();
                    }
                });
                ui.menu_button(self.texts.menu_view(), |ui| {
                    ui.menu_button(self.texts.menu_appearance(), |ui| {
                        for appearance in Appearance::ALL {
                            let label = self.texts.appearance(appearance);
                            let current = appearance == self.config.appearance;
                            if ui.selectable_label(current, label).clicked() {
                                view_pick = Some(ViewAction::Appearance(appearance));
                                ui.close();
                            }
                        }
                    });
                    ui.menu_button(self.texts.menu_editor_theme(), |ui| {
                        let auto = self.config.editor_theme == AUTO_THEME;
                        if ui
                            .selectable_label(auto, self.texts.editor_theme_auto())
                            .clicked()
                        {
                            view_pick = Some(ViewAction::EditorTheme(AUTO_THEME.to_owned()));
                            ui.close();
                        }
                        ui.separator();
                        for name in &theme_names {
                            let current = self.config.editor_theme == *name;
                            if ui.selectable_label(current, *name).clicked() {
                                view_pick = Some(ViewAction::EditorTheme((*name).to_owned()));
                                ui.close();
                            }
                        }
                    });
                });
            });
        });
        (picked, view_pick)
    }

    /// Draws the tab bar. Returns what was done with the mouse, if anything.
    fn tab_bar(&mut self, ui: &mut egui::Ui) -> Option<TabAction> {
        let fill = ui.visuals().extreme_bg_color;
        let action = egui::Panel::top("tabs")
            .frame(egui::Frame::NONE.fill(fill))
            .show(ui, |ui| {
                tabs::tab_bar(
                    ui,
                    &self.tabs,
                    self.active,
                    &self.languages,
                    self.texts,
                    self.reveal_active_tab,
                )
            })
            .inner;
        self.reveal_active_tab = false;
        action
    }

    fn handle_window_events(&mut self, ctx: &egui::Context) {
        let close_requested = ctx.input(|i| i.viewport().close_requested());
        if close_requested && !self.allow_close {
            // Unsaved changes go into the session and come back next time. Only if it cannot
            // be written does the user have to decide about them now.
            if self.save_session_now() {
                self.allow_close = true;
            } else if self.tabs.iter().any(|tab| tab.doc.is_dirty()) {
                ctx.send_viewport_cmd(ViewportCommand::CancelClose);
                self.pending = Some(PendingAction::CloseWindow);
            }
        }

        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        for path in dropped {
            self.open_file(ctx, path);
        }
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let tab = self.active_tab();
        let dirty = if tab.doc.is_dirty() { " ●" } else { "" };
        let title = format!("{}{dirty} — Bracket", tab.title(self.texts));
        if title != self.title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    /// Draws the status bar. Returns what was picked in one of its menus, if anything.
    /// Draws the status bar. Returns what was picked in one of its menus, if anything.
    ///
    /// Right to left: interface language, document language, encoding, line ending, position
    /// and selections. The message (status, warnings or a hint) takes the space left.
    fn status_bar(&self, ui: &mut egui::Ui) -> Option<StatusAction> {
        let mut picked = None;
        let tab = self.active_tab();
        let doc = &tab.doc;
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button(self.texts.language_name(), |ui| {
                    for language in LANGUAGES {
                        let name = i18n::texts(language).language_name();
                        if ui
                            .selectable_label(language == self.config.language, name)
                            .clicked()
                        {
                            picked = Some(StatusAction::InterfaceLanguage(language));
                            ui.close();
                        }
                    }
                })
                .response
                .on_hover_text(self.texts.language_tooltip());
                ui.separator();

                let current = tab.language.as_deref();
                let current_name = current
                    .and_then(|id| self.languages.get(id))
                    .map_or(self.texts.plain_text(), |l| l.name.as_str());
                ui.menu_button(current_name, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(400.0)
                        .show(ui, |ui| {
                            if ui
                                .selectable_label(current.is_none(), self.texts.plain_text())
                                .clicked()
                            {
                                picked = Some(StatusAction::DocumentLanguage(None));
                                ui.close();
                            }
                            for language in self.languages.languages() {
                                let selected = current == Some(language.id.as_str());
                                if ui.selectable_label(selected, &language.name).clicked() {
                                    picked = Some(StatusAction::DocumentLanguage(Some(
                                        language.id.clone(),
                                    )));
                                    ui.close();
                                }
                            }
                        });
                })
                .response
                .on_hover_text(self.texts.document_language_tooltip());
                ui.separator();

                let current_encoding = doc.encoding();
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
                if doc.is_read_only() {
                    ui.label(self.texts.read_only_short())
                        .on_hover_text(self.texts.read_only());
                }
                ui.separator();

                let current_line_ending = doc.line_ending();
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

                ui.label(position_text(self.texts, doc));
                ui.separator();

                let warnings: Vec<String> = self
                    .config_warnings
                    .iter()
                    .map(|w| self.texts.config_warning(w))
                    .chain(
                        self.package_warnings
                            .iter()
                            .map(|w| self.texts.package_warning(w)),
                    )
                    .collect();
                let notice = warnings_summary(self.texts, &warnings).or_else(|| {
                    let (family, used) = self.missing_font.as_ref()?;
                    Some(self.texts.font_missing(family, used))
                });
                ui.with_layout(
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| match self.status.as_ref().or(notice.as_ref()) {
                        Some(message) => {
                            ui.add(egui::Label::new(message.as_str()).truncate());
                        }
                        None if doc.path().is_none() => {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(self.texts.drop_hint()).weak(),
                                )
                                .truncate(),
                            );
                        }
                        None => {}
                    },
                );
            });
        });
        picked
    }

    fn run_status_action(&mut self, ctx: &egui::Context, action: StatusAction) {
        let index = self.active;
        if let StatusAction::DocumentLanguage(id) = action {
            let tab = &mut self.tabs[index];
            tab.language = id;
            tab.language_chosen = true;
            // Highlighting starts again with the new language's grammar.
            tab.highlight = None;
            return;
        }
        let doc = &mut self.tabs[index].doc;
        match action {
            StatusAction::InterfaceLanguage(language) => self.set_language(ctx, language),
            StatusAction::DocumentLanguage(_) => {} // handled above
            StatusAction::ConvertLineEndings(line_ending) => {
                if !doc.is_read_only() {
                    doc.convert_line_endings(line_ending);
                }
            }
            StatusAction::SaveWith(encoding) => {
                if !doc.is_read_only() {
                    doc.set_encoding(encoding);
                }
            }
            StatusAction::ReopenWith(encoding) => {
                let Some(path) = doc.path().map(PathBuf::from) else {
                    return;
                };
                if doc.is_dirty() {
                    self.status = Some(self.texts.reopen_needs_clean().to_owned());
                } else {
                    self.start_loading(ctx, index, path, Some(encoding), false);
                }
            }
        }
    }

    fn run_view_action(&mut self, ctx: &egui::Context, action: ViewAction) {
        match action {
            ViewAction::Appearance(appearance) => {
                if appearance == self.config.appearance {
                    return;
                }
                self.config.appearance = appearance;
                ctx.set_theme(theme_preference(appearance));
            }
            ViewAction::EditorTheme(name) => {
                if name == self.config.editor_theme {
                    return;
                }
                self.config.editor_theme = name;
            }
        }
        self.follow_theme(ctx);
        self.start_config_save(ctx);
    }

    /// Keeps the editor theme in step with the configuration and the interface (light or
    /// dark, which may follow Windows). A new theme restarts highlighting.
    fn follow_theme(&mut self, ctx: &egui::Context) {
        let configured = self.config.editor_theme.as_str();
        let theme = if configured != AUTO_THEME && highlight::theme_names().contains(&configured) {
            configured
        } else if ctx.theme() == egui::Theme::Dark {
            highlight::DARK_THEME
        } else {
            highlight::LIGHT_THEME
        };
        if theme == self.editor_theme {
            return;
        }
        self.editor_theme = theme.to_owned();
        self.theme_colors = highlight::theme_colors(theme);
        for tab in &mut self.tabs {
            tab.highlight = None;
        }
        ctx.request_repaint();
    }

    /// The editor-area colours: the theme's, or the interface's where it has none.
    fn editor_style(&self, visuals: &egui::Visuals) -> EditorStyle {
        let base = EditorStyle::from_visuals(visuals);
        let colors = &self.theme_colors;
        EditorStyle {
            text: colors.foreground.map_or(base.text, color32),
            line_number: colors.gutter_foreground.map_or(base.line_number, color32),
            selection: colors.selection.map_or(base.selection, color32),
            caret: colors.caret.map_or(base.caret, color32),
            gutter: colors.gutter.map(color32),
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

    fn unsaved_dialog(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        let Some(action) = self.pending else {
            return;
        };
        let dirty: Vec<usize> = match action {
            PendingAction::CloseTab(id) => self.tab_index(id).into_iter().collect(),
            PendingAction::CloseWindow => (0..self.tabs.len()).collect(),
        };
        let dirty: Vec<usize> = dirty
            .into_iter()
            .filter(|&index| self.tabs[index].doc.is_dirty())
            .collect();
        if dirty.is_empty() {
            self.pending = None;
            self.finish_pending(ctx, action);
            return;
        }

        let texts = self.texts;
        let (message, save_label) = match dirty.as_slice() {
            [index] => (
                texts.unsaved_message(&self.tabs[*index].title(texts)),
                texts.save(),
            ),
            many => (texts.unsaved_many_message(many.len()), texts.save_all()),
        };
        let mut choice = None;
        egui::Modal::new(Id::new("unsaved-changes")).show(ctx, |ui| {
            ui.heading(texts.unsaved_title());
            ui.label(message);
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button(save_label).clicked() {
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
        let Some(save) = choice else { return };
        self.pending = None;
        if save {
            for index in dirty {
                // Show the document being saved; stop at the first one that is not saved.
                self.activate(ctx, index);
                if !self.save_now(frame, index) {
                    return;
                }
            }
        }
        self.finish_pending(ctx, action);
    }

    fn finish_pending(&mut self, ctx: &egui::Context, action: PendingAction) {
        match action {
            PendingAction::CloseTab(id) => {
                if let Some(index) = self.tab_index(id) {
                    self.remove_tab(ctx, index);
                }
            }
            PendingAction::CloseWindow => {
                self.allow_close = true;
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
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
        let tab = &mut self.tabs[self.active];
        if tab.loading.is_some() || ctx.current_pass_index() != 0 {
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
                let buffer = &mut tab.doc.buffer;
                if buffer.selections().primary().head == buffer.len_chars() {
                    buffer.set_selections(bracket_core::Selections::caret(0));
                } else {
                    buffer.move_selections(Motion::Down(tab.view.page_lines()), false);
                }
                tab.view.request_scroll_to_caret();
                scroll.frame();
            }
        }
    }
}

impl eframe::App for BracketApp {
    /// eframe keeps only the window position and size (see `lib.rs`), not egui's widget state.
    fn persist_egui_memory(&self) -> bool {
        false
    }

    /// eframe has just stored the window position and size. While minimized it would store a
    /// tiny window at (0, 0), so the last value stored with the window not minimized goes back.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        if !self.minimized {
            self.stored_window = storage.get_string(WINDOW_KEY);
        } else if let Some(window) = &self.stored_window {
            storage.set_string(WINDOW_KEY, window.clone());
        } else {
            // Nothing good to put back: next time the window opens centered.
            storage.remove_string(WINDOW_KEY);
        }
    }

    /// Often enough that the value kept for a minimized window is recent. eframe only writes
    /// the file when something changed.
    fn auto_save_interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(5)
    }

    /// Everything that does not paint. eframe calls it before each `ui`, and also while the
    /// window is minimized, so files handed over by another Bracket, changes on disk, the
    /// session and closing from the taskbar are handled even then.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.minimized = ctx.input(|i| i.viewport().minimized.unwrap_or(false));
        self.follow_theme(ctx);
        self.poll_loading(ctx);
        self.poll_saving();
        self.poll_config_saving();
        self.poll_fallbacks(ctx);
        self.poll_inbox(ctx);
        self.follow_disk(ctx);
        if !self.grammar_warnings_seen
            && let Some(warnings) = bracket_syntax::highlight::external_grammar_warnings()
        {
            self.package_warnings.extend_from_slice(warnings);
            self.grammar_warnings_seen = true;
        }
        self.handle_window_events(ctx);
        self.keep_session(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // Application shortcuts work wherever the keyboard focus is (for example after using a
        // status bar menu); consuming them keeps the editor from seeing them too.
        let shortcut = Command::SHORTCUTS
            .iter()
            .find(|(_, shortcut)| ctx.input_mut(|i| i.consume_shortcut(shortcut)))
            .map(|(command, _)| *command);
        let (menu_command, view_action) = self.menu_bar(ui);
        if let Some(command) = shortcut.or(menu_command) {
            self.run_command(&ctx, frame, command);
        }
        if let Some(action) = view_action {
            self.run_view_action(&ctx, action);
            self.focus_editor(&ctx);
        }
        match self.tab_bar(ui) {
            Some(TabAction::Activate(index)) => self.activate(&ctx, index),
            Some(TabAction::Close(index)) => self.close_tab(&ctx, index),
            None => {}
        }
        if let Some(action) = self.status_bar(ui) {
            self.run_status_action(&ctx, action);
            // Give the keyboard back to the editor after using a menu.
            self.focus_editor(&ctx);
        }
        let mut missing_glyph = None;
        let mut notice_action = None;
        let style = self.editor_style(ui.visuals());
        let background = self
            .theme_colors
            .background
            .map_or(ui.visuals().panel_fill, color32);
        let panel = egui::Frame::central_panel(ui.style()).fill(background);
        egui::CentralPanel::default().frame(panel).show(ui, |ui| {
            let tab = &mut self.tabs[self.active];
            if let Some(loading) = &tab.loading {
                let waited = loading.started.elapsed();
                if waited >= tabs::LOADING_MESSAGE_DELAY {
                    ui.centered_and_justified(|ui| ui.label(self.texts.loading()));
                } else {
                    ctx.request_repaint_after(tabs::LOADING_MESSAGE_DELAY - waited);
                }
            } else {
                if let Some(notice) = &tab.disk.notice {
                    let name = tab.title(self.texts);
                    notice_action = disk::notice_bar(ui, notice, &name, self.texts);
                }
                let Tab {
                    view,
                    doc,
                    highlight,
                    ..
                } = tab;
                let highlights = highlight.as_mut().and_then(TabHighlight::current);
                let output = view.show(ui, doc, highlights, &style);
                if let Some(highlight) = highlight {
                    highlight.follow(doc, output.visible_lines);
                }
                missing_glyph = output.missing_glyph;
            }
        });
        if let Some(action) = notice_action {
            self.resolve_disk_notice(action);
            self.focus_editor(&ctx);
        }
        if let Some(c) = missing_glyph {
            self.request_fallback(&ctx, c);
        }
        self.unsaved_dialog(&ctx, frame);
        self.update_title(&ctx);
        self.maximize_once_shown(&ctx);

        self.run_probe(&ctx);
    }
}

fn theme_preference(appearance: Appearance) -> egui::ThemePreference {
    match appearance {
        Appearance::System => egui::ThemePreference::System,
        Appearance::Light => egui::ThemePreference::Light,
        Appearance::Dark => egui::ThemePreference::Dark,
    }
}

fn color32([r, g, b, a]: [u8; 4]) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(r, g, b, a)
}

/// Starts highlighting a tab if its language has a grammar and the text is not too big.
fn start_highlight(tab: &mut Tab, languages: &LanguageRegistry, theme: &str, ctx: &egui::Context) {
    let Some(language) = tab.language.as_deref().and_then(|id| languages.get(id)) else {
        return;
    };
    if tab.doc.buffer.text().len_bytes() > MAX_HIGHLIGHT_BYTES {
        return;
    }
    let ctx = ctx.clone();
    tab.highlight = Some(TabHighlight::start(
        language.scope.clone(),
        theme.to_owned(),
        move || ctx.request_repaint(),
    ));
}

/// The folder where other Bracket processes leave files for this one, and its watcher.
struct Inbox {
    dir: PathBuf,
    _watcher: Option<FileWatcher>,
    rx: Receiver<()>,
    /// Requests left before the watcher started are read on the first frame.
    checked: bool,
}

impl Inbox {
    fn start(ctx: &egui::Context, dir: PathBuf) -> Option<Self> {
        std::fs::create_dir_all(&dir).ok()?;
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let watcher = FileWatcher::new(move |_| {
            let _ = tx.send(());
            ctx.request_repaint();
        })
        .ok()
        .map(|mut watcher| {
            watcher.set_paths([dir.as_path()]);
            watcher
        });
        Some(Self {
            dir,
            _watcher: watcher,
            rx,
            checked: false,
        })
    }

    /// The requests waiting in the folder, if something arrived since the last look.
    fn take(&mut self) -> Option<Vec<OpenRequest>> {
        let mut arrived = !self.checked;
        while self.rx.try_recv().is_ok() {
            arrived = true;
        }
        self.checked = true;
        if !arrived {
            return None;
        }
        requests::take_requests(&self.dir)
    }
}

/// Puts the caret on a 1-based line and column (in characters), clamped to the text.
fn go_to(tab: &mut Tab, (line, column): (usize, usize)) {
    let text = tab.doc.buffer.text();
    let line = line.saturating_sub(1).min(text.len_lines() - 1);
    let start = movement::line_start(text, line);
    let pos = (start + column.saturating_sub(1)).min(movement::line_end(text, line));
    tab.doc.buffer.set_selections(Selections::caret(pos));
    tab.view.request_scroll_to_caret();
}

/// Applies the result of a disk check to a tab. Returns true if the document was reloaded
/// silently (it had no unsaved changes).
fn apply_disk_state(tab: &mut Tab, state: DiskState) -> bool {
    match state {
        DiskState::Unchanged => false,
        DiskState::Missing => {
            if !matches!(tab.disk.notice, Some(DiskNotice::Missing)) {
                // Modified until saved again, so closing asks and the session keeps the text.
                tab.doc.mark_missing();
                tab.disk.notice = Some(DiskNotice::Missing);
                tab.disk.stamp = None;
            }
            false
        }
        DiskState::Changed { loaded, stamp } => {
            if tab.doc.matches(&loaded) {
                // Touched, or written back with the same text: in sync again.
                let version = tab.doc.buffer.version();
                tab.doc.mark_saved(version, loaded.encoding);
                tab.disk.stamp = Some(stamp);
                tab.disk.notice = None;
                false
            } else if !tab.doc.is_dirty() {
                tab.doc.reload(loaded);
                tab.disk.stamp = Some(stamp);
                tab.disk.notice = None;
                true
            } else {
                tab.disk.notice = Some(DiskNotice::Changed { loaded, stamp });
                false
            }
        }
    }
}

fn save_error_text(texts: &Texts, err: &SaveError) -> String {
    match err {
        SaveError::Encode(err) => {
            let character = err.character.map(String::from).unwrap_or_default();
            texts.save_encode_failed(&err.encoding.label(), &character)
        }
        SaveError::Io(err) => texts.save_failed(&err.to_string()),
    }
}

/// "Ln 3, Col 7", with the number of selections and of selected characters when there are.
fn position_text(texts: &Texts, doc: &Document) -> String {
    let text = doc.buffer.text();
    let selections = doc.buffer.selections();
    let head = selections.primary().head;
    let line = movement::line_of(text, head) + 1;
    let column = movement::visual_column(text, head, doc.buffer.tab_width()) + 1;
    let selected: usize = selections.iter().map(|s| s.len()).sum();
    let base = match selections.len() {
        1 => texts.position(line, column),
        count => texts.selections(count),
    };
    if selected == 0 {
        base
    } else {
        format!("{base} {}", texts.selected_chars(selected))
    }
}

/// The first warning plus how many more there are.
fn warnings_summary(texts: &Texts, warnings: &[String]) -> Option<String> {
    let first = warnings.first()?;
    Some(match warnings.len() - 1 {
        0 => first.clone(),
        more => format!("{first} {}", texts.more_warnings(more)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bracket_core::{Selection, Selections};

    fn doc(text: &str, selections: Vec<Selection>) -> Document {
        let mut doc = Document::new_empty(LineEnding::Lf, TextEncoding::UTF8);
        doc.buffer.insert(text);
        doc.buffer.set_selections(Selections::new(selections, 0));
        doc
    }

    #[test]
    fn the_position_shows_selections_and_selected_characters() {
        let texts = i18n::texts(Language::English);
        let text = "one\ntwo\tx\n";
        assert_eq!(
            position_text(texts, &doc(text, vec![Selection::caret(6)])),
            "Ln 2, Col 3"
        );
        assert_eq!(
            position_text(texts, &doc(text, vec![Selection::caret(9)])),
            "Ln 2, Col 6",
            "columns count tab stops"
        );
        assert_eq!(
            position_text(texts, &doc(text, vec![Selection::new(0, 3)])),
            "Ln 1, Col 4 (3 selected)"
        );
        assert_eq!(
            position_text(
                texts,
                &doc(text, vec![Selection::caret(0), Selection::new(4, 7)])
            ),
            "2 selections (3 selected)"
        );
    }
}
