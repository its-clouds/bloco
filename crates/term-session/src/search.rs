//! Find in the terminal output: every match of a query in the scrollback and on
//! the screen.
//!
//! The grid is read one logical line at a time: rows joined while the output
//! wrapped, with the blank cells after the last character dropped. Each line is
//! searched with the `regex` crate, so `^` and `$` mean the start and end of the
//! line as the program printed it. A map from each character back to its cell
//! turns byte offsets into grid positions.

use std::collections::VecDeque;

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::Term;
use regex::{Regex, RegexBuilder};

/// Matches stop being collected after this many, so a one-letter query on a
/// full scrollback stays cheap to store and to draw.
pub const MAX_MATCHES: usize = 10_000;

/// What to look for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FindQuery {
    pub text: String,
    /// Treat `text` as a regular expression. Otherwise it is literal text.
    pub regex: bool,
    pub case_sensitive: bool,
}

/// A cell in the whole buffer. Line 0 is the oldest line of the scrollback, so
/// a point keeps its line while new output arrives (until the scrollback is
/// full and drops its oldest lines).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct BufferPoint {
    pub line: usize,
    pub column: usize,
}

/// One match. `end` is the last cell of the match (inclusive). A match can run
/// over several lines when the output wrapped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FindMatch {
    pub start: BufferPoint,
    pub end: BufferPoint,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FindResults {
    /// Oldest first.
    pub matches: Vec<FindMatch>,
    /// There were more than [`MAX_MATCHES`] matches. Only the newest are kept.
    pub truncated: bool,
}

/// Where one character of a logical line sits in the grid.
#[derive(Debug, Clone, Copy)]
struct CharCell {
    /// Byte offset of the character in the line text.
    byte: usize,
    /// Its first cell.
    start: BufferPoint,
    /// Its last cell: the spacer column for a wide character.
    end: BufferPoint,
}

/// Every match of `query` in `term`, oldest first. An empty query has no
/// matches, and neither has a regex that only matches empty text. Fails when
/// `query` is an invalid regular expression.
pub fn find_all<T>(term: &Term<T>, query: &FindQuery) -> Result<FindResults, String> {
    if query.text.is_empty() {
        return Ok(FindResults::default());
    }
    let regex = compile(query)?;
    let grid = term.grid();
    let history = grid.history_size() as i32;
    let columns = grid.columns();
    let last_line = grid.bottommost_line().0;

    // The newest output matters most, so past the limit the oldest matches go.
    let mut matches = VecDeque::new();
    let mut truncated = false;
    // Reused for every logical line.
    let mut text = String::new();
    let mut cells: Vec<CharCell> = Vec::new();

    let mut line = grid.topmost_line().0;
    while line <= last_line {
        text.clear();
        cells.clear();
        // Rows of one logical line: each row that ends in WRAPLINE continues below.
        loop {
            let row = &grid[Line(line)];
            let buffer_line = (line + history) as usize;
            for column in 0..columns {
                let cell = &row[Column(column)];
                if cell
                    .flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
                {
                    continue;
                }
                let start = BufferPoint {
                    line: buffer_line,
                    column,
                };
                let end = if cell.flags.contains(Flags::WIDE_CHAR) {
                    BufferPoint {
                        line: buffer_line,
                        column: (column + 1).min(columns - 1),
                    }
                } else {
                    start
                };
                let mut push = |c: char| {
                    cells.push(CharCell {
                        byte: text.len(),
                        start,
                        end,
                    });
                    text.push(c);
                };
                push(cell.c);
                for &c in cell.zerowidth().unwrap_or_default() {
                    push(c);
                }
            }
            let wrapped = row[Column(columns - 1)].flags.contains(Flags::WRAPLINE);
            line += 1;
            if !wrapped || line > last_line {
                break;
            }
        }

        // The blank cells after the last character are not output.
        let trimmed = text.trim_end_matches(' ').len();
        text.truncate(trimmed);
        let kept = cells.partition_point(|cell| cell.byte < trimmed);
        cells.truncate(kept);

        for found in regex.find_iter(&text) {
            if found.is_empty() {
                continue;
            }
            // The character that holds the last byte of the match.
            let first = cells.partition_point(|cell| cell.byte <= found.start()) - 1;
            let last = cells.partition_point(|cell| cell.byte < found.end()) - 1;
            if matches.len() == MAX_MATCHES {
                matches.pop_front();
                truncated = true;
            }
            matches.push_back(FindMatch {
                start: cells[first].start,
                end: cells[last].end,
            });
        }
    }

    Ok(FindResults {
        matches: matches.into(),
        truncated,
    })
}

/// Builds the regex. Literal text is escaped first.
fn compile(query: &FindQuery) -> Result<Regex, String> {
    let pattern = if query.regex {
        query.text.clone()
    } else {
        regex::escape(&query.text)
    };
    RegexBuilder::new(&pattern)
        .case_insensitive(!query.case_sensitive)
        .build()
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::term::test::TermSize;
    use alacritty_terminal::term::Config;
    use alacritty_terminal::vte::ansi::Processor;

    fn term(columns: usize, rows: usize, output: &str) -> Term<VoidListener> {
        let mut term = Term::new(
            Config::default(),
            &TermSize::new(columns, rows),
            VoidListener,
        );
        let mut parser: Processor = Processor::new();
        parser.advance(&mut term, output.as_bytes());
        term
    }

    fn query(text: &str) -> FindQuery {
        FindQuery {
            text: text.into(),
            ..FindQuery::default()
        }
    }

    fn at(line: usize, column: usize) -> BufferPoint {
        BufferPoint { line, column }
    }

    /// (line, first column, last column) of each single-line match.
    fn spans(results: &FindResults) -> Vec<(usize, usize, usize)> {
        results
            .matches
            .iter()
            .map(|m| {
                assert_eq!(m.start.line, m.end.line);
                (m.start.line, m.start.column, m.end.column)
            })
            .collect()
    }

    #[test]
    fn finds_plain_text_oldest_first() {
        let term = term(40, 5, "hello world\r\nsay world peace\r\n");
        let results = find_all(&term, &query("world")).unwrap();
        assert_eq!(spans(&results), [(0, 6, 10), (1, 4, 8)]);
        assert!(!results.truncated);
    }

    #[test]
    fn an_empty_query_has_no_matches() {
        let term = term(40, 5, "hello\r\n");
        assert_eq!(find_all(&term, &query("")).unwrap(), FindResults::default());
    }

    #[test]
    fn plain_text_is_literal() {
        let term = term(40, 5, "abc a.c (x) 1+1\r\n");
        assert_eq!(spans(&find_all(&term, &query("a.c")).unwrap()), [(0, 4, 6)]);
        assert_eq!(spans(&find_all(&term, &query("(x)")).unwrap()), [(0, 8, 10)]);
        assert_eq!(spans(&find_all(&term, &query("1+1")).unwrap()), [(0, 12, 14)]);
        assert_eq!(find_all(&term, &query("[")).unwrap().matches.len(), 0);
    }

    #[test]
    fn case_insensitive_by_default_even_with_capitals() {
        let term = term(40, 5, "Foo foo FOO\r\n");
        assert_eq!(find_all(&term, &query("foo")).unwrap().matches.len(), 3);
        // An upper-case query is still case-insensitive with the toggle off.
        assert_eq!(find_all(&term, &query("FOO")).unwrap().matches.len(), 3);
    }

    #[test]
    fn case_sensitive_matches_exact_case() {
        let term = term(40, 5, "Foo foo FOO\r\n");
        let sensitive = |text: &str| FindQuery {
            case_sensitive: true,
            ..query(text)
        };
        assert_eq!(spans(&find_all(&term, &sensitive("foo")).unwrap()), [(0, 4, 6)]);
        assert_eq!(spans(&find_all(&term, &sensitive("FOO")).unwrap()), [(0, 8, 10)]);
    }

    #[test]
    fn regex_queries() {
        let term = term(40, 5, "abc123 def45\r\nerror: E42\r\n");
        let regex = |text: &str| FindQuery {
            regex: true,
            ..query(text)
        };
        assert_eq!(
            spans(&find_all(&term, &regex(r"\d+")).unwrap()),
            [(0, 3, 5), (0, 10, 11), (1, 8, 9)]
        );
        // Case-insensitive across every alternative.
        assert_eq!(
            spans(&find_all(&term, &regex("ABC|ERROR")).unwrap()),
            [(0, 0, 2), (1, 0, 4)]
        );
        // The same text without the regex toggle is literal.
        assert_eq!(find_all(&term, &query(r"\d+")).unwrap().matches.len(), 0);
    }

    #[test]
    fn invalid_regex_is_an_error() {
        let term = term(40, 5, "a(b\r\n");
        let regex = FindQuery {
            regex: true,
            ..query("a(")
        };
        assert!(find_all(&term, &regex).is_err());
        // As plain text it is fine.
        assert_eq!(spans(&find_all(&term, &query("a(")).unwrap()), [(0, 0, 1)]);
    }

    #[test]
    fn a_match_can_span_a_wrapped_line() {
        // Ten columns: the text wraps after `9`.
        let term = term(10, 5, "0123456789abcdef\r\n");
        let results = find_all(&term, &query("89ab")).unwrap();
        assert_eq!(
            results.matches,
            [FindMatch {
                start: at(0, 8),
                end: at(1, 1),
            }]
        );
    }

    #[test]
    fn finds_matches_in_the_scrollback() {
        let output: String = (0..20).map(|i| format!("line {i}\r\n")).collect();
        let term = term(20, 5, &output);
        let history = term.grid().history_size();
        assert!(history > 10, "the test needs a scrollback, got {history}");

        let results = find_all(&term, &query("line 2")).unwrap();
        // Line 0 of the buffer is the oldest line, `line 0`.
        assert_eq!(spans(&results), [(2, 0, 5)]);
        assert!(results.matches[0].start.line < history);

        let all = find_all(&term, &query("line")).unwrap();
        assert_eq!(all.matches.len(), 20);
        assert_eq!(all.matches[19].start, at(19, 0));
    }

    #[test]
    fn stops_at_the_match_limit() {
        let line = "x".repeat(100);
        let output: String = (0..120).map(|_| format!("{line}\r\n")).collect();
        let term = term(100, 10, &output);
        let results = find_all(&term, &query("x")).unwrap();
        assert_eq!(results.matches.len(), MAX_MATCHES);
        assert!(results.truncated);
        // The newest matches are kept: the last one is on the last line of output.
        let last = results.matches[MAX_MATCHES - 1];
        assert_eq!(last.start, at(119, 99));
        assert!(results.matches.windows(2).all(|w| w[0].start < w[1].start));
    }

    #[test]
    fn anchors_match_whole_lines() {
        let output: String = (1..=5000).map(|i| format!("{i}\r\n")).collect();
        let term = term(80, 24, &output);
        let regex = FindQuery {
            regex: true,
            ..query("^4.*2$")
        };
        let results = find_all(&term, &regex).unwrap();
        // `seq 1 5000` fits in the default 10,000-line scrollback.
        assert!(term.grid().history_size() + 24 > 5000);
        let expected: Vec<usize> = (1..=5000)
            .filter(|i| {
                let s = i.to_string();
                s.starts_with('4') && s.ends_with('2')
            })
            .collect();
        assert!(!expected.is_empty());
        assert_eq!(results.matches.len(), expected.len());
        for (found, number) in results.matches.iter().zip(&expected) {
            let digits = number.to_string().len();
            assert_eq!(found.start.column, 0);
            assert_eq!(found.end.column, digits - 1);
        }
    }

    #[test]
    fn dollar_ignores_the_blank_cells_after_the_text() {
        let term = term(40, 5, "42\r\n42 x\r\n");
        let regex = FindQuery {
            regex: true,
            ..query("2$")
        };
        assert_eq!(spans(&find_all(&term, &regex).unwrap()), [(0, 1, 1)]);
    }

    #[test]
    fn caret_matches_once_per_line() {
        let term = term(40, 5, "4242\r\n");
        let regex = FindQuery {
            regex: true,
            ..query("^42")
        };
        assert_eq!(spans(&find_all(&term, &regex).unwrap()), [(0, 0, 1)]);
    }

    #[test]
    fn anchors_apply_to_the_logical_line() {
        // Ten columns: the line wraps after `9`, so `abcdef` is the same line.
        let term = term(10, 5, "0123456789abcdef\r\n");
        let regex = |text: &str| FindQuery {
            regex: true,
            ..query(text)
        };
        assert_eq!(
            find_all(&term, &regex("^0.*f$")).unwrap().matches,
            [FindMatch {
                start: at(0, 0),
                end: at(1, 5),
            }]
        );
        // The second row does not start a line.
        assert!(find_all(&term, &regex("^abc")).unwrap().matches.is_empty());
        assert!(find_all(&term, &regex("9$")).unwrap().matches.is_empty());
    }

    #[test]
    fn wide_characters_map_to_their_cells() {
        let term = term(40, 5, "日本語 ok 🙂x\r\n");
        // 日 0-1, 本 2-3, 語 4-5, space 6, o 7, k 8, space 9, 🙂 10-11, x 12.
        assert_eq!(spans(&find_all(&term, &query("本")).unwrap()), [(0, 2, 3)]);
        assert_eq!(spans(&find_all(&term, &query("語 ok")).unwrap()), [(0, 4, 8)]);
        assert_eq!(spans(&find_all(&term, &query("🙂x")).unwrap()), [(0, 10, 12)]);
        assert_eq!(spans(&find_all(&term, &query("x")).unwrap()), [(0, 12, 12)]);
    }

    #[test]
    fn a_wide_character_that_wrapped_keeps_its_cells() {
        // Five columns: `日` does not fit after `abcd`, so it starts the next row.
        let term = term(5, 5, "abcd日z\r\n");
        assert_eq!(
            find_all(&term, &query("d日")).unwrap().matches,
            [FindMatch {
                start: at(0, 3),
                end: at(1, 1),
            }]
        );
    }

    #[test]
    fn a_regex_that_only_matches_empty_text_finds_nothing() {
        let term = term(40, 5, "hello\r\n");
        let regex = FindQuery {
            regex: true,
            ..query("x*")
        };
        assert!(find_all(&term, &regex).unwrap().matches.is_empty());
    }
}
