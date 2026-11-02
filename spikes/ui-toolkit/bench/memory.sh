#!/usr/bin/env bash
# Records phys_footprint for every process of an app. Run it after the memory
# scenario is set up (see README). Writes results/memory-<app>-<date>.txt.
#
#   bench/memory.sh <AppName> [run-number]
set -euo pipefail
source "$(dirname "$0")/lib.sh"

app=${1:?usage: memory.sh <AppName> [run-number]}
run=${2:-1}
out="$RESULTS_DIR/memory-$app-$(date +%Y%m%d).txt"

pids=$(app_pids "$app")
[[ -n "$pids" ]] || { echo "no running processes under /Applications/$app.app" >&2; exit 1; }

{
  echo "== run $run $(date +%H:%M:%S) =="
  hygiene_report
  for pid in $pids; do
    echo "-- footprint pid $pid:"
    footprint -p "$pid" 2>&1 | grep -iE "footprint|process" | head -5
  done
} | tee -a "$out"

echo "saved to $out (sum the 'Footprint' totals across the pids for the app total)"
