//! User configuration stored in `config.toml`.
//!
//! Loading never fails: a missing file means defaults, and every problem (unreadable file,
//! syntax error, invalid value, unknown key) becomes a warning while the valid values are kept.
//! Saving keeps the user's comments, order and unknown keys, and refuses to overwrite a file
//! that does not parse, so a hand-edited file is never destroyed.
//!
//! ```toml
//! [interface]
//! language = "es"      # "es" or "en"
//!
//! [editor]
//! font_family = "Consolas" # any installed font; falls back to Consolas
//! font_size = 14.0     # 8 to 48
//! tab_width = 4        # 1 to 16
//!
//! [files]
//! line_ending = "crlf" # line ending of new files: "crlf", "lf" or "cr"
//! encoding = "utf-8"  # encoding of new files: "utf-8", "utf-8-bom", or another label
//! ```

use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

use bracket_core::LineEnding;
use bracket_core::document::{RenameReplacer, write_bytes_atomic};
use bracket_core::encoding::TextEncoding;
use toml_edit::{DocumentMut, Item, Table, Value};

/// Interface language.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Language {
    #[default]
    Spanish,
    English,
}

impl Language {
    pub fn code(self) -> &'static str {
        match self {
            Self::Spanish => "es",
            Self::English => "en",
        }
    }

    fn from_code(code: &str) -> Option<Self> {
        match code {
            "es" => Some(Self::Spanish),
            "en" => Some(Self::English),
            _ => None,
        }
    }
}

pub const FONT_SIZE_MIN: f32 = 8.0;
pub const FONT_SIZE_MAX: f32 = 48.0;
pub const TAB_WIDTH_MAX: usize = 16;
pub const FONT_FAMILY_MAX_LEN: usize = 100;

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub language: Language,
    /// Editor font family, looked up among the installed fonts.
    pub font_family: String,
    pub font_size: f32,
    pub tab_width: usize,
    /// Line ending used for new files.
    pub line_ending: LineEnding,
    /// Encoding used for new files.
    pub encoding: TextEncoding,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            language: Language::Spanish,
            font_family: "Consolas".to_owned(),
            font_size: 14.0,
            tab_width: 4,
            line_ending: LineEnding::CrLf,
            encoding: TextEncoding::UTF8,
        }
    }
}

/// A problem found while loading the configuration. The interface turns it into a translated
/// message; the `String` fields carry details that are shown as they are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigWarning {
    /// The file exists but could not be read.
    Unreadable { error: String },
    /// The file is not valid TOML. All values are defaults.
    Syntax { error: String },
    /// A known key has a value of the wrong type or out of range; its default is used.
    InvalidValue { key: String, found: String },
    /// A key Bracket does not know (probably a typo). It is kept when saving.
    UnknownKey { key: String },
}

#[derive(Clone, Debug)]
pub struct LoadedConfig {
    pub config: Config,
    pub warnings: Vec<ConfigWarning>,
}

/// Reads the configuration file. Never fails; see [`ConfigWarning`].
pub fn load(path: &Path) -> LoadedConfig {
    match fs::read_to_string(path) {
        Ok(text) => parse(&text),
        Err(err) if err.kind() == io::ErrorKind::NotFound => LoadedConfig {
            config: Config::default(),
            warnings: Vec::new(),
        },
        Err(err) => LoadedConfig {
            config: Config::default(),
            warnings: vec![ConfigWarning::Unreadable {
                error: err.to_string(),
            }],
        },
    }
}

/// Parses configuration text. Never fails; see [`ConfigWarning`].
pub fn parse(text: &str) -> LoadedConfig {
    let mut config = Config::default();
    let doc = match text.parse::<DocumentMut>() {
        Ok(doc) => doc,
        Err(err) => {
            return LoadedConfig {
                config,
                warnings: vec![ConfigWarning::Syntax {
                    error: err.to_string(),
                }],
            };
        }
    };
    let mut warnings = Vec::new();

    for (section, item) in doc.iter() {
        let Some(table) = item.as_table_like() else {
            warnings.push(ConfigWarning::UnknownKey {
                key: section.to_owned(),
            });
            continue;
        };
        for (key, item) in table.iter() {
            let full_key = format!("{section}.{key}");
            let applied = match (section, key) {
                ("interface", "language") => item
                    .as_str()
                    .and_then(Language::from_code)
                    .map(|v| config.language = v),
                ("editor", "font_family") => item
                    .as_str()
                    .map(str::trim)
                    .filter(|v| !v.is_empty() && v.len() <= FONT_FAMILY_MAX_LEN)
                    .map(|v| config.font_family = v.to_owned()),
                ("editor", "font_size") => number(item)
                    .filter(|v| (FONT_SIZE_MIN..=FONT_SIZE_MAX).contains(v))
                    .map(|v| config.font_size = v),
                ("editor", "tab_width") => item
                    .as_integer()
                    .and_then(|v| usize::try_from(v).ok())
                    .filter(|v| (1..=TAB_WIDTH_MAX).contains(v))
                    .map(|v| config.tab_width = v),
                ("files", "line_ending") => item
                    .as_str()
                    .and_then(line_ending_from_code)
                    .map(|v| config.line_ending = v),
                ("files", "encoding") => item
                    .as_str()
                    .and_then(TextEncoding::from_code)
                    .map(|v| config.encoding = v),
                _ => {
                    warnings.push(ConfigWarning::UnknownKey { key: full_key });
                    continue;
                }
            };
            if applied.is_none() {
                warnings.push(ConfigWarning::InvalidValue {
                    key: full_key,
                    found: item.to_string().trim().to_owned(),
                });
            }
        }
    }

    LoadedConfig { config, warnings }
}

/// Accepts both `14` and `14.5`.
fn number(item: &Item) -> Option<f32> {
    item.as_float()
        .or_else(|| item.as_integer().map(|v| v as f64))
        .map(|v| v as f32)
}

pub(crate) fn line_ending_code(line_ending: LineEnding) -> &'static str {
    match line_ending {
        LineEnding::CrLf => "crlf",
        LineEnding::Lf => "lf",
        LineEnding::Cr => "cr",
    }
}

pub(crate) fn line_ending_from_code(code: &str) -> Option<LineEnding> {
    match code {
        "crlf" => Some(LineEnding::CrLf),
        "lf" => Some(LineEnding::Lf),
        "cr" => Some(LineEnding::Cr),
        _ => None,
    }
}

#[derive(Debug)]
pub enum SaveError {
    /// The existing file is not valid TOML; overwriting it would lose the user's text.
    ExistingFileInvalid {
        error: String,
    },
    Io(io::Error),
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExistingFileInvalid { error } => {
                write!(
                    f,
                    "the existing configuration file is not valid TOML: {error}"
                )
            }
            Self::Io(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for SaveError {}

/// Writes `config` to `path`, creating its folder if needed. Existing comments, order and
/// unknown keys are kept; only Bracket's keys are updated. The write is atomic.
pub fn save(config: &Config, path: &Path) -> Result<(), SaveError> {
    let mut doc = match fs::read_to_string(path) {
        Ok(text) => text
            .parse::<DocumentMut>()
            .map_err(|err| SaveError::ExistingFileInvalid {
                error: err.to_string(),
            })?,
        Err(err) if err.kind() == io::ErrorKind::NotFound => DocumentMut::new(),
        Err(err) => return Err(SaveError::Io(err)),
    };

    set(
        &mut doc,
        "interface",
        "language",
        config.language.code().into(),
    );
    set(
        &mut doc,
        "editor",
        "font_family",
        config.font_family.as_str().into(),
    );
    set(
        &mut doc,
        "editor",
        "font_size",
        f64::from(config.font_size).into(),
    );
    set(
        &mut doc,
        "editor",
        "tab_width",
        (config.tab_width as i64).into(),
    );
    set(
        &mut doc,
        "files",
        "line_ending",
        line_ending_code(config.line_ending).into(),
    );
    set(&mut doc, "files", "encoding", config.encoding.code().into());

    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(SaveError::Io)?;
    }
    write_bytes_atomic(path, doc.to_string().as_bytes(), &RenameReplacer).map_err(SaveError::Io)
}

/// Sets `section.key` to `new`. A missing section becomes a regular `[section]` table at the end
/// of the file; an existing key keeps its position and its comments.
fn set(doc: &mut DocumentMut, section: &str, key: &str, mut new: Value) {
    if doc.get(section).and_then(Item::as_table_like).is_none() {
        doc.insert(section, Item::Table(Table::new()));
    }
    let Some(table) = doc[section].as_table_like_mut() else {
        return;
    };
    match table.get_mut(key) {
        Some(item) => {
            if let Some(old) = item.as_value() {
                *new.decor_mut() = old.decor().clone();
            }
            *item = Item::Value(new);
        }
        None => {
            table.insert(key, Item::Value(new));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("bracket-config-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn empty_text_gives_defaults() {
        let loaded = parse("");
        assert_eq!(loaded.config, Config::default());
        assert!(loaded.warnings.is_empty());
    }

    #[test]
    fn valid_values_are_read() {
        let loaded = parse(
            "[interface]\nlanguage = \"en\"\n[editor]\nfont_family = \" Fira Code \"\n\
             font_size = 16\ntab_width = 2\n[files]\nline_ending = \"lf\"\n",
        );
        assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
        assert_eq!(
            loaded.config,
            Config {
                language: Language::English,
                font_family: "Fira Code".into(),
                font_size: 16.0,
                tab_width: 2,
                line_ending: LineEnding::Lf,
                encoding: TextEncoding::UTF8,
            }
        );
    }

    #[test]
    fn one_bad_value_keeps_the_rest() {
        let loaded = parse("[editor]\ntab_width = \"four\"\nfont_size = 12.5\n");
        assert_eq!(loaded.config.tab_width, 4, "default kept");
        assert_eq!(loaded.config.font_size, 12.5, "valid value still read");
        assert_eq!(
            loaded.warnings,
            vec![ConfigWarning::InvalidValue {
                key: "editor.tab_width".into(),
                found: "\"four\"".into(),
            }]
        );
    }

    #[test]
    fn out_of_range_values_are_rejected() {
        let loaded = parse("[editor]\ntab_width = 0\nfont_size = 200\n");
        assert_eq!(loaded.config, Config::default());
        assert_eq!(loaded.warnings.len(), 2);
    }

    #[test]
    fn unknown_keys_and_sections_are_reported() {
        let loaded = parse("top = 3\n[editor]\ntab_widht = 2\n[colors]\nx = 1\n");
        assert_eq!(
            loaded.warnings,
            vec![
                ConfigWarning::UnknownKey { key: "top".into() },
                ConfigWarning::UnknownKey {
                    key: "editor.tab_widht".into()
                },
                ConfigWarning::UnknownKey {
                    key: "colors.x".into()
                },
            ]
        );
    }

    #[test]
    fn syntax_error_gives_defaults_and_a_warning() {
        let loaded = parse("[editor\ntab_width = 2");
        assert_eq!(loaded.config, Config::default());
        assert!(matches!(
            loaded.warnings.as_slice(),
            [ConfigWarning::Syntax { .. }]
        ));
    }

    #[test]
    fn missing_file_gives_defaults_without_warnings() {
        let loaded = load(Path::new(r"C:\definitely\not\here\config.toml"));
        assert_eq!(loaded.config, Config::default());
        assert!(loaded.warnings.is_empty());
    }

    #[test]
    fn save_creates_folder_and_round_trips() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("Bracket").join(CONFIG_FILE_FOR_TESTS);
        let config = Config {
            language: Language::English,
            font_family: "Cascadia Mono".into(),
            font_size: 13.5,
            tab_width: 8,
            line_ending: LineEnding::Cr,
            encoding: TextEncoding::UTF8_BOM,
        };
        save(&config, &path).unwrap();
        let loaded = load(&path);
        assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
        assert_eq!(loaded.config, config);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn save_keeps_comments_order_and_unknown_keys() {
        let dir = temp_dir("comments");
        let path = dir.join(CONFIG_FILE_FOR_TESTS);
        let original =
            "# My settings\n[editor]\n# Wide tabs\ntab_width = 8 # really\nmine = true\n";
        fs::write(&path, original).unwrap();

        let mut config = load(&path).config;
        config.tab_width = 2;
        save(&config, &path).unwrap();

        let saved = fs::read_to_string(&path).unwrap();
        assert!(
            saved.starts_with("# My settings\n[editor]\n# Wide tabs\n"),
            "{saved}"
        );
        assert!(saved.contains("tab_width = 2 # really"), "{saved}");
        assert!(saved.contains("mine = true"), "{saved}");
        assert!(
            saved.contains("\n[interface]\nlanguage = \"es\"\n"),
            "new sections are regular tables: {saved}"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn save_refuses_to_overwrite_an_invalid_file() {
        let dir = temp_dir("invalid");
        let path = dir.join(CONFIG_FILE_FOR_TESTS);
        fs::write(&path, "[editor\nbroken").unwrap();
        let err = save(&Config::default(), &path).unwrap_err();
        assert!(matches!(err, SaveError::ExistingFileInvalid { .. }));
        assert_eq!(fs::read_to_string(&path).unwrap(), "[editor\nbroken");
        fs::remove_dir_all(&dir).unwrap();
    }

    const CONFIG_FILE_FOR_TESTS: &str = "config.toml";
}
