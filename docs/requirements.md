# Requirements

Priorities come from the initial interview. **v1 focus, in order:**

1. Rock-solid emulator
2. Blocks + input editor
3. Tabs, splits and sidebar

The plugin API is designed alongside v1 so the core doesn't paint itself into a
corner, but it ships after v1.

Delivery order is in [roadmap.md](roadmap.md). The first goal is a terminal the
author can use daily. Tabs and splits come right after the emulator because
daily use needs them.

## Platforms

| Platform | Target |
| --- | --- |
| macOS (Apple Silicon + Intel) | v1 |
| Linux (X11 + Wayland) | v1.x; architecture must be portable from day one |
| Windows | Not planned; don't make choices that rule it out |

## v1 — functional requirements

### 1. Terminal emulation

- Correct VT100/xterm emulation: alternate screen, mouse reporting, bracketed
  paste, true color, 256 color, Unicode (wide chars, emoji, combining marks),
  OSC 8 hyperlinks, OSC 52 clipboard, focus events, synchronized output.
- Kitty keyboard protocol (needed by modern TUIs like nvim and Claude Code).
- Scrollback with search.
- Ligature and font fallback support.

### 2. Blocks

- Each command + its output is a **block**, delimited through shell integration
  (see [architecture.md](architecture.md#blocks-and-shell-integration)).
- Block header shows: command, exit status (success/fail marker), duration,
  working directory, timestamp.
- **Sticky header** stays visible while scrolling through a long block's output.
- Block actions: select, copy command, copy output, copy both, collapse/expand,
  re-run, search within block, jump to previous/next block.
- Full-screen apps (alternate screen) take over the pane and do not produce a
  block body.
- Programs reading from stdin mid-command (e.g. `read`, password prompts,
  REPLs) receive keystrokes directly.

### 3. Input editor

- Multi-line, editor-style input owned by the terminal, not by the shell's line
  editor.
- Mouse cursor placement and selection, Opt+←/→ word moves, Cmd+←/→ line
  start/end, Cmd+Z/Shift+Cmd+Z undo/redo, multi-cursor.
- Syntax highlighting of the command line.
- History search (shell history is the source of truth).
- Must work with **zsh, bash and fish at v1**.

### 4. Completions

- Rich completion menu: subcommands, flags with descriptions, paths, git
  branches, etc.
- Source: **declarative completion specs** (subcommands, options, arguments,
  dynamic generators) with **fallback to the shell's own completion system**
  when no spec exists.
- Specs are plain files shipped with the app and overridable by the user.

### 5. Workspace and layout

- Tabs, with a **vertical tabs / session list** sidebar option.
- **Split panes**, horizontal and vertical, keyboard navigable and resizable.
- **File tree / project sidebar** rooted at the active pane's working
  directory.
- Command palette listing every action and its keybinding.

### 6. Keybindings (defaults, Warp-compatible where possible)

| Action | Default |
| --- | --- |
| Command palette | Cmd+Shift+P (also Cmd+P) |
| New tab / close | Cmd+T / Cmd+W |
| Split right / down | Cmd+D / Cmd+Shift+D |
| Move between panes | Cmd+Opt+arrows |
| Previous / next tab | Cmd+Shift+[ / Cmd+Shift+] |
| Previous / next block | Cmd+↑ / Cmd+↓ |
| Copy block command / output | defined in the keymap; shown in the palette |
| Undo / redo in input | Cmd+Z / Cmd+Shift+Z |

All bindings are remappable in config, including chords.

### 7. Configuration

- **TOML file is the source of truth**, hot-reloaded on save.
- A **settings UI** edits the same file, preserving comments and ordering.
- Config, keymaps, themes and completion specs live under one directory
  (e.g. `~/.config/<name>/`) so they fit in a dotfiles repo.

### 8. Themes

- Built-in light and dark themes; import of common formats (iTerm2
  `.itermcolors`, base16) is desirable.
- Themes cover both the terminal palette and UI chrome.
- Optional background image/opacity/blur.

## Later (post-v1)

- **WASM plugins** that can:
  - add **sidebar panels** (git status, docker, k8s contexts, SSH hosts…)
  - **react to blocks** (read command, exit code, output; decorate the block
    or add actions to it)
  - Plugins run sandboxed with explicit permissions (filesystem, network,
    process spawn).
- Linux release.
- SSH integration: bootstrap shell integration on the remote host so blocks
  work over SSH. v1 falls back to classic mode inside SSH.
- Saved workflows/snippets as plain files.
- Session restore (tabs, splits, cwd) on relaunch.

## Non-functional requirements

The performance numbers below are **targets, not gates for the early
milestones**. They're measured during the performance pass, or sooner if
something feels slow in daily use (see
[ADR 0002](decisions/0002-ui-toolkit.md#revisit-if)).

| Area | Requirement |
| --- | --- |
| Input latency | Key-to-screen latency in the same range as Alacritty/Ghostty; benchmark against both and track in CI. |
| Throughput | `cat` of a 100 MB file must not freeze the UI; scrolling stays at display refresh rate. |
| Memory | Initial target: < 150 MB RSS for one window, one tab, 10k lines of scrollback. Revisit after the first prototype. |
| Startup | Window visible and shell prompt usable in < 300 ms on a modern Mac. |
| Correctness | Passes `vttest` core screens and a curated `esctest` subset; TUI checklist below is green. |
| Privacy | Zero network calls besides the update check, which can be disabled. No telemetry, no crash upload. |
| AI | None in core. The plugin API must not grant any plugin the ability to run on errors/typos without an explicit user action (no "auto-invoke" hooks that bypass user intent). |
| License | GPL-3.0. All dependencies must be GPL-3.0 compatible (MIT, Apache-2.0, BSD, MPL-2.0 are fine). |

## Acceptance checklist (TUI compatibility)

Every release must render and behave correctly with:

- [ ] vim / neovim (incl. Kitty keyboard protocol, true color, undercurl)
- [ ] tmux (nested, mouse mode)
- [ ] htop / btop
- [ ] less / man
- [ ] fzf (incl. inside the input editor flow, e.g. Ctrl+R widgets)
- [ ] lazygit
- [ ] Claude Code
- [ ] ssh into a host without integration (classic mode)
- [ ] zsh with oh-my-zsh / powerlevel10k, bash, fish, starship prompt
