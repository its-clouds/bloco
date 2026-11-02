//! The command palette: the query, the fuzzy filter over the command registry,
//! and the shortcut text shown next to each command.

use crate::commands::{COMMANDS, Command};

/// What the palette shows: the typed query and the commands that match it.
#[derive(Debug, Default)]
pub struct PaletteState {
    pub query: String,
    /// Indices into [`COMMANDS`], best match first.
    pub matches: Vec<usize>,
    /// The highlighted row in `matches`.
    pub selected: usize,
    /// Shortcut text for each entry of [`COMMANDS`]. Empty when unbound.
    pub shortcuts: Vec<String>,
}

impl PaletteState {
    pub fn new(shortcuts: Vec<String>) -> Self {
        let mut state = Self {
            shortcuts,
            ..Self::default()
        };
        state.refilter();
        state
    }

    pub fn push_text(&mut self, text: &str) {
        self.query.extend(text.chars().filter(|c| !c.is_control()));
        self.refilter();
    }

    pub fn pop_char(&mut self) {
        self.query.pop();
        self.refilter();
    }

    pub fn move_selection(&mut self, delta: isize) {
        let len = self.matches.len() as isize;
        if len > 0 {
            self.selected = (self.selected as isize + delta).rem_euclid(len) as usize;
        }
    }

    /// The command under the highlight, if any command matches.
    pub fn selected_command(&self) -> Option<&'static Command> {
        self.matches.get(self.selected).map(|&index| &COMMANDS[index])
    }

    fn refilter(&mut self) {
        self.matches = filter(&self.query, COMMANDS);
        self.selected = 0;
    }
}

/// The commands whose titles match `query`, best first. Ties keep the
/// registry order.
pub fn filter(query: &str, commands: &[Command]) -> Vec<usize> {
    let mut scored: Vec<(u32, usize)> = commands
        .iter()
        .enumerate()
        .filter_map(|(index, command)| score(query, command.title).map(|s| (s, index)))
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, index)| index).collect()
}

/// A fuzzy score: every character of `query` must appear in `title`, in
/// order, ignoring case. Matches at the start of a word and runs of matched
/// characters score higher. `None` when `query` does not match.
pub fn score(query: &str, title: &str) -> Option<u32> {
    let mut score = 0;
    let mut title_chars = title.chars().enumerate().peekable();
    let mut previous_match: Option<usize> = None;
    for wanted in query.chars().filter(|c| !c.is_whitespace()) {
        let wanted = wanted.to_ascii_lowercase();
        let (index, _) = title_chars.find(|(_, c)| c.to_ascii_lowercase() == wanted)?;
        score += 1;
        if index == 0 || title.chars().nth(index - 1).is_some_and(|p| p == ' ') {
            score += 4;
        }
        if previous_match.is_some_and(|previous| previous + 1 == index) {
            score += 3;
        }
        previous_match = Some(index);
    }
    Some(score)
}

/// Turns GPUI's keystroke text (`cmd-shift-p`) into macOS symbols (`⇧⌘P`).
/// Chords keep their spaces.
pub fn pretty_keys(text: &str) -> String {
    text.split(' ')
        .map(|stroke| {
            let mut pieces: Vec<&str> = stroke.split('-').collect();
            let key = pieces.pop().unwrap_or_default();
            let mut out = String::new();
            for (name, symbol) in [("ctrl", "⌃"), ("alt", "⌥"), ("shift", "⇧"), ("cmd", "⌘")] {
                if pieces.contains(&name) {
                    out.push_str(symbol);
                }
            }
            if key.chars().count() == 1 {
                out.push_str(&key.to_uppercase());
            } else {
                out.push_str(key);
            }
            out
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_characters_must_appear_in_order() {
        assert!(score("nt", "New tab").is_some());
        assert!(score("tn", "New tab").is_none());
        assert!(score("zzz", "New tab").is_none());
        assert_eq!(score("", "anything"), Some(0));
    }

    #[test]
    fn word_starts_and_runs_score_higher() {
        let word_start = score("sp", "Split pane").unwrap();
        let mid_word = score("li", "Split pane").unwrap();
        assert!(word_start > mid_word);
    }

    #[test]
    fn filter_orders_best_matches_first() {
        let commands = [
            Command { id: "a", title: "Close pane", keys: &[] },
            Command { id: "b", title: "New tab", keys: &[] },
            Command { id: "c", title: "Split right", keys: &[] },
        ];
        assert_eq!(filter("sp", &commands), vec![2, 0]);
        assert_eq!(filter("", &commands), vec![0, 1, 2]);
        assert_eq!(filter("tab", &commands), vec![1]);
    }

    #[test]
    fn pretty_keys_uses_symbols() {
        assert_eq!(pretty_keys("cmd-shift-p"), "⇧⌘P");
        assert_eq!(pretty_keys("cmd-alt-left"), "⌥⌘left");
        assert_eq!(pretty_keys("cmd-k cmd-t"), "⌘K ⌘T");
    }
}
