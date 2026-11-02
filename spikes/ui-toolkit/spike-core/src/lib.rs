//! Shared core for the UI toolkit spike. Throwaway code, not product code.
//! See `spikes/ui-toolkit/README.md`.

mod blocks;
mod osc133;
mod session;
mod timing;

pub use alacritty_terminal::term::cell::Flags;
pub use alacritty_terminal::vte::ansi::{Color, CursorShape};
pub use blocks::{Block, Blocks};
pub use osc133::{parse_payload, Mark, Scanner};
pub use session::{Cursor, Damage, LineDamage, Session, Snapshot, SnapshotCell};
pub use timing::StartupTimer;
