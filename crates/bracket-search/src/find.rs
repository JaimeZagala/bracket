//! Finding text in one document: plain text or a regular expression, matching case or not,
//! whole words or not. Matches come back as char ranges, the unit of the editor's selections.

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
        Ok(Self { regex })
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
