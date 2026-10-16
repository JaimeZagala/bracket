//! Where Bracket keeps its data: next to the executable in portable mode, or in the user's
//! application data folder otherwise.
//!
//! The caller passes in the executable's folder and the `%APPDATA%` folder, so this module does
//! not depend on the environment and can be tested anywhere.

use std::path::{Path, PathBuf};

use crate::session::SessionFiles;

/// A file with this name next to the executable turns on portable mode.
pub const PORTABLE_MARKER: &str = "portable.toml";
/// Configuration file name inside the data folder.
pub const CONFIG_FILE: &str = "config.toml";
/// Session file name inside the data folder.
pub const SESSION_FILE: &str = "session.toml";
/// Folder for the text of documents with unsaved changes, inside the data folder.
pub const BACKUP_DIR: &str = "backups";
/// Folder created inside `%APPDATA%` in installed mode.
pub const APP_DIR_NAME: &str = "Bracket";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Everything lives next to the executable; nothing is written to `%APPDATA%` or the registry.
    Portable,
    /// Data lives in `%APPDATA%\Bracket`.
    Installed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paths {
    pub mode: Mode,
    /// Folder for configuration, session and user languages.
    pub data_dir: PathBuf,
}

impl Paths {
    /// Portable mode if `portable.toml` exists in `exe_dir`; otherwise `%APPDATA%\Bracket`.
    /// Without an application data folder (which should not happen on Windows), falls back to
    /// portable mode so data is never written to an unexpected place.
    pub fn resolve(exe_dir: &Path, app_data: Option<&Path>) -> Self {
        let portable = exe_dir.join(PORTABLE_MARKER).is_file();
        match app_data {
            Some(app_data) if !portable => Self {
                mode: Mode::Installed,
                data_dir: app_data.join(APP_DIR_NAME),
            },
            _ => Self {
                mode: Mode::Portable,
                data_dir: exe_dir.to_path_buf(),
            },
        }
    }

    pub fn config_file(&self) -> PathBuf {
        self.data_dir.join(CONFIG_FILE)
    }

    pub fn session_files(&self) -> SessionFiles {
        SessionFiles {
            file: self.data_dir.join(SESSION_FILE),
            backup_dir: self.data_dir.join(BACKUP_DIR),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bracket-paths-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn installed_mode_uses_app_data() {
        let exe_dir = temp_dir("installed");
        let paths = Paths::resolve(&exe_dir, Some(Path::new(r"C:\Users\u\AppData\Roaming")));
        assert_eq!(paths.mode, Mode::Installed);
        assert_eq!(
            paths.config_file(),
            Path::new(r"C:\Users\u\AppData\Roaming\Bracket\config.toml")
        );
        fs::remove_dir_all(&exe_dir).unwrap();
    }

    #[test]
    fn marker_file_turns_on_portable_mode() {
        let exe_dir = temp_dir("portable");
        fs::write(exe_dir.join(PORTABLE_MARKER), "").unwrap();
        let paths = Paths::resolve(&exe_dir, Some(Path::new(r"C:\AppData")));
        assert_eq!(paths.mode, Mode::Portable);
        assert_eq!(paths.config_file(), exe_dir.join(CONFIG_FILE));
        fs::remove_dir_all(&exe_dir).unwrap();
    }

    #[test]
    fn a_folder_named_like_the_marker_does_not_count() {
        let exe_dir = temp_dir("marker-dir");
        fs::create_dir(exe_dir.join(PORTABLE_MARKER)).unwrap();
        let paths = Paths::resolve(&exe_dir, Some(Path::new(r"C:\AppData")));
        assert_eq!(paths.mode, Mode::Installed);
        fs::remove_dir_all(&exe_dir).unwrap();
    }

    #[test]
    fn missing_app_data_falls_back_to_portable() {
        let exe_dir = temp_dir("no-appdata");
        let paths = Paths::resolve(&exe_dir, None);
        assert_eq!(paths.mode, Mode::Portable);
        fs::remove_dir_all(&exe_dir).unwrap();
    }
}
