# M5 exit checklist

M5 is done when Tab completes the commands you use every day in Bloco's input
editor, for a full week of normal work, and you stop reaching for the shell's
completion. This checklist covers the M5 features and the checks that can be run
before that week. Build it with:

```bash
cargo run -p app --release
```

Each row says who checked it. **Checked** means the change's author verified it
from outside the app (build, tests, a shell run through a real PTY). **Passed
(user)** means the user ran it in the window. **Pending** means it still needs a run.

Status as of this change:

- Build and tests: `cargo build --workspace` and `cargo test --workspace` pass.
- Spec logic: the position in the line, the subcommand and option matching, the
  value skipping and the bundled specs have unit tests (`cargo test -p input-editor completion`).
  The zsh output parser has tests (`cargo test -p input-editor zsh`).
- App side: the menu's selection, the cursor cell, the path listing and the zsh
  call have tests (`cargo test -p app`). The zsh test runs the real `zsh`.
- In the window: the user ran rows 7–33 on 2026-11-02 and all passed. Speed felt
  fine.

## Build and tests

| # | Check | How to check | Status |
| --- | --- | --- | --- |
| 1 | Debug build | `cargo build --workspace` | Checked |
| 2 | Tests | `cargo test --workspace` passes | Checked |
| 3 | Spec matching | `cargo test -p input-editor completion` passes | Checked |
| 4 | zsh output parsing | `cargo test -p input-editor zsh` passes | Checked |
| 5 | Real zsh fallback | `cargo test -p app completion` passes (runs `zsh`, so zsh must be on PATH) | Checked |
| 6 | Release build | `cargo build --release -p app` | Checked |

## Spec commands

Type each line at the prompt, press Tab, and check what it does. Accepting an
item with Enter inserts it with a trailing space (none after a directory or `=`).

| # | Check | How to check | Status |
| --- | --- | --- | --- |
| 7 | git subcommand, one match | Type `git ch`, Tab: `checkout ` is inserted. The line does not run | Passed (user) |
| 8 | git subcommands, menu | Type `git `, Tab: a menu lists `add`, `bisect`, … with a description at the right of each | Passed (user) |
| 9 | git options | Type `git commit -`, Tab: `-C`, `-m` and `--amend` are listed | Passed (user) |
| 10 | git value skipped | Type `git -C /tmp ch`, Tab: `checkout` is inserted | Passed (user) |
| 11 | docker subcommand | Type `docker ru`, Tab: `run ` is inserted | Passed (user) |
| 12 | docker compose | Type `docker compose u`, Tab: `up ` is inserted | Passed (user) |
| 13 | kubectl subcommand | Type `kubectl ge`, Tab: `get ` is inserted | Passed (user) |
| 14 | cargo subcommands | Type `cargo b`, Tab: a menu lists `build` and `bench` | Passed (user) |
| 15 | npm subcommand | Type `npm ru`, Tab: `run ` is inserted | Passed (user) |

## Menu keys

| # | Check | How to check | Status |
| --- | --- | --- | --- |
| 16 | Tab opens and moves | In the `git ` menu, Tab moves the highlight down, and the top row is selected first | Passed (user) |
| 17 | Arrows and Shift+Tab | Down and Up move the highlight. Shift+Tab moves up. The highlight wraps at both ends | Passed (user) |
| 18 | Enter accepts | Pick `checkout` with the arrows and press Enter: `git checkout ` is in the line, and nothing runs | Passed (user) |
| 19 | Escape closes | Open the menu and press Escape: it closes, and the line is unchanged | Passed (user) |
| 20 | Typing closes it | Open the menu and type a letter: it closes. Tab opens it again for what is typed | Passed (user) |
| 21 | The menu follows the cursor | Type `git ` in the middle of a long line and Tab: the menu opens under the cursor, on the row below | Passed (user) |
| 22 | Menu near the bottom | Open the menu with the prompt on the last rows: it moves up so it stays on screen | Passed (user) |
| 23 | Long list scrolls | Type `gi` at a new line and Tab: a long list opens, and Down past the eighth row scrolls it | Passed (user) |
| 24 | Enter still runs | With no menu open, Enter runs the line as before | Passed (user) |
| 25 | Single match is inserted | A word with one candidate is completed at once, with no menu | Passed (user) |

## Shell and file fallback

| # | Check | How to check | Status |
| --- | --- | --- | --- |
| 26 | Branch names fall back to zsh | In a git repo, type `git checkout ma`, Tab: `main` (or a matching branch) comes from zsh | Passed (user) |
| 27 | Directory names | Type `ls /us`, Tab: `/usr/` is inserted | Passed (user) |
| 28 | Home-relative paths | Type `cd ~/Doc`, Tab: a menu or `Documents/` appears | Passed (user) |
| 29 | Hidden files stay hidden | Type `ls ./`, Tab: dotfiles are not listed; type `ls ./.`, Tab: they are | Passed (user) |
| 30 | Command names | Type `gi` at the start of a line, Tab: a menu lists commands starting with `gi` | Passed (user) |
| 31 | After a pipe | Type `ls -la | gi`, Tab: command names are listed for the part after `|` | Passed (user) |
| 32 | Unknown command | Type `xyzzy `, Tab: zsh's default file names for the directory are offered, and the line is unchanged when nothing matches | Passed (user) |
| 33 | Speed | The menu appears within about half a second of Tab for a zsh lookup | Passed (user) |

## Known limitations

These are known and are not M5 failures. They are listed here so the week of use
can judge them.

- **zsh runs with `-f`.** Your `~/.zshrc`, fpath and plugins are not loaded, so
  oh-my-zsh and Homebrew completions for other commands are missing. The spec
  commands and file names still work.
- **zsh only.** The fallback runs zsh, whatever `$SHELL` is. Bash and fish users
  get the specs and file names, but no shell completion.
- **Menu keys only.** The menu does not take mouse clicks, and it does not filter
  as you type: a typed key closes it and Tab opens it again.
- **Descriptions come from the specs.** Zsh candidates show no description.
- **Some zsh candidates are internal names.** A few option lists show zsh's own
  helper words (such as `-default-`) in the menu.
- **Quoted words are not completed by specs.** With an open quote, the spec is
  skipped and zsh answers.
- **Specs are a short list.** Only the subcommands and flags in the five files
  are known. Other commands use zsh, if zsh knows them.
