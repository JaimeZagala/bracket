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
- **Symbols:** tree-sitter for C#, CSS, JSON and XML. T-SQL through regex rules (CREATE/ALTER of PROCEDURE, FUNCTION, VIEW, TABLE, TRIGGER; `GO` as separator). Razor in three layers: TextMate highlighting; directive symbols (`@page`, `@inject`, `@using`, `@inherits`) and `@code` blocks passed to the C# extractor; navigation from `<MyComponent />` to `MyComponent.razor` by file name.
- **Projects and search:** ignore, regex + grep-searcher, nucleo-matcher.
- **Other:** notify, similar, encoding_rs + chardetng, serde + toml, anyhow + thiserror + tracing, crossbeam-channel.
- **Distribution:** winreg, winresource or embed-resource, ureq, semver, sha2 + ed25519-dalek, self-replace, zip. Inno Setup installer. Publishing with GitHub Actions on a Windows runner.
- **Testing:** cargo test, proptest, criterion.

The crate structure is described in [architecture.md](architecture.md).

## Languages

C#, Razor, CSS, T-SQL, JSON and XML are built into the executable. An optional `languages/` folder adds languages or overrides a built-in one. Lookup order: `%APPDATA%\Bracket\languages`, `languages/` next to the `.exe`, then built-in. In portable mode only the folder next to the `.exe` counts.

```text
languages/<id>/language.toml          format_version, name, extensions, comments, indentation, pairs
languages/<id>/syntax.sublime-syntax  (or .tmLanguage)
languages/<id>/symbols.toml           optional: regex symbol rules (name, pattern, kind)
```

`format_version` lets the editor reject an incompatible package with a clear message. `.xsig`, `.xml`, `.xsd`, `.config` and `.csproj` map to XML. Origin and license of every grammar go in `THIRD_PARTY.en.md`.

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

## Way of working

1. Before writing code: plan, open questions and risks; wait for approval.
2. Small steps: each one builds, has tests and gets its own commit.
3. Before closing a step: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
4. For a bug: root cause first, plus a test that reproduces it. After two attempts without progress, stop and propose options.
5. Do not invent APIs: check docs.rs or the dependency's source code.
