//! Go to definition and the navigation history: the name at the caret, and the places to go
//! back and forward to (Alt+← and Alt+→).

use std::path::PathBuf;

use bracket_core::Rope;

/// Places kept in each direction.
const MAX_PLACES: usize = 50;

/// The name at `pos` for go to definition: a word (letters, digits and `_`; in CSS also `-`),
/// and whether it names a Razor component (`<Name` or `</Name` in a Razor file, starting with
/// a capital). `None` if there is no name there, or only a number.
pub fn name_at(text: &Rope, pos: usize, language: Option<&str>) -> Option<(String, bool)> {
    let css = matches!(language, Some("css" | "scss" | "less"));
    let is_name = |c: char| c.is_alphanumeric() || c == '_' || (css && c == '-');
    let len = text.len_chars();
    let probe = if pos < len && is_name(text.char(pos)) {
        pos
    } else if pos > 0 && pos <= len && is_name(text.char(pos - 1)) {
        pos - 1
    } else {
        return None;
    };
    let mut start = probe;
    while start > 0 && is_name(text.char(start - 1)) {
        start -= 1;
    }
    let mut end = probe + 1;
    while end < len && is_name(text.char(end)) {
        end += 1;
    }
    let name = text.slice(start..end).to_string();
    let first = name.chars().next()?;
    if first.is_ascii_digit() || name.chars().all(|c| c == '-') {
        return None;
    }
    let before = |n: usize| (start >= n).then(|| text.char(start - n));
    let component = language == Some("razor")
        && first.is_uppercase()
        && (before(1) == Some('<') || (before(1) == Some('/') && before(2) == Some('<')));
    Some((name, component))
}

/// Whether references to a name in `language` must match its case: they must, except in T-SQL
/// and in text without a language.
pub fn references_match_case(language: Option<&str>) -> bool {
    language.is_some_and(|l| l != "tsql")
}

/// Whether go to definition, started in a document in `from_language`, looks in a file in
/// `language` (`same_file`: the document itself). C# and Razor look in each other; JSON and XML,
/// whose keys and elements are not declarations elsewhere, only in their own file; the rest, in
/// the files of their language.
pub fn looks_in(from_language: &str, language: &str, same_file: bool) -> bool {
    match from_language {
        "json" | "xml" => same_file,
        "csharp" | "razor" => matches!(language, "csharp" | "razor"),
        _ => language == from_language,
    }
}

/// A place in a document: its tab, its file (to open it again if the tab was closed) and
/// the caret's line and column, 1-based.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub tab: u64,
    pub path: Option<PathBuf>,
    pub line: usize,
    pub column: usize,
}

/// Where the caret was before each jump (go to definition, a symbol, a line, a search result),
/// to go back there and forward again.
#[derive(Debug, Default)]
pub struct History {
    back: Vec<Place>,
    forward: Vec<Place>,
}

impl History {
    /// Records the place a jump leaves. Going forward is no longer possible.
    pub fn leave(&mut self, place: Place) {
        if self.back.last() != Some(&place) {
            self.back.push(place);
            if self.back.len() > MAX_PLACES {
                self.back.remove(0);
            }
        }
        self.forward.clear();
    }

    /// The place to go back to; `current` is kept for going forward.
    pub fn back(&mut self, current: Place) -> Option<Place> {
        let place = self.back.pop()?;
        self.forward.push(current);
        Some(place)
    }

    /// The place to go forward to; `current` is kept for going back.
    pub fn forward(&mut self, current: Place) -> Option<Place> {
        let place = self.forward.pop()?;
        self.back.push(current);
        Some(place)
    }

    pub fn can_go_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn can_go_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    /// Forgets everything (another project).
    pub fn clear(&mut self) {
        self.back.clear();
        self.forward.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(text: &str, pos: usize, language: Option<&str>) -> Option<(String, bool)> {
        name_at(&Rope::from_str(text), pos, language)
    }

    #[test]
    fn the_name_at_the_caret() {
        let line = "var total = invoice.ComputeTotal(2);";
        assert_eq!(name(line, 22, None), Some(("ComputeTotal".into(), false)));
        assert_eq!(
            name(line, 9, None),
            Some(("total".into(), false)),
            "at its end"
        );
        assert_eq!(name(line, 10, None), None, "between spaces");
        assert_eq!(name(line, 33, None), None, "a number");
        assert_eq!(name("a-b", 0, None), Some(("a".into(), false)));
        assert_eq!(
            name("color: var(--main-color);", 13, Some("css")),
            Some(("--main-color".into(), false))
        );
        assert_eq!(name("", 0, None), None);
    }

    #[test]
    fn razor_components_are_told_apart() {
        let razor = Some("razor");
        let text = "<Counter Start=\"1\" /></Counter><div>";
        assert_eq!(name(text, 3, razor), Some(("Counter".into(), true)));
        assert_eq!(name(text, 25, razor), Some(("Counter".into(), true)));
        assert_eq!(name(text, 10, razor), Some(("Start".into(), false)));
        assert_eq!(
            name(text, 33, razor),
            Some(("div".into(), false)),
            "lower case"
        );
        assert_eq!(name(text, 3, Some("html")), Some(("Counter".into(), false)));
    }

    #[test]
    fn references_match_case_except_in_sql_and_plain_text() {
        assert!(references_match_case(Some("csharp")));
        assert!(references_match_case(Some("json")));
        assert!(!references_match_case(Some("tsql")));
        assert!(!references_match_case(None));
    }

    #[test]
    fn definitions_stay_among_related_languages() {
        assert!(looks_in("csharp", "razor", false));
        assert!(looks_in("razor", "csharp", false));
        assert!(
            !looks_in("csharp", "json", false),
            "a profile in launchSettings.json"
        );
        assert!(looks_in("json", "json", true));
        assert!(!looks_in("json", "json", false), "keys of other files");
        assert!(!looks_in("xml", "xml", false));
        assert!(looks_in("css", "css", false));
        assert!(!looks_in("tsql", "csharp", false));
        assert!(looks_in("python", "python", false), "a package's language");
    }

    fn place(line: usize) -> Place {
        Place {
            tab: 1,
            path: None,
            line,
            column: 1,
        }
    }

    #[test]
    fn back_and_forward_retrace_the_jumps() {
        let mut history = History::default();
        assert!(!history.can_go_back());
        history.leave(place(10));
        history.leave(place(20));
        // Now at 30.
        assert_eq!(history.back(place(30)), Some(place(20)));
        assert_eq!(history.back(place(20)), Some(place(10)));
        assert_eq!(history.back(place(10)), None);
        assert_eq!(history.forward(place(10)), Some(place(20)));
        assert!(history.can_go_forward());
        // A new jump drops the way forward.
        history.leave(place(20));
        assert!(!history.can_go_forward());
        for line in 0..2 * MAX_PLACES {
            history.leave(place(line));
        }
        assert_eq!(history.back.len(), MAX_PLACES);
    }
}
