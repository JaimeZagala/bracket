//! Cancellable regex find and replace in a file and across a folder, with streamed results.

pub mod find;
pub mod folder;

pub use find::{FindError, FindOptions, Finder, Matches, Replacement};
pub use folder::{FileMatches, FolderQuery, LineMatch, Summary, search_folder};
