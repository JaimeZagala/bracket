//! Finding and replacing text in one document: plain text or a regular expression, matching
//! case or not, whole words or not. Matches come back as char ranges, the unit of the editor's
//! selections.
//!
//! With regular expressions the replacement can use the groups of the match (`$1`, `${name}`,
//! `$$` for a dollar sign) and `\n`, `\t` and `\\`; `\n` becomes the document's line break.

use std::ops::Range;

use bracket_core::CancelToken;
use regex::{Regex, RegexBuilder};

/// How the query is matched.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct FindOptions {
    pub match_case: bool,
    pub whole_word: bool,
    pub regex: bool,
}

/// Why a query cannot be searched for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FindError {
    Empty,
    /// Not a valid regular expression; the message says why.
    Invalid(String),
}

/// The matches found, in order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Matches {
    /// Char ranges, never empty.
    pub ranges: Vec<Range<usize>>,
    /// The limit was reached: there are more.
    pub truncated: bool,
}

/// A compiled query.
#[derive(Clone, Debug)]
pub struct Finder {
    regex: Regex,
    is_regex: bool,
}

/// What matches are replaced with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Replacement {
    template: String,
    /// The template expands groups and escapes (only for regular expressions).
    expand: bool,
}

impl Replacement {
    /// `line_break`: the document's line break, for `\n` in the template.
    pub fn new(template: &str, finder: &Finder, line_break: &str) -> Self {
        if !finder.is_regex {
            return Self {
                template: template.to_owned(),
                expand: false,
            };
        }
        let mut unescaped = String::with_capacity(template.len());
        let mut chars = template.chars();
        while let Some(c) = chars.next() {
            if c != '\\' {
                unescaped.push(c);
                continue;
            }
            match chars.next() {
                Some('n') => unescaped.push_str(line_break),
                Some('t') => unescaped.push('\t'),
                Some('\\') => unescaped.push('\\'),
                Some(other) => {
                    unescaped.push('\\');
                    unescaped.push(other);
                }
                None => unescaped.push('\\'),
            }
        }
        Self {
            template: unescaped,
            expand: true,
        }
    }

    fn apply(&self, captures: &regex::Captures<'_>) -> String {
        if !self.expand {
            return self.template.clone();
        }
        let mut out = String::new();
        captures.expand(&self.template, &mut out);
        out
    }
}

impl Finder {
    pub fn new(query: &str, options: FindOptions) -> Result<Self, FindError> {
        if query.is_empty() {
            return Err(FindError::Empty);
        }
        let mut pattern = if options.regex {
            query.to_owned()
        } else {
            regex::escape(query)
        };
        if options.whole_word {
            pattern = format!(r"\b(?:{pattern})\b");
        }
        let regex = RegexBuilder::new(&pattern)
            .case_insensitive(!options.match_case)
            // `^` and `$` at every line, also before "\r\n".
            .multi_line(true)
            .crlf(true)
            .build()
            .map_err(|e| FindError::Invalid(first_line(&e.to_string())))?;
        Ok(Self {
            regex,
            is_regex: options.regex,
        })
    }

    /// Every match in `text` with what replaces it, in order, as char ranges. `None` if
    /// cancelled.
    pub fn replace_all(
        &self,
        text: &str,
        replacement: &Replacement,
        cancel: &CancelToken,
    ) -> Option<Vec<(Range<usize>, String)>> {
        let mut found = Vec::new();
        let (mut byte, mut char) = (0, 0);
        for (count, captures) in self.regex.captures_iter(text).enumerate() {
            if count % 1024 == 0 && cancel.is_cancelled() {
                return None;
            }
            let whole = captures.get(0).expect("group 0 is the whole match");
            if whole.is_empty() {
                continue;
            }
            char += text[byte..whole.start()].chars().count();
            let start = char;
            char += whole.as_str().chars().count();
            byte = whole.end();
            found.push((start..char, replacement.apply(&captures)));
        }
        Some(found)
    }

    /// What replaces the match at `range` (chars of `context`, a piece of the document that
    /// starts at a line start and ends at a line end, so anchors still work). `None` if the
    /// query does not match exactly there (the text changed).
    pub fn replace_at(
        &self,
        context: &str,
        range: Range<usize>,
        replacement: &Replacement,
    ) -> Option<String> {
        let start = context.char_indices().nth(range.start).map(|(b, _)| b)?;
        let captures = self.regex.captures_at(context, start)?;
        let whole = captures.get(0)?;
        (whole.start() == start && whole.as_str().chars().count() == range.len())
            .then(|| replacement.apply(&captures))
    }

    /// Up to `limit` matches in `text`. `None` if cancelled. Empty matches (a regex like `x*`)
    /// are skipped: there is nothing to show or select.
    pub fn find_all(&self, text: &str, limit: usize, cancel: &CancelToken) -> Option<Matches> {
        let mut matches = Matches::default();
        // Byte offsets become char offsets by counting the chars between matches, once.
        let (mut byte, mut char) = (0, 0);
        for (count, found) in self.regex.find_iter(text).enumerate() {
            if count % 1024 == 0 && cancel.is_cancelled() {
                return None;
            }
            if found.is_empty() {
                continue;
            }
            if matches.ranges.len() == limit {
                matches.truncated = true;
                break;
            }
            char += text[byte..found.start()].chars().count();
            let start = char;
            char += found.as_str().chars().count();
            byte = found.end();
            matches.ranges.push(start..char);
        }
        Some(matches)
    }
}

/// regex's messages span several lines (the pattern, a caret, the reason); the status shows one.
fn first_line(message: &str) -> String {
    message
        .lines()
        .rev()
        .find(|l| l.trim_start().starts_with("error:"))
        .or_else(|| message.lines().last())
        .unwrap_or(message)
        .trim()
        .trim_start_matches("error:")
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The matches as (start, end) pairs.
    fn find(text: &str, query: &str, options: FindOptions) -> Vec<(usize, usize)> {
        Finder::new(query, options)
            .unwrap()
            .find_all(text, usize::MAX, &CancelToken::new())
            .unwrap()
            .ranges
            .into_iter()
            .map(|r| (r.start, r.end))
            .collect()
    }

    const PLAIN: FindOptions = FindOptions {
        match_case: false,
        whole_word: false,
        regex: false,
    };

    #[test]
    fn plain_text_ignores_case_unless_asked() {
        let text = "Foo foo FOO";
        assert_eq!(find(text, "foo", PLAIN), [(0, 3), (4, 7), (8, 11)]);
        let case = FindOptions {
            match_case: true,
            ..PLAIN
        };
        assert_eq!(find(text, "foo", case), [(4, 7)]);
    }

    #[test]
    fn plain_text_is_not_a_pattern() {
        assert_eq!(find("a.b axb", "a.b", PLAIN), [(0, 3)]);
        assert_eq!(find("(x)", "(x)", PLAIN), [(0, 3)]);
    }

    #[test]
    fn whole_words_only() {
        let words = FindOptions {
            whole_word: true,
            ..PLAIN
        };
        assert_eq!(
            find("cat concat cat_s cat.", "cat", words),
            [(0, 3), (17, 20)]
        );
    }

    #[test]
    fn regular_expressions_work_per_line() {
        let regex = FindOptions {
            regex: true,
            ..PLAIN
        };
        assert_eq!(
            find("let a = 10;\r\nlet b = 200;", r"\d+", regex),
            [(8, 10), (21, 24)]
        );
        assert_eq!(
            find("one\r\ntwo\nthree", r"^t\w+$", regex),
            [(5, 8), (9, 14)]
        );
        assert_eq!(
            find("aaa", "x*", regex),
            Vec::<(usize, usize)>::new(),
            "empty matches are skipped"
        );
    }

    #[test]
    fn positions_are_in_chars() {
        // "ñ" and "€" take several bytes; the editor counts chars.
        assert_eq!(find("ñandú €uro ñandú", "ñandú", PLAIN), [(0, 5), (11, 16)]);
        assert_eq!(find("€€x", "x", PLAIN), [(2, 3)]);
    }

    #[test]
    fn bad_queries_are_reported() {
        assert_eq!(Finder::new("", PLAIN).unwrap_err(), FindError::Empty);
        let regex = FindOptions {
            regex: true,
            ..PLAIN
        };
        let Err(FindError::Invalid(message)) = Finder::new("(unclosed", regex) else {
            panic!("an unclosed group is invalid");
        };
        assert!(!message.is_empty() && !message.contains('\n'), "{message}");
    }

    fn replace(text: &str, query: &str, options: FindOptions, with: &str) -> String {
        let finder = Finder::new(query, options).unwrap();
        let replacement = Replacement::new(with, &finder, "\r\n");
        let found = finder
            .replace_all(text, &replacement, &CancelToken::new())
            .unwrap();
        let chars: Vec<char> = text.chars().collect();
        let mut out = String::new();
        let mut at = 0;
        for (range, new) in found {
            out.extend(&chars[at..range.start]);
            out.push_str(&new);
            at = range.end;
        }
        out.extend(&chars[at..]);
        out
    }

    #[test]
    fn plain_replacements_are_taken_literally() {
        assert_eq!(replace("a.b a.b", "a.b", PLAIN, "$1 c"), "$1 c $1 c");
    }

    #[test]
    fn regex_replacements_use_groups_and_escapes() {
        let regex = FindOptions {
            regex: true,
            ..PLAIN
        };
        assert_eq!(
            replace("Smith, John; Doe, Jane", r"(\w+), (\w+)", regex, "$2 $1"),
            "John Smith; Jane Doe"
        );
        assert_eq!(replace("id=7", r"id=(?<n>\d+)", regex, "${n}$$"), "7$");
        // \n becomes the document's line break (CRLF here); \\ is a backslash.
        assert_eq!(replace("a;b", ";", regex, r"\n"), "a\r\nb");
        assert_eq!(replace("a;b", ";", regex, r"\t\\"), "a\t\\b");
    }

    #[test]
    fn one_match_is_replaced_in_its_context() {
        let regex = FindOptions {
            regex: true,
            ..PLAIN
        };
        let finder = Finder::new(r"^(\w+) = (\d+)", regex).unwrap();
        let replacement = Replacement::new("$2 = $1", &finder, "\n");
        let line = "count = 42";
        assert_eq!(
            finder.replace_at(line, 0..10, &replacement).as_deref(),
            Some("42 = count")
        );
        assert_eq!(
            finder.replace_at(line, 1..10, &replacement),
            None,
            "not a match there"
        );
        let words = Finder::new("ñu", PLAIN).unwrap();
        let replacement = Replacement::new("gnu", &words, "\n");
        assert_eq!(
            words.replace_at("el ñu", 3..5, &replacement).as_deref(),
            Some("gnu")
        );
    }

    #[test]
    fn the_limit_and_cancelling_stop_the_search() {
        let text = "x ".repeat(100);
        let finder = Finder::new("x", PLAIN).unwrap();
        let found = finder.find_all(&text, 10, &CancelToken::new()).unwrap();
        assert_eq!(found.ranges.len(), 10);
        assert!(found.truncated);
        let cancel = CancelToken::new();
        cancel.cancel();
        assert_eq!(finder.find_all(&text, 10, &cancel), None);
    }
}
