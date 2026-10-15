//! Startup time probe used by `scripts/measure.ps1`.
//!
//! When the `BRACKET_MEASURE_STARTUP` environment variable names a file, the application writes
//! its startup timestamps there once the first frame is on screen, and exits.

use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub const ENV_VAR: &str = "BRACKET_MEASURE_STARTUP";

#[derive(Debug)]
pub struct StartupProbe {
    output: PathBuf,
    main_wall: SystemTime,
    main_instant: Instant,
}

impl StartupProbe {
    /// Call first thing in `main()`. Returns `None` unless a measurement was requested.
    pub fn from_env() -> Option<Self> {
        let output = std::env::var_os(ENV_VAR)?;
        Some(Self {
            output: PathBuf::from(output),
            main_wall: SystemTime::now(),
            main_instant: Instant::now(),
        })
    }

    /// Writes the report and terminates the process.
    pub(crate) fn finish(&self) -> ! {
        let report = format!(
            "main_unix_us={}\nframe_unix_us={}\nmain_to_frame_ms={:.1}\n",
            unix_micros(self.main_wall),
            unix_micros(SystemTime::now()),
            self.main_instant.elapsed().as_secs_f64() * 1000.0
        );
        // Best effort: a failed write simply shows up as a missing measurement.
        let _ = std::fs::write(&self.output, report);
        std::process::exit(0);
    }
}

fn unix_micros(t: SystemTime) -> u128 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros())
        .unwrap_or(0)
}
