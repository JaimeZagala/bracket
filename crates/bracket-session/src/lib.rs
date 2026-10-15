//! TOML configuration, portable mode, recoverable session, recent files and file watching.

pub mod config;
pub mod paths;

pub use config::{Config, ConfigWarning, Language, LoadedConfig};
pub use paths::{Mode, Paths};
