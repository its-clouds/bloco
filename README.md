# Bloco

An open-source, GPU-accelerated terminal for macOS (Linux next) that brings the
block-based workflow popularized by Warp — command blocks, an editor-style input,
rich completions, vertical tabs, splits and sidebars — **without accounts, cloud
sync, telemetry, or AI that acts on its own.**

*Bloco* is Portuguese for "block". It's also the name of Brazil's Carnival
street parades.

> Status: early, pre-1.0, macOS only. M1–M6 of the [roadmap](docs/roadmap.md)
> are built: tabs and splits, a command palette, command blocks, a Warp-style
> input editor with completions, sidebars, a settings panel and themes. There
> are no releases yet; build from source.
> Code: [github.com/its-clouds/bloco](https://github.com/its-clouds/bloco).

## Build and run

```bash
cargo run -p app --release          # run from source
./scripts/bundle-macos.sh           # build target/release/bundle/Bloco.app
```

## Principles

1. **The terminal never acts on your behalf.** Nothing runs, opens, or "fixes"
   anything unless you pressed a key that asked for it.
2. **Fast and correct first.** Typing latency, scrolling and VT correctness
   (vim, tmux, htop, Claude Code…) beat any feature.
3. **Local and plain-text.** No login. Config, themes, keybindings and
   workflows are files you can keep in your dotfiles.
4. **Extensible through a sandbox.** Features beyond the core come as WASM
   plugins, not as patches to core.
5. **Free software.** GPL-3.0.

## Docs

| Doc | What it covers |
| --- | --- |
| [docs/vision.md](docs/vision.md) | Why this exists, who it's for, what we keep from Warp and what we drop |
| [docs/requirements.md](docs/requirements.md) | v1 scope, later scope, non-functional targets, acceptance checklist |
| [docs/architecture.md](docs/architecture.md) | High-level component design: PTY, VT core, blocks, input editor, UI, plugins |
| [docs/roadmap.md](docs/roadmap.md) | Milestones, from the first usable window to a feature-complete v1 |
| [docs/decisions/0001-implementation-language.md](docs/decisions/0001-implementation-language.md) | Why Rust (and what else was considered) |
| [docs/decisions/0002-ui-toolkit.md](docs/decisions/0002-ui-toolkit.md) | GPUI + `alacritty_terminal`, and when to revisit |
| [docs/open-questions.md](docs/open-questions.md) | Decisions still pending, with the spike that should settle each |
| [docs/spikes/0001-ui-toolkit.md](docs/spikes/0001-ui-toolkit.md) | GPUI vs custom `wgpu` spike plan (deferred after Phase 0; kept for later) |

## Security

Found a vulnerability? Please report it privately, not in a public issue. See
[SECURITY.md](SECURITY.md).

## License

GPL-3.0-only, see [LICENSE](LICENSE). Code adapted from Zed is listed in
[NOTICE](NOTICE). Licensing of the future plugin SDK is still open, see
[docs/open-questions.md](docs/open-questions.md).
