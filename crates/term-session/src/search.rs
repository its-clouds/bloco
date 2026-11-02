//! Find in the terminal output: every match of a query in the scrollback and on
//! the screen.
//!
//! The search runs on `alacritty_terminal`'s regex engine (`RegexSearch` and
//! `RegexIter`), the way `zed-industries/zed` does it in
//! crates/terminal/src/alacritty.rs (`all_search_matches`, GPL-3.0-or-later).
//! See NOTICE.

use std::collections::VecDeque;

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Direction, Point};
use alacritty_terminal::term::search::{RegexIter, RegexSearch};
use alacritty_terminal::term::Term;

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

/// Every match of `query` in `term`, oldest first. An empty query has no
/// matches. Fails when `query` is an invalid regular expression.
pub fn find_all<T>(term: &Term<T>, query: &FindQuery) -> Result<FindResults, String> {
    if query.text.is_empty() {
        return Ok(FindResults::default());
    }
    let mut regex = compile(query)?;
    let grid = term.grid();
    let history = grid.history_size() as i32;
    let to_buffer = |point: Point| BufferPoint {
        line: (point.line.0 + history) as usize,
        column: point.column.0,
    };
    let top = Point::new(grid.topmost_line(), Column(0));
    let bottom = Point::new(grid.bottommost_line(), grid.last_column());

    // Top down, because a leftward search finds different (overlapping) matches
    // for greedy patterns. The newest output matters most, so past the limit the
    // oldest matches are dropped.
    let mut matches = VecDeque::new();
    let mut truncated = false;
    for found in RegexIter::new(top, bottom, Direction::Right, term, &mut regex) {
        if matches.len() == MAX_MATCHES {
            matches.pop_front();
            truncated = true;
        }
        matches.push_back(FindMatch {
            start: to_buffer(*found.start()),
            end: to_buffer(*found.end()),
        });
    }
    Ok(FindResults {
        matches: matches.into(),
        truncated,
    })
}

/// Builds the search automaton. `alacritty_terminal` picks the case rule from
/// the query (case-insensitive unless it has an upper-case letter), so a
/// leading flag sets the rule the user chose instead. A flag at the start of a
/// pattern covers every alternative after it.
fn compile(query: &FindQuery) -> Result<RegexSearch, String> {
    let pattern = if query.regex {
        query.text.clone()
    } else {
        escape(&query.text)
    };
    let flag = if query.case_sensitive { "(?-i)" } else { "(?i)" };
    RegexSearch::new(&format!("{flag}{pattern}")).map_err(|e| e.to_string())
}

/// Escapes the regex syntax characters in `text`, so it matches literally.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if r"\.+*?()|[]{}^$#&-~".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
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
        // alacritty's own rule would make this query case-sensitive.
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
        // The case flag covers every alternative.
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
    fn escape_covers_the_regex_syntax() {
        assert_eq!(escape("a.b*c"), r"a\.b\*c");
        assert_eq!(escape("ação"), "ação");
    }
}
