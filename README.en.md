**English** · [Español](README.md)

# Bracket

A lightweight native code editor for Windows 10/11 (x64), written in Rust. Built for people who want more than Notepad++ (projects, symbols, folder search) without the weight of VS Code, and who still use a full IDE for in-depth work.

> **Status: Phase 0 (prototype).** There is no released version and nothing to install yet. This README describes the goal; the [Roadmap](#roadmap) section says what exists today.

**This is not Brackets.** Bracket (singular) is unrelated to Brackets, Adobe's discontinued editor, and to its successors Brackets Continued and Phoenix Code.

## Who it is for

Windows developers who open many loose files and project folders (C#, Razor, CSS, T-SQL, JSON, XML) and want an editor that starts instantly, navigates symbols and searches the project, with no extensions or setup.

## Planned features

- Tabs, recoverable session and automatic reload when a file changes on disk.
- Atomic save that preserves encoding, BOM and line endings. XML/XSIG files are saved byte for byte (signed files are not invalidated).
- Folder as a project: side tree that honours `.gitignore`, Ctrl+P (file), Ctrl+T (project symbol), Ctrl+Shift+O (file symbol), command palette, regex find and replace in a file and across a folder.
- Symbol navigation without a compiler: outline, go to definition and find references by name (approximate).
- Highlighting and symbols for C#, Razor, CSS, T-SQL, JSON and XML/XSIG, built into the executable. Other languages are added by copying a folder, without recompiling.
- Per-user installer (no administrator rights) or portable zip. "Open with" and file associations.
- Updates only when the user accepts them, with a stable channel and an optional preview channel.
- Interface in Spanish (default) and English.
- No telemetry. The only network connection is the update check, which can be turned off.

## Performance goals

These are goals, not results: they will be measured and published together with the method, version and test machine.

| Measure | Goal |
|---|---|
| Executable size | < 15 MB |
| Cold start until typing is possible | < 150 ms |
| Idle RAM with 10 tabs | < 80 MB |
| 100 MB file | visible in < 1 s without blocking the interface |
| Ctrl+P in a 50,000-file project | < 50 ms per keystroke |

Known limits: no extension system and no built-in LSP (optional LSP planned for 1.1.0).

## Building from source

Requirements: Windows 10/11 x64, [rustup](https://rustup.rs) and the Visual Studio Build Tools with the C++ workload (MSVC and Windows SDK). The Rust version is installed automatically from `rust-toolchain.toml`.

```powershell
git clone https://github.com/JaimeZagala/bracket.git
cd bracket
cargo build --release -p bracket-app
# Executable: target\release\bracket.exe
./scripts/check.ps1   # formatting, clippy, tests and documentation
```

## Roadmap

| Phase | Version | Scope | Status |
|---|---|---|---|
| 0 | — | Foundations and editor prototype | In progress |
| 1 | 0.1.0 | Base editor: tabs, session, encodings, highlighting, themes, configuration, languages | Pending |
| 2 | 0.2.0 | Projects: folder tree, Ctrl+P, palette, find and replace in folder | Pending |
| 3 | 0.3.0 | Symbols and navigation | Pending |
| 4 | 0.4.0 | Advanced editing: multi-cursor, folding, diff, large files | Pending |
| 5 | 0.5.0 → 1.0.0 | Installer, portable zip, updater, signing and publishing | Pending |
| 6 | 1.1.0 | Optional LSP | Pending |

Details: [docs/en/plan.md](docs/en/plan.md). Architecture: [docs/en/architecture.md](docs/en/architecture.md). Changes: [CHANGELOG.en.md](CHANGELOG.en.md).

## License

Bracket is dual-licensed, at your option, under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE). Third-party components: [THIRD_PARTY.en.md](THIRD_PARTY.en.md).
