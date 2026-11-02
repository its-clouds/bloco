#!/usr/bin/env bash
# Writes bench/data/100mb.txt: 100 MiB of printable text lines, for `cat` timing.
source "$(dirname "$0")/lib.sh"

out="$DATA_DIR/100mb.txt"
yes 'The quick brown fox jumps over the lazy dog 0123456789 abcdefghijklmnopqrstuvwxyz' \
  | head -c 104857600 > "$out"
echo "wrote $out ($(stat -f %z "$out") bytes)"
