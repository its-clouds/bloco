//! A shell process in a PTY, with its output parsed by `alacritty_terminal`.

use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::selection::Selection;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, Term, TermDamage};
use alacritty_terminal::vte::ansi::{Color, CursorShape, Processor};
use anyhow::{Context, Result};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};

use std::ops::Range;

use crate::blocks::{self, Block, Blocks, Row};
use crate::cwd::pid_cwd;
use crate::links;
use crate::osc133::{Mark, Scanner};
use crate::search::{self, BufferPoint, FindQuery, FindResults};
use crate::shell::{self, ScriptDir};

pub use alacritty_terminal::index::Side;
pub use alacritty_terminal::selection::SelectionType as SelectionKind;
pub use alacritty_terminal::term::TermMode;

/// Lines kept above the screen. Once this many exist, the oldest line goes for
/// each new one, so the lines that block positions refer to move. See
/// [`buffer_point`].
const SCROLLBACK_LINES: usize = 10_000;

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
    /// The scripts that start the shell with integration. Only kept so they are
    /// removed on drop.
    _scripts: Option<ScriptDir>,
}

struct State {
    term: Term<PtyResponder>,
    blocks: Blocks,
    scrollback: Scrollback,
}

/// Counts the lines that leave the top of a full scrollback, so block positions
/// can follow the lines they point at.
///
/// Alacritty drops those lines without a count. While counting, the view offset
/// of the primary screen is 1 plus the lines scrolled into history, because
/// alacritty adds to a non-zero offset on each scroll. The offset is read at
/// every escape sequence and mark, which is when the lines are needed.
#[derive(Default)]
struct Scrollback {
    /// The offset the user had before counting. `Some` while counting.
    user_offset: Option<usize>,
    /// The history size when the current count began.
    start_history: usize,
    /// Lines dropped before the current count began.
    base: usize,
    /// Lines dropped so far, as far as the block positions know.
    applied: usize,
}

impl Scrollback {
    /// Starts counting on the primary screen, if it has history and is not counting.
    fn start(&mut self, term: &mut Term<PtyResponder>) {
        if self.user_offset.is_some() || term.mode().contains(TermMode::ALT_SCREEN) {
            return;
        }
        let history = term.grid().history_size();
        if history == 0 {
            return;
        }
        self.user_offset = Some(term.grid().display_offset());
        self.start_history = history;
        self.base = self.applied;
        set_offset(term, 1);
    }

    /// Moves the block positions by the lines dropped so far. Does nothing on
    /// the alternate screen, which is not counted.
    fn count(&mut self, term: &mut Term<PtyResponder>, blocks: &mut Blocks) {
        if self.user_offset.is_none() || term.mode().contains(TermMode::ALT_SCREEN) {
            return;
        }
        let offset = term.grid().display_offset();
        if offset + 1 >= SCROLLBACK_LINES {
            // The count hit alacritty's limit, so lines were lost. The positions
            // cannot be trusted, and counting starts again from here.
            blocks.forget_positions();
            set_offset(term, 1);
            self.base = self.applied;
            self.start_history = term.grid().history_size();
            return;
        }
        let pushed = offset.saturating_sub(1);
        // Until the scrollback is full, lines scrolled up only add to the history.
        let room = SCROLLBACK_LINES.saturating_sub(self.start_history);
        let dropped = self.base + pushed.saturating_sub(room);
        if dropped > self.applied {
            blocks.shift_up(dropped - self.applied);
            self.applied = dropped;
        }
    }

    /// Ends counting and gives the user's view offset back. Does nothing on the
    /// alternate screen: the count waits until the primary screen is active.
    fn stop(&mut self, term: &mut Term<PtyResponder>) {
        let (Some(user), false) = (
            self.user_offset,
            term.mode().contains(TermMode::ALT_SCREEN),
        ) else {
            return;
        };
        let pushed = term.grid().display_offset().saturating_sub(1);
        // Alacritty keeps a view that is scrolled up in place as lines arrive.
        // A view at the bottom stays there.
        let target = if user == 0 {
            0
        } else {
            (user + pushed).min(term.grid().history_size())
        };
        set_offset(term, target);
        self.user_offset = None;
    }
}

/// Moves the primary screen's view offset to `offset`.
fn set_offset(term: &mut Term<PtyResponder>, offset: usize) {
    let delta = offset as i32 - term.grid().display_offset() as i32;
    term.scroll_display(Scroll::Delta(delta));
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

/// What a pane draws in one frame: visual rows from the top, each with its
/// cells. A plain frame has one row per viewport line. With blocks, a header
/// row replaces a finished command's prompt and command lines.
pub struct Frame {
    /// One entry per visual row, top first.
    pub rows: Vec<Row>,
    pub columns: usize,
    /// `rows.len() * columns` cells, row-major. The `line` of each cell is its
    /// visual row. Cells of a header row are blank.
    pub cells: Vec<SnapshotCell>,
    /// `line` is the visual row of the cursor. `visible` is false when the
    /// cursor's line is not drawn.
    pub cursor: Cursor,
    /// The blocks whose header is in `rows`.
    pub headers: Vec<Block>,
    /// The block whose output fills the top row, so its header is stuck there.
    pub sticky: Option<Block>,
    /// The visual rows of the selected block (its header and output), if it is
    /// on screen and finished.
    pub selected: Option<Range<usize>>,
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
    /// Starts the user's login shell in a new PTY. zsh, bash and fish get the
    /// OSC 133 integration; other shells start as plain login shells.
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
        Self::spawn_shell(&login_shell(), cols, rows, cwd, listener)
    }

    /// Like [`Session::spawn_in`], for the shell at path `shell`.
    pub(crate) fn spawn_shell(
        shell: &str,
        cols: u16,
        rows: u16,
        cwd: Option<&Path>,
        listener: impl Fn(SessionEvent) + Send + Sync + 'static,
    ) -> Result<Self> {
        let listener: Listener = Arc::new(listener);
        let pair = native_pty_system()
            .openpty(pty_size(cols, rows))
            .context("openpty")?;

        let mut command = CommandBuilder::new(shell);
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
        let scripts = shell::configure(shell, &mut command)?;

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
            Config {
                scrolling_history: SCROLLBACK_LINES,
                ..Config::default()
            },
            &TermSize::new(cols as usize, rows as usize),
            PtyResponder {
                writer: writer.clone(),
                listener: listener.clone(),
            },
        );
        let state = Arc::new(Mutex::new(State {
            term,
            blocks: Blocks::default(),
            scrollback: Scrollback::default(),
        }));
        spawn_reader(reader, state.clone(), listener);

        Ok(Self {
            master: pair.master,
            writer,
            child,
            pid,
            state,
            _scripts: scripts,
        })
    }

    /// The shell's working directory. Uses the last OSC 7 report from the
    /// shell. Without one (no integration), asks the OS for the directory of
    /// the shell process.
    pub fn cwd(&self) -> Option<PathBuf> {
        let reported = self
            .state
            .lock()
            .expect("state lock poisoned")
            .blocks
            .cwd()
            .cloned();
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
    /// A width change rewraps the buffer, so the blocks forget their positions.
    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        self.master.resize(pty_size(cols, rows))?;
        let mut guard = self.state.lock().expect("state lock poisoned");
        let State {
            term,
            blocks,
            scrollback,
        } = &mut *guard;
        if cols as usize != term.columns() {
            scrollback.stop(term);
            blocks.forget_positions();
        }
        term.resize(TermSize::new(cols as usize, rows as usize));
        Ok(())
    }

    /// Returns the visible grid and damage since the last call. Only one consumer
    /// should call this, because it resets the damage.
    pub fn snapshot(&self) -> Snapshot {
        let mut guard = self.state.lock().expect("state lock poisoned");
        take_snapshot(&mut guard.term)
    }

    /// What to draw this frame. With command blocks on screen, the rows follow
    /// the blocks (see [`Row`]). On the alternate screen, or with no blocks, the
    /// frame is the plain grid, one row per viewport line. Resets damage, like
    /// [`Session::snapshot`].
    pub fn frame(&self, selected: Option<u64>) -> Frame {
        let mut guard = self.state.lock().expect("state lock poisoned");
        let State { term, blocks, .. } = &mut *guard;
        let all = blocks.all();
        if term.mode().contains(TermMode::ALT_SCREEN) || all.is_empty() {
            let snapshot = take_snapshot(term);
            return Frame {
                rows: (0..snapshot.rows)
                    .map(|line| Row::Line(snapshot.top_line + line))
                    .collect(),
                columns: snapshot.columns,
                cells: snapshot.cells,
                cursor: snapshot.cursor,
                headers: Vec::new(),
                sticky: None,
                selected: None,
            };
        }

        let history = term.grid().history_size();
        let screen = term.screen_lines();
        let top = history - term.grid().display_offset();
        let rows = blocks::frame_rows(all, top, history + screen, screen);
        let columns = term.columns();
        let content = term.renderable_content();
        let selection = content.selection;

        let mut cells = Vec::with_capacity(rows.len() * columns);
        let mut headers = Vec::new();
        for (line, row) in rows.iter().enumerate() {
            match *row {
                Row::Line(buffer_line) => {
                    let grid_line = Line(buffer_line as i32 - history as i32);
                    for column in 0..columns {
                        let cell = &term.grid()[grid_line][Column(column)];
                        cells.push(SnapshotCell {
                            line,
                            column,
                            c: cell.c,
                            zerowidth: cell.zerowidth().map(Box::from),
                            fg: cell.fg,
                            bg: cell.bg,
                            flags: cell.flags,
                            selected: selection
                                .is_some_and(|s| s.contains(Point::new(grid_line, Column(column)))),
                        });
                    }
                }
                Row::Header(id) => {
                    headers.extend(all.iter().find(|block| block.id == id).cloned());
                    cells.extend((0..columns).map(|column| blank_cell(line, column)));
                }
            }
        }

        let cursor_line = history + content.cursor.point.line.0 as usize;
        let cursor_row = rows.iter().position(|row| *row == Row::Line(cursor_line));
        let cursor = Cursor {
            line: cursor_row.unwrap_or(0),
            column: content.cursor.point.column.0,
            visible: cursor_row.is_some() && content.cursor.shape != CursorShape::Hidden,
            shape: content.cursor.shape,
        };
        let sticky = match rows.first() {
            Some(Row::Line(line)) => blocks::block_containing_output(all, *line).cloned(),
            _ => None,
        };

        let selected = selected.and_then(|id| selected_rows(all, &rows, id));
        Frame {
            rows,
            columns,
            cells,
            cursor,
            headers,
            sticky,
            selected,
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

    /// Scrolls so buffer line `line` is the top line of the view, as far as the
    /// scrollback allows.
    pub fn scroll_top_to(&self, line: usize) {
        let mut state = self.state.lock().expect("state lock poisoned");
        let term = &mut state.term;
        let history = term.grid().history_size();
        let wanted = history.saturating_sub(line) as i32;
        let offset = term.grid().display_offset() as i32;
        term.scroll_display(Scroll::Delta(wanted - offset));
    }

    /// A handle that runs find queries on this session's buffer. It can be sent
    /// to a background thread.
    pub fn finder(&self) -> Finder {
        Finder {
            state: self.state.clone(),
        }
    }

    /// Starts a selection at a buffer cell (see [`BufferPoint`](crate::BufferPoint)).
    pub fn start_selection(&self, kind: SelectionKind, line: usize, column: usize, side: Side) {
        let mut state = self.state.lock().expect("state lock poisoned");
        let point = grid_point(&state.term, line, column);
        state.term.selection = Some(Selection::new(kind, point, side));
    }

    /// Extends the selection to a buffer cell. Does nothing without a selection.
    pub fn update_selection(&self, line: usize, column: usize, side: Side) {
        let mut state = self.state.lock().expect("state lock poisoned");
        let point = grid_point(&state.term, line, column);
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

    /// The URL under a buffer cell, if there is one.
    pub fn url_at(&self, line: usize, column: usize) -> Option<links::Link> {
        let state = self.state.lock().expect("state lock poisoned");
        let term = &state.term;
        let grid_line = grid_line(term, line);
        let row: Vec<_> = (0..term.columns())
            .map(|c| {
                let cell = &term.grid()[grid_line][Column(c)];
                (cell.c, cell.flags)
            })
            .collect();
        links::link_at(&row, column)
    }

    /// The buffer line at the top of the view. Its frame may start above it,
    /// when headers and hidden output make the rows fill upward.
    pub fn top_line(&self) -> usize {
        let state = self.state.lock().expect("state lock poisoned");
        state.term.grid().history_size() - state.term.grid().display_offset()
    }

    /// The last block whose prompt is above buffer line `line`.
    pub fn previous_block(&self, line: usize) -> Option<Block> {
        let state = self.state.lock().expect("state lock poisoned");
        state
            .blocks
            .all()
            .iter()
            .rev()
            .find(|b| b.prompt_line.is_some_and(|p| p < line))
            .cloned()
    }

    /// The first block whose prompt is below buffer line `line`.
    pub fn next_block(&self, line: usize) -> Option<Block> {
        let state = self.state.lock().expect("state lock poisoned");
        state
            .blocks
            .all()
            .iter()
            .find(|b| b.prompt_line.is_some_and(|p| p > line))
            .cloned()
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

    /// Shows or hides the output of block `id`. Returns `false` for an unknown id.
    pub fn toggle_block(&self, id: u64) -> bool {
        let mut state = self.state.lock().expect("state lock poisoned");
        state.blocks.toggle_collapsed(id)
    }

    /// The command line of block `id`, as the shell reported it.
    pub fn block_command(&self, id: u64) -> Option<String> {
        let state = self.state.lock().expect("state lock poisoned");
        state
            .blocks
            .all()
            .iter()
            .find(|b| b.id == id)?
            .command
            .clone()
    }

    /// The text of block `id`'s output: its lines without trailing spaces, with
    /// wrapped rows joined. `None` for an unknown block or one without output
    /// lines (still running, or on the alternate screen).
    pub fn block_output(&self, id: u64) -> Option<String> {
        let state = self.state.lock().expect("state lock poisoned");
        let block = state.blocks.all().iter().find(|b| b.id == id)?;
        let term = &state.term;
        let start = block.output_line?;
        let end = block
            .end_line?
            .min(term.grid().history_size() + term.screen_lines());
        let mut text = String::new();
        for line in start..end {
            let grid_line = grid_line(term, line);
            let row = &term.grid()[grid_line];
            let mut row_text = String::new();
            for column in 0..term.columns() {
                let cell = &row[Column(column)];
                if !cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                    row_text.push(cell.c);
                    if let Some(extra) = cell.zerowidth() {
                        row_text.extend(extra.iter());
                    }
                }
            }
            text.push_str(row_text.trim_end());
            if !row[Column(term.columns() - 1)]
                .flags
                .contains(Flags::WRAPLINE)
            {
                text.push('\n');
            }
        }
        Some(text.trim_end_matches('\n').to_string())
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
    }
}

/// The visible grid as a snapshot. See [`Session::snapshot`].
fn take_snapshot(term: &mut Term<PtyResponder>) -> Snapshot {
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

/// The visual rows that belong to block `id`: its header row, and the rows
/// of its output. `None` for a block that is unknown, still running, or not in
/// `rows`.
fn selected_rows(blocks: &[Block], rows: &[Row], id: u64) -> Option<Range<usize>> {
    let block = blocks.iter().find(|b| b.id == id)?;
    let (start, end) = (block.prompt_line?, block.end_line?);
    let mut matching = rows.iter().enumerate().filter_map(|(index, row)| {
        let inside = match row {
            Row::Header(header) => *header == id,
            Row::Line(line) => (start..end).contains(line),
        };
        inside.then_some(index)
    });
    let first = matching.next()?;
    let last = matching.last().unwrap_or(first);
    Some(first..last + 1)
}

/// A cell with no text, drawn as the background.
fn blank_cell(line: usize, column: usize) -> SnapshotCell {
    SnapshotCell {
        line,
        column,
        c: ' ',
        zerowidth: None,
        fg: Color::Named(alacritty_terminal::vte::ansi::NamedColor::Foreground),
        bg: Color::Named(alacritty_terminal::vte::ansi::NamedColor::Background),
        flags: Flags::empty(),
        selected: false,
    }
}

/// The grid line of a buffer line. Grid lines are 0 at the top of the screen
/// and negative in the history.
fn grid_line(term: &Term<PtyResponder>, line: usize) -> Line {
    Line(line as i32 - term.grid().history_size() as i32)
}

fn grid_point(term: &Term<PtyResponder>, line: usize, column: usize) -> Point {
    Point::new(grid_line(term, line), Column(column))
}

/// The cursor's buffer point. `None` on the alternate screen, which has no
/// buffer of its own to refer to.
fn buffer_point(term: &Term<PtyResponder>) -> Option<BufferPoint> {
    if term.mode().contains(TermMode::ALT_SCREEN) {
        return None;
    }
    let cursor = term.grid().cursor.point;
    Some(BufferPoint {
        line: term.grid().history_size() + cursor.line.0 as usize,
        column: cursor.column.0,
    })
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
                        {
                            let mut guard = state.lock().expect("state lock poisoned");
                            process(&mut guard, &mut parser, &mut scanner, &buf[..n]);
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

/// Parses one read of PTY output into the terminal and the block list.
///
/// Each mark is applied with the grid as it is at the mark, so the bytes before
/// it are parsed first. The bytes are also split at each escape, so the count of
/// dropped lines is taken before anything that could scroll the screen.
fn process(state: &mut State, parser: &mut Processor, scanner: &mut Scanner, bytes: &[u8]) {
    const ESC: u8 = 0x1b;

    let mut marks = Vec::new();
    scanner.scan(bytes, |mark: Mark, end| marks.push((mark, end)));

    let State {
        term,
        blocks,
        scrollback,
    } = state;
    scrollback.start(term);
    let mut marks = marks.into_iter().peekable();
    let mut parsed = 0;
    for (index, &byte) in bytes.iter().enumerate() {
        while let Some((mark, end)) = marks.next_if(|(_, end)| *end <= index) {
            parser.advance(term, &bytes[parsed..end]);
            parsed = end;
            scrollback.count(term, blocks);
            blocks.apply(mark, buffer_point(term));
        }
        if byte == ESC {
            parser.advance(term, &bytes[parsed..index]);
            parsed = index;
            scrollback.count(term, blocks);
        }
    }
    for (mark, end) in marks {
        parser.advance(term, &bytes[parsed..end]);
        parsed = end;
        scrollback.count(term, blocks);
        blocks.apply(mark, buffer_point(term));
    }
    parser.advance(term, &bytes[parsed..]);
    scrollback.count(term, blocks);
    scrollback.stop(term);
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
