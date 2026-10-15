//! egui user interface: editor view, tabs, project tree, palette and status bar.

mod startup;

use eframe::egui;

pub use startup::StartupProbe;

/// Opens the main window and runs the event loop until it is closed.
pub fn run(probe: Option<StartupProbe>) -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Bracket")
            .with_inner_size([1000.0, 700.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Bracket",
        options,
        Box::new(|_cc| Ok(Box::new(BracketApp { probe, frames: 0 }))),
    )
}

struct BracketApp {
    probe: Option<StartupProbe>,
    frames: u32,
}

impl eframe::App for BracketApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.label("Bracket");
        });

        if let Some(probe) = &self.probe {
            self.frames += 1;
            // The second ui() call happens once the first frame has been presented.
            if self.frames >= 2 {
                probe.finish();
            }
            ui.ctx().request_repaint();
        }
    }
}
