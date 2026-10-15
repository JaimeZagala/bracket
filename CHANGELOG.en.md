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
