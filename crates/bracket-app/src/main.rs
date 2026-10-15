//! Bracket executable entry point.

// Release builds are GUI applications without a console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::error::Error;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn Error>> {
    // Taken first so the startup measurement covers as much as possible.
    let probe = bracket_ui::StartupProbe::from_env();
    // Phase 0: the only argument is an optional file to open.
    let path = std::env::args_os().nth(1).map(PathBuf::from);
    bracket_ui::run(path, probe)?;
    Ok(())
}
