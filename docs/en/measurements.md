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
