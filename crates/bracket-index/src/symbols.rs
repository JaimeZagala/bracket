//! Symbols of a document: the declarations an outline, Ctrl+T and go to definition work with.
//!
//! C#, CSS, JSON and XML are parsed with tree-sitter and their symbols come from a query per
//! language (`decisions/0003-symbols.md`). A query marks each declaration with a capture named
//! `@definition.<kind>` (`@definition.class`, `@definition.key`…) and its name with `@name`,
//! like tree-sitter's own tag queries; a kind ending in `_to_end` extends the declaration to the
//! end of the document (a C# file-scoped namespace). The queries are in `queries/`. tree-sitter works in bytes; positions are turned into
//! chars, the editor's unit, before they leave this module. Nesting comes from the ranges:
//! a symbol inside another one's range is its child.
//!
//! T-SQL uses regex rules (`queries/tsql.toml`, the format of the packages' `symbols.toml`, see
//! `rules.rs`). Razor lists its directives and parses its `@code` and `@functions` blocks with
//! the C# grammar.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::OnceLock;

use regex::Regex;
use std::sync::Arc;

use tree_sitter::{
    InputEdit, Language, Parser, Point, Query, QueryCursor, StreamingIterator, Tree,
};

use crate::rules::RuleSet;

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
        let kind = kind.strip_suffix("_to_end").unwrap_or(kind);
        let kind = kind.strip_suffix("_index").unwrap_or(kind);
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
        query: include_str!("../queries/csharp.scm"),
        compiled: OnceLock::new(),
    },
    Grammar {
        id: "css",
        language: || tree_sitter_css::LANGUAGE.into(),
        query: include_str!("../queries/css.scm"),
        compiled: OnceLock::new(),
    },
    Grammar {
        id: "json",
        language: || tree_sitter_json::LANGUAGE.into(),
        query: include_str!("../queries/json.scm"),
        compiled: OnceLock::new(),
    },
    Grammar {
        id: "xml",
        language: || tree_sitter_xml::LANGUAGE_XML.into(),
        query: include_str!("../queries/xml.scm"),
        compiled: OnceLock::new(),
    },
];

fn grammar(language: &str) -> Option<&'static Grammar> {
    GRAMMARS.iter().find(|g| g.id == language)
}

/// Languages with symbols that are not parsed with a grammar of their own.
const OTHER_LANGUAGES: [&str; 2] = ["tsql", "razor"];

/// Whether `language` (a `bracket-syntax` id) has a built-in symbol extractor.
pub fn has_symbols(language: &str) -> bool {
    grammar(language).is_some() || OTHER_LANGUAGES.contains(&language)
}

/// The symbols of `text` in `language`, in document order; `None` if the language has no
/// built-in extractor.
pub fn extract(language: &str, text: &str) -> Option<Vec<Symbol>> {
    match language {
        "tsql" => Some(extract_with_rules(tsql_rules(), text)),
        "razor" => Some(razor(text)),
        _ => {
            let grammar = grammar(language)?;
            Some(run_query(&(grammar.language)(), grammar.query()?, text))
        }
    }
}

/// Extracts the symbols of one document again and again as it changes. With a grammar, the
/// previous tree is reused: the old and new texts are compared, their common start and end give
/// one edit covering every change, and tree-sitter parses only what that edit touched.
pub struct SymbolParser {
    kind: ParserKind,
}

enum ParserKind {
    Grammar {
        grammar: &'static Grammar,
        parser: Parser,
        /// The text last parsed and its tree.
        last: Option<(String, Tree)>,
    },
    Rules(Arc<RuleSet>),
    Builtin(fn(&str) -> Vec<Symbol>),
}

impl SymbolParser {
    /// A parser for `language`; a language package's `rules` win over the built-in extractor.
    /// `None` if the language has no symbols.
    pub fn new(language: &str, rules: Option<Arc<RuleSet>>) -> Option<Self> {
        let kind = if let Some(rules) = rules {
            ParserKind::Rules(rules)
        } else {
            match language {
                "tsql" => ParserKind::Builtin(|text| extract_with_rules(tsql_rules(), text)),
                "razor" => ParserKind::Builtin(razor),
                _ => {
                    let grammar = grammar(language)?;
                    grammar.query()?;
                    let mut parser = Parser::new();
                    parser.set_language(&(grammar.language)()).ok()?;
                    ParserKind::Grammar {
                        grammar,
                        parser,
                        last: None,
                    }
                }
            }
        };
        Some(Self { kind })
    }

    /// The symbols of the document's current `text`.
    pub fn symbols(&mut self, text: String) -> Vec<Symbol> {
        match &mut self.kind {
            ParserKind::Rules(rules) => extract_with_rules(rules, &text),
            ParserKind::Builtin(extract) => extract(&text),
            ParserKind::Grammar {
                grammar,
                parser,
                last,
            } => {
                let old = last.take().map(|(old_text, mut tree)| {
                    tree.edit(&edit_between(&old_text, &text));
                    tree
                });
                let Some(tree) = parser.parse(&text, old.as_ref()) else {
                    return Vec::new();
                };
                let query = grammar.query().expect("checked when the parser was made");
                let symbols = symbols(&text, query_tree(query, &tree, &text));
                *last = Some((text, tree));
                symbols
            }
        }
    }

    /// The symbols of a `text` unrelated to the previous one: nothing is kept for the next call.
    /// For extracting many files with one parser.
    pub fn extract(&mut self, text: &str) -> Vec<Symbol> {
        match &mut self.kind {
            ParserKind::Rules(rules) => extract_with_rules(rules, text),
            ParserKind::Builtin(extract) => extract(text),
            ParserKind::Grammar {
                grammar,
                parser,
                last,
            } => {
                *last = None;
                let Some(tree) = parser.parse(text, None) else {
                    return Vec::new();
                };
                let query = grammar.query().expect("checked when the parser was made");
                symbols(text, query_tree(query, &tree, text))
            }
        }
    }
}

/// The single edit that turns `old` into `new`: everything between their common start and their
/// common end. Both ends fall on char boundaries.
fn edit_between(old: &str, new: &str) -> InputEdit {
    let (a, b) = (old.as_bytes(), new.as_bytes());
    let mut start = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    while !old.is_char_boundary(start) || !new.is_char_boundary(start) {
        start -= 1;
    }
    let room = a.len().min(b.len()) - start;
    let mut suffix = a
        .iter()
        .rev()
        .zip(b.iter().rev())
        .take(room)
        .take_while(|(x, y)| x == y)
        .count();
    while !old.is_char_boundary(a.len() - suffix) || !new.is_char_boundary(b.len() - suffix) {
        suffix -= 1;
    }
    let (old_end, new_end) = (a.len() - suffix, b.len() - suffix);
    InputEdit {
        start_byte: start,
        old_end_byte: old_end,
        new_end_byte: new_end,
        start_position: point_at(old, start),
        old_end_position: point_at(old, old_end),
        new_end_position: point_at(new, new_end),
    }
}

/// Row and byte column of a byte offset, as tree-sitter counts them (rows end at `\n`).
fn point_at(text: &str, byte: usize) -> Point {
    let before = &text.as_bytes()[..byte];
    let row = before.iter().filter(|&&c| c == b'\n').count();
    let column = byte
        - before
            .iter()
            .rposition(|&c| c == b'\n')
            .map_or(0, |i| i + 1);
    Point { row, column }
}

/// The symbols that a language package's rules (its `symbols.toml`) find in `text`.
pub fn extract_with_rules(rules: &RuleSet, text: &str) -> Vec<Symbol> {
    let found = rules
        .find(text, MAX_SYMBOLS)
        .into_iter()
        .map(|m| Found {
            kind: m.kind,
            range: m.range,
            name_range: m.name_range,
            name: m.name,
        })
        .collect();
    symbols(text, found)
}

/// The built-in T-SQL rules, compiled once.
fn tsql_rules() -> &'static RuleSet {
    static RULES: OnceLock<RuleSet> = OnceLock::new();
    RULES.get_or_init(|| {
        RuleSet::parse(include_str!("../queries/tsql.toml"))
            .expect("the built-in T-SQL rules are valid")
    })
}

/// Razor: its directives (`@page`, `@inject`…) and the members of its `@code` and `@functions`
/// blocks, found with the C# grammar. Each block is parsed as the body of a class, so the
/// grammar itself finds where it ends, past strings and comments with braces in them.
fn razor(text: &str) -> Vec<Symbol> {
    static DIRECTIVE: OnceLock<Regex> = OnceLock::new();
    static BLOCK: OnceLock<Regex> = OnceLock::new();
    let directive = DIRECTIVE.get_or_init(|| {
        Regex::new(
            r"(?m)^[ \t]*(@(?:page|inject|using|inherits|implements|layout|attribute|namespace|model|typeparam|rendermode))\b[^\r\n]*",
        )
        .expect("valid pattern")
    });
    let block = BLOCK.get_or_init(|| Regex::new(r"@(code|functions)\s*\{").expect("valid pattern"));
    let mut found = Vec::new();
    for captures in directive.captures_iter(text) {
        let (whole, keyword) = (captures.get(0).expect("whole match"), &captures[1]);
        let at = whole.start() + whole.as_str().find('@').unwrap_or(0);
        found.push(Found {
            kind: SymbolKind::Directive,
            range: at..whole.end(),
            name_range: at..at + keyword.len(),
            name: tidy_name(&text[at..whole.end()]),
        });
    }
    let Some(csharp) = grammar("csharp") else {
        return symbols(text, found);
    };
    let Some(query) = csharp.query() else {
        return symbols(text, found);
    };
    const WRAPPER: &str = "class R ";
    for captures in block.captures_iter(text) {
        let (m, keyword) = (captures.get(0).expect("whole match"), &captures[1]);
        let brace = m.end() - 1;
        let wrapped = format!("{WRAPPER}{}", &text[brace..]);
        let inner = find(&(csharp.language)(), query, &wrapped);
        let Some(class) = inner
            .iter()
            .find(|f| f.range.start == 0 && f.kind == SymbolKind::Class)
        else {
            continue;
        };
        let class_end = class.range.end;
        let back = |byte: usize| byte - WRAPPER.len() + brace;
        found.push(Found {
            kind: SymbolKind::Directive,
            range: m.start()..back(class_end),
            name_range: m.start()..m.start() + 1 + keyword.len(),
            name: format!("@{keyword}"),
        });
        found.extend(
            inner
                .into_iter()
                .filter(|f| f.range.start > WRAPPER.len() && f.range.end <= class_end)
                .map(|f| Found {
                    range: back(f.range.start)..back(f.range.end),
                    name_range: back(f.name_range.start)..back(f.name_range.end),
                    ..f
                }),
        );
    }
    symbols(text, found)
}

/// A declaration found by a query, in bytes, with its name already worked out.
#[derive(Debug, PartialEq, Eq)]
struct Found {
    kind: SymbolKind,
    range: Range<usize>,
    name_range: Range<usize>,
    name: String,
}

/// How a `@definition.<kind>` capture reads its name and range.
struct Capture {
    kind: SymbolKind,
    /// `_to_end`: the declaration runs to the end of the document.
    to_end: bool,
    /// `_index`: named after its position in its parent, like `[2]` in a JSON array.
    index: bool,
}

fn parse_capture(name: &str) -> Option<Capture> {
    let kind = SymbolKind::from_capture(name)?;
    Some(Capture {
        kind,
        to_end: name.ends_with("_to_end"),
        index: name.ends_with("_index"),
    })
}

/// Parses `text`, runs `query` over it and builds the symbols.
fn run_query(language: &Language, query: &Query, text: &str) -> Vec<Symbol> {
    symbols(text, find(language, query, text))
}

/// Parses `text` and runs `query` over it: the declarations, in bytes.
///
/// Besides `@definition.<kind>` and `@name`, a query may capture `@detail`: text added to the
/// name (an XML element's `id="…"`). Without `@name`, the name is the declaration's first line
/// up to `{` (a CSS `@media (…)` rule).
fn find(language: &Language, query: &Query, text: &str) -> Vec<Found> {
    let mut parser = Parser::new();
    if parser.set_language(language).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(text, None) else {
        return Vec::new();
    };
    query_tree(query, &tree, text)
}

/// Runs `query` over a parsed `tree` of `text`: the declarations, in bytes.
fn query_tree(query: &Query, tree: &Tree, text: &str) -> Vec<Found> {
    let names = query.capture_names();
    let name_index = names.iter().position(|n| *n == "name");
    let detail_index = names.iter().position(|n| *n == "detail");
    // The position of each child in its parent, worked out once per parent.
    let mut positions: HashMap<usize, HashMap<usize, usize>> = HashMap::new();
    let mut found: Vec<Found> = Vec::new();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, tree.root_node(), text.as_bytes());
    while let Some(m) = matches.next() {
        let mut definition = None;
        let mut name = None;
        let mut detail = None;
        for capture in m.captures() {
            let index = Some(capture.index as usize);
            if index == name_index {
                name = Some(capture.node.byte_range());
            } else if index == detail_index {
                detail = Some(capture.node.byte_range());
            } else if let Some(parsed) = names
                .get(capture.index as usize)
                .and_then(|n| parse_capture(n))
            {
                definition = Some((parsed, capture.node));
            }
        }
        let Some((capture, node)) = definition else {
            continue;
        };
        let mut range = node.byte_range();
        if capture.to_end {
            range.end = text.len();
        }
        let (name_range, name) = if capture.index {
            let position = node.parent().map_or(0, |parent| {
                let children = positions.entry(parent.id()).or_insert_with(|| {
                    let mut walk = parent.walk();
                    parent
                        .named_children(&mut walk)
                        .enumerate()
                        .map(|(i, child)| (child.id(), i))
                        .collect()
                });
                children.get(&node.id()).copied().unwrap_or(0)
            });
            (range.start..range.start, format!("[{position}]"))
        } else if let Some(name) = name {
            let mut label = tidy_name(text.get(name.clone()).unwrap_or_default());
            if let Some(detail) = detail {
                label.push(' ');
                label.push_str(&tidy_name(text.get(detail).unwrap_or_default()));
            }
            (name, label)
        } else {
            let declaration = text.get(range.clone()).unwrap_or_default();
            let header = declaration
                .split(['{', '\n'])
                .next()
                .unwrap_or_default()
                .trim_end();
            (range.start..range.start + header.len(), tidy_name(header))
        };
        found.push(Found {
            kind: capture.kind,
            range,
            name_range,
            name,
        });
        if found.len() == MAX_SYMBOLS {
            break;
        }
    }
    found
}

/// Builds the symbols from what the query found: char positions and nesting.
fn symbols(text: &str, mut found: Vec<Found>) -> Vec<Symbol> {
    // Outer declarations before the ones they contain.
    found.sort_by(|a, b| {
        a.range
            .start
            .cmp(&b.range.start)
            .then(b.range.end.cmp(&a.range.end))
    });
    // The same declaration once ("int a, b;" has two names, so two symbols). When several
    // patterns matched it, the longest name wins: it has the detail.
    found.dedup_by(|later, kept| {
        let same = later.kind == kept.kind
            && later.range == kept.range
            && later.name_range == kept.name_range;
        if same && later.name.len() > kept.name.len() {
            std::mem::swap(&mut later.name, &mut kept.name);
        }
        same
    });
    let chars = CharPositions::new(
        text,
        found.iter().flat_map(|f| {
            [
                f.range.start,
                f.range.end,
                f.name_range.start,
                f.name_range.end,
            ]
        }),
    );
    let mut symbols: Vec<Symbol> = Vec::with_capacity(found.len());
    // The open declarations that may contain the next one, with their byte range. One with the
    // same range is a sibling ("int a, b;"), not a parent.
    let mut open: Vec<(usize, Range<usize>)> = Vec::new();
    for f in found {
        while open
            .last()
            .is_some_and(|(_, last)| f.range.start >= last.end || *last == f.range)
        {
            open.pop();
        }
        let parent = open.last().map(|(index, _)| *index);
        let depth = parent.map_or(0, |p| symbols[p].depth + 1);
        symbols.push(Symbol {
            name: f.name,
            kind: f.kind,
            range: chars.of(f.range.start)..chars.of(f.range.end),
            name_range: chars.of(f.name_range.start)..chars.of(f.name_range.end),
            parent,
            depth,
        });
        open.push((symbols.len() - 1, f.range));
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
        assert!(extract("python", "def x(): pass").is_none());
        assert!(has_symbols("csharp") && !has_symbols("rust"));
    }

    /// The outline as text: one line per symbol, indented by depth.
    fn outline(language: &str, text: &str) -> String {
        extract(language, text)
            .unwrap()
            .iter()
            .map(|s| {
                format!(
                    "{}{} {:?}
",
                    "  ".repeat(s.depth),
                    s.name,
                    s.kind
                )
            })
            .collect()
    }

    #[test]
    fn csharp_declarations_are_found_and_nested() {
        let expected = "Demo.Billing Namespace
  InvoiceChanged Delegate
  Invoice Class
    _lines Field
    _removed Field
    Count Field
    Changed Event
    Saved Event
    Invoice Constructor
    Number Property
    Total Property
    this Indexer
    CompareTo Method
    Describe Method
    Dump Method
    Line Class
      Sku Property
      Quantity Property
  Customer Record
  Money Record
  Point Struct
    X Property
    Y Property
  IInvoiceStore Interface
    Find Method
    Count Property
  Status Enum
    Draft EnumMember
    Sent EnumMember
    Paid EnumMember
";
        let text = include_str!("../corpus/Sample.cs");
        assert_eq!(outline("csharp", text), expected);
    }

    #[test]
    fn css_rules_at_rules_and_variables() {
        let expected = "\
:root Selector
  --accent Variable
  --gap Variable
body, html Selector
.card > .title:hover Selector
@media (max-width: 600px) AtRule
  .card Selector
  @supports (display: grid) AtRule
    .grid Selector
fade-in Keyframes
@font-face AtRule
";
        assert_eq!(
            outline("css", include_str!("../corpus/Sample.css")),
            expected
        );
    }

    #[test]
    fn json_keys_and_array_items() {
        let expected = "\
name Key
version Key
settings Key
  theme Key
  font Key
    family Key
    size Key
items Key
  [0] Key
    id Key
    tags Key
  [1] Key
    id Key
  [2] Key
ñandú Key
";
        assert_eq!(
            outline("json", include_str!("../corpus/Sample.json")),
            expected
        );
    }

    #[test]
    fn xml_elements_with_their_id_or_name() {
        let expected = "\
Invoice id=\"INV-1\" Element
  Header Element
    Number Element
    Customer name=\"Demo\" Element
  Lines Element
    Line id=\"1\" Element
      Sku Element
    Line ID=\"2\" Element
    Line Element
      Sku Element
";
        assert_eq!(
            outline("xml", include_str!("../corpus/Sample.xml")),
            expected
        );
        // The name range is the tag's, for go to definition.
        let text = "<a id=\"x\"/>";
        let symbols = extract("xml", text).unwrap();
        assert_eq!(symbols[0].name_range, 1..2);
    }

    #[test]
    fn tsql_objects_by_rules() {
        let text = include_str!("../corpus/Sample.sql");
        let expected = "\
dbo.Invoice Table
dbo.GetInvoice Procedure
dbo.InvoiceTotal Function
Reporting.OpenInvoices View
trg_Invoice_Insert Trigger
dbo.InvoiceIds Type
";
        assert_eq!(outline("tsql", text), expected);
        // A procedure ends at its GO; its name range is the name as written.
        let symbols = extract("tsql", text).unwrap();
        let chars: Vec<char> = text.chars().collect();
        let procedure = &symbols[1];
        let body: String = chars[procedure.range.clone()].iter().collect();
        assert!(body.starts_with("CREATE PROCEDURE") && body.trim_end().ends_with("END"));
        let name: String = chars[procedure.name_range.clone()].iter().collect();
        assert_eq!(name, "[dbo].[GetInvoice]");
    }

    #[test]
    fn razor_directives_and_code_blocks() {
        let text = include_str!("../corpus/Sample.razor");
        let expected = "\
@page \"/invoices/{Id:int}\" Directive
@using Demo.Billing Directive
@inject IInvoiceStore Store Directive
@inherits ComponentBase Directive
@code Directive
  Id Property
  title Field
  OnInitialized Method
  Row Class
    Sku Property
@functions Directive
  Count Method
";
        assert_eq!(outline("razor", text), expected);
        // The @code block ends at its own closing brace, not at a brace in a string.
        let symbols = extract("razor", text).unwrap();
        let code = symbols.iter().find(|s| s.name == "@code").unwrap();
        let chars: Vec<char> = text.chars().collect();
        let block: String = chars[code.range.clone()].iter().collect();
        assert!(block.ends_with("}\n}") || block.ends_with('}'));
        assert!(!block.contains("<footer>"));
    }

    #[test]
    fn package_rules_find_symbols_too() {
        let rules = RuleSet::parse(
            "format_version = 1\n[[symbol]]\nkind = \"function\"\npattern = '^def (?<name>\\w+)'\n",
        )
        .unwrap();
        let symbols = extract_with_rules(&rules, "def run():\n    pass\ndef stop():\n");
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["run", "stop"]);
        assert!(has_symbols("tsql") && has_symbols("razor"));
    }

    #[test]
    fn huge_documents_stop_at_the_limit() {
        let text = format!(
            "{{{}}}",
            (0..MAX_SYMBOLS + 10)
                .map(|i| format!("\"k{i}\":1"))
                .collect::<Vec<_>>()
                .join(",")
        );
        assert_eq!(extract("json", &text).unwrap().len(), MAX_SYMBOLS);
    }

    #[test]
    fn a_file_scoped_namespace_holds_the_rest_of_the_file() {
        let text = "namespace A;
class B { }
";
        let symbols = extract("csharp", text).unwrap();
        assert_eq!(symbols[0].range, 0..text.chars().count());
        assert_eq!(symbols[1].parent, Some(0));
        // A block namespace ends at its closing brace.
        let text = "namespace A { class B { } }
class C { }";
        let symbols = extract("csharp", text).unwrap();
        let c = symbols.iter().find(|s| s.name == "C").unwrap();
        assert_eq!(c.parent, None);
    }

    #[test]
    fn csharp_being_typed_keeps_its_symbols() {
        // A statement being typed inside a method: the members around it are still found.
        let text = "class A {
  int x;
  void M() {
    var y = 
  }
  string Name { get; }
}";
        let names: Vec<String> = extract("csharp", text)
            .unwrap()
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert!(names.contains(&"A".to_owned()) && names.contains(&"x".to_owned()));
        assert!(names.contains(&"Name".to_owned()), "{names:?}");
    }

    #[test]
    fn incremental_parsing_gives_the_same_symbols_as_a_fresh_one() {
        let mut parser = SymbolParser::new("csharp", None).unwrap();
        let mut text = include_str!("../corpus/Sample.cs").to_owned();
        assert_eq!(
            parser.symbols(text.clone()),
            extract("csharp", &text).unwrap()
        );
        // A member added in the middle, one renamed, non-ASCII text, and a deletion.
        for (from, to) in [
            (
                "    public static int Count;",
                "    public static int Count;\n    public void Añadir() { }",
            ),
            ("Describe()", "Describir()"),
            ("public enum Status", "public enum Estado"),
            ("    public int X { get; }\n", ""),
        ] {
            text = text.replacen(from, to, 1);
            assert_eq!(
                parser.symbols(text.clone()),
                extract("csharp", &text).unwrap(),
                "after replacing {from:?}"
            );
        }
        let names: Vec<String> = parser
            .symbols(text.clone())
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert!(names.contains(&"Añadir".to_owned()) && names.contains(&"Estado".to_owned()));
        assert!(!names.contains(&"X".to_owned()));
    }

    #[test]
    fn the_edit_between_two_texts_is_their_middle() {
        let edit = edit_between("let ñ = 1;\nfoo();", "let ñ = 22;\nfoo();");
        assert_eq!(
            (edit.start_byte, edit.old_end_byte, edit.new_end_byte),
            (9, 10, 11)
        );
        assert_eq!(edit.start_position, Point { row: 0, column: 9 });
        let edit = edit_between("aaa", "aaaa");
        assert_eq!(
            (edit.start_byte, edit.old_end_byte, edit.new_end_byte),
            (3, 3, 4)
        );
        let edit = edit_between("€", "£");
        assert_eq!(
            (edit.start_byte, edit.old_end_byte, edit.new_end_byte),
            (0, 3, 2)
        );
    }

    #[test]
    fn parsers_exist_for_every_language_with_symbols() {
        for language in ["csharp", "css", "json", "xml", "tsql", "razor"] {
            let mut parser = SymbolParser::new(language, None).unwrap();
            parser.symbols(String::new());
        }
        assert!(SymbolParser::new("rust", None).is_none());
        let rules = Arc::new(
            RuleSet::parse(
                "format_version = 1\n[[symbol]]\nkind = \"function\"\npattern = '^fn (\\w+)'\n",
            )
            .unwrap(),
        );
        let mut parser = SymbolParser::new("rust", Some(rules)).unwrap();
        assert_eq!(parser.symbols("fn main() {}".into())[0].name, "main");
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
        assert_eq!(
            SymbolKind::from_capture("definition.namespace_to_end"),
            Some(SymbolKind::Namespace)
        );
        assert_eq!(SymbolKind::from_capture("name"), None);
        assert_eq!(SymbolKind::from_capture("definition.unknown"), None);
    }
}
