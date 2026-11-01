#!/usr/bin/env bash
# A real native-Wayland file drop onto LeadStinger (D-108), on a headless sway: a GTK
# drag source (dragon-drop) offers a WAV as text/uri-list and a virtual pointer
# (vpointer.py) drags it onto the board; leadstinger-cli checks that it became a tile,
# and that a second WAV dropped onto that tile replaced its audio (so the drop position
# is right). Screenshots of each drag and its result land in $OUT (default: .).
# Usage (Linux with Nix, from the repo root): scripts/wayland-drop-test/run.sh [BIN_DIR]
# BIN_DIR holds leadstinger and leadstinger-cli (default result/bin, from nix build).
# DROP_TEST_DEBUG=1 logs both clients' Wayland traffic (WAYLAND_DEBUG) into $OUT.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
bin=$(realpath "${1:-result/bin}")
out=$(realpath "${OUT:-.}")
work=$(mktemp -d)
trap 'kill $(jobs -p) 2>/dev/null || true; rm -rf "$work"' EXIT

# Tools: sway, dragon-drop, grim, jq, and Python with pywayland plus the protocol XML
# (pywayland's scanner asks pkg-config where wayland-scanner is).
nixpkgs=${NIXPKGS:-nixpkgs}
tools=$(nix build --no-link --print-out-paths "$nixpkgs#sway" "$nixpkgs#dragon-drop" "$nixpkgs#grim" "$nixpkgs#jq" "$nixpkgs#pkg-config" | tr '\n' ':')
export PATH="${tools//:/\/bin:}$PATH"
python=$(nix build --no-link --print-out-paths --impure --expr \
  "(builtins.getFlake \"$nixpkgs\").legacyPackages.\${builtins.currentSystem}.python3.withPackages (ps: [ ps.pywayland ])")/bin/python3
wlr=$(nix build --no-link --print-out-paths "$nixpkgs#wlr-protocols")/share/wlr-protocols
wayland_xml=$(nix build --no-link --print-out-paths "$nixpkgs#wayland-scanner.out")/share/wayland/wayland.xml
# Generated as one package, so the virtual pointer's module finds its wl_seat.
export VPOINTER_PROTOCOLS=$work/vp
mkdir -p "$VPOINTER_PROTOCOLS/protocols" && touch "$VPOINTER_PROTOCOLS/protocols/__init__.py"
"$python" -m pywayland.scanner -i "$wayland_xml" "$wlr/unstable/wlr-virtual-pointer-unstable-v1.xml" \
  -o "$VPOINTER_PROTOCOLS/protocols" >"$work/scanner.log" 2>&1 || { cat "$work/scanner.log"; exit 1; }

# A headless sway, 1600×900, windows side by side.
cat >"$work/sway.conf" <<EOF
output HEADLESS-1 resolution 1600x900 position 0 0
default_border none
focus_follows_mouse no
for_window [app_id="dragon-drop"] floating disable
exec echo "\$WAYLAND_DISPLAY \$SWAYSOCK" > $work/env
EOF
unset DISPLAY WAYLAND_DISPLAY SWAYSOCK
WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 sway -c "$work/sway.conf" >"$work/sway.log" 2>&1 &
for _ in $(seq 50); do [ -s "$work/env" ] && break; sleep 0.2; done
read -r WAYLAND_DISPLAY SWAYSOCK <"$work/env"
export WAYLAND_DISPLAY SWAYSOCK
echo "== sway on $WAYLAND_DISPLAY"

window() { swaymsg -t get_tree | jq -c --arg id "$1" '.. | objects | select(.app_id? == $id) | .rect' | head -1; }
wait_window() {
  for _ in $(seq 150); do [ -n "$(window "$1")" ] && return; sleep 0.2; done
  echo "no $1 window"; cat "$work"/*.log; exit 1
}

# LeadStinger on native Wayland, in a scratch library with no sound.
data=$work/data
export LEADSTINGER_DATA_DIR=$data LEADSTINGER_SILENT=1 LIBGL_ALWAYS_SOFTWARE=1
debug=()
[ -n "${DROP_TEST_DEBUG:-}" ] && debug=(env WAYLAND_DEBUG=client)
"${debug[@]}" "$bin/leadstinger" >"$work/leadstinger.log" 2>&1 &
wait_window leadstinger
for _ in $(seq 50); do "$bin/leadstinger-cli" status >/dev/null 2>&1 && break; sleep 0.2; done

tiles() { "$bin/leadstinger-cli" tile.list --json; }
fail() {
  echo "== FAIL: $1"; cp "$work"/*.log "$out/"; tail -n 20 "$work/leadstinger.log" "$work/dragon.log"; exit 1
}

# wav NAME SECONDS: a silent sound in $work.
wav() {
  "$python" -c 'import sys, wave
w = wave.open(sys.argv[1], "wb"); w.setnchannels(1); w.setsampwidth(2); w.setframerate(48000)
w.writeframes(b"\0\0" * int(48000 * float(sys.argv[2]))); w.close()' "$work/$1" "$2"
}

# drag FILE X Y SHOT: drag FILE from dragon-drop to (X, Y) inside LeadStinger's window,
# saving $out/SHOT-dragging.png during the drag and $out/SHOT-done.png after it.
drag() {
  GDK_BACKEND=wayland "${debug[@]}" dragon-drop --and-exit "$work/$1" >>"$work/dragon.log" 2>&1 &
  wait_window dragon-drop
  sleep 1
  local src dst sx sy dx dy pointer
  src=$(window dragon-drop)
  dst=$(window leadstinger)
  read -r sx sy <<<"$(jq -r '"\(.x + 40) \(.y + 25)"' <<<"$src")"
  read -r dx dy <<<"$(jq -r --argjson x "$2" --argjson y "$3" '"\(.x + $x) \(.y + $y)"' <<<"$dst")"
  echo "== drag '$1' from ($sx, $sy) to ($dx, $dy)"
  "$python" "$here/vpointer.py" 1600 900 "move $sx $sy" "wait 0.5" "down" "wait 0.2" \
    "glide $sx $sy $dx $dy 40" "wait 2" "up" "wait 1" >/dev/null &
  pointer=$!
  sleep 4
  grim "$out/$4-dragging.png"
  wait $pointer
  sleep 1
  grim "$out/$4-done.png"
}

# 1. Onto the empty board: a new tile. The name has a space, so its URI is
# percent-encoded.
wav "drop me.wav" 0.5
drag "drop me.wav" 400 450 wayland-drop
[ "$(tiles | jq length)" = 1 ] || fail "no tile after the first drop"
[ "$(tiles | jq -r '.[0].title')" = "drop me" ] || fail "the new tile isn't 'drop me'"
first=$(tiles | jq -c '.[0] | {id, duration}')
echo "== added: $first"

# 2. Onto that tile (the board's first slot, at (313, 170) in an 800 px wide window):
# one sound dropped on a tile replaces its audio, so the drop position must be right.
wav "replacement.wav" 1.5
drag "replacement.wav" 313 170 wayland-drop-on-tile
[ "$(tiles | jq length)" = 1 ] || fail "the drop onto the tile added a tile instead of replacing its audio"
second=$(tiles | jq -c '.[0] | {id, duration}')
echo "== after: $second"
[ "$second" != "$first" ] || fail "the tile's audio didn't change"
echo "== PASS: a drop added a tile, and a drop onto it replaced its audio"
