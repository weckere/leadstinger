#!/usr/bin/env bash
# Regenerate LeadStinger's icons from docs/design/logo.py (a cassette, D-257; it
# replaced D-094's cart), from the repo root.
# Needs python3 and figcraft-cli; the .icns step needs macOS (iconutil). Writes:
#   assets/linux/leadstinger.svg          the launcher icon, scalable
#   assets/icons/leadstinger-N.png        the same at N = 16 32 64 128 256 512 px; 16 and
#                                         32 come from the simplified small twin. The
#                                         window icon is the 256 (theme::app_icon).
#   assets/icons/leadstinger-macos.png    the macOS icon at 512 px: the window icon on
#                                         macOS, which is also the Dock's while the app
#                                         runs, so it sits on Apple's plate (D-129)
#   assets/icons/cassette.svg, cassette-N.png
#                                         the cassette alone, in colour (small buttons)
#   assets/icons/cassette-mono.svg        the cassette alone in one colour (currentColor)
#   assets/macos/LeadStinger.icns         the macOS app icon (scripts/bundle-macos.sh)
#   docs/design/logo/ship.figcraft        the FigCraft document they come from
set -euo pipefail

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
python3 docs/design/logo.py "$tmp/ship.json" ship
figcraft-cli run "$tmp/ship.json" --out "$tmp/ship.figcraft" >/dev/null
cp "$tmp/ship.figcraft" docs/design/logo/ship.figcraft

# Every frame as SVG, and as PNG at each size (frames are 1024 px).
figcraft-cli export "$tmp/ship.figcraft" -o "$tmp/svg" --format svg >/dev/null
for px in 16 32 64 128 256 512 1024; do
  figcraft-cli export "$tmp/ship.figcraft" -o "$tmp/$px" --format png \
    --scale "$(python3 -c "print($px / 1024)")" >/dev/null
done
# The frame to use at a size: the small twin at 16 and 32 px.
at() { if [ "$2" -le 32 ]; then echo "$tmp/$2/$1 small.png"; else echo "$tmp/$2/$1.png"; fi; }

mkdir -p assets/icons assets/macos
cp "$tmp/svg/leadstinger.svg" assets/linux/leadstinger.svg
cp "$tmp/svg/cassette.svg" assets/icons/cassette.svg
sed 's/fill="#000000"/fill="currentColor"/g' "$tmp/svg/cassette-mono.svg" > assets/icons/cassette-mono.svg
for px in 16 32 64 128 256 512; do
  cp "$(at leadstinger $px)" "assets/icons/leadstinger-$px.png"
done
cp "$(at macos 512)" assets/icons/leadstinger-macos.png
for px in 16 32 64; do
  cp "$(at cassette $px)" "assets/icons/cassette-$px.png"
done

if command -v iconutil >/dev/null; then
  set_=$tmp/LeadStinger.iconset
  mkdir -p "$set_"
  for n in 16 32 128 256 512; do
    cp "$(at macos $n)" "$set_/icon_${n}x${n}.png"
    cp "$(at macos $((n * 2)))" "$set_/icon_${n}x${n}@2x.png"
  done
  iconutil -c icns "$set_" -o assets/macos/LeadStinger.icns
else
  echo "iconutil not found (not macOS): assets/macos/LeadStinger.icns left as it was" >&2
fi
echo "Icons written"
