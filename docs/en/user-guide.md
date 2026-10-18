**English** · [Español](../es/user-guide.md)

# User guide

Bracket 0.1.0 (Phase 1, base editor). This guide describes what already works and grows with each phase.

## Opening and saving

- **File** menu: New (Ctrl+N), Open (Ctrl+O), Save (Ctrl+S), Save As (Ctrl+Shift+S) and Exit. Open and Save As use the Windows dialogs.
- Files can also be opened by passing them as arguments (`bracket.exe a.cs b.json`, see [Command line](#command-line)) or by dropping them on the window.
- Saving an untitled document opens Save As. Saving is atomic and keeps the file's BOM and line endings. Saving over an existing file keeps its attributes (hidden, for example), permissions and creation time. The window title marks unsaved changes with ●; if you undo back to the saved text, the mark goes away.
- If there are unsaved changes, Bracket asks before closing the tab. It does not ask when exiting: the changes stay in the session (see below).
- Bracket detects the encoding when opening (UTF-8, UTF-16 and legacy encodings such as windows-1252) and saves in the same one, byte for byte. If a file cannot be written back exactly the same, it opens read-only and the status bar says so.

## Projects

A folder opens as a project with **File → Open Folder**, with `bracket.exe C:\path\folder`, by dropping it on the window or by handing it to a Bracket that is already open. The window title shows the project's name, the Open dialog starts in it and the session remembers it when Bracket closes. **File → Close Folder** closes it; open tabs stay.

**File → Recent** shows the last files (up to 15) and folders (up to 10) that were opened, newest first; the ones that no longer exist are not shown. A recent folder opens as the project. "Clear the list" empties it. The list is kept in `recent.toml`, next to `config.toml`.

The folder tree panel, Ctrl+P and project search come in the next steps of Phase 2.

## Tabs

- Each file opens in its own tab. If the active tab is an empty new document, the file takes its place; if the file is already open, its tab is shown.
- The colored badge shows the language from the extension: `C#` (C#), `@` (Razor), `#` (CSS), `SQL` (T-SQL), `{}` (JSON) and `<>` (XML, also `.xsig`, `.config`, `.csproj`, `.resx`...). Hovering over a tab shows the full path.
- A dot instead of the × means unsaved changes.
- **Close:** Ctrl+W, the × or a middle click. Closing the last one leaves a new document.
- **Switch tabs:** Ctrl+Tab and Ctrl+Shift+Tab, or Ctrl+PageDown and Ctrl+PageUp.
- New documents are numbered: "Untitled 1", "Untitled 2"...

## Command line

```text
bracket [options] [file[:line[:column]]]...
```

- `file:line` opens the file with the caret on that line; `file:line:column`, also on that column (it counts characters: a tab is one). Useful to open a compiler's errors directly, such as `bracket Program.cs:42:13`.
- `-h`, `--help` shows the help in the interface language; `-v`, `--version`, the version. They are printed to the console Bracket was started from, or shown in a message if it was started from Explorer or a shortcut.
- `--` means what follows are files even if they start with `-`.

**One window.** If Bracket is already open, starting it again opens the files in that window and brings it to the front, even if it is minimized. Portable Bracket and installed Bracket are separate instances, because each has its own data folder.

## Session

When Bracket closes it saves the session: the open tabs, which one was active, the caret and scroll position of each, and the text of documents with unsaved changes, untitled ones included. When it opens again everything is as it was and those documents are still marked as modified: nothing is written to your files until you save.

- On startup only the active tab is read (and those with unsaved changes); the others are read the first time you activate them, so Bracket starts quickly even if the session has big files.
- The session is also saved a few seconds after each change, so if Bracket or Windows closes suddenly, at most those seconds are lost.
- If a file no longer exists and had no changes, its tab is not opened. If it had unsaved changes, it opens with them and saving creates it again.
- It is stored in `session.toml`, and the unsaved text in the `backups` folder, next to `config.toml` (in `%APPDATA%\Bracket`, or next to the exe in portable mode). If `session.toml` were damaged, Bracket sets it aside as `session.damaged.toml`, starts without a session and deletes nothing in `backups`.
- If the session could not be written when exiting, Bracket asks what to do with the unsaved changes, as before.
- The window comes back to the position and size it had, and maximized if it was (kept in `window.ron`, next to `session.toml`). If its monitor is no longer connected, it opens on a visible one. The first time it opens centered.

## Syntax highlighting

C#, Razor (`.razor`, `.cshtml`), CSS, T-SQL (`.sql`), JSON and XML files (also `.xsig`, `.config`, `.csproj`...) are shown in colour, matching their tab's badge. Highlighting is computed in the background: when a long file opens, what is in view is coloured first.

- Files over 16 MB and lines over 20,000 characters (minified JSON, for example) are shown without colours, so the editor stays fast.
- In Razor, directives (`@page`, `@using`...), `@* *@` comments, expressions (`@Model.Name`, `@(a + b)`) and the C# in `@{ }` and `@code { }` are coloured. HTML inside code blocks is not recognised yet.
- Colours depend on the editor theme (see [Appearance and themes](#appearance-and-themes)).

## Appearance and themes

In the **View** menu:

- **Appearance:** light interface, dark, or following Windows (the default). With "Follow Windows", Bracket changes on its own when Windows' mode changes.
- **Editor theme:** the colours of the editor's text and background. "Automatic" (the default) uses InspiredGitHub with the light appearance and base16 Ocean dark with the dark one; a fixed one can also be chosen: InspiredGitHub, Solarized (light or dark), base16-ocean (light or dark), base16-eighties or base16-mocha.

Changes apply at once and are saved in `config.toml`.

## External languages

Languages can be added, or a built-in one replaced, with one folder per language inside `languages`:

- **Installed:** `%APPDATA%\Bracket\languages\` and, with lower priority, `languages\` next to `bracket.exe`.
- **Portable:** only `languages\` next to `bracket.exe`.

The folder name is the language id; if it matches a built-in one (`csharp`, `razor`, `css`, `tsql`, `json`, `xml`), it replaces it. Inside goes `language.toml`:

```toml
format_version = 1             # required: format version
name = "Python"                # required
extensions = ["py", "pyw"]     # required
scope = "source.python"        # a grammar Bracket already includes (optional)
tab_width = 4                  # tab width for this language (optional)
line_comment = "#"             # optional, for future features
block_comment = ['"""', '"""'] # optional, for future features
pairs = [["(", ")"], ["[", "]"]] # optional, for future features

[badge]                        # optional: by default the first two letters on grey
text = "Py"                    # 1 to 3 characters
color = "#3776AB"
```

- **Grammar:** a `syntax.sublime-syntax` in the same folder (Sublime Text format, version 1; grammars using Sublime Text 4's `extends` and `.tmLanguage` files are not supported). Without a file, `scope` can use any of the included grammars, which are syntect's bundle: Python, Rust, Go, Java, JavaScript, HTML, Markdown, YAML, Shell, Batch... (about 75). Without a grammar, the language is recognised and has a badge, but no colours.
- If a package has a problem (a missing key, a `format_version` from another version, a grammar with errors), it is skipped and the status bar says so. Changes to packages apply when Bracket restarts.

## Changes on disk

Bracket watches open files. If another program changes them while they are open:

- **No changes in Bracket:** the file reloads by itself and the status bar says so. The caret stays on the same line and column, and Undo goes back to the previous text.
- **Unsaved changes:** a notice appears above the text. "Reload from disk" replaces your text with the one on disk (Undo brings it back); "Keep my changes" leaves it as it is and the next save overwrites the file.
- **Deleted or moved:** a notice says so and the tab stays modified, so the text is not lost: saving creates the file again. If the file comes back with the same contents, the tab is no longer modified.

## Status bar

From left to right:

- **Messages:** the result of the last action (saved, reloaded...), configuration or language package warnings, or a hint for opening files.
- **Position:** `Ln 12, Col 5` (the column counts tabs up to their stop). With selected text it adds how many characters (`(25 selected)`), and with several selections it shows how many (`3 selections`).
- **Line ending** and **encoding:** see below. "Read-only" appears next to the encoding when the file cannot be saved without altering its bytes.
- **Document language:** the one detected from the extension. Click it to pick another one for this tab, or "Plain Text"; the session remembers the choice. Saving under another name detects it from the extension again.
- **Interface language.**

## Encoding and line endings

The status bar shows the encoding (for example `UTF-8`, `UTF-8 BOM`, `UTF-16 LE BOM` or `windows-1252`) and the line ending (`CRLF`, `LF` or `CR`). Click them to:

- **Reopen with encoding:** read the file from disk again with the chosen encoding (in case detection was wrong). Save or discard your changes first.
- **Save with encoding:** the next save uses that encoding. If the text has characters that do not exist in it, nothing is saved and the status bar says which one.
- **Convert to CRLF / LF / CR:** change every line break in the document in a single undoable step, and use that line ending for new lines.

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
appearance = "system" # appearance: "system" (follow Windows), "light" or "dark"

[editor]
font_family = "Consolas" # editor font: any installed font
theme = "auto"       # editor theme: "auto" or a name, e.g. "Solarized (dark)"
font_size = 14.0     # font size, 8 to 48
tab_width = 4        # tab width, 1 to 16

[files]
line_ending = "crlf" # line ending of new files: "crlf", "lf" or "cr"
encoding = "utf-8"   # encoding of new files: "utf-8", "utf-8-bom" or another one (e.g. "windows-1252")

[project]
exclude = ["bin/", "obj/"] # left out of every project, like .gitignore lines
```

Projects also leave out the files excluded by `.gitignore` and `.ignore` (even if the folder is not a Git repository) and the `.git`, `.svn` and `.hg` folders; hidden files do appear.

The language can also be changed from the button on the right of the status bar: the change is immediate and saved in `config.toml`. Other changes made by hand take effect when Bracket restarts. If a value is invalid, its default is used and the status bar says so; the rest of the file is honoured. When Bracket saves the configuration, it writes all its keys with their current value, keeps your comments, the order and any keys it does not know, and it never overwrites a file with syntax errors.

## Fonts

Bracket ships no fonts: it uses the ones installed on Windows. The editor uses the `font_family` font (Consolas by default; any installed font works, including fonts installed only for your user). If it is not installed, Consolas, Cascadia Mono or Courier New is used and the status bar says so. The interface uses Segoe UI.

If the text contains characters the font lacks, Bracket automatically loads a system font that has them: Segoe UI Symbol for symbols, Microsoft YaHei (or Yu Gothic, MS Gothic) for Chinese and Japanese, and Malgun Gothic for Korean. These fonts are only loaded when such text appears on screen, because they take a fair amount of memory (about 40 MB for Microsoft YaHei).

## Portable mode

If there is a file named `portable.toml` next to `bracket.exe` (it can be empty), Bracket runs in portable mode: the configuration and other data are stored in that same folder, and nothing is written to `%APPDATA%` or the Windows registry. This way it can be carried on a USB stick or kept in a synced folder.
