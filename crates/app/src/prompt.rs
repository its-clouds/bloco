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

use gpui::{Hsla, Pixels, Point, point};
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
    /// The selected chars, as indices into `text`. Drawn with the selection colour.
    pub selection: Option<Range<usize>>,
    /// The ghost text drawn after the editor text, not part of it.
    pub suggestion: Option<String>,
}

/// Rows the completion menu shows at once. The rest scroll with the selection.
pub const MENU_ROWS: usize = 8;

/// The completion menu over the dock's editor: the candidates and the selected one.
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

/// Where the dock's editor is on screen, in window pixels. The element writes
/// it on each frame that draws the editor, and the mouse handlers read it.
#[derive(Debug, Clone, Copy)]
pub struct EditorGeometry {
    /// The top-left corner of the editor's first cell.
    pub origin: Point<Pixels>,
    /// Cells per editor row: the grid's column count.
    pub columns: usize,
    pub cell_width: Pixels,
    pub line_height: Pixels,
}

pub struct PromptState {
    pub buffer: Buffer,
    pub history: History,
    /// Up and Down through `history`.
    pub walk: Walk,
    /// The Ctrl+R search, while it is open.
    pub search: Option<HistorySearch>,
    /// Where the editor is drawn, `None` when it is not. See [`EditorGeometry`].
    pub geometry: Rc<Cell<Option<EditorGeometry>>>,
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
            geometry: Rc::new(Cell::new(None)),
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
                    selection: None,
                    suggestion: None,
                }
            }
            None => {
                let text = self.buffer.text();
                EditorDraw {
                    cursor: self.buffer.cursor(),
                    spans: highlight(&text),
                    selection: self.buffer.selection(),
                    suggestion: self.suggestion(),
                    text,
                }
            }
        }
    }

    /// The rest of the suggested history entry, shown after the cursor. Only when
    /// the cursor is at the end of a single-line buffer with no selection, and
    /// neither the Ctrl+R search nor the completion menu is open.
    pub fn suggestion(&self) -> Option<String> {
        if self.search.is_some() || self.menu.is_some() || self.buffer.selection().is_some() {
            return None;
        }
        let text = self.buffer.text();
        if text.contains('\n') || self.buffer.cursor() != text.chars().count() {
            return None;
        }
        self.history
            .suggest(&text)
            .map(|entry| entry[text.len()..].to_string())
    }
}

/// The first word of a suggestion, with the spaces before it: what Option+Right accepts.
pub fn first_word(suggestion: &str) -> &str {
    let word_start = suggestion
        .find(|c: char| !c.is_whitespace())
        .unwrap_or(suggestion.len());
    let word_end = suggestion[word_start..]
        .find(char::is_whitespace)
        .map_or(suggestion.len(), |offset| word_start + offset);
    &suggestion[..word_end]
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
        Some(Kind::Command) => palette.resolve(Color::Named(NamedColor::Green)),
        Some(Kind::Keyword) | Some(Kind::Operator) => palette.chrome.accent,
        Some(Kind::Option) => palette.resolve(Color::Named(NamedColor::Cyan)),
        Some(Kind::String) => palette.resolve(Color::Named(NamedColor::Yellow)),
        Some(Kind::Variable) => palette.resolve(Color::Named(NamedColor::Magenta)),
        Some(Kind::Comment) => palette.chrome.muted,
        None => palette.foreground,
    }
}

/// The top-left corner of the editor's cursor cell, in window pixels. The text
/// wraps at the dock's column count the same way the element draws it.
pub fn cursor_point(geometry: EditorGeometry, text: &str, cursor: usize) -> Point<Pixels> {
    let chars: Vec<char> = text.chars().collect();
    let rows = wrap(&chars, geometry.columns, geometry.columns);
    let (row, column) = position(&rows, cursor);
    point(
        geometry.origin.x + geometry.cell_width * column as f32,
        geometry.origin.y + geometry.line_height * row as f32,
    )
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
    // Text from history or elsewhere must not carry control characters.
    let text = sanitize_paste(text);
    if text.contains('\n') && bracketed {
        format!("\x1b[200~{text}\x1b[201~\r").into_bytes()
    } else {
        format!("{}\r", text.replace('\n', "\r")).into_bytes()
    }
}

/// Text pasted into the input editor, made safe to keep in the buffer. Line
/// endings become `\n`. Tabs and newlines stay, and every other control
/// character (C0, DEL, C1) is removed, so nothing invisible reaches the shell.
pub fn sanitize_paste(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .chars()
        .filter(|&c| c == '\n' || c == '\t' || !c.is_control())
        .collect()
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
    use gpui::px;

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

    fn geometry(columns: usize) -> EditorGeometry {
        EditorGeometry {
            origin: point(px(100.), px(50.)),
            columns,
            cell_width: px(8.),
            line_height: px(20.),
        }
    }

    #[test]
    fn cursor_point_is_the_origin_plus_the_typed_text() {
        assert_eq!(cursor_point(geometry(80), "abc", 3), point(px(124.), px(50.)));
        assert_eq!(cursor_point(geometry(80), "abc", 1), point(px(108.), px(50.)));
    }

    #[test]
    fn cursor_point_wraps_onto_the_next_row() {
        // The dock's rows are 80 columns wide, so char 82 is at column 2 of the
        // second row.
        let text = "x".repeat(90);
        assert_eq!(cursor_point(geometry(80), &text, 82), point(px(116.), px(70.)));
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

    #[test]
    fn control_characters_never_reach_the_shell_in_a_submit() {
        assert_eq!(
            submit_bytes("a\x03b\x0fc\x15\x7f\u{9b}d", false),
            b"abcd\r".to_vec()
        );
        assert_eq!(
            submit_bytes("a\x00\x1b\n\tb", true),
            b"\x1b[200~a\n\tb\x1b[201~\r".to_vec()
        );
    }

    #[test]
    fn pasted_line_endings_become_newlines() {
        assert_eq!(sanitize_paste("a\r\nb\rc\nd"), "a\nb\nc\nd");
    }

    #[test]
    fn pasted_tabs_and_text_stay() {
        assert_eq!(sanitize_paste("ls\t-la ünï"), "ls\t-la ünï");
    }

    #[test]
    fn pasted_control_characters_are_removed() {
        // ESC, ^C, ^O, ^U, NUL, vertical tab, form feed, DEL and C1 controls.
        let text = "a\x1b[31m\x03\x0f\x15\x00\x0b\x0c\x7f\u{80}\u{9b}\u{9f}b";
        assert_eq!(sanitize_paste(text), "a[31mb");
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

    fn state_with_history(entries: &[&str]) -> PromptState {
        let mut state = PromptState::new();
        state.history = History::new(entries.iter().map(|entry| entry.to_string()).collect());
        state
    }

    #[test]
    fn suggestion_is_the_rest_of_the_newest_match() {
        let mut state = state_with_history(&["cargo test --workspace"]);
        state.buffer.set_text("cargo t");
        assert_eq!(state.suggestion().as_deref(), Some("est --workspace"));
        assert_eq!(state.draw().suggestion.as_deref(), Some("est --workspace"));
    }

    #[test]
    fn no_suggestion_when_the_cursor_is_not_at_the_end() {
        let mut state = state_with_history(&["cargo test --workspace"]);
        state.buffer.set_text("cargo t");
        state.buffer.set_cursor(3);
        assert_eq!(state.suggestion(), None);
    }

    #[test]
    fn no_suggestion_while_the_search_is_open() {
        let mut state = state_with_history(&["cargo test --workspace"]);
        state.buffer.set_text("cargo t");
        state.search = Some(HistorySearch::new(&state.history));
        assert_eq!(state.suggestion(), None);
        assert_eq!(state.draw().suggestion, None);
    }

    #[test]
    fn first_word_takes_the_leading_spaces_and_one_word() {
        assert_eq!(first_word(" --workspace now"), " --workspace");
        assert_eq!(first_word("est --x"), "est");
        assert_eq!(first_word("word"), "word");
        assert_eq!(first_word(""), "");
    }
}
