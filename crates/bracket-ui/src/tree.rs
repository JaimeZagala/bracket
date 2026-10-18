//! The project tree in the side panel. Each folder is listed on a worker thread the first time
//! it is opened; after that the project watcher tells which listed folders changed (without a
//! watcher, a folder is listed again each time it is reopened). Only the visible rows are drawn.
//! Works with the mouse and the keyboard, and shows a language badge next to files. Its context
//! menu creates, renames and deletes entries; names are typed in place, in the tree.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use bracket_core::CancelToken;
use bracket_index::{BadPattern, Entry, WalkOptions, list_dir};
use bracket_session::ProjectChanges;
use bracket_syntax::LanguageRegistry;
use eframe::egui::{
    self, Color32, EventFilter, FontId, Id, Key, KeyboardShortcut, Modifiers, Rect, Sense, Shape,
    Stroke, TextStyle, Ui, UiBuilder, WidgetInfo, WidgetType, pos2,
    text::{CCursor, CCursorRange},
    vec2,
};

use crate::file_ops::{self, FileOp, NameProblem};
use crate::i18n::Texts;
use crate::tabs;

const ROW_HEIGHT: f32 = 22.0;
const INDENT: f32 = 14.0;
const LEFT_PADDING: f32 = 6.0;
const CHEVRON_WIDTH: f32 = 16.0;
const GAP: f32 = 5.0;

/// What the user did in the tree that the app has to carry out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeAction {
    /// Open the file in a normal tab and move the keyboard to it.
    Open(PathBuf),
    /// Show the file in the preview tab; the keyboard stays in the tree.
    Preview(PathBuf),
    /// Escape: give the keyboard back to the editor.
    Leave,
    /// Create or rename an entry (the name has been checked).
    Run(FileOp),
    /// Move an entry to the Recycle Bin, once the user confirms.
    ConfirmTrash(PathBuf),
    /// A message for the status bar.
    Status(String),
}

/// A name being typed in the tree.
struct NameEdit {
    kind: EditKind,
    text: String,
    /// Give it the keyboard (and on the first frame, select the name without its extension).
    focus: bool,
    first_frame: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum EditKind {
    NewFile { dir: PathBuf },
    NewFolder { dir: PathBuf },
    Rename { path: PathBuf },
}

/// Something picked in a context menu.
enum MenuPick {
    NewFile(PathBuf),
    NewFolder(PathBuf),
    Rename(PathBuf),
    Trash(PathBuf),
    CopyPath(PathBuf),
    CopyRelativePath(PathBuf),
    Reveal(PathBuf),
}

/// What happened to the name being typed this frame.
enum EditOutcome {
    Typing,
    Commit,
    Cancel,
}

/// A folder listed on the worker thread.
struct Listing {
    dir: PathBuf,
    entries: Vec<Entry>,
    /// The exclusions could not be used (the folder was listed without them).
    bad_pattern: Option<BadPattern>,
}

/// One visible row.
struct Row {
    path: PathBuf,
    name: String,
    depth: usize,
    is_dir: bool,
    /// The row where the name of a new entry is typed.
    placeholder: bool,
}

pub struct ProjectTree {
    root: PathBuf,
    /// Listed folders; `None` while a folder's first listing is on its way.
    folders: HashMap<PathBuf, Option<Vec<Entry>>>,
    /// Open folders. A folder inside a closed one stays open, as it was, for when its parent
    /// opens again.
    expanded: HashSet<PathBuf>,
    selected: Option<PathBuf>,
    /// The visible rows, rebuilt when a listing arrives or a folder opens or closes.
    rows: Vec<Row>,
    rows_stale: bool,
    /// Scroll the selected row into view on the next frame.
    reveal_selected: bool,
    /// The scroll offset and viewport height of the last frame.
    scroll: (f32, f32),
    requests: Sender<PathBuf>,
    listings: Receiver<Listing>,
    cancel: CancelToken,
    bad_pattern_reported: bool,
    /// The project watcher reports changes, so open folders are not listed again on reopening.
    watched: bool,
    /// Listings requested and not received yet.
    pending: usize,
    edit: Option<NameEdit>,
}

impl ProjectTree {
    /// Starts listing `root`. `notify` is called whenever a listing is ready.
    pub fn new(root: PathBuf, options: WalkOptions, notify: impl Fn() + Send + 'static) -> Self {
        let (requests, request_rx) = mpsc::channel::<PathBuf>();
        let (listing_tx, listings) = mpsc::channel();
        let cancel = CancelToken::new();
        let worker_root = root.clone();
        let worker_cancel = cancel.clone();
        thread::Builder::new()
            .name("project-tree".to_owned())
            .spawn(move || {
                let mut options = options;
                while let Ok(dir) = request_rx.recv() {
                    if worker_cancel.is_cancelled() {
                        break;
                    }
                    let (entries, bad_pattern) = match list_dir(&worker_root, &dir, &options) {
                        Ok(entries) => (entries, None),
                        Err(bad) => {
                            // Reported once; the tree goes on without the exclusions.
                            options = WalkOptions::default();
                            let entries =
                                list_dir(&worker_root, &dir, &options).unwrap_or_default();
                            (entries, Some(bad))
                        }
                    };
                    let listing = Listing {
                        dir,
                        entries,
                        bad_pattern,
                    };
                    if listing_tx.send(listing).is_err() {
                        break;
                    }
                    notify();
                }
            })
            .expect("the project tree thread starts");
        let mut tree = Self {
            folders: HashMap::new(),
            expanded: HashSet::new(),
            selected: None,
            rows: Vec::new(),
            rows_stale: true,
            reveal_selected: false,
            scroll: (0.0, 0.0),
            requests,
            listings,
            cancel,
            bad_pattern_reported: false,
            watched: false,
            pending: 0,
            edit: None,
            root,
        };
        tree.request(tree.root.clone());
        tree
    }

    fn id() -> Id {
        Id::new("project-tree")
    }

    /// Opens these folders (from the session); the ones outside the project are ignored.
    pub fn expand(&mut self, folders: impl IntoIterator<Item = PathBuf>) {
        let root = self.root.clone();
        self.expanded.extend(
            folders
                .into_iter()
                .filter(|f| f.starts_with(&root) && *f != root),
        );
        self.rows_stale = true;
    }

    /// The open folders worth keeping in the session: the ones that still exist as far as the
    /// listings tell, sorted.
    pub fn expanded_for_session(&self) -> Vec<PathBuf> {
        let mut folders: Vec<PathBuf> = self
            .expanded
            .iter()
            .filter(|folder| self.may_exist(folder))
            .cloned()
            .collect();
        folders.sort();
        folders
    }

    /// False if a listing of one of `folder`'s parents shows it is gone.
    fn may_exist(&self, folder: &Path) -> bool {
        let mut child = folder;
        while let Some(parent) = child.parent() {
            if let Some(Some(entries)) = self.folders.get(parent)
                && !entries.iter().any(|e| e.is_dir && e.path == child)
            {
                return false;
            }
            if parent == self.root {
                return true;
            }
            child = parent;
        }
        false
    }

    fn request(&mut self, dir: PathBuf) {
        self.folders.entry(dir.clone()).or_insert(None);
        self.pending += 1;
        let _ = self.requests.send(dir);
    }

    /// Takes the listings that are ready. Returns the exclusion pattern that could not be used,
    /// the first time one is found.
    pub fn poll(&mut self) -> Option<BadPattern> {
        let mut bad = None;
        while let Ok(listing) = self.listings.try_recv() {
            self.pending -= 1;
            if let Some(pattern) = listing.bad_pattern
                && !self.bad_pattern_reported
            {
                self.bad_pattern_reported = true;
                bad = Some(pattern);
            }
            self.folders.insert(listing.dir, Some(listing.entries));
            self.rows_stale = true;
        }
        bad
    }

    /// Moves the keyboard to the tree.
    pub fn focus(&mut self, ctx: &egui::Context) {
        ctx.memory_mut(|m| m.request_focus(Self::id()));
        self.reveal_selected = true;
    }

    /// Whether the project watcher reports changes to the tree.
    pub fn set_watched(&mut self, watched: bool) {
        self.watched = watched;
    }

    /// Lists again the folders where something was created, deleted or renamed, if they were
    /// listed. A change to `.gitignore` or `.ignore` can change any folder, so then they all are.
    pub fn apply_changes(&mut self, changes: &ProjectChanges) {
        let rules_changed = changes.paths.iter().any(|path| {
            path.file_name().is_some_and(|n| {
                n.eq_ignore_ascii_case(".gitignore") || n.eq_ignore_ascii_case(".ignore")
            })
        });
        let dirs: HashSet<PathBuf> = if changes.rescan || rules_changed {
            self.folders.keys().cloned().collect()
        } else {
            changes
                .paths
                .iter()
                .filter_map(|path| path.parent())
                .filter(|dir| self.folders.contains_key(*dir))
                .map(Path::to_path_buf)
                .collect()
        };
        for dir in dirs {
            self.request(dir);
        }
    }

    fn set_expanded(&mut self, folder: &Path, open: bool) {
        if open {
            self.expanded.insert(folder.to_path_buf());
            if !self.watched {
                // Listed again, in case it changed since it was last open.
                self.request(folder.to_path_buf());
            }
        } else {
            self.expanded.remove(folder);
        }
        self.rows_stale = true;
    }

    /// Rebuilds the visible rows; open folders that were never listed are requested.
    fn rebuild_rows(&mut self) {
        let mut rows = Vec::new();
        let mut missing = Vec::new();
        self.collect_rows(&self.root, 0, &mut rows, &mut missing);
        self.rows = rows;
        for dir in missing {
            self.request(dir);
        }
        self.rows_stale = false;
    }

    fn collect_rows(
        &self,
        dir: &Path,
        depth: usize,
        rows: &mut Vec<Row>,
        missing: &mut Vec<PathBuf>,
    ) {
        if let Some(edit) = &self.edit
            && let EditKind::NewFile { dir: target } | EditKind::NewFolder { dir: target } =
                &edit.kind
            && target == dir
        {
            rows.push(Row {
                path: dir.to_path_buf(),
                name: String::new(),
                depth,
                is_dir: matches!(edit.kind, EditKind::NewFolder { .. }),
                placeholder: true,
            });
        }
        match self.folders.get(dir) {
            Some(Some(entries)) => {
                for entry in entries {
                    rows.push(Row {
                        path: entry.path.clone(),
                        name: entry.name.clone(),
                        depth,
                        is_dir: entry.is_dir,
                        placeholder: false,
                    });
                    if entry.is_dir && self.expanded.contains(&entry.path) {
                        self.collect_rows(&entry.path, depth + 1, rows, missing);
                    }
                }
            }
            Some(None) => {}
            None => missing.push(dir.to_path_buf()),
        }
    }

    fn selected_index(&self) -> Option<usize> {
        let selected = self.selected.as_ref()?;
        self.rows
            .iter()
            .position(|row| !row.placeholder && &row.path == selected)
    }

    /// Selects an entry that may not be listed yet (just created or renamed); it is scrolled
    /// into view once it appears.
    pub fn select_path(&mut self, path: PathBuf) {
        self.selected = Some(path);
        self.reveal_selected = true;
    }

    /// Lists again the folder that holds `path`, if it was listed: an operation of ours changed
    /// it, and the watcher (if any) would take a moment longer.
    pub fn refresh_parent_of(&mut self, path: &Path) {
        if let Some(dir) = path.parent()
            && self.folders.contains_key(dir)
        {
            self.request(dir.to_path_buf());
        }
    }

    /// Starts typing a name in the tree.
    fn begin_edit(&mut self, kind: EditKind) {
        let text = match &kind {
            EditKind::Rename { path } => path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            EditKind::NewFile { dir } | EditKind::NewFolder { dir } => {
                if *dir != self.root && !self.expanded.contains(dir) {
                    self.set_expanded(&dir.clone(), true);
                }
                String::new()
            }
        };
        self.edit = Some(NameEdit {
            kind,
            text,
            focus: true,
            first_frame: true,
        });
        self.rows_stale = true;
    }

    /// Carries out a context menu pick.
    fn run_pick(
        &mut self,
        ctx: &egui::Context,
        pick: MenuPick,
        texts: &Texts,
    ) -> Option<TreeAction> {
        match pick {
            MenuPick::NewFile(dir) => self.begin_edit(EditKind::NewFile { dir }),
            MenuPick::NewFolder(dir) => self.begin_edit(EditKind::NewFolder { dir }),
            MenuPick::Rename(path) => self.begin_edit(EditKind::Rename { path }),
            MenuPick::Trash(path) => return Some(TreeAction::ConfirmTrash(path)),
            MenuPick::CopyPath(path) => ctx.copy_text(path.display().to_string()),
            MenuPick::CopyRelativePath(path) => {
                let relative = path.strip_prefix(&self.root).unwrap_or(&path);
                ctx.copy_text(relative.display().to_string());
            }
            MenuPick::Reveal(path) => {
                if let Err(err) = file_ops::reveal_in_explorer(&path) {
                    let name = display_name(&path);
                    return Some(TreeAction::Status(
                        texts.file_op_failed(&name, &err.to_string()),
                    ));
                }
            }
        }
        None
    }

    /// Ends typing a name: checks it and returns the operation to run. A bad name keeps the
    /// edit open and returns the problem for the status bar.
    fn commit_edit(&mut self, texts: &Texts) -> Option<TreeAction> {
        let edit = self.edit.as_mut()?;
        let name = edit.text.trim().to_owned();
        if let EditKind::Rename { path } = &edit.kind
            && path.file_name().is_some_and(|n| n == name.as_str())
        {
            self.cancel_edit();
            return None;
        }
        if let Err(problem) = file_ops::check_name(&name) {
            edit.focus = true;
            let message = match problem {
                NameProblem::Empty => texts.name_empty().to_owned(),
                NameProblem::ForbiddenCharacter => texts.name_forbidden(file_ops::FORBIDDEN),
                NameProblem::BadEnding => texts.name_bad_ending().to_owned(),
                NameProblem::Reserved => texts.name_reserved(&name),
            };
            return Some(TreeAction::Status(message));
        }
        let op = match &edit.kind {
            EditKind::NewFile { dir } => FileOp::CreateFile(dir.join(&name)),
            EditKind::NewFolder { dir } => FileOp::CreateFolder(dir.join(&name)),
            EditKind::Rename { path } => FileOp::Rename {
                from: path.clone(),
                to: path.with_file_name(&name),
            },
        };
        self.cancel_edit();
        Some(TreeAction::Run(op))
    }

    fn cancel_edit(&mut self) {
        self.edit = None;
        self.rows_stale = true;
    }

    fn select(&mut self, index: usize) {
        if let Some(row) = self.rows.get(index) {
            self.selected = Some(row.path.clone());
            self.reveal_selected = true;
        }
    }

    /// The keys the tree handles while it has the keyboard.
    fn handle_keys(&mut self, ctx: &egui::Context) -> Option<TreeAction> {
        if self.rows.is_empty() {
            return None;
        }
        let pressed = |key| ctx.input_mut(|i| i.consume_key(Modifiers::NONE, key));
        let page = ((self.scroll.1 / ROW_HEIGHT) as usize).max(1);
        let last = self.rows.len() - 1;
        let Some(index) = self.selected_index() else {
            if [Key::ArrowDown, Key::ArrowUp, Key::Home, Key::End]
                .into_iter()
                .any(pressed)
            {
                self.select(0);
            }
            return None;
        };
        if pressed(Key::ArrowDown) {
            self.select((index + 1).min(last));
        } else if pressed(Key::ArrowUp) {
            self.select(index.saturating_sub(1));
        } else if pressed(Key::PageDown) {
            self.select((index + page).min(last));
        } else if pressed(Key::PageUp) {
            self.select(index.saturating_sub(page));
        } else if pressed(Key::Home) {
            self.select(0);
        } else if pressed(Key::End) {
            self.select(last);
        } else if pressed(Key::ArrowRight) {
            let row = &self.rows[index];
            if row.is_dir {
                if self.expanded.contains(&row.path) {
                    // Into the first child, if it is listed already.
                    if self
                        .rows
                        .get(index + 1)
                        .is_some_and(|r| r.depth > row.depth)
                    {
                        self.select(index + 1);
                    }
                } else {
                    let path = row.path.clone();
                    self.set_expanded(&path, true);
                }
            }
        } else if pressed(Key::ArrowLeft) {
            let row = &self.rows[index];
            if row.is_dir && self.expanded.contains(&row.path) {
                let path = row.path.clone();
                self.set_expanded(&path, false);
            } else if let Some(parent) = self.rows[..index]
                .iter()
                .rposition(|r| r.depth + 1 == row.depth)
            {
                self.select(parent);
            }
        } else if pressed(Key::Enter) {
            let row = &self.rows[index];
            if row.is_dir {
                let (path, open) = (row.path.clone(), !self.expanded.contains(&row.path));
                self.set_expanded(&path, open);
            } else {
                return Some(TreeAction::Open(row.path.clone()));
            }
        } else if pressed(Key::Space) {
            let row = &self.rows[index];
            if !row.is_dir {
                return Some(TreeAction::Preview(row.path.clone()));
            }
        } else if pressed(Key::F2) {
            let path = self.rows[index].path.clone();
            self.begin_edit(EditKind::Rename { path });
        } else if pressed(Key::Delete) {
            return Some(TreeAction::ConfirmTrash(self.rows[index].path.clone()));
        } else if pressed(Key::Escape) {
            return Some(TreeAction::Leave);
        }
        None
    }

    /// Draws the tree in the side panel.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        registry: &LanguageRegistry,
        texts: &Texts,
    ) -> Option<TreeAction> {
        let id = Self::id();
        // Registered first and under the rows, so it never takes their clicks; it only holds
        // the keyboard focus for the whole tree.
        let area = ui.interact(ui.max_rect(), id, Sense::focusable_noninteractive());
        let focused = area.has_focus();
        let mut action = None;
        if focused {
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    id,
                    EventFilter {
                        tab: false,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: true,
                    },
                );
            });
            if self.rows_stale {
                self.rebuild_rows();
            }
            action = self.handle_keys(ui.ctx());
        }
        if self.rows_stale {
            self.rebuild_rows();
        }

        let name = self.root.file_name().map_or_else(
            || self.root.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let mut pick = None;
        ui.horizontal(|ui| {
            ui.add_space(LEFT_PADDING);
            let header = ui
                .add(egui::Label::new(egui::RichText::new(name).strong()).sense(Sense::click()))
                .on_hover_text(self.root.display().to_string());
            header.context_menu(|ui| {
                pick = context_menu(ui, &self.root, &self.root, true, texts);
            });
        });
        ui.add_space(2.0);
        if let Some(picked) = pick.take() {
            if let Some(picked_action) = self.run_pick(ui.ctx(), picked, texts) {
                action = Some(picked_action);
            }
            if self.rows_stale {
                self.rebuild_rows();
            }
        }

        if self.rows.is_empty() {
            if matches!(self.folders.get(&self.root), Some(Some(_))) {
                ui.horizontal(|ui| {
                    ui.add_space(LEFT_PADDING);
                    ui.weak(texts.tree_empty());
                });
            }
            return action;
        }

        let mut scroll = egui::ScrollArea::vertical()
            .id_salt("project-tree-rows")
            .auto_shrink(false);
        // A row being edited is scrolled into view like a selected one.
        let edit_row = self.rows.iter().position(|row| match &self.edit {
            Some(NameEdit {
                kind: EditKind::Rename { path },
                ..
            }) => !row.placeholder && row.path == *path,
            Some(_) => row.placeholder,
            None => false,
        });
        let reveal = match edit_row {
            Some(index) if self.edit.as_ref().is_some_and(|e| e.first_frame) => Some(index),
            _ if self.reveal_selected => self.selected_index(),
            _ => None,
        };
        if let Some(index) = reveal {
            let (offset, height) = self.scroll;
            let top = index as f32 * ROW_HEIGHT;
            if top < offset {
                scroll = scroll.vertical_scroll_offset(top);
            } else if top + ROW_HEIGHT > offset + height && height > 0.0 {
                scroll = scroll.vertical_scroll_offset(top + ROW_HEIGHT - height);
            }
            // A selected entry that is not listed yet keeps waiting to be revealed.
            self.reveal_selected = false;
        }
        // The row clicked, and whether it was a double click.
        let mut clicked = None;
        let mut outcome = EditOutcome::Typing;
        let (rows, edit, root, selected_path, expanded) = (
            &self.rows,
            &mut self.edit,
            &self.root,
            &self.selected,
            &self.expanded,
        );
        let output = scroll.show_rows(ui, ROW_HEIGHT, rows.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for index in range {
                let row = &rows[index];
                if Some(index) == edit_row
                    && let Some(edit) = edit.as_mut()
                {
                    outcome = edit_widget(ui, row, edit, registry);
                    continue;
                }
                let selected = selected_path.as_ref() == Some(&row.path);
                let is_open = row.is_dir && expanded.contains(&row.path);
                let response = row_widget(ui, row, selected, focused, is_open, registry);
                if response.clicked() || response.double_clicked() {
                    clicked = Some((index, response.double_clicked(), false));
                }
                if response.secondary_clicked() {
                    clicked = Some((index, false, true));
                }
                response.context_menu(|ui| {
                    pick = context_menu(ui, root, &row.path, row.is_dir, texts);
                });
            }
        });
        self.scroll = (output.state.offset.y, output.inner_rect.height());

        match outcome {
            EditOutcome::Typing => {}
            EditOutcome::Commit => {
                if let Some(commit) = self.commit_edit(texts) {
                    action = Some(commit);
                }
                if self.edit.is_none() {
                    ui.memory_mut(|m| m.request_focus(id));
                }
            }
            EditOutcome::Cancel => {
                self.cancel_edit();
                ui.memory_mut(|m| m.request_focus(id));
            }
        }
        if let Some(picked) = pick {
            if let Some(picked_action) = self.run_pick(ui.ctx(), picked, texts) {
                action = Some(picked_action);
            }
            return action;
        }

        if let Some((index, double, secondary)) = clicked {
            ui.memory_mut(|m| m.request_focus(id));
            self.select(index);
            let row = &self.rows[index];
            if secondary {
                // Right click: only selects, for the context menu.
            } else if row.is_dir {
                let (path, open) = (row.path.clone(), !self.expanded.contains(&row.path));
                self.set_expanded(&path, open);
            } else if double {
                action = Some(TreeAction::Open(row.path.clone()));
            } else {
                action = Some(TreeAction::Preview(row.path.clone()));
            }
        }
        action
    }
}

impl Drop for ProjectTree {
    /// Folders still waiting to be listed are not listed.
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

/// The entry's name, for messages.
pub fn display_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// The context menu of an entry (or of the project folder, `path == root`).
fn context_menu(
    ui: &mut Ui,
    root: &Path,
    path: &Path,
    is_dir: bool,
    texts: &Texts,
) -> Option<MenuPick> {
    let folder = if is_dir {
        path.to_path_buf()
    } else {
        path.parent().unwrap_or(root).to_path_buf()
    };
    let is_root = path == root;
    let shortcut = |key| {
        ui.ctx()
            .format_shortcut(&KeyboardShortcut::new(Modifiers::NONE, key))
    };
    let (rename_keys, delete_keys) = (shortcut(Key::F2), shortcut(Key::Delete));
    let mut items: Vec<(&str, Option<String>, MenuPick)> = vec![
        (
            texts.tree_new_file(),
            None,
            MenuPick::NewFile(folder.clone()),
        ),
        (texts.tree_new_folder(), None, MenuPick::NewFolder(folder)),
    ];
    if !is_root {
        items.push((
            texts.tree_rename(),
            Some(rename_keys),
            MenuPick::Rename(path.to_path_buf()),
        ));
        items.push((
            texts.tree_delete(),
            Some(delete_keys),
            MenuPick::Trash(path.to_path_buf()),
        ));
    }
    items.push((
        texts.tree_copy_path(),
        None,
        MenuPick::CopyPath(path.to_path_buf()),
    ));
    if !is_root {
        items.push((
            texts.tree_copy_relative_path(),
            None,
            MenuPick::CopyRelativePath(path.to_path_buf()),
        ));
    }
    items.push((
        texts.tree_reveal(),
        None,
        MenuPick::Reveal(path.to_path_buf()),
    ));

    let mut picked = None;
    for (index, (label, keys, pick)) in items.into_iter().enumerate() {
        // Separators after "new" and after "delete".
        if index == 2 || (index == 4 && !is_root) {
            ui.separator();
        }
        let mut button = egui::Button::new(label);
        if let Some(keys) = keys {
            button = button.shortcut_text(keys);
        }
        if ui.add(button).clicked() {
            picked = Some(pick);
            ui.close();
        }
    }
    picked
}

/// The row where a name is typed: the same indentation and icon as a row, with a text field.
fn edit_widget(
    ui: &mut Ui,
    row: &Row,
    edit: &mut NameEdit,
    registry: &LanguageRegistry,
) -> EditOutcome {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::hover());
    let mut x = rect.left() + LEFT_PADDING + row.depth as f32 * INDENT + CHEVRON_WIDTH;
    let center_y = rect.center().y;
    let is_rename = matches!(edit.kind, EditKind::Rename { .. });
    if !row.is_dir {
        let typed = Path::new(&edit.text);
        let shown = if is_rename && edit.text.is_empty() {
            &row.path
        } else {
            typed
        };
        if let Some(language) = registry.for_path(shown) {
            let font = TextStyle::Body.resolve(ui.style());
            let badge_font = FontId::proportional((font.size * 0.75).round());
            x += tabs::paint_badge(ui.painter(), &language.badge, badge_font, pos2(x, center_y))
                + GAP;
        }
    }
    let field = Rect::from_min_max(
        pos2(x, rect.top() + 1.0),
        pos2(rect.right() - 4.0, rect.bottom() - 1.0),
    );
    let id = Id::new("project-tree-name");
    let mut output = ui
        .scope_builder(UiBuilder::new().max_rect(field), |ui| {
            egui::TextEdit::singleline(&mut edit.text)
                .id(id)
                .desired_width(f32::INFINITY)
                .margin(vec2(4.0, 1.0))
                .show(ui)
        })
        .inner;
    if edit.focus {
        output.response.request_focus();
        edit.focus = false;
    }
    if edit.first_frame {
        edit.first_frame = false;
        // The name without its extension, like File Explorer.
        let stem = match edit.text.rfind('.') {
            Some(dot) if dot > 0 && !row.is_dir => edit.text[..dot].chars().count(),
            _ => edit.text.chars().count(),
        };
        let range = CCursorRange::two(CCursor::new(0), CCursor::new(stem));
        output.state.cursor.set_char_range(Some(range));
        output.state.store(ui.ctx(), id);
    }
    let response = output.response;
    if ui.input(|i| i.key_pressed(Key::Escape)) && (response.lost_focus() || response.has_focus()) {
        EditOutcome::Cancel
    } else if response.lost_focus() {
        // Enter, or a click elsewhere: like File Explorer, both keep the name.
        EditOutcome::Commit
    } else {
        EditOutcome::Typing
    }
}

/// One row: indentation, an arrow for folders or a badge for files, and the name.
fn row_widget(
    ui: &mut Ui,
    row: &Row,
    selected: bool,
    focused: bool,
    expanded: bool,
    registry: &LanguageRegistry,
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
    response.widget_info(|| {
        WidgetInfo::selected(
            WidgetType::SelectableLabel,
            true,
            selected,
            row.name.clone(),
        )
    });
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let visuals = ui.visuals();
    let painter = ui.painter();
    if selected {
        let fill = if focused {
            visuals.selection.bg_fill
        } else {
            visuals.widgets.inactive.weak_bg_fill
        };
        painter.rect_filled(rect, 0.0, fill);
    } else if response.hovered() {
        painter.rect_filled(rect, 0.0, visuals.widgets.hovered.weak_bg_fill);
    }
    let text_color = if selected && focused {
        visuals.selection.stroke.color
    } else {
        visuals.text_color()
    };

    let mut x = rect.left() + LEFT_PADDING + row.depth as f32 * INDENT;
    let center_y = rect.center().y;
    if row.is_dir {
        let center = pos2(x + CHEVRON_WIDTH / 2.0, center_y);
        let s = 3.5;
        let points = if expanded {
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
    x += CHEVRON_WIDTH;

    let font = TextStyle::Body.resolve(ui.style());
    if !row.is_dir
        && let Some(language) = registry.for_path(&row.path)
    {
        let badge_font = FontId::proportional((font.size * 0.75).round());
        x += tabs::paint_badge(painter, &language.badge, badge_font, pos2(x, center_y)) + GAP;
    }
    let galley = painter.layout_no_wrap(row.name.clone(), font, text_color);
    let text_rect = Rect::from_min_max(pos2(x, rect.top()), rect.max);
    painter.with_clip_rect(text_rect).galley(
        pos2(x, center_y - galley.size().y / 2.0),
        galley,
        Color32::WHITE,
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{Duration, Instant};

    fn project(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("bracket-tree-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for path in ["src/Models/User.cs", "src/Program.cs", "README.md"] {
            let path = root.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "x").unwrap();
        }
        root
    }

    /// Rebuilds the rows until every folder they need is listed.
    fn settle(tree: &mut ProjectTree) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            tree.poll();
            tree.rebuild_rows();
            if tree.pending == 0 {
                return;
            }
            assert!(Instant::now() < deadline, "listings never arrived");
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn rows(tree: &ProjectTree) -> Vec<String> {
        tree.rows
            .iter()
            .map(|r| format!("{}{}", "  ".repeat(r.depth), r.name))
            .collect()
    }

    #[test]
    fn open_folders_show_their_children_in_order() {
        let root = project("rows");
        let mut tree = ProjectTree::new(root.clone(), WalkOptions::default(), || {});
        settle(&mut tree);
        assert_eq!(rows(&tree), ["src", "README.md"]);

        tree.expand([root.join("src"), root.join(r"src\Models")]);
        settle(&mut tree);
        assert_eq!(
            rows(&tree),
            [
                "src",
                "  Models",
                "    User.cs",
                "  Program.cs",
                "README.md"
            ]
        );

        // Closing src keeps Models open inside it.
        tree.set_expanded(&root.join("src"), false);
        settle(&mut tree);
        assert_eq!(rows(&tree), ["src", "README.md"]);
        assert_eq!(
            tree.expanded_for_session(),
            [root.join(r"src\Models")],
            "kept for when src opens again"
        );
        drop(tree);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_new_name_is_typed_as_the_first_child_and_checked() {
        let texts = crate::i18n::texts(bracket_session::Language::English);
        let root = project("new");
        let mut tree = ProjectTree::new(root.clone(), WalkOptions::default(), || {});
        settle(&mut tree);
        tree.begin_edit(EditKind::NewFile {
            dir: root.join("src"),
        });
        settle(&mut tree);
        let placeholder = tree.rows.iter().position(|r| r.placeholder);
        assert_eq!(placeholder, Some(1), "right under src, which opened");
        assert_eq!(tree.rows[1].depth, 1);

        tree.edit.as_mut().unwrap().text = "what?.cs".into();
        assert!(matches!(
            tree.commit_edit(texts),
            Some(TreeAction::Status(_))
        ));
        assert!(tree.edit.is_some(), "a bad name keeps the edit open");

        tree.edit.as_mut().unwrap().text = " Startup.cs ".into();
        assert_eq!(
            tree.commit_edit(texts),
            Some(TreeAction::Run(FileOp::CreateFile(
                root.join(r"src\Startup.cs")
            )))
        );
        assert!(tree.edit.is_none());
        tree.rebuild_rows();
        assert!(!tree.rows.iter().any(|r| r.placeholder));
        drop(tree);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn renaming_to_the_same_name_does_nothing() {
        let texts = crate::i18n::texts(bracket_session::Language::English);
        let root = project("rename");
        let mut tree = ProjectTree::new(root.clone(), WalkOptions::default(), || {});
        let path = root.join("README.md");
        tree.begin_edit(EditKind::Rename { path: path.clone() });
        assert_eq!(tree.edit.as_ref().unwrap().text, "README.md");
        assert_eq!(tree.commit_edit(texts), None);
        assert!(tree.edit.is_none());

        tree.begin_edit(EditKind::Rename { path: path.clone() });
        tree.edit.as_mut().unwrap().text = "readme.md".into();
        assert_eq!(
            tree.commit_edit(texts),
            Some(TreeAction::Run(FileOp::Rename {
                from: path,
                to: root.join("readme.md"),
            })),
            "a change of case is a rename"
        );
        drop(tree);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn changes_list_again_only_the_folders_already_listed() {
        let root = project("changes");
        let mut tree = ProjectTree::new(root.clone(), WalkOptions::default(), || {});
        tree.set_watched(true);
        tree.expand([root.join("src")]);
        settle(&mut tree);
        assert_eq!(
            rows(&tree),
            ["src", "  Models", "  Program.cs", "README.md"]
        );

        fs::write(root.join(r"src\Startup.cs"), "x").unwrap();
        fs::write(root.join(r"src\Models\Order.cs"), "x").unwrap();
        let changes = ProjectChanges {
            paths: [
                root.join(r"src\Startup.cs"),
                root.join(r"src\Models\Order.cs"),
            ]
            .into(),
            rescan: false,
        };
        tree.apply_changes(&changes);
        assert!(
            !tree.folders.contains_key(&root.join(r"src\Models")),
            "Models was never listed, so it is not listed now"
        );
        settle(&mut tree);
        assert_eq!(
            rows(&tree),
            [
                "src",
                "  Models",
                "  Program.cs",
                "  Startup.cs",
                "README.md"
            ]
        );

        // A rule change lists everything again.
        fs::write(root.join(".gitignore"), "*.md\n").unwrap();
        let changes = ProjectChanges {
            paths: [root.join(".gitignore")].into(),
            rescan: false,
        };
        tree.apply_changes(&changes);
        settle(&mut tree);
        assert_eq!(
            rows(&tree),
            [
                "src",
                "  Models",
                "  Program.cs",
                "  Startup.cs",
                ".gitignore"
            ]
        );
        drop(tree);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn folders_that_are_gone_or_outside_are_not_kept() {
        let root = project("session");
        let mut tree = ProjectTree::new(root.clone(), WalkOptions::default(), || {});
        tree.expand([
            root.join("src"),
            root.join("deleted"),
            PathBuf::from(r"C:\elsewhere"),
        ]);
        settle(&mut tree);
        assert_eq!(tree.expanded_for_session(), [root.join("src")]);
        drop(tree);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_bad_exclusion_is_reported_once_and_the_tree_still_works() {
        let root = project("bad");
        let options = WalkOptions {
            exclude: vec!["src/[".into()],
        };
        let mut tree = ProjectTree::new(root.clone(), options, || {});
        tree.expand([root.join("src")]);
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut reported = Vec::new();
        while tree.folders.len() < 2 || tree.folders.values().any(Option::is_none) {
            reported.extend(tree.poll());
            tree.rebuild_rows();
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
        reported.extend(tree.poll());
        assert_eq!(reported.len(), 1);
        assert_eq!(reported[0].pattern, "src/[");
        assert_eq!(
            rows(&tree),
            ["src", "  Models", "  Program.cs", "README.md"]
        );
        drop(tree);
        fs::remove_dir_all(&root).unwrap();
    }
}
