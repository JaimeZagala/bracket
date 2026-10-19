**English** · [Español](../es/languages.md)

# Languages

Which languages Bracket brings, what it does with each one and how to add others with a package.

## Built-in languages

| Language | Id | Extensions | Badge | Symbols | Path at the caret |
|---|---|---|---|---|---|
| C# | `csharp` | `.cs`, `.csx` | C# | tree-sitter | — |
| Razor | `razor` | `.razor`, `.cshtml` | @ | directives by rules; `@code` with tree-sitter | — |
| CSS | `css` | `.css` | # | tree-sitter | — |
| T-SQL | `tsql` | `.sql` | SQL | regex rules | — |
| JSON | `json` | `.json`, `.jsonc` | {} | tree-sitter | JSON path |
| XML | `xml` | `.xml`, `.xsig`, `.xsd`, `.xsl`, `.xslt`, `.config`, `.csproj`, `.vbproj`, `.props`, `.targets`, `.resx`, `.nuspec`, `.xaml` | <> | tree-sitter | XPath |
| Rust | `rust` | `.rs` | RS | — | — |

All of them have syntax highlighting (syntect's grammars, compiled into the executable). The language is chosen by the extension and can be changed in the status bar.

## What symbols are used for

Symbols are a file's declarations: classes, methods, CSS rules, JSON keys, procedures… They drive:

- **Outline** (a side panel tab, **View → Outline**): the active document's symbols as a tree.
- **Ctrl+Shift+O** (`@` in the palette): go to a symbol of the active document.
- **Ctrl+T** (`#` in the palette): go to a symbol of any file in the project.
- **F12 and Ctrl+click**: go to the definition of the name at the caret.

**Shift+F12** (references) does not use symbols: it searches the project for the name as a whole word, so it works in any language. It matches case except in T-SQL and in text without a language.

Limits: the outline and the path at the caret, documents up to 10 MB; the project index, files up to 1 MB; at most 50,000 symbols per file.

## Symbols of each language

### C#

Namespaces (also file-scoped ones, `namespace Demo;`, which span the rest of the file), classes, structs, interfaces, records, enums and their members, delegates, methods, constructors, properties, fields (one per variable: `int a, b;` is two), events and indexers (`this`). Each symbol hangs from the one that contains it.

### Razor

The directives `@page`, `@using`, `@inject`, `@inherits`, `@implements`, `@layout`, `@attribute`, `@namespace`, `@model`, `@typeparam` and `@rendermode`, named by their line; and the members of `@code` and `@functions` blocks, parsed with the C# grammar. In Razor, go to definition on `<MyComponent />` opens `MyComponent.razor`.

### CSS

Rules, named by their selectors (`.btn, .btn:hover`); `@media`, `@supports` and other `@` rules, by their first line; `@keyframes`, by their name; and variables (`--main-color`). Rules inside `@media` hang from it. Go to definition takes `-` as part of the name, so it works with variables.

### JSON

Every key, nested as in the document. Objects and arrays inside an array are named by their position: `[0]`, `[1]`…

### XML and XSIG

Every element, named by its tag (with its prefix: `ds:Signature`) and, if it has an `id`, `Id`, `ID`, `name` or `Name` attribute, that attribute too: `Line id="3"`.

### T-SQL

`CREATE`, `ALTER` and `CREATE OR ALTER` of `PROCEDURE` (or `PROC`), `FUNCTION`, `VIEW` and `TRIGGER`; `CREATE TABLE` and `CREATE TYPE`. Ignoring case. Brackets are taken out of names (`[dbo].[GetInvoice]` becomes `dbo.GetInvoice`) and each symbol runs to the next `GO` or the next symbol. These are the same rules a package can bring (see [symbols.toml](#symbolstoml)).

### Rust

No symbols for now: highlighting only. A package can add them with a `symbols.toml`.

## Path at the caret

In XML and XSIG, the status bar shows the XPath of the element at the caret: `/Invoice/Lines/Line[3]/Amount`. An element carries its position (from 1) only if it has siblings with the same tag, and inside an attribute `/@name` is added. In JSON, the path of the value: `$.items[2].name`, with array positions from 0 and `['other key']` for keys that are not identifiers. A click copies it.

## Language packages

A package adds a language, or replaces a built-in one, with one folder per language inside `languages`:

- **Installed:** `%APPDATA%\Bracket\languages\` and, with lower priority, `languages\` next to `bracket.exe`.
- **Portable:** only `languages\` next to `bracket.exe`.

The folder name is the language id; if it matches a built-in one (`csharp`, `razor`, `css`, `tsql`, `json`, `xml`, `rust`), it replaces it. If a package has a problem (a missing key, a `format_version` from another version, a grammar with errors), it is skipped and the status bar says so; if the error is in `symbols.toml`, the language loads without those rules. Changes to packages apply when Bracket restarts.

```
languages\
  python\
    language.toml          required
    syntax.sublime-syntax  optional: the grammar for highlighting
    symbols.toml           optional: rules for the symbols
```

### language.toml

```toml
format_version = 1               # required: format version
name = "Python"                  # required: the name shown
extensions = ["py", "pyw"]       # required: without the dot
scope = "source.python"          # optional: a grammar Bracket already includes
tab_width = 4                    # optional: tab width for this language
line_comment = "#"               # optional, for future features
block_comment = ['"""', '"""']   # optional, for future features
pairs = [["(", ")"], ["[", "]"]] # optional, for future features

[badge]                          # optional: by default the first two letters on grey
text = "Py"                      # 1 to 3 characters
color = "#3776AB"
```

### Grammar

A `syntax.sublime-syntax` in the package folder (Sublime Text format, version 1; grammars using Sublime Text 4's `extends` and `.tmLanguage` files are not supported). Without a file, `scope` can use one of the included grammars, syntect's bundle: Python, Go, Java, JavaScript, HTML, Markdown, YAML, Shell, Batch… (about 75). Without a grammar, the language is recognised and has a badge, but no colours.

### symbols.toml

Regular expression rules that find the language's symbols. The outline, Ctrl+Shift+O, Ctrl+T and go to definition work with them as with the built-in ones. A package's rules win over the built-in extractor of the same id; a package that replaces a built-in language without bringing a `symbols.toml` keeps its symbols.

```toml
format_version = 1

[[symbol]]
kind = "class"
pattern = '^class (?<name>\w+)'

[[symbol]]
kind = "function"
pattern = '^[ \t]*def (?<name>\w+)'
end = '^\S'        # optional: where the symbol ends
strip = "[]"       # optional: characters taken out of the name
```

- **`kind`:** the symbol kind, one from the table below.
- **`pattern`:** the `regex` crate's syntax (no backreferences or lookaround), with `^` and `$` at every line (also with CRLF line ends); `(?i)` ignores case. The name is the `name` group, or the first group, or the whole match. A compiled pattern over 1 MB is rejected.
- **`end`** (optional): the symbol runs to where `end` matches or to the next symbol, whichever comes first. Without `end`, to the next symbol.
- **`strip`** (optional): the characters taken out of the name.
- Rule symbols do not nest: they all sit at the outline's first level.

### Symbol kinds

| `kind` | Badge | Used by |
|---|---|---|
| `namespace` | N | C# |
| `class`, `struct`, `record`, `enum`, `delegate` | C, S, R, E, D | C# |
| `interface` | I | C# |
| `enum_member` | e | C# |
| `method`, `constructor` | M | C# |
| `property`, `field`, `indexer` | P, F, [] | C# |
| `event` | Ev | C# |
| `selector`, `at_rule`, `keyframes`, `variable` | #, @, K, V | CSS |
| `key` | K | JSON |
| `element` | <> | XML |
| `procedure`, `function`, `view`, `table`, `trigger`, `type` | P, F, V, T, Tr, T | T-SQL |
| `directive` | @ | Razor |

Any package can use any of them.
