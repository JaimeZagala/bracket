//! Language loading (built-in and external) and incremental, line-based syntax highlighting.

pub mod languages;

pub use languages::{Badge, Language, LanguageRegistry};
