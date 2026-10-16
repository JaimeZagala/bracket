//! Watching open files for changes made by other programs.
//!
//! A thin layer over `notify` (`ReadDirectoryChangesW` on Windows). It only reports which path
//! changed; deciding whether the file really differs (and reading it) is the caller's job, on its
//! own threads.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

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
}
