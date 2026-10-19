//! External language packages: folders that add languages or replace built-in ones.
//!
//! ```text
//! languages/<id>/language.toml          required: format_version, name, extensions...
//! languages/<id>/syntax.sublime-syntax  optional: the grammar (Sublime Text format, version 1)
//! languages/<id>/symbols.toml           optional: regex symbol rules, read by bracket-index
//! ```
//!
//! ```toml
//! format_version = 1
//! name = "Python"
//! extensions = ["py", "pyw"]
//! scope = "source.python"     # a grammar Bracket already has, instead of syntax.sublime-syntax
//! tab_width = 4
//! line_comment = "#"
//! block_comment = ['"""', '"""']
//! pairs = [["(", ")"], ["[", "]"], ["{", "}"]]
//!
//! [badge]
//! text = "Py"
//! color = "#3776AB"
//! ```
//!
//! Folders are searched in priority order and the first package with an id wins. Problems never
//! stop Bracket: a bad package is skipped with a warning. The manifests are read at startup
//! (they are small); grammars are parsed later, on the highlighter thread, when first needed.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use toml_edit::{DocumentMut, Item, Table};

use crate::languages::{Badge, Language};

pub const FORMAT_VERSION: i64 = 1;
pub const MANIFEST: &str = "language.toml";
pub const GRAMMAR: &str = "syntax.sublime-syntax";
pub const SYMBOLS: &str = "symbols.toml";
const TAB_WIDTH_MAX: i64 = 16;
const DEFAULT_BADGE_COLOR: [u8; 3] = [0x6E, 0x76, 0x81];

/// A problem with a package. The interface turns it into a translated message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PackageWarning {
    Unreadable {
        path: PathBuf,
        error: String,
    },
    /// `language.toml` is not valid TOML.
    Syntax {
        path: PathBuf,
        error: String,
    },
    MissingKey {
        path: PathBuf,
        key: String,
    },
    InvalidValue {
        path: PathBuf,
        key: String,
    },
    /// Written for a newer Bracket (or not a number).
    UnsupportedVersion {
        path: PathBuf,
        found: String,
    },
    /// The grammar has no top-level `scope:`.
    GrammarWithoutScope {
        path: PathBuf,
    },
    /// The grammar could not be parsed (found when it is first needed).
    Grammar {
        path: PathBuf,
        error: String,
    },
}

#[derive(Debug, Default)]
pub struct Packages {
    /// In priority order.
    pub languages: Vec<Language>,
    /// Grammar files of those languages, to compile with the built-in ones.
    pub grammars: Vec<PathBuf>,
    pub warnings: Vec<PackageWarning>,
}

/// Reads the packages in `dirs`, highest priority first. Missing folders are fine.
pub fn load(dirs: &[PathBuf]) -> Packages {
    let mut packages = Packages::default();
    for dir in dirs {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        let mut folders: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|path| path.join(MANIFEST).is_file())
            .collect();
        folders.sort();
        for folder in folders {
            match load_package(&folder) {
                Ok((language, grammar)) => {
                    if packages.languages.iter().any(|l| l.id == language.id) {
                        continue; // a higher-priority folder already has it
                    }
                    packages.grammars.extend(grammar);
                    packages.languages.push(language);
                }
                Err(warning) => packages.warnings.push(warning),
            }
        }
    }
    packages
}

/// The package's language and its grammar file, if it has one.
fn load_package(folder: &Path) -> Result<(Language, Option<PathBuf>), PackageWarning> {
    let path = folder.join(MANIFEST);
    let text = fs::read_to_string(&path).map_err(|e| PackageWarning::Unreadable {
        path: path.clone(),
        error: e.to_string(),
    })?;
    let doc = text
        .parse::<DocumentMut>()
        .map_err(|e| PackageWarning::Syntax {
            path: path.clone(),
            error: first_line(&e.to_string()),
        })?;
    let manifest = Manifest {
        path: &path,
        doc: &doc,
    };

    match doc.get("format_version") {
        None => return Err(manifest.missing("format_version")),
        Some(item) if item.as_integer() == Some(FORMAT_VERSION) => {}
        Some(item) => {
            return Err(PackageWarning::UnsupportedVersion {
                path: path.clone(),
                found: item.to_string().trim().to_owned(),
            });
        }
    }
    let name = manifest
        .string("name")?
        .filter(|n| !n.trim().is_empty())
        .ok_or_else(|| manifest.missing("name"))?;
    let extensions = manifest
        .strings("extensions")?
        .ok_or_else(|| manifest.missing("extensions"))?
        .into_iter()
        .map(|e| e.trim_start_matches('.').to_ascii_lowercase())
        .collect();
    let tab_width = match doc.get("tab_width") {
        None => None,
        Some(item) => Some(
            item.as_integer()
                .filter(|w| (1..=TAB_WIDTH_MAX).contains(w))
                .ok_or_else(|| manifest.invalid("tab_width"))? as usize,
        ),
    };
    let block_comment = match manifest.strings("block_comment")? {
        None => None,
        Some(tokens) => match <[String; 2]>::try_from(tokens) {
            Ok([open, close]) => Some((open, close)),
            Err(_) => return Err(manifest.invalid("block_comment")),
        },
    };
    let pairs = match doc.get("pairs") {
        None => Vec::new(),
        Some(item) => parse_pairs(item).ok_or_else(|| manifest.invalid("pairs"))?,
    };
    let badge = match doc.get("badge") {
        None => default_badge(&name),
        Some(item) => {
            let table = item.as_table().ok_or_else(|| manifest.invalid("badge"))?;
            parse_badge(table, &name).ok_or_else(|| manifest.invalid("badge"))?
        }
    };

    let grammar = folder.join(GRAMMAR);
    let (scope, grammar) = if grammar.is_file() {
        let source = fs::read_to_string(&grammar).map_err(|e| PackageWarning::Unreadable {
            path: grammar.clone(),
            error: e.to_string(),
        })?;
        let scope = grammar_scope(&source).ok_or_else(|| PackageWarning::GrammarWithoutScope {
            path: grammar.clone(),
        })?;
        (scope, Some(grammar))
    } else {
        (manifest.string("scope")?.unwrap_or_default(), None)
    };

    let symbols_path = folder.join(SYMBOLS);
    let symbols = if symbols_path.is_file() {
        Some(
            fs::read_to_string(&symbols_path).map_err(|e| PackageWarning::Unreadable {
                path: symbols_path.clone(),
                error: e.to_string(),
            })?,
        )
    } else {
        None
    };

    let id = folder
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let language = Language {
        id,
        name,
        extensions,
        badge,
        scope,
        tab_width,
        line_comment: manifest.string("line_comment")?,
        block_comment,
        pairs,
        package: Some(folder.to_path_buf()),
        symbols,
    };
    Ok((language, grammar))
}

struct Manifest<'a> {
    path: &'a Path,
    doc: &'a DocumentMut,
}

impl Manifest<'_> {
    fn missing(&self, key: &str) -> PackageWarning {
        PackageWarning::MissingKey {
            path: self.path.to_path_buf(),
            key: key.to_owned(),
        }
    }

    fn invalid(&self, key: &str) -> PackageWarning {
        PackageWarning::InvalidValue {
            path: self.path.to_path_buf(),
            key: key.to_owned(),
        }
    }

    /// An optional string; present with another type is an error.
    fn string(&self, key: &str) -> Result<Option<String>, PackageWarning> {
        match self.doc.get(key) {
            None => Ok(None),
            Some(item) => item
                .as_str()
                .map(|s| Some(s.to_owned()))
                .ok_or_else(|| self.invalid(key)),
        }
    }

    /// An optional list of strings.
    fn strings(&self, key: &str) -> Result<Option<Vec<String>>, PackageWarning> {
        match self.doc.get(key) {
            None => Ok(None),
            Some(item) => string_list(item).map(Some).ok_or_else(|| self.invalid(key)),
        }
    }
}

fn string_list(item: &Item) -> Option<Vec<String>> {
    item.as_array()?
        .iter()
        .map(|v| v.as_str().map(str::to_owned))
        .collect()
}

fn parse_pairs(item: &Item) -> Option<Vec<(String, String)>> {
    item.as_array()?
        .iter()
        .map(|pair| {
            let pair = pair.as_array()?;
            match (pair.len(), pair.get(0)?.as_str(), pair.get(1)?.as_str()) {
                (2, Some(open), Some(close)) => Some((open.to_owned(), close.to_owned())),
                _ => None,
            }
        })
        .collect()
}

fn parse_badge(table: &Table, name: &str) -> Option<Badge> {
    let default = default_badge(name);
    let text = match table.get("text") {
        None => default.text,
        Some(item) => item
            .as_str()
            .filter(|t| (1..=3).contains(&t.chars().count()))?
            .to_owned(),
    };
    let color = match table.get("color") {
        None => default.color,
        Some(item) => parse_color(item.as_str()?)?,
    };
    Some(Badge { text, color })
}

/// The first two characters of the name, on a grey badge.
fn default_badge(name: &str) -> Badge {
    Badge {
        text: name.trim().chars().take(2).collect(),
        color: DEFAULT_BADGE_COLOR,
    }
}

/// `#RRGGBB`.
fn parse_color(text: &str) -> Option<[u8; 3]> {
    let hex = text.strip_prefix('#')?;
    if hex.len() != 6 || !hex.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

/// The top-level `scope:` of a `.sublime-syntax` file, read without parsing the whole YAML
/// (the full parse happens later, when the grammar is needed).
fn grammar_scope(source: &str) -> Option<String> {
    source.lines().find_map(|line| {
        let value = line.strip_prefix("scope:")?.trim();
        let value = value.split(" #").next()?.trim().trim_matches(['\'', '"']);
        (!value.is_empty()).then(|| value.to_owned())
    })
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or_default().to_owned()
}

/// Reads a grammar file and parses it (on the highlighter thread).
pub(crate) fn parse_grammar(
    path: &Path,
) -> Result<syntect::parsing::SyntaxDefinition, PackageWarning> {
    let warning = |error: String| PackageWarning::Grammar {
        path: path.to_path_buf(),
        error,
    };
    let source = fs::read_to_string(path).map_err(|e: io::Error| warning(e.to_string()))?;
    syntect::parsing::SyntaxDefinition::load_from_str(&source, true, None)
        .map_err(|e| warning(first_line(&e.to_string())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("bracket-packages-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn package(root: &Path, id: &str, manifest: &str) -> PathBuf {
        let folder = root.join(id);
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join(MANIFEST), manifest).unwrap();
        folder
    }

    const PYTHON: &str = r##"
format_version = 1
name = "Python"
extensions = ["py", ".PYW"]
scope = "source.python"
tab_width = 4
line_comment = "#"
block_comment = ['"""', '"""']
pairs = [["(", ")"], ["[", "]"]]

[badge]
text = "Py"
color = "#3776AB"
"##;

    #[test]
    fn a_complete_package_is_read() {
        let root = temp_dir("complete");
        package(&root, "python", PYTHON);
        let packages = load(std::slice::from_ref(&root));
        assert_eq!(packages.warnings, []);
        let python = &packages.languages[0];
        assert_eq!(python.id, "python");
        assert_eq!(python.extensions, ["py", "pyw"]);
        assert_eq!(python.scope, "source.python");
        assert_eq!(python.tab_width, Some(4));
        assert_eq!(python.line_comment.as_deref(), Some("#"));
        assert_eq!(python.badge.color, [0x37, 0x76, 0xAB]);
        assert_eq!(python.pairs.len(), 2);
        assert!(packages.grammars.is_empty());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn symbol_rules_are_read_as_text() {
        let root = temp_dir("symbols");
        let folder = package(&root, "python", PYTHON);
        let rules =
            "format_version = 1\n[[symbol]]\nkind = \"function\"\npattern = '^def (?<name>\\w+)'\n";
        fs::write(folder.join(SYMBOLS), rules).unwrap();
        package(
            &root,
            "ini",
            "format_version = 1\nname = \"INI\"\nextensions = [\"ini\"]\n",
        );
        let packages = load(std::slice::from_ref(&root));
        let python = packages
            .languages
            .iter()
            .find(|l| l.id == "python")
            .unwrap();
        assert_eq!(python.symbols.as_deref(), Some(rules));
        let ini = packages.languages.iter().find(|l| l.id == "ini").unwrap();
        assert_eq!(ini.symbols, None);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_grammar_file_gives_the_scope() {
        let root = temp_dir("grammar");
        let folder = package(
            &root,
            "ini",
            "format_version = 1\nname = \"INI\"\nextensions = [\"ini\"]\n",
        );
        fs::write(
            folder.join(GRAMMAR),
            "%YAML 1.2\n---\nname: INI\nscope: source.ini # comment\ncontexts:\n  main: []\n",
        )
        .unwrap();
        let packages = load(std::slice::from_ref(&root));
        assert_eq!(packages.languages[0].scope, "source.ini");
        assert_eq!(packages.languages[0].badge.text, "IN");
        assert_eq!(packages.grammars, [folder.join(GRAMMAR)]);
        assert!(parse_grammar(&packages.grammars[0]).is_ok());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn earlier_folders_win() {
        let first = temp_dir("first");
        let second = temp_dir("second");
        package(&first, "python", PYTHON);
        package(
            &second,
            "python",
            "format_version = 1\nname = \"Other\"\nextensions = []\n",
        );
        package(
            &second,
            "lua",
            "format_version = 1\nname = \"Lua\"\nextensions = [\"lua\"]\n",
        );
        let packages = load(&[first.clone(), second.clone()]);
        let names: Vec<&str> = packages.languages.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["Python", "Lua"]);
        fs::remove_dir_all(&first).unwrap();
        fs::remove_dir_all(&second).unwrap();
    }

    #[test]
    fn bad_packages_are_skipped_with_a_warning() {
        let root = temp_dir("bad");
        package(
            &root,
            "a",
            "format_version = 2\nname = \"A\"\nextensions = []\n",
        );
        package(&root, "b", "name = \"B\"\nextensions = []\n");
        package(
            &root,
            "c",
            "format_version = 1\nname = \"C\"\nextensions = \"c\"\n",
        );
        package(&root, "d", "format_version = 1\nname = [\n");
        package(
            &root,
            "e",
            "format_version = 1\nname = \"E\"\nextensions = []\n[badge]\ncolor = \"red\"\n",
        );
        let folder = package(
            &root,
            "f",
            "format_version = 1\nname = \"F\"\nextensions = []\n",
        );
        fs::write(folder.join(GRAMMAR), "name: F\ncontexts: {}\n").unwrap();
        package(
            &root,
            "ok",
            "format_version = 1\nname = \"Ok\"\nextensions = [\"ok\"]\n",
        );

        let packages = load(std::slice::from_ref(&root));
        assert_eq!(packages.languages.len(), 1);
        let kinds: Vec<String> = packages
            .warnings
            .iter()
            .map(|w| format!("{w:?}").split(' ').next().unwrap().to_owned())
            .collect();
        assert_eq!(
            kinds,
            [
                "UnsupportedVersion",
                "MissingKey",
                "InvalidValue",
                "Syntax",
                "InvalidValue",
                "GrammarWithoutScope"
            ]
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn missing_folders_are_fine() {
        let packages = load(&[PathBuf::from(r"Z:\no\such\languages")]);
        assert!(packages.languages.is_empty() && packages.warnings.is_empty());
    }

    #[test]
    fn invalid_grammars_are_reported_when_parsed() {
        let root = temp_dir("broken-grammar");
        let path = root.join(GRAMMAR);
        fs::write(&path, "scope: source.x\ncontexts: {main: [\n").unwrap();
        assert!(matches!(
            parse_grammar(&path),
            Err(PackageWarning::Grammar { .. })
        ));
        fs::remove_dir_all(&root).unwrap();
    }
}
