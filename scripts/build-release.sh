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
# clap prints the version from Cargo.toml, so that file is the source of truth for the
# artifact names — reading it avoids building the tool just to ask it its own version.
VERSION="$(awk -F'"' '/^version = / { print $2; exit }' Cargo.toml)"
DIST="dist"
mkdir -p "$DIST"

# The matrix lives in scripts/release-targets so the fresh-machine test can tell a
# foreign target apart from a name that is not a release artifact at all.
ALL_TARGETS=()
while IFS= read -r line; do
  case "$line" in ''|'#'*) continue ;; esac
  ALL_TARGETS+=("$line")
done < "scripts/release-targets"

TARGETS=("${ALL_TARGETS[@]}")
if [ $# -ge 1 ]; then
  case " ${ALL_TARGETS[*]} " in
    *" $1 "*) TARGETS=("$1") ;;
    *) echo "unknown target '$1' (scripts/release-targets lists: ${ALL_TARGETS[*]})" >&2
       exit 1 ;;
  esac
fi

build_one() {
  local target="$1"
  echo "==> $target"
  cargo zigbuild --release --target "$target"
  # Windows builds the executable as cv.exe; every other target is plain `cv`.
  local entry=cv
  case "$target" in *windows*) entry=cv.exe ;; esac
  local out_name="cv-$VERSION-$target"
  local out="$DIST/$out_name"
  rm -rf "$out"
  mkdir -p "$out"
  cp "target/$target/release/$entry" "$out/$entry"
  cp LICENSE "$out/LICENSE"
  # Archive the same bytes twice, get the same file: members in sorted order (bsdtar has
  # no `--sort`) with pinned timestamps. The builder's uid/gid is still recorded, so the
  # checksums are stable on a build host rather than across every machine.
  local -a members=()
  while IFS= read -r member; do
    members+=("$member")
  done < <(cd "$DIST" && find "$out_name" -type f | LC_ALL=C sort)
  (cd "$DIST" && touch -t 202001010000.00 "${members[@]}")
  case "$entry" in
    *.exe) (cd "$DIST" && rm -f "cv-$VERSION-$target.zip"
            zip -q -X "cv-$VERSION-$target.zip" "${members[@]}") ;;
    *) (cd "$DIST" && rm -f "cv-$VERSION-$target.tar.xz"
        tar -cJf "cv-$VERSION-$target.tar.xz" "${members[@]}") ;;
  esac
  rm -rf "$out"
}

for target in "${TARGETS[@]}"; do
  build_one "$target"
done

echo "==> checksums"
# Bare file names, sorted, files only: a leftover staging directory from an interrupted
# build must not end up in the list `shasum -c` is checked against.
(cd "$DIST" && rm -f SHA256SUMS.txt
  for artifact in "cv-$VERSION"-*; do
    [ -f "$artifact" ] && printf '%s\n' "$artifact"
  done | LC_ALL=C sort | xargs shasum -a 256 > SHA256SUMS.txt
  # Bare names, not "./name": the file is read from wherever the user unpacked it.
  sed -i.bak 's|^\./||' SHA256SUMS.txt && rm -f SHA256SUMS.txt.bak)
ls -la "$DIST"
