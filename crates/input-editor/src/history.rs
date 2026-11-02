//! Shell history: the commands the user ran, read from the history file and
//! extended with the commands typed in this session. Up and Down walk it, and
//! Ctrl+R searches it.

/// Commands oldest first. A command already at the end is not stored twice.
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct History {
    entries: Vec<String>,
}

impl History {
    pub fn new(entries: Vec<String>) -> Self {
        let mut history = Self::default();
        for entry in entries {
            history.push(&entry);
        }
        history
    }

    /// Reads the commands from the text of a history file. Handles zsh's
    /// extended format (`: 1700000000:0;command`), bash's `#timestamp` lines,
    /// and plain one-command-per-line files. A line ending in `\` continues
    /// on the next line.
    pub fn from_file_text(text: &str) -> Self {
        let mut entries = Vec::new();
        let mut pending: Option<String> = None;
        for line in text.lines() {
            let (body, continues) = match line.strip_suffix('\\') {
                Some(rest) => (rest, true),
                None => (line, false),
            };
            let body = match pending.take() {
                Some(mut earlier) => {
                    earlier.push('\n');
                    earlier.push_str(body);
                    earlier
                }
                None => {
                    if is_bash_timestamp(line) {
                        continue;
                    }
                    strip_zsh_prefix(body).to_string()
                }
            };
            if continues {
                pending = Some(body);
            } else {
                entries.push(body);
            }
        }
        if let Some(rest) = pending {
            entries.push(rest);
        }
        Self::new(entries)
    }

    /// Adds a command at the end. Empty commands and a repeat of the newest
    /// one are skipped.
    pub fn push(&mut self, command: &str) {
        let command = command.trim_end_matches('\n');
        if command.trim().is_empty() || self.entries.last().map(String::as_str) == Some(command) {
            return;
        }
        self.entries.push(command.to_string());
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, index: usize) -> Option<&str> {
        self.entries.get(index).map(String::as_str)
    }

    /// Indices of the entries that contain `query`, ignoring case, newest first.
    /// An empty query matches everything.
    pub fn matching(&self, query: &str) -> Vec<usize> {
        let query = query.to_lowercase();
        (0..self.entries.len())
            .rev()
            .filter(|&i| self.entries[i].to_lowercase().contains(&query))
            .collect()
    }
}

/// A `#` followed by digits is bash's timestamp line, not a command.
fn is_bash_timestamp(line: &str) -> bool {
    line.strip_prefix('#')
        .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
}

/// Removes zsh's `: time:duration;` prefix. Lines without it are unchanged.
fn strip_zsh_prefix(line: &str) -> &str {
    let Some(rest) = line.strip_prefix(": ") else {
        return line;
    };
    match rest.split_once(';') {
        Some((meta, command)) if meta.chars().all(|c| c.is_ascii_digit() || c == ':') => command,
        _ => line,
    }
}

/// Up and Down through the history. The text being typed when Up is first
/// pressed is kept as the draft, and Down past the newest entry brings it back.
#[derive(Default, Debug)]
pub struct Walk {
    index: Option<usize>,
    draft: String,
}

impl Walk {
    /// The entry one step older, or `None` when there is none. `current` is
    /// what the buffer shows now; it becomes the draft on the first step.
    pub fn older(&mut self, history: &History, current: &str) -> Option<String> {
        let next = match self.index {
            None => {
                if history.is_empty() {
                    return None;
                }
                self.draft = current.to_string();
                history.len() - 1
            }
            Some(0) => return None,
            Some(i) => i - 1,
        };
        self.index = Some(next);
        history.get(next).map(str::to_string)
    }

    /// The entry one step newer. Past the newest entry this is the draft.
    /// `None` when the walk has not started.
    pub fn newer(&mut self, history: &History) -> Option<String> {
        let current = self.index?;
        if current + 1 < history.len() {
            self.index = Some(current + 1);
            history.get(current + 1).map(str::to_string)
        } else {
            self.index = None;
            Some(std::mem::take(&mut self.draft))
        }
    }

    /// Ends the walk, for example after the line is submitted or edited.
    pub fn reset(&mut self) {
        self.index = None;
        self.draft.clear();
    }

    pub fn is_active(&self) -> bool {
        self.index.is_some()
    }
}

/// The state of a Ctrl+R search: the query, the matching entries newest
/// first, and which one is selected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistorySearch {
    query: String,
    matches: Vec<usize>,
    selected: usize,
}

impl HistorySearch {
    /// A search with an empty query, which shows the newest entry first.
    pub fn new(history: &History) -> Self {
        Self {
            query: String::new(),
            matches: history.matching(""),
            selected: 0,
        }
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    /// The entry that Enter would take, or `None` when nothing matches.
    pub fn current<'a>(&self, history: &'a History) -> Option<&'a str> {
        let index = *self.matches.get(self.selected)?;
        history.get(index)
    }

    /// How many entries match the query.
    pub fn match_count(&self) -> usize {
        self.matches.len()
    }

    pub fn type_char(&mut self, history: &History, c: char) {
        self.query.push(c);
        self.refresh(history);
    }

    pub fn backspace(&mut self, history: &History) {
        self.query.pop();
        self.refresh(history);
    }

    /// Selects the next older match (Ctrl+R again).
    pub fn older(&mut self) {
        if self.selected + 1 < self.matches.len() {
            self.selected += 1;
        }
    }

    /// Selects the next newer match.
    pub fn newer(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    fn refresh(&mut self, history: &History) {
        self.matches = history.matching(&self.query);
        self.selected = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> History {
        History::new(vec![
            "ls".to_string(),
            "git status".to_string(),
            "cargo test".to_string(),
        ])
    }

    #[test]
    fn zsh_extended_format_loses_its_prefix() {
        let history =
            History::from_file_text(": 1700000000:0;git status\n: 1700000001:2;cargo test\n");
        assert_eq!(history.get(0), Some("git status"));
        assert_eq!(history.get(1), Some("cargo test"));
    }

    #[test]
    fn bash_timestamps_are_skipped_and_plain_lines_kept() {
        let history = History::from_file_text("#1700000000\nls\n#1700000001\ncd ..\n");
        assert_eq!(history.len(), 2);
        assert_eq!(history.get(1), Some("cd .."));
    }

    #[test]
    fn backslash_continues_a_multi_line_command() {
        let history = History::from_file_text("echo one \\\n  two\nls\n");
        assert_eq!(history.get(0), Some("echo one \n  two"));
        assert_eq!(history.get(1), Some("ls"));
    }

    #[test]
    fn push_skips_empty_and_repeated_commands() {
        let mut history = History::default();
        history.push("ls");
        history.push("ls");
        history.push("   ");
        history.push("pwd\n");
        assert_eq!(history.len(), 2);
        assert_eq!(history.get(1), Some("pwd"));
    }

    #[test]
    fn matching_is_case_insensitive_and_newest_first() {
        let history = History::new(vec![
            "Git log".to_string(),
            "ls".to_string(),
            "git status".to_string(),
        ]);
        assert_eq!(history.matching("GIT"), vec![2, 0]);
        assert_eq!(history.matching("").len(), 3);
        assert!(history.matching("zzz").is_empty());
    }

    #[test]
    fn walk_goes_back_and_returns_the_draft() {
        let history = sample();
        let mut walk = Walk::default();
        assert_eq!(walk.older(&history, "dra"), Some("cargo test".to_string()));
        assert_eq!(
            walk.older(&history, "ignored"),
            Some("git status".to_string())
        );
        assert_eq!(walk.older(&history, ""), Some("ls".to_string()));
        assert_eq!(walk.older(&history, ""), None);
        assert_eq!(walk.newer(&history), Some("git status".to_string()));
        assert_eq!(walk.newer(&history), Some("cargo test".to_string()));
        assert_eq!(walk.newer(&history), Some("dra".to_string()));
        assert!(!walk.is_active());
        assert_eq!(walk.newer(&history), None);
    }

    #[test]
    fn walk_on_empty_history_does_nothing() {
        let mut walk = Walk::default();
        assert_eq!(walk.older(&History::default(), "x"), None);
    }

    #[test]
    fn search_filters_as_the_query_grows() {
        let history = sample();
        let mut search = HistorySearch::new(&history);
        assert_eq!(search.current(&history), Some("cargo test"));
        search.type_char(&history, 'g');
        search.type_char(&history, 'i');
        assert_eq!(search.query(), "gi");
        assert_eq!(search.match_count(), 1);
        assert_eq!(search.current(&history), Some("git status"));
        search.backspace(&history);
        search.backspace(&history);
        assert_eq!(search.match_count(), 3);
    }

    #[test]
    fn search_cycles_through_matches_and_stops_at_the_ends() {
        let history = History::new(vec!["make a".to_string(), "make b".to_string()]);
        let mut search = HistorySearch::new(&history);
        search.type_char(&history, 'm');
        assert_eq!(search.current(&history), Some("make b"));
        search.older();
        assert_eq!(search.current(&history), Some("make a"));
        search.older();
        assert_eq!(search.current(&history), Some("make a"));
        search.newer();
        assert_eq!(search.current(&history), Some("make b"));
        search.newer();
        assert_eq!(search.current(&history), Some("make b"));
    }

    #[test]
    fn search_with_no_match_has_no_current() {
        let history = sample();
        let mut search = HistorySearch::new(&history);
        for c in "nothing".chars() {
            search.type_char(&history, c);
        }
        assert_eq!(search.current(&history), None);
    }
}
