//! Bracket executable entry point.

// Release builds are GUI applications without a console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cli;
mod console;
mod file_replace;
mod instance;
mod system_fonts;

use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

use bracket_session::{Paths, config, session};
use bracket_ui::fonts::{self, FontSetup};

use crate::cli::Cli;
use crate::file_replace::WindowsReplacer;
use crate::instance::Instance;
use crate::system_fonts::SystemFonts;

const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The window icon in raw RGBA, drawn by build.rs like the executable's icon.
const WINDOW_ICON: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/window-icon.rgba"));
const WINDOW_ICON_SIZE: u32 = 64;

fn main() -> Result<(), Box<dyn Error>> {
    // Taken first so the startup measurement covers as much as possible.
    let probe = bracket_ui::Probe::from_env();
    let cli = cli::parse(std::env::args_os().skip(1));

    let exe = std::env::current_exe()?;
    let exe_dir = exe.parent().unwrap_or(&exe);
    let app_data = std::env::var_os("APPDATA").map(PathBuf::from);
    let paths = Paths::resolve(exe_dir, app_data.as_deref());
    let settings = config::load(&paths.config_file());
    let language = settings.config.language;

    let files = match cli {
        Cli::Open(files) => files,
        Cli::Help => {
            console::show(&bracket_ui::cli_help(language, VERSION));
            return Ok(());
        }
        Cli::Version => {
            console::show(&format!("Bracket {VERSION}"));
            return Ok(());
        }
        Cli::Unknown(option) => {
            let message = bracket_ui::cli_unknown_option(language, &option);
            console::show(&format!(
                "{message}\n\n{}",
                bracket_ui::cli_help(language, VERSION)
            ));
            std::process::exit(2);
        }
    };

    // One Bracket per data folder: a second one gives its files to the first and exits. While
    // measuring, every run is on its own.
    let mut requests_dir = None;
    let _lock = if probe.is_none() {
        match instance::acquire(&paths.data_dir) {
            Instance::Running => {
                if instance::hand_over(&paths.requests_dir(), &files).unwrap_or(false) {
                    return Ok(());
                }
                // Nobody took them (the other Bracket was closing): open them here.
                None
            }
            Instance::First(lock) => {
                requests_dir = Some(paths.requests_dir());
                lock
            }
        }
    } else {
        None
    };

    // Reading the registry and the font files overlaps with the OpenGL setup in eframe.
    let family = settings.config.font_family.clone();
    let fonts = thread::spawn(move || {
        let source = Arc::new(SystemFonts::read());
        let startup = fonts::load_startup_fonts(source.as_ref(), &family);
        FontSetup { source, startup }
    });

    // While measuring, the user's session is neither restored nor changed.
    let session = probe.is_none().then(|| {
        let files = paths.session_files();
        let (previous, error) = session::load_previous(&files);
        bracket_ui::RestoredSession {
            files,
            previous,
            error,
        }
    });
    let window_file = probe.is_none().then(|| paths.window_file());

    // A few small manifests: read now, so tabs know their language from the first frame.
    // Their grammars are parsed later, on the highlighter thread.
    let languages = bracket_syntax::packages::load(&paths.language_dirs);

    let launch = bracket_ui::Launch {
        files,
        requests_dir,
        settings,
        config_path: paths.config_file(),
        fonts,
        replacer: Arc::new(WindowsReplacer),
        session,
        window_file,
        languages,
        window_icon: Some(bracket_ui::egui::IconData {
            rgba: WINDOW_ICON.to_vec(),
            width: WINDOW_ICON_SIZE,
            height: WINDOW_ICON_SIZE,
        }),
    };
    bracket_ui::run(launch, probe)?;
    Ok(())
}
