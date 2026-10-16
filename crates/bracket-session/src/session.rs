//! The recoverable session: which tabs were open, where the caret was and the text of documents
//! with unsaved changes, so closing Bracket (or a crash) loses nothing.
//!
//! `session.toml` lists the tabs. The text of each modified document is kept in its own UTF-8
//! file in the `backups` folder, written before the list that refers to it, so the list never
//! points to a missing or half-written backup.
//!
//! ```toml
//! format_version = 1
//! active = 1
//!
//! [[tab]]
//! path = 'C:\src\Program.cs'
//! anchor = 120
//! head = 120
//! top_line = 0
//!
//! [[tab]]             # untitled, with unsaved text
//! backup = "1760600000000-3.txt"
//! encoding = "utf-8"
//! line_ending = "crlf"
//! ```

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use bracket_core::LineEnding;
use bracket_core::Rope;
use bracket_core::document::{RenameReplacer, write_atomic, write_bytes_atomic};
use bracket_core::encoding::TextEncoding;
use toml_edit::{ArrayOfTables, DocumentMut, Item, Table, value};

use crate::config::{line_ending_code, line_ending_from_code};

pub const FORMAT_VERSION: i64 = 1;
/// A session that cannot be read is moved to this file (next to it), so it is not overwritten.
pub const DAMAGED_FILE: &str = "session.damaged.toml";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Session {
    pub tabs: Vec<SessionTab>,
    /// Index of the active tab.
    pub active: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionTab {
    /// File shown in the tab; `None` for an untitled document (which then has a backup).
    pub path: Option<PathBuf>,
    /// Encoding picked with "reopen with encoding"; `None` detects it again.
    pub read_encoding: Option<TextEncoding>,
    /// Primary selection, in chars.
    pub anchor: usize,
    pub head: usize,
    /// First visible line.
    pub top_line: usize,
    pub unsaved: Option<Unsaved>,
}

/// Where the unsaved text of a tab is kept and how it will be saved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unsaved {
    /// File name inside the backups folder.
    pub backup: String,
    /// Encoding for the next save.
    pub encoding: TextEncoding,
    /// Line ending for new lines.
    pub line_ending: LineEnding,
}

impl Session {
    /// Backup files the session refers to.
    pub fn backups(&self) -> impl Iterator<Item = &str> {
        self.tabs
            .iter()
            .filter_map(|tab| tab.unsaved.as_ref().map(|u| u.backup.as_str()))
    }
}

/// Where the session is stored: `session.toml` and the `backups` folder next to it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionFiles {
    pub file: PathBuf,
    pub backup_dir: PathBuf,
}

impl SessionFiles {
    pub fn backup(&self, name: &str) -> PathBuf {
        self.backup_dir.join(name)
    }
}

/// Reads the session. A missing file is an empty session; a damaged one is an error, and the
/// caller should not overwrite it blindly.
pub fn load(path: &Path) -> io::Result<Session> {
    match fs::read_to_string(path) {
        Ok(text) => parse(&text),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(Session::default()),
        Err(err) => Err(err),
    }
}

/// Reads the session left by the previous run. A damaged file is moved aside to
/// [`DAMAGED_FILE`] (its backups are left alone) and its error comes back with an empty session.
pub fn load_previous(files: &SessionFiles) -> (Session, Option<String>) {
    match load(&files.file) {
        Ok(session) => (session, None),
        Err(err) => {
            let _ = fs::rename(&files.file, files.file.with_file_name(DAMAGED_FILE));
            // The first line is enough for the status bar (TOML errors span several).
            let error = err
                .to_string()
                .lines()
                .next()
                .unwrap_or_default()
                .to_owned();
            (Session::default(), Some(error))
        }
    }
}

/// Parses `session.toml`. Tabs with invalid values are skipped; a syntax error or an unknown
/// format version is an error.
pub fn parse(text: &str) -> io::Result<Session> {
    let invalid = |message: String| io::Error::new(io::ErrorKind::InvalidData, message);
    let doc: DocumentMut = text.parse().map_err(|err| invalid(format!("{err}")))?;
    let version = doc.get("format_version").and_then(Item::as_integer);
    if version != Some(FORMAT_VERSION) {
        return Err(invalid(format!(
            "unsupported session format version: {version:?}"
        )));
    }
    let mut session = Session::default();
    let active = doc
        .get("active")
        .and_then(Item::as_integer)
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(0);
    let mut active_index = None;
    if let Some(tabs) = doc.get("tab").and_then(Item::as_array_of_tables) {
        for (index, table) in tabs.iter().enumerate() {
            if let Some(tab) = parse_tab(table) {
                if index == active {
                    active_index = Some(session.tabs.len());
                }
                session.tabs.push(tab);
            }
        }
    }
    session.active = active_index.unwrap_or(0);
    Ok(session)
}

fn parse_tab(table: &Table) -> Option<SessionTab> {
    let position = |key: &str| {
        table
            .get(key)
            .and_then(Item::as_integer)
            .and_then(|v| usize::try_from(v).ok())
            .unwrap_or(0)
    };
    let path = table
        .get("path")
        .and_then(Item::as_str)
        .filter(|p| !p.is_empty())
        .map(PathBuf::from);
    let read_encoding = match table.get("read_encoding") {
        Some(item) => Some(item.as_str().and_then(TextEncoding::from_code)?),
        None => None,
    };
    let unsaved = match table.get("backup") {
        Some(item) => {
            let backup = item.as_str().filter(|name| is_backup_name(name))?;
            Some(Unsaved {
                backup: backup.to_owned(),
                encoding: table
                    .get("encoding")
                    .and_then(Item::as_str)
                    .and_then(TextEncoding::from_code)?,
                line_ending: table
                    .get("line_ending")
                    .and_then(Item::as_str)
                    .and_then(line_ending_from_code)?,
            })
        }
        None => None,
    };
    // A tab with neither a file nor saved text has nothing to restore.
    if path.is_none() && unsaved.is_none() {
        return None;
    }
    Some(SessionTab {
        path,
        read_encoding,
        anchor: position("anchor"),
        head: position("head"),
        top_line: position("top_line"),
        unsaved,
    })
}

/// Backup names are plain file names written by Bracket: never a path that could point
/// outside the backups folder.
fn is_backup_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
        && !name.starts_with('.')
}

pub fn to_toml(session: &Session) -> String {
    let mut doc = DocumentMut::new();
    doc.insert("format_version", value(FORMAT_VERSION));
    doc.insert("active", value(session.active as i64));
    let mut tabs = ArrayOfTables::new();
    for tab in &session.tabs {
        let mut table = Table::new();
        if let Some(path) = &tab.path {
            table.insert("path", value(path.to_string_lossy().as_ref()));
        }
        if let Some(encoding) = tab.read_encoding {
            table.insert("read_encoding", value(encoding.code()));
        }
        table.insert("anchor", value(tab.anchor as i64));
        table.insert("head", value(tab.head as i64));
        table.insert("top_line", value(tab.top_line as i64));
        if let Some(unsaved) = &tab.unsaved {
            table.insert("backup", value(unsaved.backup.as_str()));
            table.insert("encoding", value(unsaved.encoding.code()));
            table.insert("line_ending", value(line_ending_code(unsaved.line_ending)));
        }
        tabs.push(table);
    }
    doc.insert("tab", Item::ArrayOfTables(tabs));
    format!("# Bracket session, written by Bracket when it closes and every few seconds.\n{doc}")
}

/// The session plus the backup texts that changed since they were last written.
pub struct Snapshot {
    pub session: Session,
    /// Backup file name and its text.
    pub backups: Vec<(String, Rope)>,
    /// Backup files that may be deleted once no longer referenced (written by this run or
    /// restored from the previous session). Other files in the folder are never touched.
    pub owned: HashSet<String>,
}

/// Writes the backups, then the session that refers to them, then deletes the owned backups
/// that are no longer referenced. Returns the backups deleted.
pub fn save(files: &SessionFiles, snapshot: &Snapshot) -> io::Result<Vec<String>> {
    if !snapshot.backups.is_empty() {
        fs::create_dir_all(&files.backup_dir)?;
    }
    for (name, text) in &snapshot.backups {
        write_atomic(&files.backup(name), text, false, &RenameReplacer)?;
    }
    if let Some(dir) = files.file.parent() {
        fs::create_dir_all(dir)?;
    }
    let toml = to_toml(&snapshot.session);
    write_bytes_atomic(&files.file, toml.as_bytes(), &RenameReplacer)?;

    let referenced: HashSet<&str> = snapshot.session.backups().collect();
    let mut deleted = Vec::new();
    for name in &snapshot.owned {
        if !referenced.contains(name.as_str()) {
            match fs::remove_file(files.backup(name)) {
                Ok(()) => deleted.push(name.clone()),
                Err(err) if err.kind() == io::ErrorKind::NotFound => deleted.push(name.clone()),
                // Left for the next save; the session is already written.
                Err(_) => {}
            }
        }
    }
    Ok(deleted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_files(name: &str) -> SessionFiles {
        let dir =
            std::env::temp_dir().join(format!("bracket-session-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        SessionFiles {
            file: dir.join("session.toml"),
            backup_dir: dir.join("backups"),
        }
    }

    fn sample() -> Session {
        Session {
            tabs: vec![
                SessionTab {
                    path: Some(PathBuf::from(r"C:\src\Program.cs")),
                    read_encoding: None,
                    anchor: 10,
                    head: 14,
                    top_line: 3,
                    unsaved: None,
                },
                SessionTab {
                    path: Some(PathBuf::from(r"C:\src\legacy.txt")),
                    read_encoding: Some(TextEncoding::from_code("windows-1252").unwrap()),
                    anchor: 0,
                    head: 0,
                    top_line: 0,
                    unsaved: Some(Unsaved {
                        backup: "1-2.txt".to_owned(),
                        encoding: TextEncoding::UTF8_BOM,
                        line_ending: LineEnding::Lf,
                    }),
                },
                SessionTab {
                    unsaved: Some(Unsaved {
                        backup: "1-3.txt".to_owned(),
                        encoding: TextEncoding::UTF8,
                        line_ending: LineEnding::CrLf,
                    }),
                    ..SessionTab::default()
                },
            ],
            active: 1,
        }
    }

    #[test]
    fn a_session_survives_a_round_trip() {
        let session = sample();
        assert_eq!(parse(&to_toml(&session)).unwrap(), session);
    }

    #[test]
    fn a_missing_file_is_an_empty_session() {
        let files = temp_files("missing");
        assert_eq!(load(&files.file).unwrap(), Session::default());
    }

    #[test]
    fn damaged_or_unknown_sessions_are_errors() {
        assert!(parse("tab = [").is_err());
        assert!(parse("format_version = 99").is_err());
        assert!(parse("").is_err());
    }

    #[test]
    fn a_damaged_session_is_moved_aside() {
        let files = temp_files("damaged");
        fs::create_dir_all(files.file.parent().unwrap()).unwrap();
        fs::write(&files.file, "tab = [").unwrap();
        let (session, error) = load_previous(&files);
        assert_eq!(session, Session::default());
        let error = error.unwrap();
        assert!(
            !error.is_empty() && !error.contains('\n'),
            "one line: {error}"
        );
        assert!(!files.file.exists());
        let aside = files.file.with_file_name(DAMAGED_FILE);
        assert_eq!(fs::read_to_string(&aside).unwrap(), "tab = [");
        fs::remove_dir_all(files.file.parent().unwrap()).unwrap();
    }

    #[test]
    fn invalid_tabs_are_skipped_and_the_active_one_follows() {
        let text = r#"
format_version = 1
active = 2

[[tab]]
path = 'C:\a.txt'

[[tab]]
backup = "../../evil.txt"
encoding = "utf-8"
line_ending = "crlf"

[[tab]]
path = 'C:\c.txt'
"#;
        let session = parse(text).unwrap();
        let paths: Vec<_> = session.tabs.iter().map(|t| t.path.clone()).collect();
        assert_eq!(
            paths,
            [
                Some(PathBuf::from(r"C:\a.txt")),
                Some(PathBuf::from(r"C:\c.txt"))
            ]
        );
        assert_eq!(session.active, 1);
    }

    #[test]
    fn backup_names_cannot_leave_the_folder() {
        assert!(is_backup_name("1760600000000-3.txt"));
        assert!(!is_backup_name(r"..\x.txt"));
        assert!(!is_backup_name("a/b.txt"));
        assert!(!is_backup_name(".hidden"));
        assert!(!is_backup_name(""));
    }

    #[test]
    fn saving_writes_backups_and_removes_only_owned_unreferenced_ones() {
        let files = temp_files("save");
        fs::create_dir_all(&files.backup_dir).unwrap();
        fs::write(files.backup("0-9.txt"), "old run").unwrap();
        fs::write(files.backup("foreign.txt"), "not ours").unwrap();

        let session = sample();
        let snapshot = Snapshot {
            session: session.clone(),
            backups: vec![
                ("1-2.txt".to_owned(), Rope::from_str("héllo\n")),
                ("1-3.txt".to_owned(), Rope::from_str("untitled")),
            ],
            owned: ["0-9.txt", "1-2.txt", "1-3.txt"].map(String::from).into(),
        };
        let deleted = save(&files, &snapshot).unwrap();

        assert_eq!(deleted, ["0-9.txt"]);
        assert_eq!(load(&files.file).unwrap(), session);
        assert_eq!(
            fs::read_to_string(files.backup("1-2.txt")).unwrap(),
            "héllo\n"
        );
        assert!(!files.backup("0-9.txt").exists());
        assert!(files.backup("foreign.txt").exists(), "unknown files stay");
        fs::remove_dir_all(files.file.parent().unwrap()).unwrap();
    }
}
