**English** · [Español](CHANGELOG.md)

# Changelog

All notable changes to Bracket are recorded here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses [Semantic Versioning](https://semver.org/). Before 1.0.0, minor versions may contain breaking changes; each version lists them.

## [Unreleased]

### Added

- Outline of the active document: an "Outline" tab in the side panel (also without a project, with View → Outline) with the symbols as a tree, collapsible, a click to go to each one and the one at the caret marked. Symbols for C#, CSS, JSON and XML with tree-sitter; for T-SQL by rules; for Razor, its directives and the members of `@code`. Language packages can bring a `symbols.toml` with regex rules. The outline is worked out again on a thread, incrementally, when typing stops.
- Go to symbol in file: Ctrl+Shift+O (or `@` in the palette) lists the active document's symbols, with their kind, where they are declared and their line, filtered by typing.
- Go to symbol in project: Ctrl+T (or `#` in the palette) searches by name among the symbols of every file in the project, with their kind, where they are declared and their file, and opens the file at the symbol.
- Go to definition: F12 or Ctrl+click on a name looks for where it is declared, first in the same file, then in its folder or namespace and then in the rest of the project; if there are several, they are chosen in the palette. In Razor, `<MyComponent />` opens `MyComponent.razor`. Without a project, it looks in the document itself. Navigation history: Alt+← and Alt+→ go back to where the caret was before each jump.
- Find references: Shift+F12 searches the whole project for the name at the caret as a whole word, with the results in the Search tab; it matches case except in T-SQL and in text without a language. It is approximate: it compares names, without compiling.
- Project symbol index: when a folder is opened, the symbols of all its files (up to 1 MB each) are extracted in the background, in parallel, and kept up to date as files change, also when another program modifies them. Tabs with unsaved changes lend their text. The foot of Ctrl+P shows how many symbols there are. It is the base for Ctrl+T and go to definition.

## [0.2.0] - 2026-10-19

Projects (Phase 2). Not published: only the local `v0.2.0` tag exists.

### Added

- Open a folder as a project: File → Open Folder, `bracket <folder>`, dropping the folder on the window or handing it to an open Bracket. The window title shows the project and the session remembers it; File → Close Folder closes it without touching the tabs.
- Recent items: File → Recent lists the last files (15) and folders (10) opened, without the ones that no longer exist, and can clear the list. They are kept in `recent.toml`.
- Project walk (`bracket-index`): lists the files of a folder following `.gitignore` (also outside a Git repository), `.ignore` and the new `[project] exclude` key in `config.toml`; hidden files are included and `.git`, `.svn` and `.hg` never are. It runs on several threads and can be cancelled: 110 ms with 50,000 files.
- Project panel: a folder tree on the left with the same exclusion rules. Each folder is listed on a thread when it is opened; language badge on files; resizable; Ctrl+B hides it and Ctrl+Shift+E moves the keyboard to the tree (arrows, Home/End, PgUp/PgDn, Enter, Esc). The session remembers its width, whether it is shown and the open folders.
- Preview tab: clicking a file in the tree opens it in a provisional tab, with its title in italics, that the next file looked at replaces. Editing it, double-clicking it or the file, or opening the file with Enter turns it into a normal tab. The session remembers it.
- Project watching: the tree follows the files and folders that other programs create, delete or rename in the project, in under a second; a change to `.gitignore` or `.ignore` applies the rules again. Changes are grouped (200 ms without changes, or 1 s at most) and writing to a file does not count. The file index is updated folder by folder (10 ms at the root of 50,000 files, against 109 ms to walk it all), ready for Ctrl+P.
- Tree actions (context menu): new file, new folder, rename (F2), delete to the Recycle Bin (Del, with confirmation), copy the path or the relative path and show in File Explorer. Names are typed in the tree itself and checked against Windows' rules; an existing file is never replaced. A new file opens; tabs of a renamed file or folder follow the new name.
- Go to file (Ctrl+P, also in the Go menu): our own fuzzy search over the project index, with the matching letters highlighted; with no text it shows the recent files. `file:line` and `file:line:column` open at that position and `:line` goes to a line of the active document. The index is built on its own thread when the project opens and follows the watcher's changes; with 50,000 files each keystroke takes 7 ms at most (budget: 50 ms).
- Command palette (Ctrl+Shift+P or F1): searches the available commands by name, with their shortcut next to them. Go to line (Ctrl+G) takes `line` and `line:column`. Commands have a single registry (id, translated text, shortcuts, menu and availability) that menus, shortcuts and the palette come from; a command that is not available is shown disabled in the menu. New Go menu with Go to file, Go to line, Go to the project tree and the next and previous tabs.
- Find in file (Ctrl+F, Edit menu): a bar above the editor with match case, whole word and regular expression (Alt+C, Alt+W, Alt+R), the matches marked in the text, a "3 of 120" counter, Enter/Shift+Enter and F3/Shift+F3 to go to the next or previous one, and Esc to close. While typing, the first match from the caret is selected. Documents over 1 MB are searched on a thread (50 MB: about 35 ms).
- Replace in file (Ctrl+H, Edit menu): a second row in the find bar. "Replace" (Enter) changes the selected match and moves to the next one; if the selection is not a match, it selects one first. "Replace all" (Ctrl+Alt+Enter) changes every match in one undo step. With regular expressions, `$1` and `${name}` insert groups and `\n` inserts the document's line break.
- Find in folder (Ctrl+Shift+F, Edit menu): a "Search" tab in the side panel, next to the explorer, with the same options as Ctrl+F and filters for files to include and exclude. It searches as you type (or on Enter) on a cancellable thread and results arrive file by file, grouped and collapsible, with the line of each match; a click opens the file there. Each file is decoded as the editor would open it (windows-1252, UTF-16…); binary files and files over 10 MB are skipped. At most 20,000 results are shown.
- Replace in folder (Ctrl+Shift+H, Edit menu): a replace row in the Search tab with a preview (what goes struck out and the new text after it) and check boxes per file and per match. "Replace all" asks for confirmation; in open tabs the change goes into the document (undoable and left unsaved) and closed files are rewritten with an atomic save keeping their encoding, BOM and line breaks. Read-only files, files with an inexact encoding and files that changed since the search are skipped; a summary goes to the status bar at the end.
- Rust as a built-in language (`.rs`, badge `RS`), highlighted with syntect's grammar that was already in the executable, so its size does not change. Single quotes are not closed in pairs, because of lifetimes (`'a`).

## [0.1.0] - 2026-10-18

First version of the base editor (Phase 1). Not published: only the local `v0.1.0` tag exists.

### Added

- Cargo workspace with eight crates (`bracket-core`, `bracket-syntax`, `bracket-index`, `bracket-search`, `bracket-session`, `bracket-update`, `bracket-ui`, `bracket-app`), pinned toolchain and optimized release profile.
- Initial documentation in Spanish and English, plus a language pair check.
- Dual license: MIT or Apache-2.0.
- `bracket.exe` main window (egui/eframe with the OpenGL backend) and a script that measures size, startup and memory.
- Editor prototype: open a UTF-8 file (argument or drag and drop), edit with multiple selections, undo/redo, clipboard, IME, tabs and atomic save that preserves BOM and line endings. Files that are not UTF-8 open read-only.
- Configuration in `config.toml` (interface language, font size, tab width and line ending of new files), with status bar warnings for invalid values.
- Portable mode: with `portable.toml` next to the exe, all data is stored in that folder.
- User guide in `docs/en/user-guide.md`.
- Language selector in the status bar: switches the interface at once and saves it in the configuration. Interface texts live in catalogs, one per language (`es.toml` and `en.toml`).
- System fonts: the editor font is configurable (`font_family`, Consolas by default) and fallback fonts for symbols, Chinese, Japanese and Korean are loaded on demand. The executable no longer bundles fonts (1.36 MB smaller).
- Encodings: UTF-8, UTF-16 (with or without BOM) and legacy encodings (windows-1252, Shift_JIS, GBK...) are detected, and files are saved in the same encoding. If a file cannot be kept byte for byte, it opens read-only. Status bar menus reopen or save with another encoding and convert line endings (in a single undo step). Ctrl+S works even while a menu has the focus.
- File menu with New (Ctrl+N), Open (Ctrl+O), Save (Ctrl+S), Save As (Ctrl+Shift+S) and Exit, using the native Windows dialogs. Saving an untitled document opens Save As.
- Saving over an existing file keeps its attributes, permissions and creation time (`ReplaceFileW`). Undoing back to the saved text leaves the document unmodified.
- Tabs: each file opens in its own tab, with a language badge (C#, Razor, CSS, T-SQL, JSON, XML) and a dot while it has unsaved changes. Ctrl+W or a middle click closes, Ctrl+Tab and Ctrl+PageUp/PageDown switch tabs. Opening a file that is already open goes to its tab. Several files can be passed as arguments.
- Recoverable session: when Bracket closes it keeps the open tabs, the caret and scroll position, and the text of documents with unsaved changes (untitled ones too); when it opens again everything is back as it was, with those changes still waiting to be saved. The session is also saved a few seconds after each change, so an unexpected exit loses at most those seconds. Exiting no longer asks about unsaved changes.
- The window remembers its position, its size and whether it was maximized; if its monitor is no longer connected, it opens on a visible one. The first time it opens centered (it could end up partly below the taskbar).
- Reload on disk changes: if another program changes an open file that has no changes in Bracket, it reloads by itself (undo goes back to the previous text) and the status bar says so. If it had unsaved changes, a notice offers to reload from disk (undo brings the changes back) or keep them. If the file is deleted or moved, its tab stays modified so the text is not lost.
- Single instance: if Bracket is already open, `bracket.exe file` opens the file in that window and brings it to the front (even if it was minimized). Each data folder (installation or portable copy) has its own instance.
- Command line: `file:line` and `file:line:column` open the file with the caret at that position; `--help` and `--version` (the help in the interface language) are printed to the console Bracket was started from, or shown in a message when there is no console.
- Fixed: closing the window while minimized stored a tiny window at (0, 0) and did not save the session.
- Fixed: opening Bracket with the window maximized showed a black window for a moment before the interface. Also, "Loading…" only appears if loading takes over 200 ms, so it does not flash for ordinary files.
- Restoring the session only reads the active tab (and those with unsaved changes); the others are read when activated. With two big files in inactive tabs, startup memory goes from 226 MB to 47 MB.
- Syntax highlighting for C#, Razor, CSS, T-SQL, JSON and XML (syntect, on one thread per document). Typing does not make the other lines flicker and, when an edit does not change the code's structure, only what is needed is computed again. Files over 16 MB and lines over 20,000 characters are shown without colours.
- External language packages: a `languages/<id>/` folder with `language.toml` (`format_version = 1`) adds a language or replaces a built-in one. It can bring its grammar (`syntax.sublime-syntax`) or use one Bracket already includes (`scope = "source.python"`), and set its badge and tab width. A package with problems is skipped and the status bar says so.
- Themes: View menu with the interface appearance (follow Windows, light or dark) and the editor colour theme (automatic, following the appearance, or one of the seven included: InspiredGitHub, Solarized light and dark, base16 Ocean light and dark, Eighties and Mocha). They apply at once and are saved in the configuration (`appearance` and `theme`).
- Full status bar, on the right: position (or number of selections) with the selected characters, line ending, encoding, document language and interface language; messages on the left. The language can be changed by hand for the tab (plain text too) and the session remembers it.
- Bracket's own icon (`[|]` on blue) in the executable, the window and the taskbar, and version information in the file properties. Windows manifest: per-monitor DPI from startup, long paths, Windows 10/11 and the current look of system dialogs.
