//! TOML configuration, portable mode, recoverable session, recent files and file watching.

pub mod config;
pub mod paths;
pub mod recent;
pub mod requests;
pub mod session;
pub mod watch;

pub use config::{Appearance, Config, ConfigWarning, Language, LoadedConfig};
pub use paths::{Mode, Paths};
pub use recent::Recent;
pub use requests::OpenRequest;
pub use session::{Session, SessionFiles, SessionTab, Unsaved};
pub use watch::FileWatcher;
