#!/usr/bin/env bash
#
# Gather the two desktop images the stick carries (gitignored build inputs,
# like the EFI bits): the UNMODIFIED squashfs.img out of Fedora's own live
# ISOs - Workstation (GNOME) and KDE - verified against the sha256 Fedora
# publishes in the release CHECKSUM files. The kickstart installs one of
# them with `liveimg` (architecture.md, "Design constraint: offline"): no
# composition of our own, the same bytes Fedora ships, so the desktop the
# person gets is exactly the one they would get from the live ISO.
#
#   payload-bits/LiveOS/gnome.squashfs   <- Fedora-Workstation-Live-42-1.1.x86_64.iso:LiveOS/squashfs.img
#   payload-bits/LiveOS/kde.squashfs     <- Fedora-KDE-Desktop-Live-42-1.1.x86_64.iso:LiveOS/squashfs.img
#   payload-bits/LiveOS/source.txt       <- where they came from, the ISO hashes, the image hashes
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/artifacts"
mkdir -p payload-bits/LiveOS iso
BASE=${FEDORA_BASE:-https://archives.fedoraproject.org/pub/archive/fedora/linux/releases/42}

# name  dir  iso  sha256 (from Fedora-<ed>-42-1.1-x86_64-CHECKSUM, read 2026-09-09)
DESKTOPS="gnome Workstation Fedora-Workstation-Live-42-1.1.x86_64.iso 98958d80e8a80eabe61275337f969c8e2212adc3a223d9bbdab9411bb1c95cba
kde KDE Fedora-KDE-Desktop-Live-42-1.1.x86_64.iso cf4beecc21ffae86d0e368797cbcc02ba7f4c6549c626db01b1d3f4e1444da85"

: > payload-bits/LiveOS/source.txt.new
while read -r name dir iso sha; do
    [ -n "$name" ] || continue
    out=payload-bits/LiveOS/$name.squashfs
    if [ -f "$out" ] && grep -q "$iso" payload-bits/LiveOS/source.txt 2>/dev/null; then
        echo "fetch-desktops: $out present"
        grep "$name\|$iso" payload-bits/LiveOS/source.txt >> payload-bits/LiveOS/source.txt.new || true
        continue
    fi
    if [ ! -f "iso/$iso" ] || ! echo "$sha  iso/$iso" | sha256sum -c --quiet 2>/dev/null; then
        echo "fetch-desktops: downloading $BASE/$dir/x86_64/iso/$iso"
        curl -L --fail --retry 5 --retry-delay 10 -C - -o "iso/$iso.part" "$BASE/$dir/x86_64/iso/$iso"
        mv "iso/$iso.part" "iso/$iso"
        echo "$sha  iso/$iso" | sha256sum -c --quiet || { echo "fetch-desktops: sha256 MISMATCH for $iso - refusing" >&2; exit 1; }
    fi
    echo "fetch-desktops: iso verified: $iso"
    curl -sL --fail -o "iso/$iso.CHECKSUM" "$BASE/$dir/x86_64/iso/Fedora-$dir-42-1.1-x86_64-CHECKSUM" || true
    bsdtar -x -O -f "iso/$iso" LiveOS/squashfs.img > "$out.part"
    mv "$out.part" "$out"
    img_sha=$(sha256sum "$out" | cut -c1-64)
    printf '%s: %s (%s bytes) from %s/%s/x86_64/iso/%s sha256 %s\n' "$name" "$out" "$(stat -c %s "$out")" "$BASE" "$dir" "$iso" "$sha" >> payload-bits/LiveOS/source.txt.new
    printf '%s.squashfs sha256 %s\n' "$name" "$img_sha" >> payload-bits/LiveOS/source.txt.new
    echo "fetch-desktops: extracted $out ($(stat -c %s "$out") bytes)"
done <<< "$DESKTOPS"
mv payload-bits/LiveOS/source.txt.new payload-bits/LiveOS/source.txt
echo "fetch-desktops: done."
ls -l payload-bits/LiveOS/
