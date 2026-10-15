//! Bracket executable entry point.

// Release builds are GUI applications without a console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod system_fonts;

use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

use bracket_session::{Paths, config};
use bracket_ui::fonts::{self, FontSetup};

use crate::system_fonts::SystemFonts;

fn main() -> Result<(), Box<dyn Error>> {
    // Taken first so the startup measurement covers as much as possible.
    let probe = bracket_ui::Probe::from_env();
    // Phase 0: the only argument is an optional file to open.
    let file = std::env::args_os().nth(1).map(PathBuf::from);

    let exe = std::env::current_exe()?;
    let exe_dir = exe.parent().unwrap_or(&exe);
    let app_data = std::env::var_os("APPDATA").map(PathBuf::from);
    let paths = Paths::resolve(exe_dir, app_data.as_deref());
    let settings = config::load(&paths.config_file());

    // Reading the registry and the font files overlaps with the OpenGL setup in eframe.
    let family = settings.config.font_family.clone();
    let fonts = thread::spawn(move || {
        let source = Arc::new(SystemFonts::read());
        let startup = fonts::load_startup_fonts(source.as_ref(), &family);
        FontSetup { source, startup }
    });

    let launch = bracket_ui::Launch {
        file,
        settings,
        config_path: paths.config_file(),
        fonts,
    };
    bracket_ui::run(launch, probe)?;
    Ok(())
}
