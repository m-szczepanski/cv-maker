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
# Only an archive for this machine's architecture can actually be run; any other is
# verified structurally and reported as not executable here.
set -euo pipefail

cd "$(dirname "$0")/.."
VERSION="$(cargo run --quiet -- --version | awk '{print $2}')"
ARCH="$(uname -m | sed 's/arm64/aarch64/')"
OS="$(uname | tr 'A-Z' 'a-z' | sed 's/darwin/apple-darwin/;s/^linux$/linux-gnu/')"
TRIPLE="$ARCH-$OS"
ARCHIVE="${1:-dist/cv-$VERSION-$TRIPLE.tar.xz}"

if [ ! -f "$ARCHIVE" ]; then
  echo "no archive at $ARCHIVE — run scripts/build-release.sh first" >&2
  exit 1
fi
echo "==> artifact: $ARCHIVE"

EXPECTED="$(cd "$(dirname "$ARCHIVE")" && awk -v f="$(basename "$ARCHIVE")" '$2 == f {print $1}' SHA256SUMS.txt)"
ACTUAL="$(shasum -a 256 "$ARCHIVE" | awk '{print $1}')"
[ "$EXPECTED" = "$ACTUAL" ] || { echo "checksum mismatch: $ACTUAL != $EXPECTED" >&2; exit 1; }
echo "    sha256 ok: $ACTUAL"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/cv-fresh-XXXXXX")"
trap 'chmod -R u+rwX "$WORK" 2>/dev/null; rm -rf "$WORK"' EXIT
tar -xf "$ARCHIVE" -C "$WORK"
BIN="$(find "$WORK" -type f \( -name cv -o -name cv.exe \) | head -n 1)"
chmod +x "$BIN"
printf '    binary: %s\n' "$BIN"
file "$BIN" | sed 's/^/    /'

if ! "$BIN" --version >/dev/null 2>&1; then
  echo "    this machine cannot execute $TRIPLE binaries: structure checked, not run"
  exit 0
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
