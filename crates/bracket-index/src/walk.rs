//! Walking a project folder: which files and folders belong to it.
//!
//! The rules are ripgrep's (the `ignore` crate): `.gitignore` files (also outside a Git
//! repository), `.ignore` files and the project's own exclusions, written like `.gitignore`
//! lines. Hidden files are included, but version control folders (`.git`, `.svn`, `.hg`) never
//! are. The same rules serve the whole-project walk (the file index) and listing one folder (the
//! tree).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use bracket_core::CancelToken;
use ignore::overrides::{Override, OverrideBuilder};
use ignore::{WalkBuilder, WalkState};

/// Folders that are never part of a project.
const VCS_FOLDERS: [&str; 3] = [".git", ".svn", ".hg"];

/// How to walk a project.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WalkOptions {
    /// Extra exclusions, like `.gitignore` lines (`bin/`, `*.log`, `docs/old/**`).
    pub exclude: Vec<String>,
}

/// The files of a project, as paths relative to its root.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileIndex {
    pub root: PathBuf,
    /// Sorted, with `\\` separators.
    pub files: Vec<String>,
    /// The project's folders (not the root), relative to it. Changes in folders that are not
    /// here (ignored ones, like `target\`) are left out of the index.
    pub dirs: HashSet<String>,
}

/// A folder or file in a listing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
}

/// An exclusion pattern that is not valid `.gitignore` syntax.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BadPattern {
    pub pattern: String,
    pub error: String,
}

/// The walk was cancelled before it finished.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cancelled;

fn builder(root: &Path, start: &Path, options: &WalkOptions) -> Result<WalkBuilder, BadPattern> {
    let mut builder = WalkBuilder::new(start);
    builder
        .hidden(false)
        .require_git(false)
        .git_global(false)
        .overrides(exclusions(root, options)?)
        .filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            !VCS_FOLDERS.iter().any(|vcs| name.eq_ignore_ascii_case(vcs))
        });
    Ok(builder)
}

/// The project's exclusions. In `ignore`'s overrides a plain glob means "only these", so every
/// exclusion is written as `!glob`.
fn exclusions(root: &Path, options: &WalkOptions) -> Result<Override, BadPattern> {
    let mut overrides = OverrideBuilder::new(root);
    for pattern in &options.exclude {
        let pattern = pattern.trim();
        if pattern.is_empty() {
            continue;
        }
        overrides
            .add(&format!("!{pattern}"))
            .map_err(|e| BadPattern {
                pattern: pattern.to_owned(),
                error: e.to_string(),
            })?;
    }
    overrides.build().map_err(|e| BadPattern {
        pattern: options.exclude.join(", "),
        error: e.to_string(),
    })
}

/// Lists every file of the project on several threads. Checks `cancel` as it goes.
pub fn walk(
    root: &Path,
    options: &WalkOptions,
    cancel: &CancelToken,
) -> Result<Result<FileIndex, Cancelled>, BadPattern> {
    let Some((mut files, dirs)) = entries_under(root, root, options, cancel)? else {
        return Ok(Err(Cancelled));
    };
    files.sort_unstable_by_key(|f| f.to_lowercase());
    Ok(Ok(FileIndex {
        root: root.to_path_buf(),
        files,
        dirs: dirs.into_iter().collect(),
    }))
}

/// Files and folders found by a walk, relative to the project root.
type Walked = (Vec<String>, Vec<String>);

/// The files and folders under `start` (a folder of the project at `root`, included in the
/// folders unless it is the root), relative to `root`, unsorted. `None` if cancelled.
fn entries_under(
    root: &Path,
    start: &Path,
    options: &WalkOptions,
    cancel: &CancelToken,
) -> Result<Option<Walked>, BadPattern> {
    let found = Mutex::new((Vec::new(), Vec::new()));
    builder(root, start, options)?.build_parallel().run(|| {
        let found = &found;
        Box::new(move |entry| {
            if cancel.is_cancelled() {
                return WalkState::Quit;
            }
            let Ok(entry) = entry else {
                return WalkState::Continue; // unreadable: skipped
            };
            let Some(kind) = entry.file_type() else {
                return WalkState::Continue;
            };
            if let Ok(relative) = entry.path().strip_prefix(root)
                && !relative.as_os_str().is_empty()
                && (kind.is_file() || kind.is_dir())
            {
                let relative = relative.to_string_lossy().into_owned();
                let mut found = found.lock().expect("no panics while holding it");
                if kind.is_file() {
                    found.0.push(relative);
                } else {
                    found.1.push(relative);
                }
            }
            WalkState::Continue
        })
    });
    if cancel.is_cancelled() {
        return Ok(None);
    }
    Ok(Some(
        found.into_inner().expect("no panics while holding it"),
    ))
}

impl FileIndex {
    /// Brings the index up to date after something changed directly inside `dir` (a file or
    /// folder created, deleted or renamed there): the files directly in it are compared with
    /// a new listing, subfolders that appeared are walked and the files of subfolders that
    /// disappeared are removed. Subfolders that are still there are left alone; changes inside
    /// them come with their own folder. A folder the index does not know (an ignored one, or
    /// one outside the project) is left alone: a new folder is found from its parent.
    pub fn resync_dir(&mut self, dir: &Path, options: &WalkOptions) -> Result<(), BadPattern> {
        let Ok(relative) = dir.strip_prefix(&self.root) else {
            return Ok(()); // outside the project
        };
        let relative = relative.to_string_lossy().into_owned();
        if !relative.is_empty() && !self.dirs.contains(&relative) {
            return Ok(());
        }
        let prefix = match relative.as_str() {
            "" => String::new(),
            r => format!("{r}\\"),
        };
        let listing = list_dir(&self.root, dir, options)?;
        let files: HashSet<&str> = listing
            .iter()
            .filter(|e| !e.is_dir)
            .map(|e| e.name.as_str())
            .collect();
        let folders: HashSet<&str> = listing
            .iter()
            .filter(|e| e.is_dir)
            .map(|e| e.name.as_str())
            .collect();

        // Every path starting with the prefix sits in one block of the sorted list.
        let lower = prefix.to_lowercase();
        let start = self.files.partition_point(|f| f.to_lowercase() < lower);
        let end =
            start + self.files[start..].partition_point(|f| f.to_lowercase().starts_with(&lower));
        let mut known_files = HashSet::new();
        let mut block: Vec<String> = Vec::with_capacity(end - start);
        for path in &self.files[start..end] {
            // The same folder written with different case: left as it is.
            let Some(rest) = path.strip_prefix(prefix.as_str()) else {
                block.push(path.clone());
                continue;
            };
            let keep = match rest.split_once('\\') {
                None => {
                    known_files.insert(rest.to_owned());
                    files.contains(rest)
                }
                Some((folder, _)) => folders.contains(folder),
            };
            if keep {
                block.push(path.clone());
            }
        }
        for name in files.iter().filter(|n| !known_files.contains(**n)) {
            block.push(format!("{prefix}{name}"));
        }
        // Subfolders that are gone take their folders with them; new ones are walked.
        let gone: Vec<String> = self
            .dirs
            .iter()
            .filter_map(|d| d.strip_prefix(prefix.as_str()))
            .filter(|rest| !rest.contains('\\') && !folders.contains(rest))
            .map(|rest| format!("{prefix}{rest}"))
            .collect();
        for folder in gone {
            let inside = format!("{folder}\\");
            self.dirs
                .retain(|d| *d != folder && !d.starts_with(&inside));
        }
        let cancel = CancelToken::new();
        for name in folders {
            let folder = format!("{prefix}{name}");
            if self.dirs.contains(&folder) {
                continue;
            }
            if let Some((files, dirs)) =
                entries_under(&self.root, &dir.join(name), options, &cancel)?
            {
                block.extend(files);
                self.dirs.extend(dirs);
            }
        }
        block.sort_unstable_by_key(|f| f.to_lowercase());
        self.files.splice(start..end, block);
        Ok(())
    }
}

/// The folders and files directly inside `dir` (a folder of the project at `root`), folders
/// first, each group sorted by name.
pub fn list_dir(root: &Path, dir: &Path, options: &WalkOptions) -> Result<Vec<Entry>, BadPattern> {
    let mut entries: Vec<Entry> = builder(root, dir, options)?
        .max_depth(Some(1))
        .build()
        .filter_map(Result::ok)
        .filter(|entry| entry.depth() == 1)
        .map(|entry| Entry {
            name: entry.file_name().to_string_lossy().into_owned(),
            is_dir: entry.file_type().is_some_and(|t| t.is_dir()),
            path: entry.into_path(),
        })
        .collect();
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// A small project with ignored files, a hidden file and a Git folder.
    fn project(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("bracket-walk-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for (path, text) in [
            (".gitignore", "bin/\n*.log\n"),
            (".editorconfig", "root = true\n"),
            ("src/Program.cs", "class P {}\n"),
            ("src/App.razor", "<h1/>\n"),
            ("src/old/Legacy.cs", "\n"),
            ("src/.ignore", "old/\n"),
            ("bin/Debug/app.dll", "x"),
            ("build.log", "x"),
            ("docs/readme.md", "x"),
            (".git/HEAD", "ref: refs/heads/main\n"),
            (".git/config", "x"),
        ] {
            let path = root.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        root
    }

    #[test]
    fn the_walk_follows_ignore_files_and_skips_version_control() {
        let root = project("walk");
        let index = walk(&root, &WalkOptions::default(), &CancelToken::new())
            .unwrap()
            .unwrap();
        assert_eq!(
            index.files,
            [
                ".editorconfig",
                ".gitignore",
                r"docs\readme.md",
                r"src\.ignore",
                r"src\App.razor",
                r"src\Program.cs",
            ]
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn exclusions_from_the_configuration_apply() {
        let root = project("exclude");
        let options = WalkOptions {
            exclude: vec!["docs/".into(), "*.razor".into()],
        };
        let index = walk(&root, &options, &CancelToken::new()).unwrap().unwrap();
        assert!(!index.files.iter().any(|f| f.starts_with("docs")));
        assert!(!index.files.iter().any(|f| f.ends_with(".razor")));
        assert!(index.files.contains(&r"src\Program.cs".to_owned()));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_bad_pattern_is_reported() {
        let root = project("bad");
        let options = WalkOptions {
            exclude: vec!["src/[".into()],
        };
        let error = walk(&root, &options, &CancelToken::new()).unwrap_err();
        assert_eq!(error.pattern, "src/[");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_cancelled_walk_returns_nothing() {
        let root = project("cancel");
        let cancel = CancelToken::new();
        cancel.cancel();
        let result = walk(&root, &WalkOptions::default(), &cancel).unwrap();
        assert_eq!(result, Err(Cancelled));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn resyncing_a_folder_follows_what_changed_in_it() {
        let root = project("resync");
        let options = WalkOptions::default();
        let mut index = walk(&root, &options, &CancelToken::new()).unwrap().unwrap();

        // In src: a file added, one deleted, a folder added with files, and one ignored.
        fs::write(root.join(r"src\Startup.cs"), "x").unwrap();
        fs::remove_file(root.join(r"src\App.razor")).unwrap();
        fs::create_dir_all(root.join(r"src\Models\Dto")).unwrap();
        fs::write(root.join(r"src\Models\User.cs"), "x").unwrap();
        fs::write(root.join(r"src\Models\Dto\UserDto.cs"), "x").unwrap();
        fs::write(root.join(r"src\debug.log"), "x").unwrap();
        index.resync_dir(&root.join("src"), &options).unwrap();
        let fresh = walk(&root, &options, &CancelToken::new()).unwrap().unwrap();
        assert_eq!(index.files, fresh.files);
        assert!(
            index
                .files
                .contains(&r"src\Models\Dto\UserDto.cs".to_owned())
        );

        // At the root: a folder renamed.
        fs::rename(root.join("docs"), root.join("Documentation")).unwrap();
        index.resync_dir(&root, &options).unwrap();
        let fresh = walk(&root, &options, &CancelToken::new()).unwrap().unwrap();
        assert_eq!(index.files, fresh.files);

        assert_eq!(index.dirs, fresh.dirs);
        assert!(index.dirs.contains("Documentation"));
        assert!(!index.dirs.contains("docs"));

        // Changes inside an ignored folder stay out of the index.
        fs::write(root.join(r"bin\Debug\new.dll"), "x").unwrap();
        index
            .resync_dir(&root.join(r"bin\Debug"), &options)
            .unwrap();
        assert!(!index.files.iter().any(|f| f.starts_with("bin")));

        // A folder that is gone entirely, and one outside the project.
        fs::remove_dir_all(root.join(r"src\Models")).unwrap();
        index
            .resync_dir(&root.join(r"src\Models"), &options)
            .unwrap();
        index.resync_dir(&root.join("src"), &options).unwrap();
        index
            .resync_dir(Path::new(r"C:\elsewhere"), &options)
            .unwrap();
        assert!(!index.files.iter().any(|f| f.starts_with(r"src\Models")));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn listing_a_folder_uses_the_same_rules() {
        let root = project("list");
        let names = |dir: &Path| -> Vec<(String, bool)> {
            list_dir(&root, dir, &WalkOptions::default())
                .unwrap()
                .into_iter()
                .map(|e| (e.name, e.is_dir))
                .collect()
        };
        assert_eq!(
            names(&root),
            [
                ("docs".to_owned(), true),
                ("src".to_owned(), true),
                (".editorconfig".to_owned(), false),
                (".gitignore".to_owned(), false),
            ],
            "folders first; bin/, *.log and .git are left out"
        );
        assert_eq!(
            names(&root.join("src")),
            [
                (".ignore".to_owned(), false),
                ("App.razor".to_owned(), false),
                ("Program.cs".to_owned(), false),
            ],
            "src/.ignore hides old/"
        );
        fs::remove_dir_all(&root).unwrap();
    }
}
