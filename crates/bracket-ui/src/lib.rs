//! egui user interface: editor view, tabs, project tree, palette and status bar.

mod app;
mod dialogs;
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
    /// Window size (and whether it was maximized) from the previous session, or the default.
    pub window: bracket_session::WindowState,
}

/// The session left by the previous run.
pub struct RestoredSession {
    pub files: bracket_session::SessionFiles,
    pub previous: bracket_session::Session,
    /// Why the previous session could not be read, if it could not.
    pub error: Option<String>,
}

/// Window size when there is no session.
pub const DEFAULT_WINDOW: bracket_session::WindowState = bracket_session::WindowState {
    width: 1000.0,
    height: 700.0,
    maximized: false,
};

/// Opens the main window and runs the event loop until it closes.
pub fn run(launch: Launch, probe: Option<Probe>) -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Bracket")
            .with_inner_size([launch.window.width, launch.window.height])
            .with_maximized(launch.window.maximized || measure::maximized_requested())
            .with_drag_and_drop(true),
        // Centered on the monitor, the window never opens partly off screen.
        centered: true,
        ..Default::default()
    };
    eframe::run_native(
        "Bracket",
        options,
        Box::new(move |cc| Ok(Box::new(app::BracketApp::new(&cc.egui_ctx, launch, probe)))),
    )
}
