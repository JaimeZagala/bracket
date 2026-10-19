**English** · [Español](../../es/decisions/0003-symbols.md)

# 0003 — Symbol extraction with tree-sitter

- **Status:** accepted on 2026-10-19 when Phase 3 was approved. The plan already expected tree-sitter; before approving it, its cost was measured and the alternative of our own extractors was considered.
- **Measurement context:** Rust 1.99.0, a minimal program with the workspace's release profile (`lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, `strip = true`), 2026-10-19, machine A.

## Requirements

- Symbols for C#, CSS, JSON, XML/XSIG, T-SQL and Razor for the outline, Ctrl+Shift+O, Ctrl+T, go to definition and references by name (approximate, without a compiler).
- XPath and JSON path at the caret.
- Without blocking the interface: parsing on worker threads, incremental while editing.

## Options measured

| Option | Size added | Notes |
|---|---|---|
| tree-sitter 0.27.1 + `tree-sitter-c-sharp` 0.23.5 | +5.4 MB | The C# grammar is almost all of the cost (the parser's tables) |
| … + `tree-sitter-css` 0.25.0, `tree-sitter-json` 0.24.8 and `tree-sitter-xml` 0.7.0 | +0.17 MB more | +5.59 MB in total |
| Our own extractors (a scanner per language) | Tens of KB (estimated) | They only recognize declarations; the C# one is the big piece of work |

With the executable at 9.1 MB (0.2.0), tree-sitter takes it to about 14.7 MB against the 15 MB budget.

## Decision

**tree-sitter** for C#, CSS, JSON and XML; regex rules for T-SQL and for external language packages (`symbols.toml`); Razor combines its directives with the C# grammar for `@code` blocks.

- A full syntax tree that tolerates errors: a half-written file is still parsed well.
- Incremental parsing while editing.
- Grammars maintained by the community (MIT); symbols come from our own queries on the tree.
- The same tree serves the XPath and JSON path and, later, folding (Phase 4).

## Consequences

- The executable ends close to the size budget. Phase 3 step 1 measures whether it can be reduced without losing speed, and Phase 5 will have to fit in what is left; if it does not, the budget will be reviewed with data.
- The grammars are C code compiled with MSVC (already needed to build Bracket): builds and CI take a little longer and that code is outside the workspace's `unsafe` rules.
- The grammars may lag behind new C# versions; symbols that depend on new syntax are checked in the test corpus.
- The grammars cannot be loaded without recompiling: external packages keep using regex rules.
- Extractors sit behind a common interface (file → symbols), so a language could change method without touching the index, the palette or navigation.
