#!/usr/bin/env bash
# Build, test and screenshot LeadStinger on a Linux host with Nix (run from the repo root).
# Usage: scripts/linux-check.sh [cores]
#   LINUX_CHECK_FULL=1  also `nix build` the package (a cold build in the sandbox, ~20 min on
#                       6 cores): the coordinator's merge check does this; a branch check needn't.
# Every clone on the host shares one cargo target dir (CARGO_TARGET_DIR, default
# ~/.cache/leadstinger-target), so a branch check only rebuilds what its branch changed, and
# checks queue on one lock instead of running at once: two cold release builds on a 6-core
# box took the load to 27 on 2026-11-02.
set -euo pipefail
cores=${1:-0}
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$HOME/.cache/leadstinger-target}
mkdir -p "$CARGO_TARGET_DIR"
lock=${LINUX_CHECK_LOCK:-/tmp/leadstinger-linux-check.lock}
exec 9>"$lock"
if ! flock -n 9; then echo "== waiting for another linux-check to finish ($lock)"; flock 9; fi
echo "== $(uname -m) $(hostname)  target: $CARGO_TARGET_DIR"
nice -n 19 nix develop --cores "$cores" -c cargo test --release -j "${cores/#0/$(nproc)}" 2>&1 | grep -E "test result|FAILED|panicked|^error" || true
if [ "${LINUX_CHECK_FULL:-0}" = 1 ]; then
  nice -n 19 nix build --cores "$cores" --max-jobs 1 -L .#default 2>&1 | tail -3
  ls -la result/bin/
else
  echo "== nix build skipped (LINUX_CHECK_FULL=1 runs it)"
  nice -n 19 nix develop --cores "$cores" -c cargo build --release -j "${cores/#0/$(nproc)}" 2>&1 | grep -E "^warning|^error|Finished" || true
  mkdir -p result/bin && ln -sf "$CARGO_TARGET_DIR/release/leadstinger" result/bin/leadstinger
fi
data=$(mktemp -d)
LEADSTINGER_DATA_DIR=$data LEADSTINGER_DEMO=1 LEADSTINGER_SILENT=1 \
LEADSTINGER_SCRIPT="play:0,play:5,play:8,select:4" \
LEADSTINGER_SNAPSHOT=$PWD/linux-$(uname -m).png LEADSTINGER_SNAPSHOT_FRAMES=90 \
  nix develop -c xvfb-run -a -s "-screen 0 1400x900x24" timeout 120 ./result/bin/leadstinger 2>&1 | grep -v "^ALSA" | tail -5
rm -rf "$data"
