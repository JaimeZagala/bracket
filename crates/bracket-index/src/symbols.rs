//! Symbols of a document: the declarations an outline, Ctrl+T and go to definition work with.
//!
//! C#, CSS, JSON and XML are parsed with tree-sitter and their symbols come from a query per
//! language (`decisions/0003-symbols.md`). A query marks each declaration with a capture named
//! `@definition.<kind>` (`@definition.class`, `@definition.key`…) and its name with `@name`,
//! like tree-sitter's own tag queries. tree-sitter works in bytes; positions are turned into
//! chars, the editor's unit, before they leave this module. Nesting comes from the ranges:
//! a symbol inside another one's range is its child.

use std::ops::Range;
use std::sync::OnceLock;

use tree_sitter::{Language, Parser, Query, QueryCursor, StreamingIterator};

/// Symbols kept for one document at most; a huge generated file stops here.
pub const MAX_SYMBOLS: usize = 50_000;

/// What a symbol declares.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    Namespace,
    Class,
    Struct,
    Interface,
    Record,
    Enum,
    EnumMember,
    Delegate,
    Method,
    Constructor,
    Property,
    Field,
    Event,
    Indexer,
    Selector,
    AtRule,
    Keyframes,
    Variable,
    Key,
    Element,
    Procedure,
    Function,
    View,
    Table,
    Trigger,
    Type,
    Directive,
}

impl SymbolKind {
    /// The kind a query capture names: `definition.class` → `Class`.
    pub fn from_capture(capture: &str) -> Option<Self> {
        let kind = capture.strip_prefix("definition.")?;
        Some(match kind {
            "namespace" => Self::Namespace,
            "class" => Self::Class,
            "struct" => Self::Struct,
            "interface" => Self::Interface,
            "record" => Self::Record,
            "enum" => Self::Enum,
            "enum_member" => Self::EnumMember,
            "delegate" => Self::Delegate,
            "method" => Self::Method,
            "constructor" => Self::Constructor,
            "property" => Self::Property,
            "field" => Self::Field,
            "event" => Self::Event,
            "indexer" => Self::Indexer,
            "selector" => Self::Selector,
            "at_rule" => Self::AtRule,
            "keyframes" => Self::Keyframes,
            "variable" => Self::Variable,
            "key" => Self::Key,
            "element" => Self::Element,
            "procedure" => Self::Procedure,
            "function" => Self::Function,
            "view" => Self::View,
            "table" => Self::Table,
            "trigger" => Self::Trigger,
            "type" => Self::Type,
            "directive" => Self::Directive,
            _ => return None,
        })
    }
}

/// A declaration in a document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    /// The whole declaration, in chars.
    pub range: Range<usize>,
    /// Its name, in chars: where go to definition puts the caret.
    pub name_range: Range<usize>,
    /// The symbol it is declared in (an index into the same list), if any.
    pub parent: Option<usize>,
    /// 0 for top-level symbols.
    pub depth: usize,
}

/// A language parsed with tree-sitter, and its symbol query.
struct Grammar {
    id: &'static str,
    language: fn() -> Language,
    query: &'static str,
    compiled: OnceLock<Option<Query>>,
}

impl Grammar {
    /// The compiled query, compiled once; `None` if it does not compile (a bug: tests check
    /// every built-in query).
    fn query(&self) -> Option<&Query> {
        self.compiled
            .get_or_init(|| Query::new(&(self.language)(), self.query).ok())
            .as_ref()
    }
}

/// The built-in grammars, by the language ids of `bracket-syntax`. Their queries are filled in
/// language by language.
static GRAMMARS: [Grammar; 4] = [
    Grammar {
        id: "csharp",
        language: || tree_sitter_c_sharp::LANGUAGE.into(),
        query: "",
        compiled: OnceLock::new(),
    },
    Grammar {
        id: "css",
        language: || tree_sitter_css::LANGUAGE.into(),
        query: "",
        compiled: OnceLock::new(),
    },
    Grammar {
        id: "json",
        language: || tree_sitter_json::LANGUAGE.into(),
        query: "",
        compiled: OnceLock::new(),
    },
    Grammar {
        id: "xml",
        language: || tree_sitter_xml::LANGUAGE_XML.into(),
        query: "",
        compiled: OnceLock::new(),
    },
];

fn grammar(language: &str) -> Option<&'static Grammar> {
    GRAMMARS.iter().find(|g| g.id == language)
}

/// Whether `language` (a `bracket-syntax` id) has a symbol extractor.
pub fn has_symbols(language: &str) -> bool {
    grammar(language).is_some()
}

/// The symbols of `text` in `language`, in document order; `None` if the language has no
/// extractor.
pub fn extract(language: &str, text: &str) -> Option<Vec<Symbol>> {
    let grammar = grammar(language)?;
    Some(run_query(&(grammar.language)(), grammar.query()?, text))
}

/// Parses `text` and runs `query` over it.
fn run_query(language: &Language, query: &Query, text: &str) -> Vec<Symbol> {
    let mut parser = Parser::new();
    if parser.set_language(language).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(text, None) else {
        return Vec::new();
    };
    let names = query.capture_names();
    let name_index = names.iter().position(|n| *n == "name");
    // Byte ranges first; chars once all positions are known.
    let mut found: Vec<(SymbolKind, Range<usize>, Range<usize>)> = Vec::new();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, tree.root_node(), text.as_bytes());
    while let Some(m) = matches.next() {
        let mut definition = None;
        let mut name = None;
        for capture in m.captures() {
            let index = capture.index as usize;
            if Some(index) == name_index {
                name = Some(capture.node.byte_range());
            } else if let Some(kind) = names.get(index).and_then(|n| SymbolKind::from_capture(n)) {
                definition = Some((kind, capture.node.byte_range()));
            }
        }
        if let Some((kind, range)) = definition {
            let name = name.unwrap_or(range.clone());
            found.push((kind, range, name));
            if found.len() == MAX_SYMBOLS {
                break;
            }
        }
    }
    symbols(text, found)
}

/// Builds the symbols from byte ranges: names, char positions and nesting.
fn symbols(text: &str, mut found: Vec<(SymbolKind, Range<usize>, Range<usize>)>) -> Vec<Symbol> {
    // Outer declarations before the ones they contain; the same declaration once.
    found.sort_by(|a, b| a.1.start.cmp(&b.1.start).then(b.1.end.cmp(&a.1.end)));
    found.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);
    let chars = CharPositions::new(
        text,
        found
            .iter()
            .flat_map(|(_, r, n)| [r.start, r.end, n.start, n.end]),
    );
    let mut symbols: Vec<Symbol> = Vec::with_capacity(found.len());
    // The open declarations that may contain the next one, with their end in bytes.
    let mut open: Vec<(usize, usize)> = Vec::new();
    for (kind, range, name) in found {
        while open.last().is_some_and(|&(_, end)| range.start >= end) {
            open.pop();
        }
        let parent = open.last().map(|&(index, _)| index);
        let depth = parent.map_or(0, |p| symbols[p].depth + 1);
        let name_text = text.get(name.clone()).unwrap_or_default();
        let end = range.end;
        symbols.push(Symbol {
            name: tidy_name(name_text),
            kind,
            range: chars.of(range.start)..chars.of(range.end),
            name_range: chars.of(name.start)..chars.of(name.end),
            parent,
            depth,
        });
        open.push((symbols.len() - 1, end));
    }
    symbols
}

/// A name on one line, with runs of whitespace (a name split over lines) as one space.
fn tidy_name(name: &str) -> String {
    name.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Byte offsets of a text and the char offsets they correspond to, worked out in one pass.
struct CharPositions {
    /// (byte, char), sorted by byte.
    pairs: Vec<(usize, usize)>,
}

impl CharPositions {
    fn new(text: &str, bytes: impl Iterator<Item = usize>) -> Self {
        let mut wanted: Vec<usize> = bytes.collect();
        wanted.sort_unstable();
        wanted.dedup();
        let mut pairs = Vec::with_capacity(wanted.len());
        let (mut byte, mut char) = (0, 0);
        for target in wanted {
            let target = target.min(text.len());
            char += text[byte..target].chars().count();
            byte = target;
            pairs.push((byte, char));
        }
        Self { pairs }
    }

    fn of(&self, byte: usize) -> usize {
        match self.pairs.binary_search_by_key(&byte, |p| p.0) {
            Ok(i) => self.pairs[i].1,
            // Only offsets given to `new` are asked for; past the end is the end.
            Err(i) => self.pairs.get(i.saturating_sub(1)).map_or(0, |p| p.1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A test query on the JSON grammar: every key.
    fn json_keys(text: &str) -> Vec<Symbol> {
        let language: Language = tree_sitter_json::LANGUAGE.into();
        let query = Query::new(
            &language,
            "(pair key: (string (string_content) @name)) @definition.key",
        )
        .unwrap();
        run_query(&language, &query, text)
    }

    #[test]
    fn symbols_nest_by_their_ranges() {
        let symbols = json_keys(r#"{"a": {"b": 1, "c": {"d": 2}}, "e": 3}"#);
        let outline: Vec<(String, usize, Option<usize>)> = symbols
            .iter()
            .map(|s| (s.name.clone(), s.depth, s.parent))
            .collect();
        assert_eq!(
            outline,
            [
                ("a".to_owned(), 0, None),
                ("b".to_owned(), 1, Some(0)),
                ("c".to_owned(), 1, Some(0)),
                ("d".to_owned(), 2, Some(2)),
                ("e".to_owned(), 0, None),
            ]
        );
        assert!(symbols.iter().all(|s| s.kind == SymbolKind::Key));
    }

    #[test]
    fn positions_are_chars_not_bytes() {
        // "ñ" and "€" take several bytes in UTF-8.
        let text = r#"{"€uro": 1, "ñu": 2}"#;
        let symbols = json_keys(text);
        let chars: Vec<char> = text.chars().collect();
        for symbol in &symbols {
            let name: String = chars[symbol.name_range.clone()].iter().collect();
            assert_eq!(name, symbol.name);
        }
        assert_eq!(symbols[1].name_range, 13..15);
    }

    #[test]
    fn half_written_documents_still_have_symbols() {
        let symbols = json_keys(r#"{"done": 1, "typing": "#);
        assert!(symbols.iter().any(|s| s.name == "done"));
    }

    #[test]
    fn every_built_in_grammar_loads_and_its_query_compiles() {
        for grammar in &GRAMMARS {
            let mut parser = Parser::new();
            parser.set_language(&(grammar.language)()).unwrap();
            assert!(grammar.query().is_some(), "{} query", grammar.id);
            assert!(extract(grammar.id, "").is_some());
        }
        assert!(extract("tsql", "CREATE TABLE t (id int)").is_none());
        assert!(has_symbols("csharp") && !has_symbols("rust"));
    }

    #[test]
    fn kinds_come_from_capture_names() {
        assert_eq!(
            SymbolKind::from_capture("definition.class"),
            Some(SymbolKind::Class)
        );
        assert_eq!(
            SymbolKind::from_capture("definition.enum_member"),
            Some(SymbolKind::EnumMember)
        );
        assert_eq!(SymbolKind::from_capture("name"), None);
        assert_eq!(SymbolKind::from_capture("definition.unknown"), None);
    }
}
