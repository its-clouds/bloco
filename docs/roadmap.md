# Roadmap

**Goal: get to a terminal the author uses every day as soon as possible**, then
add the Warp-style features one milestone at a time. Performance is checked
when it becomes noticeable, not up front (see
[ADR 0002](decisions/0002-ui-toolkit.md#revisit-if)).

Every milestone ends with something you can run and use. A milestone is done
when its exit criterion is met in real use, not when the code compiles.

| # | Milestone | Usable as |
| --- | --- | --- |
| M0 | Session core | Done (headless only) |
| M1 | Single-window terminal | Done (2026-11-01) |
| M2 | Workspace | **Daily driver** as a classic terminal (replaces iTerm2/Warp for plain use) |
| M3 | Blocks | Daily driver with Warp-style output (built, manual checks pending) |
| M4 | Input editor | Daily driver with Warp-style input |
| M5 | Completions | |
| M6 | Sidebars, settings UI, themes | Feature-complete v1 |
| — | Later | Performance pass, Linux, SSH integration, WASM plugins, public release |

## M0 — Session core (done)

`spikes/ui-toolkit/spike-core`: zsh in a PTY, `alacritty_terminal` on a reader
thread, `snapshot()`, OSC 133 block records, tests.

## M1 — Single-window terminal (done)

Done 2026-11-01 and accepted by the author after real use. Code is in
`crates/term-session` and `crates/app`, with GPUI pinned to Zed `c4273271`
(v1.26.2). Manual checks and known issues are in
[m1-checklist.md](m1-checklist.md).

- Set up the product workspace at the repo root: `crates/term-session`
  (promoted from `spike-core`) and `crates/app` (GPUI binary). Pin GPUI to one
  Zed commit.
- Use Zed's `terminal_view` as the reference for rendering and input.
- Render the grid: colors (256 + true color), bold/italic/underline, cursor,
  wide chars, emoji, box drawing.
- Keyboard: all keys and modifiers, **dead keys and IME** (`ação`, `ç`),
  Cmd+C/Cmd+V, bracketed paste.
- Mouse: selection, copy, scroll through scrollback, clickable URLs.
- Window resize resizes the PTY.
- Font family and size from a minimal `config.toml`.

**Exit:** you can do an hour of real work in it with vim/nvim, git, htop, less
and Claude Code, and nothing renders wrong.

## M2 — Workspace (first daily-driver milestone)

Status: built, and the build and tests pass. Manual checks are pending; see
[m2-checklist.md](m2-checklist.md). M2 is done after the week of use.

- Tabs (Cmd+T, Cmd+W, Cmd+Shift+[ ]).
- Splits (Cmd+D, Cmd+Shift+D, Cmd+Opt+arrows, resize by dragging).
- New tabs and splits open in the current pane's directory.
- Command palette (Cmd+Shift+P) listing every action and its keybinding.
- `config.toml` + `keymap.toml` with hot reload; Warp-like default bindings
  ([requirements.md §6](requirements.md#6-keybindings-defaults-warp-compatible-where-possible)).
- One dark and one light theme.
- Build a local `.app` bundle you can keep in `/Applications` (unsigned is fine).

**Exit:** you use it instead of Warp for a full week of normal work.

## M3 — Blocks

Status: built, and the build and tests pass. Manual checks are pending; see
[m3-checklist.md](m3-checklist.md). M3 is done after the week of use.

- Shell integration for zsh, bash and fish, injected automatically (OSC 133 + metadata).
- Block rendering: header with command, exit status, duration and cwd; sticky
  header; collapse.
- Block navigation (Cmd+↑/↓) and copy command/output.
- Full-screen apps and SSH without integration fall back to classic mode.

**Exit:** blocks are on by default in your daily use and nothing about them
annoys you enough to turn them off.

## M4 — Input editor

Status: built, and the build and tests pass. The manual checks in the window
passed on 2026-11-02; see [m4-checklist.md](m4-checklist.md). M4 is done after
the week of use.

- The editor owns input at the prompt: multi-line, mouse cursor, Opt/Cmd moves,
  undo/redo, syntax highlighting.
- Shell history search.
- Session mode switching (prompt / running / full-screen / classic) as in
  [architecture.md](architecture.md#session-mode-state-machine).
- Decide prompt rendering ([open question 3](open-questions.md#3-prompt-rendering-in-the-input-editor)),
  decided in [ADR 0003](decisions/0003-input-editor-at-the-prompt.md): the shell's own prompt.

## M5 — Completions

- Completion menu with spec files and the shell's own completions as fallback
  ([open question 4](open-questions.md#4-completion-spec-format-and-source)).
- Start with specs for the commands you use most (git, docker, kubectl, cargo, npm).

## M6 — Sidebars, settings UI, themes

- Vertical tabs / session list.
- File tree rooted at the active pane's cwd.
- Settings UI that edits `config.toml` without losing comments.
- Theme import (iTerm2 `.itermcolors`, base16), background opacity and blur.

## Later

- **Performance pass:** run the harness in `spikes/ui-toolkit/bench/` against
  Alacritty and Ghostty. Earlier if something feels slow.
- Linux build.
- SSH integration (blocks over SSH).
- WASM plugin host (sidebar panels, block hooks).
- Public release: pick the name, plugin-SDK licensing, code signing,
  Homebrew cask, contributor docs.
