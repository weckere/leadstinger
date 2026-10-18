#!/usr/bin/env bash
# Build a single-file LeadStinger AppImage. Run it on Linux (x86_64 or aarch64) with Nix,
# from the repo root.
#   scripts/build-appimage.sh [output file]
set -euo pipefail

version=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
out=${1:-dist/LeadStinger-$version-$(uname -m).AppImage}

# nix-appimage, pinned: a type-2 AppImage holding the whole Nix closure, started through a
# user-namespace chroot, so it needs no Nix, FUSE or libraries on the target machine.
bundler=github:ralismark/nix-appimage/7946addbc0d97e358a6d7aefe5e82310f0fe6b18

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
nix bundle --bundler "$bundler" .#appimage -o "$tmp/result"
mkdir -p "$(dirname "$out")"
install -m755 "$(readlink -f "$tmp/result")" "$out"
echo "Built $out ($(du -h "$out" | cut -f1))"
