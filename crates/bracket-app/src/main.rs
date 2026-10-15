//! Bracket executable entry point.

// Release builds are GUI applications without a console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::error::Error;
use std::path::PathBuf;

use bracket_session::{Paths, config};

fn main() -> Result<(), Box<dyn Error>> {
    // Taken first so the startup measurement covers as much as possible.
    let probe = bracket_ui::Probe::from_env();
    // Phase 0: the only argument is an optional file to open.
    let path = std::env::args_os().nth(1).map(PathBuf::from);

    let exe = std::env::current_exe()?;
    let exe_dir = exe.parent().unwrap_or(&exe);
    let app_data = std::env::var_os("APPDATA").map(PathBuf::from);
    let paths = Paths::resolve(exe_dir, app_data.as_deref());
    let settings = config::load(&paths.config_file());

    bracket_ui::run(path, settings, paths.config_file(), probe)?;
    Ok(())
}
