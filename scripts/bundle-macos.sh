#!/usr/bin/env bash
# Builds the release binary and wraps it in a macOS app bundle:
#
#   target/release/bundle/Bloco.app
#
# The bundle is unsigned, apart from an ad-hoc signature (no developer identity).
# It uses only tools that ship with macOS (sips, iconutil, codesign) and python3.
# Copy it to /Applications with:
#
#   cp -R target/release/bundle/Bloco.app /Applications/
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

version="$(sed -n 's/^version = "\(.*\)"/\1/p' crates/app/Cargo.toml | head -n 1)"
bundle="target/release/bundle/Bloco.app"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

cargo build --release -p app

rm -rf "$bundle"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
cp target/release/bloco "$bundle/Contents/MacOS/bloco"

cat > "$bundle/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key>
  <string>Bloco</string>
  <key>CFBundleDisplayName</key>
  <string>Bloco</string>
  <key>CFBundleIdentifier</key>
  <string>io.github.its-clouds.bloco</string>
  <key>CFBundleExecutable</key>
  <string>bloco</string>
  <key>CFBundleIconFile</key>
  <string>AppIcon</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>${version}</string>
  <key>CFBundleVersion</key>
  <string>${version}</string>
  <key>LSMinimumSystemVersion</key>
  <string>12.0</string>
  <key>LSApplicationCategoryType</key>
  <string>public.app-category.utilities</string>
  <key>NSHighResolutionCapable</key>
  <true/>
  <key>NSPrincipalClass</key>
  <string>NSApplication</string>
</dict>
</plist>
PLIST

# Placeholder icon: a dark square with a light ">_" prompt mark, drawn with plain Python.
python3 - "$work/icon-1024.png" <<'PY'
import struct, sys, zlib

SIZE = 1024
BG, FG = (0x1d, 0x1f, 0x21), (0xc5, 0xc8, 0xc6)

def inside_chevron(x, y):
    # A ">" stroke from (300, 330) to (520, 512) to (300, 694), 56 px thick.
    for (ax, ay), (bx, by) in (((300, 330), (520, 512)), ((520, 512), (300, 694))):
        dx, dy = bx - ax, by - ay
        t = max(0.0, min(1.0, ((x - ax) * dx + (y - ay) * dy) / (dx * dx + dy * dy)))
        px, py = ax + t * dx, ay + t * dy
        if (x - px) ** 2 + (y - py) ** 2 <= 28 ** 2:
            return True
    return False

def inside_cursor(x, y):
    return 580 <= x <= 720 and 620 <= y <= 676

rows = []
for y in range(SIZE):
    row = bytearray([0])  # PNG filter type: none
    for x in range(SIZE):
        color = FG if inside_chevron(x, y) or inside_cursor(x, y) else BG
        row.extend(color)
    rows.append(bytes(row))

def chunk(tag, data):
    return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)

png = b"\x89PNG\r\n\x1a\n"
png += chunk(b"IHDR", struct.pack(">IIBBBBB", SIZE, SIZE, 8, 2, 0, 0, 0))
png += chunk(b"IDAT", zlib.compress(b"".join(rows), 9))
png += chunk(b"IEND", b"")
open(sys.argv[1], "wb").write(png)
PY

iconset="$work/AppIcon.iconset"
mkdir -p "$iconset"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" "$work/icon-1024.png" --out "$iconset/icon_${size}x${size}.png" >/dev/null
  sips -z "$((size * 2))" "$((size * 2))" "$work/icon-1024.png" --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$bundle/Contents/Resources/AppIcon.icns"

codesign --force --sign - "$bundle" >/dev/null 2>&1 || echo "note: ad-hoc signing failed; the bundle is left unsigned"

echo "Built $bundle (version $version)"
