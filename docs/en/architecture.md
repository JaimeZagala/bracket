**English** · [Español](../es/architecture.md)

# Architecture

Status: Phase 1 complete (0.1.0). This document describes the agreed structure; each section states what is implemented.

## Workspace crates

| Crate | Responsibility | Depends on (workspace) | Status |
|---|---|---|---|
| `bracket-core` | Buffer (ropey), multiple selections, transactional undo/redo, cursor movement, encoding, line endings, document model. No UI and no Windows APIs. | — | Phase 1: buffer, selections, history with exact saved state, movement, encodings, line endings, document with atomic saving, reloading and disk stamps |
| `bracket-syntax` | Loading of built-in and external languages, incremental line-based highlighting with cache, themes. | `bracket-core` | Phase 1: language registry (the six built-in languages, extensions, badge and grammar) and highlighting with syntect |
| `bracket-index` | Project walking, symbol extraction (tree-sitter and regex), incremental index, fuzzy search, references by name. | `bracket-core` (planned) | Empty |
| `bracket-search` | Cancellable regex find and replace in a file and across a folder, with streamed results. | `bracket-core` (planned) | Empty |
| `bracket-session` | TOML configuration, portable mode, recoverable session, recent files, file watching, command registry. | `bracket-core` | Phase 1: paths (portable mode or `%APPDATA%`), TOML configuration, recoverable session and file watching |
| `bracket-update` | Release channel checks, download, checksum and signature verification, apply and rollback. | — | Empty |
| `bracket-ui` | egui interface: editor view, tabs, project tree, palette, status bar. | `bracket-core`, `bracket-session`, `bracket-syntax` | Phase 1: tabs, File menu, status bar, system fonts, text catalogs |
| `bracket-app` | `bracket.exe` binary: startup, single instance, arguments, Windows manifest, HKCU registration. | `bracket-ui`, `bracket-session`, `bracket-syntax`, `bracket-update` | Phase 0: startup and file argument. Phase 1: resolves paths, loads the configuration and language packages, single instance and command line |

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
- **File replacement:** the core defines the `FileReplacer` trait (renaming by default). `bracket-app` uses `ReplaceFileW` (`file_replace.rs`), which keeps attributes, ACLs and creation time. Together with `console.rs` (see below), they are the only `unsafe` blocks in the project, each with its `SAFETY` comment.
- **Document:** buffer + path + encoding + line ending + saved version. Atomic save: temporary file in the same folder, `sync_all` and rename over the target.
- **Encodings (`encoding.rs`):** detection by BOM, UTF-16 without BOM (zero-byte pattern), valid UTF-8 and otherwise `chardetng`. A file is only editable if encoding its text again reproduces exactly its bytes; otherwise it is read-only. When saving, a character that does not exist in the encoding makes the save fail (encoding_rs would replace it with `&#NNNN;`). UTF-16 is encoded by hand because encoding_rs only writes UTF-8 for it. Valid UTF-8 is decoded without copying the bytes.

## Files and shortcuts (Phase 1)

- `dialogs.rs` wraps `rfd`'s native dialogs with Bracket's window as parent: they are modal and run on the UI thread, as in any editor.
- Application shortcuts (Ctrl+N/O/S, Ctrl+Shift+S) are consumed by the app before the editor paints, so they work wherever the focus is. Saving an untitled document goes through Save As.

## Tabs (Phase 1)

- `tabs.rs` defines `Tab` (document, view, language, loading and saving in progress) and draws the tab bar. The app keeps a `Vec<Tab>` that is never empty plus the active index; the status bar, the title and the commands act on the active tab.
- Each tab has an id that is unique for the run, and its `EditorView` derives its egui `Id` from it, so focus and scrolling are independent per tab.
- Loading and saving run per tab on worker threads; the result reaches its tab even if it is no longer active. If loading a file opened in a new tab fails, the tab is closed.
- Each tab's language comes from `LanguageRegistry::for_path` (`bracket-syntax`) when opening and on Save As. Badges are colored text; the registry allows adding SVG icons later.

## Session (Phase 1)

- `bracket-session` (`session.rs`) defines the format: `session.toml` with the tabs (path, forced encoding, primary selection, first visible line) plus one UTF-8 file per modified document in `backups/`. Saving writes the backups first and then `session.toml`, both atomically, so the list never points to a half-written backup.
- Only backups the current session no longer uses and that are "owned" are deleted: those restored at startup or written by this run. Names carry the startup time and the PID, so one run never overwrites another run's backups. A damaged session is set aside as `session.damaged.toml` and its backups are left alone.
- `bracket-ui` (`session.rs`) restores each tab on a thread: it reads the file and applies the saved text on top as a single undoable step (`Document::restore_unsaved`), so undo goes back to the text on disk. `SessionKeeper` saves on a thread 2 s after the last change (detected with a hash of the state) and rewrites a backup only if its text changed. When the window closes it saves on the UI thread; if that fails, it asks about the unsaved changes again.
- Lazy loading: at startup only the active tab and those with unsaved text are restored (so closing and "Save all" still see their changes). The others stay as `Tab::unloaded` with their session entry, which is saved again as it is until the tab is first activated.
- `bracket-app` reads the session at startup (30 µs with 10 tabs).
- The window (position, size, maximized) is saved by eframe with its `persistence` feature in `window.ron`, in the data folder (portable mode too). eframe moves it back onto a connected monitor if its own is gone and accounts for each screen's scaling. Only the window is saved, not egui's widget state. eframe applies `centered` after the saved position, so it is only turned on while `window.ron` does not exist yet. Cost: 547 KB (serde and ron).
- While the measurement probes are on, the session is neither read nor written, so measurements neither depend on the user's session nor change it.

## Windows resources (Phase 1)

- `crates/bracket-app/build.rs` generates a resource script and compiles it with the Windows SDK's `rc.exe` (`embed-resource`); if that is not possible, the build fails rather than producing an exe without a manifest.
- **Manifest** (`res/bracket.manifest`): per-monitor DPI (PerMonitorV2), long paths, Windows 10/11 as a supported OS, common controls v6 (the current look of dialogs) and running without asking for administrator rights.
- **Version:** `VERSIONINFO` with the version from `Cargo.toml`, shown in the file properties and Task Manager.
- **Icon:** drawn by code in `build/icon.rs` (no binary files in the repository): a rounded square with `[|]`, sampled 4×4 per pixel. The `.ico` holds 9 sizes (16 to 256; bitmaps up to 48 and PNG from 64, 32 KB in total). A 64×64 RGBA copy is the window icon (`ViewportBuilder::with_icon`).

## Command line and single instance (Phase 1)

- `bracket-app` (`cli.rs`) parses the arguments; `OpenRequest::parse` (`bracket-session`) splits `file:line:column` (a trailing `:number` is always a position, because Windows file names cannot contain `:` except after the drive).
- `--help` and `--version` are handled before the window is created. Release builds are GUI programs without a console of their own: `console.rs` uses the redirected output if there is one, attaches to the parent process's console (`AttachConsole`) or, if there is none, shows a message. It is the project's second `unsafe` block (`GetStdHandle` and `AttachConsole`); the standard library has no safe alternative.
- Single instance without new dependencies (`instance.rs`): the first instance keeps `instance.lock` in the data folder open without sharing it (`share_mode(0)`); Windows closes it when the process ends, even after a crash, so a stale lock cannot remain. Another instance writes its files (absolute paths) to `requests/` and waits up to 3 s for the request to disappear; if nobody takes it, it withdraws it and opens its own window.
- The main instance watches `requests/` with the same `FileWatcher` as the open files, reads any pending requests at startup and discards those older than 30 s.
- Everything that does not paint (thread polling, inbox, changes on disk, session, close requests) runs in `App::logic`, which eframe calls before each `ui` and also while the window is minimized or hidden. That way requests are handled while minimized and the session is saved when closing from the taskbar.
- eframe stores a tiny window at (0, 0) if it saves while minimized (on exit or in the autosave). `App::save` puts back the last value stored with the window not minimized; the autosave runs every 5 s (eframe only writes when something changed).
- Opening maximized showed a black frame: eframe paints the first one while the window is hidden and Windows drops it when maximizing the window as it appears. eframe's `window_builder` hook creates the window not maximized but with the maximized size eframe stored, and the app maximizes it on the second frame, already on screen.
- With the measurement probes there is no single instance: every run is on its own.

## Highlighting (Phase 1)

- Grammars: syntect's (from sublimehq/Packages) for C#, CSS, JSON and XML, and two of our own in `crates/bracket-syntax/grammars/` for Razor and T-SQL (`.sublime-syntax` version 1, what syntect understands; Sublime Text 4 grammars with `extends` do not work). Razor puts the Razor constructs on top of HTML with `with_prototype`, like the "HTML (ASP)" grammar, and embeds the C# grammar's `code_block_in` context.
- `build.rs` compiles the grammars into one bundle embedded in the exe (344 KB compressed) and checks that no reference is left unresolved. syntect's whole bundle is kept (75 languages): its grammars refer to each other by position and cannot be taken out one by one. The bundle is loaded the first time it is needed, on the highlighter thread (≈1 ms; syntect compiles each regex when it is first used).
- Engine (`highlight.rs`, no egui): it keeps the parser state every 128 lines. After an edit, the checkpoints above the first changed line remain right. If the number of lines did not change, the ones below are kept: when parsing reaches one with the same state (and below the last changed line), everything below is right again.
- Each highlighted tab has its own thread (`HighlightWorker`). The UI asks for the visible lines plus a page above and below. The thread only serves the newest request, gives up a long one if another arrives (keeping the work done) and returns colour spans per line. Until the new result arrives, lines whose text did not change use the previous one.
- The view builds one egui `LayoutJob` per line with those colours (egui caches the galleys).

## Themes (Phase 1)

- Two independent settings: the interface appearance (`Appearance`: system, light or dark, applied with `egui::Context::set_theme`, which also changes Windows' title bar) and the editor theme (`auto` or the name of one of syntect's seven themes).
- `follow_theme` (in `App::logic`) works out the effective theme: the configured one or, when automatic, the light or dark one from `ctx.theme()`, so it follows Windows live. When it changes, the highlighting threads are dropped and created again with the new theme.
- `bracket-syntax` provides the editor-area colours (`ThemeColors`: background, text, caret, selection, gutter and line numbers) without depending on egui; the interface uses its own where the theme defines none (`EditorStyle`).

## Language packages (Phase 1)

- `bracket-syntax` (`packages.rs`) reads `languages/<id>/language.toml` in the folders of `Paths::language_dirs` (priority: `%APPDATA%\Bracket\languages`, `languages` next to the exe; in portable mode only the latter). The first folder with an id wins; a package with a built-in language's id replaces it, and for extensions packages come before built-in languages.
- The manifests are read in `main` before the window is created (they are few and small), so tabs have a language from the first frame. Of the grammar, only its top-level `scope:` is read.
- External grammars are parsed on the highlighter thread the first time something is highlighted, together with the built-in ones (`into_builder` + `build`), so they can include or replace built-in grammars (for one scope, the last one added wins). An `include` of a grammar that does not exist stays as plain text. Errors are collected and the interface adds them to its warnings.
- No package problem stops Bracket from starting: the package is skipped with a translated warning (`PackageWarning`).

## Changes on disk (Phase 1)

- `bracket-session` (`watch.rs`) wraps `notify` (`ReadDirectoryChangesW`): it watches exactly the paths of the loaded tabs (updated when they change) and only reports which path changed.
- `bracket-ui` (`disk.rs`) keeps each tab's file stamp (modification time and size, `DiskStamp`), taken when reading it and after each save. A report from the watcher schedules a check 200 ms later (editors write, rename and touch the file in a row); the check runs on a thread and only reads the file if the stamp changed. While Bracket is saving the tab the check waits, so its own saves are not taken for outside changes.
- Reports are matched to tabs by file name: the watcher may spell the folder differently (long names instead of 8.3 ones, other case) and a false match only costs checking a stamp.
- Reloading uses `Document::reload`: it replaces the text in one undoable step keeping each caret's line and column. A deleted file leaves the document modified (`Document::mark_missing`), so closing asks and the session keeps its text.

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
- **Startup (all phases):** 120–135 ms without load in 0.1.0; about 60 ms are OpenGL initialization on Windows and no eframe setting reduces it (see [measurements.md](measurements.md)). About 15–25 ms of margin remain: new work must stay off the first frame's path, and `check.ps1 -Perf` watches for regressions.
- **Licenses in the distribution (Phase 5):** the BSD-3-Clause notice of encoding_rs (and those of the other dependencies) must ship with the installer and the zip.
- **Saving from the unsaved-changes dialog:** it runs on the UI thread and blocks it while it lasts (usually milliseconds). Hard links to the original file are not kept when saving (inherent to atomic saving).
- **Text units (Phase 4):** grapheme clusters are not treated as a unit and wide (CJK) characters count as one column.
- **Large files (Phase 4):** very long lines are laid out whole, and egui's `ScrollArea` loses `f32` precision beyond ~1M lines (a custom line-based scroll will be needed). A 100 MB file takes about 200 MB of memory.
- **Highlighting speed:** about 110 µs per line with the pure Rust regex engine; what is visible is coloured first, but racing through a long file top to bottom leaves colours pending for a moment.
- **Grammar bundle:** syntect's whole bundle is included (344 KB compressed) because its grammars cannot be separated; copying only the needed sources into the repository would save about 300 KB.
- **Razor:** HTML inside code blocks (`@{ <p>…</p> }`) is not recognised and `@if` blocks are shown as text.

## Decisions

Architecture decisions are recorded in [decisions/](decisions/).
