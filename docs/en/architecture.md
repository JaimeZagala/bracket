**English** · [Español](../es/architecture.md)

# Architecture

Status: Phase 0. This document describes the agreed structure; each section states what is implemented.

## Workspace crates

| Crate | Responsibility | Depends on (workspace) | Status |
|---|---|---|---|
| `bracket-core` | Buffer (ropey), multiple selections, transactional undo/redo, cursor movement, encoding, line endings, document model. No UI and no Windows APIs. | — | Phase 0: buffer, selections, transactions, history, movement and UTF-8 document |
| `bracket-syntax` | Loading of built-in and external languages, incremental line-based highlighting with cache, themes. | `bracket-core` (planned) | Empty |
| `bracket-index` | Project walking, symbol extraction (tree-sitter and regex), incremental index, fuzzy search, references by name. | `bracket-core` (planned) | Empty |
| `bracket-search` | Cancellable regex find and replace in a file and across a folder, with streamed results. | `bracket-core` (planned) | Empty |
| `bracket-session` | TOML configuration, portable mode, recoverable session, recent files, file watching, command registry. | `bracket-core` | Phase 1: paths (portable mode or `%APPDATA%`) and TOML configuration |
| `bracket-update` | Release channel checks, download, checksum and signature verification, apply and rollback. | — | Empty |
| `bracket-ui` | egui interface: editor view, tabs, project tree, palette, status bar. | `bracket-core`, `bracket-session` | Phase 0: prototype (editor view, window, minimal status bar, text catalog) |
| `bracket-app` | `bracket.exe` binary: startup, single instance, arguments, Windows manifest, HKCU registration. | `bracket-ui`, `bracket-session`, `bracket-update` | Phase 0: startup and file argument. Phase 1: resolves paths and loads the configuration |

## Dependency rules

```text
bracket-app ──► bracket-ui ──► bracket-syntax ─┐
     │               ├──────► bracket-index  ──┤
     │               ├──────► bracket-search ──┼──► bracket-core
     │               └──────► bracket-session ─┘
     └──► bracket-update   (isolated)
```

- Only `bracket-app` depends on `bracket-ui` and `bracket-update`.
- `bracket-core` and `bracket-update` depend on no other workspace crate.
- Arrows always point down; there are no cycles.
- Nothing below `bracket-ui` uses egui or Windows APIs, so the interface can be replaced without touching the core.
- Inter-crate dependencies are added when they are used, not before.

## Threads and communication

- The UI thread only paints and handles events.
- Indexing, searching, folder loading, heavy highlighting and networking run on worker threads.
- The UI sends commands over channels and receives events. Every long task carries a cancellation token and a generation number; results from an outdated generation are discarded.

In Phase 0, file loading and saving use `std::thread` and `std::sync::mpsc`; cancellation tokens and generations arrive with the first cancellable tasks (Phase 2).

## Core model (`bracket-core`)

- **Positions:** char indices (Unicode scalar values), the native unit of ropey 1.x. A position can never fall inside a UTF-8 character. tree-sitter and regex work in bytes; the conversion (O(log n)) will happen at the boundary.
- **Line breaks:** only LF, CRLF and CR break lines, as in Notepad++ and VS Code (U+2028 does not). Text is never normalized; the document's line ending (the first one found, CRLF for new files) is only used for new text: Enter and paste.
- **CRLF as a unit:** the caret never stops between `\r` and `\n`, and Backspace/Delete remove the whole pair. Grapheme clusters (e.g. `e` + combining accent) are not treated as a unit yet.
- **Selections:** a non-empty, sorted, non-overlapping set with a primary selection. Each selection has an anchor, a head and a goal column for vertical movement.
- **Editing:** every edit is planned per selection against the previous text and applied as a single transaction (a list of changes that store the removed text, so they can be inverted).
- **Undo:** typing characters in a row or deleting char by char forms one step; paste, Enter, deleting a selection and multi-caret edits are steps of their own. Moving the caret ends the group.
- **Modified state:** every undo step has a unique id and the buffer version is the id of the last step applied; undo and redo bring earlier ids back, so returning to the saved text leaves it unmodified.
- **File replacement:** the core defines the `FileReplacer` trait (renaming by default). `bracket-app` uses `ReplaceFileW` (`file_replace.rs`), which keeps attributes, ACLs and creation time. It is the only `unsafe` block in the project, with its `SAFETY` comment.
- **Document:** buffer + path + encoding + line ending + saved version. Atomic save: temporary file in the same folder, `sync_all` and rename over the target.
- **Encodings (`encoding.rs`):** detection by BOM, UTF-16 without BOM (zero-byte pattern), valid UTF-8 and otherwise `chardetng`. A file is only editable if encoding its text again reproduces exactly its bytes; otherwise it is read-only. When saving, a character that does not exist in the encoding makes the save fail (encoding_rs would replace it with `&#NNNN;`). UTF-16 is encoded by hand because encoding_rs only writes UTF-8 for it. Valid UTF-8 is decoded without copying the bytes.

## Files and shortcuts (Phase 1)

- `dialogs.rs` wraps `rfd`'s native dialogs with Bracket's window as parent: they are modal and run on the UI thread, as in any editor.
- Application shortcuts (Ctrl+N/O/S, Ctrl+Shift+S) are consumed by the app before the editor paints, so they work wherever the focus is. Saving an untitled document goes through Save As.

## Fonts (Phase 1)

- `bracket-app` (`system_fonts.rs`) finds installed fonts by family name in the Windows registry (HKCU, then HKLM) and implements the `FontSource` trait.
- `bracket-ui` (`fonts.rs`) decides which families to use: the configured monospace one (or Consolas, Cascadia Mono, Courier New) and Segoe UI for the interface. They are read on a thread that starts at the beginning of `main` and finishes while eframe sets up OpenGL.
- The view reports visible characters without a glyph (it only checks non-ASCII ones, and stops checking a kind once its fallback was requested) and the app loads Segoe UI Symbol, a CJK font or Malgun Gothic on a thread.

## Interface prototype (`bracket-ui`, Phase 0)

- `EditorView` paints only the visible lines inside an egui `ScrollArea` and turns keyboard, mouse, clipboard and IME input into core commands. The IME composition is shown at the caret and only enters the buffer when committed.
- Known limit: egui uses `f32` coordinates. With 1M lines (about 17 million pixels) precision is about 2 px at the end of the document; beyond a few million lines a custom line-based scroll will be needed.
- Visible texts live in `locales/es.toml` and `locales/en.toml` and are requested through methods in `i18n.rs` (see [decisions/0002-i18n.md](decisions/0002-i18n.md)).

## Known technical debt

Still to be solved; each item states the planned phase.

- **Fallback font memory:** egui takes about twice the size of each loaded font; with Japanese or Chinese text on screen, Microsoft YaHei adds about 42 MB. Alternative if it matters: a smaller CJK font with less coverage (MS Gothic, 9 MB).
- **No system fonts:** if no font at all were found (Consolas and Segoe UI ship with Windows), the application would exit without any message.
- **Startup (all phases):** 123–133 ms without load; about 60 ms are OpenGL initialization on Windows and no eframe setting reduces it (see [measurements.md](measurements.md)). About 20 ms of margin remain: new work must stay off the first frame's path, and `check.ps1 -Perf` watches for regressions.
- **Licenses in the distribution (Phase 5):** the BSD-3-Clause notice of encoding_rs (and those of the other dependencies) must ship with the installer and the zip.
- **Saving from the unsaved-changes dialog:** it runs on the UI thread and blocks it while it lasts (usually milliseconds). Hard links to the original file are not kept when saving (inherent to atomic saving).
- **Text units (Phase 4):** grapheme clusters are not treated as a unit and wide (CJK) characters count as one column.
- **Large files (Phase 4):** very long lines are laid out whole, and egui's `ScrollArea` loses `f32` precision beyond ~1M lines (a custom line-based scroll will be needed).

## Decisions

Architecture decisions are recorded in [decisions/](decisions/).
