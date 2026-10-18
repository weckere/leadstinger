#!/usr/bin/env bash
# Build a release macOS app into dist/ (run inside `nix develop`, from the repo root):
#   scripts/bundle-macos.sh            LeadStinger.app (the public brand)
#   BRAND=sttnger scripts/bundle-macos.sh   STTNGer.app (shown as ST:TNGer, the gift build)
#   UNIVERSAL=1 scripts/bundle-macos.sh      one binary for arm64 and x86_64 (needs both
#                                            Rust targets installed; used for releases)
set -euo pipefail

version=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
case "${BRAND:-leadstinger}" in
  leadstinger) features=() file=LeadStinger name=LeadStinger id=io.github.weckere.leadstinger ;;
  sttnger) features=(--features sttnger) file=STTNGer name=ST:TNGer id=local.sttnger ;;
  *) echo "BRAND must be leadstinger or sttnger" >&2; exit 2 ;;
esac
if [ -n "${UNIVERSAL:-}" ]; then
  targets=(aarch64-apple-darwin x86_64-apple-darwin)
  for t in "${targets[@]}"; do cargo build --release --target "$t" ${features[@]+"${features[@]}"}; done
  bin=target/universal-apple-darwin/release
  mkdir -p "$bin"
  for b in leadstinger leadstinger-cli; do
    lipo -create -output "$bin/$b" $(for t in "${targets[@]}"; do echo "target/$t/release/$b"; done)
  done
else
  cargo build --release ${features[@]+"${features[@]}"}
  bin=target/release
fi
app=dist/$file.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$bin/leadstinger" "$app/Contents/MacOS/$file"
# The companion CLI (docs/CLI.md); link it onto PATH with
#   ln -s "$PWD/$app/Contents/MacOS/leadstinger-cli" ~/.local/bin/
cp "$bin/leadstinger-cli" "$app/Contents/MacOS/leadstinger-cli"

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>$name</string>
  <key>CFBundleDisplayName</key><string>$name</string>
  <key>CFBundleIdentifier</key><string>$id</string>
  <key>CFBundleExecutable</key><string>$file</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSApplicationCategoryType</key><string>public.app-category.music</string>
</dict>
</plist>
PLIST

# A private pack is bundled only when asked for, never by default.
if [ -n "${STTNGER_BUNDLE_GIFT:-}" ]; then
  [ -f "$STTNGER_BUNDLE_GIFT/manifest.json" ] || { echo "STTNGER_BUNDLE_GIFT: no manifest.json in $STTNGER_BUNDLE_GIFT" >&2; exit 1; }
  cp -R "$STTNGER_BUNDLE_GIFT" "$app/Contents/Resources/Gift"
  echo "Bundled the gift pack from $STTNGER_BUNDLE_GIFT (keep this build private)"
fi

# Ad-hoc signatures, inside out: lipo drops the linker's signature, and codesign won't
# sign an app around an unsigned executable.
codesign --force --sign - "$app/Contents/MacOS/leadstinger-cli" "$app/Contents/MacOS/$file" >/dev/null
codesign --force --sign - "$app" >/dev/null
codesign --verify --strict "$app"
echo "Built $app ($version)"
