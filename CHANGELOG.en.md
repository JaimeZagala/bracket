**English** · [Español](CHANGELOG.md)

# Changelog

All notable changes to Bracket are recorded here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses [Semantic Versioning](https://semver.org/). Before 1.0.0, minor versions may contain breaking changes; each version lists them.

## [Unreleased]

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
