//! A shell process in a PTY, with its output parsed by `alacritty_terminal`.

use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::selection::Selection;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{viewport_to_point, Config, Term, TermDamage};
use alacritty_terminal::vte::ansi::{Color, CursorShape, Processor};
use anyhow::{Context, Result};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};

use crate::blocks::{Block, Blocks};
use crate::cwd::pid_cwd;
use crate::links;
use crate::osc133::{Mark, Scanner};
use crate::search::{self, FindQuery, FindResults};

pub use alacritty_terminal::index::Side;
pub use alacritty_terminal::selection::SelectionType as SelectionKind;
pub use alacritty_terminal::term::TermMode;

const INTEGRATION: &str = include_str!("../shell/integration.zsh");

type SharedWriter = Arc<Mutex<Box<dyn Write + Send>>>;
type Listener = Arc<dyn Fn(SessionEvent) + Send + Sync>;

/// Something the UI should know about. Called from the reader thread, so the
/// listener should only forward the event (for example into a channel).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEvent {
    /// New bytes were parsed. The grid may have changed.
    Output,
    /// The shell asked to change the window title (OSC 0 or 2).
    Title(String),
    /// The shell closed the PTY, usually because it exited.
    Exit,
    /// The shell sent text for the clipboard with OSC 52. The primary selection
    /// is written to the same clipboard. Read requests are not answered.
    Clipboard(String),
    /// The shell rang the bell (BEL, 0x07).
    Bell,
}

pub struct Session {
    master: Box<dyn MasterPty + Send>,
    writer: SharedWriter,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    /// The shell's process id, used to ask the OS for its directory.
    pid: Option<u32>,
    state: Arc<Mutex<State>>,
    /// The temporary ZDOTDIR written for zsh. Removed on drop.
    zdotdir: Option<PathBuf>,
}

struct State {
    term: Term<PtyResponder>,
    blocks: Blocks,
    /// The last directory the shell reported with OSC 7.
    cwd: Option<PathBuf>,
}

/// Sends terminal replies (for example to cursor-position queries) back to the
/// PTY, and title changes to the listener. `alacritty_terminal` calls this
/// synchronously while it holds the state lock.
struct PtyResponder {
    writer: SharedWriter,
    listener: Listener,
}

impl EventListener for PtyResponder {
    fn send_event(&self, event: Event) {
        match event {
            Event::PtyWrite(text) => {
                if let Ok(mut writer) = self.writer.lock() {
                    let _ = writer.write_all(text.as_bytes());
                    let _ = writer.flush();
                }
            }
            Event::Title(title) => (self.listener)(SessionEvent::Title(title)),
            Event::ClipboardStore(_, text) => (self.listener)(SessionEvent::Clipboard(text)),
            Event::Bell => (self.listener)(SessionEvent::Bell),
            _ => {}
        }
    }
}

/// The visible grid at one moment.
pub struct Snapshot {
    pub columns: usize,
    pub rows: usize,
    /// Every visible cell, row-major, including blanks. The cell for viewport
    /// line `l` and column `c` is at index `l * columns + c`.
    pub cells: Vec<SnapshotCell>,
    pub cursor: Cursor,
    /// Lines changed since the previous snapshot. Taking a snapshot resets damage.
    pub damage: Damage,
    /// The buffer line (see [`BufferPoint`](crate::BufferPoint)) shown on the
    /// first viewport line.
    pub top_line: usize,
}

#[derive(Debug, Clone)]
pub struct SnapshotCell {
    /// Viewport line, 0 at the top of the screen.
    pub line: usize,
    pub column: usize,
    pub c: char,
    /// Combining marks and the rest of a grapheme cluster (emoji ZWJ sequences,
    /// variation selectors). `None` for most cells.
    pub zerowidth: Option<Box<[char]>>,
    pub fg: Color,
    pub bg: Color,
    pub flags: Flags,
    /// Inside the current selection.
    pub selected: bool,
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

    /// The cells of one viewport line.
    pub fn line(&self, line: usize) -> &[SnapshotCell] {
        &self.cells[line * self.columns..(line + 1) * self.columns]
    }
}

impl Session {
    /// Starts the user's login shell in a new PTY. zsh gets the OSC 133
    /// integration; other shells start as plain login shells.
    pub fn spawn(cols: u16, rows: u16) -> Result<Self> {
        Self::spawn_in(cols, rows, None, |_| {})
    }

    /// Like [`Session::spawn`], and calls `listener` for each [`SessionEvent`].
    pub fn spawn_with_listener(
        cols: u16,
        rows: u16,
        listener: impl Fn(SessionEvent) + Send + Sync + 'static,
    ) -> Result<Self> {
        Self::spawn_in(cols, rows, None, listener)
    }

    /// Like [`Session::spawn_with_listener`], and starts the shell in `cwd`.
    /// Without `cwd` the shell starts in `$HOME`.
    pub fn spawn_in(
        cols: u16,
        rows: u16,
        cwd: Option<&Path>,
        listener: impl Fn(SessionEvent) + Send + Sync + 'static,
    ) -> Result<Self> {
        let listener: Listener = Arc::new(listener);
        let pair = native_pty_system()
            .openpty(pty_size(cols, rows))
            .context("openpty")?;

        let shell = login_shell();
        let mut command = CommandBuilder::new(&shell);
        command.arg("-l");
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        match cwd {
            Some(dir) => command.cwd(dir),
            None => {
                if let Some(home) = std::env::var_os("HOME") {
                    command.cwd(home);
                }
            }
        }

        let zdotdir = if is_zsh(&shell) {
            let dir = write_zdotdir()?;
            command.env("TERM_SESSION_USER_ZDOTDIR", user_zdotdir());
            command.env("ZDOTDIR", &dir);
            Some(dir)
        } else {
            None
        };

        let child = pair
            .slave
            .spawn_command(command)
            .with_context(|| format!("spawn {shell}"))?;
        let pid = child.process_id();
        drop(pair.slave);

        let reader = pair.master.try_clone_reader().context("pty reader")?;
        let writer: SharedWriter =
            Arc::new(Mutex::new(pair.master.take_writer().context("pty writer")?));

        let term = Term::new(
            Config::default(),
            &TermSize::new(cols as usize, rows as usize),
            PtyResponder {
                writer: writer.clone(),
                listener: listener.clone(),
            },
        );
        let state = Arc::new(Mutex::new(State {
            term,
            blocks: Blocks::default(),
            cwd: None,
        }));
        spawn_reader(reader, state.clone(), listener);

        Ok(Self {
            master: pair.master,
            writer,
            child,
            pid,
            state,
            zdotdir,
        })
    }

    /// The shell's working directory. Uses the last OSC 7 report from the
    /// shell. Without one (no integration), asks the OS for the directory of
    /// the shell process.
    pub fn cwd(&self) -> Option<PathBuf> {
        let reported = self.state.lock().expect("state lock poisoned").cwd.clone();
        reported.or_else(|| pid_cwd(self.pid?))
    }

    /// The command line that is running now, if one is. Taken from the OSC 133
    /// `C` mark. At the prompt this is `None`.
    pub fn current_command(&self) -> Option<String> {
        let state = self.state.lock().expect("state lock poisoned");
        state.blocks.running()?.command.clone()
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
        let selection = content.selection;
        let cells = content
            .display_iter
            .map(|indexed| SnapshotCell {
                line: (indexed.point.line.0 + offset) as usize,
                column: indexed.point.column.0,
                c: indexed.cell.c,
                zerowidth: indexed.cell.zerowidth().map(Box::from),
                fg: indexed.cell.fg,
                bg: indexed.cell.bg,
                flags: indexed.cell.flags,
                selected: selection.is_some_and(|s| s.contains(indexed.point)),
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
            top_line: term.grid().history_size() - term.grid().display_offset(),
        }
    }

    /// The terminal mode flags: alternate screen, mouse reporting, bracketed
    /// paste, application cursor keys and so on.
    pub fn mode(&self) -> TermMode {
        *self.state.lock().expect("state lock poisoned").term.mode()
    }

    /// Moves the view through the scrollback. Positive lines scroll up
    /// (towards older output). Has no effect on the alternate screen.
    pub fn scroll(&self, lines: i32) {
        let mut state = self.state.lock().expect("state lock poisoned");
        state.term.scroll_display(Scroll::Delta(lines));
    }

    /// Moves the view back to the newest output.
    pub fn scroll_to_bottom(&self) {
        let mut state = self.state.lock().expect("state lock poisoned");
        state.term.scroll_display(Scroll::Bottom);
    }

    /// Scrolls so a buffer line (see [`BufferPoint`](crate::BufferPoint)) is on
    /// screen. A line that is already visible does not move. Any other line is
    /// put in the middle of the screen, as far as the scrollback allows.
    pub fn scroll_to_line(&self, line: usize) {
        let mut state = self.state.lock().expect("state lock poisoned");
        let term = &mut state.term;
        let history = term.grid().history_size();
        let offset = term.grid().display_offset();
        let rows = term.screen_lines();
        let top = history - offset;
        if (top..top + rows).contains(&line) {
            return;
        }
        // The viewport starts at buffer line `history - offset`.
        let wanted = (history + rows / 2).saturating_sub(line).min(history);
        term.scroll_display(Scroll::Delta(wanted as i32 - offset as i32));
    }

    /// A handle that runs find queries on this session's buffer. It can be sent
    /// to a background thread.
    pub fn finder(&self) -> Finder {
        Finder {
            state: self.state.clone(),
        }
    }

    /// Starts a selection at a viewport cell. `line` and `column` are viewport
    /// coordinates, as in [`Snapshot`].
    pub fn start_selection(&self, kind: SelectionKind, line: usize, column: usize, side: Side) {
        let mut state = self.state.lock().expect("state lock poisoned");
        let point = viewport_point(&state.term, line, column);
        state.term.selection = Some(Selection::new(kind, point, side));
    }

    /// Extends the selection to a viewport cell. Does nothing without a selection.
    pub fn update_selection(&self, line: usize, column: usize, side: Side) {
        let mut state = self.state.lock().expect("state lock poisoned");
        let point = viewport_point(&state.term, line, column);
        if let Some(selection) = state.term.selection.as_mut() {
            selection.update(point, side);
        }
    }

    pub fn clear_selection(&self) {
        self.state.lock().expect("state lock poisoned").term.selection = None;
    }

    /// The selected text, or `None` without a selection.
    pub fn selection_text(&self) -> Option<String> {
        self.state
            .lock()
            .expect("state lock poisoned")
            .term
            .selection_to_string()
    }

    /// The URL under a viewport cell, if there is one. Reads the grid directly,
    /// so it does not take a full snapshot.
    pub fn url_at(&self, line: usize, column: usize) -> Option<links::Link> {
        let state = self.state.lock().expect("state lock poisoned");
        let term = &state.term;
        let grid_line = Line(line as i32 - term.grid().display_offset() as i32);
        let row: Vec<_> = (0..term.columns())
            .map(|c| {
                let cell = &term.grid()[grid_line][Column(c)];
                (cell.c, cell.flags)
            })
            .collect();
        links::link_at(&row, column)
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

/// Runs find queries on a session's buffer, from any thread. See [`Session::finder`].
#[derive(Clone)]
pub struct Finder {
    state: Arc<Mutex<State>>,
}

impl Finder {
    /// Every match of `query` in the scrollback and on the screen, oldest first.
    /// Holds the terminal lock while it searches, so output waits until it is done.
    pub fn find(&self, query: &FindQuery) -> Result<FindResults, String> {
        let state = self.state.lock().expect("state lock poisoned");
        search::find_all(&state.term, query)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        if let Some(dir) = &self.zdotdir {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

fn viewport_point(term: &Term<PtyResponder>, line: usize, column: usize) -> Point {
    viewport_to_point(
        term.grid().display_offset(),
        Point::new(line, Column(column)),
    )
}

fn spawn_reader(mut reader: Box<dyn Read + Send>, state: Arc<Mutex<State>>, listener: Listener) {
    thread::Builder::new()
        .name("term-session-pty-reader".into())
        .spawn(move || {
            let mut scanner = Scanner::default();
            let mut parser: Processor = Processor::new();
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let bytes = &buf[..n];
                        {
                            let mut guard = state.lock().expect("state lock poisoned");
                            let State { term, blocks, cwd } = &mut *guard;
                            scanner.feed(bytes, |mark| match mark {
                                Mark::Cwd(path) => *cwd = Some(PathBuf::from(path)),
                                mark => blocks.apply(mark),
                            });
                            parser.advance(term, bytes);
                        }
                        listener(SessionEvent::Output);
                    }
                    Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                    // EIO here means the shell exited and the PTY closed.
                    Err(_) => break,
                }
            }
            listener(SessionEvent::Exit);
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

/// `$SHELL`, or `/bin/zsh` when it is unset.
fn login_shell() -> String {
    std::env::var("SHELL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/bin/zsh".into())
}

fn is_zsh(shell: &str) -> bool {
    Path::new(shell).file_name().is_some_and(|name| name == "zsh")
}

/// Where the user keeps their zsh files: `$ZDOTDIR`, or `$HOME`.
fn user_zdotdir() -> PathBuf {
    std::env::var_os("ZDOTDIR")
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// Writes a ZDOTDIR for this process. zsh reads the four startup files from
/// it. Each one sources the user's file of the same name first. `.zshrc`
/// then loads the integration and gives `ZDOTDIR` back to the user's directory,
/// so `.zlogin` and child shells see the normal setup.
fn write_zdotdir() -> Result<PathBuf> {
    // Unique per session: several sessions can exist in one process (tabs, splits).
    static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("term-session-{}-{id}", std::process::id()));
    std::fs::create_dir_all(&dir).context("create ZDOTDIR")?;

    let source = |name: &str| {
        format!("[[ -r \"$TERM_SESSION_USER_ZDOTDIR/{name}\" ]] && source \"$TERM_SESSION_USER_ZDOTDIR/{name}\"\n")
    };
    std::fs::write(dir.join(".zshenv"), source(".zshenv")).context("write .zshenv")?;
    std::fs::write(dir.join(".zprofile"), source(".zprofile")).context("write .zprofile")?;

    let rc = format!(
        "{}{INTEGRATION}\nexport ZDOTDIR=\"$TERM_SESSION_USER_ZDOTDIR\"\nunset TERM_SESSION_USER_ZDOTDIR\n",
        source(".zshrc")
    );
    std::fs::write(dir.join(".zshrc"), rc).context("write .zshrc")?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feeds raw PTY bytes through the same parser the reader thread uses, and
    /// returns the events the listener received.
    fn events_for(bytes: &[u8]) -> Vec<SessionEvent> {
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = events.clone();
        let listener: Listener = Arc::new(move |event| sink.lock().unwrap().push(event));
        let writer: SharedWriter = Arc::new(Mutex::new(Box::new(std::io::sink())));
        let mut term = Term::new(
            Config::default(),
            &TermSize::new(80, 24),
            PtyResponder { writer, listener },
        );
        let mut parser: Processor = Processor::new();
        parser.advance(&mut term, bytes);
        let events = events.lock().unwrap().clone();
        events
    }

    #[test]
    fn osc_52_store_reaches_the_listener_as_text() {
        // "Hello" in base64, ST-terminated.
        let events = events_for(b"\x1b]52;c;SGVsbG8=\x1b\\");
        assert_eq!(events, [SessionEvent::Clipboard("Hello".into())]);
    }

    #[test]
    fn osc_52_read_request_is_not_answered() {
        let events = events_for(b"\x1b]52;c;?\x07");
        assert!(events.is_empty());
    }

    #[test]
    fn bel_reaches_the_listener() {
        let events = events_for(b"a\x07b\x07");
        assert_eq!(events, [SessionEvent::Bell, SessionEvent::Bell]);
    }
}
