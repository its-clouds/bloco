# ADR 0001 — Implementation language

- **Status:** Accepted (2026-11-01, together with [ADR 0002](0002-ui-toolkit.md))
- **Date:** 2026-10-31

## Context

What the language has to support, taken from [requirements.md](../requirements.md):

- A **custom, GPU-rendered UI** (blocks, input editor, sidebars, palette).
  Native widget toolkits can't draw blocks and an editor this way.
- **Low latency and low memory**, which are listed dealbreakers. This rules out
  shipping a browser engine per window.
- **macOS first, Linux soon**, so one codebase has to cover both.
- **WASM plugin host**, so the language needs a mature WASM runtime that embeds well.
- **Community OSS**, so the contributor pool and the existing terminal
  ecosystem matter.
- The author is comfortable in **Rust, Go, TypeScript/JS, C, Java and Python**.

## Options considered

| | Rust | Zig | Go | TypeScript + Electron | Swift | C/C++ |
| --- | --- | --- | --- | --- | --- | --- |
| Latency / memory | Excellent | Excellent | Good (GC) | Poor (dealbreaker) | Excellent | Excellent |
| Custom GPU UI ecosystem | Strong: GPUI (Zed), wgpu, cosmic-text | Thin (Ghostty uses native UI per OS) | Weak (Gio, Fyne) | Strong (DOM) | macOS only | Do-it-yourself |
| macOS + Linux from one codebase | Yes | Yes, with two UI frontends | Yes, but weak UI | Yes | No | Yes |
| Terminal ecosystem | Alacritty, WezTerm, Rio, Zed's terminal, Warp itself | Ghostty | Wave (Go backend + Electron) | xterm.js, Hyper, Tabby | — | Kitty (C + Python), Konsole |
| WASM host | wasmtime (reference runtime, component model) | Bindings only | wazero (good) | Native in V8 | Bindings only | wasmtime C API |
| Author fluency | Yes | No | Yes | Yes | No | Yes (C) |
| Contributor pool for this kind of app | Large | Small, growing | Medium | Large | Small | Shrinking |
| Memory safety | Yes | Partial | Yes | Yes | Yes | No |

### Why not the others

- **TypeScript/Electron:** fails the memory and latency dealbreakers (Hyper and
  Tabby are the evidence). Off the table.
- **Go:** pleasant and the author knows it, but there's no production-grade GPU
  text/UI stack. We would end up writing a renderer from scratch in a language
  with a GC and weaker SIMD/FFI ergonomics, or falling back to Electron (as
  Wave does).
- **Zig:** Ghostty shows it can be done brilliantly. But the language is pre-1.0,
  the author doesn't know it, and Ghostty's approach (native Swift UI on macOS,
  GTK on Linux) means writing two UIs, which is the opposite of what a small
  OSS team needs. Ghostty's core is being split out as an embeddable C-ABI
  library (`libghostty`); that's worth watching as a possible VT core
  *consumed from Rust*.
- **Swift:** best native macOS feel, but no realistic Linux GUI story.
- **C/C++:** possible, but no memory safety, and contributors expect more from
  a new project in 2026.

## Decision

**Rust.**

1. **Proven exact stack.** Zed's terminal panel is GPUI plus `alacritty_terminal`,
   in production. Warp itself is written in Rust. We'd be standing on known-good
   ground rather than inventing it.
2. **Every hard piece already has a crate:** VT core (`alacritty_terminal`,
   `vte`), PTY (`portable-pty`), text shaping and rasterizing (`cosmic-text`,
   `swash`, or the toolkit's own), GPU (`wgpu`/Metal), WASM (`wasmtime`, the
   reference component-model runtime), config (`toml_edit` preserves comments
   for the settings UI).
3. **Meets the dealbreakers:** no GC pauses, predictable memory, easy SIMD.
4. **The author already knows it**, and it has the biggest pool of people who
   want to contribute to a terminal.
5. **License fit:** the relevant crates are MIT/Apache-2.0, which are
   compatible with GPL-3.0.

## Consequences

- Slower iteration than TS for UI work. To soften that, keep the UI layer thin
  and data-driven, and pick a toolkit with good hot-reload or fast incremental
  builds.
- The **UI toolkit** was the biggest follow-up decision. It was settled in
  [ADR 0002](0002-ui-toolkit.md): GPUI.
- Rust compile times affect contributors. Use a Cargo workspace with small
  crates, `sccache` in CI and `mold`/`lld` on Linux.
