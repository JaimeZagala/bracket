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
