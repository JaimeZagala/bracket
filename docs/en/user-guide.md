**English** · [Español](../es/user-guide.md)

# User guide

Bracket is under development (Phase 1). This guide describes what already works and grows with each phase.

## Opening and saving

- Open a file by passing it as an argument (`bracket.exe file.txt`) or by dropping it on the window.
- Ctrl+S saves. Saving is atomic and keeps the file's BOM and line endings.
- For now only UTF-8 files can be edited; other files open read-only.

## Language

The interface is available in Spanish (default) and English. To switch, click the language name on the right of the status bar and pick another one: the change is immediate and remembered in the configuration.

## Configuration

The configuration lives in `config.toml`:

- **Installed:** `%APPDATA%\Bracket\config.toml`.
- **Portable:** `config.toml` next to `bracket.exe` (see below).

The file is optional: if it does not exist the defaults are used, and Bracket does not create it until a setting is changed from the interface. It can be edited by hand:

```toml
[interface]
language = "es"      # interface language: "es" (Spanish) or "en" (English)

[editor]
font_family = "Consolas" # editor font: any installed font
font_size = 14.0     # font size, 8 to 48
tab_width = 4        # tab width, 1 to 16

[files]
line_ending = "crlf" # line ending of new files: "crlf", "lf" or "cr"
```

The language can also be changed from the button on the right of the status bar: the change is immediate and saved in `config.toml`. Other changes made by hand take effect when Bracket restarts. If a value is invalid, its default is used and the status bar says so; the rest of the file is honoured. When Bracket saves the configuration, it writes all its keys with their current value, keeps your comments, the order and any keys it does not know, and it never overwrites a file with syntax errors.

## Fonts

Bracket ships no fonts: it uses the ones installed on Windows. The editor uses the `font_family` font (Consolas by default; any installed font works, including fonts installed only for your user). If it is not installed, Consolas, Cascadia Mono or Courier New is used and the status bar says so. The interface uses Segoe UI.

If the text contains characters the font lacks, Bracket automatically loads a system font that has them: Segoe UI Symbol for symbols, Microsoft YaHei (or Yu Gothic, MS Gothic) for Chinese and Japanese, and Malgun Gothic for Korean. These fonts are only loaded when such text appears on screen, because they take a fair amount of memory (about 40 MB for Microsoft YaHei).

## Portable mode

If there is a file named `portable.toml` next to `bracket.exe` (it can be empty), Bracket runs in portable mode: the configuration and other data are stored in that same folder, and nothing is written to `%APPDATA%` or the Windows registry. This way it can be carried on a USB stick or kept in a synced folder.
