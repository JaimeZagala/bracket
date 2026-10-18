//! Keeping the session: rebuilding the previous tabs at startup and saving them while Bracket
//! runs, a few seconds after each change, so a crash loses at most those seconds.
//!
//! The file format lives in `bracket_session::session`; this module turns tabs into session
//! entries and back.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::hash::{Hash, Hasher};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bracket_core::document::{self, Document, LineEnding};
use bracket_core::{Selection, Selections};
use bracket_session::session::{self, Snapshot};
use bracket_session::{Session, SessionFiles, SessionTab, Unsaved};
use eframe::egui;

use crate::tabs::Tab;

/// How long the session must stay unchanged before it is written.
const SAVE_DELAY: Duration = Duration::from_secs(2);

/// Reads a tab of the previous session, on a worker thread: the file from disk (if any) with
/// the unsaved text from its backup on top, as one undoable change.
pub fn restore_document(
    entry: &SessionTab,
    files: &SessionFiles,
    fallback: (LineEnding, bracket_core::encoding::TextEncoding),
    tab_width: usize,
) -> io::Result<Document> {
    let unsaved = match &entry.unsaved {
        Some(unsaved) => Some((unsaved, fs::read_to_string(files.backup(&unsaved.backup))?)),
        None => None,
    };
    let (line_ending, encoding) = fallback;
    let mut doc = match &entry.path {
        Some(path) => {
            let loaded = match entry.read_encoding {
                Some(encoding) => document::load_text_as(path, encoding),
                None => document::load_text(path),
            };
            match loaded {
                Ok(loaded) => Document::from_loaded(path.clone(), loaded, line_ending),
                // The file is gone but its unsaved text is not: keep it, bound to its path.
                Err(err) if err.kind() == io::ErrorKind::NotFound && unsaved.is_some() => {
                    let mut doc = Document::new_empty(line_ending, encoding);
                    doc.set_path(path.clone());
                    doc
                }
                Err(err) => return Err(err),
            }
        }
        None => Document::new_empty(line_ending, encoding),
    };
    if let Some((unsaved, text)) = unsaved {
        doc.restore_unsaved(&text, unsaved.encoding, unsaved.line_ending);
    }
    doc.buffer.set_tab_width(tab_width);
    let selection = Selection::new(entry.anchor, entry.head).clamped(doc.buffer.len_chars());
    doc.buffer.set_selections(Selections::single(selection));
    Ok(doc)
}

/// Result of a save on the worker thread.
struct Saved {
    result: io::Result<Vec<String>>,
    /// Version of the text in each tab's backup, if the save worked.
    backups: HashMap<u64, u64>,
    owned: HashSet<String>,
}

/// Saves the session of the running app.
pub struct SessionKeeper {
    files: SessionFiles,
    /// Prefix of this run's backup names, so they never replace another run's backups.
    run: String,
    /// Backups this keeper may delete: restored at startup or written by this run.
    owned: HashSet<String>,
    /// Version of the text already in each modified tab's backup.
    backups: HashMap<u64, u64>,
    saving: Option<Receiver<Saved>>,
    /// Folder open as a project, kept in the session.
    project: Option<PathBuf>,
    last_saved: Option<u64>,
    changed_at: Option<Instant>,
}

impl SessionKeeper {
    /// `previous` is the session restored at startup; its backups become this keeper's.
    pub fn new(files: SessionFiles, previous: &Session) -> Self {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        Self {
            files,
            run: format!("{millis}-{}", std::process::id()),
            owned: previous.backups().map(String::from).collect(),
            backups: HashMap::new(),
            saving: None,
            project: None,
            last_saved: None,
            changed_at: None,
        }
    }

    pub fn files(&self) -> &SessionFiles {
        &self.files
    }

    /// The folder open as a project (the next save writes it).
    pub fn set_project(&mut self, project: Option<PathBuf>) {
        self.project = project;
    }

    /// Call every frame. Writes the session on a worker thread once it has been stable for a
    /// moment. Returns the error of a failed save, if one has just finished.
    pub fn tick(&mut self, ctx: &egui::Context, tabs: &[Tab], active: usize) -> Option<String> {
        let error = self.poll();
        if self.saving.is_some() {
            return error;
        }
        let signature = signature(tabs, active, self.project.as_deref());
        if self.last_saved == Some(signature) {
            self.changed_at = None;
            return error;
        }
        let now = Instant::now();
        let since = *self.changed_at.get_or_insert(now);
        let wait = SAVE_DELAY.saturating_sub(now - since);
        if !wait.is_zero() {
            ctx.request_repaint_after(wait);
            return error;
        }
        let (snapshot, backups) = self.snapshot(tabs, active);
        let (tx, rx) = mpsc::channel();
        let files = self.files.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result = session::save(&files, &snapshot);
            let _ = tx.send(Saved {
                result,
                backups,
                owned: snapshot.owned,
            });
            ctx.request_repaint();
        });
        self.saving = Some(rx);
        self.last_saved = Some(signature);
        self.changed_at = None;
        error
    }

    /// Writes the session now, on this thread (when the window closes). Waits for a save that
    /// is still running first, so two saves never write the same files at once.
    pub fn save_now(&mut self, tabs: &[Tab], active: usize) -> io::Result<()> {
        if let Some(rx) = self.saving.take()
            && let Ok(saved) = rx.recv()
        {
            self.finish(saved);
        }
        let (snapshot, backups) = self.snapshot(tabs, active);
        let result = session::save(&self.files, &snapshot);
        let failed = result.as_ref().err().map(io::Error::to_string);
        self.finish(Saved {
            result,
            backups,
            owned: snapshot.owned,
        });
        match failed {
            Some(error) => Err(io::Error::other(error)),
            None => Ok(()),
        }
    }

    fn poll(&mut self) -> Option<String> {
        let rx = self.saving.as_ref()?;
        match rx.try_recv() {
            Ok(saved) => {
                self.saving = None;
                self.finish(saved)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.saving = None;
                None
            }
        }
    }

    fn finish(&mut self, saved: Saved) -> Option<String> {
        match saved.result {
            Ok(deleted) => {
                self.backups = saved.backups;
                self.owned = saved.owned;
                for name in deleted {
                    self.owned.remove(&name);
                }
                None
            }
            Err(err) => {
                // Owned names stay owned: a backup may have been written before the failure.
                self.owned.extend(saved.owned);
                // Try again after the next delay.
                self.last_saved = None;
                Some(err.to_string())
            }
        }
    }

    /// The session as it is now, plus the backup texts that changed since they were written.
    fn snapshot(&self, tabs: &[Tab], active: usize) -> (Snapshot, HashMap<u64, u64>) {
        let mut session = Session {
            project: self.project.clone(),
            ..Session::default()
        };
        let mut texts = Vec::new();
        let mut backups = HashMap::new();
        let mut owned = self.owned.clone();
        for (index, tab) in tabs.iter().enumerate() {
            if index == active {
                session.active = session.tabs.len();
            }
            let entry = match (&tab.loading, &tab.unloaded) {
                // Not loaded yet: keep what is known (a restored entry keeps its backup).
                (Some(loading), _) => loading.restore.clone().unwrap_or_else(|| SessionTab {
                    path: loading.path.clone(),
                    read_encoding: loading.read_encoding,
                    ..SessionTab::default()
                }),
                // Never activated: saved again exactly as it was restored.
                (None, Some(entry)) => entry.clone(),
                (None, None) => {
                    let doc = &tab.doc;
                    // An untitled document with nothing in it has nothing to restore.
                    if doc.path().is_none() && !doc.is_dirty() {
                        continue;
                    }
                    let unsaved = doc.is_dirty().then(|| {
                        let name = format!("{}-{}.txt", self.run, tab.id);
                        let version = doc.buffer.version();
                        if self.backups.get(&tab.id) != Some(&version) {
                            texts.push((name.clone(), doc.buffer.text().clone()));
                        }
                        backups.insert(tab.id, version);
                        owned.insert(name.clone());
                        Unsaved {
                            backup: name,
                            encoding: doc.encoding(),
                            line_ending: doc.line_ending(),
                        }
                    });
                    let selection = doc.buffer.selections().primary();
                    SessionTab {
                        path: doc.path().map(PathBuf::from),
                        read_encoding: tab.read_encoding,
                        anchor: selection.anchor,
                        head: selection.head,
                        top_line: tab.view.top_line(),
                        unsaved,
                        language: tab
                            .language_chosen
                            .then(|| tab.language.clone().unwrap_or_default()),
                    }
                }
            };
            session.tabs.push(entry);
        }
        session.active = session.active.min(session.tabs.len().saturating_sub(1));
        let snapshot = Snapshot {
            session,
            backups: texts,
            owned,
        };
        (snapshot, backups)
    }
}

/// A hash of everything the session stores, to notice when it changes.
fn signature(tabs: &[Tab], active: usize, project: Option<&Path>) -> u64 {
    let mut hasher = DefaultHasher::new();
    active.hash(&mut hasher);
    project.hash(&mut hasher);
    for tab in tabs {
        tab.id.hash(&mut hasher);
        tab.path().hash(&mut hasher);
        tab.loading.is_some().hash(&mut hasher);
        tab.unloaded.is_some().hash(&mut hasher);
        (tab.language_chosen, &tab.language).hash(&mut hasher);
        tab.read_encoding.map(|e| e.code()).hash(&mut hasher);
        let doc = &tab.doc;
        doc.is_dirty().hash(&mut hasher);
        doc.buffer.version().hash(&mut hasher);
        doc.encoding().code().hash(&mut hasher);
        doc.line_ending().as_str().hash(&mut hasher);
        let selection = doc.buffer.selections().primary();
        (selection.anchor, selection.head).hash(&mut hasher);
        tab.view.top_line().hash(&mut hasher);
    }
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor_view::EditorView;
    use bracket_core::encoding::TextEncoding;

    fn temp_files(name: &str) -> SessionFiles {
        let dir =
            std::env::temp_dir().join(format!("bracket-ui-session-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        SessionFiles {
            file: dir.join("session.toml"),
            backup_dir: dir.join("backups"),
        }
    }

    fn tab(id: u64, doc: Document) -> Tab {
        Tab::new(id, doc, EditorView::for_tab(id, 14.0), 1)
    }

    const DEFAULTS: (LineEnding, TextEncoding) = (LineEnding::CrLf, TextEncoding::UTF8);

    #[test]
    fn tabs_and_unsaved_text_come_back() {
        let files = temp_files("round-trip");
        let dir = files.file.parent().unwrap().to_path_buf();
        let clean_path = dir.join("clean.cs");
        let dirty_path = dir.join("dirty.txt");
        fs::write(&clean_path, "class A {}\r\n").unwrap();
        fs::write(&dirty_path, "on disk\r\n").unwrap();

        let load = |path: &PathBuf| {
            Document::from_loaded(
                path.clone(),
                document::load_text(path).unwrap(),
                LineEnding::CrLf,
            )
        };
        let mut clean = load(&clean_path);
        clean
            .buffer
            .set_selections(Selections::single(Selection::new(2, 5)));
        let mut dirty = load(&dirty_path);
        dirty.buffer.insert("new ");
        let mut untitled = Document::new_empty(LineEnding::Lf, TextEncoding::UTF8);
        untitled.buffer.insert("draft");
        let empty = Document::new_empty(LineEnding::CrLf, TextEncoding::UTF8);
        let tabs = vec![
            tab(0, clean),
            tab(1, dirty),
            tab(2, untitled),
            tab(3, empty),
        ];

        let mut keeper = SessionKeeper::new(files.clone(), &Session::default());
        keeper.set_project(Some(dir.clone()));
        keeper.save_now(&tabs, 1).unwrap();

        let (session, error) = session::load_previous(&files);
        assert_eq!(error, None);
        assert_eq!(session.project.as_deref(), Some(dir.as_path()));
        assert_eq!(session.tabs.len(), 3, "the empty untitled tab is not kept");
        assert_eq!(session.active, 1);

        let restored: Vec<Document> = session
            .tabs
            .iter()
            .map(|entry| restore_document(entry, &files, DEFAULTS, 4).unwrap())
            .collect();
        assert!(!restored[0].is_dirty());
        assert_eq!(
            *restored[0].buffer.selections().primary(),
            Selection::new(2, 5)
        );
        assert!(restored[1].is_dirty());
        assert_eq!(restored[1].buffer.text().to_string(), "new on disk\r\n");
        assert_eq!(restored[2].buffer.text().to_string(), "draft");
        assert_eq!(restored[2].line_ending(), LineEnding::Lf);
        assert_eq!(restored[2].path(), None);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn backups_are_rewritten_only_when_the_text_changes_and_removed_when_saved() {
        let files = temp_files("backups");
        let mut untitled = Document::new_empty(LineEnding::CrLf, TextEncoding::UTF8);
        untitled.buffer.insert("one");
        let mut tabs = vec![tab(7, untitled)];
        let mut keeper = SessionKeeper::new(files.clone(), &Session::default());

        let (snapshot, _) = keeper.snapshot(&tabs, 0);
        assert_eq!(snapshot.backups.len(), 1);
        keeper.save_now(&tabs, 0).unwrap();
        let (snapshot, _) = keeper.snapshot(&tabs, 0);
        assert!(
            snapshot.backups.is_empty(),
            "unchanged text is not written again"
        );

        tabs[0].doc.buffer.insert(" two");
        let (snapshot, _) = keeper.snapshot(&tabs, 0);
        assert_eq!(snapshot.backups.len(), 1);

        // Saved to a file: the backup is no longer needed.
        let path = files.file.parent().unwrap().join("saved.txt");
        tabs[0].doc.set_path(path);
        let request = tabs[0].doc.save_request().unwrap();
        request.write(&document::RenameReplacer).unwrap();
        tabs[0].doc.mark_saved(request.version, request.encoding);
        keeper.save_now(&tabs, 0).unwrap();
        let left = fs::read_dir(&files.backup_dir).unwrap().count();
        assert_eq!(left, 0);
        fs::remove_dir_all(files.file.parent().unwrap()).unwrap();
    }

    #[test]
    fn tabs_never_activated_are_saved_as_they_were_restored() {
        let files = temp_files("unloaded");
        let entry = SessionTab {
            path: Some(PathBuf::from(r"C:\src\big.log")),
            anchor: 7,
            head: 9,
            top_line: 120,
            ..SessionTab::default()
        };
        let mut waiting = tab(4, Document::new_empty(LineEnding::CrLf, TextEncoding::UTF8));
        waiting.unloaded = Some(entry.clone());
        let keeper = SessionKeeper::new(files.clone(), &Session::default());
        let (snapshot, _) = keeper.snapshot(&[waiting], 0);
        assert_eq!(snapshot.session.tabs, [entry]);
        fs::remove_dir_all(files.file.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_missing_file_with_unsaved_text_keeps_its_path() {
        let files = temp_files("missing-file");
        fs::create_dir_all(&files.backup_dir).unwrap();
        fs::write(files.backup("1-1.txt"), "kept").unwrap();
        let path = files.file.parent().unwrap().join("deleted.txt");
        let entry = SessionTab {
            path: Some(path.clone()),
            unsaved: Some(Unsaved {
                backup: "1-1.txt".to_owned(),
                encoding: TextEncoding::UTF8,
                line_ending: LineEnding::CrLf,
            }),
            ..SessionTab::default()
        };
        let doc = restore_document(&entry, &files, DEFAULTS, 4).unwrap();
        assert_eq!(doc.path(), Some(path.as_path()));
        assert_eq!(doc.buffer.text().to_string(), "kept");
        assert!(doc.is_dirty());

        let gone = SessionTab {
            path: Some(path),
            ..SessionTab::default()
        };
        assert!(restore_document(&gone, &files, DEFAULTS, 4).is_err());
        fs::remove_dir_all(files.file.parent().unwrap()).unwrap();
    }
}
