# Open questions

Each question lists the options and the smallest experiment (spike) that
should settle it. Settled questions graduate into an ADR under
[decisions/](decisions/).

## 1. UI toolkit

> **Resolved: GPUI.** See [ADR 0002](decisions/0002-ui-toolkit.md). The
> analysis below is kept for reference.

The biggest technical decision after the language.

| Option | Pros | Cons |
| --- | --- | --- |
| **GPUI** (Zed's framework, Apache-2.0) | Built for exactly this (editor and terminal in one GPU UI); Metal on macOS, Vulkan on Linux; Zed's terminal is a reference implementation | API still changes with Zed's needs; thin docs outside Zed; we'd track their releases |
| **Custom on `wgpu` + `winit`** | Full control, as Warp and Rio did; no upstream churn | Text input, IME, accessibility, layout and widgets all have to be built by us. Months of work before the first block shows up |
| **iced / Floem / egui** | Mature general-purpose toolkits | Not designed for a terminal grid with tens of thousands of glyphs per frame; risk on performance and on how text input feels |

**Spike:** planned in [spikes/0001-ui-toolkit.md](spikes/0001-ui-toolkit.md).
Both GPUI and custom `wgpu` build the same minimal terminal on a shared core
and are measured against gates fixed in advance.

## 2. VT core

> **Resolved for now: `alacritty_terminal`.** See
> [ADR 0002](decisions/0002-ui-toolkit.md). Revisit if per-block output storage
> (M3) doesn't fit its scrollback model.

`alacritty_terminal` (fast start, used by Zed) vs. our own grid on the `vte`
parser (needed if blocks require the grid to be split per block) vs.
`libghostty` through FFI (excellent correctness, but check its maturity and
license compatibility).
**Spike:** prototype blocks on top of `alacritty_terminal` and see whether
storing output per block fits its scrollback model.

## 3. Prompt rendering in the input editor

> **Resolved (2026-11-02): the shell's own prompt (PS1).** The editor starts
> after the shell's prompt, and keeps a local buffer until Enter. See
> [ADR 0003](decisions/0003-input-editor-at-the-prompt.md). The options are kept
> below for reference.

When the editor owns the input, whose prompt is displayed?

- Render the user's own `PS1` (powerlevel10k, starship…) above the editor.
  This is the most familiar option.
- Render our own prompt (cwd, git branch, etc.) from integration metadata.
- Let the user choose.

## 4. Completion spec format and source

- Format: JSON/TOML data specs (no code execution) vs. allowing generators as
  shell commands vs. generators as WASM.
- Source: write our own, or convert existing open specs (for example the
  MIT-licensed Fig autocomplete specs, after verifying the license and that
  they are maintained).

> **Accepted (M5, 2026-11-02).** Built on this branch.
>
> - **Format: TOML data, one file per command, no code.** A spec has a `name`,
>   a `description`, `option` tables (with `takes_value`) and `subcommand`
>   tables, nested for two levels (`git remote add`). Options of a parent apply
>   below it. Generators (for example "list branches") are not in v1: a word the
>   spec does not cover goes to the shell instead. WASM generators wait for the
>   plugin API (open question 5).
> - **Where specs live:** in the repo at `crates/input-editor/specs/*.toml`,
>   compiled in with `include_str!`. A user directory (`~/.config/bloco/completions/`)
>   that overrides them by command name is the next step, not in M5.
> - **Source:** written by hand for git, docker, kubectl, cargo and npm, covering
>   the subcommands and flags used most. The Fig specs are not converted yet: the
>   license and upkeep still need checking.
> - **Order:** the spec answers first. Then, for a word it does not cover, zsh's
>   own completion (below). Then file names in the shell's directory.
> - **zsh fallback:** Bloco starts `zsh -f` in a pseudo-terminal (`zpty`), types
>   the line and Tab, and reads the candidates from a wrapped `compadd`, so
>   nothing is drawn by zsh. It takes about 0.3 s and has a 3 s timeout. It runs
>   with `-f`, so the user's `~/.zshrc`, fpath and plugins are not loaded: their
>   extra completions (oh-my-zsh, Homebrew) are missing. Loading the user's rc
>   is slower and runs their code on each Tab, so it is a later choice. File names
>   are listed in Rust, because zsh's file candidates need its own line editor.
>   Paths (words with `/`, `~` or `.`) skip zsh.

## 5. Plugin licensing

Core is GPL-3.0. Do plugins that talk to the host through the WIT interface
have to be GPL too? Options:

- License the WIT interface and plugin SDK as MIT/Apache-2.0 and state that
  plugins using only that interface are separate works.
- Add a GPL "plugin exception" (similar to the GCC runtime exception).

Decide before the plugin API ships.

## 6. SSH

v1: classic mode inside SSH. Later: optional "bootstrap" that installs
integration scripts on the remote host temporarily (Warp's approach), or rely
on the user installing them. Privacy check: never copy anything to a remote
host without explicit consent.

## 7. Block output storage

Keeping all block outputs in memory vs. spilling old blocks to disk
(compressed) to meet the memory target with large histories.

## 8. Name

> **Resolved (2026-11-02): Bloco.** Portuguese for "block", and also the street
> parades of Brazilian Carnival. Repo `its-clouds/bloco`, app `Bloco.app`, binary
> `bloco`, bundle ID `io.github.its-clouds.bloco`, config in `~/.config/bloco/`.
> `bloco` was free on crates.io and Homebrew when checked. Search GitHub and the
> web for conflicts before the first public announcement.

## 9. Windows

Out of scope, but confirm each decision doesn't rule it out (ConPTY support in
the PTY crate, GPU backend through DX12/Vulkan).
