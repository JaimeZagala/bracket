//! File operations started from the project tree: creating, renaming and moving to the Recycle
//! Bin, on worker threads, plus checking new names and showing an entry in File Explorer.

use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use eframe::egui;

/// Characters Windows does not allow in file names.
pub const FORBIDDEN: &str = "<>:\"/\\|?*";

/// Device names Windows reserves, with or without an extension.
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Why a name cannot be used for a file or folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameProblem {
    Empty,
    ForbiddenCharacter,
    /// Ends with a space or a dot (Windows drops them).
    BadEnding,
    Reserved,
}

/// Checks a new file or folder name against Windows' rules.
pub fn check_name(name: &str) -> Result<(), NameProblem> {
    if name.trim().is_empty() || name == "." || name == ".." {
        return Err(NameProblem::Empty);
    }
    if name
        .chars()
        .any(|c| FORBIDDEN.contains(c) || c.is_control())
    {
        return Err(NameProblem::ForbiddenCharacter);
    }
    if name.ends_with(' ') || name.ends_with('.') {
        return Err(NameProblem::BadEnding);
    }
    let stem = name.split('.').next().unwrap_or(name).trim_end();
    if RESERVED.iter().any(|r| stem.eq_ignore_ascii_case(r)) {
        return Err(NameProblem::Reserved);
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileOp {
    CreateFile(PathBuf),
    CreateFolder(PathBuf),
    Rename { from: PathBuf, to: PathBuf },
    Trash(PathBuf),
}

impl FileOp {
    /// The entry the operation leaves behind, if any.
    pub fn result_path(&self) -> Option<&Path> {
        match self {
            Self::CreateFile(path) | Self::CreateFolder(path) => Some(path),
            Self::Rename { to, .. } => Some(to),
            Self::Trash(_) => None,
        }
    }

    /// The entry the operation is about, as the user named it.
    pub fn subject(&self) -> &Path {
        match self {
            Self::CreateFile(path) | Self::CreateFolder(path) | Self::Trash(path) => path,
            Self::Rename { from, .. } => from,
        }
    }
}

/// Why an operation failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpError {
    /// Something with that name is already there.
    Exists,
    Other(String),
}

/// Carries out an operation. Never replaces an existing entry.
pub fn run(op: &FileOp) -> Result<(), OpError> {
    let io_error = |err: io::Error| match err.kind() {
        io::ErrorKind::AlreadyExists => OpError::Exists,
        _ => OpError::Other(err.to_string()),
    };
    match op {
        FileOp::CreateFile(path) => OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map(drop)
            .map_err(io_error),
        FileOp::CreateFolder(path) => fs::create_dir(path).map_err(io_error),
        FileOp::Rename { from, to } => {
            // On Windows a rename replaces an existing file. Changing only the case of a name
            // is fine: the "existing" entry is the same one.
            let same = from.as_os_str().eq_ignore_ascii_case(to.as_os_str());
            if !same && fs::symlink_metadata(to).is_ok() {
                return Err(OpError::Exists);
            }
            fs::rename(from, to).map_err(io_error)
        }
        FileOp::Trash(path) => trash::delete(path).map_err(|e| OpError::Other(e.to_string())),
    }
}

/// Opens File Explorer with the entry selected.
pub fn reveal_in_explorer(path: &Path) -> io::Result<()> {
    use std::os::windows::process::CommandExt;
    // Explorer parses its own command line: the path goes in quotes, as one raw argument.
    std::process::Command::new("explorer.exe")
        .raw_arg(format!("/select,\"{}\"", path.display()))
        .spawn()
        .map(drop)
}

/// Runs operations on worker threads and hands back their results.
pub struct FileOps {
    tx: Sender<(FileOp, Result<(), OpError>)>,
    rx: Receiver<(FileOp, Result<(), OpError>)>,
}

impl FileOps {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self { tx, rx }
    }

    pub fn start(&self, ctx: &egui::Context, op: FileOp) {
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result = run(&op);
            let _ = tx.send((op, result));
            ctx.request_repaint();
        });
    }

    /// The operations that have finished since the last call.
    pub fn finished(&self) -> Vec<(FileOp, Result<(), OpError>)> {
        self.rx.try_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bracket-ops-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn names_follow_windows_rules() {
        assert_eq!(check_name("Program.cs"), Ok(()));
        assert_eq!(check_name(".gitignore"), Ok(()));
        assert_eq!(check_name("Ñandú v2.txt"), Ok(()));
        assert_eq!(check_name(""), Err(NameProblem::Empty));
        assert_eq!(check_name("   "), Err(NameProblem::Empty));
        assert_eq!(check_name(".."), Err(NameProblem::Empty));
        assert_eq!(check_name("a/b.cs"), Err(NameProblem::ForbiddenCharacter));
        assert_eq!(
            check_name("what?.txt"),
            Err(NameProblem::ForbiddenCharacter)
        );
        assert_eq!(
            check_name("tab\there"),
            Err(NameProblem::ForbiddenCharacter)
        );
        assert_eq!(check_name("notes."), Err(NameProblem::BadEnding));
        assert_eq!(check_name("notes "), Err(NameProblem::BadEnding));
        assert_eq!(check_name("con"), Err(NameProblem::Reserved));
        assert_eq!(check_name("NUL.txt"), Err(NameProblem::Reserved));
        assert_eq!(check_name("com1.tar.gz"), Err(NameProblem::Reserved));
        assert_eq!(check_name("console.cs"), Ok(()));
    }

    #[test]
    fn creating_never_replaces_an_existing_entry() {
        let dir = temp_dir("create");
        let file = dir.join("a.cs");
        assert_eq!(run(&FileOp::CreateFile(file.clone())), Ok(()));
        fs::write(&file, "kept").unwrap();
        assert_eq!(run(&FileOp::CreateFile(file.clone())), Err(OpError::Exists));
        assert_eq!(fs::read_to_string(&file).unwrap(), "kept");
        assert_eq!(run(&FileOp::CreateFolder(dir.join("src"))), Ok(()));
        assert_eq!(
            run(&FileOp::CreateFolder(dir.join("src"))),
            Err(OpError::Exists)
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn renaming_never_replaces_another_file_but_can_change_case() {
        let dir = temp_dir("rename");
        let (a, b) = (dir.join("a.cs"), dir.join("b.cs"));
        fs::write(&a, "a").unwrap();
        fs::write(&b, "b").unwrap();
        let op = FileOp::Rename {
            from: a.clone(),
            to: b.clone(),
        };
        assert_eq!(run(&op), Err(OpError::Exists));
        assert_eq!(fs::read_to_string(&b).unwrap(), "b");

        let upper = dir.join("A.cs");
        let op = FileOp::Rename {
            from: a.clone(),
            to: upper.clone(),
        };
        assert_eq!(run(&op), Ok(()));
        let names: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(names.contains(&"A.cs".to_owned()));
        fs::remove_dir_all(&dir).unwrap();
    }
}
