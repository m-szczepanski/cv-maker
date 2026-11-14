#!/usr/bin/env bash
# Build the release binaries locally (spec §12 M6, ADR-0013: no CI).
#
# Every target is cross-compiled with cargo-zigbuild (zig provides the linkers), so
# this runs on one Apple silicon machine. Usage:
#
#   scripts/build-release.sh            # all targets, artifacts in dist/
#   scripts/build-release.sh <triple>   # one target, e.g. aarch64-apple-darwin
#
# Requires: rustup targets for each triple (`rustup target add ...`), `zig`, and
# `cargo-zigbuild`. Archives are named cv-<version>-<triple>.<ext>; SHA256SUMS.txt
# covers them, so a fresh machine can verify what it downloaded.
set -euo pipefail

cd "$(dirname "$0")/.."
VERSION="$(cargo run --quiet -- --version | awk '{print $2}')"
DIST="dist"
mkdir -p "$DIST"

# target triple → archive command, in the order they are worth building first.
TARGETS=(
  "aarch64-apple-darwin"
  "x86_64-apple-darwin"
  "x86_64-unknown-linux-gnu"
  "aarch64-unknown-linux-gnu"
  "x86_64-pc-windows-gnu"
)

if [ $# -ge 1 ]; then
  TARGETS=("$1")
fi

build_one() {
  local target="$1"
  echo "==> $target"
  cargo zigbuild --release --target "$target"
  # Windows builds the executable as cv.exe; every other target is plain `cv`.
  local entry=cv
  case "$target" in *windows*) entry=cv.exe ;; esac
  local out="$DIST/cv-$VERSION-$target"
  rm -rf "$out"
  mkdir -p "$out"
  cp "target/$target/release/$entry" "$out/$entry"
  cp LICENSE "$out/LICENSE"
  case "$entry" in
    *.exe) (cd "$DIST" && rm -f "cv-$VERSION-$target.zip"
            zip -q -X "cv-$VERSION-$target.zip" "cv-$VERSION-$target/$entry" "cv-$VERSION-$target/LICENSE") ;;
    *) (cd "$DIST" && rm -f "cv-$VERSION-$target.tar.xz"
        tar -cJf "cv-$VERSION-$target.tar.xz" "cv-$VERSION-$target") ;;
  esac
  rm -rf "$out"
}

for target in "${TARGETS[@]}"; do
  build_one "$target"
done

echo "==> checksums"
(cd "$DIST" && rm -f SHA256SUMS.txt && shasum -a 256 cv-"$VERSION"-* > SHA256SUMS.txt)
ls -la "$DIST"
