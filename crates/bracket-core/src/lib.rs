//! Text buffer, multiple selections, undo/redo and document model. No UI, no platform APIs.
//!
//! Positions are char indices (Unicode scalar values), the native unit of `ropey`.

pub mod buffer;
pub mod history;
pub mod movement;
pub mod selection;
pub mod transaction;

pub use buffer::Buffer;
pub use selection::{Selection, Selections};
pub use transaction::{Change, Transaction};
