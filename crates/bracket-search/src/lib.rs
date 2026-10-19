//! Cancellable regex find and replace in a file and across a folder, with streamed results.

pub mod find;
pub mod folder;

pub use find::{FindError, FindOptions, Finder, Matches, Replacement};
pub use folder::{
    FileMatches, FileOutcome, FileReplace, FolderQuery, LineMatch, Summary, replace_in_file,
    search_folder,
};
