#!/bin/sh
# Build cabextract, static, so the same file runs on every Linux (like
# settle-in and wimlib-imagex). "Go back to Windows" reads Microsoft's own
# catalog of Windows files (products.cab, the file its Media Creation Tool
# reads), and that cabinet is LZX-compressed (seen 2026-09-29, RISKS R33).
#
# cabextract (with its bundled libmspack) is GPL-3.0-or-later, the same
# licence as this project. The source tarball is pinned by SHA-256; the same
# hash is in Debian's cabextract_1.11-2.dsc (read 2026-09-29).
#
#   settle-in/tools/build-cabextract.sh OUTDIR     -> OUTDIR/cabextract
set -eu
VER=1.11
SHA=b5546db1155e4c718ff3d4b278573604f30dd64c3c5bfd4657cd089b823a3ac6
OUT=${1:?usage: build-cabextract.sh OUTDIR}
mkdir -p "$OUT"
OUT=$(cd "$OUT" && pwd)
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
curl -fsSL -o "$WORK/cabextract.tar.gz" "https://www.cabextract.org.uk/cabextract-$VER.tar.gz"
echo "$SHA  $WORK/cabextract.tar.gz" | sha256sum -c - >/dev/null || { echo "build-cabextract: the tarball's SHA-256 is wrong; not building" >&2; exit 1; }
tar -xzf "$WORK/cabextract.tar.gz" -C "$WORK"
cd "$WORK/cabextract-$VER"
./configure LDFLAGS=-static >/dev/null
make -j"$(nproc)" >/dev/null
strip cabextract
file cabextract | grep -q 'statically linked' || { echo "build-cabextract: the result is not static" >&2; exit 1; }
cp cabextract "$OUT/cabextract"
"$OUT/cabextract" --version | head -1
