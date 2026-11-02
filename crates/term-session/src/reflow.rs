//! Keeps block positions on their text when the grid is resized.
//!
//! Alacritty rewraps the buffer when the width changes, so a buffer line moves:
//! a logical line (the text up to a hard newline) takes more or fewer rows. The
//! same holds for a height change that pushes lines into the history. Instead of
//! the buffer line, a position is recorded relative to the cursor, which alacritty
//! moves with the text, and it is found again in the new buffer from there:
//!
//! - how many logical lines lie between the position's logical line and the
//!   cursor's (counting only the lines between them, so lines dropped from the top
//!   of a full scrollback change nothing);
//! - how many cells lie between the start of that logical line and the position.
//!
//! The cells are counted in the old width and mapped to rows of the new one.

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::Term;

/// The logical lines of the primary screen's buffer, as they are now.
pub(crate) struct Layout {
    /// The buffer line of the first row of each logical line, top first.
    starts: Vec<usize>,
    /// The number of buffer lines. The buffer's last line is `total - 1`.
    total: usize,
    columns: usize,
    /// Index in `starts` of the logical line the cursor is on.
    cursor: usize,
}

/// Where a buffer line is in a [`Layout`], independent of the width it was taken at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Anchor {
    /// A line of the buffer. `lines` counts logical lines from the cursor's, and is
    /// negative above it. `cells` counts from the start of the line's logical line.
    Line { lines: isize, cells: usize },
    /// The line after the buffer's last line. A block that ends with output on the
    /// last row has this end.
    End,
}

impl Layout {
    /// The layout of the grid that is active in `term`.
    pub(crate) fn of<T: EventListener>(term: &Term<T>) -> Self {
        let columns = term.columns();
        let history = term.grid().history_size();
        let total = history + term.screen_lines();
        // A row whose last cell carries WRAPLINE continues on the next row.
        let wrapped = |line: usize| {
            let row = &term.grid()[Line(line as i32 - history as i32)];
            row[Column(columns - 1)].flags.contains(Flags::WRAPLINE)
        };
        let starts: Vec<usize> = (0..total)
            .filter(|&line| line == 0 || !wrapped(line - 1))
            .collect();
        let cursor_line = history + term.grid().cursor.point.line.0 as usize;
        let cursor = starts.partition_point(|&start| start <= cursor_line) - 1;
        Self {
            starts,
            total,
            columns,
            cursor,
        }
    }

    /// The anchor of buffer line `line` in this layout. `None` for a line past
    /// the end of the buffer.
    pub(crate) fn anchor(&self, line: usize) -> Option<Anchor> {
        if line == self.total {
            return Some(Anchor::End);
        }
        if line > self.total {
            return None;
        }
        let index = self.starts.partition_point(|&start| start <= line) - 1;
        Some(Anchor::Line {
            lines: index as isize - self.cursor as isize,
            cells: (line - self.starts[index]) * self.columns,
        })
    }

    /// The buffer line of `anchor` in this layout. `None` when its logical line
    /// is no longer in the buffer, for example after it left a full scrollback.
    pub(crate) fn line(&self, anchor: Anchor) -> Option<usize> {
        match anchor {
            Anchor::End => Some(self.total),
            Anchor::Line { lines, cells } => {
                let index = usize::try_from(self.cursor as isize + lines).ok()?;
                let start = *self.starts.get(index)?;
                let next = self.starts.get(index + 1).copied().unwrap_or(self.total);
                // The cells can run past the end of a logical line that the new
                // width made shorter, so the row is kept inside the line.
                Some((start + cells / self.columns).min(next - 1))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::event::Event;
    use alacritty_terminal::term::test::TermSize;
    use alacritty_terminal::term::Config;
    use alacritty_terminal::vte::ansi::Processor;

    struct Silent;

    impl EventListener for Silent {
        fn send_event(&self, _: Event) {}
    }

    /// A 10-row term at `cols` columns, with `text` written to it.
    fn term_with(cols: usize, text: &str) -> Term<Silent> {
        let mut term = Term::new(Config::default(), &TermSize::new(cols, 10), Silent);
        let mut parser: Processor = Processor::new();
        parser.advance(&mut term, text.as_bytes());
        term
    }

    #[test]
    fn a_line_keeps_its_text_when_the_width_changes() {
        // 100 letters wrap to two rows at 80 columns: rows 0 and 1. "b" is row 2,
        // and the cursor is on row 3.
        let mut term = term_with(80, &format!("{}\r\nb\r\n", "a".repeat(100)));
        let before = Layout::of(&term);
        let b = before.anchor(2).unwrap();
        let wrapped_end = before.anchor(1).unwrap();

        // At 40 columns the letters take three rows, so "b" moves to row 3.
        term.resize(TermSize::new(40, 10));
        let after = Layout::of(&term);
        assert_eq!(after.line(b), Some(3));
        // Row 1 started at letter 80, which is row 2 at the new width.
        assert_eq!(after.line(wrapped_end), Some(2));
        // The cursor is on row 4 now, and its anchor is the cursor's own line.
        assert_eq!(after.anchor(4), Some(Anchor::Line { lines: 0, cells: 0 }));
    }

    #[test]
    fn end_of_the_buffer_stays_at_the_end() {
        let term = term_with(80, "a\r\n");
        let layout = Layout::of(&term);
        assert_eq!(layout.anchor(layout.total), Some(Anchor::End));
        assert_eq!(layout.line(Anchor::End), Some(layout.total));
        assert_eq!(layout.anchor(layout.total + 1), None);
    }
}
