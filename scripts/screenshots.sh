#!/usr/bin/env bash
# Retake the README's screenshots (docs/screenshots/*.png) from the generated demo
# library (LEADSTINGER_DEMO=1, src/demo.rs): Console and Panel, grid and list, with a
# few tiles playing so the countdown, progress and meter show.
#
#   scripts/screenshots.sh [OUT_DIR]     # default docs/screenshots
#
# Each shot gets its own scratch library, sound is off, and the window stays out of
# sight (a snapshot run is in the background by itself). The window is 1280x800
# points, so a Retina Mac writes 2560x1600 pixels, the size the README's images are.
set -euo pipefail
cd "$(dirname "$0")/.."
out=${1:-docs/screenshots}
mkdir -p "$out"

if [ -z "${IN_NIX_SHELL:-}" ] && command -v nix >/dev/null; then
    nix develop -c cargo build --release --bin leadstinger
else
    cargo build --release --bin leadstinger
fi
bin=${CARGO_TARGET_DIR:-target}/release/leadstinger

scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT

# shot NAME SCRIPT: one screenshot, $out/NAME.png.
shot() {
    LEADSTINGER_DATA_DIR="$scratch/$1" \
    LEADSTINGER_DEMO=1 \
    LEADSTINGER_SILENT=1 \
    LEADSTINGER_BACKGROUND=1 \
    LEADSTINGER_WINDOW_SIZE=1280x800 \
    LEADSTINGER_SNAPSHOT_FRAMES=150 \
    LEADSTINGER_SCRIPT="$2" \
    LEADSTINGER_SNAPSHOT="$out/$1.png" \
        "$bin"
}

# Game Night (grid): the intro music, the crowd bed and a spoken line playing.
grid="hide:inspector,play:0,play:10,play:12"
# Theatre Cues (list): GO has started the preshow music.
list="set:1,hide:inspector,go"

shot console-grid "console,$grid"
shot panel-grid "panel,$grid"
shot console-list "console,$list"
shot panel-list "panel,$list"

if command -v sips >/dev/null; then
    for f in "$out"/{console,panel}-{grid,list}.png; do
        size=$(sips -g pixelWidth -g pixelHeight "$f" | awk '/pixel/ {print $2}' | paste -sd x -)
        echo "$f: $size"
        [ "$size" = 2560x1600 ] || echo "  (the README's images are 2560x1600: take them on a Retina display)" >&2
    done
fi
