//! The language registry: which languages Bracket knows, how files map to them and how each
//! one is shown (its badge on tabs).
//!
//! Seven languages are built in. External language packages (see `packages.rs`) add languages or
//! replace built-in ones with the same id, so every field is owned data rather than a
//! compile-time constant.

use std::path::{Path, PathBuf};

/// A short label drawn on tabs to identify the language, like `C#` or `{}`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Badge {
    pub text: String,
    /// Background color as RGB.
    pub color: [u8; 3],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Language {
    /// Stable identifier, e.g. `csharp`. Packages use it to override a built-in language.
    pub id: String,
    /// Display name, e.g. `C#`. Language names are not translated.
    pub name: String,
    /// File extensions without the dot, lowercase.
    pub extensions: Vec<String>,
    pub badge: Badge,
    /// Scope of the grammar that highlights it, e.g. `source.cs`. Empty for no highlighting.
    pub scope: String,
    /// Tab width for this language, instead of the configured one.
    pub tab_width: Option<usize>,
    /// Comment tokens and bracket pairs, for commenting lines and closing pairs.
    pub line_comment: Option<String>,
    pub block_comment: Option<(String, String)>,
    pub pairs: Vec<(String, String)>,
    /// The package folder it comes from; `None` for a built-in language.
    pub package: Option<PathBuf>,
}

/// Brackets and quotes closed in pairs, for the built-in languages.
fn common_pairs() -> Vec<(String, String)> {
    [("(", ")"), ("[", "]"), ("{", "}"), ("\"", "\""), ("'", "'")]
        .iter()
        .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
        .collect()
}

struct Builtin<'a> {
    id: &'a str,
    name: &'a str,
    scope: &'a str,
    extensions: &'a [&'a str],
    badge: &'a str,
    color: [u8; 3],
    line_comment: Option<&'a str>,
    block_comment: Option<(&'a str, &'a str)>,
}

impl From<Builtin<'_>> for Language {
    fn from(b: Builtin<'_>) -> Self {
        Self {
            id: b.id.to_owned(),
            name: b.name.to_owned(),
            extensions: b.extensions.iter().map(|e| (*e).to_owned()).collect(),
            badge: Badge {
                text: b.badge.to_owned(),
                color: b.color,
            },
            scope: b.scope.to_owned(),
            tab_width: None,
            line_comment: b.line_comment.map(str::to_owned),
            block_comment: b.block_comment.map(|(a, z)| (a.to_owned(), z.to_owned())),
            pairs: common_pairs(),
            package: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct LanguageRegistry {
    /// Packages first, so they win over built-in languages for an extension.
    languages: Vec<Language>,
}

impl LanguageRegistry {
    /// The languages built into the executable.
    pub fn builtin() -> Self {
        Self::with_packages(Vec::new())
    }

    /// The built-in languages plus `packages`, in priority order. A package with a built-in
    /// language's id replaces it; when two languages claim an extension, the first one wins.
    pub fn with_packages(packages: Vec<Language>) -> Self {
        let mut languages = packages;
        for language in builtin_languages() {
            if !languages.iter().any(|l| l.id == language.id) {
                languages.push(language);
            }
        }
        Self { languages }
    }

    pub fn languages(&self) -> &[Language] {
        &self.languages
    }

    pub fn get(&self, id: &str) -> Option<&Language> {
        self.languages.iter().find(|l| l.id == id)
    }

    /// The language of a file, by its extension (case-insensitive).
    pub fn for_path(&self, path: &Path) -> Option<&Language> {
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        self.languages
            .iter()
            .find(|l| l.extensions.contains(&extension))
    }
}

fn builtin_languages() -> Vec<Language> {
    let c_comments = (Some("//"), Some(("/*", "*/")));
    vec![
        Builtin {
            id: "csharp",
            name: "C#",
            scope: "source.cs",
            extensions: &["cs", "csx"],
            badge: "C#",
            color: [0x68, 0x21, 0x7A],
            line_comment: c_comments.0,
            block_comment: c_comments.1,
        }
        .into(),
        Builtin {
            id: "razor",
            name: "Razor",
            scope: "text.html.razor",
            extensions: &["razor", "cshtml"],
            badge: "@",
            color: [0x51, 0x2B, 0xD4],
            line_comment: None,
            block_comment: Some(("@*", "*@")),
        }
        .into(),
        Builtin {
            id: "css",
            name: "CSS",
            scope: "source.css",
            extensions: &["css"],
            badge: "#",
            color: [0x26, 0x4D, 0xE4],
            line_comment: None,
            block_comment: c_comments.1,
        }
        .into(),
        Builtin {
            id: "tsql",
            name: "T-SQL",
            scope: "source.tsql",
            extensions: &["sql"],
            badge: "SQL",
            color: [0xCC, 0x29, 0x27],
            line_comment: Some("--"),
            block_comment: c_comments.1,
        }
        .into(),
        Builtin {
            id: "json",
            name: "JSON",
            scope: "source.json",
            extensions: &["json", "jsonc"],
            badge: "{}",
            color: [0xB0, 0x8A, 0x10],
            line_comment: None,
            block_comment: None,
        }
        .into(),
        Builtin {
            id: "xml",
            name: "XML",
            scope: "text.xml",
            extensions: &[
                "xml", "xsig", "xsd", "xsl", "xslt", "config", "csproj", "vbproj", "props",
                "targets", "resx", "nuspec", "xaml",
            ],
            badge: "<>",
            color: [0xE3, 0x79, 0x33],
            line_comment: None,
            block_comment: Some(("<!--", "-->")),
        }
        .into(),
        rust(),
    ]
}

/// Rust, highlighted with syntect's bundled grammar. A quote is not closed in pairs: in Rust it
/// starts lifetimes (`'a`) more often than character literals.
fn rust() -> Language {
    let mut rust: Language = Builtin {
        id: "rust",
        name: "Rust",
        scope: "source.rust",
        extensions: &["rs"],
        badge: "RS",
        color: [0xB7, 0x41, 0x0E],
        line_comment: Some("//"),
        block_comment: Some(("/*", "*/")),
    }
    .into();
    rust.pairs.retain(|(open, _)| open != "'");
    rust
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_seven_builtin_languages_are_registered() {
        let registry = LanguageRegistry::builtin();
        let ids: Vec<&str> = registry.languages().iter().map(|l| l.id.as_str()).collect();
        assert_eq!(
            ids,
            ["csharp", "razor", "css", "tsql", "json", "xml", "rust"]
        );
    }

    #[test]
    fn rust_does_not_close_quotes_in_pairs() {
        let registry = LanguageRegistry::builtin();
        let opens = |id: &str| -> Vec<String> {
            let pairs = &registry.get(id).unwrap().pairs;
            pairs.iter().map(|(open, _)| open.clone()).collect()
        };
        assert!(!opens("rust").contains(&"'".to_owned()), "lifetimes: 'a");
        assert!(opens("rust").contains(&"\"".to_owned()));
        assert!(opens("csharp").contains(&"'".to_owned()));
    }

    #[test]
    fn files_map_to_languages_by_extension() {
        let registry = LanguageRegistry::builtin();
        let id = |path: &str| registry.for_path(Path::new(path)).map(|l| l.id.as_str());
        assert_eq!(id(r"C:\src\Program.cs"), Some("csharp"));
        assert_eq!(id("Index.cshtml"), Some("razor"));
        assert_eq!(
            id("site.CSS"),
            Some("css"),
            "extensions are case-insensitive"
        );
        assert_eq!(id("create_tables.sql"), Some("tsql"));
        assert_eq!(id("appsettings.json"), Some("json"));
        assert_eq!(id("invoice.xsig"), Some("xml"));
        assert_eq!(id("Bracket.csproj"), Some("xml"));
        assert_eq!(id("web.config"), Some("xml"));
        assert_eq!(id(r"src\main.rs"), Some("rust"));
        assert_eq!(id("notes.txt"), None);
        assert_eq!(id("Makefile"), None);
    }

    #[test]
    fn every_language_has_a_short_badge() {
        for language in LanguageRegistry::builtin().languages() {
            let len = language.badge.text.chars().count();
            assert!((1..=3).contains(&len), "{} badge too long", language.id);
            assert!(
                language
                    .extensions
                    .iter()
                    .all(|e| *e == e.to_ascii_lowercase())
            );
        }
    }

    #[test]
    fn every_builtin_language_has_its_grammar() {
        for language in LanguageRegistry::builtin().languages() {
            assert!(
                crate::highlight::Engine::new(&language.scope).is_some(),
                "{} has no grammar for {}",
                language.id,
                language.scope
            );
        }
    }

    #[test]
    fn languages_can_be_found_by_id() {
        let registry = LanguageRegistry::builtin();
        assert_eq!(registry.get("json").unwrap().badge.text, "{}");
        assert!(registry.get("cobol").is_none());
    }

    #[test]
    fn packages_add_languages_and_replace_builtin_ones() {
        let package = |id: &str, extensions: &[&str]| Language {
            id: id.to_owned(),
            extensions: extensions.iter().map(|e| (*e).to_owned()).collect(),
            package: Some(PathBuf::from(id)),
            ..builtin_languages()[0].clone()
        };
        let registry = LanguageRegistry::with_packages(vec![
            package("python", &["py"]),
            package("csharp", &["cs"]),
            package("mysql", &["sql"]),
        ]);
        let id = |path: &str| registry.for_path(Path::new(path)).map(|l| l.id.as_str());
        assert_eq!(id("a.py"), Some("python"));
        assert_eq!(id("a.csx"), None, "the replaced C# no longer claims .csx");
        assert_eq!(id("a.sql"), Some("mysql"), "a package wins an extension");
        assert_eq!(registry.languages().len(), 3 + 6);
        assert!(registry.get("csharp").unwrap().package.is_some());
    }
}
