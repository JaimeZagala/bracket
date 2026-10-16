//! A buffer bound to a file: loading with encoding detection, line endings and atomic saving.
//!
//! A file is editable only if its text can be written back to exactly the same bytes (see
//! [`crate::encoding`]); otherwise it opens read-only and can never be overwritten with altered
//! bytes. Saving never substitutes characters the encoding cannot represent: it fails instead.

use std::fmt;
use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use ropey::Rope;

use crate::buffer::Buffer;
use crate::encoding::{self, EncodeError, TextEncoding};

const UTF8_BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

/// Line ending used when inserting new lines. Existing line endings are only changed by an
/// explicit conversion ([`Document::convert_line_endings`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineEnding {
    CrLf,
    Lf,
    Cr,
}

impl LineEnding {
    pub const ALL: [Self; 3] = [Self::CrLf, Self::Lf, Self::Cr];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::CrLf => "\r\n",
            Self::Lf => "\n",
            Self::Cr => "\r",
        }
    }

    /// Short name for the status bar: `CRLF`, `LF` or `CR`.
    pub fn label(self) -> &'static str {
        match self {
            Self::CrLf => "CRLF",
            Self::Lf => "LF",
            Self::Cr => "CR",
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

    /// Rewrites every line break in `text` (CRLF, LF or CR) as `self`.
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
    pub encoding: TextEncoding,
    /// False when writing `text` back would not reproduce the file's bytes (read-only).
    pub exact: bool,
}

impl From<encoding::Decoded> for LoadedText {
    fn from(decoded: encoding::Decoded) -> Self {
        Self {
            text: decoded.text,
            encoding: decoded.encoding,
            exact: decoded.exact,
        }
    }
}

/// Reads a file and detects its encoding. Never fails on undecodable bytes: see
/// [`LoadedText::exact`].
pub fn load_text(path: &Path) -> io::Result<LoadedText> {
    Ok(encoding::decode(fs::read(path)?).into())
}

/// Reads a file decoding it as `encoding` ("reopen with encoding").
pub fn load_text_as(path: &Path, encoding: TextEncoding) -> io::Result<LoadedText> {
    Ok(encoding::decode_as(&fs::read(path)?, encoding).into())
}

/// Puts a completely written temporary file in place of the target file.
///
/// The core only knows how to rename ([`RenameReplacer`]). On Windows, `bracket-app` provides
/// one based on `ReplaceFileW` that also keeps the target's attributes and permissions.
pub trait FileReplacer: Send + Sync {
    fn replace(&self, temp: &Path, target: &Path) -> io::Result<()>;
}

/// Replaces by renaming. The new file keeps the temporary file's attributes, not the target's.
pub struct RenameReplacer;

impl FileReplacer for RenameReplacer {
    fn replace(&self, temp: &Path, target: &Path) -> io::Result<()> {
        fs::rename(temp, target)
    }
}

/// Writes UTF-8 `text` to `path` atomically: the content goes to a temporary file in the same
/// folder, is flushed to disk, and then `replacer` puts it in place of the target. If anything
/// fails, the original file is left untouched and the temporary file is removed.
pub fn write_atomic(
    path: &Path,
    text: &Rope,
    has_bom: bool,
    replacer: &dyn FileReplacer,
) -> io::Result<()> {
    write_atomic_with(path, replacer, |writer| {
        if has_bom {
            writer.write_all(UTF8_BOM)?;
        }
        for chunk in text.chunks() {
            writer.write_all(chunk.as_bytes())?;
        }
        Ok(())
    })
}

/// Writes `bytes` to `path` atomically, like [`write_atomic`].
pub fn write_bytes_atomic(
    path: &Path,
    bytes: &[u8],
    replacer: &dyn FileReplacer,
) -> io::Result<()> {
    write_atomic_with(path, replacer, |writer| writer.write_all(bytes))
}

fn write_atomic_with(
    path: &Path,
    replacer: &dyn FileReplacer,
    write: impl FnOnce(&mut BufWriter<File>) -> io::Result<()>,
) -> io::Result<()> {
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

    let result = write_and_replace(&temp, path, replacer, write);
    if result.is_err() {
        // Best effort clean-up; the original error is what matters.
        let _ = fs::remove_file(&temp);
    }
    result
}

fn write_and_replace(
    temp: &Path,
    target: &Path,
    replacer: &dyn FileReplacer,
    write: impl FnOnce(&mut BufWriter<File>) -> io::Result<()>,
) -> io::Result<()> {
    let file = File::create(temp)?;
    let mut writer = BufWriter::new(file);
    write(&mut writer)?;
    let file = writer.into_inner().map_err(|e| e.into_error())?;
    file.sync_all()?;
    drop(file);
    replacer.replace(temp, target)
}

/// A buffer plus what is needed to save it back exactly as it was read.
#[derive(Debug)]
pub struct Document {
    pub buffer: Buffer,
    path: Option<PathBuf>,
    /// Encoding used for the next save.
    encoding: TextEncoding,
    /// Encoding of the file on disk; differs from `encoding` after "save with encoding".
    saved_encoding: TextEncoding,
    read_only: bool,
    line_ending: LineEnding,
    saved_version: u64,
}

impl Document {
    /// An empty, unsaved document that will use `line_ending` for new lines.
    pub fn new_empty(line_ending: LineEnding, encoding: TextEncoding) -> Self {
        Self {
            buffer: Buffer::new(),
            path: None,
            encoding,
            saved_encoding: encoding,
            read_only: false,
            line_ending,
            saved_version: 0,
        }
    }

    /// A document for text read from `path`. New lines use the file's first line ending, or
    /// `fallback` if it has none.
    pub fn from_loaded(path: PathBuf, loaded: LoadedText, fallback: LineEnding) -> Self {
        let buffer = Buffer::from_text(&loaded.text);
        let line_ending = LineEnding::detect(buffer.text()).unwrap_or(fallback);
        let saved_version = buffer.version();
        Self {
            buffer,
            path: Some(path),
            encoding: loaded.encoding,
            saved_encoding: loaded.encoding,
            read_only: !loaded.exact,
            line_ending,
            saved_version,
        }
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Binds the document to another file ("save as"). The next save writes there.
    pub fn set_path(&mut self, path: PathBuf) {
        self.path = Some(path);
    }

    pub fn encoding(&self) -> TextEncoding {
        self.encoding
    }

    /// Chooses the encoding for the next save ("save with encoding"). The document becomes
    /// modified until it is saved.
    pub fn set_encoding(&mut self, encoding: TextEncoding) {
        self.encoding = encoding;
    }

    /// True when the file could not be decoded exactly and must not be saved.
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    pub fn line_ending(&self) -> LineEnding {
        self.line_ending
    }

    /// Rewrites every line break as `line_ending` in one undo step, and uses it for new lines.
    pub fn convert_line_endings(&mut self, line_ending: LineEnding) {
        self.line_ending = line_ending;
        let converted = line_ending.normalize(&self.buffer.text().to_string());
        self.buffer.replace_all_keeping_positions(&converted);
    }

    /// Puts back text that had not been saved when the session ended, in one undo step:
    /// undoing it returns to the text read from disk. `encoding` and `line_ending` are the ones
    /// the document had, so the next save writes what the user would have written.
    pub fn restore_unsaved(&mut self, text: &str, encoding: TextEncoding, line_ending: LineEnding) {
        self.buffer.replace_all_keeping_positions(text);
        self.buffer.seal_undo_step();
        self.encoding = encoding;
        self.line_ending = line_ending;
    }

    pub fn is_dirty(&self) -> bool {
        self.buffer.version() != self.saved_version || self.encoding != self.saved_encoding
    }

    /// Everything a background thread needs to save the current text. Cloning a rope is cheap.
    pub fn save_request(&self) -> Option<SaveRequest> {
        if self.read_only {
            return None;
        }
        Some(SaveRequest {
            path: self.path.clone()?,
            text: self.buffer.text().clone(),
            encoding: self.encoding,
            version: self.buffer.version(),
        })
    }

    /// Records that the text at `version`, in `encoding`, is now on disk. The next edit starts a
    /// new undo step, so undo can always return exactly to the saved text.
    pub fn mark_saved(&mut self, version: u64, encoding: TextEncoding) {
        self.saved_version = version;
        self.saved_encoding = encoding;
        self.buffer.seal_undo_step();
    }
}

/// Why a save failed.
#[derive(Debug)]
pub enum SaveError {
    /// A character cannot be represented in the chosen encoding. Nothing was written.
    Encode(EncodeError),
    Io(io::Error),
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Encode(err) => write!(f, "{err}"),
            Self::Io(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for SaveError {}

/// A snapshot of a document to be written by [`SaveRequest::write`], possibly on another thread.
#[derive(Debug, Clone)]
pub struct SaveRequest {
    pub path: PathBuf,
    pub text: Rope,
    pub encoding: TextEncoding,
    pub version: u64,
}

impl SaveRequest {
    pub fn write(&self, replacer: &dyn FileReplacer) -> Result<(), SaveError> {
        if self.encoding.is_utf8() {
            // The common case streams the rope without building a single string.
            return write_atomic(&self.path, &self.text, self.encoding.has_bom(), replacer)
                .map_err(SaveError::Io);
        }
        let bytes = self
            .encoding
            .encode(&self.text.to_string())
            .map_err(SaveError::Encode)?;
        write_bytes_atomic(&self.path, &bytes, replacer).map_err(SaveError::Io)
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

    fn open(path: &Path) -> Document {
        Document::from_loaded(
            path.to_path_buf(),
            load_text(path).unwrap(),
            LineEnding::CrLf,
        )
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

        let doc = open(&path);
        assert_eq!(doc.encoding(), TextEncoding::UTF8_BOM);
        assert!(!doc.is_read_only());
        assert_eq!(doc.line_ending(), LineEnding::CrLf);
        doc.save_request().unwrap().write(&RenameReplacer).unwrap();

        assert_eq!(fs::read(&path).unwrap(), bytes);
        let leftovers: Vec<_> = fs::read_dir(&dir).unwrap().collect();
        assert_eq!(leftovers.len(), 1, "no temporary file left behind");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn legacy_and_utf16_files_round_trip_byte_exact() {
        let dir = temp_dir("legacy");
        let cases: [(&str, Vec<u8>); 2] = [
            (
                "latin1.txt",
                b"Configuraci\xF3n del \xE1rbol con se\xF1al\r\n".to_vec(),
            ),
            (
                "utf16.txt",
                TextEncoding::UTF16LE.encode("Hola ñ 日本\r\n").unwrap(),
            ),
        ];
        for (name, bytes) in cases {
            let path = dir.join(name);
            fs::write(&path, &bytes).unwrap();
            let doc = open(&path);
            assert!(!doc.is_read_only(), "{name} should be editable");
            doc.save_request().unwrap().write(&RenameReplacer).unwrap();
            assert_eq!(fs::read(&path).unwrap(), bytes, "{name} changed on save");
        }
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn undecodable_file_opens_read_only_and_cannot_be_saved() {
        let dir = temp_dir("broken");
        let path = dir.join("broken.txt");
        let bytes = b"\xEF\xBB\xBFok \xFF\xFE broken";
        fs::write(&path, bytes).unwrap();

        let doc = open(&path);
        assert!(doc.is_read_only());
        assert!(doc.save_request().is_none());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn saving_unrepresentable_text_fails_and_keeps_the_file() {
        let dir = temp_dir("unmappable");
        let path = dir.join("latin1.txt");
        let bytes = b"caf\xE9 con se\xF1al y m\xE1s acentos del espa\xF1ol";
        fs::write(&path, bytes).unwrap();

        let mut doc = open(&path);
        doc.buffer.insert("日");
        let err = doc
            .save_request()
            .unwrap()
            .write(&RenameReplacer)
            .unwrap_err();
        assert!(matches!(
            err,
            SaveError::Encode(EncodeError {
                character: Some('日'),
                ..
            })
        ));
        assert_eq!(fs::read(&path).unwrap(), bytes);

        doc.set_encoding(TextEncoding::UTF8);
        doc.save_request().unwrap().write(&RenameReplacer).unwrap();
        assert!(fs::read_to_string(&path).unwrap().starts_with("日café"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reopening_with_an_encoding_reinterprets_the_bytes() {
        let dir = temp_dir("reopen");
        let path = dir.join("short.txt");
        fs::write(&path, b"caf\xE9").unwrap();
        let loaded = load_text_as(&path, TextEncoding::from_code("windows-1252").unwrap()).unwrap();
        assert_eq!(loaded.text, "café");
        assert!(loaded.exact);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn changing_the_encoding_makes_the_document_modified() {
        let mut doc = Document::new_empty(LineEnding::CrLf, TextEncoding::UTF8);
        assert!(!doc.is_dirty());
        doc.set_encoding(TextEncoding::UTF8_BOM);
        assert!(doc.is_dirty());
        doc.mark_saved(doc.buffer.version(), TextEncoding::UTF8_BOM);
        assert!(!doc.is_dirty());
    }

    #[test]
    fn converting_line_endings_is_one_undo_step() {
        let mut doc = Document::new_empty(LineEnding::CrLf, TextEncoding::UTF8);
        doc.buffer.insert("a");
        doc.buffer.insert("\r\n");
        doc.buffer.insert("b\rc\n");
        doc.convert_line_endings(LineEnding::Lf);
        assert_eq!(doc.buffer.text().to_string(), "a\nb\nc\n");
        assert_eq!(doc.line_ending(), LineEnding::Lf);
        doc.buffer.undo();
        assert_eq!(doc.buffer.text().to_string(), "a\r\nb\rc\n");
    }

    #[test]
    fn restored_unsaved_text_is_modified_and_undoes_to_the_disk_text() {
        let dir = temp_dir("restore");
        let path = dir.join("a.txt");
        fs::write(
            &path, "disk
",
        )
        .unwrap();
        let mut doc = open(&path);
        doc.restore_unsaved(
            "edited
",
            TextEncoding::UTF8_BOM,
            LineEnding::Lf,
        );
        assert!(doc.is_dirty());
        assert_eq!(
            doc.buffer.text().to_string(),
            "edited
"
        );
        assert_eq!(doc.line_ending(), LineEnding::Lf);
        assert_eq!(doc.save_request().unwrap().encoding, TextEncoding::UTF8_BOM);
        doc.buffer.undo();
        assert_eq!(
            doc.buffer.text().to_string(),
            "disk
"
        );

        let mut untitled = Document::new_empty(LineEnding::CrLf, TextEncoding::UTF8);
        untitled.restore_unsaved("draft", TextEncoding::UTF8, LineEnding::CrLf);
        assert!(untitled.is_dirty());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn save_as_writes_to_the_new_path() {
        let dir = temp_dir("save-as");
        let mut doc = Document::new_empty(LineEnding::Lf, TextEncoding::UTF8);
        assert!(
            doc.save_request().is_none(),
            "an untitled document has nowhere to go"
        );
        doc.buffer.insert("hola");
        let path = dir.join("nuevo.txt");
        doc.set_path(path.clone());
        let request = doc.save_request().unwrap();
        request.write(&RenameReplacer).unwrap();
        doc.mark_saved(request.version, request.encoding);
        assert_eq!(fs::read(&path).unwrap(), b"hola");
        assert!(!doc.is_dirty());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn undoing_back_to_the_saved_text_is_not_modified() {
        let mut doc = Document::new_empty(LineEnding::Lf, TextEncoding::UTF8);
        doc.buffer.insert("a");
        doc.buffer.insert("b");
        doc.mark_saved(doc.buffer.version(), TextEncoding::UTF8);
        assert!(!doc.is_dirty());

        doc.buffer.insert("c");
        assert!(
            doc.is_dirty(),
            "typing after a save is a change even if it could merge"
        );
        doc.buffer.undo();
        assert!(!doc.is_dirty(), "undo returned exactly to the saved text");
        doc.buffer.redo();
        assert!(doc.is_dirty());
        doc.buffer.undo();
        doc.buffer.undo();
        assert!(doc.is_dirty(), "undoing past the save is a change too");
        doc.buffer.redo();
        assert!(!doc.is_dirty(), "redo back to the saved text");
    }

    #[test]
    fn a_new_edit_after_undo_is_modified_even_if_it_looks_alike() {
        let mut doc = Document::new_empty(LineEnding::Lf, TextEncoding::UTF8);
        doc.buffer.insert("x");
        doc.mark_saved(doc.buffer.version(), TextEncoding::UTF8);
        doc.buffer.undo();
        doc.buffer.insert("y");
        assert!(doc.is_dirty(), "ids are never reused");
    }

    #[test]
    fn dirty_tracks_edits_and_saves() {
        let mut doc = Document::new_empty(LineEnding::CrLf, TextEncoding::UTF8);
        assert!(!doc.is_dirty());
        doc.buffer.insert("x");
        assert!(doc.is_dirty());
        let version = doc.buffer.version();
        doc.mark_saved(version, TextEncoding::UTF8);
        assert!(!doc.is_dirty());
    }

    #[test]
    fn failed_save_keeps_original_and_cleans_up() {
        let dir = temp_dir("failed");
        // Renaming a file over an existing directory fails.
        let target = dir.join("target");
        fs::create_dir(&target).unwrap();
        let err = write_atomic(&target, &Rope::from_str("new"), false, &RenameReplacer);
        assert!(err.is_err());
        let entries: Vec<_> = fs::read_dir(&dir).unwrap().collect();
        assert_eq!(entries.len(), 1, "only the original entry remains");
        fs::remove_dir_all(&dir).unwrap();
    }
}
