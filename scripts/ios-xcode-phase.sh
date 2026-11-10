#!/usr/bin/env bash
# The Run Script phase of ios/project.yml: build the Rust binary for the platform Xcode is
# building (device or simulator) and put it, with the app icons, in the bundle Xcode signs.
set -euo pipefail
cd "$SRCROOT/.."
export PATH="$HOME/.cargo/bin:$PATH"
case "$PLATFORM_NAME" in
  iphoneos) target=aarch64-apple-ios triple=arm64-apple-ios17.0 ;;
  iphonesimulator) target=aarch64-apple-ios-sim triple=arm64-apple-ios17.0-simulator ;;
  *) echo "unsupported platform $PLATFORM_NAME" >&2; exit 1 ;;
esac
profile=debug; flags=()
if [ "$CONFIGURATION" = Release ]; then profile=release; flags=(--release); fi
# Xcode's compiler variables are for its own builds; cargo picks the iOS SDK itself.
unset SDKROOT CC CXX LD LDPLUSPLUS CPATH LIBRARY_PATH IPHONEOS_DEPLOYMENT_TARGET MACOSX_DEPLOYMENT_TARGET
sdkroot=$(xcrun --sdk "$PLATFORM_NAME" --show-sdk-path)
export "BINDGEN_EXTRA_CLANG_ARGS_${target//-/_}=--target=$triple -isysroot $sdkroot"
cargo build --target "$target" --bin leadstinger ${flags[@]+"${flags[@]}"}
cp "target/$target/$profile/leadstinger" "$TARGET_BUILD_DIR/$EXECUTABLE_PATH"
res="$TARGET_BUILD_DIR/$UNLOCALIZED_RESOURCES_FOLDER_PATH"
for spec in 60x60:120 76x76:152 83.5x83.5:167; do
  sips -z "${spec#*:}" "${spec#*:}" assets/icons/leadstinger-512.png --out "$res/AppIcon${spec%%:*}@2x.png" >/dev/null
done
