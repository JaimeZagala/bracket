//! Measurement probes used by `scripts/measure.ps1`. They are inactive unless their environment
//! variable is set, and each one writes a small `key=value` report and exits the process.
//!
//! - `BRACKET_MEASURE_STARTUP=<file>`: startup timestamps once the first frame with the final
//!   content (the loaded file, if any) is on screen.
//! - `BRACKET_MEASURE_SCROLL=<file>`: after that first frame, scrolls down one page per frame
//!   and reports the wall time of 300 frames.
//! - `BRACKET_MEASURE_MAXIMIZED=1`: starts with the window maximized.

use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const STARTUP_VAR: &str = "BRACKET_MEASURE_STARTUP";
const SCROLL_VAR: &str = "BRACKET_MEASURE_SCROLL";
const MAXIMIZED_VAR: &str = "BRACKET_MEASURE_MAXIMIZED";

/// Frames measured by the scroll probe.
pub const SCROLL_FRAMES: u32 = 300;

#[derive(Debug)]
pub enum Probe {
    Startup(StartupProbe),
    Scroll(ScrollProbe),
}

impl Probe {
    /// Call first thing in `main()`. Returns `None` unless a measurement was requested.
    pub fn from_env() -> Option<Self> {
        let main_wall = SystemTime::now();
        let main_instant = Instant::now();
        if let Some(output) = std::env::var_os(STARTUP_VAR) {
            return Some(Self::Startup(StartupProbe {
                output: PathBuf::from(output),
                main_wall,
                main_instant,
                app_created: None,
                first_ui: None,
            }));
        }
        let output = std::env::var_os(SCROLL_VAR)?;
        Some(Self::Scroll(ScrollProbe {
            output: PathBuf::from(output),
            frames: 0,
            started: None,
        }))
    }
}

/// True when the window should start maximized for a measurement.
pub fn maximized_requested() -> bool {
    std::env::var_os(MAXIMIZED_VAR).is_some_and(|v| v == "1")
}

#[derive(Debug)]
pub struct StartupProbe {
    output: PathBuf,
    main_wall: SystemTime,
    main_instant: Instant,
    /// When eframe handed over control: window and graphics context exist.
    app_created: Option<SystemTime>,
    /// Start of the first `ui()` call.
    first_ui: Option<SystemTime>,
}

impl StartupProbe {
    pub(crate) fn mark_app_created(&mut self) {
        self.app_created.get_or_insert_with(SystemTime::now);
    }

    pub(crate) fn mark_first_ui(&mut self) {
        self.first_ui.get_or_insert_with(SystemTime::now);
    }

    /// Writes the report and terminates the process.
    pub(crate) fn finish(&self) -> ! {
        let optional = |name: &str, t: Option<SystemTime>| {
            t.map(|t| format!("{name}={}\n", unix_micros(t)))
                .unwrap_or_default()
        };
        let report = format!(
            "main_unix_us={}\n{}{}frame_unix_us={}\nmain_to_frame_ms={:.1}\n",
            unix_micros(self.main_wall),
            optional("app_unix_us", self.app_created),
            optional("first_ui_unix_us", self.first_ui),
            unix_micros(SystemTime::now()),
            self.main_instant.elapsed().as_secs_f64() * 1000.0
        );
        // Best effort: a failed write simply shows up as a missing measurement.
        let _ = std::fs::write(&self.output, report);
        std::process::exit(0);
    }
}

#[derive(Debug)]
pub struct ScrollProbe {
    output: PathBuf,
    frames: u32,
    started: Option<Instant>,
}

impl ScrollProbe {
    /// Call once per scroll frame. Exits the process after the last one.
    pub(crate) fn frame(&mut self) {
        let started = *self.started.get_or_insert_with(Instant::now);
        self.frames += 1;
        if self.frames >= SCROLL_FRAMES {
            let report = format!(
                "frames={}\nscroll_ms={:.1}\n",
                self.frames,
                started.elapsed().as_secs_f64() * 1000.0
            );
            let _ = std::fs::write(&self.output, report);
            std::process::exit(0);
        }
    }
}

fn unix_micros(t: SystemTime) -> u128 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros())
        .unwrap_or(0)
}
