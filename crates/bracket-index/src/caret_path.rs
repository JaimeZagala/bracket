//! The path to the caret in a structured document: an XPath in XML (`/Invoice/Lines/Line[3]/@id`)
//! and a JSON path in JSON (`$.items[2].name`), worked out from the tree-sitter tree. The parser
//! keeps the previous tree, so moving the caret costs no parse and an edit only a partial one.

use tree_sitter::{Language, Node, Parser, Tree};

use crate::symbols::edit_between;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Xml,
    Json,
}

/// Whether documents in `language` (a `bracket-syntax` id) have a caret path.
pub fn has_path(language: &str) -> bool {
    kind(language).is_some()
}

fn kind(language: &str) -> Option<Kind> {
    match language {
        "xml" => Some(Kind::Xml),
        "json" => Some(Kind::Json),
        _ => None,
    }
}

/// Works out the caret path of one document again and again as it changes.
pub struct PathParser {
    kind: Kind,
    parser: Parser,
    /// The text last parsed and its tree.
    last: Option<(String, Tree)>,
}

impl PathParser {
    /// `None` if `language` has no caret path.
    pub fn new(language: &str) -> Option<Self> {
        let kind = kind(language)?;
        let grammar: Language = match kind {
            Kind::Xml => tree_sitter_xml::LANGUAGE_XML.into(),
            Kind::Json => tree_sitter_json::LANGUAGE.into(),
        };
        let mut parser = Parser::new();
        parser.set_language(&grammar).ok()?;
        Some(Self {
            kind,
            parser,
            last: None,
        })
    }

    /// The path to `caret` (in chars) in `text`; `None` outside any element or value.
    pub fn path_at(&mut self, text: String, caret: usize) -> Option<String> {
        let tree = match self.last.take() {
            Some((old, tree)) if old == text => tree,
            Some((old, mut tree)) => {
                tree.edit(&edit_between(&old, &text));
                self.parser.parse(&text, Some(&tree))?
            }
            None => self.parser.parse(&text, None)?,
        };
        let byte = text
            .char_indices()
            .nth(caret)
            .map_or(text.len(), |(i, _)| i);
        let path = match self.kind {
            Kind::Xml => xml_path(&tree, &text, byte),
            Kind::Json => json_path(&tree, &text, byte),
        };
        self.last = Some((text, tree));
        path
    }
}

fn node_text<'a>(node: Node, text: &'a str) -> &'a str {
    &text[node.byte_range()]
}

/// The node at `byte`: the smallest one holding it, or the one ending right there (the caret just
/// after a name still points at it).
fn node_at(tree: &Tree, byte: usize) -> Node<'_> {
    let root = tree.root_node();
    let node = root.descendant_for_byte_range(byte, byte).unwrap_or(root);
    if byte > 0 && node.start_byte() == byte && node.kind() != "Name" {
        let before = root
            .descendant_for_byte_range(byte - 1, byte - 1)
            .unwrap_or(node);
        if before.end_byte() == byte && before.kind() == "Name" {
            return before;
        }
    }
    node
}

/// `/Root/Child[2]/@attribute`: each element by its tag, with its position among the siblings
/// of the same name when there are several, and the attribute the caret is in.
fn xml_path(tree: &Tree, text: &str, byte: usize) -> Option<String> {
    let tag_name = |element: Node| {
        let tag = element.named_child(0)?;
        let name = tag.named_child(0).filter(|n| n.kind() == "Name")?;
        Some(node_text(name, text))
    };
    let mut segments = Vec::new();
    let mut attribute = None;
    let mut node = Some(node_at(tree, byte));
    while let Some(current) = node {
        match current.kind() {
            "Attribute" if attribute.is_none() && segments.is_empty() => {
                attribute = current
                    .named_child(0)
                    .filter(|n| n.kind() == "Name")
                    .map(|n| node_text(n, text));
            }
            "element" => {
                let name = tag_name(current)?;
                let mut position = None;
                if let Some(parent) = current.parent() {
                    let mut cursor = parent.walk();
                    let same: Vec<Node> = parent
                        .named_children(&mut cursor)
                        .filter(|n| n.kind() == "element" && tag_name(*n) == Some(name))
                        .collect();
                    if same.len() > 1 {
                        position = same.iter().position(|n| n.id() == current.id());
                    }
                }
                segments.push(match position {
                    Some(i) => format!("{name}[{}]", i + 1),
                    None => name.to_owned(),
                });
            }
            _ => {}
        }
        node = current.parent();
    }
    if segments.is_empty() {
        return None;
    }
    segments.reverse();
    let mut path = format!("/{}", segments.join("/"));
    if let Some(attribute) = attribute {
        path.push_str("/@");
        path.push_str(attribute);
    }
    Some(path)
}

/// `$.key[2]['other key']`: each key of an object, and each position (from 0) in an array.
fn json_path(tree: &Tree, text: &str, byte: usize) -> Option<String> {
    let mut segments = Vec::new();
    let mut child = node_at(tree, byte);
    let mut inside_value = false;
    while let Some(parent) = child.parent() {
        match parent.kind() {
            "pair" => {
                let key = parent.child_by_field_name("key")?;
                let content = key.named_child(0).map_or("", |c| node_text(c, text));
                segments.push(key_segment(content));
            }
            "array" => {
                let mut cursor = parent.walk();
                let index = parent
                    .named_children(&mut cursor)
                    .filter(|n| n.kind() != "comment")
                    .position(|n| n.id() == child.id());
                if let Some(index) = index {
                    segments.push(format!("[{index}]"));
                }
            }
            "document" => inside_value = child.kind() != "comment",
            _ => {}
        }
        child = parent;
    }
    if !inside_value {
        return None;
    }
    segments.reverse();
    Some(format!("${}", segments.concat()))
}

/// `.name` for a key that is an identifier, `['any key']` for the rest.
fn key_segment(key: &str) -> String {
    let mut chars = key.chars();
    let identifier = chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$');
    if identifier {
        format!(".{key}")
    } else {
        format!("['{}']", key.replace('\\', "\\\\").replace('\'', "\\'"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The path at the `|` in `marked`.
    fn path(language: &str, marked: &str) -> Option<String> {
        let caret = marked.chars().position(|c| c == '|').unwrap();
        let text = marked.replacen('|', "", 1);
        PathParser::new(language).unwrap().path_at(text, caret)
    }

    #[test]
    fn xpaths_number_repeated_elements_and_name_attributes() {
        let xml = "<?xml version=\"1.0\"?>\n<Invoice>\n  <Lines>\n    <Line/>\n    <Line/>\n    <Line id=\"3\">\n      <Amount>1|0</Amount>\n    </Line>\n  </Lines>\n</Invoice>\n";
        assert_eq!(
            path("xml", xml),
            Some("/Invoice/Lines/Line[3]/Amount".into())
        );
        let attribute = xml.replace("|", "").replace("id=\"3\"", "id=\"|3\"");
        assert_eq!(
            path("xml", &attribute),
            Some("/Invoice/Lines/Line[3]/@id".into())
        );
        let on_tag = xml.replace("|", "").replace("<Lines>", "<Li|nes>");
        assert_eq!(path("xml", &on_tag), Some("/Invoice/Lines".into()));
        let prolog = xml.replace("|", "").replace("version", "ver|sion");
        assert_eq!(path("xml", &prolog), None, "outside the root element");
    }

    #[test]
    fn xpaths_keep_namespace_prefixes() {
        let xsig = "<ds:Signature xmlns:ds=\"x\"><ds:SignedInfo><ds:Ref|erence/></ds:SignedInfo></ds:Signature>";
        assert_eq!(
            path("xml", xsig),
            Some("/ds:Signature/ds:SignedInfo/ds:Reference".into())
        );
    }

    #[test]
    fn json_paths_have_keys_and_positions() {
        let json =
            "{\n  \"items\": [1, {\"name\": \"a|b\"}],\n  \"two words\": {\"it's\": true}\n}";
        assert_eq!(path("json", json), Some("$.items[1].name".into()));
        let key = json.replace("|", "").replace("\"name\"", "\"na|me\"");
        assert_eq!(path("json", &key), Some("$.items[1].name".into()));
        let odd = json.replace("|", "").replace("true", "tr|ue");
        assert_eq!(path("json", &odd), Some("$['two words']['it\\'s']".into()));
        let number = json.replace("|", "").replace("[1,", "[|1,");
        assert_eq!(path("json", &number), Some("$.items[0]".into()));
        let root = json.replace("|", "").replacen("{\n", "{|\n", 1);
        assert_eq!(path("json", &root), Some("$".into()));
    }

    #[test]
    fn edits_and_moves_reuse_the_tree() {
        let mut parser = PathParser::new("json").unwrap();
        let text = "{\"a\": {\"b\": 1}}".to_owned();
        assert_eq!(parser.path_at(text.clone(), 12), Some("$.a.b".into()));
        assert_eq!(parser.path_at(text, 3), Some("$.a".into()));
        let edited = "{\"a\": {\"b\": 1, \"c\": [2]}}".to_owned();
        assert_eq!(parser.path_at(edited, 21), Some("$.a.c[0]".into()));
        assert!(PathParser::new("csharp").is_none());
        assert!(has_path("xml") && !has_path("css"));
    }
}
