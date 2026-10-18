//! Project walking, symbol extraction, incremental in-memory index and fuzzy search.

pub mod walk;

pub use walk::{BadPattern, Cancelled, Entry, FileIndex, WalkOptions, list_dir, walk};
