# Architecture (draft)

High-level shape only. Concrete crate choices are proposals until the spikes in
[open-questions.md](open-questions.md) confirm them. Language rationale is in
[decisions/0001-implementation-language.md](decisions/0001-implementation-language.md).

## Components

```
┌─────────────────────────────── App (UI thread) ───────────────────────────────┐
│  Window / Workspace                                                           │
│   ├─ Tabs + vertical session list                                             │
│   ├─ Sidebar panels (file tree, later: plugin panels)                         │
│   ├─ Command palette                                                          │
│   └─ Panes (split tree)                                                       │
│        └─ TerminalView                                                        │
│             ├─ BlockList  ── renders blocks, sticky headers, block actions    │
│             ├─ InputEditor ── multi-line editor, highlighting, completions    │
│             └─ GridView   ── raw grid for full-screen apps / classic mode     │
├───────────────────────────────────────────────────────────────────────────────┤
│  Renderer (GPU): glyph atlas, text shaping, rects, images                     │
└───────────────────────────────────────────────────────────────────────────────┘
          ▲ snapshots / damage                     │ keystrokes, commands, resize
          │                                        ▼
┌───────────────────────────── Session (per pane) ──────────────────────────────┐
│  PTY  ──bytes──►  VT parser  ──►  Terminal state (grid, scrollback, modes)    │
│                        │                                                      │
│                        └─ shell-integration marks (OSC 133 + our own OSC)     │
│                              ──►  Block model (command, cwd, exit, timing)     │
│  Shell (zsh / bash / fish) started with injected integration scripts          │
└───────────────────────────────────────────────────────────────────────────────┘
          │
┌─────────┴──────── Services ───────────────────────────────────────────────────┐
│  Config (TOML, hot reload, comment-preserving writes)                         │
│  Keymap + action registry (feeds palette and keybindings)                     │
│  Completion engine (spec files + shell fallback)                              │
│  Theme loader                                                                 │
│  Plugin host (WASM, post-v1)                                                  │
└───────────────────────────────────────────────────────────────────────────────┘
```

### Proposed workspace crates

| Crate | Responsibility | Likely building blocks |
| --- | --- | --- |
| `term-core` | VT parsing, grid, scrollback, modes | `alacritty_terminal` to start (see open questions) |
| `term-pty` | Spawn shell, resize, read/write | `portable-pty` or `rustix` directly |
| `shell-integration` | zsh/bash/fish bootstrap scripts + parser for marks | plain shell scripts embedded at build time |
| `blocks` | Block model built from marks + grid ranges | own code |
| `input-editor` | Text buffer, cursors, undo, highlighting | `ropey` or similar, `tree-sitter-bash` for highlighting |
| `completions` | Spec loading, matching, generators, shell fallback | own code |
| `config` | TOML load/validate/write, hot reload | `serde`, `toml_edit`, `notify` |
| `ui` | Workspace, views, palette, sidebars | UI toolkit TBD (GPUI vs custom `wgpu`) |
| `plugin-host` | WASM runtime, WIT interface, permissions | `wasmtime` + component model |
| `app` | Binary: wires everything together | — |

## Threading

- One **PTY reader thread per session** reads bytes and feeds the parser, which
  mutates terminal state under a lock.
- The **UI thread** never blocks on the PTY. It takes a snapshot of visible state
  (and damage info) once per frame and renders.
- Under heavy output the parser coalesces work and the UI draws at most once per
  display refresh. This is what keeps `cat bigfile` from freezing the app.

## Blocks and shell integration

The terminal knows where each command starts and ends because the shell tells
it, using **OSC 133 semantic prompt marks** (the de-facto standard used by
iTerm2, WezTerm, Ghostty, Kitty and others) plus a private OSC for extra
metadata.

| Mark | Emitted by the shell when… | Used for |
| --- | --- | --- |
| `OSC 133;A` | prompt starts | block boundary |
| `OSC 133;B` | prompt ends, input begins | hand off to input editor |
| `OSC 133;C` | command is executed | output range starts, timer starts |
| `OSC 133;D;<exit>` | command finished | exit status, timer stops, block closes |
| private OSC | at precmd | cwd, git branch, shell name, history id… |

The integration scripts are injected when the shell starts (zsh via `ZDOTDIR`
redirect, bash via `--rcfile`/`PROMPT_COMMAND`, fish via `XDG_DATA_DIRS`
vendor conf), so users don't need to edit their dotfiles.

### Session mode state machine

```
             OSC 133;B                       OSC 133;C
  ┌────────┐ ─────────► ┌──────────────┐ ───────────► ┌──────────────┐
  │ Unknown│            │ Prompt        │              │ Running      │
  │(classic│ ◄───────── │ (InputEditor  │ ◄─────────── │ (keys go raw │
  │  mode) │  no marks  │  owns keys)   │  OSC 133;D   │  to the PTY) │
  └────────┘            └──────────────┘              └──────┬───────┘
                                                             │ alt screen on
                                                             ▼
                                                      ┌──────────────┐
                                                      │ FullScreen   │
                                                      │ (GridView,   │
                                                      │  no block    │
                                                      │  body)       │
                                                      └──────────────┘
```

- **Unknown / classic mode**: no marks seen, for example inside `ssh` to a host
  without integration. The pane behaves like a classic terminal: GridView, the
  shell owns the line. The UI chrome around it (tabs, palette, splits) still works.
- **Prompt**: the InputEditor owns the keyboard. On Enter, the full command text
  is written to the PTY. The shell executes it and history stays in the shell.
- **Running**: keystrokes go straight to the PTY so `read`, password prompts
  and REPLs work.
- **FullScreen**: alternate screen active. The pane shows the raw grid.

## Completion engine

1. Tokenize the current input (shell-aware quoting).
2. If a spec exists for the command, walk it: subcommands, options, args.
   Dynamic generators run a command (e.g. `git branch --format…`) with a
   timeout and cache.
3. Otherwise, ask the running shell for candidates through the integration
   channel (zsh `compadd` capture, bash `complete -p`/`compgen`, fish
   `complete -C`).
4. Merge, rank, and render in the menu with descriptions.

## Plugin host (post-v1, designed now)

- Runtime: **wasmtime** with the **WebAssembly component model**. The API is
  defined in **WIT**, so plugins can be written in Rust, Go (TinyGo), JS/TS
  (via componentize-js), C, and others.
- Capabilities v1 of the API:
  - register sidebar panels (declarative UI tree rendered by the host)
  - subscribe to block lifecycle events (`block-finished` with command, cwd, exit, output ref)
  - add block decorations and block actions
  - register palette commands
- Sandboxed by default. Plugins request permissions (read filesystem path,
  network host, spawn process) in a manifest, and the user approves them on install.
- Invariant: **no plugin action runs without a user gesture** unless the user
  explicitly enabled that automation. This keeps the "never auto-AI" promise
  even with third-party AI plugins.

## Configuration layout

```
~/.config/<name>/
  config.toml        # source of truth, edited by hand or by settings UI
  keymap.toml
  themes/*.toml
  completions/*.json # user specs override built-ins
  plugins/           # post-v1
```
