//! Finding text in every file of a project folder.
//!
//! The files are the project's (the same walk and rules as the tree and Ctrl+P), narrowed by
//! inclusions and exclusions. Each one is decoded the way the editor would open it, so legacy
//! encodings and UTF-16 are searched as text, and searched with the same [`Finder`] as Ctrl+F.
//! Binary files and files over a size limit are skipped. Several threads search at once and
//! the results of each file are handed over as soon as it is done.
//!
//! Replacing in a closed file reads it again, checks it still has the matches the search
//! found, replaces the chosen ones and writes it back atomically in the same encoding (BOM and
//! line breaks included). Files that are read-only, or whose encoding cannot reproduce their
//! bytes exactly, are left alone.

use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use bracket_core::document::{FileReplacer, write_bytes_atomic};
use bracket_core::encoding;
use bracket_core::{CancelToken, Rope};
use bracket_index::{BadPattern, WalkOptions, walk};

use crate::find::{Finder, Replacement};

/// Files larger than this are skipped.
pub const MAX_FILE_SIZE: u64 = 10 << 20;
/// A file is binary if a NUL character appears in its first chars once decoded.
const BINARY_PROBE: usize = 8192;
/// Context shown around a match on a long line, in chars.
const CONTEXT_BEFORE: usize = 60;
const CONTEXT_LENGTH: usize = 250;

/// What to search for, and where.
#[derive(Clone, Debug)]
pub struct FolderQuery {
    pub root: PathBuf,
    pub finder: Finder,
    /// The project's exclusions plus the user's; inclusions narrow the files.
    pub walk: WalkOptions,
    /// Matches reported in total before stopping.
    pub limit: usize,
    /// A replacement template: each match then comes with what would replace it.
    pub replacement: Option<String>,
}

/// One match: where it is and the line it is on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineMatch {
    /// 1-based line and column (in chars), as `file:line:column`.
    pub line: usize,
    pub column: usize,
    /// The line, without its indentation, cut around the match if it is long.
    pub preview: String,
    /// The match within `preview`, in chars.
    pub highlight: Range<usize>,
    /// What would replace the match, when the query has a replacement.
    pub replacement: Option<String>,
}

/// The matches in one file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileMatches {
    pub path: PathBuf,
    /// Relative to the root, with `\` separators.
    pub relative: String,
    pub matches: Vec<LineMatch>,
}

/// How a search ended.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub files_searched: usize,
    /// Binary, too large or unreadable.
    pub files_skipped: usize,
    pub matches: usize,
    /// The limit was reached.
    pub truncated: bool,
    pub cancelled: bool,
}

/// Searches the folder on several threads, calling `found` for each file with matches (from
/// any thread, in no particular order).
pub fn search_folder(
    query: &FolderQuery,
    cancel: &CancelToken,
    found: impl Fn(FileMatches) + Sync,
) -> Result<Summary, BadPattern> {
    let index = match walk(&query.root, &query.walk, cancel)? {
        Ok(index) => index,
        Err(_) => {
            return Ok(Summary {
                cancelled: true,
                ..Summary::default()
            });
        }
    };
    let next = AtomicUsize::new(0);
    let matches = AtomicUsize::new(0);
    let skipped = AtomicUsize::new(0);
    let searched = AtomicUsize::new(0);
    let truncated = AtomicBool::new(false);
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(8)
        .min(index.files.len().max(1));
    // Files are taken in order by whichever thread is free.
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    if cancel.is_cancelled() || truncated.load(Ordering::Relaxed) {
                        return;
                    }
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(relative) = index.files.get(i) else {
                        return;
                    };
                    let path = query.root.join(relative);
                    let Some(text) = read_text(&path) else {
                        skipped.fetch_add(1, Ordering::Relaxed);
                        continue;
                    };
                    searched.fetch_add(1, Ordering::Relaxed);
                    let room = query.limit.saturating_sub(matches.load(Ordering::Relaxed));
                    if room == 0 {
                        truncated.store(true, Ordering::Relaxed);
                        return;
                    }
                    let (mut lines, more) =
                        find_lines(&query.finder, &text, room, query.replacement.as_deref());
                    if more {
                        truncated.store(true, Ordering::Relaxed);
                    }
                    if lines.is_empty() {
                        continue;
                    }
                    // Other threads may have taken some of the room meanwhile: reserve what is
                    // left, all at once, and drop the rest.
                    let mut taken = 0;
                    let _ = matches.try_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
                        taken = lines.len().min(query.limit.saturating_sub(used));
                        Some(used + taken)
                    });
                    if taken < lines.len() {
                        lines.truncate(taken);
                        truncated.store(true, Ordering::Relaxed);
                    }
                    if lines.is_empty() {
                        return;
                    }
                    found(FileMatches {
                        path,
                        relative: relative.clone(),
                        matches: lines,
                    });
                }
            });
        }
    });
    Ok(Summary {
        files_searched: searched.into_inner(),
        files_skipped: skipped.into_inner(),
        matches: matches.into_inner().min(query.limit),
        truncated: truncated.into_inner(),
        cancelled: cancel.is_cancelled(),
    })
}

/// The file as text, decoded like the editor would, or `None` if it is too large, binary or
/// unreadable.
fn read_text(path: &Path) -> Option<String> {
    let size = fs::metadata(path).ok()?.len();
    if size > MAX_FILE_SIZE {
        return None;
    }
    let decoded = encoding::decode(fs::read(path).ok()?);
    let binary = decoded.text.chars().take(BINARY_PROBE).any(|c| c == '\0');
    (!binary).then_some(decoded.text)
}

/// Up to `limit` matches in `text`, with their line, column and preview; and whether there
/// were more.
fn find_lines(
    finder: &Finder,
    text: &str,
    limit: usize,
    template: Option<&str>,
) -> (Vec<LineMatch>, bool) {
    let replacement = template.map(|t| Replacement::new(t, finder, line_break(text)));
    let mut found = Vec::new();
    // Line number and start of the line of the last match, so lines are counted once.
    let (mut line, mut line_start) = (1, 0);
    let mut counted = 0;
    for range in finder.byte_matches(text) {
        if found.len() == limit {
            return (found, true);
        }
        line += text[counted..range.start].matches('\n').count();
        counted = range.start;
        if let Some(newline) = text[line_start..range.start].rfind('\n') {
            line_start += newline + 1;
        }
        let line_end = text[range.start..]
            .find('\n')
            .map_or(text.len(), |i| range.start + i);
        let line_text = text[line_start..line_end].trim_end_matches('\r');
        let column = text[line_start..range.start].chars().count();
        let length = text[range.start..range.end.min(line_end)].chars().count();
        let (preview, highlight) = preview(line_text, column, length);
        found.push(LineMatch {
            line,
            column: column + 1,
            preview,
            highlight,
            replacement: replacement
                .as_ref()
                .and_then(|r| finder.expand_at(text, range.start, r)),
        });
    }
    (found, false)
}

/// The text's line break, for `\n` in replacements: the first one found, CRLF if there is none.
fn line_break(text: &str) -> &'static str {
    match text.find(['\n', '\r']) {
        Some(i) if text[i..].starts_with("\r\n") => "\r\n",
        Some(i) if text[i..].starts_with('\r') => "\r",
        Some(_) => "\n",
        None => "\r\n",
    }
}

/// A closed file to replace in: the matches the search found in it and the ones chosen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileReplace {
    pub path: PathBuf,
    /// How many matches the search found; if the file has a different number now, it changed.
    pub expected: usize,
    /// Indices (in the search's order) of the matches to replace, ascending.
    pub chosen: Vec<usize>,
}

/// What replacing in one file did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileOutcome {
    /// This many matches were replaced and the file was written.
    Replaced(usize),
    ReadOnly,
    /// Its encoding cannot reproduce its bytes exactly.
    Inexact,
    /// It no longer has the matches the search found.
    Changed,
    Failed(String),
}

/// Replaces the chosen matches in a closed file and writes it back atomically, in its own
/// encoding and with its own line breaks.
pub fn replace_in_file(
    finder: &Finder,
    template: &str,
    file: &FileReplace,
    replacer: &dyn FileReplacer,
) -> FileOutcome {
    let failed = |err: std::io::Error| FileOutcome::Failed(err.to_string());
    let metadata = match fs::metadata(&file.path) {
        Ok(metadata) => metadata,
        Err(err) => return failed(err),
    };
    if metadata.permissions().readonly() {
        return FileOutcome::ReadOnly;
    }
    let bytes = match fs::read(&file.path) {
        Ok(bytes) => bytes,
        Err(err) => return failed(err),
    };
    let decoded = encoding::decode(bytes);
    if !decoded.exact {
        return FileOutcome::Inexact;
    }
    let replacement = Replacement::new(template, finder, line_break(&decoded.text));
    let Some(all) = finder.replace_all(&decoded.text, &replacement, &CancelToken::new()) else {
        return FileOutcome::Changed;
    };
    if all.len() != file.expected || file.chosen.iter().any(|&i| i >= all.len()) {
        return FileOutcome::Changed;
    }
    // From the end, so the earlier char positions stay valid.
    let mut text = Rope::from_str(&decoded.text);
    for &i in file.chosen.iter().rev() {
        let (range, new) = &all[i];
        text.remove(range.clone());
        text.insert(range.start, new);
    }
    let bytes = match decoded.encoding.encode(&text.to_string()) {
        Ok(bytes) => bytes,
        Err(err) => return FileOutcome::Failed(err.to_string()),
    };
    match write_bytes_atomic(&file.path, &bytes, replacer) {
        Ok(()) => FileOutcome::Replaced(file.chosen.len()),
        Err(err) => failed(err),
    }
}

/// The line without its indentation, cut to [`CONTEXT_LENGTH`] chars around the match.
fn preview(line: &str, column: usize, length: usize) -> (String, Range<usize>) {
    let chars: Vec<char> = line.chars().collect();
    let indent = chars
        .iter()
        .take(column)
        .take_while(|c| c.is_whitespace())
        .count();
    let mut start = indent;
    let mut prefix = "";
    if chars.len() - start > CONTEXT_LENGTH && column - start > CONTEXT_BEFORE {
        start = column - CONTEXT_BEFORE;
        prefix = "…";
    }
    let end = chars.len().min(start + CONTEXT_LENGTH);
    let mut text: String = prefix.to_owned();
    text.extend(&chars[start..end]);
    if end < chars.len() {
        text.push('…');
    }
    let offset = prefix.chars().count();
    let from = column - start + offset;
    let to = (from + length).min(offset + end - start);
    (text, from..to)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::find::FindOptions;
    use std::sync::Mutex;

    fn project(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("bracket-folder-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::write(root.join(".gitignore"), "bin/\n").unwrap();
        fs::write(
            root.join(r"src\Program.cs"),
            "class Program\r\n{\r\n    // Customer list\r\n    var customer = new Customer();\r\n}\r\n",
        )
        .unwrap();
        fs::write(root.join(r"src\notes.txt"), "no match here\n").unwrap();
        fs::write(root.join(r"bin\Customer.cs"), "Customer").unwrap();
        // windows-1252: "Señal Customer" with ñ as the single byte 0xF1.
        fs::write(root.join(r"src\legacy.txt"), b"Se\xf1al Customer\r\n").unwrap();
        // UTF-16 LE with BOM.
        let utf16: Vec<u8> = "\u{feff}Customer en UTF-16"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        fs::write(root.join(r"src\wide.txt"), utf16).unwrap();
        fs::write(
            root.join(r"src\image.png"),
            b"\x89PNG\r\n\x1a\n\0\0Customer",
        )
        .unwrap();
        root
    }

    fn query(root: &Path, text: &str) -> FolderQuery {
        FolderQuery {
            root: root.to_path_buf(),
            finder: Finder::new(text, FindOptions::default()).unwrap(),
            walk: WalkOptions::default(),
            limit: 1000,
            replacement: None,
        }
    }

    fn run(query: &FolderQuery) -> (Vec<FileMatches>, Summary) {
        let files = Mutex::new(Vec::new());
        let summary = search_folder(query, &CancelToken::new(), |f| {
            files.lock().unwrap().push(f);
        })
        .unwrap();
        let mut files = files.into_inner().unwrap();
        // The index's order: case-insensitive.
        files.sort_by_key(|f| f.relative.to_lowercase());
        (files, summary)
    }

    #[test]
    fn matches_come_with_their_line_column_and_preview() {
        let root = project("lines");
        let (files, summary) = run(&query(&root, "customer"));
        let names: Vec<&str> = files.iter().map(|f| f.relative.as_str()).collect();
        assert_eq!(
            names,
            [r"src\legacy.txt", r"src\Program.cs", r"src\wide.txt"],
            "bin/ is ignored and the PNG is binary"
        );
        let program = &files[1].matches;
        assert_eq!(program.len(), 3);
        assert_eq!((program[0].line, program[0].column), (3, 8));
        assert_eq!(program[0].preview, "// Customer list");
        assert_eq!(program[0].highlight, 3..11);
        assert_eq!((program[2].line, program[2].column), (4, 24));
        assert_eq!(
            files[0].matches[0].preview, "Señal Customer",
            "decoded as windows-1252"
        );
        assert_eq!(files[0].matches[0].column, 7);
        assert_eq!(summary.matches, 5);
        assert_eq!(summary.files_skipped, 1);
        assert!(!summary.truncated);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn inclusions_and_exclusions_narrow_the_files() {
        let root = project("filters");
        let mut q = query(&root, "customer");
        q.walk.include = vec!["*.cs".into()];
        let (files, _) = run(&q);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].relative, r"src\Program.cs");
        q.walk.include.clear();
        q.walk.exclude = vec!["*.txt".into()];
        let (files, _) = run(&q);
        assert_eq!(files.len(), 1);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_limit_stops_the_search() {
        let root = project("limit");
        let mut q = query(&root, "customer");
        q.limit = 2;
        let (files, summary) = run(&q);
        let total: usize = files.iter().map(|f| f.matches.len()).sum();
        assert!(total <= 2, "{total} matches");
        assert!(summary.truncated);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_cancelled_search_reports_nothing() {
        let root = project("cancel");
        let cancel = CancelToken::new();
        cancel.cancel();
        let summary = search_folder(&query(&root, "customer"), &cancel, |_| {
            panic!("nothing is reported");
        })
        .unwrap();
        assert!(summary.cancelled);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn matches_come_with_their_replacement_when_asked() {
        let root = project("preview");
        let mut q = query(&root, "customer");
        q.replacement = Some("Client".into());
        let (files, _) = run(&q);
        let program = files
            .iter()
            .find(|f| f.relative == r"src\Program.cs")
            .unwrap();
        assert!(
            program
                .matches
                .iter()
                .all(|m| m.replacement.as_deref() == Some("Client"))
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn closed_files_keep_their_encoding_and_only_chosen_matches_change() {
        use bracket_core::document::RenameReplacer;
        let root = project("replace");
        let finder = Finder::new("customer", FindOptions::default()).unwrap();
        // Program.cs has 3 matches; replace the first and the last.
        let program = FileReplace {
            path: root.join(r"src\Program.cs"),
            expected: 3,
            chosen: vec![0, 2],
        };
        let outcome = replace_in_file(&finder, "Client", &program, &RenameReplacer);
        assert_eq!(outcome, FileOutcome::Replaced(2));
        assert_eq!(
            fs::read_to_string(&program.path).unwrap(),
            "class Program\r\n{\r\n    // Client list\r\n    var customer = new Client();\r\n}\r\n"
        );
        // windows-1252 stays windows-1252: "ñ" is still the single byte 0xF1.
        let legacy = FileReplace {
            path: root.join(r"src\legacy.txt"),
            expected: 1,
            chosen: vec![0],
        };
        assert_eq!(
            replace_in_file(&finder, "Cliente", &legacy, &RenameReplacer),
            FileOutcome::Replaced(1)
        );
        assert_eq!(fs::read(&legacy.path).unwrap(), b"Se\xf1al Cliente\r\n");
        // UTF-16 with BOM stays UTF-16 with BOM.
        let wide = FileReplace {
            path: root.join(r"src\wide.txt"),
            expected: 1,
            chosen: vec![0],
        };
        replace_in_file(&finder, "Client", &wide, &RenameReplacer);
        let bytes = fs::read(&wide.path).unwrap();
        assert_eq!(&bytes[..2], [0xFF, 0xFE]);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn changed_and_read_only_files_are_left_alone() {
        use bracket_core::document::RenameReplacer;
        let root = project("skip");
        let finder = Finder::new("customer", FindOptions::default()).unwrap();
        let program = root.join(r"src\Program.cs");
        let before = fs::read(&program).unwrap();
        let changed = FileReplace {
            path: program.clone(),
            expected: 5,
            chosen: vec![0],
        };
        assert_eq!(
            replace_in_file(&finder, "Client", &changed, &RenameReplacer),
            FileOutcome::Changed
        );
        let mut permissions = fs::metadata(&program).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&program, permissions.clone()).unwrap();
        let read_only = FileReplace {
            path: program.clone(),
            expected: 3,
            chosen: vec![0],
        };
        assert_eq!(
            replace_in_file(&finder, "Client", &read_only, &RenameReplacer),
            FileOutcome::ReadOnly
        );
        assert_eq!(fs::read(&program).unwrap(), before);
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        fs::set_permissions(&program, permissions).unwrap();
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_line_break_follows_the_text() {
        assert_eq!(line_break("a\r\nb"), "\r\n");
        assert_eq!(line_break("a\nb\r\n"), "\n");
        assert_eq!(line_break("a\rb"), "\r");
        assert_eq!(line_break("one line"), "\r\n");
    }

    #[test]
    fn long_lines_are_cut_around_the_match() {
        let line = format!("{}needle{}", "a".repeat(500), "b".repeat(500));
        let (text, highlight) = preview(&line, 500, 6);
        assert!(text.starts_with('…') && text.ends_with('…'));
        assert_eq!(
            text.chars()
                .skip(highlight.start)
                .take(6)
                .collect::<String>(),
            "needle"
        );
        let (text, highlight) = preview("    let x = 1;", 8, 1);
        assert_eq!(text, "let x = 1;");
        assert_eq!(highlight, 4..5);
    }
}
