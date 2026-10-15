**English** · [Español](../es/architecture.md)

# Architecture

Status: Phase 0. This document describes the agreed structure; each section states what is implemented.

## Workspace crates

| Crate | Responsibility | Depends on (workspace) | Status |
|---|---|---|---|
| `bracket-core` | Buffer (ropey), multiple selections, transactional undo/redo, cursor movement, encoding, line endings, document model. No UI and no Windows APIs. | — | Empty |
| `bracket-syntax` | Loading of built-in and external languages, incremental line-based highlighting with cache, themes. | `bracket-core` (planned) | Empty |
| `bracket-index` | Project walking, symbol extraction (tree-sitter and regex), incremental index, fuzzy search, references by name. | `bracket-core` (planned) | Empty |
| `bracket-search` | Cancellable regex find and replace in a file and across a folder, with streamed results. | `bracket-core` (planned) | Empty |
| `bracket-session` | TOML configuration, portable mode, recoverable session, recent files, file watching, command registry. | `bracket-core` (planned) | Empty |
| `bracket-update` | Release channel checks, download, checksum and signature verification, apply and rollback. | — | Empty |
| `bracket-ui` | egui interface: editor view, tabs, project tree, palette, status bar. | `bracket-core` | Empty |
| `bracket-app` | `bracket.exe` binary: startup, single instance, arguments, Windows manifest, HKCU registration. | `bracket-ui`, `bracket-update` | Empty |

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

## Decisions

Architecture decisions are recorded in [decisions/](decisions/).
