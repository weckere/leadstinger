#!/usr/bin/env bash
# Build LeadStinger for a physical iPad with Xcode's automatic signing (your Apple
# Development identity) and install and launch it with `xcrun devicectl`.
#
# usage: scripts/ios-install.sh [--device NAME-OR-UDID] [--release] [--no-install]
#   --no-install   only build and sign
# Needs: Xcode, the rustup iOS targets (see scripts/ios-build.sh), `nix` (for xcodegen),
# the iPad paired and unlocked, Developer Mode on, and LEADSTINGER_TEAM set to your team id
# (10 characters; Xcode ▸ Settings ▸ Accounts shows it). Run it outside `nix develop`.
# The first run registers the app id with your team and creates a development profile.
set -euo pipefail
cd "$(dirname "$0")/.."

device="" config=Debug install=1
while [ $# -gt 0 ]; do
  case $1 in
    --device) device=$2; shift ;;
    --release) config=Release ;;
    --no-install) install=0 ;;
    *) echo "unknown argument $1" >&2; exit 2 ;;
  esac
  shift
done
: "${LEADSTINGER_TEAM:?set LEADSTINGER_TEAM to your Apple developer team id}"
export LEADSTINGER_TEAM

if [ -z "$device" ] && [ $install = 1 ]; then
  device=$(xcrun devicectl list devices 2>/dev/null | grep -E 'iPad.*physical' | grep -E '[[:space:]](available|connected)' | grep -oE '[0-9A-F]{8}-[0-9A-F]{16}' | head -1 || true)
  [ -n "$device" ] || { echo "no connected iPad found; pass --device or plug it in (xcrun devicectl list devices)" >&2; exit 1; }
fi

nix run nixpkgs#xcodegen -- generate --spec ios/project.yml --project ios >/dev/null
xcodebuild -project ios/LeadStinger.xcodeproj -scheme LeadStinger -configuration $config \
  -destination 'generic/platform=iOS' -derivedDataPath target/ios-xcode \
  -allowProvisioningUpdates build | tail -5
app=$(find target/ios-xcode/Build/Products -maxdepth 2 -name leadstinger.app | head -1)
echo "built $app"
if [ $install = 1 ]; then
  xcrun devicectl device install app --device "$device" "$app"
  xcrun devicectl device process launch --device "$device" io.github.weckere.leadstinger
fi
