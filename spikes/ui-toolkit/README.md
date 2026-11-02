# UI toolkit spike (throwaway)

Code for [spike 0001](../../docs/spikes/0001-ui-toolkit.md). None of this is
product code. It is kept clean and small so the next phases can reuse it.

## Layout

| Path | What it is |
| --- | --- |
| `spike-core/` | Shared core (Phase 0 Part B). PTY + `alacritty_terminal` + OSC 133 blocks + startup timing |
| `spike-core/shell/integration.zsh` | zsh snippet that emits OSC 133 A/B/C/D marks. Embedded in the crate |
| `spike-core/examples/headless.rs` | Runs `echo hi; false` headless, prints the grid and the block records |
| `bench/` | Measurement harness (Phase 0 Part A). See [bench/README.md](bench/README.md) |

Phase 1 (GPUI) and Phase 2 (winit + wgpu) are not started.

## Build, test, run

```bash
cargo build -p spike-core --all-targets
cargo test -p spike-core
cargo run -p spike-core --example headless
```

The headless example uses your own `~/.zshrc`. The generated `ZDOTDIR` sources
it first, then the OSC 133 snippet. Your shell startup time is part of the first
frame (see the notes below).

## spike-core API

- `Session::spawn(cols, rows)`: starts `zsh` in a PTY.
- `Session::write(bytes)`: sends bytes to the shell.
- `Session::resize(cols, rows)`: resizes the PTY and the grid.
- `Session::snapshot()`: visible cells, cursor and damage since the last
  snapshot. Taking a snapshot resets damage, so use one consumer.
- `Session::blocks()`: command blocks (command text, exit code, start and end
  time, duration).
- `StartupTimer::start()` in `main`, then `first_frame_presented()` from the UI
  after the first frame. It logs process start to first frame once.

OSC 133 parsing is in `osc133.rs`. It runs on the raw bytes before the VT parser.
Marks survive any split across PTY reads (the tests check every split point).

## Notes from Phase 0

- The user's own `zsh -i -c exit` takes about 2.7 s on this machine (measured
  with `time`). The headless example's "first frame" of 3.2 s is mostly that.
  `zsh -f` takes about 0.03 s. Measure the shell separately from the terminal
  when comparing startup numbers.
- The `PS1` B mark is appended once. A prompt theme that rewrites `PS1` after
  the snippet loads will drop it. For the spike that is acceptable. Block
  headers will be missing in that case.

## Dependency licenses

The project is GPL-3.0. Every dependency must be GPL-3.0 compatible. The four
licenses in the spike doc are MIT, Apache-2.0, BSD and MPL-2.0.

Source: `cargo metadata --locked`, 74 packages besides `spike-core`. Versions
are in `Cargo.lock`. Several crates are dual-licensed. We take the MIT or
Apache-2.0 option, which is compatible.

| License (as declared) | Crates |
| --- | --- |
| `Apache-2.0` | alacritty_terminal |
| `Apache-2.0 OR MIT` | atomic-waker, concurrent-queue, fastrand, pin-project-lite, polling, vte |
| `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | linux-raw-sys, rustix, rustix-openpty |
| `Apache-2.0/MIT` | shared_library, signal-hook |
| `BSD-2-Clause OR Apache-2.0` | serial2 |
| `MIT` | cfg_aliases, filedescriptor, nix, portable-pty, redox_syscall, winreg |
| `MIT OR Apache-2.0` | anyhow, arrayvec, base64, bitflags, cfg-if, crossbeam-utils, errno, futures-io, hermit-abi, home, lazy_static, libc, lock_api, log, miow, parking_lot, parking_lot_core, piper, proc-macro2, quote, regex-automata, regex-syntax, scopeguard, serde, serde_core, serde_derive, signal-hook-registry, smallvec, syn, thiserror, thiserror-impl, unicode-width, windows-link, windows-sys, windows-targets, windows_* (Windows-only target crates, not built on macOS) |
| `MIT OR Apache-2.0 OR Zlib` | cursor-icon (use MIT) |
| `MIT/Apache-2.0` | bitflags 1.x, downcast-rs, shell-words, winapi and its gnu shims |
| `Unlicense OR MIT` | aho-corasick, memchr (use MIT) |
| `(MIT OR Apache-2.0) AND Unicode-3.0` | unicode-ident |

**Needs a decision:** `unicode-ident` carries the **Unicode-3.0** license for its
Unicode data tables. That license is not one of the four in the spike doc. It is
a permissive license. I have not confirmed its GPL-3.0 compatibility against the
FSF list. It comes in through `proc-macro2`, used by the proc-macro crates
`serde_derive` and `thiserror-impl`. Proc macros run at compile time and are not
linked into the binary. Before the spike turns into product code, check it, or
find another route.
