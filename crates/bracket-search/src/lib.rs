//! Cancellable regex find and replace in a file and across a folder, with streamed results.

pub mod find;

pub use find::{FindError, FindOptions, Finder, Matches};
