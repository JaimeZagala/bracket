//! User-visible texts. Every string shown in the interface comes from here.
//!
//! The texts live in one TOML catalog per language (`locales/es.toml`, the reference, and
//! `locales/en.toml`), embedded in the executable and parsed on first use. Code asks for texts
//! through typed methods (`texts.untitled()`), so a typo does not compile; tests check that both
//! catalogs have exactly the same keys and the same `{placeholders}`, and that every method
//! finds its key.
//!
//! Decision record: `docs/es/decisions/0002-i18n.md`.

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::OnceLock;

use bracket_session::ConfigWarning;
pub use bracket_session::Language;

const SPANISH_CATALOG: &str = include_str!("../locales/es.toml");
const ENGLISH_CATALOG: &str = include_str!("../locales/en.toml");

/// Languages offered by the interface, in menu order.
pub const LANGUAGES: [Language; 2] = [Language::Spanish, Language::English];

/// The texts of one language.
pub struct Texts {
    strings: HashMap<String, String>,
}

/// Texts for `language`. Each catalog is parsed once and kept for the rest of the run.
pub fn texts(language: Language) -> &'static Texts {
    static SPANISH: OnceLock<Texts> = OnceLock::new();
    static ENGLISH: OnceLock<Texts> = OnceLock::new();
    match language {
        Language::Spanish => SPANISH.get_or_init(|| Texts::parse(SPANISH_CATALOG)),
        Language::English => ENGLISH.get_or_init(|| Texts::parse(ENGLISH_CATALOG)),
    }
}

impl Texts {
    /// Built-in catalogs are checked by the tests below, so a parse error is a bug.
    fn parse(source: &str) -> Self {
        let doc: toml_edit::DocumentMut = source.parse().expect("built-in catalog is valid TOML");
        let strings = doc
            .iter()
            .filter_map(|(key, item)| Some((key.to_owned(), item.as_str()?.to_owned())))
            .collect();
        Self { strings }
    }

    /// The text for `key`, or the key itself if it is missing (tests prevent that).
    fn get<'a>(&'a self, key: &'a str) -> &'a str {
        self.strings.get(key).map(String::as_str).unwrap_or(key)
    }

    /// The text for `key` with each `{name}` replaced by its value.
    fn fill(&self, key: &str, args: &[(&str, &dyn Display)]) -> String {
        let mut text = self.get(key).to_owned();
        for (name, value) in args {
            text = text.replace(&format!("{{{name}}}"), &value.to_string());
        }
        text
    }

    /// The language's own name, as shown in the language menu.
    pub fn language_name(&self) -> &str {
        self.get("language_name")
    }

    pub fn language_tooltip(&self) -> &str {
        self.get("language_tooltip")
    }

    pub fn untitled(&self, number: u32) -> String {
        self.fill("untitled", &[("number", &number)])
    }

    pub fn loading(&self) -> &str {
        self.get("loading")
    }

    pub fn read_only(&self) -> &str {
        self.get("read_only")
    }

    pub fn drop_hint(&self) -> &str {
        self.get("drop_hint")
    }

    pub fn position(&self, line: usize, column: usize) -> String {
        self.fill("position", &[("line", &line), ("column", &column)])
    }

    pub fn menu_file(&self) -> &str {
        self.get("menu_file")
    }

    pub fn menu_new(&self) -> &str {
        self.get("menu_new")
    }

    pub fn menu_open(&self) -> &str {
        self.get("menu_open")
    }

    pub fn menu_save(&self) -> &str {
        self.get("menu_save")
    }

    pub fn menu_save_as(&self) -> &str {
        self.get("menu_save_as")
    }

    pub fn menu_close_tab(&self) -> &str {
        self.get("menu_close_tab")
    }

    pub fn menu_exit(&self) -> &str {
        self.get("menu_exit")
    }

    pub fn dialog_open_title(&self) -> &str {
        self.get("dialog_open_title")
    }

    pub fn dialog_save_as_title(&self) -> &str {
        self.get("dialog_save_as_title")
    }

    pub fn saved(&self) -> &str {
        self.get("saved")
    }

    pub fn save_read_only(&self) -> &str {
        self.get("save_read_only")
    }

    pub fn open_failed(&self, error: &str) -> String {
        self.fill("open_failed", &[("error", &error)])
    }

    pub fn save_failed(&self, error: &str) -> String {
        self.fill("save_failed", &[("error", &error)])
    }

    pub fn unsaved_title(&self) -> &str {
        self.get("unsaved_title")
    }

    pub fn unsaved_message(&self, name: &str) -> String {
        self.fill("unsaved_message", &[("name", &name)])
    }

    pub fn unsaved_many_message(&self, count: usize) -> String {
        self.fill("unsaved_many_message", &[("count", &count)])
    }

    pub fn save(&self) -> &str {
        self.get("save")
    }

    pub fn save_all(&self) -> &str {
        self.get("save_all")
    }

    pub fn discard(&self) -> &str {
        self.get("discard")
    }

    pub fn cancel(&self) -> &str {
        self.get("cancel")
    }

    pub fn config_save_failed(&self, error: &str) -> String {
        self.fill("config_save_failed", &[("error", &error)])
    }

    pub fn font_missing(&self, family: &str, used: &str) -> String {
        self.fill("font_missing", &[("family", &family), ("used", &used)])
    }

    pub fn encoding_tooltip(&self) -> &str {
        self.get("encoding_tooltip")
    }

    pub fn reopen_with_encoding(&self) -> &str {
        self.get("reopen_with_encoding")
    }

    pub fn save_with_encoding(&self) -> &str {
        self.get("save_with_encoding")
    }

    pub fn reopen_needs_clean(&self) -> &str {
        self.get("reopen_needs_clean")
    }

    pub fn save_encode_failed(&self, encoding: &str, character: &str) -> String {
        self.fill(
            "save_encode_failed",
            &[("encoding", &encoding), ("character", &character)],
        )
    }

    pub fn line_ending_tooltip(&self) -> &str {
        self.get("line_ending_tooltip")
    }

    pub fn convert_line_endings(&self, line_ending: &str) -> String {
        self.fill("convert_line_endings", &[("line_ending", &line_ending)])
    }

    pub fn session_damaged(&self, error: &str) -> String {
        self.fill("session_damaged", &[("error", &error)])
    }

    pub fn session_save_failed(&self, error: &str) -> String {
        self.fill("session_save_failed", &[("error", &error)])
    }

    pub fn disk_changed(&self, name: &str) -> String {
        self.fill("disk_changed", &[("name", &name)])
    }

    pub fn disk_reload(&self) -> &str {
        self.get("disk_reload")
    }

    pub fn disk_reload_tooltip(&self) -> &str {
        self.get("disk_reload_tooltip")
    }

    pub fn disk_keep(&self) -> &str {
        self.get("disk_keep")
    }

    pub fn disk_missing(&self, name: &str) -> String {
        self.fill("disk_missing", &[("name", &name)])
    }

    pub fn disk_dismiss(&self) -> &str {
        self.get("disk_dismiss")
    }

    pub fn disk_reloaded(&self, name: &str) -> String {
        self.fill("disk_reloaded", &[("name", &name)])
    }

    /// Translated text for a configuration warning.
    pub fn config_warning(&self, warning: &ConfigWarning) -> String {
        match warning {
            ConfigWarning::Unreadable { error } => {
                self.fill("config_unreadable", &[("error", error)])
            }
            ConfigWarning::Syntax { error } => self.fill("config_syntax", &[("error", error)]),
            ConfigWarning::InvalidValue { key, found } => {
                self.fill("config_invalid_value", &[("key", key), ("found", found)])
            }
            ConfigWarning::UnknownKey { key } => self.fill("config_unknown_key", &[("key", key)]),
        }
    }

    /// "(and N more warnings)", with the singular form for one.
    pub fn more_warnings(&self, count: usize) -> String {
        match count {
            1 => self.get("more_warnings_one").to_owned(),
            n => self.fill("more_warnings_many", &[("count", &n)]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// `{name}` placeholders in a text.
    fn placeholders(text: &str) -> BTreeSet<String> {
        let mut found = BTreeSet::new();
        let mut rest = text;
        while let Some(start) = rest.find('{') {
            let Some(len) = rest[start..].find('}') else {
                break;
            };
            found.insert(rest[start + 1..start + len].to_owned());
            rest = &rest[start + len + 1..];
        }
        found
    }

    #[test]
    fn catalogs_have_the_same_keys_and_placeholders() {
        let es = texts(Language::Spanish);
        let en = texts(Language::English);
        let es_keys: BTreeSet<_> = es.strings.keys().collect();
        let en_keys: BTreeSet<_> = en.strings.keys().collect();
        assert_eq!(
            es_keys, en_keys,
            "es.toml and en.toml must have the same keys"
        );
        for key in es_keys {
            assert_eq!(
                placeholders(&es.strings[key]),
                placeholders(&en.strings[key]),
                "placeholders of `{key}` differ between languages"
            );
        }
    }

    #[test]
    fn catalogs_contain_only_text_values() {
        for source in [SPANISH_CATALOG, ENGLISH_CATALOG] {
            let doc: toml_edit::DocumentMut = source.parse().unwrap();
            for (key, item) in doc.iter() {
                assert!(item.as_str().is_some(), "`{key}` must be a plain string");
            }
        }
    }

    /// Each method paired with the key it reads. Add new methods here.
    #[test]
    fn every_method_finds_its_key() {
        let warning = ConfigWarning::UnknownKey { key: "k".into() };
        for language in LANGUAGES {
            let t = texts(language);
            let pairs = [
                ("language_name", t.language_name().to_owned()),
                ("language_tooltip", t.language_tooltip().to_owned()),
                ("untitled", t.untitled(1)),
                ("loading", t.loading().to_owned()),
                ("read_only", t.read_only().to_owned()),
                ("drop_hint", t.drop_hint().to_owned()),
                ("position", t.position(1, 2)),
                ("menu_file", t.menu_file().to_owned()),
                ("menu_new", t.menu_new().to_owned()),
                ("menu_open", t.menu_open().to_owned()),
                ("menu_save", t.menu_save().to_owned()),
                ("menu_save_as", t.menu_save_as().to_owned()),
                ("menu_close_tab", t.menu_close_tab().to_owned()),
                ("menu_exit", t.menu_exit().to_owned()),
                ("dialog_open_title", t.dialog_open_title().to_owned()),
                ("dialog_save_as_title", t.dialog_save_as_title().to_owned()),
                ("saved", t.saved().to_owned()),
                ("save_read_only", t.save_read_only().to_owned()),
                ("open_failed", t.open_failed("e")),
                ("save_failed", t.save_failed("e")),
                ("unsaved_title", t.unsaved_title().to_owned()),
                ("unsaved_message", t.unsaved_message("a.txt")),
                ("unsaved_many_message", t.unsaved_many_message(2)),
                ("save", t.save().to_owned()),
                ("save_all", t.save_all().to_owned()),
                ("discard", t.discard().to_owned()),
                ("cancel", t.cancel().to_owned()),
                ("config_save_failed", t.config_save_failed("e")),
                ("config_unknown_key", t.config_warning(&warning)),
                ("more_warnings_one", t.more_warnings(1)),
                ("more_warnings_many", t.more_warnings(2)),
                ("font_missing", t.font_missing("A", "B")),
                ("encoding_tooltip", t.encoding_tooltip().to_owned()),
                ("reopen_with_encoding", t.reopen_with_encoding().to_owned()),
                ("save_with_encoding", t.save_with_encoding().to_owned()),
                ("reopen_needs_clean", t.reopen_needs_clean().to_owned()),
                ("save_encode_failed", t.save_encode_failed("X", "y")),
                ("line_ending_tooltip", t.line_ending_tooltip().to_owned()),
                ("convert_line_endings", t.convert_line_endings("LF")),
                ("session_damaged", t.session_damaged("e")),
                ("session_save_failed", t.session_save_failed("e")),
                ("disk_changed", t.disk_changed("a.txt")),
                ("disk_reload", t.disk_reload().to_owned()),
                ("disk_reload_tooltip", t.disk_reload_tooltip().to_owned()),
                ("disk_keep", t.disk_keep().to_owned()),
                ("disk_missing", t.disk_missing("a.txt")),
                ("disk_dismiss", t.disk_dismiss().to_owned()),
                ("disk_reloaded", t.disk_reloaded("a.txt")),
            ];
            for (key, text) in pairs {
                assert!(
                    t.strings.contains_key(key),
                    "`{key}` missing in {language:?}"
                );
                assert_ne!(text, key, "`{key}` fell back to the key in {language:?}");
                assert!(!text.contains('{'), "unfilled placeholder in `{text}`");
            }
        }
    }

    #[test]
    fn placeholders_are_filled() {
        assert_eq!(texts(Language::Spanish).position(3, 7), "Lín 3, Col 7");
        assert_eq!(texts(Language::English).position(3, 7), "Ln 3, Col 7");
        assert_eq!(texts(Language::Spanish).more_warnings(1), "(y 1 aviso más)");
        assert_eq!(
            texts(Language::English).more_warnings(3),
            "(3 more warnings)"
        );
    }

    #[test]
    fn spanish_is_the_default() {
        assert_eq!(Language::default(), Language::Spanish);
        assert_eq!(LANGUAGES[0], Language::Spanish);
    }
}
