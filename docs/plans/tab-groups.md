# Plan: move tabs and sessions, rename, and tab groups

## Goal

Four capabilities in the workspace (`crates/app/src/workspace.rs`):

1. **Move tabs**: reorder tabs in the tab bar (drag, plus keyboard actions) and move a tab into a group.
2. **Move sessions**: move a pane (session) from one tab to another, either into a tab's split tree or as its own tab.
3. **Rename**: rename a tab and a group (and a session/pane title, if it is cheap in the same pass).
4. **Groups**: create, rename, delete a group; a group holds tabs and shows as a labeled section in the tab bar and sidebar.

"Session" here means a pane (a `TerminalView`) inside a tab's `SplitTree`. The sidebar's Sessions list shows tabs, so moving a tab there is the sidebar's job too.

## Current state (what the code already does)

- `Tab { panes: SplitTree<Entity<TerminalView>>, active: Entity<TerminalView> }` in `workspace.rs:57`. Tabs live in `Workspace.tabs: Vec<Tab>`, selected by `active_tab: usize`.
- Tab title comes from the active pane: `TerminalView::tab_title()` (`terminal_view.rs:271`) -> `tab_label`. No user-set name exists.
- `SplitTree` (`split_tree.rs`) supports `split`, `remove`, `leaves`, `layout`. It has no "detach" or "insert" by pane; `remove` returns `Option<Self>` (None when the last pane goes).
- `remove_tab` (`workspace.rs:2021`) is a pure generic helper with tests. Good pattern to copy.
- Persistence: `layout.rs` has `SavedLayout { tabs, active_tab, sidebar_width }` and `SavedTab { root, active }`. Written on change and on quit. Uses `#[serde(default)]` for back-compat already.
- Tab bar: `render_tab_bar` (`workspace.rs:815`) renders tabs as clickable `div`s with `on_mouse_down`. No drag support yet.
- Commands are registered in `commands.rs` via `actions!` and a `Command` list with default keys; keymap overrides in `config/src/keymap.rs`.
- Sidebar: `sidebar.rs` holds state (`Sidebar`, `Mode::Sessions | Files`). `render_sidebar` (`workspace.rs:1303`) draws the list.

## Design

### Data model (pure, no GPUI)

New module `crates/app/src/tab_groups.rs` with unit tests, same style as `split_tree.rs`:

```rust
pub type GroupId = u64;

pub struct Group { pub id: GroupId, pub name: String }

/// Pure list ops on tabs' order and grouping, generic over the tab payload.
pub fn move_tab(order: &mut Vec<..>, from: usize, to: usize) -> usize   // returns new active index
pub fn group_of(...)
pub fn remove_group(...)  // its tabs become ungrouped, keep their order
```

Keep grouping data separate from `Tab` so the pure logic can be tested without GPUI:

- `Tab` gains `name: Option<String>` (user rename; `None` means use the automatic title) and `group: Option<GroupId>`.
- `Workspace` gains `groups: Vec<Group>` and `next_group_id: GroupId`.
- Rule: tabs of one group are kept **contiguous** in `tabs`. Moving a tab into a group inserts it at the end of that group's run. Moving a tab out of a group places it right after the group's run. This keeps the tab bar render simple (a group = a run of tabs) and makes the sidebar grouping trivial.

### Moving tabs

- Actions: `MoveTabLeft`, `MoveTabRight` (swap with neighbour, stay inside the group run; crossing a group boundary moves the tab in/out of that group).
- Mouse: drag a tab in the tab bar onto another tab's position to reorder. Use the existing `DividerDrag` pattern: a `TabDrag { from: usize }` state set on mouse-down, resolved on mouse-up over a target index. Keep it simple: no ghost image, just a drop indicator line.
- Keep `active_tab` pointing at the same tab after a move (track by index before/after).
- Layout save: write immediately after any change (the existing "only a change is written" path already does this on render/tick; confirm).

### Moving sessions (panes) between tabs

- Action: `MovePaneToTab(index)` via the palette and a keybinding, and `MovePaneToNewTab`.
- Mouse: a pane can be dragged to a tab in the tab bar (drop on tab = move there; drop on empty bar area = new tab). Optional in the first pass, keyboard + palette first.
- Implementation:
  1. Take the active pane `p` from source tab `s` with `SplitTree::remove(&p)`.
  2. If the source tree becomes `None`, close the source tab (reuse `close_tab` logic, adjust `active_tab`).
  3. Insert into target tab `t` with `split(&target_active, Axis::Horizontal, p)` (side by side with the target's active pane). If target is empty, which should not happen, use `SplitTree::new(p)`.
  4. Set target `active = p`, make target tab active, focus `p`.
- Pane keeps its shell, scrollback and subscriptions. Only its position changes. Subscriptions in `pane_subscriptions` are keyed by `EntityId`, so they stay valid.
- Must not break `tab_index_of` (`workspace.rs:240`): it searches by pane; verify after the move.

### Rename

- Tab: double-click a tab title, or action `RenameTab`. Show an inline text field in the tab (reuse the input approach from `find.rs` or `palette.rs`, whichever is the simpler existing text field). Enter commits (empty string clears the name), Esc cancels.
- Group: same inline editor on the group label in the tab bar and the sidebar header.
- Session (pane): optional in this pass. If done, it lives on `TerminalView` as `custom_title: Option<String>` and `tab_title()` returns it first. Do this only after tab and group rename work.
- Persist: `SavedTab.name: Option<String>` (`#[serde(default)]`), `SavedLayout.groups: Vec<SavedGroup>` (`#[serde(default)]`), `SavedTab.group: Option<GroupId>`.

### Groups

- Action `NewGroup` creates a group named `Group N`, and moves the active tab into it. Action `RenameGroup`, `DeleteGroup` (its tabs become ungrouped, tabs are not closed), `MoveTabToGroup` (palette lists existing groups plus "New group").
- Tab bar: a group renders as a labeled chip before its tabs (name + thin accent line). Clicking the chip label renames it. Right-click or a small `×` deletes it.
- Sidebar Sessions list: shows groups as section headers with their tabs underneath. Ungrouped tabs at the bottom or top, one rule for the whole app (pick top, and say so in the PR).
- Empty groups: allowed (created, then the only tab leaves). Delete them with `DeleteGroup` only, so the user does not lose a group by accident.

### Commands and keys

Add to `commands.rs` `actions!` and the `COMMANDS` list, with titles that show in the palette:

| Action | Default key (macOS-friendly, check existing keys first) | Title |
|---|---|---|
| `MoveTabLeft` | `cmd-shift-[` | Move Tab Left |
| `MoveTabRight` | `cmd-shift-]` | Move Tab Right |
| `MovePaneToNewTab` | none | Move Session to New Tab |
| `MovePaneToTab` | none (palette) | Move Session to Tab... |
| `RenameTab` | none (also double-click) | Rename Tab |
| `RenameGroup` | none | Rename Group |
| `NewGroup` | none | New Group |
| `DeleteGroup` | none | Delete Group |
| `MoveTabToGroup` | none | Move Tab to Group... |

Check `config/src/keymap.rs` for conflicts with existing defaults before choosing keys. Keep the table's keys only if free.

### Persistence and back-compat

- Old `layout.json` (no groups, no names) must load unchanged: all new fields `#[serde(default)]`.
- Add a unit test in `layout.rs` that parses an old-format JSON string.

## Steps (each one a reviewable commit, in this order)

1. **Pure model**: `tab_groups.rs` with `move_tab`, group run rules, `remove_group`, unit tests. No UI change.
2. **Persistence fields**: `SavedTab.name`, `SavedTab.group`, `SavedLayout.groups`, back-compat test in `layout.rs`. Wire save/load to the new fields (defaults for now).
3. **Tab names and rename**: `Tab.name`, `tab_title` respects it, `RenameTab` action + inline editor + double-click. Persist.
4. **Tab reorder**: `MoveTabLeft/Right` actions, then mouse drag in the tab bar. Keep `active_tab` stable.
5. **Groups model + actions**: `groups` on `Workspace`, `NewGroup`, `RenameGroup`, `DeleteGroup`, `MoveTabToGroup`. Persist.
6. **Group UI**: group chips in the tab bar, group sections in the sidebar Sessions list, inline rename on the chip.
7. **Move pane between tabs**: `MovePaneToTab`, `MovePaneToNewTab`, close source tab if it empties. Palette entries. Drag a pane to a tab is optional, do last.
8. **Keys and palette**: bind the keys from the table (after checking conflicts), add palette entries, add `keymap.toml` docs if the file documents actions.
9. **Session (pane) rename**: only if steps 1-8 are done and green.

## Verification

- `cargo test -p app` (and `-p config`, `-p term-session` if touched) must pass after every step.
- `cargo clippy --workspace` clean for new code.
- Manual check in the running app (`run` skill or `cargo run -p app`): create 3 tabs, group two, reorder, rename, move a pane to another tab, quit, relaunch, confirm names/groups/order come back, and confirm an old `layout.json` still loads.
- Screen recordings in the repo root are the user's; do not touch them.

## Constraints for the implementer

- No `git add` and no `git push` (user's CLAUDE.md). Leave changes in the working tree; the user commits.
- Match the surrounding code: doc comments in the same plain style, the same naming, tests next to code.
- Do not refactor unrelated code. Keep each step small.
- If a step turns out to need a design decision (for example, drag-to-group semantics or where ungrouped tabs sit in the sidebar), pick the simplest option that matches this plan and list it in the final report.

## Open decisions (defaults chosen)

- Ungrouped tabs sit **above** groups in the sidebar. (Default, change if the user prefers otherwise.)
- Tabs of a group are contiguous in the tab bar. Moving a tab out of a group places it after the group's run.
- Moving a pane to another tab splits beside the target tab's active pane, horizontal axis.
- Deleting a group never closes its tabs.

---

## Status on 2026-11-05 and remaining work (phase 2)

Steps 1 to 8 are in the working tree (uncommitted) and `cargo test -p app` passes (136 tests):
`tab_groups.rs` (pure ops + tests), `Tab.name`/`Tab.group`, `Renaming::{Tab, Group}` inline editor with
double-click, `MoveTabLeft/Right`, tab-bar drag (`tab_drag` + `drag_tab_over`), `NewGroup`/`RenameGroup`/
`DeleteGroup`/`MoveTabToGroup` with a picker, group chips, grouped sidebar Sessions list
(`sidebar_sessions`, `render_group_header`), `MovePaneToTab`/`MovePaneToNewTab` (`detach_pane`,
`move_active_pane`), and persistence (`SavedTab.name/group`, `SavedLayout.groups`, back-compat test).

What is still missing, in this order. Each step must leave `cargo test -p app` green.

### Step A: drag sessions in the sidebar (reorder and move between groups)

Today sidebar rows only click to select. Make them draggable, same pattern as `tab_drag`:

1. In `tab_groups.rs` add a pure, tested helper:
   `pub fn move_to_group<T: Grouped>(items: &mut Vec<T>, from: usize, group: Option<GroupId>, before: Option<usize>) -> usize`
   - `before = Some(i)`: the item takes `items[i]`'s group and lands right before it (index adjusted for the removal).
   - `before = None`: the item joins `group` at the end of that group's run (reuse `set_group`).
   - Must keep the contiguity rule. Tests: drop on a row of another group, drop on a group header,
     drop on an ungrouped row, drop on itself (no-op), and contiguity holds after each.
2. In `workspace.rs` add `sidebar_drag: Option<usize>` (tab index). Set it on mouse-down on a session row
   (where rows are built, near `workspace.rs:2261`), alongside the existing select-on-click.
3. On mouse-up over a session row: `move_to_group(.., before: Some(target))`. On mouse-up over a group
   header (`render_group_header`): `move_to_group(.., Some(id), None)`. On mouse-up over the
   "ungrouped" area/top: group `None`. Keep `active_tab` on the same tab via `tab_groups::follow`
   (or by remembering the active tab's pane entity and finding it again — simpler and safer).
4. Clear `sidebar_drag` in the root `on_mouse_up` (next to `ws.tab_drag = None`, `workspace.rs:~2674`).
5. Visual: while dragging, give the row under the mouse a 2px accent top border (`chrome.accent`),
   and the header an accent background tint. No ghost image.
6. A click without movement must still just select the tab (only treat it as a drop when the target
   differs from the source).

### Step B: drag a tab in the tab bar onto a group chip

The tab bar drag already reorders. Add: mouse-up over a group chip puts the dragged tab in that group
(`set_tab_group(index, Some(id))`). Reuse `move_to_group` from step A.

### Step C: rename a session (pane)

1. `TerminalView` gets `custom_title: Option<String>` plus `pub fn set_custom_title(&mut self, Option<String>)`
   and `pub fn custom_title(&self) -> Option<&str>`. `tab_title()` returns the custom title first.
2. `Renaming::Pane { pane: EntityId (or Entity<TerminalView>), text: String }`. `finish_rename` handles it
   with `tab_groups::clean_name` (blank clears it).
3. Action `RenamePane` in `commands.rs` (`id: "pane.rename"`, title "Rename session", no default key),
   registered in `workspace.rs` `on_action`. It renames the active pane of the active tab. Where the
   editor shows: in the tab bar on the active tab (same spot as tab rename) is fine.
4. Persist: `SavedNode::Pane { cwd, #[serde(default)] name: Option<String> }`. Extend the existing
   back-compat test so an old pane without `name` still loads.
5. Precedence for a tab's title: `Tab.name` > active pane's custom title > automatic title.

### Step D: clean up

- Fix the clippy warnings in files this feature touched (`workspace.rs`, `tab_groups.rs`, `layout.rs`,
  `commands.rs`, `terminal_view.rs`): collapsible `if`, `.last()` on a double-ended iterator,
  single-arm `match`. Do not touch warnings in unrelated crates or code.
- Update `crates/app/README.md` with the new actions (sidebar drag, rename session) if it lists features.

### Done means

- `cargo test -p app` and `cargo test -p config` pass; `cargo clippy -p app` has no warnings in touched code.
- `cargo build -p app` succeeds.
- A final report that lists: what was done per step, new tests, any decision taken that this plan did
  not settle, and anything not finished.
- No `git add`, no `git commit`, no `git push`. Do not touch the `.mov` files or `iterm2.itermcolors`.
