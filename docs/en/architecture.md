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
| `bracket-session` | TOML configuration, portable mode, recoverable session, recent files, file watching, command registry. | `bracket-core` (planned) | Empty |
| `bracket-update` | Release channel checks, download, checksum and signature verification, apply and rollback. | — | Empty |
| `bracket-ui` | egui interface: editor view, tabs, project tree, palette, status bar. | `bracket-core` | Phase 0: prototype (editor view, window, minimal status bar, text catalog) |
| `bracket-app` | `bracket.exe` binary: startup, single instance, arguments, Windows manifest, HKCU registration. | `bracket-ui`, `bracket-update` | Phase 0: startup and file argument |

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
- **Document:** buffer + path + BOM + line ending + saved version. Atomic save: temporary file in the same folder, `sync_all` and rename over the target. Phase 0 supports UTF-8 only; a file that is not UTF-8 opens read-only and cannot be saved.

## Interface prototype (`bracket-ui`, Phase 0)

- `EditorView` paints only the visible lines inside an egui `ScrollArea` and turns keyboard, mouse, clipboard and IME input into core commands. The IME composition is shown at the caret and only enters the buffer when committed.
- Known limit: egui uses `f32` coordinates. With 1M lines (about 17 million pixels) precision is about 2 px at the end of the document; beyond a few million lines a custom line-based scroll will be needed.
- Visible texts live in `i18n.rs` (Spanish and English). The final translation approach is decided in Phase 1.

## Decisions

Architecture decisions are recorded in [decisions/](decisions/).
