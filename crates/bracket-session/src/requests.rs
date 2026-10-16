//! Requests to open files: from the command line (`file:line:column`) and from a second Bracket
//! started while one is already running. The second one writes its request to the `requests`
//! folder of the data folder and exits; the running one picks it up and opens the files.
//!
//! ```toml
//! [[open]]
//! path = 'C:\src\Program.cs'
//! line = 12
//! column = 5
//! ```

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bracket_core::document::{RenameReplacer, write_bytes_atomic};
use toml_edit::{ArrayOfTables, DocumentMut, Item, Table, value};

/// A file to open, optionally at a position (1-based line and column, in characters).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenRequest {
    pub path: PathBuf,
    pub line: Option<usize>,
    pub column: Option<usize>,
}

impl OpenRequest {
    pub fn file(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            line: None,
            column: None,
        }
    }

    /// `file`, `file:line` or `file:line:column`. Windows file names cannot contain `:` except
    /// after the drive letter, so a trailing `:digits` is always a position.
    pub fn parse(arg: &OsStr) -> Self {
        let Some(text) = arg.to_str() else {
            return Self::file(arg);
        };
        let (rest, last) = split_number(text);
        let Some(last) = last else {
            return Self::file(text);
        };
        match split_number(rest) {
            (path, Some(line)) => Self {
                path: PathBuf::from(path),
                line: Some(line),
                column: Some(last),
            },
            (path, None) => Self {
                path: PathBuf::from(path),
                line: Some(last),
                column: None,
            },
        }
    }
}

/// Splits `text` into the part before a trailing `:number` and the number. A bare drive letter
/// (`C:12`) is not split: it is a path relative to that drive.
fn split_number(text: &str) -> (&str, Option<usize>) {
    let Some((before, after)) = text.rsplit_once(':') else {
        return (text, None);
    };
    let is_number = !after.is_empty() && after.bytes().all(|b| b.is_ascii_digit());
    if !is_number || before.chars().count() <= 1 {
        return (text, None);
    }
    match after.parse() {
        Ok(number) => (before, Some(number)),
        Err(_) => (text, None),
    }
}

/// Writes a request for the running Bracket. Paths must already be absolute (the running
/// Bracket has another working folder). Returns the request file, which the running Bracket
/// deletes once it has read it.
pub fn write_request(dir: &Path, requests: &[OpenRequest]) -> io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let file = dir.join(format!("{millis}-{}.toml", std::process::id()));
    write_bytes_atomic(&file, to_toml(requests).as_bytes(), &RenameReplacer)?;
    Ok(file)
}

fn to_toml(requests: &[OpenRequest]) -> String {
    let mut doc = DocumentMut::new();
    let mut open = ArrayOfTables::new();
    for request in requests {
        let mut table = Table::new();
        table.insert("path", value(request.path.to_string_lossy().as_ref()));
        if let Some(line) = request.line {
            table.insert("line", value(line as i64));
        }
        if let Some(column) = request.column {
            table.insert("column", value(column as i64));
        }
        open.push(table);
    }
    doc.insert("open", Item::ArrayOfTables(open));
    doc.to_string()
}

fn parse(text: &str) -> Vec<OpenRequest> {
    let Ok(doc) = text.parse::<DocumentMut>() else {
        return Vec::new();
    };
    let number = |table: &Table, key: &str| {
        table
            .get(key)
            .and_then(Item::as_integer)
            .and_then(|v| usize::try_from(v).ok())
    };
    let Some(open) = doc.get("open").and_then(Item::as_array_of_tables) else {
        return Vec::new();
    };
    open.iter()
        .filter_map(|table| {
            let path = table.get("path")?.as_str().filter(|p| !p.is_empty())?;
            Some(OpenRequest {
                path: PathBuf::from(path),
                line: number(table, "line"),
                column: number(table, "column"),
            })
        })
        .collect()
}

/// Requests older than this are left over from a run that ended before reading them: they are
/// deleted without opening anything.
pub const MAX_AGE: Duration = Duration::from_secs(30);

/// Reads and deletes the pending requests, oldest first. `None` if there were none; an empty
/// list means "just bring the window to the front".
pub fn take_requests(dir: &Path) -> Option<Vec<OpenRequest>> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .ok()?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| {
            let name = path.file_name().and_then(OsStr::to_str).unwrap_or("");
            // Files starting with a dot are atomic writes still in progress.
            !name.starts_with('.') && name.ends_with(".toml")
        })
        .collect();
    if files.is_empty() {
        return None;
    }
    files.sort();
    let now = SystemTime::now();
    let mut requests = Vec::new();
    let mut found = false;
    for file in files {
        let fresh = fs::metadata(&file)
            .and_then(|m| m.modified())
            .is_ok_and(|modified| now.duration_since(modified).unwrap_or_default() < MAX_AGE);
        if fresh && let Ok(text) = fs::read_to_string(&file) {
            requests.extend(parse(&text));
            found = true;
        }
        let _ = fs::remove_file(&file);
    }
    found.then_some(requests)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(arg: &str) -> (PathBuf, Option<usize>, Option<usize>) {
        let request = OpenRequest::parse(OsStr::new(arg));
        (request.path, request.line, request.column)
    }

    #[test]
    fn positions_are_split_from_the_path() {
        assert_eq!(parsed(r"C:\src\a.cs"), (r"C:\src\a.cs".into(), None, None));
        assert_eq!(
            parsed(r"C:\src\a.cs:12"),
            (r"C:\src\a.cs".into(), Some(12), None)
        );
        assert_eq!(
            parsed(r"C:\src\a.cs:12:5"),
            (r"C:\src\a.cs".into(), Some(12), Some(5))
        );
        assert_eq!(parsed("a.cs:7"), ("a.cs".into(), Some(7), None));
    }

    #[test]
    fn things_that_are_not_positions_stay_in_the_path() {
        assert_eq!(
            parsed("C:12"),
            ("C:12".into(), None, None),
            "drive-relative"
        );
        assert_eq!(parsed("a.cs:"), ("a.cs:".into(), None, None));
        assert_eq!(parsed("a.cs:x"), ("a.cs:x".into(), None, None));
        assert_eq!(parsed("a.cs:-3"), ("a.cs:-3".into(), None, None));
        assert_eq!(
            parsed("a.cs:99999999999999999999999"),
            ("a.cs:99999999999999999999999".into(), None, None)
        );
    }

    #[test]
    fn requests_are_handed_over_once() {
        let dir =
            std::env::temp_dir().join(format!("bracket-requests-{}-once", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(take_requests(&dir), None, "no folder yet");

        let requests = vec![
            OpenRequest {
                path: r"C:\src\a.cs".into(),
                line: Some(3),
                column: Some(9),
            },
            OpenRequest::file(r"C:\src\b.json"),
        ];
        let file = write_request(&dir, &requests).unwrap();
        assert!(file.exists());
        assert_eq!(take_requests(&dir), Some(requests));
        assert!(!file.exists(), "a request is deleted once read");
        assert_eq!(take_requests(&dir), None);

        write_request(&dir, &[]).unwrap();
        assert_eq!(
            take_requests(&dir),
            Some(Vec::new()),
            "just focus the window"
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}
