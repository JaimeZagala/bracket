//! The language registry: which languages Bracket knows, how files map to them and how each
//! one is shown (its badge on tabs).
//!
//! Phase 1 starts with the six built-in languages. External language packages (a `languages/`
//! folder with `language.toml` files) will add to or override these entries, so every field is
//! owned data rather than a compile-time constant.

use std::path::Path;

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
}

impl Language {
    fn builtin(id: &str, name: &str, extensions: &[&str], badge: &str, color: [u8; 3]) -> Self {
        Self {
            id: id.to_owned(),
            name: name.to_owned(),
            extensions: extensions.iter().map(|e| (*e).to_owned()).collect(),
            badge: Badge {
                text: badge.to_owned(),
                color,
            },
        }
    }
}

#[derive(Clone, Debug)]
pub struct LanguageRegistry {
    languages: Vec<Language>,
}

impl LanguageRegistry {
    /// The languages built into the executable.
    pub fn builtin() -> Self {
        Self {
            languages: vec![
                Language::builtin("csharp", "C#", &["cs", "csx"], "C#", [0x68, 0x21, 0x7A]),
                Language::builtin(
                    "razor",
                    "Razor",
                    &["razor", "cshtml"],
                    "@",
                    [0x51, 0x2B, 0xD4],
                ),
                Language::builtin("css", "CSS", &["css"], "#", [0x26, 0x4D, 0xE4]),
                Language::builtin("tsql", "T-SQL", &["sql"], "SQL", [0xCC, 0x29, 0x27]),
                Language::builtin("json", "JSON", &["json", "jsonc"], "{}", [0xB0, 0x8A, 0x10]),
                Language::builtin(
                    "xml",
                    "XML",
                    &[
                        "xml", "xsig", "xsd", "xsl", "xslt", "config", "csproj", "vbproj", "props",
                        "targets", "resx", "nuspec", "xaml",
                    ],
                    "<>",
                    [0xE3, 0x79, 0x33],
                ),
            ],
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_six_builtin_languages_are_registered() {
        let registry = LanguageRegistry::builtin();
        let ids: Vec<&str> = registry.languages().iter().map(|l| l.id.as_str()).collect();
        assert_eq!(ids, ["csharp", "razor", "css", "tsql", "json", "xml"]);
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
    fn languages_can_be_found_by_id() {
        let registry = LanguageRegistry::builtin();
        assert_eq!(registry.get("json").unwrap().badge.text, "{}");
        assert!(registry.get("cobol").is_none());
    }
}
