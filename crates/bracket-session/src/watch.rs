//! Watching open files, and the project folder, for changes made by other programs.
//!
//! A thin layer over `notify` (`ReadDirectoryChangesW` on Windows). It only reports which path
//! changed; deciding whether the file really differs (and reading it) is the caller's job, on its
//! own threads.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use notify::event::ModifyKind;
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

/// Watches a set of files and calls back with the path of each one that changes.
pub struct FileWatcher {
    watcher: RecommendedWatcher,
    watched: HashSet<PathBuf>,
}

impl FileWatcher {
    /// `on_change` runs on the watcher's thread, once per changed path (often several times for
    /// one save: editors write, rename and touch files).
    pub fn new(on_change: impl Fn(PathBuf) + Send + 'static) -> notify::Result<Self> {
        let watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            let Ok(event) = event else { return };
            if matches!(
                event.kind,
                EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) | EventKind::Any
            ) {
                for path in event.paths {
                    on_change(path);
                }
            }
        })?;
        Ok(Self {
            watcher,
            watched: HashSet::new(),
        })
    }

    /// Watches exactly `paths`: new ones are added and the rest are dropped. A path that cannot
    /// be watched (a missing folder, a drive without notifications) is skipped silently and
    /// tried again on the next call.
    pub fn set_paths<'a>(&mut self, paths: impl IntoIterator<Item = &'a Path>) {
        let wanted: HashSet<PathBuf> = paths.into_iter().map(Path::to_path_buf).collect();
        let gone: Vec<PathBuf> = self.watched.difference(&wanted).cloned().collect();
        for path in gone {
            let _ = self.watcher.unwatch(&path);
            self.watched.remove(&path);
        }
        for path in wanted {
            if !self.watched.contains(&path)
                && self
                    .watcher
                    .watch(&path, RecursiveMode::NonRecursive)
                    .is_ok()
            {
                self.watched.insert(path);
            }
        }
    }

    pub fn watched(&self) -> usize {
        self.watched.len()
    }
}

/// Changes inside a project folder, gathered until they settle.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProjectChanges {
    /// Files and folders created, deleted or renamed.
    pub paths: HashSet<PathBuf>,
    /// Files whose content changed. They change neither the tree nor the file index, only what
    /// is read from the files (their symbols).
    pub modified: HashSet<PathBuf>,
    /// Notifications were lost (too many at once): everything should be listed again.
    pub rescan: bool,
}

impl ProjectChanges {
    /// Whether folder listings may have changed: something was created, deleted or renamed, or
    /// notifications were lost.
    pub fn changes_listing(&self) -> bool {
        self.rescan || !self.paths.is_empty()
    }
}

/// One change, as the watcher passes it to the thread that gathers them.
enum Change {
    Listing(PathBuf),
    Content(PathBuf),
    /// Notifications were lost.
    Rescan,
}

/// How long the project must stay quiet before its changes are reported.
pub const PROJECT_QUIET: Duration = Duration::from_millis(200);
/// Changes are reported at least this often, even while they keep coming (during a build).
pub const PROJECT_MAX_WAIT: Duration = Duration::from_secs(1);

/// Watches a project folder and everything inside it. Dropping it stops the watching.
pub struct ProjectWatcher {
    _watcher: RecommendedWatcher,
}

impl ProjectWatcher {
    /// `on_changes` runs on the watcher's own thread with each batch of changes.
    pub fn new(
        root: &Path,
        on_changes: impl Fn(ProjectChanges) + Send + 'static,
    ) -> notify::Result<Self> {
        let (tx, rx) = mpsc::channel::<Change>();
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| match event {
                Ok(event) if event.need_rescan() => {
                    let _ = tx.send(Change::Rescan);
                }
                Ok(event) => {
                    let change: fn(PathBuf) -> Change = if changes_the_listing(event.kind) {
                        Change::Listing
                    } else if changes_the_content(event.kind) {
                        Change::Content
                    } else {
                        return;
                    };
                    for path in event.paths {
                        let _ = tx.send(change(path));
                    }
                }
                Err(_) => {
                    let _ = tx.send(Change::Rescan);
                }
            })?;
        watcher.watch(root, RecursiveMode::Recursive)?;
        thread::Builder::new()
            .name("project-watch".to_owned())
            .spawn(move || gather(&rx, on_changes))
            .map_err(notify::Error::io)?;
        Ok(Self { _watcher: watcher })
    }
}

/// Whether an event can add, remove or rename an entry. On Windows a content change is
/// `Modify(Any)`, and a rename is `Modify(Name)`.
fn changes_the_listing(kind: EventKind) -> bool {
    matches!(
        kind,
        EventKind::Create(_)
            | EventKind::Remove(_)
            | EventKind::Modify(ModifyKind::Name(_))
            | EventKind::Other
    )
}

/// Whether an event changes what a file holds: on Windows, `Modify(Any)`.
fn changes_the_content(kind: EventKind) -> bool {
    matches!(
        kind,
        EventKind::Modify(ModifyKind::Any | ModifyKind::Data(_))
    )
}

/// Collects changes until none arrive for [`PROJECT_QUIET`] (or [`PROJECT_MAX_WAIT`] passes),
/// then reports them. Ends when the watcher is dropped.
fn gather(rx: &Receiver<Change>, on_changes: impl Fn(ProjectChanges)) {
    let add = |changes: &mut ProjectChanges, change: Change| match change {
        Change::Listing(path) => {
            changes.paths.insert(path);
        }
        Change::Content(path) => {
            changes.modified.insert(path);
        }
        Change::Rescan => changes.rescan = true,
    };
    while let Ok(first) = rx.recv() {
        let mut changes = ProjectChanges::default();
        add(&mut changes, first);
        let started = Instant::now();
        loop {
            let left = PROJECT_MAX_WAIT.saturating_sub(started.elapsed());
            if left.is_zero() {
                break;
            }
            match rx.recv_timeout(PROJECT_QUIET.min(left)) {
                Ok(change) => add(&mut changes, change),
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
        on_changes(changes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::mpsc;
    use std::time::Duration;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bracket-watch-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        // Canonical, so the reported paths can be compared with it.
        dir.canonicalize().unwrap()
    }

    /// Waits until `path` is reported (other paths in the folder may come first).
    fn reported(rx: &mpsc::Receiver<PathBuf>, path: &Path) -> bool {
        let name = path.file_name().unwrap();
        while let Ok(changed) = rx.recv_timeout(Duration::from_secs(5)) {
            if changed.file_name() == Some(name) {
                return true;
            }
        }
        false
    }

    #[test]
    fn changes_to_watched_files_are_reported() {
        let dir = temp_dir("changes");
        let watched = dir.join("watched.txt");
        fs::write(&watched, "one").unwrap();
        let (tx, rx) = mpsc::channel();
        let mut watcher = FileWatcher::new(move |path| {
            let _ = tx.send(path);
        })
        .unwrap();
        watcher.set_paths([watched.as_path()]);
        assert_eq!(watcher.watched(), 1);

        fs::write(&watched, "two").unwrap();
        assert!(reported(&rx, &watched), "a write is reported");
        fs::remove_file(&watched).unwrap();
        assert!(reported(&rx, &watched), "a deletion is reported");

        watcher.set_paths([]);
        assert_eq!(watcher.watched(), 0);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_missing_folder_is_skipped() {
        let (tx, _rx) = mpsc::channel();
        let mut watcher = FileWatcher::new(move |path| {
            let _ = tx.send(path);
        })
        .unwrap();
        watcher.set_paths([Path::new(r"Z:\no\such\folder\file.txt")]);
        assert_eq!(watcher.watched(), 0);
    }

    /// Waits for batches until one contains a path named `name`.
    fn batch_with(rx: &mpsc::Receiver<ProjectChanges>, name: &str) -> Option<ProjectChanges> {
        while let Ok(changes) = rx.recv_timeout(Duration::from_secs(5)) {
            if changes
                .paths
                .iter()
                .any(|p| p.file_name().is_some_and(|n| n == name))
            {
                return Some(changes);
            }
        }
        None
    }

    #[test]
    fn new_deleted_and_renamed_entries_in_the_project_are_reported() {
        let root = temp_dir("project");
        fs::create_dir_all(root.join(r"src\Models")).unwrap();
        let (tx, rx) = mpsc::channel();
        let _watcher = ProjectWatcher::new(&root, move |changes| {
            let _ = tx.send(changes);
        })
        .unwrap();

        fs::write(root.join(r"src\Models\User.cs"), "class User {}").unwrap();
        let changes = batch_with(&rx, "User.cs").expect("a new file deep inside is reported");
        assert!(!changes.rescan);
        assert!(changes.paths.iter().all(|p| p.starts_with(&root)));

        fs::rename(
            root.join(r"src\Models\User.cs"),
            root.join(r"src\Models\Person.cs"),
        )
        .unwrap();
        assert!(
            batch_with(&rx, "Person.cs").is_some(),
            "a rename is reported"
        );

        fs::remove_dir_all(root.join("src")).unwrap();
        assert!(
            batch_with(&rx, "Models").is_some(),
            "a deletion is reported"
        );
        drop(_watcher);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn content_changes_are_reported_apart() {
        let root = temp_dir("content");
        let file = root.join("Program.cs");
        fs::write(&file, "one").unwrap();
        let (tx, rx) = mpsc::channel();
        let watcher = ProjectWatcher::new(&root, move |changes| {
            let _ = tx.send(changes);
        })
        .unwrap();
        fs::write(&file, "two").unwrap();
        let changes = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("a content change is reported");
        assert!(changes.modified.contains(&file));
        assert!(!changes.changes_listing(), "the listing stays as it was");
        drop(watcher);
        fs::remove_dir_all(&root).unwrap();
    }
}
