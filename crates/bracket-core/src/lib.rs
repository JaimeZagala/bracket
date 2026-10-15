//! Text buffer, multiple selections, undo/redo and document model. No UI, no platform APIs.
//!
//! Positions are char indices (Unicode scalar values), the native unit of `ropey`.

pub mod buffer;
pub mod selection;

pub use buffer::Buffer;
pub use selection::{Selection, Selections};
