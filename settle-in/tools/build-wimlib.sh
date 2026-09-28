#!/bin/sh
# Build the one outside program "Go back to Windows" carries: wimlib-imagex,
# static, so the same file runs on every Linux (like settle-in itself). It
# splits Windows' install.wim into install.swm parts for a FAT32 stick, the
# step Microsoft's own instructions do with DISM on Windows.
#
# wimlib is GPL-3.0-or-later (built without libntfs-3g and FUSE), the same
# licence as this project. The source tarball is pinned by SHA-256.
#
#   settle-in/tools/build-wimlib.sh OUTDIR     -> OUTDIR/wimlib-imagex
set -eu
VER=1.14.4
SHA=3633db2b6c8b255eb86d3bf3df3059796bd1f08e50b8c9728c7eb66662e51300
OUT=${1:?usage: build-wimlib.sh OUTDIR}
mkdir -p "$OUT"
OUT=$(cd "$OUT" && pwd)
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
curl -fsSL -o "$WORK/wimlib.tar.gz" "https://wimlib.net/downloads/wimlib-$VER.tar.gz"
echo "$SHA  $WORK/wimlib.tar.gz" | sha256sum -c - >/dev/null || { echo "build-wimlib: the tarball's SHA-256 is wrong; not building" >&2; exit 1; }
tar -xzf "$WORK/wimlib.tar.gz" -C "$WORK"
cd "$WORK/wimlib-$VER"
./configure --without-ntfs-3g --without-fuse --disable-shared --enable-static >/dev/null
make -j"$(nproc)" >/dev/null
rm -f wimlib-imagex
make wimlib-imagex LDFLAGS=-all-static >/dev/null
strip wimlib-imagex
file wimlib-imagex | grep -q 'statically linked' || { echo "build-wimlib: the result is not static" >&2; exit 1; }
cp wimlib-imagex "$OUT/wimlib-imagex"
"$OUT/wimlib-imagex" --version | head -1
