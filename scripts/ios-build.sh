#!/usr/bin/env bash
# Build LeadStinger for iPadOS and, for the simulator, install and launch it
# (docs/IPAD-PLAN.md). The Rust toolchain for iOS comes from rustup, not the nix shell:
#   rustup target add aarch64-apple-ios-sim aarch64-apple-ios
#
# usage: scripts/ios-build.sh [--device] [--release] [--run] [--sim NAME-OR-UDID]
#   default: a debug build for the simulator, bundled at target/ios/LeadStinger.app
#   --run    boot the simulator (without opening its window), install, launch
#   --device build for a device (aarch64-apple-ios); signing it needs a team and
#            provisioning profile, which this script doesn't do
# Run it outside `nix develop`: the nix compiler wrappers don't know the iOS SDKs.
set -euo pipefail
cd "$(dirname "$0")/.."

device=0 release=0 run=0 sim="iPad Pro 13-inch (M5)"
while [ $# -gt 0 ]; do
  case $1 in
    --device) device=1 ;;
    --release) release=1 ;;
    --run) run=1 ;;
    --sim) sim=$2; shift ;;
    *) echo "unknown argument $1" >&2; exit 2 ;;
  esac
  shift
done

if [ $device = 1 ]; then target=aarch64-apple-ios sdk=iphoneos triple=arm64-apple-ios17.0
else target=aarch64-apple-ios-sim sdk=iphonesimulator triple=arm64-apple-ios17.0-simulator; fi
profile=debug; flags=()
if [ $release = 1 ]; then profile=release; flags=(--release); fi

sdkroot=$(xcrun --sdk $sdk --show-sdk-path)
# bindgen (signalsmith-stretch) wants the target spelled the way clang does.
export "BINDGEN_EXTRA_CLANG_ARGS_${target//-/_}=--target=$triple -isysroot $sdkroot"
cargo build --target $target --bin leadstinger ${flags[@]+"${flags[@]}"}

app=target/ios/LeadStinger.app
rm -rf $app && mkdir -p $app
cp target/$target/$profile/leadstinger $app/leadstinger
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
sed "s/\$(MARKETING_VERSION)/$version/" ios/Info.plist > $app/Info.plist
src=assets/icons/leadstinger-512.png
for spec in 60x60:120 76x76:152 83.5x83.5:167; do
  sips -z "${spec#*:}" "${spec#*:}" $src --out "$app/AppIcon${spec%%:*}@2x.png" >/dev/null
done
if [ $device = 1 ]; then
  echo "built $app for a device; sign it with an Apple Development identity and a profile before installing"
  exit 0
fi
codesign -s - -f --timestamp=none $app >/dev/null 2>&1
echo "built $app"

if [ $run = 1 ]; then
  udid=$(xcrun simctl list devices available | grep -F "$sim (" | head -1 | sed 's/.*(\([0-9A-F-]\{36\}\)).*/\1/')
  [ -n "$udid" ] || { echo "no simulator named $sim" >&2; exit 1; }
  xcrun simctl boot "$udid" 2>/dev/null || true
  xcrun simctl bootstatus "$udid" >/dev/null
  xcrun simctl install "$udid" $app
  # LEADSTINGER_* variables (DEMO, SILENT, SNAPSHOT, ...) reach the app as SIMCTL_CHILD_*.
  while IFS='=' read -r name value; do export "SIMCTL_CHILD_$name=$value"; done < <(env | grep '^LEADSTINGER_' || true)
  xcrun simctl launch --terminate-running-process "$udid" io.github.weckere.leadstinger
  echo "simulator: $udid"
fi
