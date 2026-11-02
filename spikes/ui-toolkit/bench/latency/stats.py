#!/usr/bin/env python3
"""Summarizes latency samples. One file per run.

Accepted inputs:
  - a .csv written by keypress (--out): rows with status "ok" are used, from the
    latency_ms column. Appear and disappear phases both count, so 100 keypress
    iterations give 200 samples.
  - any other file: one latency number (ms) per line.

    python3 bench/latency/stats.py runs/alacritty-run1.csv runs/alacritty-run2.csv runs/alacritty-run3.csv

Per run: sample count, median and p95 (nearest rank). Then the median of the
per-run medians and the median of the per-run p95s, which is the number the
spike doc asks for.
"""

import csv
import math
import statistics
import sys


def p95(samples):
    # Nearest-rank: the smallest value with at least 95% of samples at or below it.
    rank = math.ceil(0.95 * len(samples))
    return samples[rank - 1]


def load(path):
    if path.endswith(".csv"):
        with open(path, newline="") as f:
            values = [float(row["latency_ms"]) for row in csv.DictReader(f) if row["status"] == "ok"]
    else:
        with open(path) as f:
            values = [float(line) for line in f if line.strip()]
    if not values:
        sys.exit(f"{path}: no samples")
    return sorted(values)


def main(paths):
    if not paths:
        sys.exit(__doc__)
    medians, p95s = [], []
    for path in paths:
        samples = load(path)
        med, tail = statistics.median(samples), p95(samples)
        medians.append(med)
        p95s.append(tail)
        print(f"{path}: n={len(samples)} median={med:.2f} ms p95={tail:.2f} ms")
    print(f"runs={len(paths)} median-of-medians={statistics.median(medians):.2f} ms "
          f"median-of-p95s={statistics.median(p95s):.2f} ms")


if __name__ == "__main__":
    main(sys.argv[1:])
