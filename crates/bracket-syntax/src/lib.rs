//! Language loading (built-in and external) and incremental, line-based syntax highlighting.

pub mod highlight;
pub mod languages;

pub use highlight::{HighlightWorker, Highlights, Span};
pub use languages::{Badge, Language, LanguageRegistry};
