#!/usr/bin/env bash
# Time from `open -n` to the first window existing. This is a PROXY for
# "startup to first frame". The window appears before the first frame is drawn,
# and the osascript polling step adds roughly 100 ms of granularity.
# Treat the result as rough. Needs Accessibility permission for the terminal
# running this script, because it reads the window count through System Events.
#
#   bench/startup.sh <AppName> <SystemEvents process name> [runs]
set -euo pipefail
source "$(dirname "$0")/lib.sh"

app=${1:?usage: startup.sh <AppName> <process name> [runs]}
proc=${2:?usage: startup.sh <AppName> <process name> [runs]}
runs=${3:-3}
out="$RESULTS_DIR/startup-$app-$(date +%Y%m%d).txt"

windows() {
  osascript -e "tell application \"System Events\" to if exists process \"$proc\" then count windows of process \"$proc\"" 2>/dev/null || echo 0
}

{
  echo "== startup $app, $runs runs $(date +%H:%M:%S) =="
  hygiene_report
  for run in $(seq 1 "$runs"); do
    osascript -e "quit app \"$app\"" >/dev/null 2>&1 || true
    for _ in $(seq 1 100); do
      [[ -z "$(app_pids "$app")" ]] && break
      sleep 0.2
    done
    if [[ -n "$(app_pids "$app")" ]]; then
      echo "run $run: $app did not quit within 20 s. Quit it by hand and retry." >&2
      exit 1
    fi
    t0=$(python3 -c 'import time; print(time.time())')
    open -n -a "$app"
    waited=0
    until [[ "$(windows)" -ge 1 ]]; do
      waited=$((waited + 1))
      if (( waited > 300 )); then
        echo "run $run: no window within ~30 s. Check the process name for System Events." >&2
        exit 1
      fi
      sleep 0.02
    done
    t1=$(python3 -c 'import time; print(time.time())')
    echo "run $run: $(python3 -c "print(round(($t1 - $t0) * 1000))") ms (open -> first window)"
  done
} | tee -a "$out"
echo "saved to $out"
