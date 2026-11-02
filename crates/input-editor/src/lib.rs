//! The input editor at the prompt: a multi-line text buffer with undo, shell
//! history and syntax highlighting. No UI code here. The app crate draws the
//! buffer and turns keys into calls on it.

mod buffer;
mod highlight;
mod history;
mod layout;

pub use buffer::Buffer;
pub use highlight::{highlight, Kind, Span};
pub use history::{History, HistorySearch, Walk};
pub use layout::{position, wrap};
