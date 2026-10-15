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

iced 0.14.0 prototype in `experiments/ui-iced` (wgpu and tiny-skia variants) against `bracket.exe`. Full table and analysis in [decisions/0001-ui.md](decisions/0001-ui.md).

Scrolling (`./scripts/measure.ps1 -Exe <exe> -Arguments target\large-1m.txt -Scroll [-Maximized]`, interleaved runs): one page per frame for 300 frames.

| | CPU per frame, 1000×700 | CPU per frame, maximized | fps maximized |
|---|---|---|---|
| egui + glow (`bracket.exe`) | 3.8 ms | 4.2 ms | ~100 (vsync) |
| iced + wgpu | 3.3 ms | 3.4 ms | ~100 (vsync) |
| iced + tiny-skia | 8.6 ms | 14.8 ms | 66 (no vsync) |

Note: during these tests the machine had a varying background load (12–43 % CPU); startups measured in interleaved rounds are slower than in the previous tables, which were taken at a quiet moment.
