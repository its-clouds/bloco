# M4 exit checklist

M4 is done when you type your commands in Bloco's input editor for a full week of
normal work and nothing about it makes you reach for Warp or iTerm2. This checklist
covers the M4 features and the checks that can be run before that week. Build it with:

```bash
cargo run -p app --release
```

Each row says who checked it. **Checked** means the change's author verified it
from outside the app (build, tests, a shell run through a real PTY). **Passed
(user)** means the user ran it in the window. **Pending** means it still needs a run.

Status as of this change:

- Build and tests: `cargo build --workspace` and `cargo test --workspace` pass.
- Editor logic: the buffer, highlighting, history, Ctrl+R search and soft wrap have
  unit tests (`cargo test -p input-editor`). The mode machine has unit tests
  (`cargo test -p term-session mode`). The submit bytes have tests (`cargo test -p app prompt`).
- In the window: the user ran every row on 2026-11-02 and all pass. Enter not
  submitting the line was found and fixed during that run.

## Build and tests

| # | Check | How to check | Status |
| --- | --- | --- | --- |
| 1 | Debug build | `cargo build --workspace` | Checked |
| 2 | Release build | `cargo build --release -p app` | Pending (not run for this change) |
| 3 | Tests | `cargo test --workspace` passes | Checked |
| 4 | Editor logic | `cargo test -p input-editor` passes | Checked |
| 5 | Mode machine | `cargo test -p term-session mode` passes | Checked |

## In the window

Start a shell and type at the prompt. The editor starts after your own prompt
(zsh or bash). Fish is not covered yet, see the limitations below.

| # | Check | How to check | Status |
| --- | --- | --- | --- |
| 6 | Editor sits after your prompt | Open a tab with your usual prompt (starship, p10k or plain). Type `ls`: the text appears after the prompt, with a beam cursor | Passed (user) |
| 7 | Enter runs the line | Type `echo hi`, press Enter. The output shows and the block header has the command | Passed (user) |
| 8 | Shift+Enter makes a new line | Type `echo one`, Shift+Enter, `echo two`. Both lines show. Enter runs both, and `echo` prints `one` and `two` | Passed (user) |
| 9 | Option+←/→ move by word | Type `git commit -m fix`, Option+← twice: the cursor is before `commit` | Passed (user) |
| 10 | Option+Backspace deletes a word | Type `cargo test --workspace`, Option+Backspace: `workspace` goes. Again: `--` and the space before `test` go | Passed (user) |
| 11 | Cmd+←/→ go to the line ends | On a long line, Cmd+← goes to the start and Cmd+→ to the end | Passed (user) |
| 12 | Cmd+Backspace deletes to the line start | Type a line, Cmd+Backspace: the whole line is gone | Passed (user) |
| 13 | Ctrl+A, Ctrl+E, Ctrl+U, Ctrl+W | Each moves or deletes as in the shell (start, end, to start, word) | Passed (user) |
| 14 | Undo and redo | Type `one two three`, Cmd+Z removes `three`, Cmd+Z again removes `two`. Cmd+Shift+Z brings `two` back | Passed (user) |
| 15 | Click places the cursor | Type a line, click in the middle of it: the cursor moves there. Type: the text goes in at the click | Passed (user) |
| 16 | Long line wraps | Type a line longer than the pane. It wraps under the prompt and the cursor follows | Passed (user) |
| 17 | Up and Down recall history | Run `ls`, then `pwd`. Up shows `pwd`, Up again `ls`. Down goes back, and past the newest it brings back what you had typed | Passed (user) |
| 18 | History from your shell | Open a new tab. Up shows commands from `~/.zsh_history` (or `~/.bash_history`) | Passed (user) |
| 19 | Ctrl+R search | Press Ctrl+R, type `pwd`: the newest match shows as `(reverse-i-search)`. Ctrl+R again goes to an older match. Enter puts it in the line without running it. Escape cancels | Passed (user) |
| 20 | Highlighting | Type `ls -la "x" $HOME | grep y # note`. The command is in the accent colour, `-la` and `-y` muted, `"x"` green, `$HOME` cyan, `# note` muted | Passed (user) |
| 21 | Dead keys and IME | Type `a`, Option+c (`ç`), `ão`: the text reads `ação`. Type a dead key such as Option+e then `a` (`á`) and check it shows and runs as typed | Passed (user) |
| 22 | Paste keeps newlines | Copy two lines of text, Cmd+V: both lines are in the editor and nothing runs. Enter runs them | Passed (user) |
| 23 | Ctrl+C clears the line | Type a line, Ctrl+C: the line goes and the shell does nothing. Ctrl+D on an empty line ends the shell | Passed (user) |
| 24 | Running command gets the keys | Run `read x`, type `hello`, Enter: `read` gets it. The editor is hidden while `read` runs | Passed (user) |
| 25 | Full-screen apps get the keys | Run `vim`, then `htop`: they work as before, and the mouse wheel still scrolls. Quit: the editor is back at the prompt | Passed (user) |
| 26 | Your PS1 is untouched | `echo $PS1` in the shell shows your prompt string, with the zsh or bash marks added after it as before | Passed (user) |
| 27 | Blocks still work | Run `sleep 1; false`. The block shows `✗ 1` and the duration, as in M3 | Passed (user) |

## Known limitations

These are known and are not M4 failures. They are the follow-ups named in
[ADR 0003](decisions/0003-input-editor-at-the-prompt.md).

- **Tab does nothing in the editor.** Completion is M5. zsh's own completion and
  autosuggestions are not available while the editor owns the prompt.
- **Fish has no editor.** Fish's prompt mark would come before its prompt text, so
  the integration sends no `B`. Fish panes are classic terminals with blocks.
- **Text below the last row of the pane is not drawn.** A very long command near
  the bottom of the pane can run off the screen.
- **Wrap counts characters, not cells.** Wide characters (CJK, some emoji) in the
  editor can misalign at the wrap point.
- **A right prompt (RPROMPT) can overlap the editor text** on the same row.
- **History is read when the pane opens.** Commands from other panes or shells
  appear in a new pane, not in an open one.
- **Ctrl+L clears the screen in the shell** and keeps the text in the editor.
- **Typing ahead after Enter** is held in the editor until the shell's next prompt
  mark. Keys typed in that moment may appear in the wrong place for a moment.
