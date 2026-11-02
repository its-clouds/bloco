# Bench harness (UI toolkit spike, Phase 0)

Scripts that make the spike's measurements repeatable on macOS. Some steps
need GUI interaction and macOS permissions. Those steps are written as
procedures for a person to run. The scripts in this folder cover the rest.

Raw output goes to `results/` (committed). Generated data goes to `data/`
(ignored by `.gitignore`).

## Tool status

Run `bench/check-tools.sh` for the current state. It installs nothing.

Status after the install pass on 2026-11-01:

- Installed: `vtebench` 0.3.1 (cargo, benchmark files copied to
  `vtebench/benchmarks`), Ghostty 1.4.0 (Homebrew cask), Alacritty 0.17.0 (built
  from crates.io and wrapped in `/Applications/Alacritty.app`), OpenJDK 27
  (Homebrew formula, keg-only, so put `/opt/homebrew/opt/openjdk/bin` on PATH),
  Typometer 1.0.1 jar in `~/tools/typometer/`.
- Installed: keypress 0.2.1 at `~/.local/bin/keypress`, built from the reviewed
  source at commit `894bbef`. Nothing has been run with it yet.
- Already present: cargo, rustup, `footprint`, Warp, iTerm2.
- Alacritty's Homebrew cask is disabled (fails Gatekeeper), so it was built from
  source instead. The bundle is unsigned.
- Nothing has been measured yet. Measurements need GUI windows and permissions.
- The machine was on battery during setup, so none of the numbers from this
  session are usable. Measure with the power adapter connected.

## Measurement hygiene (from the spike doc)

- Plugged in, Low Power Mode off, other apps closed, same display.
- Record the display refresh rate (ProMotion at 120 Hz vs 60 Hz changes latency
  numbers), the macOS version and the hardware. `lib.sh` `hygiene_report`
  prints all of these. Every script writes it into its results file.
  `display max refresh` is the highest rate the screen supports. The rate in
  use can be lower, so note it if you see it differ.
- Run each measurement 3 times and report the median.

## Baseline and arm setup

Every app gets the same setup, so the numbers are comparable:

- One window, 120 x 40 cells, JetBrains Mono 14 pt, one plain theme with the
  same colors everywhere.
- `zsh -f` with `PS1='$ '` in each window. Disable cursor blink.
- Release builds for the spike arms. Installed app bundles for the baselines.
- Warp has its own input editor. Use the same `zsh -f` shell there too, and
  note that Warp's editor is part of the product.

## Scripts

| Script | Where it runs | What it does |
| --- | --- | --- |
| `check-tools.sh` | any terminal | Lists which measurement tools are installed |
| `gen-data.sh` | any terminal | Writes `data/100mb.txt` (100 MiB of text) |
| `throughput.sh` | **inside the terminal under test** | Times `cat` of the 100 MiB file. Runs `vtebench` if installed |
| `fill-scrollback.sh` | **inside the terminal under test** | Prints 10,000 lines for the memory scenario |
| `memory.sh <App>` | any terminal | `footprint` on every process under `/Applications/<App>.app` |
| `idle-cpu.sh <App> [s]` | any terminal | Samples CPU% of the app while idle |
| `startup.sh <App> <proc>` | any terminal | Times `open -n` to the first window. Rough proxy, see below |
| `latency/stats.py` | any terminal | Median and p95 per run, plus median-of-runs |
| `latency/PROCEDURE.md` | you, by hand | keypress and Typometer steps |

`lib.sh` is sourced by the scripts. Do not run it.

## Scenarios

### Throughput

1. Run `bench/gen-data.sh` once.
2. In one pane of the app under test, run:

       bench/throughput.sh <AppName> <run-number>

3. Repeat for runs 1 to 3. Record the `real` time.
4. Responsiveness: while `cat` runs in one pane, type and scroll in the other
   pane. Record yes or no. This is manual.

Set `VTEBENCH_BENCHMARKS` and `VTEBENCH_ARGS` if vtebench needs them.

### Memory (1 window, 2 panes, 10k lines of scrollback)

1. Open one window. Split it vertically (two panes).
2. In each pane, run `bench/fill-scrollback.sh` and wait for `done`.
3. Run `bench/memory.sh <AppName> <run-number>`. Repeat 3 times on the same
   state. Sum the footprint across the app's processes. Report the median.

Footprint is the `phys_footprint` value that `footprint` reports.

### Startup and idle CPU

- Startup: `bench/startup.sh <AppName> <process name in System Events> 3`.
  The script needs Accessibility permission. It measures `open -n` to the
  first window. The window appears before the first frame, and the polling step
  adds about 100 ms of granularity. So it is only a rough proxy for "startup to
  first frame". Record it as a proxy in the results.
- Idle CPU: with one window and an idle prompt, run
  `bench/idle-cpu.sh <AppName> 30 <run-number>`. Repeat 3 times.

### Latency

See `latency/PROCEDURE.md`.
