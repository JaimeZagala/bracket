//! Recently opened files and folders, newest first, kept in `recent.toml` in the data folder.
//!
//! ```toml
//! files = ['C:\src\Demo\Program.cs', 'C:\notes.txt']
//! folders = ['C:\src\Demo']
//! ```
//!
//! Reading never fails (a missing or damaged file is an empty list). Entries that no longer
//! exist are left in the file and hidden where the list is shown.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use bracket_core::document::{RenameReplacer, write_bytes_atomic};
use toml_edit::{Array, DocumentMut, Item, value};

pub const MAX_FILES: usize = 15;
pub const MAX_FOLDERS: usize = 10;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Recent {
    pub files: Vec<PathBuf>,
    pub folders: Vec<PathBuf>,
}

impl Recent {
    pub fn load(path: &Path) -> Self {
        fs::read_to_string(path)
            .map(|text| Self::parse(&text))
            .unwrap_or_default()
    }

    pub fn parse(text: &str) -> Self {
        let Ok(doc) = text.parse::<DocumentMut>() else {
            return Self::default();
        };
        let list = |key: &str, max: usize| -> Vec<PathBuf> {
            let mut paths: Vec<PathBuf> = Vec::new();
            let items = doc.get(key).and_then(Item::as_array);
            for item in items.into_iter().flatten() {
                if let Some(path) = item.as_str().filter(|p| !p.is_empty()) {
                    let path = PathBuf::from(path);
                    if !paths.iter().any(|p| same_path(p, &path)) {
                        paths.push(path);
                    }
                }
            }
            paths.truncate(max);
            paths
        };
        Self {
            files: list("files", MAX_FILES),
            folders: list("folders", MAX_FOLDERS),
        }
    }

    pub fn to_toml(&self) -> String {
        let array = |paths: &[PathBuf]| {
            let mut array = Array::new();
            for path in paths {
                array.push(path.to_string_lossy().as_ref());
            }
            value(array)
        };
        let mut doc = DocumentMut::new();
        doc.insert("files", array(&self.files));
        doc.insert("folders", array(&self.folders));
        format!("# Recently opened files and folders, newest first. Written by Bracket.\n{doc}")
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        write_bytes_atomic(path, self.to_toml().as_bytes(), &RenameReplacer)
    }

    /// Puts `path` first in the files, without duplicates. Returns whether the list changed.
    pub fn add_file(&mut self, path: &Path) -> bool {
        push_front(&mut self.files, path, MAX_FILES)
    }

    /// Puts `path` first in the folders, without duplicates. Returns whether the list changed.
    pub fn add_folder(&mut self, path: &Path) -> bool {
        push_front(&mut self.folders, path, MAX_FOLDERS)
    }

    /// Forgets `path` (it could not be opened). Returns whether the list changed.
    pub fn remove(&mut self, path: &Path) -> bool {
        let before = self.files.len() + self.folders.len();
        self.files.retain(|p| !same_path(p, path));
        self.folders.retain(|p| !same_path(p, path));
        before != self.files.len() + self.folders.len()
    }

    pub fn clear(&mut self) {
        self.files.clear();
        self.folders.clear();
    }
}

fn push_front(list: &mut Vec<PathBuf>, path: &Path, max: usize) -> bool {
    if list.first().is_some_and(|p| same_path(p, path)) {
        return false;
    }
    list.retain(|p| !same_path(p, path));
    list.insert(0, path.to_path_buf());
    list.truncate(max);
    true
}

/// Windows paths are case-insensitive.
fn same_path(a: &Path, b: &Path) -> bool {
    a == b || a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_first_without_duplicates() {
        let mut recent = Recent::default();
        assert!(recent.add_file(Path::new(r"C:\a.cs")));
        assert!(recent.add_file(Path::new(r"C:\b.cs")));
        assert!(
            recent.add_file(Path::new(r"c:\A.CS")),
            "same file, other case"
        );
        assert_eq!(
            recent.files,
            [PathBuf::from(r"c:\A.CS"), PathBuf::from(r"C:\b.cs")]
        );
        assert!(!recent.add_file(Path::new(r"C:\a.cs")), "already first");
    }

    #[test]
    fn lists_are_capped() {
        let mut recent = Recent::default();
        for i in 0..40 {
            recent.add_file(&PathBuf::from(format!(r"C:\f{i}.txt")));
            recent.add_folder(&PathBuf::from(format!(r"C:\d{i}")));
        }
        assert_eq!(recent.files.len(), MAX_FILES);
        assert_eq!(recent.folders.len(), MAX_FOLDERS);
        assert_eq!(recent.files[0], PathBuf::from(r"C:\f39.txt"));
    }

    #[test]
    fn removing_and_clearing() {
        let mut recent = Recent::default();
        recent.add_file(Path::new(r"C:\a.cs"));
        recent.add_folder(Path::new(r"C:\src"));
        assert!(recent.remove(Path::new(r"C:\SRC")));
        assert!(!recent.remove(Path::new(r"C:\other")));
        assert_eq!(recent.folders.len(), 0);
        recent.clear();
        assert_eq!(recent, Recent::default());
    }

    #[test]
    fn saving_and_loading() {
        let dir = std::env::temp_dir().join(format!("bracket-recent-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("recent.toml");
        assert_eq!(Recent::load(&path), Recent::default(), "missing file");

        let mut recent = Recent::default();
        recent.add_file(Path::new(r"C:\src\Demo\Program.cs"));
        recent.add_folder(Path::new(r"C:\src\Demo"));
        recent.save(&path).unwrap();
        assert_eq!(Recent::load(&path), recent);

        fs::write(&path, "files = [").unwrap();
        assert_eq!(Recent::load(&path), Recent::default(), "damaged file");
        fs::remove_dir_all(&dir).unwrap();
    }
}
