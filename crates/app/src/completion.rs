//! Completion for the input editor, as the app runs it. A spec from the
//! `input-editor` crate answers first. For a word no spec covers, zsh's own
//! completion answers. File names in the shell's directory are the last resort.
//!
//! zsh takes a few hundred milliseconds, so the app calls [`complete`] on a
//! background thread.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use input_editor::completion::{Completion, Item, Specs, from_spec, position};
use input_editor::zsh;

/// zsh is killed after this long, so a slow `.zshrc`-free start can't hang the menu.
const ZSH_TIMEOUT: Duration = Duration::from_secs(3);

/// The candidates for the word before the cursor. `before` is the buffer text
/// up to the cursor, and `cwd` is the shell's directory when known.
pub fn complete(before: &str, cwd: Option<&Path>, specs: &Specs) -> Completion {
    if let Some(found) = from_spec(specs, before).filter(|found| !found.items.is_empty()) {
        return found;
    }
    let found = position(before);
    let start = found.start;
    let current = found.current.to_string();
    if !is_path(&current) {
        let items = zsh_items(found.segment, cwd, &current);
        if !items.is_empty() {
            return Completion { start, items };
        }
    }
    Completion {
        start,
        items: path_items(cwd, &current),
    }
}

/// Whether a word names a path, so file names are the candidates and zsh is not asked.
fn is_path(word: &str) -> bool {
    word.contains('/') || word.starts_with('~') || word.starts_with('.')
}

fn zsh_items(segment: &str, cwd: Option<&Path>, current: &str) -> Vec<Item> {
    let Some(output) = run_zsh(segment, cwd) else {
        return Vec::new();
    };
    zsh::parse(&output)
        .into_iter()
        .filter(|word| word.starts_with(current))
        .map(|text| Item {
            text,
            description: String::new(),
        })
        .collect()
}

/// Runs the zsh driver with the segment of the line, and returns what it printed.
fn run_zsh(segment: &str, cwd: Option<&Path>) -> Option<String> {
    let mut command = Command::new("zsh");
    command
        .args(["-f", "-c", zsh::DRIVER, "bloco", segment])
        .env("BLOCO_SETUP", zsh::SETUP)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let mut child = command.spawn().ok()?;
    // Read on another thread, so a full pipe cannot stall the wait below.
    let mut stdout = child.stdout.take()?;
    let reader = thread::spawn(move || {
        let mut text = String::new();
        stdout.read_to_string(&mut text).ok().map(|_| text)
    });
    let deadline = Instant::now() + ZSH_TIMEOUT;
    loop {
        if child.try_wait().ok()?.is_some() {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        thread::sleep(Duration::from_millis(5));
    }
    reader.join().ok()?
}

/// File and directory names in the directory `word` points into, that start
/// with the part of `word` after its last `/`. Directories get a trailing `/`.
fn path_items(cwd: Option<&Path>, word: &str) -> Vec<Item> {
    let (dir_text, name) = match word.rfind('/') {
        Some(slash) => word.split_at(slash + 1),
        None => ("", word),
    };
    let Some(dir) = expand_dir(dir_text, cwd) else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut items: Vec<Item> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let file_name = entry.file_name().into_string().ok()?;
            if !file_name.starts_with(name) {
                return None;
            }
            // Hidden files only when the word starts with a dot, as the shell does.
            if file_name.starts_with('.') && !name.starts_with('.') {
                return None;
            }
            let is_dir = entry.path().is_dir();
            Some(Item {
                text: format!("{dir_text}{file_name}{}", if is_dir { "/" } else { "" }),
                description: if is_dir {
                    "directory".to_string()
                } else {
                    String::new()
                },
            })
        })
        .collect();
    items.sort_by(|a, b| a.text.cmp(&b.text));
    items
}

/// The directory a typed prefix names: `~` is the home directory, a relative
/// path is taken from `cwd`. An empty prefix is `cwd` itself.
fn expand_dir(dir_text: &str, cwd: Option<&Path>) -> Option<PathBuf> {
    if let Some(rest) = dir_text
        .strip_prefix("~/")
        .or((dir_text == "~").then_some(""))
    {
        let home = PathBuf::from(std::env::var_os("HOME")?);
        return Some(home.join(rest));
    }
    if dir_text.starts_with('/') {
        return Some(PathBuf::from(dir_text));
    }
    let base = match cwd {
        Some(cwd) => cwd.to_path_buf(),
        None => std::env::current_dir().ok()?,
    };
    Some(base.join(dir_text))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(items: &[Item]) -> Vec<&str> {
        items.iter().map(|item| item.text.as_str()).collect()
    }

    /// A fresh directory with a few entries, removed when the test is done.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("bloco-completion-{name}"));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(dir.join("src")).unwrap();
            fs::write(dir.join("Cargo.toml"), "").unwrap();
            fs::write(dir.join(".env"), "").unwrap();
            Self(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn path_items_list_matching_names_and_mark_directories() {
        let scratch = Scratch::new("names");
        let items = path_items(Some(&scratch.0), "");
        assert_eq!(texts(&items), vec!["Cargo.toml", "src/"]);
        let items = path_items(Some(&scratch.0), "sr");
        assert_eq!(texts(&items), vec!["src/"]);
    }

    #[test]
    fn path_items_keep_the_typed_directory_and_hide_dotfiles_unless_asked() {
        let scratch = Scratch::new("dirs");
        let items = path_items(Some(&scratch.0), "src/");
        assert!(items.is_empty());
        let items = path_items(Some(&scratch.0), ".");
        assert_eq!(texts(&items), vec![".env"]);
        let items = path_items(Some(&scratch.0), "C");
        assert_eq!(texts(&items), vec!["Cargo.toml"]);
    }

    #[test]
    fn a_spec_answers_before_the_shell_or_files() {
        let specs = input_editor::completion::builtin();
        let found = complete("git ch", None, &specs);
        assert_eq!(found.start, 4);
        assert!(texts_of(&found).contains(&"checkout"));
    }

    #[test]
    fn file_names_answer_for_a_plain_word() {
        let scratch = Scratch::new("paths");
        let specs = Specs::new();
        let found = complete("cat Car", Some(&scratch.0), &specs);
        assert_eq!(texts_of(&found), vec!["Cargo.toml"]);
    }

    #[test]
    fn zsh_completes_a_git_subcommand_it_has_no_spec_for() {
        // No spec is loaded, so the shell's own completion of `git` answers.
        let specs = Specs::new();
        let found = complete("git ch", None, &specs);
        assert!(
            texts_of(&found).contains(&"checkout"),
            "{:?}",
            texts_of(&found)
        );
    }

    fn texts_of(found: &Completion) -> Vec<&str> {
        found.items.iter().map(|item| item.text.as_str()).collect()
    }
}
