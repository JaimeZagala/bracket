**English** · [Español](../es/plan.md)

# Project plan

Reference document for Bracket's scope, constraints and phases. Any change of scope is reflected here in both languages.

## Goal

A native code editor for Windows 10/11 x64, shipped as a single `.exe`, halfway between Notepad++ and VS Code. It is not a full IDE. It must:

- Open, edit and save files with tabs, a recoverable session and automatic reload when files change on disk.
- Open folders as projects: side panel with preview, Ctrl+P (file), Ctrl+T (project symbol), Ctrl+Shift+O (file symbol), command palette, regex find and replace in a file and across a folder.
- Navigate symbols: outline, go to definition and references by name (approximate, no compiler).
- Highlight and navigate C#, Razor, CSS, T-SQL, JSON and XML/XSIG, built into the executable. New languages are added by copying a folder, without recompiling.
- Install with a per-user installer or run from a portable zip, with "Open with", file associations and registration as a default editor candidate.
- Detect new versions and update only when the user says so, with a stable channel (default) and an optional preview channel.
- Offer interface and documentation in Spanish (default) and English.

## Performance budgets

Measured, not assumed. If a budget cannot be met, it is reported with data and alternatives.

| Measure | Budget |
|---|---|
| Release executable (with built-in languages and updater) | < 15 MB |
| Cold start until typing is possible | < 150 ms |
| Idle RAM with 10 tabs | < 80 MB |
| 100 MB file | content visible in < 1 s without blocking the interface |
| Ctrl+P in a 50,000-file project | < 50 ms per keystroke |

## Constraints

1. Stable Rust, target `x86_64-pc-windows-msvc`. No nightly.
2. The core (everything except the interface and Windows integration) does not depend on the GUI framework or on Windows APIs.
3. No telemetry. The only network connection is the update check: optional, at most once a day, without identifying data, and nothing is downloaded or installed without confirmation. The editor never executes what it opens.
4. No data loss: atomic save (temporary file + rename), exact preservation of encoding, BOM and line endings. XML/XSIG files get no automatic clean-up: bytes are saved as they are.
5. The UI thread only paints and handles events. Indexing, searching, folder loading, heavy highlighting and networking run on cancellable worker threads.
6. Every new dependency is justified: maintenance, license and measured impact on size and startup. Versions and APIs are checked with cargo and docs.rs.
7. No `unsafe` unless proven necessary and documented with a `SAFETY` comment.
8. No phase starts without the author's approval. No refactoring outside the task's scope.
9. Public repository: no company or customer data, no secrets. Personal Git identity configured at repository level. The diff is reviewed before every push.
10. Nothing is created or published on GitHub without the author's explicit confirmation.

## Stack

- **Interface:** egui + eframe (glow backend). Decision accepted in Phase 0 after measuring it against iced ([decisions/0001-ui.md](decisions/0001-ui.md)). The UI sits behind a thin layer.
- **Buffer:** ropey. Multiple selections from the start. Custom transactional undo/redo (a multi-cursor edit or a bulk replace is one step).
- **Highlighting:** syntect with the `default-fancy` feature (no C dependencies), `.sublime-syntax`/`.tmLanguage` grammars.
- **Symbols:** tree-sitter for C#, CSS, JSON and XML ([decisions/0003-symbols.md](decisions/0003-symbols.md)). T-SQL through regex rules (CREATE/ALTER of PROCEDURE, FUNCTION, VIEW, TABLE, TRIGGER; `GO` as separator). Razor in three layers: TextMate highlighting; directive symbols (`@page`, `@inject`, `@using`, `@inherits`) and `@code` blocks passed to the C# extractor; navigation from `<MyComponent />` to `MyComponent.razor` by file name.
- **Projects and search:** ignore, regex, our own fuzzy matcher, trash (Recycle Bin). (`grep-searcher` was planned; it was not needed, see the architecture.)
- **Other:** notify, similar, encoding_rs + chardetng, toml_edit (configuration: keeps comments when saving and reads key by key; replaces serde + toml since Phase 1), anyhow + thiserror + tracing, crossbeam-channel.
- **Distribution:** winreg, winresource or embed-resource, ureq, semver, sha2 + ed25519-dalek, self-replace, zip. Inno Setup installer. Publishing with GitHub Actions on a Windows runner.
- **Testing:** cargo test, proptest, criterion.

The crate structure is described in [architecture.md](architecture.md).

## Languages

C#, Razor, CSS, T-SQL, JSON and XML are built into the executable. An optional `languages/` folder adds languages or overrides a built-in one. Lookup order: `%APPDATA%\Bracket\languages`, `languages/` next to the `.exe`, then built-in. In portable mode only the folder next to the `.exe` counts.

```text
languages/<id>/language.toml          format_version, name, extensions, badge, comments, indentation, pairs
languages/<id>/syntax.sublime-syntax  optional; or `scope` in language.toml to use an included grammar
languages/<id>/symbols.toml           optional (Phase 3): regex symbol rules (name, pattern, kind)
```

`format_version` lets the editor reject an incompatible package with a clear message. Grammars are `.sublime-syntax` version 1: syntect does not load `.tmLanguage` files or Sublime Text 4 grammars with `extends`. `.xsig`, `.xml`, `.xsd`, `.config` and `.csproj` map to XML. Origin and license of every grammar go in `THIRD_PARTY.en.md`.

## Versioning and releases

- SemVer 2.0.0. Single version in `[workspace.package]`, shown by `--version`, in About and in the `.exe` metadata.
- Before 1.0.0: `0.y.z`, one minor version per phase.
- Before every MAJOR version: `X.0.0-prerelease.N`, then `X.0.0-rc.N` (with a dot), then `X.0.0`. No new features enter during prerelease and RC; work happens on the `release/X.0.0` branch with `fix:`, `docs:` and `chore:` commits only.
- `CHANGELOG.md` and `CHANGELOG.en.md` in Keep a Changelog format.
- Tags `vX.Y.Z` and `vX.Y.Z-rc.1`. Every tag with a hyphen is published as a pre-release. `main` always builds and passes its tests. Conventional Commits.

## Installation, portable mode and updates (Phase 5)

- Same executable in two forms: per-user installer (Inno Setup, in `%LOCALAPPDATA%`, no administrator rights) and portable zip with `portable.toml` next to the `.exe`.
- The installer registers extensions, "Open with", "Edit with Bracket", "Open as project in Bracket", Start menu entry and uninstaller.
- In portable mode, configuration, session and languages live next to the executable, and nothing is written to `%APPDATA%` or the registry.
- "Register with Windows" and "Remove registration" commands (HKCU only).
- Default editor: Bracket registers as a candidate and opens the Windows default apps page. No tricks to bypass that protection.
- Code signing before 1.0.0; without a certificate, the README explains it and checksums are published.
- Updater: stable and preview channels, unauthenticated public GitHub API, at most once a day, optional. It notifies, shows the changelog and only downloads and installs when the user says so.
- Security: every release publishes `SHA256SUMS` and an ed25519 signature made with a private key that is not in the repository or on GitHub. The editor ships the public key and installs nothing it cannot verify. Key rotation procedure documented.
- Applying: the running exe is renamed to `.old`, the new one is put in place and restarted; if it fails to start, the previous one is restored. Configuration is backed up before updating.
- Bug reports: a button opens a GitHub issue with version and system details filled in. Nothing is sent automatically.

## Documentation

README, CHANGELOG, CONTRIBUTING, CODE_OF_CONDUCT, SECURITY and THIRD_PARTY in Spanish (`X.md`) and English (`X.en.md`). `docs/es/` and `docs/en/` with `user-guide.md`, `languages.md`, `architecture.md`, `release.md`, `plan.md` and `decisions/`. License files in English only; `CLAUDE.md` in Spanish only. Any advantage claimed in the README must be backed by reproducible measurements from the repository itself.

## Languages of the project

- Code and file names in English.
- Documentation in Spanish (reference) and English, updated in the same commit. `scripts/check-doc-pairs.ps1` fails if a pair is missing.
- Release notes: Spanish first, then English.
- Interface: the user picks Spanish (default) or English. Visible text lives in translation catalogs, one per language, loaded by a single module. The translation approach is chosen in Phase 1 by measuring its size impact.

## Phases

Every phase ends with a usable version. Exit criteria: tests and clippy green, no benchmark regressions, CHANGELOG, README and affected documentation updated in both languages, and a note in `decisions/` if the architecture changed.

| Phase | Version | Scope |
|---|---|---|
| 0 | — | Foundations and editor prototype. |
| 1 | 0.1.0 | Base editor: tabs, session, encoding and line endings, reload on disk changes, atomic save, highlighting with the six built-in languages and external packages, themes, configuration, status bar, single instance, portable mode, Spanish and English interface with language selector. |
| 2 | 0.2.0 | Projects: folder panel honouring `.gitignore`, preview, recent items, Ctrl+P, command palette, find and replace in a file and across a folder. |
| 3 | 0.3.0 | Symbols and navigation: extractors, incremental index, outline, Ctrl+T, go to definition, references, XPath and JSON path under the cursor, `languages.md`. |
| 4 | 0.4.0 | Advanced editing: full multi-cursor, folding, diff, matching brackets and words, move/duplicate/comment lines, large-file mode. |
| 5 | 0.5.0 → 1.0.0 | Distribution and updates: Spanish and English installer, portable zip, Windows registration, updater with channels, signing, automated publishing, complete documentation and the 1.0.0 prerelease/RC cycle. |
| 6 | 1.1.0 | Optional LSP, disabled by default. |

### Phase 0 in detail

1. Workspace with the eight crates, pinned toolchain, release profile.
2. Initial bilingual documentation and check scripts.
3. eframe hello world as the size, startup and RAM baseline.
4. `bracket-core`: ropey-based buffer with multiple selections.
5. Insertion and deletion at every selection.
6. Transactional undo/redo with property tests.
7. UI-independent cursor movement.
8. Editor view that paints only the visible lines, with scrolling.
9. Keyboard and mouse selection, clipboard, undo/redo.
10. IME and tabs.
11. Open (argument or drag and drop) and save (temporary file + rename; UTF-8 only in this phase).
12. Criterion benchmarks: opening, insertion in the middle, viewport painting.
13. Comparable iced prototype outside the workspace, with measurements.
14. Interface decision (`decisions/0001-ui.md`) and final report.

### Phase 1 in detail

Approved on 2026-10-15 and completed on 2026-10-18 (0.1.0, local `v0.1.0` tag). Steps ordered by dependencies; each one builds, has tests and gets its own commit.

1. Startup: profile eframe (window, OpenGL context, fonts, first frame), try `NativeOptions` settings measuring each change, and add a startup budget check.
2. TOML configuration and portable mode (`bracket-session`): `portable.toml` next to the exe or `%APPDATA%\Bracket`.
3. Translations: choose the library by measuring its size, language selector in the configuration and migration of the texts.
4. System fonts: family configurable by name (Consolas by default, Cascadia Mono as an alternative, or any installed font, including per-user installs) and a system CJK font as fallback. No font is bundled in the exe.
5. Encodings and line endings (encoding_rs + chardetng): detection, read-only if re-encoding does not reproduce the exact bytes, and commands to reopen or save with another encoding and to change the line ending.
6. Files: New, Open, Save and Save As, with native dialogs.
7. Robust saving: `ReplaceFileW` from the Windows layer (keeps attributes and ACLs) and an exact "modified" state when undoing.
8. Tabs: several documents, a prompt when closing with changes, and a language badge on each tab (`C#`, `@`, `#`, `SQL`, `{}`, `<>`). The language registry (id, name, extensions and badge) is created here in `bracket-syntax`.
9. Recoverable session: tabs, positions and unsaved changes.
10. Reload on disk changes (notify).
11. Single instance and command line: `bracket file:line`, `--version` and help in both languages.
12. Highlighting with syntect for the six built-in languages, on a thread, with a per-line cache and lazy grammar loading.
13. External language packages (`languages/`, `format_version`) and mapping of `.xsig`, `.config` and `.csproj` to XML.
14. Themes: editor theme and light or dark interface.
15. Complete status bar: language, encoding, line ending, position and number of selections.
16. `bracket-app` integration: Windows manifest (DPI, long paths), version and icon in the exe metadata.
17. Wrap-up: measurements against the budgets (including RAM with 10 tabs), CHANGELOG, README and documentation, and a local `v0.1.0` tag.

Defaults: new files in UTF-8 without BOM with CRLF; tab width 4. Both configurable.

### Phase 2 in detail

Approved on 2026-10-18. Steps ordered by dependencies; each one builds, has tests and gets its own commit.

1. Cancellation and generations: a cancellation token and generation number shared in `bracket-core` for long tasks (walking, index, searches).
2. Open a folder as a project: menu, command line (`bracket <folder>`), dropping a folder and single instance. The session remembers the project and the title shows the folder.
3. Recent items: files and folders in File → Recent, persistent, without the ones that no longer exist.
4. Project walking in `bracket-index` with `ignore` (`.gitignore`, `.ignore`, hidden files and `[project] exclude`), on threads and cancellable. A generator for a synthetic 50,000-file project and benchmarks.
5. Side panel with the tree: lazy loading per folder on a thread, resizable, Ctrl+B to hide it, keyboard, language badge; width, visibility and open folders in the session.
6. Preview tab: a click opens a provisional tab (italic) that the next click replaces; a double click or an edit keeps it.
7. Project watching: recursive `notify` on the root updates the tree and the index incrementally, debounced.
8. Tree actions: new file, new folder, rename, delete to the Recycle Bin (`trash`), copy path and reveal in Explorer.
9. Ctrl+P: our own fuzzy search over the index, on a thread, with the matching letters highlighted, recent files for an empty query, `file:line` and `:line`. Benchmark against the budget (< 50 ms per keystroke with 50,000 files).
10. Command registry and palette (Ctrl+Shift+P): commands with an id, translated label, shortcut and availability; menus and shortcuts use the registry; fuzzy search; Go to line (Ctrl+G).
11. Find in the file (Ctrl+F): match case, whole word and regex, highlighted matches, a counter, F3/Shift+F3; on a thread for large files.
12. Replace in the file (Ctrl+H): one by one or all (a single undo step), `$1` captures, keeping line endings.
13. Find in the folder (Ctrl+Shift+F): query, options and include/exclude filters; results streamed per file with the context line; click to open at the match; cancellable; binaries and huge files are skipped; each file's encoding is respected.
14. Replace in the folder: a preview with checkboxes; in open tabs the change goes into the buffer (undoable, not saved); closed files are rewritten with atomic saving, keeping encoding, BOM and line endings; read-only or inexactly encoded files are skipped; a summary at the end.
15. Wrap-up: measurements (Ctrl+P with 50,000 files, walking, folder search speed, RAM and startup with a project), documentation, CHANGELOG and a local `v0.2.0` tag.

Completed on 2026-10-19 (0.2.0, local `v0.2.0` tag); results in [measurements](measurements.md).

Planned dependencies, each measured before it goes in: `ignore` and `grep-searcher` + `grep-regex` (Unlicense OR MIT) and `trash` (MIT). In the end `ignore`, `trash` and `regex` went in; `grep-searcher` and `grep-regex` were not needed (step 13). The fuzzy matcher is our own, so as not to add another license (`nucleo-matcher` is MPL-2.0); if it missed the budget, `nucleo-matcher` would be measured.

### Phase 3 in detail

Approved on 2026-10-19, with tree-sitter for symbols ([decisions/0003-symbols.md](decisions/0003-symbols.md)). Steps ordered by dependencies; each one builds, has tests and gets its own commit.

**Size.** Measured with the workspace's release profile, the C# grammar adds 5.4 MB and the CSS, JSON and XML ones another 0.17 MB: the executable will grow from 9.1 to about 14.7 MB, close to the 15 MB budget. Step 1 measures whether it can be reduced without losing speed; Phase 5 (installer and updater) will have to fit in what is left and, if it does not, the budget will be reviewed with data.

1. tree-sitter in `bracket-index`: `tree-sitter` and the C#, CSS, JSON and XML grammars, measured in size and startup, with the options to reduce the size tried, and recorded in `THIRD_PARTY`. Symbol model (name, kind, range and name range in chars, container and nesting) and extraction with our own queries per language; tree-sitter works in bytes and the conversion to chars happens at the edge.
2. C# symbols: namespaces (file-scoped too), classes, structs, interfaces, records, enums, delegates, methods, constructors, properties, fields, events and indexers. A synthetic test corpus (no client code) and a benchmark in MB/s.
3. Symbols for CSS (selectors, nested `@media`/`@supports`, `@keyframes`, `--x` variables), JSON (nested keys, array indices) and XML/XSIG (nested elements, with `id`/`name` when present); with count limits for large files.
4. T-SQL by regex rules (CREATE, ALTER and CREATE OR ALTER of PROCEDURE, FUNCTION, VIEW, TABLE, TRIGGER and TYPE; `GO` as separator), with the same rule engine external packages will use (`symbols.toml`: name, pattern, kind, `format_version`). Razor: `@page`, `@inject`, `@using` and `@inherits` directives, and `@code`/`@functions` blocks parsed with the C# grammar and their offset.
5. Outline of the active document: parsing on a thread, incremental with the tree-sitter tree after each edit (debounced); an "Outline" tab in the side panel, as a tree, click to go there and the symbol at the caret marked.
6. Ctrl+Shift+O: the file's symbols in the palette (`@` prefix), with fuzzy search.
7. Project symbol index: parallel extraction on the index thread (with a size limit per file), incremental with the watcher, which now also reports content changes; tabs with unsaved changes use their buffer. Time and memory with a synthetic C# project.
8. Ctrl+T: the project's symbols in the palette (`#` prefix), with kind, container and file. Benchmark against the Ctrl+P budget (< 50 ms per keystroke) with 200,000 symbols.
9. Go to definition (F12 and Ctrl+click): the word at the caret is looked up in the index (the same file first, then the same folder or namespace); if there are several, a list to choose from. In Razor, `<MyComponent />` opens `MyComponent.razor`. Navigation history: Alt+← and Alt+→.
10. References by name (Shift+F12): a whole-word search in the project (matching case in C#), with the results in the Search tab; approximate, without a compiler.
11. Path at the caret in the status bar, from the tree-sitter tree: XPath in XML/XSIG (`/Invoice/Lines/Line[3]/@id`) and JSON path (`$.items[2].name`); click to copy it; with a size limit.
12. `languages.md` (es/en): built-in languages, extensions, the symbols each one extracts and the package format, `symbols.toml` included.
13. Close: measurements (size against the budget, extraction in MB/s, symbol index of the synthetic project in time and RAM, Ctrl+T, startup with a project), documentation, CHANGELOG and local `v0.3.0` tag.

Planned dependencies, each measured before it goes in: `tree-sitter` 0.27, `tree-sitter-c-sharp` 0.23, `tree-sitter-css` 0.25, `tree-sitter-json` 0.24 and `tree-sitter-xml` 0.7 (all MIT).

## Future improvements without an assigned phase

- Optional SVG icons for languages, defined in `language.toml` as an alternative to the text badge. The package format is designed from Phase 1 to accept them without breaking existing packages.

## Way of working

1. Before writing code: plan, open questions and risks; wait for approval.
2. Small steps: each one builds, has tests and gets its own commit.
3. Before closing a step: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
4. For a bug: root cause first, plus a test that reproduces it. After two attempts without progress, stop and propose options.
5. Do not invent APIs: check docs.rs or the dependency's source code.
