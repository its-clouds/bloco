# M3 exit checklist

M3 is done when blocks are on in your daily use and nothing about them annoys you
enough to turn them off. This checklist covers the M3 features and the checks that can
be run before that week. Build it with:

```bash
cargo run -p app --release
```

Each row says who checked it. **Checked** means the change's author verified it
from outside the app (build, tests, a shell run through the integration). **Pending**
means it still needs a manual run in the window.

Status as of this change:

- Build and tests: `cargo build --release` and `cargo test --workspace` pass.
- Shell integration: zsh, bash and fish each report a command, its exit status and the
  directory through a real PTY (`cargo test -p term-session shell`). The frame tests
  check the header row, the sticky header and collapse against a real bash.
- Manual run by the author, in the window: rows 6 to 17, 21 and 23 to 31 work and are
  Checked. Row 18 (light theme) and row 22 (copy output) were not reported and stay
  Pending. Rows 19, 20 and 32 failed in that run; they have since changed and are
  Pending again.

## Build and integration

| # | Check | How to check | Status |
| --- | --- | --- | --- |
| 1 | Release build | `cargo build --release` | Checked |
| 2 | Tests | `cargo test --workspace` passes | Checked |
| 3 | zsh reports commands | `cargo test -p term-session zsh_` passes | Checked |
| 4 | bash reports commands | `cargo test -p term-session bash_` passes (bash 3.2 from macOS) | Checked |
| 5 | fish reports commands | `cargo test -p term-session fish_` passes (fish 4.9) | Checked |
| 6 | No dotfile edits | Start Bloco, then `git status` in your dotfiles folder. Nothing changed | Checked (author, in the window) |
| 7 | Your profile still loads in bash | Start Bloco, run `echo $PATH` and `alias`. Your Homebrew path and aliases are there. Bash starts as a login shell through the injected rcfile | Checked (author, in the window) |
| 8 | Scripts are removed when the tab closes | Open a tab, close it. `ls $TMPDIR | grep term-session` shows nothing left for it | Checked (author, in the window) |

## Blocks on screen

| # | Check | How to check | Status |
| --- | --- | --- | --- |
| 9 | Header shows command, status, duration and directory | Run `ls -la` in `~/projects`, then `sleep 2; false`. Each block has a header row: the command on the left, and `✓` or `✗ 1`, the duration and `~/projects` on the right | Checked (author, in the window) |
| 10 | The header replaces the prompt and command lines | The prompt line of a finished command is no longer drawn as text, only the header row. The output is below it | Checked (author, in the window) |
| 11 | A running command has no header | Run `sleep 5`. While it runs, the line shows as plain text. The header appears when it ends | Checked (author, in the window) |
| 12 | Failed command is red | `false` shows `✗ 1` in the error color (`error` in the theme) | Checked (author, in the window) |
| 13 | Long command text is cut | Run a command longer than the pane. The header ends in `…`, and the status still shows at the right | Checked (author, in the window) |
| 14 | Multi-line prompt | With starship or powerlevel10k, the whole prompt is inside the header. No prompt line is left behind | Checked (author, in the window) |
| 15 | Sticky header | Run `seq 1 500`. Scroll up into its output with the wheel: the header of that block stays at the top of the pane | Checked (author, in the window) |
| 16 | Click a header to collapse | Click the header of `seq 1 500`: its output hides and the header stays. Click again: it shows | Checked (author, in the window) |
| 17 | Collapse from the palette | Put the header at the top, Cmd+Shift+P, "Collapse or expand block": the same result as the click | Checked (author, in the window) |
| 18 | Header colors in light theme | `theme = "light"`: the header bar and text are readable | Pending |

## Navigation and copy

| # | Check | How to check | Status |
| --- | --- | --- | --- |
| 19 | Cmd+↑ goes to the previous block | Run three commands with output. Cmd+↑ selects the newest block: its header has an accent bar and its output a tint. Cmd+↑ again selects the one before it. Typing, a click elsewhere, or Escape clears the selection | Pending. Changed after the author's run: Cmd+↑/↓ now move from the selected block and show it highlighted (see Known limitations) |
| 20 | Cmd+↓ goes to the next block | From the first block, Cmd+↓ twice. The last Cmd+↓ shows the newest output at the bottom | Pending. Changed after the author's run: Cmd+↓ moves from the selected block and clears the selection past the last one |
| 21 | Copy the command | Cmd+Shift+C on a block's header copies its command. Paste it elsewhere | Checked (author, in the window) |
| 22 | Copy the output | Cmd+Opt+C copies the output of the block at the top, without the prompt. Paste it into an editor | Checked (author, in the window) |
| 23 | Output with wrapped lines | Copy the output of `printf 'x%.0s' {1..300}; echo` in a narrow pane: one line, no added newlines in the middle | Checked (author, in the window) |
| 24 | Palette and keymap | The palette lists "Previous block", "Next block", "Copy block command", "Copy block output" and "Collapse or expand block", with ⌘↑, ⌘↓, ⇧⌘C and ⌥⌘C. In `keymap.toml`, `action = "none"` on `cmd-up` removes Cmd+↑ | Checked (author, in the window) |
| 25 | Find still works over blocks | Cmd+F and type a word from an old block. The view jumps to it. If the block is collapsed, the match is hidden until you expand it | Checked (author, in the window) |

## Fallback to classic mode

| # | Check | How to check | Status |
| --- | --- | --- | --- |
| 26 | vim is a plain grid | Run `vim`, type a few lines, `:q`. While in vim there are no headers and the screen is exactly the grid. After `:q` the earlier blocks show their headers again | Checked (author, in the window) |
| 27 | less and man | `less /etc/hosts`, then `q`. Same as row 26 | Checked (author, in the window) |
| 28 | htop | `htop`, then `q`. The meters redraw without leftover text | Checked (author, in the window) |
| 29 | ssh to a host without integration | `ssh <host>` into a host with no integration. The session is a plain grid until you exit. Then the `ssh` block shows its header | Checked (author, in the window) |
| 30 | Claude Code | Start it in a pane and run a prompt. The UI redraws correctly, and the block for `claude` gets a header when it exits | Checked (author, in the window) |
| 31 | Shell with no integration | Run `sh` in a pane. It shows plain output with no headers, and nothing breaks | Checked (author, in the window) |
| 32 | Headers after a full scrollback | `seq 1 12000`, then `seq 1 20`, then `echo hi`. Each command still shows its header and its output is right. Cmd+↑ reaches the `seq 1 20` block | Checked (author, in the window). The `seq 1 12000` block itself is not reachable: its header left the scrollback (see Known limitations) |

## Known limitations

- **Resizing the width forgets the block positions.** A width change rewraps the
  buffer, so the blocks show as plain rows from then on. New commands get headers.
- **Full scrollback is counted, not exact.** Past 10,000 lines, the lines that leave the
  top are counted through the view offset (see `Scrollback` in
  `crates/term-session/src/session.rs`). A scroll region on the primary screen can add to
  that count, so positions can drift after heavy use of one. A block whose line is gone
  shows as plain rows. Row 32 checks the basic case.
- **Bash command text comes from the history entry** when history is on. With history
  off, or for the first simple command of a pipeline, the header shows the first command
  only. zsh and fish report the whole line.
- **Bash's DEBUG trap is ours.** A DEBUG trap set in your bashrc before the integration
  runs is replaced.
- **fish 3.x** does not have the OSC 133 switch that the integration uses. It has not been
  tried. fish 4 works (the integration turns fish's own marks off for the session).
- **No setting turns blocks off.** Classic mode is the fallback for full-screen apps and
  shells without integration only.
- **A selected block is highlighted** with an accent bar on its header and a tint over its
  output. The selection is cleared by typing, a click elsewhere, or Escape. Copy and
  collapse act on the selected block when there is one. Cmd+↑ with nothing selected picks
  the newest block above the bottom of the view.
- **A block on screen cannot be scrolled to the top.** The view cannot scroll above the
  scrollback, so a block that is still on screen stays where it is, with its header
  highlighted.
- **Collapsed output is not searched.** A find match inside a collapsed block scrolls to
  the block's header.
- **Running commands show plain rows.** Their header is drawn when they finish.
- **Manual checks are pending** for every row marked Pending above.
