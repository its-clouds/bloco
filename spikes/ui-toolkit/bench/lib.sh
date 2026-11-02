# Shared helpers for the bench scripts. Sourced, not run directly.

BENCH_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DATA_DIR="$BENCH_DIR/data"
RESULTS_DIR="$BENCH_DIR/results"
mkdir -p "$DATA_DIR" "$RESULTS_DIR"

# Prints the machine and power state. Keep the output with every result.
hygiene_report() {
  echo "date: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "macos: $(sw_vers -productVersion) ($(sw_vers -buildVersion))"
  echo "chip: $(sysctl -n machdep.cpu.brand_string 2>/dev/null || echo unknown)"
  echo "memory: $(($(sysctl -n hw.memsize) / 1073741824)) GB"
  echo "power: $(pmset -g ps | head -1)"
  # pmset powermode: 1 means Low Power Mode is on.
  echo "pmset powermode: $(pmset -g | awk '/powermode/ {print $2}')"
  # Max refresh rate the main display supports. The rate in use can be lower.
  echo "display max refresh: $(osascript -l JavaScript -e 'ObjC.import("AppKit"); $.NSScreen.mainScreen.maximumFramesPerSecond' 2>/dev/null || echo unknown) Hz"
}

# Warns, but does not stop, when running on battery.
warn_if_on_battery() {
  if pmset -g ps | grep -q "Battery Power"; then
    echo "WARNING: on battery power. Plug in before measuring." >&2
  fi
}

# Prints the PIDs of processes whose executable lives in /Applications/<App>.app.
app_pids() {
  pgrep -f "/Applications/$1.app/Contents/MacOS/" || true
}
