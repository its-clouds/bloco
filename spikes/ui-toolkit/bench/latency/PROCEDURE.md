# Latency procedure (keypress primary, Typometer cross-check)

These steps need Accessibility and Screen Recording permission, and they drive
real windows. Run them yourself. The scripts in this folder only summarize the
numbers.

## 0. Before every session

- Plug in the power adapter. Turn Low Power Mode off.
- Close other apps. Use the same display for every app.
- Check the refresh rate: `bench/check-tools.sh` and the `hygiene_report`
  output. Write it next to the results.
- Alacritty 0.17.0 sets `TERM=alacritty`, but this machine has no terminfo
  entry for it. Start Alacritty with `-o 'env.TERM="xterm-256color"'`, or set
  the same value in its config, so every app runs with the same TERM.
- Setup for each terminal (same for all apps):
  - One window, 120 x 40 cells, JetBrains Mono 14 pt, one plain theme (same
    foreground and background colors everywhere).
  - Cursor blink off.
  - In the window, run `zsh -f` and then `PS1='$ '`. This removes prompt and
    rc-file differences.

## 1. Grant permissions

System Settings > Privacy & Security:

1. **Accessibility**: add the terminal app you launch the tool from (Terminal,
   iTerm2, ...) and the tool itself if it asks.
2. **Screen Recording**: add the same apps. Restart the tool after granting.

## 2. keypress (primary)

keypress is the MIT-licensed tool at https://github.com/hackermanai/keypress. It
posts a synthetic `.` key, watches a small screen region and records the time
until the pixels change. Then it posts Backspace and does the same for the
disappear phase. Each iteration gives one appear and one disappear sample.

**Status: installed** at `~/.local/bin/keypress` (version 0.2.1). It was built
from `hackermanai/keypress` at commit `894bbef` with the command below. The
source is in the scratchpad. The binary's SHA-256 is
`0c7aa84025fb0584e482a562af3c5ba0bc8ba6ebea1718e05bd9fcbf1676cb10`.

    xcrun clang -O2 -mmacosx-version-min=14.0 -Wno-deprecated-declarations keypress.c -framework ApplicationServices -framework CoreFoundation -o keypress

Use the built binary, not the `curl | sh` installer. The keypress events go to
the focused window, so click the target pane before each run.

Commands (from the keypress README and source):

    export PATH="$HOME/.local/bin:$PATH"
    keypress --pick --count 100 --out bench/results/latency/<app>-idle-run<N>.csv

`--pick` asks you to click the caret or glyph position in the target pane. Use
`--x N --y N` to pass coordinates instead. `--count 100` gives 200 samples.

Procedure, per app:

1. Launch the app and open one window running `zsh -f`, then `PS1='$ '`.
2. **Idle run:** nothing else running. Run the command above. Save the CSV.
3. **Under-load run:** split the window vertically. In the left pane run `yes`.
   Click the caret position in the right pane, then run keypress there. Save the
   CSV as `<app>-load-run<N>.csv`. Stop `yes` with Ctrl-C afterwards.
4. Repeat steps 2 and 3 for runs 1, 2 and 3. Wait about 30 seconds between runs.
5. Summarize (keypress CSV rows with status `ok` are used):

       python3 bench/latency/stats.py bench/results/latency/<app>-idle-run*.csv

   Report the median-of-medians and the median-of-p95s. Use the same numbers
   for the results table.

## 3. Typometer (cross-check)

Typometer 1.0.1 is at https://github.com/pavelfatin/typometer (Apache-2.0). The
release jar is in `~/tools/typometer/typometer-1.0.1.jar`. Its SHA-256 is
`8afc176ef7b5b94021640be3089c7f7adbe94f0bdc5b15b84308e765fcb68d35`.

Java comes from Homebrew OpenJDK 27. It is keg-only, so put it on PATH first:

    export PATH="/opt/homebrew/opt/openjdk/bin:$PATH"
    java --add-opens java.desktop/java.awt=ALL-UNNAMED \
         --add-opens java.desktop/javax.swing=ALL-UNNAMED \
         --add-opens java.desktop/sun.awt=ALL-UNNAMED \
         -jar ~/tools/typometer/typometer-1.0.1.jar

The `--add-opens` flags are required on Java 17+. Without them, Typometer
crashes at startup with `InaccessibleObjectException` (it reflects into
`java.awt.Component`).

1. Set the same window and shell setup as in section 0.
2. Run 200 samples per run, 3 runs, idle and under load, as in section 2.
3. Typometer can import and export CSV. Export each run and check the columns.
   Keep the per-keypress latency column. Write it out as one number per line in
   `bench/results/latency/typometer-<app>-<scenario>-run<N>.txt`, which is the
   plain format `stats.py` reads.
4. Summarize with `bench/latency/stats.py` in the same way.

Typometer is a 2017 release and has not been updated since. Expect rough edges.
It also needs Accessibility and Screen Recording permission for Java, or the
terminal app that launches it.

If keypress and Typometer disagree by more than about 20%, record both numbers
and say so. Do not average them.

## 4. Recording

- Note the app version, refresh rate and any subjective notes in a separate
  file, not in the scores.
- Put the final numbers in the Results table of `docs/spikes/0001-ui-toolkit.md`
  only after the 3 runs are done.
