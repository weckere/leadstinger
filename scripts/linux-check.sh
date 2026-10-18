#!/usr/bin/env bash
# Build, test and screenshot LeadStinger on a Linux host with Nix (run from the repo root).
# Usage: scripts/linux-check.sh [cores]
set -euo pipefail
cores=${1:-0}
echo "== $(uname -m) $(hostname)"
nice -n 19 nix develop --cores "$cores" -c cargo test --release -j "${cores/#0/$(nproc)}" 2>&1 | grep -E "test result|FAILED|panicked|^error" || true
nice -n 19 nix build --cores "$cores" --max-jobs 1 -L .#default 2>&1 | tail -3
ls -la result/bin/
data=$(mktemp -d)
LEADSTINGER_DATA_DIR=$data LEADSTINGER_DEMO=1 LEADSTINGER_SILENT=1 \
LEADSTINGER_SCRIPT="play:0,play:5,play:8,edit,select:4" \
LEADSTINGER_SNAPSHOT=$PWD/linux-$(uname -m).png LEADSTINGER_SNAPSHOT_FRAMES=90 \
  nix develop -c xvfb-run -a -s "-screen 0 1400x900x24" timeout 120 ./result/bin/leadstinger 2>&1 | grep -v "^ALSA" | tail -5
rm -rf "$data"
