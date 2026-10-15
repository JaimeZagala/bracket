//! A buffer bound to a file: loading, line ending detection and atomic saving.
//!
//! Phase 0 supports UTF-8 (with or without BOM) only. A file that is not valid UTF-8 is shown
//! with replacement characters and opened read-only, so it can never be overwritten with
//! altered bytes.

use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use ropey::Rope;

use crate::buffer::Buffer;

const UTF8_BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

/// Line ending used when inserting new lines. Existing line endings are never changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineEnding {
    CrLf,
    Lf,
    Cr,
}

impl LineEnding {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CrLf => "\r\n",
            Self::Lf => "\n",
            Self::Cr => "\r",
        }
    }

    /// The line ending of the first line break in `text`, if any.
    pub fn detect(text: &Rope) -> Option<Self> {
        // Only the first line needs to be scanned, so this is cheap even for huge files.
        // A line slice includes its line break, so the first line ends with one unless the
        // text has a single line.
        let first_line = text.line(0);
        let len = first_line.len_chars();
        if len == 0 {
            return None;
        }
        match first_line.char(len - 1) {
            '\n' if len >= 2 && first_line.char(len - 2) == '\r' => Some(Self::CrLf),
            '\n' => Some(Self::Lf),
            '\r' => Some(Self::Cr),
            _ => None,
        }
    }

    /// Rewrites every line break in `text` (CRLF, LF or CR) as `self`. Used for pasted text.
    pub fn normalize(self, text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\r' => {
                    if chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                    out.push_str(self.as_str());
                }
                '\n' => out.push_str(self.as_str()),
                other => out.push(other),
            }
        }
        out
    }
}

/// Text loaded from disk, before it becomes a document.
#[derive(Debug)]
pub struct LoadedText {
    pub text: String,
    pub has_bom: bool,
    /// False when the bytes were not valid UTF-8 and had to be decoded lossily.
    pub valid_utf8: bool,
}

/// Reads a file. Never fails on invalid UTF-8: see [`LoadedText::valid_utf8`].
pub fn load_text(path: &Path) -> io::Result<LoadedText> {
    let bytes = fs::read(path)?;
    let has_bom = bytes.starts_with(UTF8_BOM);
    let body = if has_bom {
        bytes[UTF8_BOM.len()..].to_vec()
    } else {
        bytes
    };
    Ok(match String::from_utf8(body) {
        Ok(text) => LoadedText {
            text,
            has_bom,
            valid_utf8: true,
        },
        Err(err) => LoadedText {
            text: String::from_utf8_lossy(err.as_bytes()).into_owned(),
            has_bom,
            valid_utf8: false,
        },
    })
}

/// Writes `text` to `path` atomically: the content goes to a temporary file in the same folder,
/// is flushed to disk, and then replaces the target with a rename. If anything fails, the
/// original file is left untouched and the temporary file is removed.
pub fn write_atomic(path: &Path, text: &Rope, has_bom: bool) -> io::Result<()> {
    let file_name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?;
    let dir = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let temp = dir.join(format!(
        ".{}.bracket-tmp-{}",
        file_name.to_string_lossy(),
        std::process::id()
    ));

    let result = write_and_replace(&temp, path, text, has_bom);
    if result.is_err() {
        // Best effort clean-up; the original error is what matters.
        let _ = fs::remove_file(&temp);
    }
    result
}

fn write_and_replace(temp: &Path, target: &Path, text: &Rope, has_bom: bool) -> io::Result<()> {
    let file = File::create(temp)?;
    let mut writer = BufWriter::new(file);
    if has_bom {
        writer.write_all(UTF8_BOM)?;
    }
    for chunk in text.chunks() {
        writer.write_all(chunk.as_bytes())?;
    }
    let file = writer.into_inner().map_err(|e| e.into_error())?;
    file.sync_all()?;
    drop(file);
    fs::rename(temp, target)
}

/// A buffer plus what is needed to save it back exactly as it was read.
#[derive(Debug)]
pub struct Document {
    pub buffer: Buffer,
    path: Option<PathBuf>,
    has_bom: bool,
    read_only: bool,
    line_ending: LineEnding,
    saved_version: u64,
}

impl Document {
    /// An empty, unsaved document. New files use CRLF, the Windows convention.
    pub fn new_empty() -> Self {
        Self {
            buffer: Buffer::new(),
            path: None,
            has_bom: false,
            read_only: false,
            line_ending: LineEnding::CrLf,
            saved_version: 0,
        }
    }

    pub fn from_loaded(path: PathBuf, loaded: LoadedText) -> Self {
        let buffer = Buffer::from_text(&loaded.text);
        let line_ending = LineEnding::detect(buffer.text()).unwrap_or(LineEnding::CrLf);
        let saved_version = buffer.version();
        Self {
            buffer,
            path: Some(path),
            has_bom: loaded.has_bom,
            read_only: !loaded.valid_utf8,
            line_ending,
            saved_version,
        }
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn has_bom(&self) -> bool {
        self.has_bom
    }

    /// True when the file could not be decoded exactly and must not be saved.
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    pub fn line_ending(&self) -> LineEnding {
        self.line_ending
    }

    pub fn is_dirty(&self) -> bool {
        self.buffer.version() != self.saved_version
    }

    /// Everything a background thread needs to save the current text. Cloning a rope is cheap.
    pub fn save_request(&self) -> Option<SaveRequest> {
        if self.read_only {
            return None;
        }
        Some(SaveRequest {
            path: self.path.clone()?,
            text: self.buffer.text().clone(),
            has_bom: self.has_bom,
            version: self.buffer.version(),
        })
    }

    /// Records that the text at `version` is now on disk.
    pub fn mark_saved(&mut self, version: u64) {
        self.saved_version = version;
    }
}

/// A snapshot of a document to be written by [`SaveRequest::write`], possibly on another thread.
#[derive(Debug, Clone)]
pub struct SaveRequest {
    pub path: PathBuf,
    pub text: Rope,
    pub has_bom: bool,
    pub version: u64,
}

impl SaveRequest {
    pub fn write(&self) -> io::Result<()> {
        write_atomic(&self.path, &self.text, self.has_bom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bracket-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn detect_line_ending_from_first_break() {
        assert_eq!(
            LineEnding::detect(&Rope::from_str("a\r\nb\n")),
            Some(LineEnding::CrLf)
        );
        assert_eq!(
            LineEnding::detect(&Rope::from_str("a\nb\r\n")),
            Some(LineEnding::Lf)
        );
        assert_eq!(
            LineEnding::detect(&Rope::from_str("a\rb")),
            Some(LineEnding::Cr)
        );
        assert_eq!(LineEnding::detect(&Rope::from_str("abc")), None);
        assert_eq!(LineEnding::detect(&Rope::from_str("")), None);
    }

    #[test]
    fn normalize_rewrites_every_kind_of_break() {
        assert_eq!(LineEnding::Lf.normalize("a\r\nb\rc\nd"), "a\nb\nc\nd");
        assert_eq!(LineEnding::CrLf.normalize("a\nb\r\n"), "a\r\nb\r\n");
    }

    #[test]
    fn load_and_save_round_trip_is_byte_exact() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("mixed.txt");
        let bytes = b"\xEF\xBB\xBFline1\r\nline2\nline3\rend \xC3\xB1";
        fs::write(&path, bytes).unwrap();

        let doc = Document::from_loaded(path.clone(), load_text(&path).unwrap());
        assert!(doc.has_bom());
        assert!(!doc.is_read_only());
        assert_eq!(doc.line_ending(), LineEnding::CrLf);
        doc.save_request().unwrap().write().unwrap();

        assert_eq!(fs::read(&path).unwrap(), bytes);
        let leftovers: Vec<_> = fs::read_dir(&dir).unwrap().collect();
        assert_eq!(leftovers.len(), 1, "no temporary file left behind");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn invalid_utf8_opens_read_only_and_cannot_be_saved() {
        let dir = temp_dir("latin1");
        let path = dir.join("latin1.txt");
        fs::write(&path, b"caf\xE9").unwrap();

        let doc = Document::from_loaded(path.clone(), load_text(&path).unwrap());
        assert!(doc.is_read_only());
        assert!(doc.save_request().is_none());
        assert_eq!(fs::read(&path).unwrap(), b"caf\xE9");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn dirty_tracks_edits_and_saves() {
        let mut doc = Document::new_empty();
        assert!(!doc.is_dirty());
        doc.buffer.insert("x");
        assert!(doc.is_dirty());
        let version = doc.buffer.version();
        doc.mark_saved(version);
        assert!(!doc.is_dirty());
    }

    #[test]
    fn failed_save_keeps_original_and_cleans_up() {
        let dir = temp_dir("failed");
        // Renaming a file over an existing directory fails.
        let target = dir.join("target");
        fs::create_dir(&target).unwrap();
        let err = write_atomic(&target, &Rope::from_str("new"), false);
        assert!(err.is_err());
        let entries: Vec<_> = fs::read_dir(&dir).unwrap().collect();
        assert_eq!(entries.len(), 1, "only the original entry remains");
        fs::remove_dir_all(&dir).unwrap();
    }
}
