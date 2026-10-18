//! Text buffer, multiple selections, undo/redo and document model. No UI, no platform APIs.
//!
//! Positions are char indices (Unicode scalar values), the native unit of `ropey`.

pub mod buffer;
pub mod document;
pub mod encoding;
pub mod history;
pub mod movement;
pub mod selection;
pub mod task;
pub mod transaction;

pub use buffer::Buffer;
pub use document::{Document, LineEnding};
pub use movement::Motion;
pub use ropey::Rope;
pub use selection::{Selection, Selections};
pub use task::{CancelToken, Generations, Task};
pub use transaction::{Change, Transaction};
