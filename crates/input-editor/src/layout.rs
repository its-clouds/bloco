//! Soft wrapping of the buffer into screen rows. The first line of the buffer
//! starts after the prompt, so it has a narrower width than the rest.

use std::ops::Range;

/// Splits `text` into screen rows, as char ranges. A newline ends a row and is
/// not part of any range. A logical line longer than the width wraps. The first
/// line is `first` chars wide, or `width` when `first` is 0 (the prompt reaches
/// the right edge). Every line has at least one row, even an empty one.
pub fn wrap(text: &[char], first: usize, width: usize) -> Vec<Range<usize>> {
    let width = width.max(1);
    let mut rows = Vec::new();
    let mut line_start = 0;
    let mut row_width = if first == 0 { width } else { first };
    loop {
        let line_end = text[line_start..]
            .iter()
            .position(|&c| c == '\n')
            .map_or(text.len(), |offset| line_start + offset);
        let mut start = line_start;
        loop {
            let end = (start + row_width).min(line_end);
            rows.push(start..end);
            start = end;
            row_width = width;
            if start >= line_end {
                break;
            }
        }
        if line_end == text.len() {
            return rows;
        }
        line_start = line_end + 1;
    }
}

/// The (row, column) of char index `cursor` in rows made by [`wrap`]. A cursor
/// at the start of a wrapped row belongs to that row, not the one before it.
pub fn position(rows: &[Range<usize>], cursor: usize) -> (usize, usize) {
    let row = rows
        .iter()
        .rposition(|range| range.start <= cursor)
        .unwrap_or(0);
    (row, cursor.saturating_sub(rows[row].start))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(text: &str) -> Vec<char> {
        text.chars().collect()
    }

    #[test]
    fn short_line_is_one_row() {
        assert_eq!(wrap(&chars("ls"), 10, 80), vec![0..2]);
    }

    #[test]
    fn first_row_is_narrower_then_rows_use_the_full_width() {
        // "abcdefgh" with 3 columns after the prompt, then 4 per row.
        assert_eq!(wrap(&chars("abcdefgh"), 3, 4), vec![0..3, 3..7, 7..8]);
    }

    #[test]
    fn zero_first_width_uses_the_full_width() {
        assert_eq!(wrap(&chars("abcde"), 0, 4), vec![0..4, 4..5]);
    }

    #[test]
    fn newlines_start_rows_and_empty_lines_keep_a_row() {
        assert_eq!(wrap(&chars("a\n\nb"), 10, 10), vec![0..1, 2..2, 3..4]);
    }

    #[test]
    fn empty_text_is_one_empty_row() {
        assert_eq!(wrap(&[], 5, 5), vec![0..0]);
    }

    #[test]
    fn cursor_position_within_and_across_rows() {
        let rows = wrap(&chars("abcdefgh"), 3, 4);
        assert_eq!(position(&rows, 0), (0, 0));
        assert_eq!(position(&rows, 3), (1, 0));
        assert_eq!(position(&rows, 6), (1, 3));
        assert_eq!(position(&rows, 8), (2, 1));
    }

    #[test]
    fn cursor_at_end_of_a_line_stays_on_that_line() {
        let rows = wrap(&chars("ab\ncd"), 10, 10);
        // Index 2 is the newline: the end of the first line.
        assert_eq!(position(&rows, 2), (0, 2));
        assert_eq!(position(&rows, 3), (1, 0));
    }
}
