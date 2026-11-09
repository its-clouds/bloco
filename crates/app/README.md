# crates/app

The GPUI binary: one window with sessions, split panes and a command palette. Everything
that touches GPUI lives here. `crates/term-session` and `crates/config` have no GPUI
dependency.

## Files

| File | What it does |
| --- | --- |
| `main.rs` | Starts GPUI, the menus, and the window that holds the workspace |
| `workspace.rs` | Tabs, the split tree rendering, dividers, the palette overlay, the config error bar and the settings reload |
| `split_tree.rs` | Pane layout as a binary tree of splits: geometry, neighbors, removal, ratios (no GPUI, unit tests) |
| `commands.rs` | The action registry: ids, titles, default keys, and the GPUI keybindings built from them |
| `palette.rs` | Command palette state, fuzzy filter, shortcut text (unit tests) |
| `terminal_view.rs` | One pane: keyboard, mouse, clipboard, scrollback, IME, Option as Meta, and its find bar. Emits `TerminalEvent`s to the workspace |
| `find.rs` | Find bar state: query, toggles, matches, current match, counter, highlighted cells (unit tests) |
| `file_links.rs` | Resolves a path printed in the terminal (`~`, relative to the block's directory) and reveals it in Finder (unit tests) |
| `preview.rs` | What the file preview shows: the file read from disk, Markdown parsed into blocks, size and binary checks (unit tests) |
| `file_viewer.rs` | The file preview panel: header with ×, rendered Markdown or plain monospace text. Emits `FileViewerEvent::Close` |
| `terminal_element.rs` | Draws the grid: cell metrics, resize to the window, background runs, text runs, cursor |
| `keys.rs` | Keystroke to escape sequence mapping (adapted from Zed), and the command-line text edits (`TextEdit`) |
| `mouse.rs` | Mouse and wheel report encoding (adapted from Zed) |
| `option_key.rs` | Which Option key is held (left or right), for `option_as_meta = "left"` or `"right"` |
| `colors.rs` | The palette for the terminal grid and for the window chrome, built from a theme |

## Keybindings

These are the defaults. `keymap.toml` can change any of them (see below).

| Action | Keys | Command id |
| --- | --- | --- |
| Command palette | Cmd+Shift+P, Cmd+P | `palette.toggle` |
| New session (also the + in the sidebar) | Cmd+T | `tab.new` |
| Close pane (closes the session with its last pane, the window with its last session) | Cmd+W | `pane.close` |
| Close session (all its panes). The × on its sidebar row does the same | none, palette only | `tab.close` |
| Previous / next session | Cmd+{ / Cmd+} (Cmd+Shift+[ and ] on a US layout) | `tab.previous`, `tab.next` |
| Go to session 1 to 9 | Cmd+1 to Cmd+9 | `tab.select.1` to `tab.select.9` |
| Move the active session up / down in the Sessions list | Cmd+Ctrl+← / Cmd+Ctrl+→ | `tab.move_left`, `tab.move_right` |
| Rename the session (also a double-click on its sidebar row). Enter commits, Esc cancels, a blank name clears it | none, palette only | `tab.rename` |
| Drag a row in the Sessions list onto another row (it takes that row's place and group: after it when dragged down, before it when dragged up), onto a group header (it joins the group), or onto the space below the list (it leaves its group). An accent line shows where it lands; the drop is where the mouse last was, so a release between rows still counts. Released over the panes, the drag is cancelled. A click without a move only selects | mouse | none |
| Rename the active pane. Shown in the row while the session has no name of its own | none, palette only | `pane.rename` |
| Move the session to a group, or to no group | none, palette only | `tab.move_to_group` |
| New group (also the ⊞ in the sidebar: takes the active session into it and starts naming it). Click a group's name to rename it, its × to delete it (its sessions stay) | none, palette only | `group.new`, `group.rename`, `group.delete` |
| Move the active pane to a new session, or into another session beside its active pane | none, palette only | `pane.move_to_new_tab`, `pane.move_to_tab` |
| Split right / split down | Cmd+D / Cmd+Shift+D | `pane.split_right`, `pane.split_down` |
| Focus the pane left, right, above, below | Cmd+Opt+arrows | `pane.focus_left`, ... |
| Copy / paste | Cmd+C / Cmd+V | `edit.copy`, `edit.paste` |
| Move back / forward one word (command line) | Option+← / Option+→ | `terminal.word_back`, `terminal.word_forward` |
| Move to start / end of line (command line) | Cmd+← / Cmd+→ | `terminal.line_start`, `terminal.line_end` |
| Delete word backward / delete to start of line | Option+Backspace / Cmd+Backspace | `terminal.delete_word_back`, `terminal.delete_to_line_start` |
| Find (opens the bar, or selects its query when open) | Cmd+F | `find.open` |
| Next match (older) / previous match (newer) | Cmd+G / Cmd+Shift+G, or Enter / Shift+Enter in the bar | `find.next`, `find.previous` |
| Close the find bar | Esc in the bar, or its ×. No key in the terminal (Esc belongs to the shell) | `find.close` |
| Cmd+click a path in the output: reveals a file in Finder, opens a directory in Finder. Relative paths resolve in the directory the command ran in | mouse | none |
| Cmd+Shift+click a file path in the output: opens it in the preview panel beside the pane | mouse | none |
| Cmd+click a file in the sidebar's Files tree: opens it in the preview panel (a plain click still inserts its path) | mouse | none |
| Close the file preview panel: its ×, or Esc while the panel has focus (click it first) | Esc, or ×. Palette: Close File Preview | `preview.close` |
| Zoom in (font +1 pt) | Cmd+= or Cmd++ (Cmd+Shift+=) | `view.zoom_in` |
| Zoom out (font −1 pt) | Cmd+- | `view.zoom_out` |
| Reset zoom to the size in `config.toml` | Cmd+0 | `view.zoom_reset` |
| Quit | Cmd+Q | `app.quit` |

Cmd combinations never reach the shell. A Cmd key with no binding does nothing.

Zoom changes the terminal text in every tab and pane, including hidden tabs. Each grid
is resized to fit the same window, so shells and full-screen apps reflow. The size is
clamped to 6 to 72 pt. The zoom is kept in memory only: it is not written to
`config.toml`, and a restart starts at the configured size. A change to `[font] size`
through hot reload keeps the zoom offset.

The command-line keys send the bytes that the shell's line editor reads: `ESC b` and
`ESC f` for words, `Ctrl-A` and `Ctrl-E` for the ends of the line, `ESC DEL` and `Ctrl-U`
for deleting. These are the emacs-keymap bytes of zsh, bash and fish, and they work
whatever `option_as_meta` is set to. In a full-screen app (alternate screen) they send the
xterm keys instead: `ESC [ 1 ; 3 D` for Option+←, Home for Cmd+←, and nothing for
Cmd+Backspace. They are bindings, so `keymap.toml` can move them or remove them. A removed
key sends nothing: it does not reach the shell.

New tabs and splits start in the active pane's directory. The directory comes from
OSC 7, which the zsh integration sends before each prompt. Without OSC 7 the app asks
the OS for the shell's directory by its process id (macOS only).

## Find

Cmd+F opens a find bar at the top right of the focused pane. It searches that pane's
scrollback and screen as you type. Every match is highlighted, and the current one has
its own color (`find_match`, `find_current` and `find_text` in the theme). A selection
in the terminal (its first line) becomes the query. Without one, the last query comes
back, selected.

- `.*` makes the query a regular expression (Rust `regex` syntax, with `^` and `$` at
  the ends of each line). An invalid one turns
  the bar red and shows "Invalid regex". `Aa` makes it case-sensitive. Both start off.
- Matches are counted from the newest output up, as in Warp: `1/163` is the match
  nearest the bottom. Next (Enter, Cmd+G, ↓) goes to older output, previous
  (Shift+Enter, Cmd+Shift+G, ↑) to newer. Moving to a match off screen centers it.
- Esc or × closes the bar and clears the highlights. The view stays where it is.
- New output reruns the search at most every 150 ms, on a background thread, and the
  current match stays current. A search over a full scrollback (10,000 lines) takes
  about 10 ms in a release build.
- At most 10,000 matches are kept, the newest ones. The counter then ends in `+`.
- In the bar, Option+Backspace deletes a word, Cmd+Backspace clears the query and Cmd+V
  pastes its first line. The other command-line keys do nothing there, and no key in the
  bar reaches the shell.
- Each pane has its own bar and query.

The search itself is `term_session::Finder` (`crates/term-session/src/search.rs`). It
reads the buffer one logical line at a time (rows joined where the output wrapped,
without the blank cells after the text) and runs the `regex` crate on each line. So `^`
and `$` match at the start and end of a printed line, even one that wrapped.
`alacritty_terminal`'s own `RegexIter` is not used: it feeds the blank cells to the
regex, so `$` never matched, and it restarts `^` after each match.

## Settings

Files live in `~/.config/bloco/`. A missing file uses the defaults. The app
checks both files every half second and applies a change when the file is saved. A
file that does not parse keeps the last good settings. Its error is logged to stderr
and shown in a bar at the bottom of the window until the file is fixed.

`config.toml`:

```toml
theme = "dark"              # "dark" or "light"
option_as_meta = false      # false, true, "left" or "right"

[font]
family = "JetBrainsMonoNL Nerd Font"
size = 14
```

`option_as_meta`: `false` types characters with Option (dead keys and IME work, as in
Terminal.app). `true` sends ESC before the key, so Option+b reaches the shell as `ESC b`
(Meta, as in Alacritty). `"left"` or `"right"` does the same for one Option key only.

`keymap.toml`: overrides for the defaults above. Each `[[bind]]` table sets keys and an action.

```toml
# Add a binding (a chord: two keystrokes separated by a space).
[[bind]]
keys = "cmd-k cmd-t"
action = "tab.new"

# Move a binding: remove the default, then bind the new keys.
[[bind]]
keys = "cmd-t"
action = "none"

[[bind]]
keys = "cmd-shift-t"
action = "tab.new"

# Remove a default binding.
[[bind]]
keys = "cmd-w"
action = "none"
```

A user binding with the same keys as a default replaces that default. Modifiers are
`ctrl`, `alt`, `shift`, `cmd` and `fn`. Key names are GPUI's (`t`, `up`, `f1`, `enter`).
Write a shifted symbol as the symbol: `cmd-{` and `cmd-shift-[` are the same keystroke.

## Bundle

`scripts/bundle-macos.sh` builds the release binary and writes
`target/release/bundle/Bloco.app`. The bundle has an Info.plist and a placeholder
icon, and it is signed ad hoc (no developer identity). To keep it, copy it to
`/Applications`:

```bash
cp -R target/release/bundle/Bloco.app /Applications/
```

## Pinned GPUI

GPUI comes from the Zed monorepo, pinned to one commit:

```
gpui / gpui_platform  git = https://github.com/zed-industries/zed
                      rev = c4273271092b0287f7d181f616bd5171e0a2f70c   (tag v1.26.2, 2026-10-28)
```

Why this commit:

- It is the latest Zed release tag. Zed's `main` (2026-11-01) is newer but untagged.
  A release tag gets fewer API changes than `main`.
- `gpui_platform` is the entry point Zed's own examples use (`application()`).
- Zed's `Cargo.toml` `[patch.crates-io]` table does not apply to git
  dependencies, so `async-task` is patched in the root `Cargo.toml` too.

Bump this rev on purpose (see ADR 0002, "Revisit if"). M2 did not need a bump. Update the
rev in `crates/app/Cargo.toml` and in the root `Cargo.toml` patch, then rebuild.

## Toolchain

- Zed pins Rust `1.98.1` in its `rust-toolchain.toml`. This repo pins `1.97.1`
  (already installed here, so nothing was downloaded). It builds.
- Rust `1.91.1` (the default here) does not build GPUI at this commit:
  `gpui_util` uses the unstable `slice_as_array`. Use `cargo +1.97.1` or the
  pinned `rust-toolchain.toml`.
- The crates.io fallback (`gpui-pre`) was not tried, because the git dependency builds.

## macOS and Metal

`xcrun -f metal` fails on this machine (only the Command Line Tools are
installed, not full Xcode). GPUI normally compiles its Metal shaders at build
time with that tool. The `runtime_shaders` feature compiles them at runtime
instead, so no Xcode install is needed. Cost: the first launch compiles the
shaders, so startup is a little slower.

## Known gaps (M2)

- The find input takes typed text only (no IME), and has no movable cursor.
- Warp's "search within the selected block" toggle waits for blocks (M3).

- Cmd+W closes the focused pane. Closing a whole session is the palette's "Close session"
  or the × on its sidebar row.
- There is no tab bar. The title bar shows the active session's title, and sessions are
  listed in the sidebar, which opens on the Sessions list at startup. Renaming opens it.
- The palette takes typed text only. IME composition does not reach it.
- Left and right Option are told apart with the keyboard device flags (CoreGraphics).
  This has not been checked on hardware yet (see `docs/m2-checklist.md`).
- A split's ratio stays between 10% and 90%, so a pane cannot be dragged to zero.
- Directory tracking needs OSC 7. The zsh integration sends it. bash and fish fall back
  to the process directory, which is only read on macOS.
- Layouts, tabs and directories are not restored when the app restarts.
- Only the built-in `dark` and `light` themes exist. Theme files come in M6.
- Option is not Meta by default (see `option_as_meta` above).
- The M1 gaps still apply: no cursor blink, no bell, no OSC 52 clipboard, and combining
  marks can shift a glyph by a fraction of a cell.
