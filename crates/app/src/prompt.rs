//! The input editor at the prompt, as the app holds it: the buffer, the history
//! the pane walks, the Ctrl+R search, and what the element draws over the
//! shell's prompt. The editing rules are in the `input-editor` crate.
//!
//! The shell's own prompt (PS1) is drawn by the shell as before. The editor
//! starts where the shell's cursor sits after `OSC 133;B`. Nothing is sent to
//! the shell while the editor owns the keys, except the whole line on Enter.

use std::cell::Cell;
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use gpui::Hsla;
use input_editor::completion::{Item, Specs};
use input_editor::{Buffer, History, HistorySearch, Kind, Span, Walk, highlight, position, wrap};
use term_session::{Color, NamedColor};

use crate::colors::Palette;

/// What the element draws over the prompt: the editor text (or the search line),
/// its highlighting, and the cursor as a char index into `text`.
pub struct EditorDraw {
    pub text: String,
    pub spans: Vec<Span>,
    pub cursor: usize,
}

/// Rows the completion menu shows at once. The rest scroll with the selection.
pub const MENU_ROWS: usize = 8;

/// The completion menu under the editor: the candidates and the selected one.
/// It is only open with two or more candidates. A single one is inserted.
pub struct Menu {
    /// The char index where the word being completed starts.
    pub start: usize,
    pub items: Vec<Item>,
    pub selected: usize,
}

impl Menu {
    pub fn new(start: usize, items: Vec<Item>) -> Self {
        Self {
            start,
            items,
            selected: 0,
        }
    }

    /// Moves the selection down, and wraps from the last item to the first.
    pub fn next(&mut self) {
        self.selected = (self.selected + 1) % self.items.len();
    }

    /// Moves the selection up, and wraps from the first item to the last.
    pub fn previous(&mut self) {
        self.selected = (self.selected + self.items.len() - 1) % self.items.len();
    }

    /// The items on screen: a window of `MENU_ROWS` that holds the selection.
    pub fn visible(&self) -> Range<usize> {
        let first = (self.selected + 1).saturating_sub(MENU_ROWS);
        first..(first + MENU_ROWS).min(self.items.len())
    }
}

pub struct PromptState {
    pub buffer: Buffer,
    pub history: History,
    /// Up and Down through `history`.
    pub walk: Walk,
    /// The Ctrl+R search, while it is open.
    pub search: Option<HistorySearch>,
    /// Where the editor starts on screen: (line, column) of the shell's cursor
    /// in the last frame that drew the editor. `None` when it is not drawn.
    /// The element writes it, and the mouse handlers read it.
    pub origin: Rc<Cell<Option<(usize, usize)>>>,
    /// The specs that answer Tab. See `completion.rs`.
    pub specs: Arc<Specs>,
    /// The completion menu, while it is open.
    pub menu: Option<Menu>,
}

impl PromptState {
    pub fn new() -> Self {
        Self {
            buffer: Buffer::new(),
            history: load_history(),
            walk: Walk::default(),
            search: None,
            origin: Rc::new(Cell::new(None)),
            specs: Arc::new(input_editor::completion::builtin()),
            menu: None,
        }
    }

    /// What the editor shows now: the search line while Ctrl+R is open, else
    /// the buffer with its highlighting.
    pub fn draw(&self) -> EditorDraw {
        match &self.search {
            Some(search) => {
                let found = search.current(&self.history).unwrap_or("");
                let text = format!("(reverse-i-search)`{}': {found}", search.query());
                EditorDraw {
                    cursor: text.chars().count(),
                    text,
                    spans: Vec::new(),
                }
            }
            None => {
                let text = self.buffer.text();
                EditorDraw {
                    cursor: self.buffer.cursor(),
                    spans: highlight(&text),
                    text,
                }
            }
        }
    }
}

/// A stretch of editor chars that share one highlight kind. `start..end` are char
/// indices into the editor text. `None` is text with no span.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Run {
    pub start: usize,
    pub end: usize,
    pub kind: Option<Kind>,
}

/// Splits the chars in `range` into runs by highlight kind. Chars outside every
/// span are runs of `None`. The runs cover `range` in order, without gaps.
pub fn runs(spans: &[Span], range: Range<usize>) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    for index in range {
        let kind = spans
            .iter()
            .find(|span| (span.start..span.end).contains(&index))
            .map(|span| span.kind);
        match runs.last_mut() {
            Some(run) if run.kind == kind => run.end = index + 1,
            _ => runs.push(Run {
                start: index,
                end: index + 1,
                kind,
            }),
        }
    }
    runs
}

/// The colour of a highlighted span. Text with no kind uses the foreground.
pub fn color(kind: Option<Kind>, palette: &Palette) -> Hsla {
    match kind {
        Some(Kind::Command) | Some(Kind::Keyword) => palette.chrome.accent,
        Some(Kind::String) => palette.resolve(Color::Named(NamedColor::Green)),
        Some(Kind::Variable) => palette.resolve(Color::Named(NamedColor::Cyan)),
        Some(Kind::Option) | Some(Kind::Comment) => palette.chrome.muted,
        Some(Kind::Operator) | None => palette.foreground,
    }
}

/// The editor's cursor on screen, as (line, column). `origin` is where the
/// editor starts (see [`PromptState::origin`]). The text wraps at `columns` the
/// same way the element draws it.
pub fn cursor_cell(
    origin: (usize, usize),
    text: &str,
    cursor: usize,
    columns: usize,
) -> (usize, usize) {
    let chars: Vec<char> = text.chars().collect();
    let rows = wrap(&chars, columns.saturating_sub(origin.1), columns);
    let (row, column) = position(&rows, cursor);
    let at = if row == 0 { origin.1 + column } else { column };
    (origin.0 + row, at)
}

/// Whether a key types text into the editor. Enter, Tab and Backspace also carry
/// a `key_char` (GPUI sets `"\n"` for Enter), so a control character does not
/// count. Cmd and Ctrl chords are shortcuts, not text.
pub fn is_text_input(key_char: Option<&str>, control_or_platform: bool) -> bool {
    !control_or_platform
        && key_char.is_some_and(|text| !text.is_empty() && !text.chars().any(char::is_control))
}

/// The bytes that run a command line in the shell. A multi-line command is sent
/// as a bracketed paste when the shell asked for one, so its newlines stay in
/// the shell's line editor. Enter then runs the whole command.
pub fn submit_bytes(text: &str, bracketed: bool) -> Vec<u8> {
    let text = text.replace('\x1b', "");
    if text.contains('\n') && bracketed {
        format!("\x1b[200~{text}\x1b[201~\r").into_bytes()
    } else {
        format!("{}\r", text.replace('\n', "\r")).into_bytes()
    }
}

/// Reads the history of the user's shell: `$HISTFILE` if set, else the default
/// file of zsh or bash. Fish keeps its history in another format, so it starts
/// empty. A missing or unreadable file gives an empty history.
fn load_history() -> History {
    let Some(path) = history_path() else {
        return History::default();
    };
    match std::fs::read(path) {
        Ok(bytes) => History::from_file_text(&String::from_utf8_lossy(&bytes)),
        Err(_) => History::default(),
    }
}

fn history_path() -> Option<PathBuf> {
    if let Some(file) = std::env::var_os("HISTFILE") {
        return Some(file.into());
    }
    let home = PathBuf::from(std::env::var_os("HOME")?);
    let shell = std::env::var("SHELL").unwrap_or_default();
    if shell.ends_with("fish") {
        return None;
    }
    let name = if shell.ends_with("bash") {
        ".bash_history"
    } else {
        ".zsh_history"
    };
    Some(home.join(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use input_editor::{Kind, highlight};

    fn menu(count: usize) -> Menu {
        let items = (0..count)
            .map(|index| Item {
                text: format!("item{index}"),
                description: String::new(),
            })
            .collect();
        Menu::new(0, items)
    }

    #[test]
    fn menu_selection_wraps_both_ways() {
        let mut menu = menu(3);
        menu.next();
        menu.next();
        assert_eq!(menu.selected, 2);
        menu.next();
        assert_eq!(menu.selected, 0);
        menu.previous();
        assert_eq!(menu.selected, 2);
    }

    #[test]
    fn menu_shows_a_window_that_holds_the_selection() {
        let mut menu = menu(20);
        assert_eq!(menu.visible(), 0..MENU_ROWS);
        menu.selected = 12;
        assert_eq!(menu.visible(), 5..13);
        menu.selected = 19;
        assert_eq!(menu.visible(), 12..20);
    }

    #[test]
    fn menu_shows_every_item_when_they_fit() {
        assert_eq!(menu(3).visible(), 0..3);
    }

    #[test]
    fn cursor_cell_is_the_origin_plus_the_typed_text() {
        assert_eq!(cursor_cell((5, 2), "abc", 3, 80), (5, 5));
        assert_eq!(cursor_cell((5, 2), "abc", 1, 80), (5, 3));
    }

    #[test]
    fn cursor_cell_wraps_onto_the_next_row() {
        // The first row has 78 columns after the origin at column 2, so char 80
        // is on the second row, at column 2 of the editor's rows.
        let text = "x".repeat(90);
        assert_eq!(cursor_cell((5, 2), &text, 80, 80), (6, 2));
    }

    #[test]
    fn enter_tab_and_backspace_are_not_text() {
        // GPUI gives Enter the key_char "\n". It must reach the submit path.
        assert!(!is_text_input(Some("\n"), false));
        assert!(!is_text_input(Some("\t"), false));
        assert!(!is_text_input(Some("\x7f"), false));
        assert!(!is_text_input(None, false));
    }

    #[test]
    fn printable_keys_and_dead_key_text_are_text() {
        assert!(is_text_input(Some(" "), false));
        assert!(is_text_input(Some("a"), false));
        assert!(is_text_input(Some("ç"), false));
    }

    #[test]
    fn chords_are_not_text() {
        assert!(!is_text_input(Some("c"), true));
    }

    #[test]
    fn single_line_submit_ends_with_enter() {
        assert_eq!(submit_bytes("ls -la", true), b"ls -la\r".to_vec());
    }

    #[test]
    fn multi_line_submit_is_a_bracketed_paste_when_the_shell_asks() {
        assert_eq!(
            submit_bytes("echo 1\necho 2", true),
            b"\x1b[200~echo 1\necho 2\x1b[201~\r".to_vec()
        );
    }

    #[test]
    fn multi_line_submit_without_bracketed_paste_turns_newlines_into_enter() {
        assert_eq!(submit_bytes("a\nb", false), b"a\rb\r".to_vec());
    }

    #[test]
    fn escape_bytes_never_reach_the_shell_in_a_submit() {
        assert_eq!(submit_bytes("x\x1b[31m", true), b"x[31m\r".to_vec());
    }

    /// The runs the element draws for `text`, as (text, kind) pairs.
    fn drawn(text: &str) -> Vec<(String, Option<Kind>)> {
        let chars: Vec<char> = text.chars().collect();
        runs(&highlight(text), 0..chars.len())
            .into_iter()
            .map(|run| (chars[run.start..run.end].iter().collect(), run.kind))
            .collect()
    }

    #[test]
    fn highlight_colours_the_command_and_options_of_ls() {
        assert_eq!(
            drawn("ls -la"),
            vec![
                ("ls".to_string(), Some(Kind::Command)),
                (" ".to_string(), None),
                ("-la".to_string(), Some(Kind::Option)),
            ]
        );
    }

    #[test]
    fn highlight_keeps_arguments_plain_after_the_command_word() {
        // Only the command, its options and quoted strings get colour. `commit`
        // is a plain argument and keeps the foreground colour.
        assert_eq!(
            drawn("git commit -m \"msg\""),
            vec![
                ("git".to_string(), Some(Kind::Command)),
                (" commit ".to_string(), None),
                ("-m".to_string(), Some(Kind::Option)),
                (" ".to_string(), None),
                ("\"msg\"".to_string(), Some(Kind::String)),
            ]
        );
    }

    #[test]
    fn highlight_splits_pipelines_and_variables() {
        assert_eq!(
            drawn("echo $HOME | grep x"),
            vec![
                ("echo".to_string(), Some(Kind::Command)),
                (" ".to_string(), None),
                ("$HOME".to_string(), Some(Kind::Variable)),
                (" ".to_string(), None),
                ("|".to_string(), Some(Kind::Operator)),
                (" ".to_string(), None),
                ("grep".to_string(), Some(Kind::Command)),
                (" x".to_string(), None),
            ]
        );
    }

    #[test]
    fn runs_cover_a_range_without_gaps_or_overlap() {
        let spans = highlight("ls -la");
        assert_eq!(
            runs(&spans, 1..5),
            vec![
                Run {
                    start: 1,
                    end: 2,
                    kind: Some(Kind::Command)
                },
                Run {
                    start: 2,
                    end: 3,
                    kind: None
                },
                Run {
                    start: 3,
                    end: 5,
                    kind: Some(Kind::Option)
                },
            ]
        );
    }
}
