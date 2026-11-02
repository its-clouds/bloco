//! The text buffer. Positions are char indices into `text`, so multi-byte
//! characters (accents, emoji) move as one step.

/// A word is letters, digits and underscore. Everything else separates words.
fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// What the last edit was, so typing a word is one undo step.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Last {
    /// A non-space character was typed. Another one joins the same step.
    Typing,
    /// Anything else. The next edit starts a new step.
    Other,
    /// The cursor moved. The next edit starts a new step.
    Moved,
}

#[derive(Clone, PartialEq, Eq)]
struct Snapshot {
    text: Vec<char>,
    cursor: usize,
}

#[derive(Default)]
pub struct Buffer {
    text: Vec<char>,
    cursor: usize,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    last: Option<Last>,
}

impl Buffer {
    pub fn new() -> Self {
        Self::default()
    }

    /// The whole text. Lines are separated by `\n`.
    pub fn text(&self) -> String {
        self.text.iter().collect()
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// The cursor as a char index into [`Buffer::text`].
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// The cursor as (line, column), both counted in chars from 0.
    pub fn line_col(&self) -> (usize, usize) {
        let before = &self.text[..self.cursor];
        let line = before.iter().filter(|&&c| c == '\n').count();
        let column = before.iter().rev().take_while(|&&c| c != '\n').count();
        (line, column)
    }

    /// Moves the cursor to a line and column. Out-of-range values are clamped:
    /// a column past the end of its line goes to the end of that line.
    pub fn set_line_col(&mut self, line: usize, column: usize) {
        let mut index = 0;
        let mut current = 0;
        while current < line {
            match self.text[index..].iter().position(|&c| c == '\n') {
                Some(offset) => {
                    index += offset + 1;
                    current += 1;
                }
                None => break,
            }
        }
        let line_end = self.text[index..]
            .iter()
            .position(|&c| c == '\n')
            .map_or(self.text.len(), |offset| index + offset);
        self.moved();
        self.cursor = (index + column).min(line_end);
    }

    /// Moves the cursor to a char index. Out-of-range values are clamped.
    pub fn set_cursor(&mut self, index: usize) {
        self.moved();
        self.cursor = index.min(self.text.len());
    }

    pub fn is_first_line(&self) -> bool {
        !self.text[..self.cursor].contains(&'\n')
    }

    pub fn is_last_line(&self) -> bool {
        !self.text[self.cursor..].contains(&'\n')
    }

    /// Replaces the whole text and puts the cursor at its end. This is not an
    /// edit the user made, so it starts a new undo step.
    pub fn set_text(&mut self, text: &str) {
        self.edit(Last::Other);
        self.text = text.chars().collect();
        self.cursor = self.text.len();
    }

    /// Inserts a character at the cursor. Typing a word joins one undo step.
    pub fn insert_char(&mut self, c: char) {
        let kind = if c.is_whitespace() {
            Last::Other
        } else {
            Last::Typing
        };
        self.edit(kind);
        self.text.insert(self.cursor, c);
        self.cursor += 1;
    }

    /// Inserts text at the cursor, for example a paste or an input method's commit.
    pub fn insert_str(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.edit(Last::Other);
        for c in text.chars() {
            self.text.insert(self.cursor, c);
            self.cursor += 1;
        }
    }

    /// Deletes the character before the cursor.
    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        self.edit(Last::Other);
        self.cursor -= 1;
        self.text.remove(self.cursor);
    }

    /// Deletes the character after the cursor.
    pub fn delete_forward(&mut self) {
        if self.cursor == self.text.len() {
            return;
        }
        self.edit(Last::Other);
        self.text.remove(self.cursor);
    }

    /// Deletes back to the start of the previous word (Option+Backspace).
    pub fn delete_word_back(&mut self) {
        let start = self.word_back_target();
        if start == self.cursor {
            return;
        }
        self.edit(Last::Other);
        self.text.drain(start..self.cursor);
        self.cursor = start;
    }

    /// Deletes from the start of the line to the cursor (Cmd+Backspace).
    pub fn delete_to_line_start(&mut self) {
        let start = self.line_start_index();
        if start == self.cursor {
            return;
        }
        self.edit(Last::Other);
        self.text.drain(start..self.cursor);
        self.cursor = start;
    }

    pub fn move_left(&mut self) {
        self.moved();
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn move_right(&mut self) {
        self.moved();
        self.cursor = (self.cursor + 1).min(self.text.len());
    }

    /// Moves up one line, keeping the column where the line is long enough.
    pub fn move_up(&mut self) {
        let (line, column) = self.line_col();
        if line > 0 {
            self.set_line_col(line - 1, column);
        }
    }

    /// Moves down one line, keeping the column where the line is long enough.
    pub fn move_down(&mut self) {
        let (line, column) = self.line_col();
        self.set_line_col(line + 1, column);
    }

    /// Moves to the start of the previous word (Option+Left).
    pub fn word_back(&mut self) {
        self.moved();
        self.cursor = self.word_back_target();
    }

    /// Moves to the end of the next word (Option+Right).
    pub fn word_forward(&mut self) {
        self.moved();
        let mut i = self.cursor;
        while i < self.text.len() && !is_word(self.text[i]) {
            i += 1;
        }
        while i < self.text.len() && is_word(self.text[i]) {
            i += 1;
        }
        self.cursor = i;
    }

    /// Moves to the start of the line (Cmd+Left).
    pub fn line_start(&mut self) {
        self.moved();
        self.cursor = self.line_start_index();
    }

    /// Moves to the end of the line (Cmd+Right).
    pub fn line_end(&mut self) {
        self.moved();
        self.cursor = self.text[self.cursor..]
            .iter()
            .position(|&c| c == '\n')
            .map_or(self.text.len(), |offset| self.cursor + offset);
    }

    /// Empties the buffer. The step can be undone.
    pub fn clear(&mut self) {
        if self.text.is_empty() {
            return;
        }
        self.edit(Last::Other);
        self.text.clear();
        self.cursor = 0;
    }

    /// Takes the text out for submitting, and leaves the buffer empty. The
    /// undo history goes with it: a submitted line is not an edit to undo.
    pub fn take(&mut self) -> String {
        let text = self.text();
        self.text.clear();
        self.cursor = 0;
        self.undo.clear();
        self.redo.clear();
        self.last = None;
        text
    }

    /// Undoes the last edit. Returns false when there is nothing to undo.
    pub fn undo(&mut self) -> bool {
        let Some(previous) = self.undo.pop() else {
            return false;
        };
        self.redo.push(self.snapshot());
        self.restore(previous);
        true
    }

    /// Redoes the last undone edit. Returns false when there is nothing to redo.
    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else {
            return false;
        };
        self.undo.push(self.snapshot());
        self.restore(next);
        true
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            text: self.text.clone(),
            cursor: self.cursor,
        }
    }

    fn restore(&mut self, snapshot: Snapshot) {
        self.text = snapshot.text;
        self.cursor = snapshot.cursor;
        self.last = Some(Last::Moved);
    }

    /// Records the state before an edit, unless the edit continues the step
    /// before it. Any edit clears the redo history.
    fn edit(&mut self, kind: Last) {
        let continues = self.last == Some(Last::Typing) && kind == Last::Typing;
        if !continues {
            self.undo.push(self.snapshot());
        }
        self.redo.clear();
        self.last = Some(kind);
    }

    /// A cursor move ends the typing step, so the next typing starts a new one.
    fn moved(&mut self) {
        self.last = Some(Last::Moved);
    }

    fn line_start_index(&self) -> usize {
        self.text[..self.cursor]
            .iter()
            .rposition(|&c| c == '\n')
            .map_or(0, |i| i + 1)
    }

    /// Where Option+Left lands: skip separators, then the word before them.
    fn word_back_target(&self) -> usize {
        let mut i = self.cursor;
        while i > 0 && !is_word(self.text[i - 1]) {
            i -= 1;
        }
        while i > 0 && is_word(self.text[i - 1]) {
            i -= 1;
        }
        i
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(text: &str) -> Buffer {
        let mut buffer = Buffer::new();
        for c in text.chars() {
            buffer.insert_char(c);
        }
        buffer
    }

    #[test]
    fn typing_builds_text_and_cursor() {
        let buffer = typed("ação");
        assert_eq!(buffer.text(), "ação");
        // The cursor counts chars, not bytes: "ação" is four chars.
        assert_eq!(buffer.cursor(), 4);
    }

    #[test]
    fn dead_key_composed_text_inserts_as_one_string() {
        let mut buffer = Buffer::new();
        buffer.insert_str("ç");
        buffer.insert_str("ão");
        assert_eq!(buffer.text(), "ção");
        buffer.backspace();
        assert_eq!(buffer.text(), "çã");
    }

    #[test]
    fn newline_splits_lines_and_line_col_follows() {
        let mut buffer = typed("ls");
        buffer.insert_char('\n');
        buffer.insert_str("-la");
        assert_eq!(buffer.text(), "ls\n-la");
        assert_eq!(buffer.line_col(), (1, 3));
        buffer.move_up();
        assert_eq!(buffer.line_col(), (0, 2));
    }

    #[test]
    fn vertical_moves_keep_column_and_clamp() {
        let mut buffer = typed("abcdef\nxy");
        buffer.set_line_col(0, 5);
        buffer.move_down();
        // Line 1 is only two chars long, so the cursor goes to its end.
        assert_eq!(buffer.line_col(), (1, 2));
        buffer.move_up();
        assert_eq!(buffer.line_col(), (0, 2));
    }

    #[test]
    fn set_line_col_clamps_past_the_end_of_a_line() {
        let mut buffer = typed("ab\ncd");
        buffer.set_line_col(0, 99);
        assert_eq!(buffer.line_col(), (0, 2));
        buffer.set_line_col(99, 0);
        assert_eq!(buffer.line_col(), (1, 0));
    }

    #[test]
    fn first_and_last_line_tell_history_when_to_take_over() {
        let mut buffer = typed("one\ntwo");
        assert!(buffer.is_last_line());
        assert!(!buffer.is_first_line());
        buffer.set_line_col(0, 0);
        assert!(buffer.is_first_line());
        assert!(!buffer.is_last_line());
    }

    #[test]
    fn word_moves_skip_separators() {
        let mut buffer = typed("git commit -m 'fix it'");
        buffer.word_back();
        assert_eq!(buffer.cursor(), 19); // before "it"
        buffer.word_back();
        assert_eq!(buffer.cursor(), 15); // before "fix"
        buffer.word_forward();
        assert_eq!(buffer.cursor(), 18); // after "fix"
        buffer.line_start();
        buffer.word_forward();
        assert_eq!(buffer.cursor(), 3); // after "git"
    }

    #[test]
    fn delete_word_back_removes_one_word() {
        let mut buffer = typed("cargo test --workspace");
        buffer.delete_word_back();
        assert_eq!(buffer.text(), "cargo test --");
        buffer.delete_word_back();
        assert_eq!(buffer.text(), "cargo ");
    }

    #[test]
    fn line_moves_stay_on_the_current_line() {
        let mut buffer = typed("first\nsecond line");
        buffer.line_start();
        assert_eq!(buffer.line_col(), (1, 0));
        buffer.line_end();
        assert_eq!(buffer.line_col(), (1, 11));
        buffer.delete_to_line_start();
        assert_eq!(buffer.text(), "first\n");
    }

    #[test]
    fn delete_forward_and_backspace_at_edges_do_nothing() {
        let mut buffer = typed("a");
        buffer.delete_forward();
        assert_eq!(buffer.text(), "a");
        buffer.move_left();
        buffer.backspace();
        assert_eq!(buffer.text(), "a");
        buffer.move_left();
        buffer.move_left();
        buffer.backspace();
        assert_eq!(buffer.text(), "a");
        assert!(!buffer.undo() || buffer.text() == "");
    }

    #[test]
    fn undo_restores_a_typed_word_in_one_step() {
        let mut buffer = typed("hello");
        buffer.insert_char(' ');
        buffer.insert_char('w');
        buffer.insert_char('o');
        buffer.undo();
        assert_eq!(buffer.text(), "hello ");
        buffer.undo();
        assert_eq!(buffer.text(), "hello");
        buffer.undo();
        assert_eq!(buffer.text(), "");
        assert!(!buffer.undo());
    }

    #[test]
    fn a_cursor_move_splits_the_undo_step() {
        let mut buffer = typed("ab");
        buffer.move_left();
        buffer.insert_char('c');
        assert_eq!(buffer.text(), "acb");
        buffer.undo();
        assert_eq!(buffer.text(), "ab");
    }

    #[test]
    fn redo_reapplies_after_undo_and_a_new_edit_clears_it() {
        let mut buffer = typed("ab");
        buffer.undo();
        assert_eq!(buffer.text(), "");
        assert!(buffer.redo());
        assert_eq!(buffer.text(), "ab");
        buffer.undo();
        buffer.insert_char('x');
        assert!(!buffer.redo());
        assert_eq!(buffer.text(), "x");
    }

    #[test]
    fn take_empties_the_buffer_and_forgets_its_undo() {
        let mut buffer = typed("ls -la");
        assert_eq!(buffer.take(), "ls -la");
        assert!(buffer.is_empty());
        assert_eq!(buffer.cursor(), 0);
        assert!(!buffer.undo());
    }

    #[test]
    fn paste_with_newlines_keeps_them() {
        let mut buffer = Buffer::new();
        buffer.insert_str("echo 1\necho 2");
        assert_eq!(buffer.line_col(), (1, 6));
        assert_eq!(buffer.text(), "echo 1\necho 2");
    }

    #[test]
    fn set_text_replaces_and_can_be_undone() {
        let mut buffer = typed("old");
        buffer.set_text("git status");
        assert_eq!(buffer.text(), "git status");
        assert_eq!(buffer.cursor(), 10);
        buffer.undo();
        assert_eq!(buffer.text(), "old");
    }
}
