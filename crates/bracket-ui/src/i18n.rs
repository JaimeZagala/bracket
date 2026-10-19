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

use std::path::Path;

pub use bracket_session::Language;
use bracket_session::{Appearance, ConfigWarning};
use bracket_syntax::PackageWarning;

const SPANISH_CATALOG: &str = include_str!("../locales/es.toml");
const ENGLISH_CATALOG: &str = include_str!("../locales/en.toml");

/// Languages offered by the interface, in menu order.
pub const LANGUAGES: [Language; 2] = [Language::Spanish, Language::English];

/// The texts of one language.
pub struct Texts {
    strings: HashMap<String, String>,
}

/// The package a file belongs to: the name of the folder that contains it.
fn package_of(path: &Path) -> String {
    path.parent()
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
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

    pub fn selections(&self, count: usize) -> String {
        self.fill("selections", &[("count", &count)])
    }

    pub fn selected_chars(&self, count: usize) -> String {
        self.fill("selected_chars", &[("count", &count)])
    }

    pub fn plain_text(&self) -> &str {
        self.get("plain_text")
    }

    pub fn document_language_tooltip(&self) -> &str {
        self.get("document_language_tooltip")
    }

    pub fn read_only_short(&self) -> &str {
        self.get("read_only_short")
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

    pub fn menu_open_folder(&self) -> &str {
        self.get("menu_open_folder")
    }

    pub fn menu_close_folder(&self) -> &str {
        self.get("menu_close_folder")
    }

    pub fn menu_quick_open(&self) -> &str {
        self.get("menu_quick_open")
    }

    pub fn quick_open_hint(&self) -> &str {
        self.get("quick_open_hint")
    }

    pub fn quick_open_indexing(&self) -> &str {
        self.get("quick_open_indexing")
    }

    pub fn quick_open_no_results(&self) -> &str {
        self.get("quick_open_no_results")
    }

    pub fn quick_open_go_to_line(&self, line: &str) -> String {
        self.fill("quick_open_go_to_line", &[("line", &line)])
    }

    pub fn quick_open_count(&self, count: &str) -> String {
        self.fill("quick_open_count", &[("count", &count)])
    }

    pub fn menu_go(&self) -> &str {
        self.get("menu_go")
    }

    pub fn menu_go_to_line(&self) -> &str {
        self.get("menu_go_to_line")
    }

    pub fn menu_command_palette(&self) -> &str {
        self.get("menu_command_palette")
    }

    pub fn menu_next_tab(&self) -> &str {
        self.get("menu_next_tab")
    }

    pub fn menu_previous_tab(&self) -> &str {
        self.get("menu_previous_tab")
    }

    pub fn go_to_line_no_document(&self) -> &str {
        self.get("go_to_line_no_document")
    }

    pub fn commands_no_results(&self) -> &str {
        self.get("commands_no_results")
    }

    pub fn go_to_line_hint(&self, count: &str) -> String {
        self.fill("go_to_line_hint", &[("count", &count)])
    }

    pub fn menu_edit(&self) -> &str {
        self.get("menu_edit")
    }

    pub fn menu_find(&self) -> &str {
        self.get("menu_find")
    }

    pub fn menu_find_next(&self) -> &str {
        self.get("menu_find_next")
    }

    pub fn menu_find_previous(&self) -> &str {
        self.get("menu_find_previous")
    }

    pub fn find_hint(&self) -> &str {
        self.get("find_hint")
    }

    pub fn find_match_case(&self) -> &str {
        self.get("find_match_case")
    }

    pub fn find_whole_word(&self) -> &str {
        self.get("find_whole_word")
    }

    pub fn find_regex(&self) -> &str {
        self.get("find_regex")
    }

    pub fn find_previous_tip(&self) -> &str {
        self.get("find_previous_tip")
    }

    pub fn find_next_tip(&self) -> &str {
        self.get("find_next_tip")
    }

    pub fn find_close_tip(&self) -> &str {
        self.get("find_close_tip")
    }

    pub fn find_none(&self) -> &str {
        self.get("find_none")
    }

    pub fn find_searching(&self) -> &str {
        self.get("find_searching")
    }

    pub fn find_invalid(&self) -> &str {
        self.get("find_invalid")
    }

    pub fn find_position(&self, current: &str, total: &str) -> String {
        self.fill("find_position", &[("current", &current), ("total", &total)])
    }

    pub fn find_total(&self, total: &str) -> String {
        self.fill("find_total", &[("total", &total)])
    }

    pub fn menu_replace(&self) -> &str {
        self.get("menu_replace")
    }

    pub fn replace_hint(&self) -> &str {
        self.get("replace_hint")
    }

    pub fn replace_one(&self) -> &str {
        self.get("replace_one")
    }

    pub fn replace_all(&self) -> &str {
        self.get("replace_all")
    }

    pub fn replace_one_tip(&self) -> &str {
        self.get("replace_one_tip")
    }

    pub fn replace_all_tip(&self) -> &str {
        self.get("replace_all_tip")
    }

    pub fn replace_read_only(&self) -> &str {
        self.get("replace_read_only")
    }

    pub fn replaced_count(&self, count: &str) -> String {
        self.fill("replaced_count", &[("count", &count)])
    }

    pub fn menu_recent(&self) -> &str {
        self.get("menu_recent")
    }

    pub fn menu_recent_clear(&self) -> &str {
        self.get("menu_recent_clear")
    }

    pub fn recent_empty(&self) -> &str {
        self.get("recent_empty")
    }

    pub fn dialog_open_folder_title(&self) -> &str {
        self.get("dialog_open_folder_title")
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

    pub fn menu_view(&self) -> &str {
        self.get("menu_view")
    }

    pub fn menu_appearance(&self) -> &str {
        self.get("menu_appearance")
    }

    pub fn appearance(&self, appearance: Appearance) -> &str {
        self.get(match appearance {
            Appearance::System => "appearance_system",
            Appearance::Light => "appearance_light",
            Appearance::Dark => "appearance_dark",
        })
    }

    pub fn menu_editor_theme(&self) -> &str {
        self.get("menu_editor_theme")
    }

    pub fn editor_theme_auto(&self) -> &str {
        self.get("editor_theme_auto")
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

    pub fn menu_sidebar(&self) -> &str {
        self.get("menu_sidebar")
    }

    pub fn menu_focus_tree(&self) -> &str {
        self.get("menu_focus_tree")
    }

    pub fn tree_new_file(&self) -> &str {
        self.get("tree_new_file")
    }

    pub fn tree_new_folder(&self) -> &str {
        self.get("tree_new_folder")
    }

    pub fn tree_rename(&self) -> &str {
        self.get("tree_rename")
    }

    pub fn tree_delete(&self) -> &str {
        self.get("tree_delete")
    }

    pub fn tree_copy_path(&self) -> &str {
        self.get("tree_copy_path")
    }

    pub fn tree_copy_relative_path(&self) -> &str {
        self.get("tree_copy_relative_path")
    }

    pub fn tree_reveal(&self) -> &str {
        self.get("tree_reveal")
    }

    pub fn name_empty(&self) -> &str {
        self.get("name_empty")
    }

    pub fn name_bad_ending(&self) -> &str {
        self.get("name_bad_ending")
    }

    pub fn trash_title(&self) -> &str {
        self.get("trash_title")
    }

    pub fn trash_confirm(&self) -> &str {
        self.get("trash_confirm")
    }

    pub fn name_forbidden(&self, chars: &str) -> String {
        self.fill("name_forbidden", &[("chars", &chars)])
    }

    pub fn name_reserved(&self, name: &str) -> String {
        self.fill("name_reserved", &[("name", &name)])
    }

    pub fn name_exists(&self, name: &str) -> String {
        self.fill("name_exists", &[("name", &name)])
    }

    pub fn file_op_failed(&self, name: &str, error: &str) -> String {
        self.fill("file_op_failed", &[("name", &name), ("error", &error)])
    }

    pub fn trashed(&self, name: &str) -> String {
        self.fill("trashed", &[("name", &name)])
    }

    pub fn trash_file_message(&self, name: &str) -> String {
        self.fill("trash_file_message", &[("name", &name)])
    }

    pub fn trash_folder_message(&self, name: &str) -> String {
        self.fill("trash_folder_message", &[("name", &name)])
    }

    pub fn tree_empty(&self) -> &str {
        self.get("tree_empty")
    }

    pub fn project_bad_exclude(&self, pattern: &str, error: &str) -> String {
        self.fill(
            "project_bad_exclude",
            &[("pattern", &pattern), ("error", &error)],
        )
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

    pub fn cli_help(&self, version: &str) -> String {
        self.fill("cli_help", &[("version", &version)])
    }

    pub fn cli_unknown_option(&self, option: &str) -> String {
        self.fill("cli_unknown_option", &[("option", &option)])
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

    /// Translated text for a language package problem. The package is named by its folder.
    pub fn package_warning(&self, warning: &PackageWarning) -> String {
        match warning {
            PackageWarning::Unreadable { path, error } => self.fill(
                "package_unreadable",
                &[("package", &package_of(path)), ("error", error)],
            ),
            PackageWarning::Syntax { path, error } => self.fill(
                "package_syntax",
                &[("package", &package_of(path)), ("error", error)],
            ),
            PackageWarning::MissingKey { path, key } => self.fill(
                "package_missing_key",
                &[("package", &package_of(path)), ("key", key)],
            ),
            PackageWarning::InvalidValue { path, key } => self.fill(
                "package_invalid_value",
                &[("package", &package_of(path)), ("key", key)],
            ),
            PackageWarning::UnsupportedVersion { path, found } => self.fill(
                "package_version",
                &[("package", &package_of(path)), ("found", found)],
            ),
            PackageWarning::GrammarWithoutScope { path } => {
                self.fill("package_grammar_scope", &[("package", &package_of(path))])
            }
            PackageWarning::Grammar { path, error } => self.fill(
                "package_grammar",
                &[("package", &package_of(path)), ("error", error)],
            ),
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
    use std::path::PathBuf;

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
        let pw = |kind: usize| {
            let path = PathBuf::from(r"x\language.toml");
            let (error, key) = ("e".to_owned(), "k".to_owned());
            match kind {
                0 => PackageWarning::Unreadable { path, error },
                1 => PackageWarning::Syntax { path, error },
                2 => PackageWarning::MissingKey { path, key },
                3 => PackageWarning::InvalidValue { path, key },
                4 => PackageWarning::UnsupportedVersion {
                    path,
                    found: "2".into(),
                },
                5 => PackageWarning::GrammarWithoutScope { path },
                _ => PackageWarning::Grammar { path, error },
            }
        };
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
                ("selections", t.selections(3)),
                ("selected_chars", t.selected_chars(12)),
                ("plain_text", t.plain_text().to_owned()),
                (
                    "document_language_tooltip",
                    t.document_language_tooltip().to_owned(),
                ),
                ("read_only_short", t.read_only_short().to_owned()),
                ("menu_file", t.menu_file().to_owned()),
                ("menu_new", t.menu_new().to_owned()),
                ("menu_open", t.menu_open().to_owned()),
                ("menu_open_folder", t.menu_open_folder().to_owned()),
                ("menu_close_folder", t.menu_close_folder().to_owned()),
                (
                    "dialog_open_folder_title",
                    t.dialog_open_folder_title().to_owned(),
                ),
                ("menu_recent", t.menu_recent().to_owned()),
                ("menu_replace", t.menu_replace().to_owned()),
                ("replace_hint", t.replace_hint().to_owned()),
                ("replace_one", t.replace_one().to_owned()),
                ("replace_all", t.replace_all().to_owned()),
                ("replace_one_tip", t.replace_one_tip().to_owned()),
                ("replace_all_tip", t.replace_all_tip().to_owned()),
                ("replace_read_only", t.replace_read_only().to_owned()),
                ("replaced_count", t.replaced_count("1")),
                ("menu_edit", t.menu_edit().to_owned()),
                ("menu_find", t.menu_find().to_owned()),
                ("menu_find_next", t.menu_find_next().to_owned()),
                ("menu_find_previous", t.menu_find_previous().to_owned()),
                ("find_hint", t.find_hint().to_owned()),
                ("find_match_case", t.find_match_case().to_owned()),
                ("find_whole_word", t.find_whole_word().to_owned()),
                ("find_regex", t.find_regex().to_owned()),
                ("find_previous_tip", t.find_previous_tip().to_owned()),
                ("find_next_tip", t.find_next_tip().to_owned()),
                ("find_close_tip", t.find_close_tip().to_owned()),
                ("find_none", t.find_none().to_owned()),
                ("find_searching", t.find_searching().to_owned()),
                ("find_invalid", t.find_invalid().to_owned()),
                ("find_position", t.find_position("1", "1")),
                ("find_total", t.find_total("1")),
                ("menu_go", t.menu_go().to_owned()),
                ("menu_go_to_line", t.menu_go_to_line().to_owned()),
                ("menu_command_palette", t.menu_command_palette().to_owned()),
                ("menu_next_tab", t.menu_next_tab().to_owned()),
                ("menu_previous_tab", t.menu_previous_tab().to_owned()),
                (
                    "go_to_line_no_document",
                    t.go_to_line_no_document().to_owned(),
                ),
                ("commands_no_results", t.commands_no_results().to_owned()),
                ("go_to_line_hint", t.go_to_line_hint("1")),
                ("menu_quick_open", t.menu_quick_open().to_owned()),
                ("quick_open_hint", t.quick_open_hint().to_owned()),
                ("quick_open_indexing", t.quick_open_indexing().to_owned()),
                (
                    "quick_open_no_results",
                    t.quick_open_no_results().to_owned(),
                ),
                ("quick_open_go_to_line", t.quick_open_go_to_line("1")),
                ("quick_open_count", t.quick_open_count("1")),
                ("menu_recent_clear", t.menu_recent_clear().to_owned()),
                ("recent_empty", t.recent_empty().to_owned()),
                ("menu_save", t.menu_save().to_owned()),
                ("menu_save_as", t.menu_save_as().to_owned()),
                ("menu_close_tab", t.menu_close_tab().to_owned()),
                ("menu_exit", t.menu_exit().to_owned()),
                ("menu_view", t.menu_view().to_owned()),
                ("menu_appearance", t.menu_appearance().to_owned()),
                (
                    "appearance_system",
                    t.appearance(Appearance::System).to_owned(),
                ),
                (
                    "appearance_light",
                    t.appearance(Appearance::Light).to_owned(),
                ),
                ("appearance_dark", t.appearance(Appearance::Dark).to_owned()),
                ("menu_editor_theme", t.menu_editor_theme().to_owned()),
                ("editor_theme_auto", t.editor_theme_auto().to_owned()),
                ("dialog_open_title", t.dialog_open_title().to_owned()),
                ("dialog_save_as_title", t.dialog_save_as_title().to_owned()),
                ("saved", t.saved().to_owned()),
                ("save_read_only", t.save_read_only().to_owned()),
                ("menu_sidebar", t.menu_sidebar().to_owned()),
                ("menu_focus_tree", t.menu_focus_tree().to_owned()),
                ("tree_empty", t.tree_empty().to_owned()),
                ("tree_new_file", t.tree_new_file().to_owned()),
                ("tree_new_folder", t.tree_new_folder().to_owned()),
                ("tree_rename", t.tree_rename().to_owned()),
                ("tree_delete", t.tree_delete().to_owned()),
                ("tree_copy_path", t.tree_copy_path().to_owned()),
                (
                    "tree_copy_relative_path",
                    t.tree_copy_relative_path().to_owned(),
                ),
                ("tree_reveal", t.tree_reveal().to_owned()),
                ("name_empty", t.name_empty().to_owned()),
                ("name_bad_ending", t.name_bad_ending().to_owned()),
                ("trash_title", t.trash_title().to_owned()),
                ("trash_confirm", t.trash_confirm().to_owned()),
                ("name_forbidden", t.name_forbidden("x")),
                ("name_reserved", t.name_reserved("x")),
                ("name_exists", t.name_exists("x")),
                ("file_op_failed", t.file_op_failed("x", "x")),
                ("trashed", t.trashed("x")),
                ("trash_file_message", t.trash_file_message("x")),
                ("trash_folder_message", t.trash_folder_message("x")),
                ("project_bad_exclude", t.project_bad_exclude("p", "e")),
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
                ("cli_help", t.cli_help("1.0.0")),
                ("package_unreadable", t.package_warning(&pw(0))),
                ("package_syntax", t.package_warning(&pw(1))),
                ("package_missing_key", t.package_warning(&pw(2))),
                ("package_invalid_value", t.package_warning(&pw(3))),
                ("package_version", t.package_warning(&pw(4))),
                ("package_grammar_scope", t.package_warning(&pw(5))),
                ("package_grammar", t.package_warning(&pw(6))),
                ("cli_unknown_option", t.cli_unknown_option("-x")),
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
    fn package_warnings_name_the_package_folder() {
        let warning = PackageWarning::MissingKey {
            path: PathBuf::from(r"C:\Bracket\languages\python\language.toml"),
            key: "name".into(),
        };
        assert_eq!(
            texts(Language::English).package_warning(&warning),
            "Language package “python”: “name” is missing in language.toml"
        );
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
