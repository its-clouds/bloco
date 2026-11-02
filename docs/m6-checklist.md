# M6 exit checklist

M6 is done when the four features below work in Bloco for a week of normal use:
the session list, the file tree, the settings panel, and theme import with
opacity and blur. This checklist covers them. Build it with:

```bash
cargo run -p app --release
```

Each row says who checked it. **Checked** means the change's author verified it
from outside the app (build, tests, a launch from the terminal). **Passed (user)**
means the user ran it in the window. **Pending** means it still needs a run.

Status as of this change:

- Build and tests: `cargo build --workspace` and `cargo test --workspace` pass.
- Config: the round trip through `toml_edit` keeps comments and unrelated keys
  (`cargo test -p config edit`). Theme files (`.itermcolors`, base16) parse and
  derive their chrome (`cargo test -p config import`).
- Settings rows: what each row shows and how a step changes the file
  (`cargo test -p app settings`).
- File tree: sorting, hidden files, opening and closing folders, a new root
  (`cargo test -p app sidebar`).
- Launch: the app starts with a theme file and `background_opacity`/`background_blur`
  set, and logs no settings error.
- In the window: the author could not see the window or send it keys (macOS screen
  recording and accessibility were not granted to the agent session). Every row
  under "In the window" is **Pending** until the user runs it.

## Build and tests

| # | Check | How to check | Status |
| --- | --- | --- | --- |
| 1 | Debug build | `cargo build --workspace` | Checked |
| 2 | Tests | `cargo test --workspace` passes | Checked |
| 3 | Config round trip | `cargo test -p config edit` passes | Checked |
| 4 | Theme file parsing | `cargo test -p config import` passes | Checked |
| 5 | Settings rows | `cargo test -p app settings` passes | Checked |
| 6 | File tree | `cargo test -p app sidebar` passes | Checked |
| 7 | Launch with a theme file | Put `theme = "sample"` and a `themes/sample.itermcolors` file in `~/.config/bloco`, launch: no `bloco:` line on stderr | Checked |
| 8 | Release build | `cargo build --release -p app` | Pending |

## Sidebar: session list

| # | Check | How to check | Status |
| --- | --- | --- | --- |
| 9 | Toggle | Cmd+Shift+B shows and hides the sidebar on the left. The palette has "Toggle sidebar" | Passed (user) |
| 10 | Sessions | With the sidebar open and two tabs, each tab is a row with its title and directory name, and the horizontal tab bar is hidden in both modes | Passed (user) |
| 11 | Select | Click a row: that tab becomes active and its pane has the keyboard | Passed (user) |
| 12 | Close | Click the × on a row: the tab closes. Closing the last tab closes the window | Passed (user) |
| 13 | New tab | Click + in the Sessions header: a new tab opens in the active pane's directory | Passed (user) |
| 14 | Mode switch | The strip on the sidebar's left edge: ▤ shows Sessions, ▦ shows Files, with no tab bar appearing. Cmd+Shift+E opens Files | Passed (user) |

## Sidebar: file tree

| # | Check | How to check | Status |
| --- | --- | --- | --- |
| 15 | Root | In a shell, `cd` to a project. The Files list shows that directory, folders first, no dotfiles | Passed (user) |
| 16 | Follows the pane | Change tabs or `cd` elsewhere: the list changes to that pane's directory | Passed (user) |
| 17 | Open a folder | Click a folder (▸): it opens (▾) and shows its children indented. Click again to close | Passed (user) |
| 18 | Insert a path | Click a file: its path is typed at the prompt with a trailing space, quoted if it has spaces. It does not run | Passed (user) |
| 19 | Live updates | Create a file in the directory from the shell: it appears in the list within about a second | Passed (user) |
| 20 | No folder | In a pane whose directory is gone, the list says "No folder for this pane" | Passed (user) |

## Settings panel

| # | Check | How to check | Status |
| --- | --- | --- | --- |
| 21 | Open and close | Cmd+, opens the panel. Escape closes it and focus returns to the pane | Passed (user) |
| 22 | Font size | The ‹ › arrows change the size by one point; the window follows at once | Passed (user) |
| 23 | Theme | The arrows step through `dark`, `light`, then each theme file | Passed (user) |
| 24 | Opacity | The arrows change the opacity by 5 points, down to 20%. The window shows what is behind it | Passed (user) |
| 25 | Blur | The arrows turn blur on and off. On macOS a see-through window shows a blurred background | Passed (user) |
| 26 | Option as Meta | The arrows cycle Off, Both, Left, Right, as in `config.toml` | Passed (user) |
| 27 | Comments kept | After each change above, `~/.config/bloco/config.toml` still has its comments, blank lines and the other keys, with only the changed value different | Passed (user) |
| 28 | Error | Break `config.toml` by hand, then press an arrow: the error bar says the file has an error and the file is not written | Passed (user) |

## Theme import

| # | Check | How to check | Status |
| --- | --- | --- | --- |
| 29 | Import an iTerm2 preset | Settings → "Import theme from a file…" (or the palette's "Import theme from file"), pick a `.itermcolors` file: it is copied to `~/.config/bloco/themes/`, selected as `theme`, and the colors change | Passed (user) |
| 30 | Import a base16 scheme | The same with a base16 `.yaml` file | Passed (user) |
| 31 | Bad file | Pick a file with the wrong contents: the error bar names the file, and nothing is copied | Passed (user) |
| 32 | Theme file edits | Edit a copied theme file in `themes/` by hand: the colors update within about a second | Passed (user) |

## Opacity and blur

| # | Check | How to check | Status |
| --- | --- | --- | --- |
| 33 | Opacity from file | Set `[window] background_opacity = 0.8` in `config.toml`: the window background is see-through. Text and the tab bar stay opaque | Passed (user) |
| 34 | Blur from file | Add `background_blur = true`: the see-through background is blurred | Passed (user) |
| 35 | Out of range | `background_opacity = 0.05` is an error in the error bar, and the last good settings stay | Pending |

## Notes

- The sidebar's open state and mode are not saved between launches. The
  settings panel and the sidebar do not change the layout file.
- Font family is not in the settings panel. Edit `[font] family` in
  `config.toml`.
- Opacity applies to the terminal background. Chrome (tab bar, sidebar, palette)
  stays opaque so its text remains readable.
