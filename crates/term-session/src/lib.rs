//! The session layer of Bloco: a shell in a PTY, its VT state as
//! `alacritty_terminal` grid snapshots, and OSC 133 command blocks.
//!
//! No UI code here. The app crate draws the snapshots and sends input.

mod blocks;
mod cwd;
mod links;
mod mode;
mod osc133;
mod reflow;
mod search;
mod session;
mod shell;
mod timing;

pub use alacritty_terminal::term::cell::Flags;
pub use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor};
pub use blocks::{Block, Blocks, Row};
pub use links::Link;
pub use mode::InputMode;
pub use osc133::{parse_payload, Mark, Scanner};
pub use search::{BufferPoint, FindMatch, FindQuery, FindResults, MAX_MATCHES};
pub use session::{
    Cursor, Damage, Finder, Frame, LineDamage, Session, SessionEvent, Side, SelectionKind,
    Snapshot, SnapshotCell, TermMode,
};
pub use timing::StartupTimer;
