//! Completion from spec files. A spec describes one command: its options and
//! its subcommands, each with a short description. Specs are TOML data, so
//! loading one runs no code.
//!
//! This module reads the command line before the cursor and lists the
//! candidates for the word under it. The shell fallback is in [`crate::zsh`],
//! and file names are listed by the app crate.

use std::collections::BTreeMap;

use serde::Deserialize;

/// One command's spec. A spec file is one of these at its top level.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Spec {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Options of this command. A parent's options also apply below it.
    #[serde(default, rename = "option")]
    pub options: Vec<Flag>,
    #[serde(default, rename = "subcommand")]
    pub subcommands: Vec<Spec>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Flag {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// The option takes the next word as its value, as `-m` does in `git commit -m`.
    #[serde(default)]
    pub takes_value: bool,
}

/// The specs in use, by command name. A spec added later with the same name
/// replaces the earlier one.
#[derive(Debug, Default, Clone)]
pub struct Specs {
    commands: BTreeMap<String, Spec>,
}

impl Specs {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds the spec in a TOML file's text.
    pub fn add_toml(&mut self, text: &str) -> Result<(), toml::de::Error> {
        let spec: Spec = toml::from_str(text)?;
        self.commands.insert(spec.name.clone(), spec);
        Ok(())
    }

    pub fn get(&self, command: &str) -> Option<&Spec> {
        self.commands.get(command)
    }
}

/// The specs that ship with Bloco.
pub fn builtin() -> Specs {
    let mut specs = Specs::new();
    let files = [
        include_str!("../specs/git.toml"),
        include_str!("../specs/docker.toml"),
        include_str!("../specs/kubectl.toml"),
        include_str!("../specs/cargo.toml"),
        include_str!("../specs/npm.toml"),
    ];
    for text in files {
        specs
            .add_toml(text)
            .expect("bundled completion spec must parse");
    }
    specs
}

/// One candidate in the completion menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// The whole word this candidate makes of the word under the cursor.
    pub text: String,
    pub description: String,
}

impl Item {
    /// The text inserted when the item is accepted. A space follows a finished
    /// word. A directory path (`src/`) or an option that takes `=` gets none.
    pub fn inserted(&self) -> String {
        if self.text.ends_with('/') || self.text.ends_with('=') {
            self.text.clone()
        } else {
            format!("{} ", self.text)
        }
    }
}

/// The candidates for the word under the cursor. `start` is the char index in
/// the text before the cursor where that word begins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub start: usize,
    pub items: Vec<Item>,
}

/// The command line before the cursor, split at the word being completed.
#[derive(Debug, PartialEq, Eq)]
pub struct Position<'a> {
    /// The current command, from its first word to the cursor. Pipes, `;` and
    /// `&&` start a new command.
    pub segment: &'a str,
    /// The words before the one under the cursor, in this command. The first is the command name.
    pub words: Vec<&'a str>,
    /// The word under the cursor. Empty after a space.
    pub current: &'a str,
    /// The char index, in the whole text, where `current` begins.
    pub start: usize,
}

/// The byte index just after the last char of `text` that matches, or 0.
fn after_last(text: &str, matches: impl Fn(char) -> bool) -> usize {
    text.char_indices()
        .rev()
        .find(|&(_, c)| matches(c))
        .map_or(0, |(index, c)| index + c.len_utf8())
}

/// Splits the text before the cursor into the command and the word under the cursor.
pub fn position(before: &str) -> Position<'_> {
    let segment_at = after_last(before, |c| matches!(c, '|' | ';' | '&' | '(' | '\n'));
    let segment = &before[segment_at..];
    let current_at = after_last(segment, char::is_whitespace);
    let start = before[..segment_at + current_at].chars().count();
    Position {
        segment,
        words: segment[..current_at].split_whitespace().collect(),
        current: &segment[current_at..],
        start,
    }
}

/// The candidates from the spec of the command under the cursor.
///
/// Returns `None` when there is no spec for the command, when the cursor is on
/// the command name, or when a quote is open. An empty list means the spec has
/// nothing for this word (a branch name, say). The caller then asks the shell.
pub fn from_spec(specs: &Specs, before: &str) -> Option<Completion> {
    let position = position(before);
    let quotes = position
        .segment
        .chars()
        .filter(|&c| c == '"' || c == '\'')
        .count();
    if quotes % 2 == 1 {
        return None;
    }
    let (command, args) = position.words.split_first()?;
    let spec = specs.get(command)?;

    let mut node = spec;
    let mut flags: Vec<&Flag> = spec.options.iter().collect();
    let mut value_next = false;
    for word in args {
        if std::mem::take(&mut value_next) {
            continue;
        }
        if word.starts_with('-') {
            value_next = flags
                .iter()
                .any(|flag| flag.name == *word && flag.takes_value);
        } else if let Some(sub) = node.subcommands.iter().find(|sub| sub.name == *word) {
            node = sub;
            flags.extend(sub.options.iter());
        }
    }

    let current = position.current;
    let items: Vec<Item> = if current.starts_with('-') {
        let mut items: Vec<Item> = Vec::new();
        for flag in flags.iter().filter(|flag| flag.name.starts_with(current)) {
            if !items.iter().any(|item| item.text == flag.name) {
                items.push(Item {
                    text: flag.name.clone(),
                    description: flag.description.clone(),
                });
            }
        }
        items
    } else {
        node.subcommands
            .iter()
            .filter(|sub| sub.name.starts_with(current))
            .map(|sub| Item {
                text: sub.name.clone(),
                description: sub.description.clone(),
            })
            .collect()
    };
    Some(Completion {
        start: position.start,
        items,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIT: &str = r#"
name = "git"
description = "Version control"

[[option]]
name = "-C"
description = "Run as if started in this directory"
takes_value = true

[[subcommand]]
name = "commit"
description = "Record changes"

[[subcommand.option]]
name = "-m"
description = "Use this message"
takes_value = true

[[subcommand.option]]
name = "--amend"
description = "Replace the last commit"

[[subcommand]]
name = "checkout"
description = "Switch branches"

[[subcommand]]
name = "remote"
description = "Manage remotes"

[[subcommand.subcommand]]
name = "add"
description = "Add a remote"
"#;

    fn git_specs() -> Specs {
        let mut specs = Specs::new();
        specs.add_toml(GIT).unwrap();
        specs
    }

    fn texts(completion: &Completion) -> Vec<&str> {
        completion
            .items
            .iter()
            .map(|item| item.text.as_str())
            .collect()
    }

    #[test]
    fn position_splits_the_current_word_and_the_words_before_it() {
        let found = position("git commit -m fi");
        assert_eq!(found.words, vec!["git", "commit", "-m"]);
        assert_eq!(found.current, "fi");
        assert_eq!(found.start, 14);
    }

    #[test]
    fn position_after_a_space_has_an_empty_current_word() {
        let found = position("git ");
        assert_eq!(found.words, vec!["git"]);
        assert_eq!(found.current, "");
        assert_eq!(found.start, 4);
    }

    #[test]
    fn position_starts_a_new_command_after_a_pipe() {
        let found = position("ls -la | gi");
        assert_eq!(found.segment, " gi");
        assert_eq!(found.words, Vec::<&str>::new());
        assert_eq!(found.current, "gi");
        assert_eq!(found.start, 9);
    }

    #[test]
    fn position_counts_chars_not_bytes_for_the_start() {
        // "ação" has more bytes than chars, so the start must count chars.
        let found = position("echo ação co");
        assert_eq!(found.current, "co");
        assert_eq!(found.start, 10);
    }

    #[test]
    fn subcommands_match_the_typed_prefix() {
        let completion = from_spec(&git_specs(), "git ch").unwrap();
        assert_eq!(completion.start, 4);
        assert_eq!(texts(&completion), vec!["checkout"]);
    }

    #[test]
    fn after_the_command_all_subcommands_are_listed() {
        let completion = from_spec(&git_specs(), "git ").unwrap();
        assert_eq!(texts(&completion), vec!["commit", "checkout", "remote"]);
    }

    #[test]
    fn a_dash_lists_the_options_of_the_command_and_its_parents() {
        let completion = from_spec(&git_specs(), "git commit --").unwrap();
        assert_eq!(texts(&completion), vec!["--amend"]);
        let completion = from_spec(&git_specs(), "git commit -").unwrap();
        assert_eq!(texts(&completion), vec!["-C", "-m", "--amend"]);
    }

    #[test]
    fn a_value_word_is_skipped_when_looking_for_the_subcommand() {
        // `-C` takes the directory, so `commit` after it is still the subcommand.
        let completion = from_spec(&git_specs(), "git -C /tmp ch").unwrap();
        assert_eq!(texts(&completion), vec!["checkout"]);
        let completion = from_spec(&git_specs(), "git -C /tmp commit -").unwrap();
        assert_eq!(texts(&completion), vec!["-C", "-m", "--amend"]);
    }

    #[test]
    fn nested_subcommands_are_found() {
        let completion = from_spec(&git_specs(), "git remote a").unwrap();
        assert_eq!(texts(&completion), vec!["add"]);
    }

    #[test]
    fn an_argument_the_spec_does_not_know_gives_no_items() {
        let completion = from_spec(&git_specs(), "git checkout ma").unwrap();
        assert!(completion.items.is_empty());
    }

    #[test]
    fn no_spec_or_the_command_name_gives_none() {
        assert_eq!(from_spec(&git_specs(), "nope "), None);
        assert_eq!(from_spec(&git_specs(), "gi"), None);
    }

    #[test]
    fn an_open_quote_gives_none() {
        assert_eq!(from_spec(&git_specs(), "git commit -m \"fix"), None);
    }

    #[test]
    fn inserted_text_gets_a_space_except_after_a_path_or_equals() {
        let item = |text: &str| Item {
            text: text.into(),
            description: String::new(),
        };
        assert_eq!(item("commit").inserted(), "commit ");
        assert_eq!(item("src/").inserted(), "src/");
        assert_eq!(item("--message=").inserted(), "--message=");
    }

    #[test]
    fn the_bundled_specs_all_load() {
        let specs = builtin();
        for name in ["git", "docker", "kubectl", "cargo", "npm"] {
            assert!(specs.get(name).is_some(), "missing spec for {name}");
        }
    }
}
