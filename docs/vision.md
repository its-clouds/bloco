# Vision

## The problem

Modern terminals split into two camps:

- **Classic, fast, plain** — iTerm2, Alacritty, Ghostty, WezTerm, Kitty. Great
  emulators, but output is one undifferentiated stream of text, the prompt is a
  1980s line editor, and the UI around it is minimal.
- **Modern, block-based, app-like** — Warp. Commands and their output become
  *blocks*, the input is a real text editor, completions are rich, and the
  workspace has vertical tabs and sidebars. But it requires an account, syncs
  to a cloud, and increasingly pushes AI into the core loop: when a script
  fails it offers to fix it, and when a command is mistyped it opens an AI
  session on its own.

There is no open-source terminal that delivers the second experience with the
values of the first.

## What we're building

A community-driven, GPL-licensed terminal that keeps Warp's ergonomics and drops
its intrusions.

### Keep (what the author likes in Warp today)

- **Keybindings** — command palette, block navigation, pane/tab keys,
  editor-style input editing.
- **How things look** — clear per-command blocks with exit status and timing,
  sticky command header, polished app-like chrome, rich completion menus,
  good and switchable themes.
- **Easy to extend** — adding features should feel like installing an
  extension, not forking the terminal.
- **Sidebars and layout** — vertical tabs / session list, project file tree,
  split panes.

### Drop

- AI in the core. No auto-triggered "fix this error" or "did you mean"
  sessions. AI can exist only as a third-party plugin that the user installs
  and invokes explicitly.
- Accounts, cloud sync, team drives.
- Telemetry of any kind. The only network call is an update check that can be
  disabled.

## Who it's for

1. **Primary:** developers who moved from iTerm2/zsh to Warp for the UX and want
   that UX in a free, local, private tool. (The author is one of them.)
2. **Secondary:** terminal power users who want blocks and a modern UI but will
   not give up vim/tmux/TUI correctness or low memory use.

## Goal

A **community open-source product**, not a personal toy: from day one it should
have contributor docs, reproducible builds, packaging (Homebrew first), and a
plugin story that lets people add features without touching core.

## Non-goals

- Built-in AI features of any kind.
- User accounts, hosted services, or collaboration features.
- Being a shell. We integrate with zsh, bash and fish; we don't replace them.
- Being a multiplexer replacement for remote sessions (tmux still works inside).
- Windows support in the near term (architecture should not rule it out).

## Dealbreakers (what would make users drop it)

- Typing latency or laggy scrolling on large output.
- Any TUI (vim/nvim, tmux, htop/btop, less, fzf, lazygit, Claude Code)
  rendering incorrectly.
- Electron-level memory use per window.

SSH degrading to "classic mode" is acceptable for v1 (see
[requirements.md](requirements.md)).
