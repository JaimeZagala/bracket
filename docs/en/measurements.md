**English** · [Español](../es/measurements.md)

# Measurements

Log of the project's performance measurements, with the method to reproduce them. Only what appears here may be quoted as an advantage in the README.

## Method

`scripts/measure.ps1 -Exe <path> -Runs 12` measures:

- **Size:** bytes of the release executable (workspace `release` profile: fat LTO, `codegen-units = 1`, `panic = "abort"`, `strip`).
- **Startup:** the script takes a precise timestamp (`GetSystemTimePreciseAsFileTime`) right before launching the process. With `BRACKET_MEASURE_STARTUP=<file>`, the executable writes its timestamp on entering `main()` and the one when the first frame is already on screen (second `ui()` call), then exits. The script reports *launch → main* and *launch → first frame*; process shutdown is not counted.
- **First run:** reported separately. After a build or a reboot it includes Windows Defender scanning the new executable and cold disk caches (0.4 to 1.5 s in these tests). The median uses the remaining (warm) runs.
- **Idle memory:** after 3 s with the window open. The reference figure is the *private working set* (Task Manager's "Memory" column). The total working set includes the graphics driver's shared DLLs.

Reference: launching and closing an empty Rust executable takes about 9 ms on this machine.

## Test machine

Machine A: Intel Core i7-13620H, 32 GB RAM, integrated Intel UHD Graphics GPU, Windows 11 build 26200, on mains power.

## Phase 0 — egui/eframe 0.36.2 baseline (2026-10-15, machine A)

Window with one label. Rust 1.99.0.

| Variant | Size | Warm startup (median) | Launch → main | Private memory |
|---|---|---|---|---|
| eframe `glow` + `default_fonts` | 4.95 MB | 143 ms | 17 ms | 28 MB |
| eframe `glow` + `default_fonts` + `accesskit` | 5.23 MB | 144 ms | 18 ms | — |
| eframe default (`wgpu`) | 9.73 MB | 264 ms | 17 ms | 244 MB |
| **`bracket.exe` 0.0.0** (`glow` + `default_fonts` + `accesskit`, 1000×700 window) | **5.23 MB** | **139 ms** | 16 ms | **40 MB** |

Conclusions:

- glow versus wgpu: half the size, about 120 ms less startup and one sixth of the memory on this GPU. glow is used.
- `accesskit` (screen reader accessibility) costs 0.3 MB and nothing measurable at startup. It is included.
- Startup of an empty window is already close to the 150 ms budget: about 125 ms are spent inside eframe creating the window, the OpenGL context and the first frame. Very little margin is left for the rest of the application.

## Phase 0 — editor prototype (2026-10-15, machine A)

`bracket.exe` 0.0.0 with the editor prototype (commit `feat(ui): add editor prototype`). Files generated with `cargo run --release -p bracket-core --example generate_large_file -- <lines> <path>` (CRLF, tabs, non-ASCII text).

| Case | Size | Launch → content visible (warm median) | Private memory |
|---|---|---|---|
| No file | 5.53 MB | 143 ms | 28 MB |
| 1,000,000 lines (52.6 MB) | 5.53 MB | 263 ms | 88 MB |
| 2,000,000 lines (100.2 MB) | 5.53 MB | 363 ms | 146 MB |

Idle memory varies by about ±10 MB between runs (the empty window measured 40 MB and later 28 MB).

### Benchmarks (criterion, `cargo bench --workspace`)

| Benchmark | Time (median) |
|---|---|
| `open/rope_from_str_1m_lines`: build the 1M-line rope from memory | 23 ms |
| `open/load_file_1m_lines`: read from disk, validate UTF-8 and create the document | 50 ms |
| `insert_char_middle_1m_lines`: insert one char in the middle | 1.6 µs |
| `frame_1000x700_middle_of_1m_lines`: full frame with no changes (cached galleys) | 91 µs |
| `frame_1000x700_scrolling_page_down_1m_lines`: frame showing a new page of lines | 393 µs |

Frame time is CPU only (egui layout, painting and tessellation); GPU work and presentation are not included.

Conclusions:

- The 100 MB file is visible in 0.36 s, under the 1 s budget, without blocking the interface (loading runs on a worker thread).
- A scrolling frame costs 0.4 ms of CPU: plenty of headroom for 60 Hz.

## Phase 0 — interface framework comparison (2026-10-15, machine A)

iced 0.14.0 prototype (wgpu and tiny-skia variants) against `bracket.exe`; its code remains in the history (commit `61c1712`). Full table and analysis in [decisions/0001-ui.md](decisions/0001-ui.md).

Scrolling (`./scripts/measure.ps1 -Exe <exe> -Arguments target\large-1m.txt -Scroll [-Maximized]`, interleaved runs): one page per frame for 300 frames.

| | CPU per frame, 1000×700 | CPU per frame, maximized | fps maximized |
|---|---|---|---|
| egui + glow (`bracket.exe`) | 3.8 ms | 4.2 ms | ~100 (vsync) |
| iced + wgpu | 3.3 ms | 3.4 ms | ~100 (vsync) |
| iced + tiny-skia | 8.6 ms | 14.8 ms | 66 (no vsync) |

Note: during these tests the machine had a varying background load (12–43 % CPU); startups measured in interleaved rounds are slower than in the previous tables, which were taken at a quiet moment.

## Phase 1, step 1 — startup analysis (2026-10-15, machine A, background load < 10 %)

`measure.ps1` now breaks startup down using timestamps from the application itself (app created and first `ui()` call):

| Phase | Warm median |
|---|---|
| Launch → `main` | 12–14 ms |
| `main` → app created (event loop, window, WGL, OpenGL context, egui painter) | 74–79 ms |
| App created → first `ui()` call | 1.5 ms |
| First `ui()` call → first frame presented | 35–38 ms |
| **Total** | **123–133 ms** |

Breakdown of "`main` → app created", from eframe's log messages with timestamps:

| Part | Time |
|---|---|
| winit event loop | ~5.5 ms |
| Window + WGL initialization (loading `opengl32` and the driver, pixel format selection) | ~60 ms |
| OpenGL context and making it current | ~8 ms |
| egui painter (OpenGL version, shaders, VAO) | ~5 ms |

Settings tried in interleaved rounds (12 rounds; noise between identical variants: ±8 ms): `vsync = false`, `dithering = false`, hardware acceleration required, no drag and drop, no emoji fonts, and the Hack font only. None changes startup beyond the noise.

Conclusions:

- Most of the startup is OpenGL initialization on Windows (driver included), outside the application's control. With a quiet machine startup meets the budget (123–133 ms < 150 ms) with about 20 ms to spare.
- `./scripts/check.ps1 -Perf` builds in release and fails if the warm median exceeds 150 ms. It does not run in continuous integration (runners have no GPU) and should be run with little background load.
- Everything added in later phases must stay off the first frame's path (on threads or deferred).

## Phase 1 — executable size over time (machine A)

| Step | Size | Change | Reason |
|---|---|---|---|
| End of Phase 0 | 5,802,496 B | — | |
| Step 2: TOML configuration | 5,971,456 B | +165 KB | `toml_edit` and the configuration code |
| Step 3: translations and language selector | 6,099,456 B | +128 KB | Catalogs and language menu. The catalog mechanism measures +7.5 KB in isolation (see [decisions/0002-i18n.md](decisions/0002-i18n.md)); the rest is probably egui's menu code, linked for the first time (not measured separately) |
| Step 4: system fonts | 4,736,000 B | −1,363 KB | No bundled egui fonts (`default_fonts`); `winreg` added |
| Step 5: encodings | 5,165,568 B | +430 KB | `encoding_rs` (tables for every WHATWG encoding) and `chardetng` |
| Step 6: file dialogs | 5,216,256 B | +50 KB | `rfd` and the File menu. Interleaved startup against step 5: 135.6 versus 134.3 ms |
| Step 7: saving with `ReplaceFileW` | 5,228,032 B | +12 KB | `windows-sys` (already in the tree). Interleaved startup against step 6 on a very busy machine: 289 versus 312 ms (no regression) |
| Step 8: tabs and language registry | 5,266,944 B | +39 KB | Tab bar, language registry and several files from the command line. Interleaved startup against step 7 (15 rounds, busy machine): 236.2 versus 238.6 ms (no regression) |
| Step 9: recoverable session | 5,323,776 B | +57 KB | Session file and backups, restoring tabs. Interleaved startup against step 8 (15 rounds, quiet machine): 120.2 versus 127.0 ms. Reading a 10-tab session takes 30 µs (`cargo bench -p bracket-session`); the probes do not read it |
| Step 9: window position | 5,870,592 B | +547 KB | eframe's `persistence` feature (serde and ron). Interleaved startup against step 9 without it (15 rounds): 126.8 versus 137.4 ms. With the probes, `window.ron` is neither read nor written |
| Step 10: reload on disk changes | 5,992,960 B | +120 KB | `notify` (and `walkdir`; `windows-sys` 0.60 was already in the tree) and file following. Interleaved startup against step 9 (15 rounds): 119.6 versus 133.6 ms |
| Step 11: single instance and command line | 6,055,424 B | +61 KB | Arguments, help in the catalogs, `rfd` message, lock and inbox. Interleaved startup against step 10 (15 rounds): 133.1 versus 131.0 ms (noise). Handing files to the open instance: the second one exits in 110–230 ms |
| Step 11: lazy session loading | — | — | Session with one small active tab and two inactive ones of 100 and 52 MB, private memory after 3 s: 226 MB before, 47 MB with lazy loading (221 MB after activating the 100 MB one) |
| Step 12: highlighting with syntect | 8,043,520 B | +1,988 KB | Grammar bundle (344 KB compressed) and code: pure Rust regex engine (`fancy-regex`, `regex-automata` with Unicode tables), syntect parser, decompression and deserialisation. Interleaved startup against step 11 (15 rounds): 122.1 versus 125.1 ms (the grammars load on the highlighter thread) |
| Step 13: language packages | 8,300,544 B | +257 KB | syntect's YAML reader (`yaml-load`) for package grammars, and the loader. Interleaved startup against step 12 (15 rounds, busy machine): 165.5 versus 164.3 ms |
| Step 14: themes | 8,369,664 B | +69 KB | View menu, appearance and editor colours (the themes were already in the exe). Interleaved startup against step 13 (15 rounds): 139.5 versus 142.2 ms |
| Step 15: full status bar | 8,400,896 B | +31 KB | Document language, selections and new layout. Interleaved startup against step 14 (15 rounds): 135.4 versus 141.9 ms |
| Step 16: manifest, icon and version | 8,454,144 B | +53 KB | Icon (32 KB), RGBA window icon (16 KB), manifest and version. Interleaved startup against step 15 (15 rounds): 120.3 versus 131.7 ms; "first `ui()` → frame" drops from 33.5 to 25.8 ms (probably from declaring DPI awareness in the manifest) |

Step 5, interleaved startup against step 4 (15 rounds): 144.9 ms versus 148.6 ms, within the noise. That day absolute medians were around 145–149 ms with a busy machine: the margin against 150 ms is minimal.

Warm startup stays at 123–134 ms in these steps (within the ±8 ms noise).

### Step 4 — memory by the text that is open

| Text | Private memory |
|---|---|
| ASCII only | 30.7 MB |
| Japanese (loads Microsoft YaHei, 19.7 MB) | 72.7 MB |
| Japanese, Korean and symbols (YaHei + Malgun Gothic + Segoe UI Symbol) | 103.3 MB |

egui takes about twice the size of each loaded font. Fallback fonts are only loaded when such text appears on screen.

Startup, compared with interleaved runs against the step 3 exe (`scripts/compare-startup.ps1`, 20 rounds, background load between 3 and 29 %): step 3 145.2 ms, step 4 139.7 ms. No regression; single measurements that day ranged from 130 to 156 ms depending on machine load, which is why regressions are checked with interleaved runs.

Painting benchmarks after the font change (Consolas instead of Hack): steady frame 83 µs, scrolling frame 420 µs. A first version of missing-glyph detection raised the scrolling frame to 2 ms by asking about unsupported characters on every frame; now only non-ASCII characters are checked and a kind is no longer checked once its fallback was requested.

### Step 12: highlighting benchmarks (`cargo bench -p bracket-syntax`)

| Benchmark | Time (median) |
|---|---|
| First pass over 10,000 lines of C# | 1.12 s (≈110 µs per line) |
| Page at line 5,000 after typing in a comment on line 12 (checkpoints reused) | 27 ms (547 ms without reusing them) |
| Page at line 5,000 already highlighted (from the nearest checkpoint) | 6.8 ms |

The cost per line is that of the pure Rust regex engine; since what is visible is highlighted first, a file's first screen is coloured in about 15 ms.

## Phase 1 — closing, 0.1.0 (2026-10-18, machine A)

`./scripts/measure.ps1` (from a temporary portable copy, without the user's session). Budgets from the plan:

| Measure | Budget | 0.1.0 | How |
|---|---|---|---|
| Executable size | < 15 MB | 8,454,144 B (8.06 MB) | Release with LTO |
| Startup until the first frame | < 150 ms | 126.1 ms (warm median of 11; minimum 119.7) | `-Runs 12`, no file |
| Idle memory, no files | — | 30.4 MB | Private working set after 3 s |
| Memory with 10 tabs | < 80 MB | 33.6 MB (active: highlighted JSON); 49.2 MB if the active one loads the symbol font | 10 files (93 KB): C#, Razor, T-SQL, JSON, XML, CSS, Markdown, TOML and Rust; after 5 s |
| 100 MB file visible | < 1 s | 334 ms (warm median of 5) | `target\large-2m.txt`; 200 MB of memory |
| Ctrl+P with 50,000 files | < 50 ms | — | Phase 2 |

The first run of each measurement (1.2–1.5 s) is that of a freshly copied exe: it includes Windows Defender's scan and does not represent normal use.

## Phase 2, step 4 — project walk (2026-10-19, machine A)

`cargo bench -p bracket-index` on `target/project-50k` (`cargo run --release -p bracket-index --example generate_project`): 50,000 files in 50 × 20 folders, plus 5,000 in `node_modules/`, which `.gitignore` excludes.

| Benchmark | Time (median) |
|---|---|
| `walk/index_50k_files`: whole index, several threads | 110.5 ms |
| `walk/list_root`: listing the root for the tree | 1.53 ms |

Warm (Windows' file cache already holds the folders). Generating the project takes about 26 s, mostly because of Windows Defender.

## Phase 2, step 5 — project panel (2026-10-19, machine A)

| Measure | Step 4 | Step 5 | Change |
|---|---|---|---|
| Executable size | 8,521,216 B | 8,795,648 B | +268 KB: `ignore` (with globset and crossbeam) and the tree |
| Startup (`compare-startup.ps1`, 15 interleaved rounds, median) | 132.2 ms | 131.4 ms | Noise |

The tree paints only the visible rows (`ScrollArea::show_rows`) and rebuilds the list of rows when it changes, so the cost per frame does not depend on the size of the project. Listing a folder costs the same as step 4's `walk/list_root` (1.5 ms with 50 folders).

## Phase 2, step 7 — project watching (2026-10-19, machine A)

| Measure | Result |
|---|---|
| `walk/resync_root`: updating the index after a change at the root (50,000 files) | 9.9 ms (walking the whole project: 109 ms) |
| `walk/resync_deep_folder`: after a change in `src07\module03` | 0.66 ms |
| Tree up to date after creating a folder and two files, renaming one and deleting another from outside | < 1.5 s (200 ms debounce) |
| Executable size | 8,837,632 B (+37.5 KB against step 6, Rust included: Rust adds nothing) |

## Phase 2, step 8 — tree actions (2026-10-19, machine A)

| Measure | Step 7 | Step 8 | Change |
|---|---|---|---|
| Executable size | 8,837,632 B | 9,058,816 B | +216 KB: `trash` (with the `windows` crate's COM interfaces) and the actions |
| Startup (`compare-startup.ps1`, 15 interleaved rounds, median) | 117.1 ms | 121.8 ms | Noise (±8 ms between identical variants) |

## Phase 2, step 9 — Ctrl+P (2026-10-19, machine A)

`cargo bench -p bracket-index --bench fuzzy`: one query over the 50,000 files of `target/project-50k`, 200 results with positions. Budget: 50 ms per keystroke.

| Query | One thread | Up to 8 threads |
|---|---|---|
| `f` (almost everything matches) | 20.8 ms | 6.2 ms |
| `file` | 24.6 ms | 6.8 ms |
| `fl12cs` | 2.1 ms | 0.90 ms |
| `src07/module03/file12` | 1.95 ms | 0.73 ms |
| `zzz` (nothing matches) | 1.88 ms | 0.72 ms |

| Measure | Step 8 | Step 9 | Change |
|---|---|---|---|
| Executable size | 9,058,816 B | 9,251,328 B | +188 KB: `ignore`'s parallel walk (no longer only listing one folder), the matcher and the palette |
| Startup (15 interleaved rounds, median) | 118.9 ms | 122.6 ms | Noise |
| Private bytes after 6 s with `target/project-50k` open | 47.1 MB | 54.3 MB | +7.2 MB: the index of 50,000 paths |

## Phase 2, step 10 — command palette (2026-10-19, machine A)

Executable size: 9,291,264 B (+39 KB against step 9: the registry, the palette's command mode and the Go menu).

## Phase 2, step 11 — find in file (2026-10-19, machine A)

`cargo bench -p bracket-search`: a 50 MB document where one line in 100 contains "Customer".

| Benchmark | Time (median) |
|---|---|
| `find/rope_to_string_50mb`: copying the rope into one string, on the thread | 25.5 ms |
| `find/plain_50mb`: "customer", case-insensitive | 8.8 ms |
| `find/whole_word_50mb`: the same, whole word | 8.1 ms |
| `find/regex_50mb`: `item\d+5\.Price` | 90.5 ms |

| Measure | Step 10 | Step 11 | Change |
|---|---|---|---|
| Executable size | 9,291,264 B | 9,373,184 B | +80 KB: `regex` and the find bar |
| Startup (15 interleaved rounds, median) | 140.3 ms | 141.9 ms | Noise (the machine was busy for both) |

## Phase 2, step 12 — replace in file (2026-10-19, machine A)

Executable size: 9,398,784 B (+25 KB against step 11).

`find/replace_all_regex_50mb` (every `new Customer(…)` with its replacement `Customer.Create($1)`, about 10,000 matches in 50 MB): 79.8 ms. In this session the machine was about 7 times slower than at step 11 (in the same run `find/plain_50mb` gave 76 ms against 8.8 ms, and a release build took 6.5 min instead of 1.5), so the figure is pessimistic and will be repeated when the phase closes (step 15). Startup was not measured for the same reason.

## Phase 2, step 13 — find in folder (2026-10-19, machine A)

| Measure | Result |
|---|---|
| `folder/search_50k_files`: "x" in the 50,000 files of `target/project-50k` (all of them match), with a warm file cache | 765 ms (about 120 ms of it the walk; reference in the same session: `walk/index_50k_files` 118 ms) |
| Executable size | 9,461,760 B (+61 KB against step 12) |
| Startup (15 interleaved rounds, median, against step 11) | 146.6 → 148.0 ms: noise |

Results arrive file by file, so the first ones show long before it finishes.

## Phase 2, step 14 — replace in folder (2026-10-19, machine A)

Executable size: 9,523,200 B (+60 KB against step 13: replacing in files, the preview, the check boxes and the confirmation).

## Phase 2 — close, 0.2.0 (2026-10-19, machine A)

`./scripts/measure.ps1` (a temporary portable copy, without the user's session) and `cargo bench`. Budgets from the plan:

| Measure | Budget | 0.1.0 | 0.2.0 | How |
|---|---|---|---|---|
| Executable size | < 15 MB | 8,454,144 B | 9,523,712 B (9.08 MB) | Release with LTO |
| Startup to the first frame | < 150 ms | 126.1 ms | 123.5 ms (minimum 115.2) | `-Runs 12`, no file |
| Startup opening a project | < 150 ms | — | 129.6 ms (minimum 120.4) | `-Runs 15 -Arguments target\project-50k` |
| Idle memory, no files | — | 30.4 MB | 30.2 MB | Private working set after 3 s |
| Memory with 10 tabs | < 80 MB | 33.6 MB | 34.0 MB | 10 files (48 KB): C#, Razor, T-SQL, XML, CSS, Markdown, TOML, Rust, text and JSON (active); after 5 s |
| Memory with the 50,000-file project | — | — | 37.1 MB (private bytes 53.7 MB) | Tree and index built; after 5 s |
| 100 MB file visible | < 1 s | 334 ms | Not measured again | Phase 2 did not change loading |
| Ctrl+P with 50,000 files | < 50 ms | — | 7.0 ms (worst query, `file`) | `fuzzy/word` |

Project benchmarks (`target/project-50k`, warm file cache):

| Benchmark | Time (median) |
|---|---|
| `walk/index_50k_files`: whole index | 121.1 ms |
| `walk/resync_root` / `walk/resync_deep_folder`: updating the index after a change | 10.5 ms / 0.76 ms |
| `fuzzy/one_letter` … `fuzzy/no_match`: Ctrl+P | 0.74–7.0 ms |
| `folder/search_50k_files`: searching the 50,000 files (all of them match) | 940 ms |
| `find/plain_50mb` / `find/regex_50mb`: Ctrl+F in 50 MB | 10.3 ms / 100.3 ms |
| `find/replace_all_regex_50mb`: "Replace all" with groups in 50 MB | 14.2 ms (repeats step 12's pessimistic measure) |

Phase 1 benchmarks, no regression: `open/rope_from_str_1m_lines` 24.2 ms (was 23), `open/load_file_1m_lines` 45.2 ms (was 50), `insert_char_middle_1m_lines` 1.2 µs (was 1.6). Viewport painting, compared on the same day with 0.1.0 built separately: unchanged frame 110.6 µs against 102.5 µs and scrolling frame 542.6 µs against 505.4 µs (+7–8 %, about the noise between builds). A first round of benchmarks, run back to back with others, gave figures 3–4 times worse; repeated on its own, it did not reproduce.

During the close, opening the 50,000-file project at startup raised startup to 151.6 ms: the index walk on every core competed with the first frame. The index now starts after the first frame (commit `perf(ui): start the file index after the first frame`) and startup with a project stays at 129.6 ms.

The first run of each measurement (1.2–2.0 s) is that of a freshly copied exe: it includes Windows Defender's scan.

## Phase 3, step 1 — tree-sitter (2026-10-19, machine A)

A minimal program with the workspace's release profile (`lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, `strip = true`) that parses a text with each grammar:

| Variant | Size | Added |
|---|---|---|
| Without tree-sitter | 118,784 B | — |
| tree-sitter 0.27.1 (no grammar in use) | 130,048 B | +11 KB |
| + C# 0.23.5 | 5,545,984 B | +5.42 MB |
| + CSS 0.25.0, JSON 0.24.8 and XML 0.7.0 | 5,711,360 B | +0.17 MB (in total: +5.59 MB) |
| All of them, with the grammars at `opt-level = "s"` or `"z"` | 5,707,264 B | −4 KB |

The C# grammar's size is almost all data: `ts_parse_table` is a dense table of 4,096 states × 530 symbols × 2 bytes (4.3 MB) in `parser.c` (29.7 MB of generated source). No compiler option reduces it; the grammar would have to be regenerated. The default optimization stays.

C# parsing speed (a synthetic 1.5 MB file, 4,000 classes): 287 ms, 5.3 MB/s, 508,008 nodes; the same with `opt-level = "s"` or `"z"`.

## Phase 3, step 2 — C# symbols (2026-10-19, machine A)

`cargo bench -p bracket-index --bench symbols`: a synthetic C# file of 1.47 MB (4,000 classes with a field, a property and methods; 20,001 symbols).

| Benchmark | Time (median) | Speed |
|---|---|---|
| `symbols/csharp_1_5mb`: parsing and extracting the symbols | 414 ms | 3.4 MiB/s |

Breakdown measured in release: tree-sitter parsing 297 ms, query 96 ms, building the symbols (names, char positions and nesting) 4 ms. A first run gave 1.38 s with the machine busy (the `walk/index_50k_files` reference was slower too: 139 ms against 121); repeated, 414 ms. The project index (step 7) will parse files in parallel and the outline (step 5) incrementally.

## Phase 3, step 5 — outline, tree-sitter in the exe (2026-10-19, machine A)

| Measure | 0.2.0 | Step 5 | Change |
|---|---|---|---|
| Executable size | 9,523,712 B (9.08 MB) | 15,282,176 B (14.57 MB) | +5.49 MB: tree-sitter with the C#, CSS, JSON and XML grammars, the queries, the rules and the outline |
| Startup (`compare-startup.ps1`, 15 interleaved rounds, median) | 168.1 ms | 169.9 ms | Noise; both figures are high because the machine was busy |

The budget is 15 MB (as in every other measurement, 1 MB = 2²⁰ bytes: 15,728,640 B): 446,464 B (436 KB) are left for the coming phases.

## Phase 3, step 7 — project symbol index (2026-10-19, machine A)

`cargo run --release -p bracket-index --example project_symbols` on `target/project-cs`: 2,000 C# files (16.6 MiB, 210,000 symbols), with a warm file cache; 16 logical threads, the index uses 15.

| Measure | Result |
|---|---|
| Building the index | 522–553 ms (30–32 MiB/s). The first pass after generating the project took 6.1 s: freshly written files, probably scanned by the antivirus |
| Checking it again with no changes (date and size of each file) | 7.8–8.8 ms |
| Reading one file again | 2.2–2.9 ms |
| Estimated memory of the structure | 6.4 MiB |

In the application (`measure.ps1 -Runs 5 -IdleSeconds 6 -Arguments <project>`), against step 6:

| Measure | Step 6 | Step 7 | Change |
|---|---|---|---|
| Executable size | 15,296,000 B (14.59 MB) | 15,374,848 B (14.66 MB) | +77 KB; 353,792 B (345 KB) left up to the 15 MB budget |
| Private working set at 6 s, `target/project-cs` | 31.6 MB | 45.4 MB | +13.8 MB: 210,000 symbols |
| Private working set at 6 s, `target/project-50k` | 37.8 MB | 58.9 MB | +21.1 MB: 33,000 tiny files in a symbol language (one entry per file, a second copy of the path list and the memory the heap keeps after extracting on 15 threads) |
| Startup with `target/project-cs` (warm median) | 134.0 ms | 137.4 ms | Noise: the index starts after the first frame |
| Startup with `target/project-50k` (warm median) | 140.1 ms | 135.0 ms | Noise |

### Reducing the symbol index's memory (2026-10-19, machine A)

Measured phase by phase in a test program on `target/project-50k` (41,668 files in a symbol language, nearly all without symbols), private working set: the copy of the path list added 2.9 MB; compiling the queries, 2.7 MB; and the extraction, 14.8 MB, of which dropping the index freed only 7.2: the heap kept 7.4 MB. What it kept grew with the threads (1 thread: nothing; 4 threads: 1.7 MB; 15 threads: 7.4 MB). Changes:

- At most 4 reading threads. Building the index of `target/project-cs` goes from 0.51 to 0.94–0.99 s (2 threads: 1.69 s).
- The path list is the same `Arc` as `FileIndex`'s, with no copy.
- Files without symbols keep only their stamp (a 48-byte entry instead of 80); `Box<str>` keys.
- One parser per thread and language, instead of one per file (`SymbolParser::extract`).

| Measure | Before | After | Step 6 (no index) |
|---|---|---|---|
| Private working set at 6 s, `target/project-50k` | 58.9 MB | 44.5 MB | 37.4–37.8 MB |
| Private working set at 6 s, `target/project-cs` | 45.4 MB | 40.8 MB | 31.6 MB |
| Estimated memory of the structure, `target/project-50k` | 5.4 MiB | 3.9 MiB | — |
| Building the index of `target/project-cs` | 0.52–0.55 s | 0.94–0.99 s | — |
| Executable size | 15,374,848 B | 15,382,016 B (14.67 MB) | 15,296,000 B |

The symbol index now costs about 7 MB with 50,000 files and 9 MB with 210,000 symbols. Startup does not change (`compare-startup.ps1` without a project, three batches: differences from −10 to +7 ms depending on the order, noise on a busy machine).

## Phase 3, step 8 — Ctrl+T (2026-10-19, machine A)

`cargo bench -p bracket-index --bench fuzzy`, group `fuzzy_symbols`: one query over the 210,000 symbols of `target/project-cs`, with 200 results; it includes gathering the names, scoring them and building the results, as each keystroke does. Budget: 50 ms per keystroke.

| Query | Time |
|---|---|
| One letter (`c`) | 16.8 ms |
| Word (`compute`) | 12.9 ms |
| Scattered letters (`svc12x3`) | 2.4 ms |
| Exact name (`Service12x3`) | 2.4 ms |
| No match (`zzz`) | 2.3 ms (gathering the names) |

Ctrl+P does not change with the generic matcher: 6.3 ms with one letter and 7.0 ms with a word over 50,000 files. Executable: 15,412,736 B (14.70 MB), +30 KB; 315,904 B (308 KB) left up to the 15 MB budget.

## Phase 3, step 9 — go to definition (2026-10-19, machine A)

Executable: 15,452,160 B (14.74 MB), +39 KB; 276,480 B (270 KB) left up to the 15 MB budget. Looking up a definition (`cargo bench -p bracket-index --bench fuzzy`, `fuzzy_symbols/definitions`: `Compute3` among the 210,000 symbols of `target/project-cs`, with 8,000 matches and 200 results) takes 2.2 ms.

## Phase 3, step 11 — path at the caret (2026-10-19, machine A)

With a generated 5.0 MiB XML (64,344 sibling `Line` elements), on the path thread: first time (full parse) 945 ms; moving the caret (no parse; counting the siblings of the same name) 41 ms; inserting an element in the middle (incremental parse) 153 ms. Executable: 15,500,288 B (14.78 MB), +48 KB; 228,352 B (223 KB) left up to the 15 MB budget.

## Phase 3 — close, 0.3.0 (2026-10-19, machine A)

`./scripts/measure.ps1` (temporary portable copy, without the user's session) with 0.3.0 and with 0.2.0 built separately the same day, and `cargo bench --workspace`. Plan budgets:

| Measure | Budget | 0.2.0 | 0.3.0 | How |
|---|---|---|---|---|
| Executable size | < 15 MB | 9,523,712 B | 15,503,360 B (14.79 MB) | Release with LTO; 225,280 B (220 KB) left |
| Startup to the first frame | < 150 ms | 146.5 ms | 136.7 ms (minimum 122.5) | `-Runs 12`, no file, one version after the other (busy machine) |
| Startup, interleaved | < 150 ms | 125.7 / 128.4 / 130.9 ms | 131.1 / 123.7 / 132.4 ms | `compare-startup.ps1`, three batches: noise |
| Startup opening a project | < 150 ms | 127.4 ms | 135.7 ms (minimum 117.2) | `-Runs 15 -Arguments target\project-50k` |
| Idle memory, no files | — | 30.1 MB | 30.3 MB | Private working set at 3 s |
| Memory with 10 tabs | < 80 MB | 43.2 MB | 43.2 MB | 10 files (26 KB): C#, Razor, T-SQL, XML, CSS, Markdown, TOML, Rust, text and JSON (active); at 5 s |
| Memory with the 50,000-file project | — | 37.5 MB | 37.7 MB | Tree and file index; at 6 s |
| Same with `[experimental] project_navigation = true` | — | — | 44.9 MB | `-Config` with the option; symbol index built |
| `target/project-cs` with navigation on | — | — | 41.8 MB | 2,000 C# files, 210,000 symbols |
| 100 MB file visible | < 1 s | 334 ms | Not measured again | Phase 3 did not change loading |
| Ctrl+P with 50,000 files | < 50 ms | 7.0 ms | 7.8 ms (worst query, `file`) | `fuzzy/word` |
| Ctrl+T with 210,000 symbols (experimental) | < 50 ms | — | 13.0 ms (worst query, one letter) | `fuzzy_symbols/one_letter` |

With navigation off (the usual case), 0.3.0 uses the same memory as 0.2.0: the outline, the path at the caret and the index only work when needed.

Phase 3 benchmarks:

| Benchmark | Time (median) |
|---|---|
| `symbols/csharp_1_5mb`: extracting the symbols of a 1.47 MB C# file | 370 ms (4.0 MiB/s; step 1: 414 ms) |
| `fuzzy_symbols/one_letter` … `no_match`: Ctrl+T with 210,000 symbols | 2.1–13.0 ms |
| `fuzzy_symbols/definitions`: go to definition, `Compute3` (8,000 matches) | 1.6 ms |
| Symbol index of `target/project-cs` (`--example project_symbols`) | 0.94–0.99 s with 4 threads (step 7, memory reduction) |

Earlier phases' benchmarks, no regression: `walk/index_50k_files` 108 ms (0.2.0: 121), `walk/resync_root` 8.0 ms (10.5), `fuzzy/*` 0.84–7.8 ms (0.74–7.0), `folder/search_50k_files` 641 ms (940), `find/plain_50mb` 7.8 ms (10.3), `find/regex_50mb` 81 ms (100), `find/replace_all_regex_50mb` 10.1 ms (14.2), `open/rope_from_str_1m_lines` 20.7 ms (24.2), `open/load_file_1m_lines` 39.9 ms (45.2), `insert_char_middle_1m_lines` 1.1 µs (1.2), unchanged viewport 82 µs (110.6) and scrolling 398 µs (542.6). Highlighting: first pass of 10,000 C# lines 1.05 s, page at line 5,000 after typing at line 12 24.7 ms, unchanged 6.2 ms.

The first run of each measurement (1.2–1.7 s) is a freshly copied exe: it includes Windows Defender's scan.
