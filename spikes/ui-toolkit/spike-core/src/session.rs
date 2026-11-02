//! A zsh process in a PTY, with its output parsed by `alacritty_terminal`.

use std::io::{ErrorKind, Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, Term, TermDamage};
use alacritty_terminal::vte::ansi::{Color, CursorShape, Processor};
use anyhow::{Context, Result};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};

use crate::blocks::{Block, Blocks};
use crate::osc133::Scanner;

const INTEGRATION: &str = include_str!("../shell/integration.zsh");

type SharedWriter = Arc<Mutex<Box<dyn Write + Send>>>;

pub struct Session {
    master: Box<dyn MasterPty + Send>,
    writer: SharedWriter,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    state: Arc<Mutex<State>>,
}

struct State {
    term: Term<PtyResponder>,
    blocks: Blocks,
}

/// Sends terminal replies (for example to cursor-position queries) back to the
/// PTY. `alacritty_terminal` calls this synchronously while it holds the state lock.
struct PtyResponder {
    writer: SharedWriter,
}

impl EventListener for PtyResponder {
    fn send_event(&self, event: Event) {
        if let Event::PtyWrite(text) = event {
            if let Ok(mut writer) = self.writer.lock() {
                let _ = writer.write_all(text.as_bytes());
                let _ = writer.flush();
            }
        }
    }
}

/// The visible grid at one moment.
pub struct Snapshot {
    pub columns: usize,
    pub rows: usize,
    /// Every visible cell, row-major, including blanks.
    pub cells: Vec<SnapshotCell>,
    pub cursor: Cursor,
    /// Lines changed since the previous snapshot. Taking a snapshot resets damage.
    pub damage: Damage,
}

#[derive(Debug, Clone, Copy)]
pub struct SnapshotCell {
    /// Viewport line, 0 at the top of the screen.
    pub line: usize,
    pub column: usize,
    pub c: char,
    pub fg: Color,
    pub bg: Color,
    pub flags: Flags,
}

#[derive(Debug, Clone, Copy)]
pub struct Cursor {
    pub line: usize,
    pub column: usize,
    pub visible: bool,
    pub shape: CursorShape,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Damage {
    Full,
    Lines(Vec<LineDamage>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineDamage {
    pub line: usize,
    pub left: usize,
    pub right: usize,
}

impl Snapshot {
    /// The text of each visible line, with trailing spaces removed.
    pub fn text_lines(&self) -> Vec<String> {
        let mut lines = vec![String::new(); self.rows];
        for cell in &self.cells {
            if !cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                lines[cell.line].push(cell.c);
            }
        }
        for line in &mut lines {
            line.truncate(line.trim_end().len());
        }
        lines
    }
}

impl Session {
    /// Starts `zsh` in a new PTY with the OSC 133 integration loaded.
    pub fn spawn(cols: u16, rows: u16) -> Result<Self> {
        let pair = native_pty_system()
            .openpty(pty_size(cols, rows))
            .context("openpty")?;

        let mut command = CommandBuilder::new("zsh");
        command.env("ZDOTDIR", zdotdir()?);
        command.env("TERM", "xterm-256color");
        let child = pair.slave.spawn_command(command).context("spawn zsh")?;
        drop(pair.slave);

        let reader = pair.master.try_clone_reader().context("pty reader")?;
        let writer: SharedWriter =
            Arc::new(Mutex::new(pair.master.take_writer().context("pty writer")?));

        let term = Term::new(
            Config::default(),
            &TermSize::new(cols as usize, rows as usize),
            PtyResponder {
                writer: writer.clone(),
            },
        );
        let state = Arc::new(Mutex::new(State {
            term,
            blocks: Blocks::default(),
        }));
        spawn_reader(reader, state.clone());

        Ok(Self {
            master: pair.master,
            writer,
            child,
            state,
        })
    }

    /// Sends bytes to the shell, for example keystrokes or a command line.
    pub fn write(&self, bytes: &[u8]) -> Result<()> {
        let mut writer = self.writer.lock().expect("writer lock poisoned");
        writer.write_all(bytes)?;
        writer.flush()?;
        Ok(())
    }

    /// Resizes the PTY (which sends SIGWINCH to the shell) and the terminal grid.
    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        self.master.resize(pty_size(cols, rows))?;
        let mut state = self.state.lock().expect("state lock poisoned");
        state
            .term
            .resize(TermSize::new(cols as usize, rows as usize));
        Ok(())
    }

    /// Returns the visible grid and damage since the last call. Only one consumer
    /// should call this, because it resets the damage.
    pub fn snapshot(&self) -> Snapshot {
        let mut guard = self.state.lock().expect("state lock poisoned");
        let term = &mut guard.term;

        let damage = match term.damage() {
            TermDamage::Full => Damage::Full,
            TermDamage::Partial(lines) => Damage::Lines(
                lines
                    .map(|l| LineDamage {
                        line: l.line,
                        left: l.left,
                        right: l.right,
                    })
                    .collect(),
            ),
        };
        term.reset_damage();

        let content = term.renderable_content();
        // Grid lines are relative to the bottom of the screen. Adding the
        // display offset maps them to viewport lines.
        let offset = content.display_offset as i32;
        let cells = content
            .display_iter
            .map(|indexed| SnapshotCell {
                line: (indexed.point.line.0 + offset) as usize,
                column: indexed.point.column.0,
                c: indexed.cell.c,
                fg: indexed.cell.fg,
                bg: indexed.cell.bg,
                flags: indexed.cell.flags,
            })
            .collect();
        let cursor = Cursor {
            line: (content.cursor.point.line.0 + offset) as usize,
            column: content.cursor.point.column.0,
            visible: content.cursor.shape != CursorShape::Hidden,
            shape: content.cursor.shape,
        };

        Snapshot {
            columns: term.columns(),
            rows: term.screen_lines(),
            cells,
            cursor,
            damage,
        }
    }

    /// Command blocks so far, oldest first. Closed blocks have an exit code.
    pub fn blocks(&self) -> Vec<Block> {
        self.state
            .lock()
            .expect("state lock poisoned")
            .blocks
            .all()
            .to_vec()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

fn spawn_reader(mut reader: Box<dyn Read + Send>, state: Arc<Mutex<State>>) {
    thread::Builder::new()
        .name("spike-core-pty-reader".into())
        .spawn(move || {
            let mut scanner = Scanner::default();
            let mut parser: Processor = Processor::new();
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let bytes = &buf[..n];
                        let mut guard = state.lock().expect("state lock poisoned");
                        let State { term, blocks } = &mut *guard;
                        scanner.feed(bytes, |mark| blocks.apply(mark));
                        parser.advance(term, bytes);
                    }
                    Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                    // EIO here means the shell exited and the PTY closed.
                    Err(_) => break,
                }
            }
        })
        .expect("spawn pty reader thread");
}

fn pty_size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    }
}

/// Writes a ZDOTDIR for this process. Its `.zshrc` sources the user's own
/// `~/.zshrc` and then the integration snippet.
fn zdotdir() -> Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("spike-core-{}", std::process::id()));
    std::fs::create_dir_all(&dir).context("create ZDOTDIR")?;
    let rc = format!("[[ -r \"$HOME/.zshrc\" ]] && source \"$HOME/.zshrc\"\n{INTEGRATION}");
    std::fs::write(dir.join(".zshrc"), rc).context("write .zshrc")?;
    Ok(dir)
}
