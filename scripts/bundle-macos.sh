#!/usr/bin/env bash
# Build a release macOS app into dist/ (run inside `nix develop`, from the repo root):
#   scripts/bundle-macos.sh               LeadStinger.app
#   UNIVERSAL=1 scripts/bundle-macos.sh   one binary for arm64 and x86_64 (needs both
#                                         Rust targets installed; used for releases)
set -euo pipefail

version=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
file=LeadStinger name=LeadStinger id=io.github.weckere.leadstinger scheme=leadstinger
icon=assets/macos/LeadStinger.icns
if [ -n "${UNIVERSAL:-}" ]; then
  targets=(aarch64-apple-darwin x86_64-apple-darwin)
  for t in "${targets[@]}"; do cargo build --release --target "$t"; done
  bin=target/universal-apple-darwin/release
  mkdir -p "$bin"
  for b in leadstinger leadstinger-cli; do
    lipo -create -output "$bin/$b" $(for t in "${targets[@]}"; do echo "target/$t/release/$b"; done)
  done
else
  cargo build --release
  bin=target/release
fi
app=dist/$file.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$bin/leadstinger" "$app/Contents/MacOS/$file"
# The companion CLI (docs/CLI.md); link it onto PATH with
#   ln -s "$PWD/$app/Contents/MacOS/leadstinger-cli" ~/.local/bin/
cp "$bin/leadstinger-cli" "$app/Contents/MacOS/leadstinger-cli"
# The app icon (scripts/icons.sh makes LeadStinger's).
icon_key=
if [ -n "$icon" ]; then
  cp "$icon" "$app/Contents/Resources/$file.icns"
  icon_key="<key>CFBundleIconFile</key><string>$file</string>"
fi

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
  $icon_key
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSApplicationCategoryType</key><string>public.app-category.music</string>
  <!-- QuipClipper's "Send to $name" links (docs/QUIPCLIPPER.md, src/quip.rs). -->
  <key>CFBundleURLTypes</key>
  <array>
    <dict>
      <key>CFBundleURLName</key><string>$id.quip</string>
      <key>CFBundleURLSchemes</key><array><string>$scheme</string></array>
      <key>CFBundleTypeRole</key><string>Viewer</string>
    </dict>
  </array>
  <!-- Set files (.lsset, src/setfile.rs): a double-click imports one after asking. -->
  <key>CFBundleDocumentTypes</key>
  <array>
    <dict>
      <key>CFBundleTypeName</key><string>$name Set</string>
      <key>CFBundleTypeRole</key><string>Viewer</string>
      <key>LSHandlerRank</key><string>Owner</string>
      <key>LSItemContentTypes</key><array><string>io.github.weckere.leadstinger.set</string></array>
    </dict>
  </array>
  <key>UTExportedTypeDeclarations</key>
  <array>
    <dict>
      <key>UTTypeIdentifier</key><string>io.github.weckere.leadstinger.set</string>
      <key>UTTypeDescription</key><string>LeadStinger Set</string>
      <key>UTTypeConformsTo</key><array><string>public.data</string><string>public.archive</string></array>
      <key>UTTypeTagSpecification</key>
      <dict>
        <key>public.filename-extension</key><array><string>lsset</string></array>
        <key>public.mime-type</key><array><string>application/x-leadstinger-set</string></array>
      </dict>
    </dict>
  </array>
  <!-- Find on Network looks for QuipClipper's Bonjour advertisement (src/quip/discover.rs). -->
  <key>NSLocalNetworkUsageDescription</key><string>$name looks for QuipClipper on your network when you ask it to find one, so you can connect without typing its address.</string>
  <key>NSBonjourServices</key><array><string>_quipclipper._tcp</string></array>
  <key>NSMicrophoneUsageDescription</key><string>$name listens to your microphone to duck music under your voice and to record clips, only when you turn those on.</string>
</dict>
</plist>
PLIST

# Ad-hoc signatures, inside out: lipo drops the linker's signature, and codesign won't
# sign an app around an unsigned executable.
codesign --force --sign - "$app/Contents/MacOS/leadstinger-cli" "$app/Contents/MacOS/$file" >/dev/null
codesign --force --sign - "$app" >/dev/null
codesign --verify --strict "$app"
echo "Built $app ($version)"
