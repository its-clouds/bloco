# M1 exit checklist

M1 is done when you can do an hour of real work in `Bloco` with the tools
below, and nothing renders wrong. Build it with:

```bash
cargo +1.97.1 run -p app --release
```

Each row says who checked it. **Passed (user)** means the author ran it and it
worked. **Checked** means the change's author verified it. **Pending** means it
still needs a manual run.

Status as of this change:

- Build and tests: `cargo build --release` and `cargo test --workspace` pass.
- Launch: the binary starts and stays up with no panic (checked, 40 s idle).
- Passed by the user: basics, full-screen and TUI apps (vim, less, git, Claude Code,
  htop), keyboard and text, window resize, shell exit, Cmd+Q.
- Passed by the user: the mouse and scrollback section (rows 15 to 20).
- Pending: nothing from the checklist. Every row has now been run by the user.

## Basics

| # | Check | How to check | Status |
| --- | --- | --- | --- |
| 1 | Window opens with a zsh prompt | Launch the app | Passed (user) |
| 2 | Typing works | Type `echo hi`, press Return | Passed (user) |
| 3 | Login shell rc files load | `echo $PATH`, check Homebrew paths, check your `~/.zshrc` aliases | Passed (user) |
| 4 | Font | Config font shows; a missing font falls back to Menlo with a warning | Passed (user) |

## Full-screen and TUI apps

| # | Check | How to check | Status |
| --- | --- | --- | --- |
| 5 | vim / nvim | Open a file, move, edit, `:q`. Colors and cursor shape look right | Passed (user) |
| 6 | htop | Run it, scroll the list, click a column header with the mouse, `q` | Passed (user) |
| 7 | less | `less /etc/hosts`, scroll with the wheel and arrow keys, `q` | Passed (user) |
| 8 | git | `git log --graph`, `git diff` with color, a pager on long output | Passed (user) |
| 9 | Claude Code | Start `claude`, type a prompt, scroll the output, exit | Passed (user) |

## Keyboard and text

| # | Check | How to check | Status |
| --- | --- | --- | --- |
| 10 | Accents (dead keys) | Brazilian layout: `´` then `a` gives `á`; `ação`, `ç` type correctly | Passed (user) |
| 11 | IME | Switch to a composing input method, compose text, commit it | Passed (user) |
| 12 | Modifiers | Ctrl+C interrupts `sleep 100`; Option+← moves by word in zsh | Passed (user) |
| 13 | Copy and paste | Select with the mouse, Cmd+C; Cmd+V into `cat` | Passed (user) |
| 14 | Bracketed paste | Paste a multi-line text into `zsh`; it is not run line by line | Passed (user) |

## Mouse and scrollback

| # | Check | How to check | Status |
| --- | --- | --- | --- |
| 15 | Scroll | `seq 1 5000`, then wheel and trackpad scroll back; Shift+PageUp works | Passed (user) |
| 16 | Selection | Drag, double-click (word), triple-click (line) select the right text | Passed (user) |
| 17 | URLs | `echo https://example.com`; Cmd+click opens the browser; hover shows a hand | Passed (user) |
| 18 | Wide and emoji | `echo "日本語 🙂 ✓"` aligns; the cursor sits after the emoji | Passed (user) |
| 19 | Box drawing | `htop` or `tree` borders are continuous | Passed (user) |
| 20 | Colors | `for i in {0..255}; do printf "\e[48;5;${i}m  \e[0m"; done; echo` shows 256 colors; a truecolor test shows gradients | Passed (user) |

## Window

| # | Check | How to check | Status |
| --- | --- | --- | --- |
| 21 | Resize | Drag the window corner while `htop` runs; the grid follows, no crash, no garbage | Passed (user) |
| 22 | Shell exit | Type `exit`; the window closes | Passed (user) |
| 23 | Cmd+Q | Quits the app | Passed (user) |

## Known issues

- Option is not Meta. Option types characters, like Terminal.app's default.
- No cursor blink, no bell, no OSC 52 clipboard.
- Combining marks can shift the glyph of their base cell by a fraction of a cell.
- The first launch is slower because Metal shaders compile at runtime (see `crates/app/README.md`).
