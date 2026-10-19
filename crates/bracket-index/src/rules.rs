//! Symbols found with regex rules: T-SQL and the `symbols.toml` of external language packages.
//!
//! ```toml
//! format_version = 1
//!
//! [[symbol]]
//! kind = "procedure"          # one of the symbol kinds: class, method, function, table…
//! pattern = '(?i)^\s*CREATE\s+PROCEDURE\s+(?<name>\S+)'
//! end = '(?i)^\s*GO\s*$'       # optional: the symbol ends where this matches
//! strip = "[]"                 # optional: characters taken out of the name
//! ```
//!
//! Patterns use the `regex` crate's syntax, with `^` and `$` at every line (CRLF included). The
//! name is the `name` group, or the first group, or the whole match. A symbol runs from its match
//! to its `end` match or to the next symbol, whichever comes first. Rule symbols do not nest.

use std::ops::Range;

use regex::{Regex, RegexBuilder};
use toml_edit::{DocumentMut, Item};

use crate::symbols::SymbolKind;

pub const FORMAT_VERSION: i64 = 1;
/// Patterns larger than this (compiled) are rejected: a rule file cannot make extraction blow up.
const SIZE_LIMIT: usize = 1 << 20;

#[derive(Debug)]
struct Rule {
    kind: SymbolKind,
    pattern: Regex,
    end: Option<Regex>,
    strip: Vec<char>,
}

/// The compiled rules of one language.
#[derive(Debug)]
pub struct RuleSet {
    rules: Vec<Rule>,
}

/// A symbol found by a rule, in bytes.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct RuleMatch {
    pub kind: SymbolKind,
    pub range: Range<usize>,
    pub name_range: Range<usize>,
    pub name: String,
}

impl RuleSet {
    /// Reads a `symbols.toml`. The error says what is wrong, for the package warning.
    pub fn parse(text: &str) -> Result<Self, String> {
        let doc: DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| {
            e.to_string().lines().next().unwrap_or_default().to_owned()
        })?;
        match doc.get("format_version").and_then(Item::as_integer) {
            Some(FORMAT_VERSION) => {}
            Some(other) => return Err(format!("unsupported format_version {other}")),
            None => return Err("missing format_version".to_owned()),
        }
        let tables = doc
            .get("symbol")
            .and_then(Item::as_array_of_tables)
            .ok_or("no [[symbol]] rules")?;
        let mut rules = Vec::new();
        for (index, table) in tables.iter().enumerate() {
            let at = |what: &str| format!("rule {}: {what}", index + 1);
            let text_of = |key: &str| table.get(key).and_then(Item::as_str);
            let kind_name = text_of("kind").ok_or_else(|| at("missing kind"))?;
            let kind = SymbolKind::from_capture(&format!("definition.{kind_name}"))
                .ok_or_else(|| at(&format!("unknown kind \"{kind_name}\"")))?;
            let compile = |key: &str, source: &str| {
                RegexBuilder::new(source)
                    .multi_line(true)
                    .crlf(true)
                    .size_limit(SIZE_LIMIT)
                    .build()
                    .map_err(|e| {
                        at(&format!(
                            "{key}: {}",
                            e.to_string().lines().last().unwrap_or_default().trim()
                        ))
                    })
            };
            let pattern = compile(
                "pattern",
                text_of("pattern").ok_or_else(|| at("missing pattern"))?,
            )?;
            let end = text_of("end").map(|e| compile("end", e)).transpose()?;
            let strip = text_of("strip").unwrap_or_default().chars().collect();
            rules.push(Rule {
                kind,
                pattern,
                end,
                strip,
            });
        }
        Ok(Self { rules })
    }

    /// The symbols the rules find in `text`, in document order, at most `limit`.
    pub(crate) fn find(&self, text: &str, limit: usize) -> Vec<RuleMatch> {
        // Every match first, with the rule that found it: ranges depend on the next symbol.
        let mut found: Vec<(usize, Range<usize>, Range<usize>)> = Vec::new();
        for (index, rule) in self.rules.iter().enumerate() {
            for captures in rule.pattern.captures_iter(text) {
                let whole = captures.get(0).expect("group 0 is the whole match");
                if whole.is_empty() {
                    continue;
                }
                let name = captures
                    .name("name")
                    .or_else(|| captures.get(1))
                    .unwrap_or(whole);
                found.push((index, whole.range(), name.range()));
                if found.len() >= limit {
                    break;
                }
            }
        }
        found.sort_by_key(|(_, range, _)| range.start);
        found.dedup_by_key(|(_, range, _)| range.start);
        found.truncate(limit);
        let starts: Vec<usize> = found.iter().map(|(_, r, _)| r.start).collect();
        found
            .into_iter()
            .enumerate()
            .map(|(i, (index, range, name_range))| {
                let rule = &self.rules[index];
                let next = starts.get(i + 1).copied().unwrap_or(text.len());
                let end = rule
                    .end
                    .as_ref()
                    .and_then(|end| end.find_at(text, range.end))
                    .map_or(next, |m| m.start().min(next));
                let name: String = text[name_range.clone()]
                    .chars()
                    .filter(|c| !rule.strip.contains(c))
                    .collect();
                RuleMatch {
                    kind: rule.kind,
                    range: range.start..end.max(range.end),
                    name_range,
                    name: name.trim().to_owned(),
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(rules: &RuleSet, text: &str) -> Vec<(String, SymbolKind)> {
        rules
            .find(text, usize::MAX)
            .into_iter()
            .map(|m| (m.name, m.kind))
            .collect()
    }

    #[test]
    fn rules_find_named_symbols_in_order() {
        let rules = RuleSet::parse(
            r#"
format_version = 1

[[symbol]]
kind = "function"
pattern = '^def (?<name>\w+)'

[[symbol]]
kind = "class"
pattern = '^class (\w+)'
"#,
        )
        .unwrap();
        let text = "class A:\n    pass\ndef run():\n    pass\nclass B:\n";
        assert_eq!(
            names(&rules, text),
            [
                ("A".to_owned(), SymbolKind::Class),
                ("run".to_owned(), SymbolKind::Function),
                ("B".to_owned(), SymbolKind::Class),
            ]
        );
        let found = rules.find(text, usize::MAX);
        assert_eq!(
            found[0].range,
            0..text.find("def").unwrap(),
            "up to the next symbol"
        );
        assert_eq!(found[2].range.end, text.len());
    }

    #[test]
    fn an_end_pattern_and_strip_shape_the_symbol() {
        let rules = RuleSet::parse(
            r#"
format_version = 1
[[symbol]]
kind = "procedure"
pattern = '(?i)^create procedure (?<name>\S+)'
end = '(?i)^go$'
strip = "[]"
"#,
        )
        .unwrap();
        let text = "CREATE PROCEDURE [dbo].[Run]\nAS SELECT 1\nGO\nPRINT 'after'\n";
        let found = rules.find(text, usize::MAX);
        assert_eq!(found[0].name, "dbo.Run");
        assert_eq!(found[0].range, 0..text.find("GO").unwrap());
    }

    #[test]
    fn bad_rule_files_say_why() {
        let error = |text: &str| RuleSet::parse(text).unwrap_err();
        assert!(error("format_version = 2").contains("format_version"));
        assert!(error("x = 1").contains("format_version"));
        assert!(error("format_version = 1").contains("[[symbol]]"));
        let bad_kind = "format_version = 1\n[[symbol]]\nkind = \"widget\"\npattern = 'x'\n";
        assert!(error(bad_kind).contains("widget"));
        let bad_regex = "format_version = 1\n[[symbol]]\nkind = \"class\"\npattern = '('\n";
        assert!(error(bad_regex).starts_with("rule 1: pattern"));
        assert!(error("not toml [").len() > 3);
    }
}
