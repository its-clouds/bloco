# Visual refresh (Warp-inspired, not a copy)

Goal: make Bloco look calm and modern. Inspiration is Warp: tabs in the title
bar, blocks separated by hairlines rather than heavy bars, a raised input area
at the prompt. The palette and details are our own.

Principles:

- Few surfaces, clearly stepped: title bar (darkest) → terminal background →
  raised surfaces (input area, chips, palette).
- Hairlines (1px) instead of filled bars. Colour carries meaning only: accent =
  selection/focus, red = failure, green = success.
- Rounded corners on everything that floats or is a chip (4px chips, 6px tabs,
  10px overlays).

GPUI is pinned to Zed `c4273271`. Its source is at
`~/.cargo/git/checkouts/zed-a70e2ad075855582/c427327/crates/gpui/` — check API
names there, do not guess.

Rules for every implementer: no `git add`, `git commit` or `git push`. Touch
only the files your work package owns. Keep the code style of the file you
edit (doc comments on items, short comments, same naming). `cargo build` and
`cargo test` for the crates you touched must pass.

---

## WP-A — Theme tokens (foundation)

Files: `crates/config/src/theme.rs`, `crates/app/src/colors.rs`,
`crates/app/src/terminal_element.rs` (only the `PADDING` constant), plus the
smallest possible fix-ups at use sites so the workspace still builds.

`Chrome` (both in `config` and the app's `colors::Chrome`) becomes:

| Field | Meaning |
| --- | --- |
| `tab_bar` | Title bar / tab strip background |
| `tab_active` | Active tab pill |
| `tab_hover` | **new**, replaces `tab_inactive`: inactive tab pill under the pointer |
| `close_hover` | Close-button / small button hover |
| `text` | Primary UI text |
| `muted` | Secondary UI text |
| `divider` | Hairlines |
| `overlay` | Palette and find bar background |
| `surface` | **new**: raised in-terminal surfaces (input area, chips) |
| `selected` | Selected palette row |
| `accent` | Focus / selection |
| `error` | Failure |
| `success` | **new**: success |

`tab_inactive` is removed; where it was used, use `tab_bar` for now (WP-B
restyles it).

Dark theme (name stays `"dark"`, doc comment: "Bloco Night: a cool near-black
with a soft periwinkle accent."):

- background `0x15171c`, foreground `0xd5d9e3`, cursor `0x8da2ff`,
  selection `0x2c3350`, find_match `0xe2b86b`, find_current `0xf08c5a`,
  find_text `0x15171c`
- ansi 0–7: `0x23262e 0xf0717d 0x9fd17a 0xe5c07b 0x7aa2f7 0xc39bf2 0x6fd0e0 0xc8ccd6`
- ansi 8–15: `0x5c6372 0xff8a95 0xb4e391 0xf2d398 0x9bb8ff 0xd5b4ff 0x8fe0ee 0xf2f4f8`
- chrome: tab_bar `0x0f1014`, tab_active `0x22252e`, tab_hover `0x1a1c23`,
  close_hover `0x2e323d`, text `0xe3e6ee`, muted `0x7d8494`, divider
  `0x262932`, overlay `0x1c1e25`, surface `0x1b1d24`, selected `0x2a2f42`,
  accent `0x8da2ff`, error `0xf0717d`, success `0x9fd17a`

Light theme (name stays `"light"`, doc comment: "Bloco Day: the light twin of
Bloco Night."):

- background `0xfbfbfc`, foreground `0x2f3340`, cursor `0x4f6bed`,
  selection `0xd9e0fb`, find_match `0xffe08a`, find_current `0xffb070`,
  find_text `0x2f3340`
- ansi: keep the current One Light values, except ansi 0 = `0x2f3340`
- chrome: tab_bar `0xeceef2`, tab_active `0xffffff`, tab_hover `0xe2e5eb`,
  close_hover `0xd6dae2`, text `0x2f3340`, muted `0x7a8091`, divider
  `0xe0e3e9`, overlay `0xffffff`, surface `0xf2f3f6`, selected `0xe3e8fc`,
  accent `0x4f6bed`, error `0xd03b4a`, success `0x3f9a4c`

`PADDING` in `terminal_element.rs`: 8.0 → 12.0.

---

## WP-B — Window chrome: title-bar tabs, dividers, palette, error bar

Files: `crates/app/src/main.rs`, `crates/app/src/workspace.rs`,
`crates/app/src/split_tree.rs` (only `DIVIDER` and its tests if needed),
`crates/app/src/palette.rs` (only if a helper is needed for key chips).

### Tabs in the title bar

- `main.rs`: open the window with a transparent title bar:
  `titlebar: Some(TitlebarOptions { title: Some("Bloco".into()),
  appears_transparent: true, traffic_light_position: Some(point(px(12.), px(13.))) })`
  (verify field names in GPUI's `platform.rs`).
- The tab bar is **always** shown (even with one tab), because it is now the
  title bar. `TAB_BAR_HEIGHT` = 38.
- On macOS the bar has 78px left padding so the traffic lights sit on it
  (`cfg!(target_os = "macos")`). Bottom border: 1px `chrome.divider`.
  Background `chrome.tab_bar`.
- Empty space in the bar moves the window on drag (`window.start_window_move()`
  on left mouse down) and zooms on double click
  (`window.titlebar_double_click()`). Clicks on tabs/buttons must not reach
  this handler (they already stop propagation or are separate elements; make
  sure).
- Tabs are pills: height 26px, centred vertically, `rounded(px(6.))`, `px_3`,
  4px gap between tabs, text size 13px (`text_size(px(13.))`), min width 90,
  max width 200, title ellipsised.
  - active: bg `tab_active`, text `chrome.text`
  - inactive: transparent bg, text `chrome.muted`; hover bg `tab_hover`,
    text `chrome.text`
  - No underline/border on the active tab any more.
- Close button `×`: 16px square, `rounded(px(4.))`, muted, hover bg
  `close_hover` + text colour. Shown on the active tab always, and on
  inactive tabs only while the tab is hovered (use GPUI `group` /
  `group_hover`; if that is impractical, make it fully transparent text on
  inactive tabs and visible on hover).
- New-tab `+`: 24px square, `rounded(px(6.))`, muted, hover bg `tab_hover`
  and text colour, 4px left margin.

### Split dividers

- Visually a 1px hairline in `chrome.divider`. Set `split_tree::DIVIDER` to
  1.0 (fix any tests that depend on the old value).
- Keep a comfortable grab area: the 1px divider element gets a `relative()`
  parent behaviour with an absolutely positioned, transparent child 7px wide
  (horizontal split) / tall (vertical) centred on the line that carries the
  resize cursor and the mouse-down handler.

### Command palette

- Backdrop: the full-window overlay gets bg `black` at 0.3 opacity
  (`gpui::black().opacity(0.3)`), still closes on click.
- Card: width 600, top offset 72, bg `chrome.overlay`, 1px border
  `chrome.divider`, `rounded(px(10.))`, `shadow_lg()`, `overflow_hidden()`,
  no outer padding.
- Query row: `px_4 py_3`, text 14px, bottom border 1px `chrome.divider`. Show
  the query text; when empty, show muted placeholder "Search commands". Drop
  the `> ` prefix. A 1px-wide, 16px-tall accent bar after the text acts as a
  caret.
- Rows container: `p_1p5` (6px). Row: `px_3 py_1p5`, `rounded(px(6.))`, text
  13px. Selected row: bg `chrome.selected`. Unselected title text
  `chrome.text`.
- Shortcuts are key chips: split the pretty string on spaces (one chip per
  stroke), each chip `px_1p5`, `rounded(px(4.))`, bg `chrome.surface`, 1px
  border `chrome.divider`, text 11px `chrome.muted`, 4px gap.
- Empty result: one muted row "No matching commands".

### Config error bar

- bg `chrome.error` at 0.12 opacity, 1px top border `chrome.error`, text
  `chrome.error`, `px_4 py_2`, text 13px.

---

## WP-C — Blocks and the find bar

Files: `crates/app/src/terminal_element.rs`, `crates/app/src/block_header.rs`,
`crates/app/src/terminal_view.rs` (only `render_find_bar` and its constants).

### Header text model (`block_header.rs`)

Replace `details: String` with chips:

```rust
pub enum ChipKind { Failed, Duration, Dir }
pub struct Chip { pub text: String, pub kind: ChipKind }
pub struct HeaderText { pub command: String, pub chips: Vec<Chip>, pub failed: bool }
```

- Success shows **no** status chip (calm by default). Failure: chip text
  `exit {code}`, kind `Failed`.
- Duration chip as today (`format_duration`). Directory chip via `display_dir`.
- Chip layout on the row, right-aligned: each chip occupies `text + 2` cells
  (one cell padding each side), 1 cell gap between chips, 1 cell margin from
  the right edge. If the chips need more than half the columns, drop the Dir
  chip first, then shorten. Expose a pure function
  `layout_chips(chips, columns) -> Vec<(usize /*start col*/, usize /*cells*/)>`
  plus `HeaderText::command` cut to fit the space left (keep 2 cells gap).
  Unit-test the layout (right alignment, dropping Dir when narrow).
- Make `display_dir` `pub` (WP-D reuses it).

### Painting (`terminal_element.rs`)

- Add rounded quads to `Prepaint`: `chips: Vec<(Bounds<Pixels>, Hsla)>` painted
  after `rects` and before text with `fill(bounds, color).corner_radii(px(4.))`
  (check the PaintQuad API). Add a small helper
  `fn chip(&self, out, metrics, line, start, cells, bg, fg, text)` that pushes
  the rounded bg (the cell bounds inset 3px top and bottom) and the shaped
  text starting one cell in. WP-D reuses this helper.
- Header row:
  - No filled background any more.
  - A 1px `chrome.divider` hairline across the **full pane width** (from
    `metrics.origin.x - PADDING`, width `metrics.size.width`) at the top of the
    header row. The very first visual row has no hairline.
  - Text: `❯ ` in `chrome.accent` (in `chrome.error` when failed), then the
    command in `chrome.text`, **bold**.
  - Chips: bg `chrome.surface`, text `chrome.muted`; the `Failed` chip uses bg
    `chrome.error.opacity(0.15)` and text `chrome.error`.
- Block tint and rail. For each block whose header is on screen (and the
  sticky block): its rows are its header row plus every `Row::Line(l)` with
  `output_line <= l < end_line`.
  - Selected block: bg `chrome.accent.opacity(0.08)` over those rows (full
    pane width) and a 3px `chrome.accent` rail on the pane's left edge
    (`metrics.origin.x - PADDING`) over the same rows. This replaces the old
    0.12 tint and header-only bar; `Frame::selected` may still be used for
    the selected range.
  - Failed block (not selected): bg `chrome.error.opacity(0.06)` and a 3px
    `chrome.error` rail.
  - Tints are pushed before cell backgrounds/text so text stays on top.

### Find bar (`terminal_view.rs`, `render_find_bar`)

- Card: bg `chrome.overlay`, 1px border (`chrome.divider`, or `chrome.error`
  when invalid), `rounded(px(8.))`, `shadow_lg()`, `px_2 py_1p5`, top 10px,
  right 14px.
- Placeholder "Find" → "Find in terminal".
- Toggle buttons and close button: `rounded(px(4.))`, active toggle bg
  `chrome.selected` text `chrome.accent`; hover bg `close_hover`.
- Match counter text 12px muted.

---

## WP-D — Docked input area (runs after WP-C)

Files: `crates/term-session/src/session.rs` (Frame), new `crates/app/src/git.rs`,
`crates/app/src/main.rs` (only `mod git;`), `crates/app/src/terminal_element.rs`,
`crates/app/src/terminal_view.rs`, `crates/app/src/prompt.rs`.

### Where the prompt starts

- `Frame` gets `pub prompt_row: Option<usize>`: the visual row of the prompt
  mark (`prompt_line`) of the **last** block, when that block has a
  `prompt_line` and no `output_line` yet (the shell is at the prompt) and that
  line is on screen. `None` on the alternate screen and when there are no
  blocks. Add a test next to the existing frame tests.

### Git branch (`git.rs`)

- `pub fn branch(dir: &Path) -> Option<String>`: walk up from `dir` to find
  `.git` (a directory, or a file with `gitdir: <path>` for worktrees —
  resolve relative paths against the file's directory). Read `HEAD`:
  `ref: refs/heads/<name>` → `<name>`; a bare hash → its first 7 chars.
  Unit tests using a temp dir under `std::env::temp_dir()` (unique name per
  test, removed at the end).
- `TerminalView` caches `(cwd, Option<branch>)` and refreshes it when the
  session's cwd changes or a block finishes (find where session events are
  handled). Never read files on every frame.

### The input area (`terminal_element.rs`)

- New field on `TerminalElement`: `context: Option<PromptContext>` where
  `pub struct PromptContext { pub dir: String, pub branch: Option<String> }`
  (dir via `block_header::display_dir`).
- When the editor is drawn (`self.editor` is some and the cursor is visible):
  - `first` = `prompt_row` if known, else the editor origin line; `last` = the
    last screen row the editor drew (make `draw_editor` return it too).
  - Push, **before** any other rect, a band in `chrome.surface` across the
    full pane width from `y(first) - 6px` to `y(last + 1) + 6px`, clamped to
    the pane bounds; and a 1px line at its top in `chrome.divider`
    (`chrome.accent.opacity(0.45)` when the pane is focused).
  - Context chips right-aligned on row `first` using WP-C's chip helper and
    layout rules: `dir` chip, then the branch chip (text = branch name) with
    text `chrome.success`. Draw them only if every cell they cover on that row
    is blank (`' '`) and the editor/cursor is not on that part of the row.
- The selection/block tints must not be drawn over the band area of the live
  prompt (the live prompt is not a finished block, so this should hold;
  verify).

### Highlight colours (`prompt.rs::color`)

- Command → ANSI green, Keyword → accent, Option → ANSI cyan, String → ANSI
  yellow, Variable → ANSI magenta, Operator → accent, Comment → muted, none →
  foreground. Update tests that assert colours, if any.

---

# Round 2 (after first use)

Feedback: the input should always sit at the bottom of the pane, Warp-style,
in a new tab too; the block header row is too plain; tabs sit too close to the
traffic lights and need an icon. The in-grid band from WP-D is replaced by a
real dock. See [ADR 0004](decisions/0004-docked-input.md).

## WP-F — Tabs and header strip (runs first)

Files: `crates/app/src/workspace.rs`, `crates/app/src/terminal_element.rs`
(only `header()` and the block tint code).

- `TRAFFIC_LIGHT_PADDING` 78 → 90.
- Each tab starts with a small terminal icon: the text `>_` in the monospace
  terminal font (`self.style.font_family`), 11px, `chrome.accent` on the active
  tab and `chrome.muted` on inactive ones, 6px gap before the title.
- Header row becomes a strip: fill the whole header row (full pane width) with
  `chrome.surface`; failed block header: `chrome.error.opacity(0.14)` instead;
  selected block header: `chrome.accent.opacity(0.16)` instead. The block
  tint/rail over output rows stays as it is. Keep the hairline on top.
- Command text on the header uses the theme's brightest text: ANSI bright
  white (index 15) for dark themes — simplest is `chrome.text`, bold, which
  already is. Leave it, but header chips now use bg `chrome.tab_active` (the
  strip is `surface`, so `surface` chips would disappear).

## WP-E — Docked input (runs after WP-F)

Files: `crates/term-session/src/blocks.rs`, `crates/term-session/src/session.rs`,
`crates/app/src/terminal_element.rs`, `crates/app/src/terminal_view.rs`,
`crates/app/src/block_header.rs`, `crates/app/src/prompt.rs` (only if needed).

### term-session

- `Blocks` gets `prompt_seen: bool`, set when a `PromptEnd` (B) mark arrives,
  never cleared. Expose `pub fn prompt_seen(&self) -> bool` and
  `pub fn live_prompt_line(&self) -> Option<usize>` (the private
  `prompt_line`: set by `A`, taken by `C`).
- `Session::docked(&self) -> bool`: `prompt_seen` and not on the alternate
  screen. Fish (no B) and shells without integration are never docked.
- `Session::frame(selected, hide_prompt: bool)`. When `hide_prompt` and not
  on the alternate screen and `live_prompt_line` is `Some(p)`: build rows only
  from buffer lines `< p` — call `frame_rows(all, top', p, screen)` with
  `top' = p.saturating_sub(screen)` when `display_offset == 0` (else the usual
  `top`). The frame then has **fewer rows than the screen** when the content
  is short; that is expected. The cursor is `visible: false`.
- Remove `Frame::prompt_row` and its helper/test from WP-D (superseded).
- Running blocks get a header too: `Block::fold` no longer requires
  `finished_at`/`end_line` — a running block (has `prompt_line` and
  `output_line`, no `end_line`) folds `prompt..output` into a header with no
  hidden lines. Update the `rows`/`frame_rows` tests and doc comments that say
  running blocks have no header. `Frame::headers` includes running blocks.
  Collapsing stays finished-only.

### Pane layout (`terminal_element.rs`, `terminal_view.rs`)

- `pub fn dock_height(line_height) -> Pixels` =
  `1 + 8 + line_height + 6 + line_height + 10` (top border, top pad, chips
  row, gap, one editor row, bottom pad).
- `grid_size(area, cell_width, line_height, docked: bool)`: when docked,
  subtract `dock_height` from the height first. Every caller passes
  `session.docked()`. Entering/leaving full-screen apps changes `docked`, so
  the grid (and PTY) resizes then; that is fine. Typing never resizes the PTY.
- When docked, the element splits its bounds: grid area on top, dock strip at
  the bottom. `Metrics` describe the grid area only.
- Call `session.frame(selected, docked)`.
- **Bottom anchoring**: when the frame has fewer rows than the grid has
  (`rows.len() < metrics.rows`), shift the grid origin down by
  `(metrics.rows - rows.len()) * line_height` and store that shifted origin in
  `self.metrics` so mouse mapping stays right. New tab ⇒ empty grid, dock at
  the bottom.
- Remove the WP-D in-grid band, its top line and in-grid context chips, and
  the drawing of the editor inside the grid (`draw_editor` from the shell
  cursor). The grid cursor is not drawn while the prompt is hidden.

### The dock (painted by `TerminalElement` in its bottom strip)

- Background `chrome.surface`; 1px top border `chrome.divider`, or
  `chrome.accent.opacity(0.45)` when the pane is focused and the editor owns
  input.
- Row 1 (chips), left-aligned starting at `PADDING`: dir chip (text = 
  `PromptContext.dir`), then the branch chip (text = branch, fg
  `chrome.success`), chip bg `chrome.tab_active`, fg `chrome.muted`, 1 cell
  gap between chips. Reuse the `chip` helper with a `Metrics` whose origin is
  the chip row.
- Row 2 (editor), starting at `PADDING`, wrapped at the grid's column count:
  - Prompt mode: the editor text with highlight colours (reuse
    `draw_editor_row` logic), beam cursor (2px, `palette.cursor`) when focused
    and blinking on, outline when unfocused. Empty buffer and no search: a
    muted placeholder `Type a command` drawn after the cursor.
  - Ctrl+R search: the search line, as before.
  - Running / Unknown: muted `Running…` (Running) or nothing (Unknown); no
    cursor. Keys go to the PTY as today; the program's own cursor shows in the
    grid.
  - More than one editor row: the dock grows **upward** over the bottom of the
    grid (paint its background over the grid area) — the PTY is not resized.
    Cap at 8 rows.
- IME bounds = the dock cursor cell. Preedit (marked text) is drawn at the
  dock cursor while docked.
- Mouse: replace `PromptState::origin: (line, column)` with a pixel geometry
  the element writes each frame:
  `pub struct EditorGeometry { pub origin: Point<Pixels>, pub columns: usize, pub cell_width: Pixels, pub line_height: Pixels }`
  (origin = top-left of the first editor cell). `place_cursor` takes a window
  position: a click inside the dock's editor rows moves the cursor; a click
  elsewhere in the dock does nothing (and must not start a grid selection).

### Header chips for running blocks (`block_header.rs`)

- `ChipKind::Running`, text `running`, drawn with fg `chrome.accent`. Add a
  `Branch` kind and use it for the dock's branch chip (layout drops `Dir`
  first, then `Branch`).

### Docs

Update `docs/decisions/0003-input-editor-at-the-prompt.md` "Status" to
"Accepted; decision 1 superseded by ADR 0004" (one line), nothing else.

---

# Round 3 — Sidebar

Feedback: the left panels look rough, and with the Sessions panel open the
tabs show twice (sidebar and title bar).

## WP-G — Sidebar redesign

Files: `crates/app/src/workspace.rs`, `crates/app/src/terminal_view.rs` (two
small accessors only), `crates/app/src/sidebar.rs` (only if needed).

### Tabs live in one place

- Sessions panel open: the title bar shows **no tab pills and no `+`**. After
  the traffic-light padding it shows the sidebar toggle (below), then the
  active session's title centred in the remaining space (12px,
  `chrome.muted`, ellipsised). The empty bar still drags the window and
  double-click zooms.
- Files panel open, or sidebar closed: the title bar is as today (pills and
  `+`), with the sidebar toggle before the first pill.

### Sidebar toggle button (title bar)

- 24×24 hit area, `rounded(px(6.))`, hover bg `tab_hover`, 8px left margin
  from the traffic-light padding, 6px before the first pill. Clicking it
  dispatches the existing sidebar toggle action (find it — `toggle_sidebar`).
- The icon is drawn with divs, no font or asset: a 14×11 box with a 1.5px
  border (use `border_1()` if 1.5 is not possible) and `rounded(px(3.))`, plus
  a 1.5px (or 1px) vertical bar 3px from its left edge, full height. Colour
  `chrome.accent` when the sidebar is open, `chrome.muted` when closed.

### Frame

- `SIDEBAR_WIDTH` 240 → 260. Remove the activity strip (`ACTIVITY_WIDTH`,
  `sidebar_switch` and its `▤ ▦` glyphs).
- Sidebar bg `chrome.tab_bar` (same as the title bar), 1px `chrome.divider`
  border on its right. Inner padding: `px(6.)` left/right, `px(8.)` top.

### Header: segmented switch

- Row height 28, 6px bottom margin. A segmented control fills the row (flex
  1): bg `chrome.surface`, `rounded(px(6.))`, 2px padding. Two equal
  segments, "Sessions" and "Files", text 11px, centred, `rounded(px(4.))`,
  `py(px(3.))`. Active segment: bg `chrome.tab_active`, text `chrome.text`;
  inactive: text `chrome.muted`, hover text `chrome.text`. Clicking a segment
  switches `sidebar.mode` (keep whatever the old switch did, including
  syncing the file tree).
- Sessions mode only: a `+` button 24×24 to the right of the switch, 6px gap,
  `rounded(px(6.))`, muted, hover bg `tab_hover` + text; new tab on click.
- Remove the old "Sessions"/"Files" title and its bottom border.

### Session rows

- Each row: `px(8.)` horizontal, `py(px(6.))`, `rounded(px(6.))`, 8px gap
  between children, 2px gap between rows, items centred.
  - Active row bg `chrome.tab_active`; hover bg `tab_hover`. No border.
- Badge: 22×22, `rounded(px(5.))`, bg `chrome.tab_active` (`chrome.surface`
  on the active row so it still shows), centred `>_` in the terminal font
  (`self.style.font_family`) at 9px. Badge text colour: `chrome.accent` on
  the active row; `palette.resolve` ANSI yellow when that pane is running a
  command (input mode `Running`); otherwise `chrome.muted`.
- Text column (flex 1, min width 0):
  - Title: `tab_title()`, 12px, `chrome.text`, ellipsised.
  - Subtitle: 10.5px (use 11px if fractional sizes look off), `chrome.muted`,
    ellipsised: the pane's directory via `dir_label`, then ` · <branch>` with
    the branch in `chrome.success` when the pane knows its git branch, and
    ` · running` when the pane is running a command. If the title and the
    directory label are identical, still show the full directory (use the
    `~/…` form from `block_header::display_dir`, not just the last
    component) so the row never reads `itscloud / itscloud`.
- Right side (flex-shrink 0): for tabs 1–9, the shortcut `⌘1`…`⌘9` at 10.5px
  in `chrome.muted` at 0.7 opacity. On row hover (GPUI `group` per row +
  `group_hover`), hide the shortcut and show a 16×16 `×` close button
  (`rounded(px(4.))`, hover bg `close_hover`). Tabs 10+ just show the close
  button on hover. Close must not select the row (stop propagation, as now).
- `TerminalView` accessors to add: `pub fn branch(&self) -> Option<String>`
  (from the cached `context`) and `pub fn is_running(&self) -> bool`
  (`session.input_mode() == InputMode::Running`).

### File tree rows

- Above the rows, the tree's root as `display_dir` text: 11px muted,
  `px(8.)`, 6px bottom padding, ellipsised.
- Row: height 24 (`h(px(24.))`), `rounded(px(4.))`, text 13px, left padding
  `8 + 14 * depth` px, hover bg `tab_hover`.
- Marker column 12px wide: `▾`/`▸` for folders at 9px in `chrome.muted` at
  0.7 opacity; empty for files. Folder names `chrome.text`; file names
  `chrome.muted`.
- Empty note ("No folder for this pane"): 12px muted, `px(8.)`.

Keep all behaviour: selecting/closing sessions, new tab, mode switching,
expanding folders, inserting a file path, and the existing actions/keybindings.

## WP-H — Resizable sidebar

Files: `crates/app/src/workspace.rs`, `crates/app/src/sidebar.rs`,
`crates/app/src/layout.rs`.

- One width for both panels (Sessions and Files): `Sidebar.width: f32`,
  default `SIDEBAR_WIDTH` (260). Clamp to `SIDEBAR_MIN` 180 ..
  `SIDEBAR_MAX` 480 (constants next to `SIDEBAR_WIDTH`). Add a pure
  `fn clamp_width(width: f32) -> f32` in `sidebar.rs` with a unit test.
- Drag handle on the sidebar's right edge: the visible 1px border stays; over
  it, an absolutely positioned transparent strip 7px wide centred on the
  border, full height, `cursor_ew_resize()`, same idea as the split divider's
  grab area. Mouse down starts a `sidebar_drag: bool` on the workspace
  (stop propagation). While dragging, the root's existing mouse-move listener
  sets `width = clamp_width(event.position.x - sidebar_left_x)` (the sidebar
  starts at the window's left edge, x = 0, so the width is the mouse x) and
  notifies. Mouse up (and a move with no left button pressed) ends the drag,
  exactly like `divider_drag`.
- Hovering the handle shows a 2px `chrome.accent` line at 0.6 opacity in
  place of the 1px border; while dragging it stays visible.
- Double-click on the handle resets the width to `SIDEBAR_WIDTH`.
- Panes resize their grids through the existing layout path; no extra work
  should be needed, but check the terminal grid follows the drag.
- Persist the width: `SavedLayout` gets
  `#[serde(default)] pub sidebar_width: Option<f32>`, written with the layout
  and applied (clamped) when the layout is restored. Old layout files without
  the field must still load (add a test).
