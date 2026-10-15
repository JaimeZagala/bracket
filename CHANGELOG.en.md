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
