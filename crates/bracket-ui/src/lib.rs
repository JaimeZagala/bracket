//! egui user interface: editor view, tabs, project tree, palette and status bar.

mod app;
mod editor_view;
mod i18n;
mod measure;

use std::path::PathBuf;

use eframe::egui;

pub use editor_view::{EditorView, ViewOutput};
pub use measure::Probe;

/// Opens the main window, optionally loading `path`, and runs the event loop until it closes.
pub fn run(
    path: Option<PathBuf>,
    settings: bracket_session::LoadedConfig,
    probe: Option<Probe>,
) -> eframe::Result {
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
        Box::new(move |cc| {
            Ok(Box::new(app::BracketApp::new(
                &cc.egui_ctx,
                path,
                settings,
                probe,
            )))
        }),
    )
}
