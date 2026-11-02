#!/usr/bin/env bash
# Run INSIDE the terminal under test. Times `cat` of the 100 MiB file and runs
# vtebench if it is installed. Results go to results/throughput-<app>-<date>.txt.
#
#   bench/throughput.sh <AppName> [run-number]
#
# VTEBENCH_BENCHMARKS defaults to bench/vtebench/benchmarks (copied from the
# vtebench 0.3.1 crate). VTEBENCH_ARGS takes any extra flags.
set -euo pipefail
source "$(dirname "$0")/lib.sh"

app=${1:?usage: throughput.sh <AppName> [run-number]}
run=${2:-1}
out="$RESULTS_DIR/throughput-$app-$(date +%Y%m%d).txt"
data="$DATA_DIR/100mb.txt"

[[ -f "$data" ]] || "$BENCH_DIR/gen-data.sh"

# The output being measured must go to the terminal itself, never to a pipe or
# /dev/null, or the terminal is not part of the measurement. So only the
# timings are written to $out.
{
  echo "== run $run $(date +%H:%M:%S) =="
  hygiene_report
} >> "$out"

clear
TIMEFORMAT=%R
secs=$( { time cat "$data" > /dev/tty; } 2>&1 )
echo "-- cat 100 MiB to the terminal (wall clock, seconds): real $secs" >> "$out"

if command -v vtebench >/dev/null 2>&1; then
  dat="$RESULTS_DIR/vtebench-$app-run$run-$(date +%Y%m%d-%H%M%S).dat"
  clear
  # shellcheck disable=SC2086
  vtebench -b "${VTEBENCH_BENCHMARKS:-$BENCH_DIR/vtebench/benchmarks}" --dat "$dat" ${VTEBENCH_ARGS:-}
  echo "-- vtebench: results in $(basename "$dat")" >> "$out"
else
  echo "-- vtebench: not installed, skipped" >> "$out"
fi

echo
echo "cat 100 MiB: ${secs}s"
echo "saved to $out"
