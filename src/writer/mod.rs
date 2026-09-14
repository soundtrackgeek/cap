//! Draft-backed interactive writer implementation.

pub mod buffer;
pub mod draft;
pub mod editor;
pub mod input;
pub mod terminal;

pub use buffer::{BufferError, TextBuffer, MAX_BUFFER_BYTES};
pub use draft::{DraftError, DraftLease};
pub use input::{InputEvent, Key, WriterAction, WriterMachine};
