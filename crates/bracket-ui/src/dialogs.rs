//! Native file dialogs (the system's Open and Save As windows, through `rfd`).
//!
//! The dialogs are modal to Bracket's window, like in any editor: they run on the UI thread
//! and the window waits until they close.

use std::path::{Path, PathBuf};

/// Shows a message with an OK button and waits until it is closed.
pub fn message(text: &str) {
    rfd::MessageDialog::new()
        .set_title("Bracket")
        .set_description(text)
        .set_level(rfd::MessageLevel::Info)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}

/// Asks for a file to open. Starts in `start_dir` if given.
pub fn pick_file_to_open(
    parent: &eframe::Frame,
    title: &str,
    start_dir: Option<&Path>,
) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new().set_title(title).set_parent(parent);
    if let Some(dir) = start_dir {
        dialog = dialog.set_directory(dir);
    }
    dialog.pick_file()
}

/// Asks for a folder to open as a project. Starts in `start_dir` if given.
pub fn pick_folder(
    parent: &eframe::Frame,
    title: &str,
    start_dir: Option<&Path>,
) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new().set_title(title).set_parent(parent);
    if let Some(dir) = start_dir {
        dialog = dialog.set_directory(dir);
    }
    dialog.pick_folder()
}

/// Asks where to save, suggesting `current` (its folder and name) if the document has a path.
pub fn pick_file_to_save(
    parent: &eframe::Frame,
    title: &str,
    current: Option<&Path>,
) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new().set_title(title).set_parent(parent);
    if let Some(path) = current {
        if let Some(dir) = path.parent() {
            dialog = dialog.set_directory(dir);
        }
        if let Some(name) = path.file_name() {
            dialog = dialog.set_file_name(name.to_string_lossy());
        }
    }
    dialog.save_file()
}
