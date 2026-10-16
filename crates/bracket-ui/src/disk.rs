//! Following open files that other programs change on disk.
//!
//! The watcher only says that a path changed. A check on a worker thread compares the file's
//! stamp (modification time and size) with the one recorded when it was read or saved, and only
//! reads the file when they differ. A document without unsaved changes is reloaded silently; one
//! with changes shows a notice and the user decides.

use std::io;
use std::path::Path;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use bracket_core::document::{self, DiskStamp, LoadedText};
use bracket_core::encoding::TextEncoding;
use eframe::egui;

use crate::i18n::Texts;

/// Editors write, rename and touch a file in quick succession: wait for the burst to end.
pub const SETTLE: Duration = Duration::from_millis(200);

/// What a check found.
pub enum DiskState {
    /// Same stamp as last time: nothing to do.
    Unchanged,
    /// The file is gone (deleted, moved or renamed).
    Missing,
    Changed {
        loaded: LoadedText,
        stamp: DiskStamp,
    },
}

/// Compares the file with the stamp recorded for it and reads it if it differs. Runs on a
/// worker thread.
pub fn check(
    path: &Path,
    read_encoding: Option<TextEncoding>,
    known: Option<DiskStamp>,
) -> io::Result<DiskState> {
    let stamp = match DiskStamp::of(path) {
        Ok(stamp) => stamp,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(DiskState::Missing),
        Err(err) => return Err(err),
    };
    if known == Some(stamp) {
        return Ok(DiskState::Unchanged);
    }
    let loaded = match read_encoding {
        Some(encoding) => document::load_text_as(path, encoding),
        None => document::load_text(path),
    };
    match loaded {
        Ok(loaded) => Ok(DiskState::Changed { loaded, stamp }),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(DiskState::Missing),
        Err(err) => Err(err),
    }
}

/// Shown above a document with unsaved changes whose file changed on disk.
pub enum DiskNotice {
    /// The file has other contents; they are kept here until the user decides.
    Changed {
        loaded: LoadedText,
        stamp: DiskStamp,
    },
    Missing,
}

/// A tab's link with its file on disk.
#[derive(Default)]
pub struct DiskWatch {
    /// The file as it was last read or written by Bracket.
    pub stamp: Option<DiskStamp>,
    /// When to check the file (a change was reported and the burst should be over).
    pub check_at: Option<Instant>,
    pub checking: Option<Receiver<io::Result<DiskState>>>,
    pub notice: Option<DiskNotice>,
}

impl DiskWatch {
    /// A fresh link after the file was read or written.
    pub fn synced(stamp: Option<DiskStamp>) -> Self {
        Self {
            stamp,
            ..Self::default()
        }
    }
}

/// What the user picked in the notice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeAction {
    /// Take the file's contents (undo brings back the user's text).
    Reload,
    /// Keep the text here; the next save overwrites the file.
    Keep,
}

/// Draws the notice at the top of the editor area.
pub fn notice_bar(
    ui: &mut egui::Ui,
    notice: &DiskNotice,
    name: &str,
    texts: &Texts,
) -> Option<NoticeAction> {
    let mut picked = None;
    let visuals = ui.visuals().clone();
    egui::Frame::new()
        .fill(visuals.extreme_bg_color)
        .stroke(egui::Stroke::new(1.0, visuals.warn_fg_color))
        .inner_margin(egui::Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                match notice {
                    DiskNotice::Changed { .. } => {
                        ui.colored_label(visuals.warn_fg_color, texts.disk_changed(name));
                        if ui
                            .button(texts.disk_reload())
                            .on_hover_text(texts.disk_reload_tooltip())
                            .clicked()
                        {
                            picked = Some(NoticeAction::Reload);
                        }
                        if ui.button(texts.disk_keep()).clicked() {
                            picked = Some(NoticeAction::Keep);
                        }
                    }
                    DiskNotice::Missing => {
                        ui.colored_label(visuals.warn_fg_color, texts.disk_missing(name));
                        if ui.button(texts.disk_dismiss()).clicked() {
                            picked = Some(NoticeAction::Keep);
                        }
                    }
                };
            });
        });
    picked
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("bracket-ui-disk-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn checks_read_the_file_only_when_its_stamp_changed() {
        let dir = temp_dir("check");
        let path = dir.join("a.txt");
        fs::write(&path, "one").unwrap();
        let stamp = DiskStamp::of(&path).unwrap();
        assert!(matches!(
            check(&path, None, Some(stamp)).unwrap(),
            DiskState::Unchanged
        ));

        fs::write(&path, "two!").unwrap();
        match check(&path, None, Some(stamp)).unwrap() {
            DiskState::Changed { loaded, stamp: new } => {
                assert_eq!(loaded.text, "two!");
                assert_ne!(new, stamp);
            }
            _ => panic!("expected a change"),
        }

        fs::remove_file(&path).unwrap();
        assert!(matches!(
            check(&path, None, Some(stamp)).unwrap(),
            DiskState::Missing
        ));
        fs::remove_dir_all(&dir).unwrap();
    }
}
