//! egui user interface: editor view, tabs, project tree, palette and status bar.

mod app;
mod editor_view;
mod i18n;
mod startup;

use std::path::PathBuf;

use eframe::egui;

pub use startup::StartupProbe;

/// Opens the main window, optionally loading `path`, and runs the event loop until it closes.
pub fn run(path: Option<PathBuf>, probe: Option<StartupProbe>) -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Bracket")
            .with_inner_size([1000.0, 700.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "Bracket",
        options,
        Box::new(move |cc| Ok(Box::new(app::BracketApp::new(&cc.egui_ctx, path, probe)))),
    )
}
