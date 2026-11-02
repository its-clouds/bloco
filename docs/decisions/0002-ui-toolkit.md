# ADR 0002 — UI toolkit and VT core

- **Status:** Accepted
- **Date:** 2026-11-01
- **Supersedes:** the comparison planned in [spike 0001](../spikes/0001-ui-toolkit.md), now deferred

## Context

The [spike](../spikes/0001-ui-toolkit.md) planned to build the same minimal
terminal twice, once on GPUI and once on a custom `winit` + `wgpu` stack, and
benchmark both against Alacritty, Ghostty, Warp and iTerm2 before choosing.

After Phase 0 (shared core done, benchmark harness written), the priority changed.
**Getting a terminal the author can use every day comes first.** Performance is
investigated if and when it becomes noticeable in real use. Building two
throwaway UIs and running a long manual benchmark session doesn't serve that
goal.

## Decision

1. **UI toolkit: GPUI** (Zed's UI framework, Apache-2.0).
2. **VT core: `alacritty_terminal`**, driven through the `spike-core` session
   layer (PTY via `portable-pty`, reader thread, snapshots, OSC 133 blocks).
   This resolves open questions 1 and 2 for now.

### Why GPUI without the comparison

- **Fastest route to a usable app.** Zed's terminal panel is GPUI plus
  `alacritty_terminal` in production. That's a working reference for exactly
  what we're building, including text input with IME, layout, splits and
  keybindings. A custom `wgpu` stack would need all of that written from
  scratch before the first day of use.
- **Built for this kind of UI.** GPUI is designed for an editor plus a terminal
  with GPU rendering. That matches the blocks, the input editor and the sidebars.
- **We can reuse Zed's code.** Zed's non-GPUI crates are GPL. Our GPL-3.0
  license lets us adapt them (for example `terminal` and `terminal_view`),
  provided we check each crate's `LICENSE` file and keep the attribution.

## Consequences

- **Dependency:** there's no official standalone `gpui` crate. Depend on
  `zed-industries/zed` through git **pinned to one commit**, and bump it on
  purpose, not by accident. A crates.io snapshot such as `gpui-pre` is a
  fallback if the git dependency is too heavy to build.
- **Upstream churn:** GPUI is pre-1.0 and its API breaks often. Keep our GPUI
  code in the `ui`/`app` crates so the session, blocks and config crates don't
  depend on it.
- **Performance is not measured up front.** The harness in
  `spikes/ui-toolkit/bench/` stays in the repo for when it's needed.
- `spike-core` becomes the starting point for the product's session crate
  instead of being thrown away (see [roadmap](../roadmap.md)).

## Revisit if

- Typing feels laggy, scrolling stutters, or memory use is noticeably high in
  daily use. Then run the bench harness against Alacritty and Ghostty.
- Bumping the GPUI commit regularly costs more than a day of fixes, or we need
  to fork GPUI to work around a limitation.
- Linux support turns out to be blocked by GPUI.

In any of these cases, resume spike 0001 Phase 2 (custom `wgpu`) using the
same `spike-core` layer.
