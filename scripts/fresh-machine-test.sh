#!/usr/bin/env bash
# Fresh-machine test for a release archive (spec §12 M6 acceptance).
#
# Verifies the quick start works from the packaged binary alone: the archive is unpacked
# into a temporary directory, the binary runs with an isolated HOME, a minimal PATH and
# no access to this repository, and the README's three commands produce a one-page PDF.
# The checksum and the architecture are checked first, the way a user would.
#
# Usage: scripts/fresh-machine-test.sh [dist/cv-<version>-<triple>.tar.xz]
#        (defaults to the archive for the machine running the test)
#
# An archive built for another triple cannot be executed here, so it is verified
# structurally and reported as such. An archive for *this* triple that will not run is a
# failure — that is exactly the broken or quarantined download this test exists to catch.
set -euo pipefail

cd "$(dirname "$0")/.."
# The version comes from Cargo.toml, not from running the binary: the point of this
# script is that it works where no Rust toolchain exists.
VERSION="$(awk -F'"' '/^version = / { print $2; exit }' Cargo.toml)"
ARCH="$(uname -m | sed 's/arm64/aarch64/')"
OS="$(uname | tr 'A-Z' 'a-z' | sed 's/darwin/apple-darwin/;s/^linux$/linux-gnu/')"
HOST="$ARCH-$OS"
ARCHIVE="${1:-dist/cv-$VERSION-$HOST.tar.xz}"
NAME="$(basename "$ARCHIVE")"
# cv-0.1.0-aarch64-apple-darwin.tar.xz → aarch64-apple-darwin
TRIPLE="${NAME#cv-"$VERSION"-}"
TRIPLE="${TRIPLE%.tar.xz}"
TRIPLE="${TRIPLE%.zip}"

if [ ! -f "$ARCHIVE" ]; then
  echo "no archive at $ARCHIVE — run scripts/build-release.sh first" >&2
  exit 1
fi
echo "==> artifact: $ARCHIVE (target $TRIPLE)"

SUMS="$(dirname "$ARCHIVE")/SHA256SUMS.txt"
[ -f "$SUMS" ] || { echo "no checksum file at $SUMS" >&2; exit 1; }
EXPECTED="$(awk -v f="$NAME" '$2 == f {print $1; exit}' "$SUMS")"
[ -n "$EXPECTED" ] || { echo "$NAME is not listed in $SUMS" >&2; exit 1; }
ACTUAL="$(shasum -a 256 "$ARCHIVE" | awk '{print $1}')"
[ "$EXPECTED" = "$ACTUAL" ] || { echo "checksum mismatch: $ACTUAL != $EXPECTED" >&2; exit 1; }
echo "    sha256 ok: $ACTUAL"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/cv-fresh-XXXXXX")"
trap 'chmod -R u+rwX "$WORK" 2>/dev/null; rm -rf "$WORK"' EXIT
case "$NAME" in
  *.zip) unzip -q "$ARCHIVE" -d "$WORK" ;;
  *) tar -xf "$ARCHIVE" -C "$WORK" ;;
esac
BIN="$(find "$WORK" -type f \( -name cv -o -name cv.exe \) -print -quit)"
[ -n "$BIN" ] || { echo "the archive holds no cv binary" >&2; exit 1; }
chmod +x "$BIN"
printf '    binary: %s\n' "$BIN"
file "$BIN" | sed 's/^/    /'

if [ "$TRIPLE" != "$HOST" ]; then
  echo "    built for $TRIPLE, this machine is $HOST: structure checked, not run"
  exit 0
fi
if ! "$BIN" --version >/dev/null 2>&1; then
  echo "    a $HOST binary that will not run here is a failed artifact" >&2
  echo "    (on macOS, a downloaded binary may need right-click → Open once)" >&2
  exit 1
fi

PROJECT="$WORK/project"
mkdir -p "$PROJECT" "$WORK/home" "$WORK/tmp"
CLEAN_ENV=(env -i HOME="$WORK/home" PATH="/usr/bin:/bin" TMPDIR="$WORK/tmp")

"${CLEAN_ENV[@]}" "$BIN" --version | sed 's/^/    /'
"${CLEAN_ENV[@]}" "$BIN" templates | sed 's/^/    /'
(cd "$PROJECT" && "${CLEAN_ENV[@]}" "$BIN" init)
(cd "$PROJECT" && "${CLEAN_ENV[@]}" "$BIN" check)
RESULT="$(cd "$PROJECT" && "${CLEAN_ENV[@]}" "$BIN" build)"
echo "    $RESULT"

PDF="$PROJECT/cv.pdf"
[ -f "$PDF" ] || { echo "cv build wrote no PDF" >&2; exit 1; }
case "$RESULT" in
  *" 1 page "*) ;;
  *) echo "    expected a one-page result, got: $RESULT" >&2; exit 1 ;;
esac

SECOND="$(cd "$PROJECT" && "${CLEAN_ENV[@]}" "$BIN" build --output cv2.pdf)"
echo "    $SECOND"
cmp -s "$PDF" "$PROJECT/cv2.pdf" && echo "    rebuild is byte-identical (determinism holds)" \
  || { echo "    the rebuild differs from the first build" >&2; exit 1; }
[ -e "$PROJECT/cv.schema.json" ] && echo "    init also wrote cv.schema.json for the editor"

echo "OK: the quick start works from the release binary alone."
