//! egui user interface: editor view, tabs, project tree, palette and status bar.

mod app;
mod editor_view;
pub mod fonts;
mod i18n;
mod measure;

use std::path::PathBuf;
use std::thread::JoinHandle;

use eframe::egui;

pub use editor_view::{EditorView, ViewOutput};
pub use measure::Probe;

/// Everything `bracket-app` prepares before the window opens.
pub struct Launch {
    /// File to open, from the command line.
    pub file: Option<PathBuf>,
    pub settings: bracket_session::LoadedConfig,
    /// Where language changes made from the interface are saved.
    pub config_path: PathBuf,
    /// Font loading, started early so it overlaps with window creation.
    pub fonts: JoinHandle<fonts::FontSetup>,
}

/// Opens the main window and runs the event loop until it closes.
pub fn run(launch: Launch, probe: Option<Probe>) -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Bracket")
            .with_inner_size([1000.0, 700.0])
            .with_maximized(measure::maximized_requested())
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "Bracket",
        options,
        Box::new(move |cc| Ok(Box::new(app::BracketApp::new(&cc.egui_ctx, launch, probe)))),
    )
}
