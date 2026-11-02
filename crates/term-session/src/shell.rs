//! Shell integration: how each supported shell is started so that it reports
//! prompts, commands and the working directory (OSC 133 and OSC 7).
//!
//! The scripts are written to a private temporary directory, which is removed
//! when the session ends. The user's own files are read but never changed.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Context, Result};
use portable_pty::CommandBuilder;

const ZSH: &str = include_str!("../shell/integration.zsh");
const BASH: &str = include_str!("../shell/integration.bash");
const FISH: &str = include_str!("../shell/integration.fish");

/// Stands in for `bash -l`. A login shell reads the profile files and not the
/// rcfile, so the rcfile reads them first, then loads the integration.
const BASH_RC_HEAD: &str = r#"# Login files, read as `bash -l` would read them.
[ -r /etc/profile ] && . /etc/profile
for __term_session_file in "$HOME/.bash_profile" "$HOME/.bash_login" "$HOME/.profile"; do
  if [ -r "$__term_session_file" ]; then
    . "$__term_session_file"
    break
  fi
done
unset __term_session_file
"#;

/// The directory holding the generated scripts. Removed on drop.
pub(crate) struct ScriptDir(PathBuf);

impl Drop for ScriptDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Adds the arguments and environment that start `shell` (a path) as a login
/// shell with the integration. Returns the script directory, which must live
/// as long as the shell does. `None` for shells without integration.
pub(crate) fn configure(shell: &str, command: &mut CommandBuilder) -> Result<Option<ScriptDir>> {
    match name(shell) {
        Some("zsh") => {
            let dir = write_dir(&[
                (".zshenv", source_user_file(".zshenv")),
                (".zprofile", source_user_file(".zprofile")),
                (".zshrc", zshrc()),
            ])?;
            command.arg("-l");
            command.env("TERM_SESSION_USER_ZDOTDIR", user_zdotdir());
            command.env("ZDOTDIR", &dir.0);
            Ok(Some(dir))
        }
        Some("bash") => {
            let rc = format!("{BASH_RC_HEAD}{BASH}");
            let dir = write_dir(&[("bashrc", rc)])?;
            command.arg("--rcfile");
            command.arg(dir.0.join("bashrc"));
            command.arg("-i");
            Ok(Some(dir))
        }
        Some("fish") => {
            let dir = write_dir(&[("integration.fish", FISH.to_string())])?;
            let script = dir.0.join("integration.fish");
            // Single quotes in fish: only `'` and `\` need a backslash.
            let quoted = script
                .display()
                .to_string()
                .replace('\\', "\\\\")
                .replace('\'', "\\'");
            // fish 4 writes its own OSC 133 marks. They would double ours, so
            // they are turned off for this session.
            command.arg("--features");
            command.arg("no-mark-prompt");
            command.arg("-l");
            command.arg("-C");
            command.arg(format!("source '{quoted}'"));
            Ok(Some(dir))
        }
        _ => {
            command.arg("-l");
            Ok(None)
        }
    }
}

/// The shell's name without its directory: `zsh` for `/bin/zsh`.
fn name(shell: &str) -> Option<&str> {
    Path::new(shell).file_name().and_then(OsStr::to_str)
}

/// Writes `files` into a new directory unique to this process and session.
fn write_dir(files: &[(&str, String)]) -> Result<ScriptDir> {
    // Unique per session: several sessions can exist in one process (tabs, splits).
    static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let dir =
        ScriptDir(std::env::temp_dir().join(format!("term-session-{}-{id}", std::process::id())));
    std::fs::create_dir_all(&dir.0).context("create script directory")?;
    for (file, text) in files {
        std::fs::write(dir.0.join(file), text).with_context(|| format!("write {file}"))?;
    }
    Ok(dir)
}

/// Where the user keeps their zsh files: `$ZDOTDIR`, or `$HOME`.
fn user_zdotdir() -> PathBuf {
    std::env::var_os("ZDOTDIR")
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn source_user_file(name: &str) -> String {
    format!(
        "[[ -r \"$TERM_SESSION_USER_ZDOTDIR/{name}\" ]] && source \"$TERM_SESSION_USER_ZDOTDIR/{name}\"\n"
    )
}

/// The generated `.zshrc`. It sources the user's `.zshrc` first, then loads the
/// integration and gives `ZDOTDIR` back to the user's directory, so `.zlogin`
/// and child shells see the normal setup.
fn zshrc() -> String {
    format!(
        "{}{ZSH}\nexport ZDOTDIR=\"$TERM_SESSION_USER_ZDOTDIR\"\nunset TERM_SESSION_USER_ZDOTDIR\n",
        source_user_file(".zshrc")
    )
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;
    use crate::blocks::Row;
    use crate::session::Session;

    /// The path of the shell called `name` on `PATH`, if it is installed.
    fn find_shell(name: &str) -> Option<String> {
        let path = std::env::var_os("PATH")?;
        std::env::split_paths(&path)
            .map(|dir| dir.join(name))
            .find(|candidate| candidate.is_file())
            .map(|candidate| candidate.display().to_string())
    }

    /// Starts `name` with the integration, runs a command that exits with
    /// status 3, and waits for its block to close. Skipped when the shell is not
    /// installed.
    fn reports_command_and_exit_status(name: &str) {
        let Some(shell) = find_shell(name) else {
            eprintln!("skipping: {name} is not installed");
            return;
        };
        let session = Session::spawn_shell(&shell, 100, 24, None, |_| {}).unwrap();
        session.write(b"sh -c 'echo hello; exit 3'\r").unwrap();

        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(block) = session.blocks().iter().find(|b| b.exit_code.is_some()) {
                assert_eq!(
                    block.command.as_deref(),
                    Some("sh -c 'echo hello; exit 3'"),
                    "{name}"
                );
                assert_eq!(block.exit_code, Some(3), "{name}");
                assert!(session.cwd().is_some(), "{name}: no working directory");
                assert!(block.prompt_line.is_some(), "{name}: no prompt line");
                assert_eq!(
                    session.block_output(block.id).as_deref(),
                    Some("hello"),
                    "{name}"
                );
                // The finished block is drawn with a header row for its prompt and command.
                let frame = session.frame(None);
                assert!(
                    frame.rows.contains(&Row::Header(block.id)),
                    "{name}: {:?}",
                    frame.rows
                );
                assert_eq!(frame.headers.len(), 1, "{name}");
                return;
            }
            assert!(
                Instant::now() < deadline,
                "{name}: no finished block. Screen: {:?}",
                session.snapshot().text_lines()
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn bash_reports_command_and_exit_status() {
        reports_command_and_exit_status("bash");
    }

    #[test]
    fn zsh_reports_command_and_exit_status() {
        reports_command_and_exit_status("zsh");
    }

    #[test]
    fn fish_reports_command_and_exit_status() {
        reports_command_and_exit_status("fish");
    }

    /// Three commands with output. Scrolled into the first block's output it
    /// sticks its header to the top. Collapsed, its output takes no rows. (The
    /// frame fills from above at the end of the buffer, so the later blocks keep
    /// the first one at the top.)
    #[test]
    fn frames_stick_and_fold_blocks() {
        let Some(shell) = find_shell("bash") else {
            eprintln!("skipping: bash is not installed");
            return;
        };
        let session = Session::spawn_shell(&shell, 80, 10, None, |_| {}).unwrap();
        session.write(b"seq 1 60\r").unwrap();
        session.write(b"seq 1 60\r").unwrap();
        session.write(b"echo done\r").unwrap();

        let deadline = Instant::now() + Duration::from_secs(20);
        let blocks = loop {
            let blocks = session.blocks();
            if blocks.iter().filter(|b| b.exit_code.is_some()).count() >= 3 {
                break blocks;
            }
            assert!(Instant::now() < deadline, "no three finished blocks");
            std::thread::sleep(Duration::from_millis(50));
        };
        let first = &blocks[0];
        let output = first.output_line.unwrap();

        session.scroll_top_to(output + 10);
        let frame = session.frame(None);
        assert_eq!(frame.rows[0], Row::Line(output + 10));
        assert_eq!(frame.sticky.as_ref().map(|b| b.id), Some(first.id));

        session.toggle_block(first.id);
        session.scroll_top_to(first.prompt_line.unwrap());
        let frame = session.frame(None);
        assert_eq!(frame.rows[0], Row::Header(first.id), "{:?}", frame.rows);
        assert!(
            !frame.rows.contains(&Row::Line(output + 1)),
            "{:?}",
            frame.rows
        );
        assert!(frame.sticky.is_none());
    }

    /// Output past the scrollback limit moves the oldest lines out. The blocks
    /// created before and after that still read back their own output.
    #[test]
    fn block_positions_survive_a_full_scrollback() {
        let Some(shell) = find_shell("bash") else {
            eprintln!("skipping: bash is not installed");
            return;
        };
        let session = Session::spawn_shell(&shell, 80, 10, None, |_| {}).unwrap();
        for command in ["seq 1 9000", "seq 1 3000", "seq 1 30", "echo hi"] {
            session.write(format!("{command}\r").as_bytes()).unwrap();
        }

        let deadline = Instant::now() + Duration::from_secs(30);
        let blocks = loop {
            let blocks = session.blocks();
            if blocks.iter().filter(|b| b.exit_code.is_some()).count() >= 4 {
                break blocks;
            }
            assert!(Instant::now() < deadline, "no four finished blocks");
            std::thread::sleep(Duration::from_millis(50));
        };
        let output = |n: usize| {
            (1..=n)
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join("\n")
        };
        assert_eq!(session.block_output(blocks[1].id), Some(output(3000)));
        assert_eq!(session.block_output(blocks[2].id), Some(output(30)));
        assert_eq!(session.block_output(blocks[3].id), Some("hi".into()));
        assert!(session
            .frame(None)
            .rows
            .contains(&Row::Header(blocks[3].id)));
    }

    /// After the scrollback fills, the previous block is found from the view's
    /// top line, and its header can be scrolled to the top. The pane is taller
    /// than the last block, so its header is the first row on screen.
    /// After the scrollback fills, blocks are found from the newest one back. A
    /// block whose header is gone is not found.
    #[test]
    fn navigation_finds_blocks_after_a_full_scrollback() {
        let Some(shell) = find_shell("bash") else {
            eprintln!("skipping: bash is not installed");
            return;
        };
        let session = Session::spawn_shell(&shell, 80, 40, None, |_| {}).unwrap();
        for command in ["seq 1 12000", "seq 1 20", "echo hi"] {
            session.write(format!("{command}\r").as_bytes()).unwrap();
        }
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if session
                .blocks()
                .iter()
                .filter(|b| b.exit_code.is_some())
                .count()
                >= 3
            {
                break;
            }
            assert!(Instant::now() < deadline, "no three finished blocks");
            std::thread::sleep(Duration::from_millis(50));
        }
        // From the bottom of the view, the newest block comes first.
        let bottom = session.top_line() + 40;
        let newest = session.previous_block(bottom).expect("the newest block");
        assert_eq!(newest.command.as_deref(), Some("echo hi"));
        let previous = session.previous_block(newest.prompt_line.unwrap()).unwrap();
        assert_eq!(previous.command.as_deref(), Some("seq 1 20"));
        // The view cannot scroll above the scrollback, so this block stays where
        // it is on screen, with its header drawn and selected.
        session.scroll_top_to(previous.prompt_line.unwrap());
        let frame = session.frame(Some(previous.id));
        assert!(frame.rows.contains(&Row::Header(previous.id)));
        assert!(frame.selected.is_some());
        // The block before it lost its header to the full scrollback.
        assert!(session
            .previous_block(previous.prompt_line.unwrap())
            .is_none());
    }

    #[test]
    fn script_directory_is_removed_on_drop() {
        let dir = write_dir(&[("file", "text".into())]).unwrap();
        let path = dir.0.clone();
        assert!(path.join("file").exists());
        drop(dir);
        assert!(!path.exists());
    }
}
