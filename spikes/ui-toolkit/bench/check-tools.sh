#!/usr/bin/env bash
# Reports which measurement tools are installed. Installs nothing.
source "$(dirname "$0")/lib.sh"

check() {
  local label=$1 found=$2
  if [[ -n "$found" ]]; then
    printf '%-14s %s\n' "$label" "$found"
  else
    printf '%-14s MISSING\n' "$label"
  fi
}

check "rustup" "$(command -v rustup || true)"
check "cargo" "$(command -v cargo || true)"
check "footprint" "$(command -v footprint || true)"
check "vtebench" "$(command -v vtebench || true)"
check "keypress" "$(command -v keypress || ls ~/.local/bin/keypress 2>/dev/null || true)"
check "java" "$(command -v java >/dev/null 2>&1 && java -version >/dev/null 2>&1 && command -v java || true)"
check "openjdk" "$(ls /opt/homebrew/opt/openjdk/bin/java 2>/dev/null || true)"
check "Typometer" "$(ls ~/tools/typometer/typometer-*.jar 2>/dev/null || true)"
for app in Alacritty Ghostty Warp iTerm; do
  check "$app.app" "$(ls -d "/Applications/$app.app" 2>/dev/null || true)"
done
