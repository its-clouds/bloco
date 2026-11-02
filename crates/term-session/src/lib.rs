//! The session layer of Bloco: a shell in a PTY, its VT state as
//! `alacritty_terminal` grid snapshots, and OSC 133 command blocks.
//!
//! No UI code here. The app crate draws the snapshots and sends input.

mod blocks;
mod cwd;
mod links;
mod osc133;
mod search;
mod session;
mod timing;

pub use alacritty_terminal::term::cell::Flags;
pub use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor};
pub use blocks::{Block, Blocks};
pub use links::Link;
pub use osc133::{parse_payload, Mark, Scanner};
pub use search::{BufferPoint, FindMatch, FindQuery, FindResults, MAX_MATCHES};
pub use session::{
    Cursor, Damage, Finder, LineDamage, Session, SessionEvent, Side, SelectionKind, Snapshot,
    SnapshotCell, TermMode,
};
pub use timing::StartupTimer;
