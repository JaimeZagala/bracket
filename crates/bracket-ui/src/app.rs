//! The application shell: open documents as tabs, file loading and saving on worker threads,
//! window title, status bar and the unsaved-changes dialog.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::Instant;

use bracket_core::document::{self, DiskStamp, Document, FileReplacer, LineEnding, SaveError};
use bracket_core::encoding::{self, TextEncoding};
use bracket_core::movement::{self, Motion};
use bracket_core::{Selection, Selections};
use bracket_index::WalkOptions;
use bracket_index::project_symbols::FoundSymbol;
use bracket_index::rules::RuleSet;
use bracket_search::{FileOutcome, replace_in_file};
use bracket_session::config::AUTO_THEME;
use bracket_session::{
    Appearance, Config, ConfigWarning, FileWatcher, Language, LoadedConfig, OpenRequest,
    ProjectChanges, ProjectWatcher, Recent, SessionTab, Sidebar, config, requests,
};
use bracket_syntax::ThemeColors;
use bracket_syntax::highlight::{self, MAX_HIGHLIGHT_BYTES};
use bracket_syntax::{LanguageRegistry, PackageWarning};
use eframe::egui::{self, Id, ViewportCommand};

use crate::commands::{self, Command, Menu};
use crate::dialogs;
use crate::disk::{self, DiskNotice, DiskState, DiskWatch, NoticeAction};
use crate::editor_view::{EditorStyle, EditorView};
use crate::file_ops::{FileOp, FileOps, OpError};
use crate::find_bar::{FindAction, FindBar, ReplaceOutcome};
use crate::fonts::{self, Fallback, FontSource, LoadedFallback};
use crate::i18n::{self, LANGUAGES, Texts};
use crate::measure::Probe;
use crate::navigation::{self, History, Place};
use crate::outline::{self, Outline, SymbolList};
use crate::quick_open::{IndexEvent, IndexWorker, PaletteAction, PaletteCommand, QuickOpen};
use crate::search_panel::{OpenMatch, ReplacePlan, SearchAction, SearchPanel, replace_in_document};
use crate::session::{self, SessionKeeper};
use crate::symbol_index::{self, SymbolCommand, SymbolEvent, SymbolWorker, UnsavedTexts};
use crate::tabs::{self, Loaded, Loading, Tab, TabAction, TabHighlight};
use crate::tree::{self, ProjectTree, TreeAction};
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

/// Something picked in a menu that is not a plain command.
#[derive(Debug, Clone)]
enum MenuAction {
    Appearance(Appearance),
    EditorTheme(String),
    /// A recent file, or a recent folder to open as the project.
    OpenRecent(PathBuf),
    ClearRecent,
}

/// Recently opened files and folders, and the thread that writes them to disk (one writer, so
/// two saves never overlap).
struct RecentList {
    list: Recent,
    writer: Sender<Recent>,
}

impl RecentList {
    fn start(file: PathBuf) -> Self {
        let list = Recent::load(&file);
        let (writer, rx) = mpsc::channel::<Recent>();
        thread::spawn(move || {
            while let Ok(first) = rx.recv() {
                let newest = rx.try_iter().last().unwrap_or(first);
                let _ = newest.save(&file);
            }
        });
        Self { list, writer }
    }

    /// Applies `change` and saves if it changed anything.
    fn update(&mut self, change: impl FnOnce(&mut Recent) -> bool) {
        if change(&mut self.list) {
            let _ = self.writer.send(self.list.clone());
        }
    }
}

/// The watcher of the project folder and the changes it reports.
struct ProjectWatch {
    _watcher: ProjectWatcher,
    changes: Receiver<ProjectChanges>,
}

impl ProjectWatch {
    /// `None` if the folder cannot be watched (some network drives); the tree then lists
    /// folders again whenever they are reopened.
    fn start(ctx: &egui::Context, root: &std::path::Path) -> Option<Self> {
        let (tx, changes) = mpsc::channel();
        let ctx = ctx.clone();
        let watcher = ProjectWatcher::new(root, move |batch| {
            let _ = tx.send(batch);
            ctx.request_repaint();
        })
        .ok()?;
        Some(Self {
            _watcher: watcher,
            changes,
        })
    }
}

/// A go to definition waiting for the symbol index, or for the document's outline.
struct PendingDefinition {
    tab: u64,
    name: String,
    generation: u64,
    /// Looked up in the document's own symbols instead of the project's.
    local: bool,
}

/// What happened to each closed file of a "Replace all" in the folder.
type FileOutcomes = Vec<(PathBuf, FileOutcome)>;

/// What "Replace all" in the folder did, for the status bar.
#[derive(Debug, Default)]
struct FolderReplaceReport {
    matches: usize,
    files: usize,
    skipped: usize,
    /// The first file that could not be written, and why.
    failed: Option<(String, String)>,
}

/// What the side panel shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum SidebarMode {
    #[default]
    Tree,
    Search,
    /// The active document's symbols; the only mode without a project.
    Outline,
}

/// What waits for the user to deal with unsaved changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingAction {
    /// Close the tab with this id.
    CloseTab(u64),
    CloseWindow,
}

pub struct BracketApp {
    texts: &'static Texts,
    config: Config,
    /// Folder open as a project.
    project: Option<PathBuf>,
    /// The tree of the project folder, in the side panel.
    tree: Option<ProjectTree>,
    /// Reports what is created, deleted or renamed in the project folder.
    project_watch: Option<ProjectWatch>,
    /// The project's file index, on its own thread, and how many files it holds once built.
    index: Option<IndexWorker>,
    /// A project whose index starts once the first frame is on screen: walking a large project
    /// on every core would otherwise slow startup down.
    index_pending: Option<(PathBuf, WalkOptions)>,
    indexed: Option<usize>,
    /// The project's symbol index, fed by the file index.
    symbol_index: Option<SymbolWorker>,
    /// The number of symbols in it, once it is up to date.
    symbols_indexed: Option<usize>,
    unsaved_texts: UnsavedTexts,
    /// Places to go back and forward to.
    history: History,
    /// A go to definition waiting for its answer.
    pending_definition: Option<PendingDefinition>,
    definition_generation: u64,
    /// The Ctrl+P palette, while it is open.
    quick_open: Option<QuickOpen>,
    /// The find bar above the editor (Ctrl+F).
    find: FindBar,
    /// Find in folder, in the side panel (Ctrl+Shift+F).
    search: SearchPanel,
    sidebar_mode: SidebarMode,
    /// The active document's outline, in the side panel.
    outline: Outline,
    /// The `symbols.toml` rules of language packages, by language id.
    symbol_rules: HashMap<String, Arc<RuleSet>>,
    /// "Replace all" in the folder, waiting for the user to confirm it.
    pending_folder_replace: Option<ReplacePlan>,
    /// Closed files being rewritten on a worker thread, and what was done in open tabs.
    folder_replace: Option<(Receiver<FileOutcomes>, FolderReplaceReport)>,
    /// Creating, renaming and deleting from the tree, on worker threads.
    file_ops: FileOps,
    /// An entry waiting for the user to confirm it goes to the Recycle Bin.
    pending_trash: Option<PathBuf>,
    /// The side panel's visibility and width (its open folders live in `tree`).
    sidebar: Sidebar,
    /// `None` while measuring: recent items are neither read nor written.
    recent: Option<RecentList>,
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
            recent_file,
        } = launch;
        let bracket_syntax::Packages {
            languages: package_languages,
            grammars,
            warnings: package_warnings,
        } = languages;
        bracket_syntax::highlight::set_external_grammars(grammars);
        let languages = LanguageRegistry::with_packages(package_languages);
        let (symbol_rules, rule_warnings) = outline::package_rules(languages.languages());
        let mut package_warnings = package_warnings;
        package_warnings.extend(rule_warnings);
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
            project: None,
            tree: None,
            project_watch: None,
            index: None,
            index_pending: None,
            indexed: None,
            symbol_index: None,
            symbols_indexed: None,
            unsaved_texts: UnsavedTexts::default(),
            history: History::default(),
            pending_definition: None,
            definition_generation: 0,
            quick_open: None,
            find: FindBar::default(),
            search: SearchPanel::default(),
            sidebar_mode: SidebarMode::Tree,
            outline: Outline::default(),
            symbol_rules,
            pending_folder_replace: None,
            folder_replace: None,
            file_ops: FileOps::new(),
            pending_trash: None,
            sidebar: Sidebar::default(),
            recent: recent_file.map(RecentList::start),
            tabs: Vec::new(),
            active: 0,
            next_tab_id: 0,
            languages,
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
            self.tabs[index].preview = entry.preview;
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
        let Sidebar {
            visible,
            width,
            expanded,
        } = previous.sidebar;
        self.sidebar = Sidebar {
            visible,
            width,
            expanded: Vec::new(),
        };
        // A project folder that was deleted or moved is simply not reopened.
        if let Some(project) = previous.project.filter(|p| p.is_dir()) {
            self.set_project(ctx, Some(project));
            if let Some(tree) = &mut self.tree {
                tree.expand(expanded);
            }
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
        keeper.set_sidebar(Sidebar {
            expanded: self
                .tree
                .as_ref()
                .map(ProjectTree::expanded_for_session)
                .unwrap_or_default(),
            ..self.sidebar.clone()
        });
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
        if path.is_dir() {
            self.set_project(ctx, Some(path));
            return;
        }
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

    /// Opens a file in a normal tab: see [`Self::open_file_as`].
    fn open_file(&mut self, ctx: &egui::Context, path: PathBuf) {
        self.open_file_as(ctx, path, false);
    }

    /// Opens a file in a tab: the one already showing it, the active one if it is blank, the
    /// preview tab when previewing, or a new one. Opening a previewed file normally keeps its
    /// tab.
    fn open_file_as(&mut self, ctx: &egui::Context, path: PathBuf, preview: bool) {
        let open = self.tabs.iter().position(|tab| {
            tab.path()
                .is_some_and(|tab_path| tabs::same_file(tab_path, &path))
        });
        if let Some(index) = open {
            if !preview {
                self.tabs[index].preview = false;
            }
            self.activate(ctx, index);
            return;
        }
        let replaced = preview
            .then(|| {
                self.tabs
                    .iter()
                    .position(|t| t.preview && !t.doc.is_dirty())
            })
            .flatten();
        let (index, new_tab) = if self.active_tab().is_blank() {
            (self.active, false)
        } else if let Some(index) = replaced {
            // A new tab in the preview tab's place.
            let doc = self.empty_document();
            self.add_tab(doc);
            let tab = self.tabs.pop().expect("just added");
            self.tabs[index] = tab;
            (index, true)
        } else {
            let doc = self.empty_document();
            (self.add_tab(doc), true)
        };
        self.start_loading(ctx, index, path, None, new_tab);
        self.tabs[index].preview = preview;
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
        let mut opened = Vec::new();
        let mut unreadable = Vec::new();
        let mut loaded_active = false;
        for (index, tab) in self.tabs.iter_mut().enumerate() {
            let Some(loading) = &tab.loading else {
                continue;
            };
            let new_tab = loading.new_tab;
            let read_encoding = loading.read_encoding;
            let top_line = loading.restore.as_ref().map(|entry| entry.top_line);
            let goto = loading.goto;
            let restored = loading.restore.is_some();
            let requested_path = loading.path.clone();
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
                    if !restored && let Some(path) = tab.doc.path() {
                        opened.push(path.to_path_buf());
                    }
                }
                Ok(Err(err)) => {
                    self.status = Some(self.texts.open_failed(&err.to_string()));
                    tab.loading = None;
                    if !restored && let Some(path) = requested_path {
                        unreadable.push(path);
                    }
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
        if let Some(recent) = &mut self.recent {
            for path in opened {
                recent.update(|r| r.add_file(&path));
            }
            for path in unreadable {
                recent.update(|r| r.remove(&path));
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
            if let Some(recent) = &mut self.recent {
                recent.update(|r| r.add_file(&path));
            }
            let tab = &mut self.tabs[index];
            tab.preview = false;
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
            Command::OpenFolder => self.open_folder_with_dialog(ctx, frame),
            Command::CloseFolder => self.set_project(ctx, None),
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
            Command::ToggleSidebar => {
                self.sidebar.visible = !self.sidebar.visible;
                // Without a project, the outline is all the panel can show.
                if self.sidebar.visible && self.project.is_none() {
                    self.sidebar_mode = SidebarMode::Outline;
                }
            }
            Command::ShowOutline => {
                self.sidebar.visible = true;
                self.sidebar_mode = SidebarMode::Outline;
            }
            Command::FocusTree => {
                if let Some(tree) = &mut self.tree {
                    self.sidebar.visible = true;
                    self.sidebar_mode = SidebarMode::Tree;
                    tree.focus(ctx);
                    return;
                }
            }
            Command::FindInFolder => {
                if self.project.is_some() {
                    self.sidebar.visible = true;
                    self.sidebar_mode = SidebarMode::Search;
                    self.search.focus();
                    return;
                }
            }
            Command::ReplaceInFolder => {
                if self.project.is_some() {
                    self.sidebar.visible = true;
                    self.sidebar_mode = SidebarMode::Search;
                    self.search.open_replace();
                    return;
                }
            }
            Command::Find => return self.open_find(false),
            Command::Replace => return self.open_find(true),
            Command::FindNext => return self.find_step(ctx, true),
            Command::FindPrevious => return self.find_step(ctx, false),
            Command::QuickOpen => return self.open_palette(""),
            Command::Palette => return self.open_palette(">"),
            Command::GoToLine => return self.open_palette(":"),
            Command::GoToSymbol => return self.open_palette("@"),
            Command::GoToProjectSymbol => return self.open_palette("#"),
            Command::GoToDefinition => self.go_to_definition(),
            Command::NavigateBack => self.navigate(ctx, true),
            Command::NavigateForward => self.navigate(ctx, false),
            // Closing goes through the unsaved-changes check in handle_window_events.
            Command::Exit => ctx.send_viewport_cmd(ViewportCommand::Close),
        }
        self.focus_editor(ctx);
    }

    /// Ctrl+F (or Ctrl+H with the replace row): a short selection on one line is what to look
    /// for.
    fn open_find(&mut self, replace: bool) {
        let doc = &self.active_tab().doc;
        let selection = doc.buffer.selections().primary();
        let selected = (!selection.is_caret())
            .then(|| {
                doc.buffer
                    .text()
                    .slice(selection.start()..selection.end())
                    .to_string()
            })
            .filter(|text| !text.contains('\n') && text.chars().count() <= 200);
        self.find.open(selected, selection.start(), replace);
    }

    /// Replaces the selected match, or every match, in the active tab.
    fn replace(&mut self, all: bool) {
        let tab = &mut self.tabs[self.active];
        let outcome = if all {
            self.find.replace_all(&mut tab.doc)
        } else {
            self.find.replace_one(tab.id, &mut tab.doc)
        };
        match outcome {
            ReplaceOutcome::Select(range) => self.select_match(range),
            ReplaceOutcome::All(count) => {
                self.status = Some(self.texts.replaced_count(&count.to_string()));
            }
            ReplaceOutcome::ReadOnly => {
                self.status = Some(self.texts.replace_read_only().to_owned());
            }
            ReplaceOutcome::Replaced | ReplaceOutcome::Nothing => {}
        }
    }

    /// F3 / Shift+F3: the next or previous match. With the bar closed, it opens and the first
    /// match after the caret is selected once found.
    fn find_step(&mut self, ctx: &egui::Context, forward: bool) {
        let index = self.active;
        let tab = &self.tabs[index];
        if !self.find.is_open() || !self.find.has_query() {
            let caret = tab.doc.buffer.selections().primary().start();
            self.find.open(None, caret, false);
            return;
        }
        if let Some(range) = self.find.step(tab.id, &tab.doc, forward) {
            self.select_match(range);
        }
        let _ = ctx;
    }

    /// Selects a match in the active tab and scrolls to it.
    fn select_match(&mut self, range: std::ops::Range<usize>) {
        let tab = &mut self.tabs[self.active];
        tab.doc
            .buffer
            .set_selections(Selections::single(Selection::new(range.start, range.end)));
        tab.view.request_scroll_to_caret();
    }

    /// Where the caret is now, for the navigation history.
    fn current_place(&self) -> Option<Place> {
        let tab = self.active_tab();
        if tab.loading.is_some() {
            return None;
        }
        let text = tab.doc.buffer.text();
        let head = tab.doc.buffer.selections().primary().head;
        let line = text.char_to_line(head);
        Some(Place {
            tab: tab.id,
            path: tab.path().map(Path::to_path_buf),
            line: line + 1,
            column: head - text.line_to_char(line) + 1,
        })
    }

    /// Records where the caret is before a jump.
    fn remember(&mut self) {
        if let Some(place) = self.current_place() {
            self.history.leave(place);
        }
    }

    /// Alt+← and Alt+→.
    fn navigate(&mut self, ctx: &egui::Context, back: bool) {
        let Some(current) = self.current_place() else {
            return;
        };
        let place = match back {
            true => self.history.back(current),
            false => self.history.forward(current),
        };
        let Some(place) = place else { return };
        let position = (place.line, place.column);
        if let Some(index) = self.tab_index(place.tab) {
            self.activate(ctx, index);
            let tab = &mut self.tabs[index];
            match &mut tab.loading {
                Some(loading) => loading.goto = Some(position),
                None => go_to(tab, position),
            }
        } else if let Some(path) = place.path {
            // The tab was closed: the file opens again.
            self.open_request(
                ctx,
                OpenRequest {
                    path,
                    line: Some(place.line),
                    column: Some(place.column),
                },
            );
        }
    }

    /// F12 or Ctrl+click: looks up the name at the caret in the project's symbol index, or in
    /// the document's own symbols when it is not a project file.
    fn go_to_definition(&mut self) {
        let tab = &self.tabs[self.active];
        if tab.loading.is_some() {
            return;
        }
        let caret = tab.doc.buffer.selections().primary().head;
        let language = tab.language.as_deref();
        let Some((name, component)) = navigation::name_at(tab.doc.buffer.text(), caret, language)
        else {
            self.status = Some(self.texts.definition_no_name().to_owned());
            return;
        };
        self.definition_generation += 1;
        let path = tab.path().map(Path::to_path_buf);
        let index = self.symbol_index.as_ref().filter(|_| {
            let root = self.project.as_deref();
            path.as_deref()
                .zip(root)
                .is_some_and(|(p, r)| p.starts_with(r))
        });
        if let Some(index) = index {
            index.definitions(name.clone(), path, component, self.definition_generation);
        }
        self.pending_definition = Some(PendingDefinition {
            tab: tab.id,
            name,
            generation: self.definition_generation,
            local: index.is_none(),
        });
    }

    /// The symbol index's answer to a go to definition: one place is gone to, several are
    /// offered in the palette, and none sends the search to the document's own symbols (it may
    /// have unsaved ones the index does not have yet).
    fn definitions_found(&mut self, ctx: &egui::Context, generation: u64, found: Vec<FoundSymbol>) {
        let active = self.tabs[self.active].id;
        let Some(mut pending) = self
            .pending_definition
            .take()
            .filter(|p| p.generation == generation && p.tab == active)
        else {
            return;
        };
        let Some(root) = self.project.clone() else {
            return;
        };
        match found.as_slice() {
            [] => {
                pending.local = true;
                self.pending_definition = Some(pending);
            }
            [one] => {
                self.remember();
                self.open_request(
                    ctx,
                    OpenRequest {
                        path: root.join(&one.relative),
                        line: Some(one.line as usize + 1),
                        column: Some(one.column as usize + 1),
                    },
                );
                self.focus_editor(ctx);
            }
            _ => self.quick_open = Some(QuickOpen::with_found(format!("#{}", pending.name), found)),
        }
    }

    /// A go to definition in the document's own symbols, once its outline is ready.
    fn local_definition(&mut self, ctx: &egui::Context) {
        let tab = &self.tabs[self.active];
        let Some(pending) = self
            .pending_definition
            .as_ref()
            .filter(|p| p.local && p.tab == tab.id)
        else {
            return;
        };
        let name = pending.name.clone();
        let ranges: Vec<std::ops::Range<usize>> = match self.outline.palette_list(tab.id, &tab.doc)
        {
            SymbolList::Pending => return,
            SymbolList::Ready { symbols, .. } => {
                let exact: Vec<_> = symbols.iter().filter(|s| s.name == name).collect();
                let chosen = match exact.is_empty() {
                    true => symbols
                        .iter()
                        .filter(|s| s.name.to_lowercase() == name.to_lowercase())
                        .collect(),
                    false => exact,
                };
                chosen.iter().map(|s| s.name_range.clone()).collect()
            }
            SymbolList::Unsupported | SymbolList::TooLarge => Vec::new(),
        };
        self.pending_definition = None;
        match ranges.as_slice() {
            [] => self.status = Some(self.texts.definition_none(&name)),
            [range] => {
                self.remember();
                self.select_match(range.clone());
                self.focus_editor(ctx);
            }
            _ => self.open_palette(&format!("@{name}")),
        }
    }

    /// Starts building the project's file index, once the first frame has been presented.
    fn start_index(&mut self, ctx: &egui::Context) {
        if self.index_pending.is_none() {
            return;
        }
        if ctx.cumulative_frame_nr() == 0 {
            ctx.request_repaint();
            return;
        }
        let (root, options) = self.index_pending.take().expect("checked above");
        let repaint = ctx.clone();
        let symbols = SymbolWorker::start(
            root.clone(),
            symbol_index::resolver(&self.languages, &self.symbol_rules),
            move || repaint.request_repaint(),
        );
        let to_symbols = symbols.sender();
        let repaint = ctx.clone();
        self.index = Some(IndexWorker::start(
            root,
            options,
            move || repaint.request_repaint(),
            move |change| {
                let _ = to_symbols.send(SymbolCommand::Files(change));
            },
        ));
        self.symbol_index = Some(symbols);
    }

    /// Opens the palette: for files, for commands (`>`) or for a line (`:`).
    fn open_palette(&mut self, prefix: &str) {
        let recent = self
            .recent
            .as_ref()
            .map(|r| r.list.files.clone())
            .unwrap_or_default();
        self.quick_open = Some(QuickOpen::new(recent, prefix));
    }

    /// What decides which commands are available now.
    fn command_state(&self) -> commands::State {
        commands::State {
            has_project: self.project.is_some(),
            active_loaded: self.active_tab().loading.is_none(),
            tabs: self.tabs.len(),
            can_go_back: self.history.can_go_back(),
            can_go_forward: self.history.can_go_forward(),
        }
    }

    /// Opens `folder` as the project, or closes the project with `None`. Open tabs stay.
    fn set_project(&mut self, ctx: &egui::Context, folder: Option<PathBuf>) {
        let folder = folder.map(|f| std::path::absolute(&f).unwrap_or(f));
        let options = WalkOptions {
            exclude: self.config.project_exclude.clone(),
            ..WalkOptions::default()
        };
        self.tree = folder.as_ref().map(|root| {
            let ctx = ctx.clone();
            ProjectTree::new(root.clone(), options.clone(), move || ctx.request_repaint())
        });
        self.index = None;
        self.index_pending = folder.as_ref().map(|root| (root.clone(), options.clone()));
        self.indexed = None;
        self.symbol_index = None;
        self.symbols_indexed = None;
        self.unsaved_texts = UnsavedTexts::default();
        self.history.clear();
        self.pending_definition = None;
        self.start_index(ctx);
        self.search.reset();
        self.project_watch = folder
            .as_ref()
            .and_then(|root| ProjectWatch::start(ctx, root));
        if let Some(tree) = &mut self.tree {
            tree.set_watched(self.project_watch.is_some());
        }
        if let Some(keeper) = &mut self.session {
            keeper.set_project(folder.clone());
        }
        if let (Some(recent), Some(folder)) = (&mut self.recent, &folder) {
            recent.update(|r| r.add_folder(folder));
        }
        self.project = folder;
    }

    fn open_folder_with_dialog(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        let title = self.texts.dialog_open_folder_title();
        if let Some(folder) = dialogs::pick_folder(frame, title, self.project.as_deref()) {
            self.set_project(ctx, Some(folder));
        }
    }

    fn open_with_dialog(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        // The active file's folder, or else the project.
        let start_dir = self
            .active_tab()
            .path()
            .and_then(|p| p.parent())
            .map(PathBuf::from)
            .or_else(|| self.project.clone());
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
            if let Some(recent) = &mut self.recent {
                recent.update(|r| r.add_file(&path));
            }
            let tab = &mut self.tabs[index];
            tab.preview = false;
            tab.doc.set_path(path);
            tab.refresh_language(&self.languages);
            // The language may have changed: highlighting starts again on the next frame.
            tab.highlight = None;
            self.start_saving(ctx, index);
        }
    }

    /// Draws the menu bar. Returns the command picked, if any.
    fn menu_bar(&self, ui: &mut egui::Ui) -> (Option<Command>, Option<MenuAction>) {
        let mut picked = None;
        let mut menu_pick = None;
        let theme_names = highlight::theme_names();
        let state = self.command_state();
        let texts = self.texts;
        let sidebar_shown = self.project.is_some() && self.sidebar.visible;
        // A menu entry for a command: its text and shortcut, disabled when not available.
        let mut entry = |ui: &mut egui::Ui, command: Command| {
            let selected = command == Command::ToggleSidebar && sidebar_shown;
            let mut button = egui::Button::new(command.label(texts)).selected(selected);
            if let Some(shortcut) = command.shortcut() {
                button = button.shortcut_text(ui.ctx().format_shortcut(&shortcut));
            }
            if ui
                .add_enabled(command.is_available(&state), button)
                .clicked()
            {
                picked = Some(command);
                ui.close();
            }
        };
        egui::Panel::top("menu").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button(texts.menu_file(), |ui| {
                    for command in Command::in_menu(Menu::File) {
                        if command == Command::Exit {
                            ui.separator();
                        }
                        entry(ui, command);
                        if command == Command::OpenFolder
                            && let Some(recent) = &self.recent
                        {
                            ui.menu_button(self.texts.menu_recent(), |ui| {
                                if let Some(action) = recent_menu(ui, &recent.list, self.texts) {
                                    menu_pick = Some(action);
                                    ui.close();
                                }
                            });
                        }
                    }
                });
                ui.menu_button(texts.menu_edit(), |ui| {
                    for command in Command::in_menu(Menu::Edit) {
                        entry(ui, command);
                    }
                });
                ui.menu_button(texts.menu_view(), |ui| {
                    for command in Command::in_menu(Menu::View) {
                        entry(ui, command);
                    }
                    ui.separator();
                    ui.menu_button(self.texts.menu_appearance(), |ui| {
                        for appearance in Appearance::ALL {
                            let label = self.texts.appearance(appearance);
                            let current = appearance == self.config.appearance;
                            if ui.selectable_label(current, label).clicked() {
                                menu_pick = Some(MenuAction::Appearance(appearance));
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
                            menu_pick = Some(MenuAction::EditorTheme(AUTO_THEME.to_owned()));
                            ui.close();
                        }
                        ui.separator();
                        for name in &theme_names {
                            let current = self.config.editor_theme == *name;
                            if ui.selectable_label(current, *name).clicked() {
                                menu_pick = Some(MenuAction::EditorTheme((*name).to_owned()));
                                ui.close();
                            }
                        }
                    });
                });
                ui.menu_button(texts.menu_go(), |ui| {
                    for command in Command::in_menu(Menu::Go) {
                        entry(ui, command);
                    }
                });
            });
        });
        (picked, menu_pick)
    }

    /// Draws the side panel with the project tree, if there is a project and the panel is not
    /// hidden. Remembers its width.
    /// Draws the side panel: the project tree, folder search or the outline. Without a project
    /// only the outline, and only when asked for. Remembers its width.
    fn side_panel(
        &mut self,
        ui: &mut egui::Ui,
    ) -> (
        Option<TreeAction>,
        Option<SearchAction>,
        Option<std::ops::Range<usize>>,
    ) {
        let project = self.tree.is_some() && self.project.is_some();
        if !self.sidebar.visible || (!project && self.sidebar_mode != SidebarMode::Outline) {
            return (None, None, None);
        }
        let (languages, texts) = (&self.languages, self.texts);
        let exclude = &self.config.project_exclude;
        let root = self.project.as_deref();
        let (mode, search, tree, outline) = (
            &mut self.sidebar_mode,
            &mut self.search,
            &mut self.tree,
            &mut self.outline,
        );
        let tab = &self.tabs[self.active];
        let frame =
            egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin::symmetric(0, 6));
        let panel = egui::Panel::left("project-panel")
            .resizable(true)
            .frame(frame)
            .default_size(self.sidebar.width as f32)
            .size_range(Sidebar::MIN_WIDTH as f32..=Sidebar::MAX_WIDTH as f32)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(6.0);
                    if project {
                        if ui
                            .selectable_label(*mode == SidebarMode::Tree, texts.sidebar_explorer())
                            .clicked()
                        {
                            *mode = SidebarMode::Tree;
                        }
                        if ui
                            .selectable_label(*mode == SidebarMode::Search, texts.sidebar_search())
                            .clicked()
                        {
                            *mode = SidebarMode::Search;
                            search.focus();
                        }
                    }
                    if ui
                        .selectable_label(*mode == SidebarMode::Outline, texts.sidebar_outline())
                        .clicked()
                    {
                        *mode = SidebarMode::Outline;
                    }
                });
                ui.separator();
                match (*mode, tree.as_mut(), root) {
                    (SidebarMode::Tree, Some(tree), Some(_)) => {
                        (tree.show(ui, languages, texts), None, None)
                    }
                    (SidebarMode::Search, Some(_), Some(root)) => {
                        (None, search.show(ui, root, exclude, languages, texts), None)
                    }
                    _ if tab.loading.is_none() => {
                        (None, None, outline.show(ui, tab.id, &tab.doc, texts))
                    }
                    _ => (None, None, None),
                }
            });
        self.sidebar.width = panel.response.rect.width().round() as u32;
        panel.inner
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
            if path.is_dir() {
                self.set_project(ctx, Some(path));
            } else {
                self.open_file(ctx, path);
            }
        }
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let tab = self.active_tab();
        let title = window_title(
            &tab.title(self.texts),
            tab.doc.is_dirty(),
            self.project.as_deref(),
        );
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

    fn run_menu_action(&mut self, ctx: &egui::Context, action: MenuAction) {
        match action {
            MenuAction::Appearance(appearance) => {
                if appearance == self.config.appearance {
                    return;
                }
                self.config.appearance = appearance;
                ctx.set_theme(theme_preference(appearance));
            }
            MenuAction::EditorTheme(name) => {
                if name == self.config.editor_theme {
                    return;
                }
                self.config.editor_theme = name;
            }
            MenuAction::OpenRecent(path) => {
                if path.is_dir() {
                    self.set_project(ctx, Some(path));
                } else {
                    self.open_file(ctx, path);
                }
                return;
            }
            MenuAction::ClearRecent => {
                if let Some(recent) = &mut self.recent {
                    recent.update(|r| {
                        r.clear();
                        true
                    });
                }
                return;
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
            find_match: colors.find_highlight.map_or(base.find_match, color32),
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

    /// Draws the Ctrl+P palette, if open, and carries out what is picked in it.
    fn quick_open_palette(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        if self.quick_open.is_none() {
            return;
        }
        let state = self.command_state();
        let texts = self.texts;
        let commands: Vec<PaletteCommand> = Command::ALL
            .into_iter()
            .filter(|c| *c != Command::Palette && c.is_available(&state))
            .map(|command| PaletteCommand {
                command,
                label: command.palette_label(texts),
                id: command.id(),
                shortcut: command.shortcut().map(|s| ctx.format_shortcut(&s)),
            })
            .collect();
        let active = &self.tabs[self.active];
        let loaded = active.loading.is_none();
        let lines = loaded.then(|| active.doc.buffer.text().len_lines());
        let symbols = if loaded {
            self.outline.palette_list(active.id, &active.doc)
        } else {
            SymbolList::Pending
        };
        let Some(palette) = &mut self.quick_open else {
            return;
        };
        let action = palette.show(
            ctx,
            self.project.as_deref(),
            self.index.as_ref(),
            self.indexed,
            self.symbol_index.as_ref(),
            self.symbols_indexed,
            &commands,
            lines,
            symbols,
            &self.languages,
            self.texts,
        );
        let Some(action) = action else { return };
        self.quick_open = None;
        if !matches!(action, PaletteAction::Run(_) | PaletteAction::Close) {
            self.remember();
        }
        match action {
            PaletteAction::Open { path, line } => {
                self.open_request(
                    ctx,
                    OpenRequest {
                        path,
                        line: line.map(|(line, _)| line),
                        column: line.and_then(|(_, column)| column),
                    },
                );
            }
            PaletteAction::GoToLine((line, column)) => {
                let tab = &mut self.tabs[self.active];
                if tab.loading.is_none() {
                    go_to(tab, (line, column.unwrap_or(1)));
                }
            }
            PaletteAction::GoToSymbol {
                path,
                position: (line, column),
            } => {
                self.open_request(
                    ctx,
                    OpenRequest {
                        path,
                        line: Some(line),
                        column: Some(column),
                    },
                );
            }
            PaletteAction::GoTo(range) => {
                if self.tabs[self.active].loading.is_none() {
                    self.select_match(range);
                }
            }
            PaletteAction::Run(command) => return self.run_command(ctx, frame, command),
            PaletteAction::Close => {}
        }
        self.focus_editor(ctx);
    }

    /// Asks before "Replace all" in the folder, which saves closed files at once.
    fn folder_replace_dialog(&mut self, ctx: &egui::Context) {
        let Some(plan) = &self.pending_folder_replace else {
            return;
        };
        let texts = self.texts;
        let message =
            texts.folder_replace_message(&plan.matches.to_string(), &plan.files.len().to_string());
        let mut choice = None;
        let modal = egui::Modal::new(Id::new("folder-replace-confirm")).show(ctx, |ui| {
            ui.set_max_width(460.0);
            ui.heading(texts.folder_replace_title());
            ui.label(message);
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button(texts.replace_all()).clicked() {
                    choice = Some(true);
                }
                if ui.button(texts.cancel()).clicked() {
                    choice = Some(false);
                }
            });
        });
        if modal.should_close() && choice.is_none() {
            choice = Some(false);
        }
        let Some(confirmed) = choice else { return };
        let plan = self.pending_folder_replace.take().expect("checked above");
        if confirmed {
            self.replace_in_folder(ctx, plan);
        }
    }

    /// Carries out "Replace all" in the folder: open tabs change in their buffer (undoable, not
    /// saved), closed files are rewritten on a worker thread.
    fn replace_in_folder(&mut self, ctx: &egui::Context, plan: ReplacePlan) {
        let mut report = FolderReplaceReport::default();
        let mut closed = Vec::new();
        for file in plan.files {
            let open = self.tabs.iter().position(|t| {
                t.unloaded.is_none() && t.path().is_some_and(|p| tabs::same_file(p, &file.path))
            });
            let Some(index) = open else {
                closed.push(file);
                continue;
            };
            let tab = &mut self.tabs[index];
            let replaced = if tab.loading.is_some() {
                None
            } else {
                replace_in_document(&mut tab.doc, &plan.finder, &plan.template, &file)
            };
            match replaced {
                Some(count) => {
                    report.matches += count;
                    report.files += 1;
                }
                None => report.skipped += 1,
            }
        }
        if closed.is_empty() {
            self.finish_folder_replace(report);
            return;
        }
        let (tx, rx) = mpsc::channel();
        let replacer = Arc::clone(&self.replacer);
        let ctx = ctx.clone();
        let (finder, template) = (plan.finder, plan.template);
        thread::spawn(move || {
            let outcomes = closed
                .into_iter()
                .map(|file| {
                    let outcome = replace_in_file(&finder, &template, &file, replacer.as_ref());
                    (file.path, outcome)
                })
                .collect();
            let _ = tx.send(outcomes);
            ctx.request_repaint();
        });
        self.folder_replace = Some((rx, report));
    }

    fn poll_folder_replace(&mut self) {
        let Some((rx, _)) = &self.folder_replace else {
            return;
        };
        let Ok(outcomes) = rx.try_recv() else { return };
        let (_, mut report) = self.folder_replace.take().expect("checked above");
        for (path, outcome) in outcomes {
            match outcome {
                FileOutcome::Replaced(count) => {
                    report.matches += count;
                    report.files += 1;
                }
                FileOutcome::Failed(error) => {
                    report
                        .failed
                        .get_or_insert((tree::display_name(&path), error));
                }
                FileOutcome::ReadOnly | FileOutcome::Inexact | FileOutcome::Changed => {
                    report.skipped += 1;
                }
            }
        }
        self.finish_folder_replace(report);
    }

    /// Says what "Replace all" did and searches again.
    fn finish_folder_replace(&mut self, report: FolderReplaceReport) {
        let texts = self.texts;
        let mut message =
            texts.folder_replaced(&report.matches.to_string(), &report.files.to_string());
        if report.skipped > 0 {
            message.push_str(" · ");
            message.push_str(&texts.folder_replace_skipped(&report.skipped.to_string()));
        }
        if let Some((name, error)) = &report.failed {
            message.push_str(" · ");
            message.push_str(&texts.folder_replace_failed(name, error));
        }
        self.status = Some(message);
        self.search.refresh();
    }

    /// Asks before moving an entry of the tree to the Recycle Bin.
    fn trash_dialog(&mut self, ctx: &egui::Context) {
        let Some(path) = &self.pending_trash else {
            return;
        };
        let texts = self.texts;
        let name = tree::display_name(path);
        let message = if path.is_dir() {
            texts.trash_folder_message(&name)
        } else {
            texts.trash_file_message(&name)
        };
        let mut choice = None;
        let modal = egui::Modal::new(Id::new("trash-confirm")).show(ctx, |ui| {
            ui.heading(texts.trash_title());
            ui.label(message);
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button(texts.trash_confirm()).clicked() {
                    choice = Some(true);
                }
                if ui.button(texts.cancel()).clicked() {
                    choice = Some(false);
                }
            });
        });
        if modal.should_close() && choice.is_none() {
            choice = Some(false);
        }
        let Some(confirmed) = choice else { return };
        let path = self.pending_trash.take().expect("checked above");
        if confirmed {
            self.file_ops.start(ctx, FileOp::Trash(path));
        }
        if let Some(tree) = &mut self.tree {
            tree.focus(ctx);
        }
    }

    /// Deals with the tree's file operations that have finished.
    fn finish_file_ops(&mut self, ctx: &egui::Context) {
        for (op, result) in self.file_ops.finished() {
            let subject = op.result_path().unwrap_or(op.subject()).to_path_buf();
            let name = tree::display_name(&subject);
            match result {
                Err(OpError::Exists) => self.status = Some(self.texts.name_exists(&name)),
                Err(OpError::Other(error)) => {
                    let name = tree::display_name(op.subject());
                    self.status = Some(self.texts.file_op_failed(&name, &error));
                }
                Ok(()) => {
                    if let Some(tree) = &mut self.tree {
                        tree.refresh_parent_of(op.subject());
                        if let Some(path) = op.result_path() {
                            tree.refresh_parent_of(path);
                            tree.select_path(path.to_path_buf());
                        }
                    }
                    match &op {
                        FileOp::CreateFile(path) => self.open_file(ctx, path.clone()),
                        FileOp::Rename { from, to } => self.follow_rename(from, to),
                        FileOp::Trash(path) => {
                            self.status = Some(self.texts.trashed(&tree::display_name(path)));
                        }
                        FileOp::CreateFolder(_) => {}
                    }
                }
            }
        }
    }

    /// Tabs showing a renamed file, or a file inside a renamed folder, follow it.
    fn follow_rename(&mut self, from: &std::path::Path, to: &std::path::Path) {
        for tab in &mut self.tabs {
            if let Some(loading) = &mut tab.loading
                && let Some(path) = &mut loading.path
                && let Some(moved) = moved_path(path, from, to)
            {
                *path = moved;
            }
            if let Some(entry) = &mut tab.unloaded
                && let Some(path) = &mut entry.path
                && let Some(moved) = moved_path(path, from, to)
            {
                *path = moved;
            }
            if let Some(moved) = tab.doc.path().and_then(|p| moved_path(p, from, to)) {
                tab.doc.set_path(moved);
            } else if tab.loading.is_none() && tab.unloaded.is_none() {
                continue;
            }
            tab.refresh_language(&self.languages);
            // The language may have changed: highlighting starts again on the next frame.
            tab.highlight = None;
        }
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
        self.start_index(ctx);
        if let Some(watch) = &self.project_watch {
            while let Ok(changes) = watch.changes.try_recv() {
                if let Some(tree) = &mut self.tree
                    && changes.changes_listing()
                {
                    tree.apply_changes(&changes);
                }
                if let Some(index) = &self.index {
                    index.changes(changes);
                }
            }
        }
        let mut definitions = None;
        if let Some(symbols) = &self.symbol_index {
            for event in symbols.events() {
                match event {
                    SymbolEvent::Ready(count) => self.symbols_indexed = Some(count),
                    SymbolEvent::Results { generation, found } => {
                        if let Some(palette) = &mut self.quick_open {
                            palette.symbol_results(generation, found);
                        }
                    }
                    SymbolEvent::Definitions { generation, found } => {
                        definitions = Some((generation, found));
                    }
                }
            }
            let documents = self.tabs.iter().filter(|t| t.loading.is_none());
            let documents = documents.filter_map(|t| Some((t.path()?, &t.doc)));
            self.unsaved_texts.update(ctx, symbols, documents);
        }
        if let Some(index) = &self.index {
            for event in index.events() {
                match event {
                    IndexEvent::Ready(files) => self.indexed = Some(files),
                    IndexEvent::Results { generation, found } => {
                        if let Some(palette) = &mut self.quick_open {
                            palette.results(generation, found);
                        }
                    }
                }
            }
        }
        self.finish_file_ops(ctx);
        self.search.poll();
        self.poll_folder_replace();
        if let Some((generation, found)) = definitions {
            self.definitions_found(ctx, generation, found);
        }
        let outline_shown = self.sidebar.visible && self.sidebar_mode == SidebarMode::Outline;
        let active_id = self.tabs[self.active].id;
        let local_definition = self
            .pending_definition
            .as_ref()
            .is_some_and(|p| p.local && p.tab == active_id);
        if outline_shown
            || local_definition
            || self
                .quick_open
                .as_ref()
                .is_some_and(QuickOpen::wants_symbols)
        {
            let tab = &self.tabs[self.active];
            if tab.loading.is_none() {
                let language = tab.language.as_deref();
                let rules = language.and_then(|l| self.symbol_rules.get(l).cloned());
                self.outline.update(ctx, tab.id, &tab.doc, language, rules);
            }
        }
        if local_definition {
            self.local_definition(ctx);
        }
        if let Some(tree) = &mut self.tree
            && let Some(bad) = tree.poll()
        {
            self.status = Some(self.texts.project_bad_exclude(&bad.pattern, &bad.error));
        }
        self.poll_loading(ctx);
        let tab = &self.tabs[self.active];
        if self.find.is_open() && tab.loading.is_none() {
            self.find.update(ctx, tab.id, &tab.doc);
            if let Some(range) = self.find.take_jump(tab.id, &tab.doc) {
                self.select_match(range);
            }
        }
        // Editing a preview tab keeps it.
        for tab in &mut self.tabs {
            tab.preview &= !tab.doc.is_dirty();
        }
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
        let state = self.command_state();
        let shortcut = commands::pressed(&ctx).filter(|c| c.is_available(&state));
        let (menu_command, menu_action) = self.menu_bar(ui);
        if let Some(command) = shortcut.or(menu_command) {
            self.run_command(&ctx, frame, command);
        }
        if let Some(action) = menu_action {
            self.run_menu_action(&ctx, action);
            self.focus_editor(&ctx);
        }
        // The status bar spans the whole width; the tab bar only the editor's, next to the
        // side panel.
        if let Some(action) = self.status_bar(ui) {
            self.run_status_action(&ctx, action);
            // Give the keyboard back to the editor after using a menu.
            self.focus_editor(&ctx);
        }
        let (tree_action, search_action, outline_pick) = self.side_panel(ui);
        if let Some(range) = outline_pick {
            self.remember();
            self.select_match(range);
            self.focus_editor(&ctx);
        }
        match search_action {
            Some(SearchAction::Open(OpenMatch { path, line, column })) => {
                self.remember();
                self.open_request(
                    &ctx,
                    OpenRequest {
                        path,
                        line: Some(line),
                        column: Some(column),
                    },
                );
                self.focus_editor(&ctx);
            }
            Some(SearchAction::ReplaceAll(plan)) => self.pending_folder_replace = Some(plan),
            None => {}
        }
        match tree_action {
            Some(TreeAction::Open(path)) => {
                self.open_file(&ctx, path);
                self.focus_editor(&ctx);
            }
            Some(TreeAction::Preview(path)) => {
                self.open_file_as(&ctx, path, true);
                if let Some(tree) = &mut self.tree {
                    tree.focus(&ctx);
                }
            }
            Some(TreeAction::Leave) => self.focus_editor(&ctx),
            Some(TreeAction::Run(op)) => self.file_ops.start(&ctx, op),
            Some(TreeAction::ConfirmTrash(path)) => self.pending_trash = Some(path),
            Some(TreeAction::Status(message)) => self.status = Some(message),
            None => {}
        }
        match self.tab_bar(ui) {
            Some(TabAction::Activate(index)) => self.activate(&ctx, index),
            Some(TabAction::Close(index)) => self.close_tab(&ctx, index),
            Some(TabAction::Keep(index)) => self.tabs[index].preview = false,
            None => {}
        }
        let mut missing_glyph = None;
        let mut definition_click = false;
        let mut notice_action = None;
        let mut find_action = None;
        // Escape in the editor closes the find bar.
        if self.find.is_open()
            && ctx.memory(|m| m.has_focus(self.active_tab().view.id()))
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            find_action = Some(FindAction::Close);
        }
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
                if self.find.is_open() {
                    find_action = self.find.show(ui, self.texts, tab.id, &tab.doc);
                }
                if let Some(notice) = &tab.disk.notice {
                    let name = tab.title(self.texts);
                    notice_action = disk::notice_bar(ui, notice, &name, self.texts);
                }
                let tab_id = tab.id;
                let Tab {
                    view,
                    doc,
                    highlight,
                    ..
                } = tab;
                let highlights = highlight.as_mut().and_then(TabHighlight::current);
                let marks = self.find.marks(tab_id, doc);
                let output = view.show(ui, doc, highlights, &style, marks);
                if let Some(highlight) = highlight {
                    highlight.follow(doc, output.visible_lines);
                }
                missing_glyph = output.missing_glyph;
                definition_click = output.definition_at.is_some();
            }
        });
        if let Some(action) = notice_action {
            self.resolve_disk_notice(action);
            self.focus_editor(&ctx);
        }
        match find_action {
            Some(FindAction::Next) => self.find_step(&ctx, true),
            Some(FindAction::Previous) => self.find_step(&ctx, false),
            Some(FindAction::ReplaceOne) => self.replace(false),
            Some(FindAction::ReplaceAll) => self.replace(true),
            Some(FindAction::Close) => {
                self.find.close();
                self.focus_editor(&ctx);
            }
            None => {}
        }
        if let Some(c) = missing_glyph {
            self.request_fallback(&ctx, c);
        }
        if definition_click {
            self.go_to_definition();
        }
        self.quick_open_palette(&ctx, frame);
        self.unsaved_dialog(&ctx, frame);
        self.trash_dialog(&ctx);
        self.folder_replace_dialog(&ctx);
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

/// Where `path` is after `from` was renamed to `to`: `to` itself, or the same place inside it.
/// `None` if `path` is neither `from` nor inside it. Compared ignoring ASCII case, like Windows.
fn moved_path(
    path: &std::path::Path,
    from: &std::path::Path,
    to: &std::path::Path,
) -> Option<PathBuf> {
    let mut rest = path.components();
    for part in from.components() {
        let other = rest.next()?;
        if !other.as_os_str().eq_ignore_ascii_case(part.as_os_str()) {
            return None;
        }
    }
    let rest = rest.as_path();
    Some(if rest.as_os_str().is_empty() {
        to.to_path_buf()
    } else {
        to.join(rest)
    })
}

/// The Recent submenu: files, then folders; entries that no longer exist are hidden.
fn recent_menu(ui: &mut egui::Ui, recent: &Recent, texts: &Texts) -> Option<MenuAction> {
    let mut picked = None;
    let files: Vec<&PathBuf> = recent.files.iter().filter(|p| p.is_file()).collect();
    let folders: Vec<&PathBuf> = recent.folders.iter().filter(|p| p.is_dir()).collect();
    if files.is_empty() && folders.is_empty() {
        ui.add_enabled(false, egui::Button::new(texts.recent_empty()));
        return None;
    }
    for (index, group) in [files, folders].into_iter().enumerate() {
        if index == 1 && !group.is_empty() {
            ui.separator();
        }
        for path in group {
            if ui
                .button(recent_label(path))
                .on_hover_text(path.display().to_string())
                .clicked()
            {
                picked = Some(MenuAction::OpenRecent(path.clone()));
            }
        }
    }
    ui.separator();
    if ui.button(texts.menu_recent_clear()).clicked() {
        picked = Some(MenuAction::ClearRecent);
    }
    picked
}

/// "Program.cs — C:\src\Demo": the name, then the folder it is in.
fn recent_label(path: &std::path::Path) -> String {
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => {
            format!("{name}  —  {}", parent.display())
        }
        _ => name,
    }
}

/// "file ● — project — Bracket": the active file, a dot for unsaved changes and the project
/// folder's name.
fn window_title(file: &str, dirty: bool, project: Option<&std::path::Path>) -> String {
    let dirty = if dirty { " ●" } else { "" };
    match project.and_then(|p| p.file_name()) {
        Some(folder) => format!("{file}{dirty} — {} — Bracket", folder.to_string_lossy()),
        None => format!("{file}{dirty} — Bracket"),
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
    fn recent_entries_show_the_name_then_the_folder() {
        assert_eq!(
            recent_label(std::path::Path::new(r"C:\src\Demo\Program.cs")),
            r"Program.cs  —  C:\src\Demo"
        );
    }

    #[test]
    fn renamed_paths_follow_the_file_or_folder() {
        let p = std::path::Path::new;
        let (from, to) = (p(r"C:\src\Models"), p(r"C:\src\Entities"));
        assert_eq!(
            moved_path(p(r"C:\src\Models"), from, to),
            Some(to.to_path_buf())
        );
        assert_eq!(
            moved_path(p(r"c:\SRC\models\User.cs"), from, to),
            Some(p(r"C:\src\Entities\User.cs").to_path_buf())
        );
        assert_eq!(moved_path(p(r"C:\src\ModelsOld\A.cs"), from, to), None);
        assert_eq!(moved_path(p(r"C:\src"), from, to), None);
    }

    #[test]
    fn the_title_names_the_file_and_the_project() {
        assert_eq!(window_title("a.cs", false, None), "a.cs — Bracket");
        assert_eq!(
            window_title("a.cs", true, Some(std::path::Path::new(r"C:\src\Demo"))),
            "a.cs ● — Demo — Bracket"
        );
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
