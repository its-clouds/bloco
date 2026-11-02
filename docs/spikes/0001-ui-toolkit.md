# Spike 0001: UI toolkit (GPUI vs custom `wgpu`)

> **Status: deferred after Phase 0 (2026-11-01).** The priority moved to
> getting a usable terminal quickly, so GPUI was chosen without the comparison.
> See [ADR 0002](../decisions/0002-ui-toolkit.md). Phase 0 is done: `spike-core`
> and the bench harness exist. This plan stays as is in case ADR 0002's
> "revisit if" conditions trigger it.

- **Settles:** [open question 1](../open-questions.md#1-ui-toolkit). Moves
  [ADR 0001](../decisions/0001-implementation-language.md) to *Accepted* and
  produces ADR 0002 (UI toolkit).
- **Timebox:** 14 focused days, hard stop. If time runs out, decide with the data gathered so far.
- **Output:** a results table in this doc, ADR 0002, and throwaway code under
  `spikes/ui-toolkit/`. None of the spike code becomes product code as is.

## Question

> Which toolkit lets us ship a terminal that meets the latency, memory and
> correctness dealbreakers **and** builds the app-like parts (blocks, input
> editor, sidebars) with the least long-term cost?

Rendering a grid fast is a solved problem for both options. The real difference is
**everything around the grid**: text input with IME, widgets and layout, and
how much upstream churn we inherit. The spike is designed to measure that.

## Candidates

| Arm | Stack | How we depend on it |
| --- | --- | --- |
| **A: GPUI** | Zed's GPUI (Metal on macOS) | No official standalone `gpui` crate exists yet. Options: git dependency on `zed-industries/zed` pinned to a commit, or a crates.io snapshot such as `gpui-pre`. The upstream docs describe it as pre-1.0 with frequent breaking changes. **Pin to one commit for the whole spike.** |
| **B: custom** | `winit` (window/input) + `wgpu` (GPU) + `cosmic-text` or `swash` (shaping/raster) | Regular crates.io dependencies |

Out of the spike: iced/Floem/egui. They aren't built for a terminal grid plus an
editor and would only dilute the comparison. Revisit only if both arms fail the
gates.

### Fairness rules

- Both arms use the **same shared core** (below): same VT core
  (`alacritty_terminal`), same PTY crate (`portable-pty`), same threading model.
  The only thing that varies is the UI layer.
- Same font (JetBrains Mono 14 pt), same theme colors, same window size,
  release builds, same machine.
- **Pass/fail gates are fixed before any arm is built** (this doc). No moving
  goalposts after seeing the results.

## Plan

### Phase 0: shared core and harness (2 days)

Crate `spike-core`:

- Spawns `zsh` in a PTY through `portable-pty` and handles resize.
- A reader thread feeds `alacritty_terminal`. It exposes
  `snapshot() -> visible cells + cursor + damage` and `write(bytes)`.
- A tiny zsh snippet that emits OSC 133 marks, so both arms can draw a block
  header from real data.
- An instrumentation hook that logs process start → first frame presented.

Harness (scripts in `spikes/ui-toolkit/bench/`):

- Latency: **keypress** (macOS, synthetic key + screen pixel watch) as the
  primary tool, with **Typometer** as a cross-check.
- Throughput: **vtebench**, plus `cat` of a 100 MB file.
- Memory: `footprint <pid>` (phys_footprint), with Activity Monitor as a sanity check.
- Frame rate: Metal Performance HUD (`MTL_HUD_ENABLED=1`).
- Run the same harness on the **baselines first**: Alacritty, Ghostty, Warp and
  iTerm2. That gives us numbers to compare against.

### Phase 1: Arm A, GPUI (5 days)

Phase 1 comes first because we expect it to be cheaper. Zed's own terminal is a
reference implementation. Zed's non-GPUI crates are GPL, which our GPL-3.0
license allows us to learn from or reuse. Verify each crate's `LICENSE` before
copying anything.

Minimum feature set (the same list for both arms):

1. Window that renders the grid: 256/true color, bold/italic/underline,
   cursor, wide chars, emoji fallback, box drawing.
2. Keyboard input, including **dead keys and IME** (`ação`, `você`, `ç`, plus
   Japanese through the macOS IME).
3. Window resize, which propagates to the PTY.
4. Scrollback with a smooth trackpad scroll.
5. **Two panes** (vertical split), with focus switching on Cmd+Opt+arrows.
6. Mouse selection and Cmd+C copy.
7. **A block header widget** (command, exit code, duration from the OSC 133
   marks) drawn above each command's output.
8. **A multi-line input widget** under the grid: mouse cursor placement,
   Opt+arrow word moves, Cmd+Z undo, IME. On Enter, it writes the text to the PTY.

Items 7–8 matter most. They are what the toolkit has to be good at.

### Phase 2: Arm B, `winit` + `wgpu` (5 days, strict)

Build the same list in the same order. If the timebox ends before the list is
done, **stop and record how far you got and an estimate for the rest**. Running
out of time is a result too: it shows what owning the toolkit costs.

### Phase 3: measure and decide (2 days)

Run the harness on both arms and the baselines, fill in the results table,
then write ADR 0002.

## Metrics

### Hard gates (failing any one disqualifies an arm)

| Gate | Threshold |
| --- | --- |
| Typing latency, idle shell | median ≤ 1.25× Alacritty's median; p95 ≤ 1.5× Alacritty's p95 (200 samples) |
| Typing latency while the other pane runs `yes` | UI never stalls more than one frame. Median stays within the gate above |
| `cat` 100 MB | Completes, and the window keeps responding (you can still type and scroll in the other pane) |
| Memory | < 150 MB phys_footprint with 1 window, 2 panes and 10k lines of scrollback |
| Idle CPU | ~0% with the cursor not blinking. No busy redraw loop |
| Input correctness | Dead keys, `ç` and Japanese IME all work in both the grid and the input widget |
| TUI smoke test | vim, htop, tmux, less and Claude Code render correctly |
| License | Every dependency is GPL-3.0 compatible |

### Scored criteria (when both arms pass the gates)

| Criterion | Weight | How it's measured |
| --- | --- | --- |
| Effort to build items 5–8 | 30% | Days spent and lines of code, per item |
| Upstream risk | 20% | For GPUI: breaking changes in the pinned window, and how hard bumping to the latest commit is (try it on day 5). For custom: amount of code we own |
| Text rendering quality | 15% | Side-by-side screenshots against Ghostty: AA, ligatures, emoji, box drawing |
| Performance margin | 15% | How far each arm is from the gates, not just whether it passes |
| Contributor experience | 10% | Clean and incremental build time, docs, error messages |
| Linux path | 10% | Builds and runs on Linux, if a machine is available. Otherwise judge from upstream status |

## Decision rule

- **Only one arm passes the gates:** pick it.
- **Both pass:** take the higher weighted score. As a tie-breaker, prefer GPUI if
  it took less than half of Arm B's effort on items 7–8. Prefer custom if GPUI
  needed patches or a fork to pass a gate.
- **Neither passes:** check whether the failure is in the shared core (VT core
  or threading) rather than the toolkit. If it is, spin off a VT core spike
  (open question 2, possibly evaluating `libghostty-vt`) before retrying.

## Measurement hygiene

- Plugged in, Low Power Mode off, other apps closed, same display.
- Record the display refresh rate (ProMotion at 120 Hz vs 60 Hz changes
  latency numbers), the macOS version and the hardware in the results.
- Run each measurement 3 times and report the median of the runs.

## Risks

| Risk | Mitigation |
| --- | --- |
| GPUI API breaks mid-spike | Pin one commit. The upgrade test on day 5 is part of the result |
| Software latency tools are noisy | Use the same tool and settings for every terminal. Compare relative numbers, not absolute |
| Author bias toward the arm that "feels" nicer | Gates are fixed beforehand. Record subjective notes separately from the scores |
| Arm B is unfinished at the timebox | Expected. Record the estimate to finish as data |

## Results (fill in during Phase 3)

| Metric | Alacritty | Ghostty | Warp | iTerm2 | Arm A (GPUI) | Arm B (wgpu) |
| --- | --- | --- | --- | --- | --- | --- |
| Latency median / p95 (ms) | | | | | | |
| Latency under load median (ms) | | | | | | |
| vtebench (summary) | | | | | | |
| `cat` 100 MB (s), responsive? | | | | | | |
| Memory, 2 panes + 10k lines (MB) | | | | | | |
| Startup to first frame (ms) | | | | | | |
| Idle CPU (%) | | | | | | |
| Items 1–8 done / days spent | — | — | — | — | | |

Phase 0 status: no baseline cells are filled yet. The baseline measurements
need GUI windows and macOS permissions, so they are pending a run by hand. See
[`spikes/ui-toolkit/bench/README.md`](../../spikes/ui-toolkit/bench/README.md).
