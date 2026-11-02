# M2 exit checklist

M2 is done when you use `Bloco` instead of Warp for a full week of normal
work. This checklist covers the M2 features and the checks that can be run before
that week. Build it with:

```bash
cargo +1.97.1 run -p app --release
```

Or make the app bundle and copy it to `/Applications`:

```bash
./scripts/bundle-macos.sh
cp -R target/release/bundle/Bloco.app /Applications/
```

Each row says who checked it. **Checked** means the change's author verified it
from outside the app (build, logs, process tree, unit tests). **Passed (user)**
means the author ran it by hand and it worked. **Pending** means it still needs a
manual run.

Status as of this change:

- Build and tests: `cargo build --release` and `cargo test --workspace` pass
  (config 20, term-session 36, app 47 tests).
- Launch: the binary and the `.app` bundle start, each with its zsh child, and stay
  up. Killing the shell closes the app.
- Hot reload: a valid file applies, and an invalid file is logged and keeps the last
  good settings.
- Pending: every keyboard, mouse and visual row below. The author's session could not
  send keystrokes to the app (macOS denied System Events keystrokes, error 1002), so
  tabs, splits, the palette and the screen were not driven.

## Build and bundle

| # | Check | How to check | Status |
| --- | --- | --- | --- |
| 1 | Release build | `cargo build --release` | Checked |
| 2 | Tests | `cargo test --workspace` passes | Checked |
| 3 | Bundle | `./scripts/bundle-macos.sh`, then `plutil -lint target/release/bundle/Bloco.app/Contents/Info.plist` | Checked |
| 4 | Bundle launches | `open target/release/bundle/Bloco.app`. The process stays up with a `zsh -l` child | Checked (2 min) |
| 5 | Install | `cp -R target/release/bundle/Bloco.app /Applications/`, then open it from Finder | Pending |

## Launch and shell exit

| # | Check | How to check | Status |
| --- | --- | --- | --- |
| 6 | Window opens with a zsh shell | Launch the binary. `ps` shows the app with a `/bin/zsh -l` child | Checked |
| 7 | Shell exit closes the app | With one pane, `kill -HUP` the shell. The app exits | Checked |
| 8 | No crash on the first frame | Launch and look at the window | Pending |

## Config and hot reload

Run the binary with a scratch `HOME` (for example `HOME=$PWD/scratch/home`), so your
own config is untouched. Watch stderr.

| # | Check | How to check | Status |
| --- | --- | --- | --- |
| 9 | A valid change applies | Save `theme = "light"` to `config.toml`. Stderr shows `settings reloaded from …` | Checked |
| 10 | Broken TOML keeps the last good settings | Save `[font` to `config.toml`. Stderr shows the TOML error. The window keeps its look | Checked (log) |
| 11 | Error bar | With a broken file, a bar at the bottom of the window shows the error, and it goes away after the fix | Pending |
| 12 | Unknown theme | `theme = "solarized"`. Stderr: `unknown theme "solarized"` | Checked |
| 13 | Unknown action | `[[bind]]` with `action = "tab.nope"`. Stderr: `unknown action "tab.nope"` | Checked |
| 14 | Bad modifier | `keys = "super-t"`. Stderr: `unknown modifier "super"` | Checked |
| 15 | Recovery | Fix the file. Stderr shows `settings reloaded` again | Checked |
| 16 | Light theme | `theme = "light"`: the terminal and the tab bar are light and readable | Pending |
| 17 | Font | `[font] size = 13` changes the text size of open panes | Pending |

## Tabs

| # | Check | How to check | Status |
| --- | --- | --- | --- |
| 18 | Cmd+T opens a tab | Press Cmd+T. A second tab appears and gets the keyboard | Pending |
| 19 | New tab in the same directory | `cd /tmp`, then Cmd+T. The new shell is in `/tmp` (`pwd`) | Pending |
| 20 | Tab title is the directory | In a tab, `cd ~/projects`. The label reads `projects`. In `$HOME` it reads `~` | Pending |
| 21 | Tab title is the running command | Run `sleep 30`. The label reads `sleep 30`. After it ends, the label is the directory again | Pending |
| 22 | Cmd+Shift+[ and ] switch tabs, with wrap-around | Three tabs: Cmd+Shift+] goes to the next tab, and at the last one back to the first | Pending |
| 23 | Cmd+1 to Cmd+9 jump to a tab | Four tabs: Cmd+2 selects the second | Pending |
| 24 | Click a tab to select it. The + button opens a tab | Mouse | Pending |
| 25 | Cmd+W closes the pane, then the tab, then the window | With one pane in each of two tabs: Cmd+W twice closes the first tab, and the next Cmd+W closes the window and quits the app | Pending |

## Splits

| # | Check | How to check | Status |
| --- | --- | --- | --- |
| 26 | Split right and split down | Cmd+D, then Cmd+Shift+D in the new pane. Three panes, one shell each (`ps -o ppid`) | Pending |
| 27 | Split opens in the same directory | In a pane, `cd /tmp`, then Cmd+D. The new pane is in `/tmp` | Pending |
| 28 | Cmd+Opt+arrows move focus | In a 2×2 grid, Cmd+Opt+Right, then Cmd+Opt+Down. The cursor moves to the right pane | Pending |
| 29 | Drag a divider to resize | Drag the 4 px line between two panes. Both panes resize. The ratio stays between 10% and 90% | Pending |
| 30 | Click a pane to focus it | Click a pane. Typing goes there | Pending |
| 31 | A pane's shell exit closes only that pane | In a split, `exit`. The other pane stays, and the layout closes the gap | Pending |
| 32 | Cmd+Opt+arrows never reach the shell | `cat -v`, then Cmd+Opt+Left. Nothing is printed | Pending |

## Working directory

| # | Check | How to check | Status |
| --- | --- | --- | --- |
| 33 | zsh reports the directory with OSC 7 | Source the integration and print the report for a path with a space and a `%`. The output has `a%20b%25c` | Checked |
| 34 | The OS gives the directory of a process | `cargo test -p term-session` runs `pid_cwd` on this process and matches the working directory | Checked |
| 35 | Directory from the OS for bash and fish | Open a bash shell in a split and `cd` there. The next split opens in that directory | Pending |

## Command palette

| # | Check | How to check | Status |
| --- | --- | --- | --- |
| 36 | Cmd+Shift+P and Cmd+P open the palette | Press each. The palette appears with a `>` prompt | Pending |
| 37 | Typing filters the list. Up and Down move. Enter runs. Esc closes | Type `split`, Down, Enter (runs "Split down") | Pending |
| 38 | Each row shows its shortcut | `⇧⌘P` next to "Show command palette", `⌘T` next to "New tab" | Pending |
| 39 | A click outside closes the palette | Click the terminal while the palette is open | Pending |
| 40 | Fuzzy matching | `cargo test -p app` covers scoring, order and symbols | Checked |

## Keys and text (M1 regression)

| # | Check | How to check | Status |
| --- | --- | --- | --- |
| 41 | Cmd+C and Cmd+V | Select text, Cmd+C, Cmd+V into `cat` | Pending |
| 42 | Dead keys and IME still work | M1 rows 10 and 11 | Pending |
| 43 | Option is not Meta by default | `cat -v`, then Option+b types `∫` | Pending |
| 44 | `option_as_meta = true` | `cat -v`, then Option+b shows `^[b` | Pending |
| 45 | `option_as_meta = "left"` or `"right"` | Only that Option key shows `^[` | Pending |

## Tab close button

| # | Check | How to check | Status |
| --- | --- | --- | --- |
| 46 | The × shows on every tab | Three tabs. Each one, active or not, has a × at its right edge with no hover needed | Pending |
| 47 | The × stays visible when the pointer leaves the tab | Hover a background tab, then move the pointer away: its × is still there | Pending |
| 48 | The × highlights and shows a pointer | Hover the × itself: its background changes (`close_hover` in the theme) and the cursor is a pointer. Check in both `dark` and `light` | Pending |
| 49 | Clicking × closes that tab and its shells | Two tabs. In the second, run `sleep 300`. Click its ×: the tab closes, and `ps` shows no `sleep 300` left | Pending |
| 50 | × on a background tab does not switch to it | Tab 2 is active. Click the × of tab 1: tab 1 closes and tab 2 stays active | Pending |
| 51 | Middle-click closes a tab | Middle-click the title of a background tab: it closes. Middle-click the active tab: it closes too | Pending |
| 52 | Long titles end in an ellipsis before the × | `cd` into a directory with a long name in a narrow tab (or run a long command). The title ends in `…`, and the × stays whole and visible at the right edge | Pending |
| 53 | Closing the last tab closes the window | One tab only: click its × (or middle-click it). The window closes and the app quits | Pending |

## Command line text editing

The keys send readline bytes at a shell prompt and xterm keys in full-screen apps. See
`crates/app/README.md`.

| # | Check | How to check | Status |
| --- | --- | --- | --- |
| 54 | Option+← and Option+→ move by word at a zsh prompt | Run `echo one two three`. Press Option+← twice and type `X`: the X goes before `two`. Option+→ moves forward a word | Pending |
| 55 | Option+Backspace deletes a word | At the same prompt, Option+Backspace removes `three` | Pending |
| 56 | Cmd+← and Cmd+→ go to the start and end of the line | Type a command, press Cmd+←, then type `X`: it lands at the start. Cmd+→ goes to the end | Pending |
| 57 | Cmd+Backspace deletes to the start of the line | Put the cursor in the middle of a line and press Cmd+Backspace: the text before the cursor goes | Pending |
| 58 | Same keys in bash and fish | Run `bash`, then `fish`, and repeat rows 54 to 57 in each | Pending |
| 59 | Word keys with `option_as_meta = true` | Set `option_as_meta = true` in `config.toml` and wait for the reload. Rows 54 and 55 still work | Pending |
| 60 | Word keys with `option_as_meta = false`, and `ç` still types | Default setting: rows 54 and 55 work. Option+C types `ç` at the prompt | Pending |
| 61 | Full-screen apps get xterm keys | In `vim`, Option+← moves back a word (vim receives `ESC [ 1 ; 3 D`). Cmd+← goes to the start of the line. Cmd+Backspace does nothing | Pending |
| 62 | A key can be removed in `keymap.toml` | Add `[[bind]]` with `keys = "cmd-left"` and `action = "none"`. Cmd+← sends nothing, and the palette shows no shortcut for "Move to start of line" | Pending |
| 63 | Cmd+Opt+arrows still move focus, and the shell gets no keys | In a split, Cmd+Opt+← moves focus to the left pane. `cat -v` in that pane prints nothing | Pending |

## Find bar

Unit tests cover the search (plain text, case, regex, `^` and `$` anchors, invalid
regex, wrapped lines, wide characters and emoji, scrollback, the match limit) and the bar's state (typing, counter, next and previous,
keeping the current match). Everything you see or type is pending.

| # | Check | How to check | Status |
| --- | --- | --- | --- |
| 64 | Search logic | `cargo test -p term-session search` and `cargo test -p app find` pass | Checked |
| 65 | Cmd+F opens the bar | Press Cmd+F. A bar appears at the top right of the focused pane with the cursor in its input. Typing goes to the bar, not the shell | Pending |
| 66 | Search as you type, highlights | Run `ls -la /`, then Cmd+F and type `x`. Every `x` is yellow, and the newest one (lowest on screen) is orange. The counter reads `1/N` | Pending |
| 67 | Next and previous | Enter and Cmd+G move the orange match up to older output (`2/N`, `3/N`, …). Shift+Enter and Cmd+Shift+G move it back down. The ↓ and ↑ buttons do the same. At the end it wraps around | Pending |
| 68 | A match in the scrollback scrolls into view | Run `seq 1 5000`, Cmd+F, type `4242`. The view jumps up to `4242` and centers it | Pending |
| 69 | Regex and case toggles | Click `.*` and `Aa` (each one highlights when on). `Aa` on: `X` no longer matches `x`. `.*` off: `a.c` matches only the literal `a.c` | Pending |
| 70 | Invalid regex | `.*` on, type `(`. The border and the counter turn red with "Invalid regex", and nothing crashes. Type `)`: results come back | Pending |
| 71 | No results | Type `zzzqqq`. The counter reads "No results" and nothing is highlighted | Pending |
| 72 | Selection pre-fills the query | Select a word with a double-click, then Cmd+F. The word is in the input and highlighted in the output | Pending |
| 73 | Cmd+F on an open bar selects the query | With text in the bar, press Cmd+F (or Cmd+A). The query is highlighted. Typing replaces it, Backspace clears it | Pending |
| 74 | Esc and × close the bar | Press Esc: the bar and the highlights go, the keyboard is back in the shell, and the view stays on the last match. Open it again and close it with × | Pending |
| 75 | Reopen offers the last query | Close the bar, then Cmd+F with no selection. The last query is there, selected | Pending |
| 76 | Results follow new output | Cmd+F, type `tick`, click the terminal and run `for i in {1..20}; do echo tick; sleep 0.2; done`. The counter grows while it runs and the current match stays put | Pending |
| 77 | Per-pane state | Split with Cmd+D. Search `foo` in one pane and `bar` in the other. Each pane keeps its own bar, query and highlights | Pending |
| 78 | Keys in the bar never reach the shell | In the bar, press Option+Backspace (deletes a word of the query), Cmd+Backspace (clears it), Ctrl+C, the arrows and Cmd+V (pastes into the query). The shell gets nothing | Pending |
| 79 | Palette and keymap | The palette lists "Find", "Find next", "Find previous" and "Close find bar". In `keymap.toml`, bind `cmd-shift-f` to `find.open`: it opens the bar | Pending |
| 80 | Light theme | `theme = "light"`: the matches are light yellow and orange, and the text on them is readable | Pending |
| 81 | Big scrollback | Run `seq 1 200000`, then search `1`. Typing stays responsive. The counter shows `1/10000+` | Pending |
| 82 | Regex anchors | Run `seq 1 5000`, Cmd+F, `.*` on, type `^4.*2$`. It matches `42`, `402`, `412` … `4992` (111 matches), each whole line, and nothing else | Pending |

## Font zoom

Cmd+= and Cmd+- change the font size of the terminal text by one point, and Cmd+0
resets it. The zoom applies to every tab and pane, and each grid resizes to fit the same
window. The offset lives in memory only. See `crates/app/README.md`.

| # | Check | How to check | Status |
| --- | --- | --- | --- |
| 83 | Zoom math | `cargo test -p app zoom` (offset, clamp at 6 and 72, steps at the limits, hot-reload base) and `cargo test -p config minus` pass | Checked |
| 84 | Zoom in | Press Cmd+=. The text in the focused pane gets bigger, and the grid gets fewer columns and rows. The window keeps its size | Pending |
| 85 | Cmd+Shift+= zooms in too | Cmd+Shift+= (Cmd++ on a US layout) does the same as Cmd+= | Pending |
| 86 | Zoom out and reset | Cmd+- makes the text one point smaller. Cmd+0 returns to the size in `config.toml` | Pending |
| 87 | Every tab and pane | Two tabs, with a split in one. After Cmd+=, the text in every pane of both tabs is bigger | Pending |
| 88 | Hidden tabs reflow too | Zoom in while tab 2 is active, then switch to tab 1. `stty size` in tab 1 shows the new rows and columns, not the old ones | Pending |
| 89 | vim reflows after a zoom | Open `vim` on a long file, then Cmd+= twice. vim redraws to the new size, and `:echo &columns &lines` shows the new values | Pending |
| 90 | htop reflows after a zoom | If `htop` is installed, run it and press Cmd+=. The columns and rows change, and the meters redraw without leftover text | Pending |
| 91 | New tab and split use the zoomed size | Zoom in, then Cmd+T and Cmd+D. In each new pane, `stty size` gives the rows and columns that fit the pane, not `24 80` | Pending |
| 92 | Clamp at 72 pt | Press Cmd+= until the text stops growing. One Cmd+- then shrinks it at once | Pending |
| 93 | Clamp at 6 pt | Press Cmd+- until the text stops shrinking. One Cmd+= then grows it at once | Pending |
| 94 | Zoom is not saved | Zoom in, quit, and start Bloco again. The text is the size in `config.toml`, and the file is unchanged | Pending |
| 95 | Hot reload keeps the zoom | With `[font] size = 14`, zoom in twice, then set `size = 16` in `config.toml`. The text is 18 pt. Cmd+0 gives 16 pt | Pending |
| 96 | Palette and keymap | The palette lists "Zoom in", "Zoom out" and "Reset zoom" with ⌘=, ⌘- and ⌘0. In `keymap.toml`, `keys = "cmd-shift-up"` with `action = "view.zoom_in"` zooms in, and `keys = "cmd--"` is accepted | Pending |

## Known issues

- **Keyboard and screen checks are not run.** macOS denied keystroke injection to this
  session (error 1002). Rows 8, 11, 16 to 17, 18 to 39, 41 to 45, 46 to 53, 54 to 63,
  65 to 82 and 84 to 96 need a manual run.
- **Command-line keys assume an emacs keymap.** In a shell's vi mode, the same bytes
  do something else.
- **Left and right Option** are told apart with the keyboard device flags
  (`CGEventSourceFlagsState`). This has not been checked on a real keyboard.
- **Cmd+W closes the focused pane**, and the tab when it is the last pane. The palette's
  "Close tab" closes all panes of the tab.
- **The palette and the find bar take typed text only.** IME composition does not reach
  them. The find input has no movable cursor: you type and delete at the end.
- **Find keeps at most 10,000 matches**, the newest ones. The counter then ends in `+`.
- **Find highlights can lag new output** by up to 150 ms (the search reruns at most
  that often). When the scrollback is full, old highlights shift by a line per new line
  until the rerun.
- **The tab bar is always shown**, even with one tab.
- **Config reload polls** the two files every half second.
- **Directory tracking** needs OSC 7 (the zsh integration sends it). Other shells use the
  process's directory, which is read on macOS only.
- **Nothing is restored** on restart (tabs, splits, directories).
- **Only the built-in themes** exist (`dark` and `light`).
- **The app bundle** is ad-hoc signed, not notarized.
- The M1 gaps still apply: no cursor blink, no bell, no OSC 52 clipboard, and combining
  marks can shift a glyph by a fraction of a cell.
