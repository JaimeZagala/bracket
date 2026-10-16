//! TOML configuration, portable mode, recoverable session, recent files and file watching.

pub mod config;
pub mod paths;
pub mod session;

pub use config::{Config, ConfigWarning, Language, LoadedConfig};
pub use paths::{Mode, Paths};
pub use session::{Session, SessionFiles, SessionTab, Unsaved, WindowState};
