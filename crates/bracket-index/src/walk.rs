//! Walking a project folder: which files and folders belong to it.
//!
//! The rules are ripgrep's (the `ignore` crate): `.gitignore` files (also outside a Git
//! repository), `.ignore` files and the project's own exclusions, written like `.gitignore`
//! lines. Hidden files are included, but version control folders (`.git`, `.svn`, `.hg`) never
//! are. The same rules serve the whole-project walk (the file index) and listing one folder (the
//! tree).

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
    let files = Mutex::new(Vec::new());
    builder(root, root, options)?.build_parallel().run(|| {
        let files = &files;
        Box::new(move |entry| {
            if cancel.is_cancelled() {
                return WalkState::Quit;
            }
            let Ok(entry) = entry else {
                return WalkState::Continue; // unreadable: skipped
            };
            if entry.file_type().is_some_and(|t| t.is_file())
                && let Ok(relative) = entry.path().strip_prefix(root)
            {
                let relative = relative.to_string_lossy().into_owned();
                files
                    .lock()
                    .expect("no panics while holding it")
                    .push(relative);
            }
            WalkState::Continue
        })
    });
    if cancel.is_cancelled() {
        return Ok(Err(Cancelled));
    }
    let mut files = files.into_inner().expect("no panics while holding it");
    files.sort_unstable_by_key(|f| f.to_lowercase());
    Ok(Ok(FileIndex {
        root: root.to_path_buf(),
        files,
    }))
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
