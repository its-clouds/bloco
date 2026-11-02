#!/usr/bin/env bash
# Samples CPU% of an app's processes while it sits idle. Put the terminal in
# the scenario state (one window, idle prompt, cursor blink off) before running.
#
#   bench/idle-cpu.sh <AppName> [seconds] [run-number]
#
# `top -l` takes one sample per interval. The first sample covers the whole
# process lifetime, so it is dropped and the rest are averaged.
set -euo pipefail
source "$(dirname "$0")/lib.sh"

app=${1:?usage: idle-cpu.sh <AppName> [seconds] [run-number]}
seconds=${2:-30}
run=${3:-1}
out="$RESULTS_DIR/idle-cpu-$app-$(date +%Y%m%d).txt"
interval=5
samples=$(( seconds / interval + 1 ))

pids=$(app_pids "$app" | paste -sd, -)
[[ -n "$pids" ]] || { echo "no running processes under /Applications/$app.app" >&2; exit 1; }

sleep 10 # let the window settle after launch
values=$(top -l "$samples" -s "$interval" -stats pid,cpu | awk -v pids="$pids" '
  BEGIN { n = split(pids, p, ","); for (i = 1; i <= n; i++) want[p[i]] = 1 }
  /^PID/ { block++; next }
  block > 1 && ($1 in want) { total[block] += $2 }
  END { for (b = 2; b <= block; b++) printf "%.2f\n", total[b] }')
avg=$(awk '{ s += $1; n++ } END { if (n) printf "%.2f", s / n }' <<<"$values")

{
  echo "== run $run $(date +%H:%M:%S), ${seconds}s idle =="
  hygiene_report
  echo "per-sample CPU% (all app processes): $(paste -sd' ' - <<<"$values")"
  echo "mean idle CPU%: $avg"
} | tee -a "$out"
echo "saved to $out"
