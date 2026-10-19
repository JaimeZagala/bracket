//! Symbols of a document: the declarations an outline, Ctrl+T and go to definition work with.
//!
//! C#, CSS, JSON and XML are parsed with tree-sitter and their symbols come from a query per
//! language (`decisions/0003-symbols.md`). A query marks each declaration with a capture named
//! `@definition.<kind>` (`@definition.class`, `@definition.key`…) and its name with `@name`,
//! like tree-sitter's own tag queries; a kind ending in `_to_end` extends the declaration to the
//! end of the document (a C# file-scoped namespace). The queries are in `queries/`. tree-sitter works in bytes; positions are turned into
//! chars, the editor's unit, before they leave this module. Nesting comes from the ranges:
//! a symbol inside another one's range is its child.

use std::collections::HashMap;
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

/// Parses `text` and runs `query` over it.
///
/// Besides `@definition.<kind>` and `@name`, a query may capture `@detail`: text added to the
/// name (an XML element's `id="…"`). Without `@name`, the name is the declaration's first line
/// up to `{` (a CSS `@media (…)` rule).
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
    symbols(text, found)
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
        assert!(extract("tsql", "CREATE TABLE t (id int)").is_none());
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
