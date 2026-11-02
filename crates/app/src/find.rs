//! The find bar of one pane: the typed query, the toggles, the matches and which
//! one is current. The search itself is `term_session::Finder`. The bar is drawn,
//! and its keys handled, in `terminal_view.rs`.
//!
//! Matches are numbered from the newest output up, as in Warp: `1/163` is the
//! match closest to the bottom. "Next" moves to older output, "previous" to newer.

use std::rc::Rc;

use term_session::{FindMatch, FindQuery, FindResults};

/// Which match becomes current when results arrive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    /// The newest match. Used when the query changes.
    Newest,
    /// The match that was current, or the nearest older one. Used when new
    /// output reruns the search.
    Keep,
}

#[derive(Debug, Default)]
pub struct FindState {
    pub query: FindQuery,
    /// The whole query is selected (after Cmd+F on an open bar): typing replaces it.
    pub all_selected: bool,
    /// Oldest first.
    pub matches: Rc<[FindMatch]>,
    /// More matches than the search keeps. The counter shows a `+`.
    pub truncated: bool,
    /// Index into `matches`.
    pub current: Option<usize>,
    /// The query is not a valid regex.
    pub invalid: bool,
    /// Counts query changes, so results for an older query can be dropped.
    pub generation: u64,
}

impl FindState {
    pub fn new(query: FindQuery, all_selected: bool) -> Self {
        Self {
            query,
            all_selected,
            ..Self::default()
        }
    }

    /// Types text into the query. Control characters are dropped.
    pub fn push_text(&mut self, text: &str) {
        self.take_selection();
        self.query
            .text
            .extend(text.chars().filter(|c| !c.is_control()));
        self.changed();
    }

    pub fn pop_char(&mut self) {
        if !self.take_selection() {
            self.query.text.pop();
        }
        self.changed();
    }

    /// Deletes the word before the end of the query, and the spaces after it.
    pub fn delete_word(&mut self) {
        if !self.take_selection() {
            let text = self.query.text.trim_end();
            let start = text
                .char_indices()
                .rev()
                .find(|(_, c)| c.is_whitespace())
                .map_or(0, |(i, c)| i + c.len_utf8());
            self.query.text.truncate(start);
        }
        self.changed();
    }

    pub fn clear(&mut self) {
        self.query.text.clear();
        self.all_selected = false;
        self.changed();
    }

    pub fn toggle_regex(&mut self) {
        self.query.regex = !self.query.regex;
        self.changed();
    }

    pub fn toggle_case(&mut self) {
        self.query.case_sensitive = !self.query.case_sensitive;
        self.changed();
    }

    /// Stores search results. Results for an older query are ignored. Returns
    /// whether they were stored.
    pub fn set_results(
        &mut self,
        generation: u64,
        results: Result<FindResults, String>,
        pick: Pick,
    ) -> bool {
        if generation != self.generation {
            return false;
        }
        let previous = self.current_match();
        let results = match results {
            Ok(results) => {
                self.invalid = false;
                results
            }
            Err(_) => {
                self.invalid = true;
                FindResults::default()
            }
        };
        let matches = results.matches;
        self.current = match (pick, previous) {
            _ if matches.is_empty() => None,
            (Pick::Keep, Some(previous)) => {
                // The same match, or else the newest one that starts before it.
                let after = matches.partition_point(|m| m.start <= previous.start);
                Some(after.saturating_sub(1))
            }
            _ => Some(matches.len() - 1),
        };
        self.matches = matches.into();
        self.truncated = results.truncated;
        true
    }

    pub fn current_match(&self) -> Option<FindMatch> {
        self.matches.get(self.current?).copied()
    }

    /// Moves to the next older match, wrapping to the newest.
    pub fn next(&mut self) -> Option<FindMatch> {
        let len = self.matches.len();
        self.current = self.current.map(|i| (i + len - 1) % len);
        self.current_match()
    }

    /// Moves to the next newer match, wrapping to the oldest.
    pub fn previous(&mut self) -> Option<FindMatch> {
        let len = self.matches.len();
        self.current = self.current.map(|i| (i + 1) % len);
        self.current_match()
    }

    /// `3/163`, `No results`, `Invalid regex`, or nothing for an empty query.
    pub fn counter(&self) -> String {
        if self.query.text.is_empty() {
            String::new()
        } else if self.invalid {
            "Invalid regex".into()
        } else if let Some(current) = self.current {
            let total = self.matches.len();
            let plus = if self.truncated { "+" } else { "" };
            format!("{}/{total}{plus}", total - current)
        } else {
            "No results".into()
        }
    }

    /// Clears the query if it is all selected. Returns whether it was.
    fn take_selection(&mut self) -> bool {
        let selected = std::mem::take(&mut self.all_selected);
        if selected {
            self.query.text.clear();
        }
        selected
    }

    fn changed(&mut self) {
        self.generation += 1;
    }
}

/// The highlighted cells of each viewport line: `(first column, last column,
/// is current)`. `top` is the buffer line on the first viewport line.
pub fn visible_spans(
    matches: &[FindMatch],
    current: Option<usize>,
    top: usize,
    rows: usize,
    columns: usize,
) -> Vec<Vec<(usize, usize, bool)>> {
    let mut lines = vec![Vec::new(); rows];
    if rows == 0 || columns == 0 {
        return lines;
    }
    let bottom = top + rows - 1;
    // Matches do not overlap, so their ends are sorted like their starts.
    let first = matches.partition_point(|m| m.end.line < top);
    for (index, m) in matches.iter().enumerate().skip(first) {
        if m.start.line > bottom {
            break;
        }
        for line in m.start.line.max(top)..=m.end.line.min(bottom) {
            let start = if line == m.start.line { m.start.column } else { 0 };
            let end = if line == m.end.line { m.end.column } else { columns - 1 };
            lines[line - top].push((start, end.min(columns - 1), current == Some(index)));
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use term_session::BufferPoint;

    fn span(line: usize, first: usize, last: usize) -> FindMatch {
        FindMatch {
            start: BufferPoint { line, column: first },
            end: BufferPoint { line, column: last },
        }
    }

    fn results(matches: Vec<FindMatch>) -> Result<FindResults, String> {
        Ok(FindResults {
            matches,
            truncated: false,
        })
    }

    #[test]
    fn typing_replaces_a_selected_query() {
        let mut find = FindState::new(
            FindQuery {
                text: "old".into(),
                ..FindQuery::default()
            },
            true,
        );
        find.push_text("n");
        assert_eq!(find.query.text, "n");
        find.push_text("ew\n");
        assert_eq!(find.query.text, "new");
        find.pop_char();
        assert_eq!(find.query.text, "ne");
        assert_eq!(find.generation, 3);
    }

    #[test]
    fn backspace_on_a_selected_query_clears_it() {
        let mut find = FindState::new(
            FindQuery {
                text: "old".into(),
                ..FindQuery::default()
            },
            true,
        );
        find.pop_char();
        assert_eq!(find.query.text, "");
    }

    #[test]
    fn delete_word_removes_the_last_word() {
        let mut find = FindState::default();
        find.push_text("cargo build  ");
        find.delete_word();
        assert_eq!(find.query.text, "cargo ");
        find.delete_word();
        assert_eq!(find.query.text, "");
    }

    #[test]
    fn counts_from_the_newest_match() {
        let mut find = FindState::default();
        find.push_text("x");
        let generation = find.generation;
        assert!(find.set_results(
            generation,
            results(vec![span(0, 0, 0), span(5, 1, 1), span(9, 2, 2)]),
            Pick::Newest
        ));
        assert_eq!(find.current, Some(2));
        assert_eq!(find.counter(), "1/3");

        assert_eq!(find.next(), Some(span(5, 1, 1)));
        assert_eq!(find.counter(), "2/3");
        find.next();
        assert_eq!(find.counter(), "3/3");
        // Wraps back to the newest.
        assert_eq!(find.next(), Some(span(9, 2, 2)));
        assert_eq!(find.previous(), Some(span(0, 0, 0)));
    }

    #[test]
    fn counter_states() {
        let mut find = FindState::default();
        assert_eq!(find.counter(), "");
        find.push_text("zzz");
        assert!(find.set_results(find.generation, results(vec![]), Pick::Newest));
        assert_eq!(find.counter(), "No results");
        assert!(find.set_results(find.generation, Err("bad".into()), Pick::Newest));
        assert_eq!(find.counter(), "Invalid regex");
        assert_eq!(find.next(), None);
        assert!(find.set_results(
            find.generation,
            Ok(FindResults {
                matches: vec![span(1, 0, 0)],
                truncated: true
            }),
            Pick::Newest
        ));
        assert_eq!(find.counter(), "1/1+");
    }

    #[test]
    fn results_for_an_older_query_are_dropped() {
        let mut find = FindState::default();
        find.push_text("a");
        let old = find.generation;
        find.push_text("b");
        assert!(!find.set_results(old, results(vec![span(0, 0, 0)]), Pick::Newest));
        assert!(find.matches.is_empty());
    }

    #[test]
    fn a_rerun_keeps_the_current_match() {
        let mut find = FindState::default();
        find.push_text("x");
        let g = find.generation;
        find.set_results(g, results(vec![span(1, 0, 0), span(4, 0, 0), span(8, 0, 0)]), Pick::Newest);
        find.next();
        assert_eq!(find.current_match(), Some(span(4, 0, 0)));

        // New output added a match at the bottom: the current one stays.
        find.set_results(
            g,
            results(vec![span(1, 0, 0), span(4, 0, 0), span(8, 0, 0), span(12, 0, 0)]),
            Pick::Keep,
        );
        assert_eq!(find.current_match(), Some(span(4, 0, 0)));
        assert_eq!(find.counter(), "3/4");

        // The current match is gone: the nearest older one takes its place.
        find.set_results(g, results(vec![span(1, 0, 0), span(12, 0, 0)]), Pick::Keep);
        assert_eq!(find.current_match(), Some(span(1, 0, 0)));
    }

    #[test]
    fn spans_cover_the_visible_lines_only() {
        let wrapped = FindMatch {
            start: BufferPoint { line: 11, column: 8 },
            end: BufferPoint { line: 12, column: 1 },
        };
        let matches = [span(2, 0, 3), span(10, 4, 6), wrapped, span(20, 0, 0)];
        let lines = visible_spans(&matches, Some(2), 10, 3, 10);
        assert_eq!(
            lines,
            vec![
                vec![(4, 6, false)],
                vec![(8, 9, true)],
                vec![(0, 1, true)],
            ]
        );
        assert!(visible_spans(&matches, None, 30, 3, 10).iter().all(Vec::is_empty));
    }
}
