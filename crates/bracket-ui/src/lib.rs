//! egui user interface: editor view, tabs, project tree, palette and status bar.

mod app;
mod dialogs;
mod disk;
mod editor_view;
pub mod fonts;
mod i18n;
mod measure;
mod session;
mod tabs;

use std::path::PathBuf;
use std::thread::JoinHandle;

use eframe::egui;

pub use editor_view::{EditorView, ViewOutput};
pub use measure::Probe;

/// Everything `bracket-app` prepares before the window opens.
pub struct Launch {
    /// Files to open from the command line, each in its own tab.
    pub files: Vec<PathBuf>,
    pub settings: bracket_session::LoadedConfig,
    /// Where language changes made from the interface are saved.
    pub config_path: PathBuf,
    /// Font loading, started early so it overlaps with window creation.
    pub fonts: JoinHandle<fonts::FontSetup>,
    /// How saved files replace the old ones (keeps attributes and permissions on Windows).
    pub replacer: std::sync::Arc<dyn bracket_core::document::FileReplacer>,
    /// The previous session and where to keep this one. `None` while measuring, so that
    /// measurements neither depend on the user's session nor change it.
    pub session: Option<RestoredSession>,
    /// Where eframe keeps the window position and size. `None` while measuring, so every
    /// measurement starts with the default window.
    pub window_file: Option<PathBuf>,
}

/// The session left by the previous run.
pub struct RestoredSession {
    pub files: bracket_session::SessionFiles,
    pub previous: bracket_session::Session,
    /// Why the previous session could not be read, if it could not.
    pub error: Option<String>,
}

/// Opens the main window and runs the event loop until it closes.
pub fn run(launch: Launch, probe: Option<Probe>) -> eframe::Result {
    let saved_window = launch.window_file.as_ref().is_some_and(|f| f.is_file());
    // eframe always needs a file; while measuring it points to one that is never written.
    let window_file = launch
        .window_file
        .clone()
        .unwrap_or_else(|| std::env::temp_dir().join("bracket-measure-window.ron"));
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Bracket")
            .with_inner_size([1000.0, 700.0])
            .with_maximized(measure::maximized_requested())
            .with_drag_and_drop(true),
        // eframe restores the saved position and size, moving the window onto a connected
        // monitor if needed. It applies `centered` after that, so it is only used the first
        // time: then the window opens centered and never partly off screen.
        centered: !saved_window,
        persist_window: launch.window_file.is_some(),
        persistence_path: Some(window_file),
        ..Default::default()
    };
    eframe::run_native(
        "Bracket",
        options,
        Box::new(move |cc| Ok(Box::new(app::BracketApp::new(&cc.egui_ctx, launch, probe)))),
    )
}
