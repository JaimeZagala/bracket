**English** · [Español](../es/user-guide.md)

# User guide

Bracket 0.2.0 (Phase 2, projects). This guide describes what already works and grows with each phase.

## Opening and saving

- **File** menu: New (Ctrl+N), Open (Ctrl+O), Save (Ctrl+S), Save As (Ctrl+Shift+S) and Exit. Open and Save As use the Windows dialogs.
- Files can also be opened by passing them as arguments (`bracket.exe a.cs b.json`, see [Command line](#command-line)) or by dropping them on the window.
- Saving an untitled document opens Save As. Saving is atomic and keeps the file's BOM and line endings. Saving over an existing file keeps its attributes (hidden, for example), permissions and creation time. The window title marks unsaved changes with ●; if you undo back to the saved text, the mark goes away.
- If there are unsaved changes, Bracket asks before closing the tab. It does not ask when exiting: the changes stay in the session (see below).
- Bracket detects the encoding when opening (UTF-8, UTF-16 and legacy encodings such as windows-1252) and saves in the same one, byte for byte. If a file cannot be written back exactly the same, it opens read-only and the status bar says so.

## Projects

A folder opens as a project with **File → Open Folder**, with `bracket.exe C:\path\folder`, by dropping it on the window or by handing it to a Bracket that is already open. The window title shows the project's name, the Open dialog starts in it and the session remembers it when Bracket closes. **File → Close Folder** closes it; open tabs stay.

**File → Recent** shows the last files (up to 15) and folders (up to 10) that were opened, newest first; the ones that no longer exist are not shown. A recent folder opens as the project. "Clear the list" empties it. The list is kept in `recent.toml`, next to `config.toml`.

With a project open, the **project panel** on the left shows the folder tree: folders first and then files, by name, with the language badge on files that have one. Clicking a folder opens or closes it. Clicking a file shows it in the **preview tab**, with its title in italics: the next file looked at this way takes its place, so the tab bar does not fill up. The tab becomes a normal one when the document is edited, when the tab is double-clicked or when the file is double-clicked in the tree. Drag the right edge to change the width.

- **Ctrl+B** hides or shows the panel (also **View → Project panel**).
- **Ctrl+Shift+E** moves the keyboard to the tree: ↑ and ↓ move the selection (also Home, End, PgUp and PgDn), → opens the folder or goes into it, ← closes it or goes up to the folder that contains it, Enter opens the file in a normal tab (or opens and closes the folder), Space shows it in the preview without leaving the tree, and Esc goes back to the editor.
- The session remembers the width, whether the panel is shown and which folders were open.
- The tree updates by itself when other programs create, delete or rename files and folders in the project (a `git checkout`, a build, File Explorer), also when `.gitignore` changes.
- The **context menu** (right click on an entry, or on the project name at the top) has New file, New folder, Rename (F2), Delete (Del), Copy path, Copy relative path and Show in File Explorer. On a file, new entries go in its folder.
- The name is typed in the tree itself: Enter or a click elsewhere confirms it and Esc cancels it. When renaming, the name is selected without its extension, as in File Explorer. A name Windows does not allow (with `<>:"/\|?*`, ending with a dot or a space, or reserved like `CON` or `NUL`) is reported in the status bar. If something with that name already exists, it is not replaced.
- **Delete** moves the entry to the Recycle Bin, after confirming, so it can be recovered. A new file opens in a tab; tabs of a renamed file or folder move to the new name.

### Go to file (Ctrl+P)

**Ctrl+P** (or **Go → Go to file…**) opens a box at the top to go to a file by typing part of its path:

- A few letters in order are enough, they need not be together: `prgcs` finds `src\Program.cs`. Letters at the start of a word, letters in a row and letters in the file name weigh more. Folders can be included with `\` or `/` (`models/user`); spaces do not count.
- The matching letters are highlighted. ↑ and ↓ choose (also PgUp and PgDn), Enter opens and Esc closes.
- With no text it shows the recently opened files.
- `Program.cs:42` opens the file at line 42 and `Program.cs:42:7` also at column 7. `:120` goes to line 120 of the active document.
- It searches the project's files, with the same exclusions as the tree. The index is prepared when the folder opens (a tenth of a second with 50,000 files) and updates by itself. Without a project, it searches the recent files.
- At the foot, how many files the project has and, with the symbol index on and ready, how many symbols (extracted in the background from files up to 1 MB, and kept up to date even when another program changes them).

### Command palette, Go to line and Go to symbol

- **Ctrl+Shift+P** or **F1** (or **View → Command palette…**) opens the same box with `>` in front: it lists the available commands, like "File: Save", with their shortcut on the right, filtered by typing as in Ctrl+P. Hovering over a command shows its identifier (for example `file.save`).
- **Ctrl+G** (or **Go → Go to line…**) opens the box with `:` in front: type the line, or `line:column`, and Enter. Typing `:` in Ctrl+P works too.
- **Ctrl+Shift+O** (or **Go → Go to symbol in file…**) opens the box with `@` in front: it lists the active document's symbols (the same as the outline) in order, with their kind, the symbol they are declared in and their line. Typing filters by name as in Ctrl+P; Enter selects the symbol's name in the editor.
- **Ctrl+T** (or **Go → Go to symbol in project…**) opens the box with `#` in front: it searches by name, like Ctrl+P, among the symbols of every file in the project. Each result shows its kind, the symbol it is declared in and, on the right, its file and line (the full path on hover); Enter opens the file with the caret at the symbol. It needs an open folder, and says so while the symbols are being prepared. *Experimental and off by default: turn it on with `project_navigation = true` in `[experimental]` (see [Configuration](#configuration)).*
- The menus show the same commands and shortcuts; the ones that cannot be used at the moment (for example, Close folder without a project) are disabled.

### Find in file (Ctrl+F)

**Ctrl+F** (or **Edit → Find…**) opens a bar above the editor. If text is selected on a single line, that text is searched for.

- While you type, the first match from the caret is selected and all of them are marked in the text. The counter says which one is selected and how many there are ("3 of 120"); from 100,000 on it shows "100000+".
- **Enter** or **F3** goes to the next one and **Shift+Enter** or **Shift+F3** to the previous one; at the end it wraps around to the start. F3 also works with the bar closed: it opens it with the last search.
- **Aa** matches case, **ab** finds whole words only and **.\*** uses regular expressions (the `regex` crate's syntax: `\d+`, `^using`, `(foo|bar)`). Alt+C, Alt+W and Alt+R turn them on from the keyboard. An invalid expression is shown in red, with the reason when hovering over it.
- **Esc** closes the bar and goes back to the editor.
- **Ctrl+H** (or **Edit → Replace…**) adds the replace row. **Replace** (or Enter in that field) changes the selected match and selects the next one; if the selection is not a match, it first selects the next one so you see what will change. **Replace all** (Ctrl+Alt+Enter) changes all of them at once; Ctrl+Z undoes it in a single step.
- With **.\*** on, the replacement can use the expression's groups: `$1`, `$2`… or `${name}` for `(?<name>…)`, and `$$` for a `$`. `\n` inserts a line break (the document's: CRLF or LF), `\t` a tab and `\\` a backslash. Without regular expressions, the text is inserted as it is.
- In files over 1 MB the search runs in the background and the bar shows "Searching…" meanwhile.

### Find in folder (Ctrl+Shift+F)

**Ctrl+Shift+F** (or **Edit → Find in folder…**) opens the **Search** tab of the side panel; **Explorer** goes back to the tree (also Ctrl+Shift+E).

- Type what you are looking for: the search starts when you stop typing (or press Enter) and is cancelled if you change anything. **Aa**, **ab** and **.\*** work as in Ctrl+F.
- **Files to include** and **Files to exclude** take comma-separated patterns, as in `.gitignore`: `*.cs, *.razor`, `src/**`, `bin/`. Files the project excludes (`.gitignore`, `.ignore` and `[project] exclude`) are always left out.
- Results appear as they are found, grouped by file (clicking the file collapses it) with the line number and the line of each match. Clicking a match opens the file there.
- Each file is read with its encoding, as when it is opened. Binary files and files over 10 MB are skipped, and at most 20,000 results are shown; the summary says so.

### Replace in folder (Ctrl+Shift+H)

**Ctrl+Shift+H** (or **Edit → Replace in folder…**, or **Replace** next to the options of the Search tab) adds the replace row.

- Each match shows what goes, struck out, and the new text after it. With **.\*** you can use `$1`, `${name}` and `\n`, as in Ctrl+H.
- The check boxes of each file and each match leave out what you do not want to change.
- **Replace all** asks for confirmation before touching anything. In files open in a tab, the change goes into the document: it stays unsaved and Ctrl+Z undoes it. The other files are saved at once, with the same encoding, BOM and line breaks, and cannot be undone from Bracket.
- Read-only files, files that cannot be saved back identical in their encoding and files that changed since the search (also a tab whose unsaved changes no longer match) are left alone. The status bar sums up how many replacements were made and how many files were skipped, and the search runs again.

### Outline

The **Outline** tab of the side panel shows the active document's symbols as a tree: namespaces, classes, methods, properties… in C#; rules, `@media` and variables in CSS; keys in JSON; elements (with their `id` or `name`) in XML and XSIG; procedures, functions, views, tables, triggers and types in T-SQL; and in Razor, the directives and the members of `@code` blocks.

- Clicking a symbol goes to it; the arrow of those with children collapses them.
- The symbol where the caret is stays marked and the outline scrolls to show it.
- It updates by itself, a moment after typing stops.
- Without a project, **View → Outline** opens the panel with this tab only; Ctrl+B hides it.
- Documents over 10 MB have no outline.

### Go to definition and history

- **F12**, **Ctrl+click** on a name or **Go → Go to definition** look for where the name at the caret is declared (with Ctrl held, the pointer is a hand). First in the same file, then in the files of its folder or of its namespace, and then in the rest of the project. It only looks in related languages: from C# or Razor, in C# and Razor; from JSON or XML, only in the file itself; from the rest, in the files of their language. The name has to match exactly, except in T-SQL, where case does not matter. If there is none, the status bar says so instead of jumping elsewhere. If there is one, it goes straight there; if there are several, they are chosen in the palette (typing goes on searching like Ctrl+T).
- In Razor, on `<MyComponent />` or `</MyComponent>` it opens `MyComponent.razor`.
- Without a project, in a file outside it or without project navigation turned on (it is experimental and off by default; see [Configuration](#configuration)), it looks only among the document's own symbols.
- It is approximate: it compares names, without compiling. A namespace is declared with its full name (`Demo.Billing`), so one of its parts leads nowhere. A method with overloads, or two classes with the same name in different namespaces, give several results.
- **Alt+←** (**Go → Back**) goes back to where the caret was before the last jump (go to definition, to a symbol, to a line, to a file from Ctrl+P, to a search result or to a symbol in the outline) and **Alt+→** (**Go → Forward**) undoes going back. If the tab was closed, the file opens again. The last 50 places are remembered.
- **Shift+F12** (**Go → Find references**) searches the whole project for the name at the caret as a whole word and shows the results in the **Search** tab. It matches case except in T-SQL and in text without a language; it clears the filter of files to include (the exclusions stay). It also finds comments, strings and other symbols with the same name: it compares names, without compiling. *Experimental and off by default: turn it on with `project_navigation = true` in `[experimental]` (see [Configuration](#configuration)).*

## Tabs

- Each file opens in its own tab. If the active tab is an empty new document, the file takes its place; if the file is already open, its tab is shown.
- The colored badge shows the language from the extension: `C#` (C#), `@` (Razor), `#` (CSS), `SQL` (T-SQL), `{}` (JSON), `<>` (XML, also `.xsig`, `.config`, `.csproj`, `.resx`...) and `RS` (Rust). Hovering over a tab shows the full path.
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

C#, Razor (`.razor`, `.cshtml`), CSS, T-SQL (`.sql`), JSON, XML (also `.xsig`, `.config`, `.csproj`...) and Rust (`.rs`) files are shown in colour, matching their tab's badge ([what Bracket does with each language](languages.md)). Highlighting is computed in the background: when a long file opens, what is in view is coloured first.

- Files over 16 MB and lines over 20,000 characters (minified JSON, for example) are shown without colours, so the editor stays fast.
- In Razor, directives (`@page`, `@using`...), `@* *@` comments, expressions (`@Model.Name`, `@(a + b)`) and the C# in `@{ }` and `@code { }` are coloured. HTML inside code blocks is not recognised yet.
- Colours depend on the editor theme (see [Appearance and themes](#appearance-and-themes)).

## Appearance and themes

In the **View** menu:

- **Appearance:** light interface, dark, or following Windows (the default). With "Follow Windows", Bracket changes on its own when Windows' mode changes.
- **Editor theme:** the colours of the editor's text and background. "Automatic" (the default) uses InspiredGitHub with the light appearance and base16 Ocean dark with the dark one; a fixed one can also be chosen: InspiredGitHub, Solarized (light or dark), base16-ocean (light or dark), base16-eighties or base16-mocha.

Changes apply at once and are saved in `config.toml`.

## External languages

Languages can be added, or a built-in one replaced, with a package: one folder per language inside `languages` (`%APPDATA%\Bracket\languages\`, or next to `bracket.exe` in portable mode) with a `language.toml`, and optionally a `.sublime-syntax` grammar for highlighting and a `symbols.toml` with rules for the symbols. The full format, and what Bracket does with each built-in language, is in [Languages](languages.md). Changes to packages apply when Bracket restarts.

## Changes on disk

Bracket watches open files. If another program changes them while they are open:

- **No changes in Bracket:** the file reloads by itself and the status bar says so. The caret stays on the same line and column, and Undo goes back to the previous text.
- **Unsaved changes:** a notice appears above the text. "Reload from disk" replaces your text with the one on disk (Undo brings it back); "Keep my changes" leaves it as it is and the next save overwrites the file.
- **Deleted or moved:** a notice says so and the tab stays modified, so the text is not lost: saving creates the file again. If the file comes back with the same contents, the tab is no longer modified.

## Status bar

From left to right:

- **Messages:** the result of the last action (saved, reloaded...), configuration or language package warnings, or a hint for opening files.
- **Position:** `Ln 12, Col 5` (the column counts tabs up to their stop). With selected text it adds how many characters (`(25 selected)`), and with several selections it shows how many (`3 selections`).
- **Path:** in XML and XSIG, the XPath of the element at the caret (`/Invoice/Lines/Line[3]/Amount`, with the position among siblings of the same name when there are several, and `/@id` inside an attribute); in JSON, the path of the value (`$.items[2].name`, or `$['other key']` if the key is not an identifier). It appears on the left a moment after the caret moves; a click copies it to the clipboard. Documents over 10 MB do not have it.
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

[experimental]
project_navigation = false # project symbol index: Ctrl+T, go to definition in other files and Shift+F12
```

`[experimental]` gathers features that are not used every day and stay off so Bracket keeps light. With `project_navigation = true`, opening a folder extracts the symbols of all its files in the background (with 2,000 C# files, about a second and some 9 MB of memory) for Ctrl+T, go to definition across the project and Shift+F12.

Projects also leave out the files excluded by `.gitignore` and `.ignore` (even if the folder is not a Git repository) and the `.git`, `.svn` and `.hg` folders; hidden files do appear.

The language can also be changed from the button on the right of the status bar: the change is immediate and saved in `config.toml`. Other changes made by hand take effect when Bracket restarts. If a value is invalid, its default is used and the status bar says so; the rest of the file is honoured. When Bracket saves the configuration, it writes all its keys with their current value, keeps your comments, the order and any keys it does not know, and it never overwrites a file with syntax errors.

## Fonts

Bracket ships no fonts: it uses the ones installed on Windows. The editor uses the `font_family` font (Consolas by default; any installed font works, including fonts installed only for your user). If it is not installed, Consolas, Cascadia Mono or Courier New is used and the status bar says so. The interface uses Segoe UI.

If the text contains characters the font lacks, Bracket automatically loads a system font that has them: Segoe UI Symbol for symbols, Microsoft YaHei (or Yu Gothic, MS Gothic) for Chinese and Japanese, and Malgun Gothic for Korean. These fonts are only loaded when such text appears on screen, because they take a fair amount of memory (about 40 MB for Microsoft YaHei).

## Portable mode

If there is a file named `portable.toml` next to `bracket.exe` (it can be empty), Bracket runs in portable mode: the configuration and other data are stored in that same folder, and nothing is written to `%APPDATA%` or the Windows registry. This way it can be carried on a USB stick or kept in a synced folder.
