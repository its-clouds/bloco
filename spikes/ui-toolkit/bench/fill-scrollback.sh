#!/usr/bin/env bash
# Run INSIDE the terminal under test, in each pane, for the memory scenario.
# Prints 10,000 numbered lines so the scrollback holds 10k lines.
seq -f 'scrollback line %06g: The quick brown fox jumps over the lazy dog' 1 10000
echo "done: 10000 lines printed"
