#!/usr/bin/env bash
#
# fetch-release.sh ID - gather one release's build inputs for the kit
# (decided 2026-10-03, the owner; RISKS R34). The release, its download
# addresses and every checksum come from data/releases.ps1, whose facts
# data/tools/measure-release.py measured from these same files. Nothing
# here is release-specific: a new release is a new table entry.
#
#   rig/vm/artifacts/releases/ID/iso/          the ISOs, verified against the table
#   rig/vm/artifacts/releases/ID/bits/         what the stick carries, extracted:
#     EFI/BOOT/BOOTX64.EFI, EFI/BOOT/grubx64.efi   (the installer ISO's signed chain)
#     images/pxeboot/vmlinuz, initrd.img, images/install.img
#     LiveOS/<desktop>.squashfs                  (each desktop ISO's live image)
#
# Any checksum that differs from the table stops it (never "close enough").
set -euo pipefail
ID=${1:?usage: fetch-release.sh RELEASE-ID}
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
R="$ROOT/rig/vm/artifacts/releases/$ID"
mkdir -p "$R/iso" "$R/bits"
J=$(powershell.exe -NoProfile -Command ". '$(wslpath -w "$ROOT/data/releases.ps1")'; Get-UpgReleaseTable | Where-Object { \$_.Id -eq '$ID' } | ConvertTo-Json -Depth 6" | tr -d '\r')
[ -n "$J" ] || { echo "fetch-release: no release '$ID' in data/releases.ps1" >&2; exit 1; }
echo "$J" > "$R/release.json.table"

get() {  # url sha256 -> iso/<name>, verified
    local url=$1 sha=$2 f="$R/iso/$(basename "$1")"
    if [ ! -f "$f" ] || ! echo "$sha  $f" | sha256sum -c --quiet 2>/dev/null; then
        echo "fetch-release: downloading $url"
        curl -L --fail --retry 5 --retry-delay 10 -C - -o "$f.part" "$url"
        mv "$f.part" "$f"
    fi
    echo "$sha  $f" | sha256sum -c --quiet || { echo "fetch-release: sha256 MISMATCH for $f - refusing" >&2; exit 1; }
    echo "$f"
}
q() { python3 -c "import json,sys; j=json.loads(sys.stdin.read()); print($1)" <<< "$J"; }

NET=$(get "$(q "j['Netinst']['Url']")" "$(q "j['Netinst']['Sha256']")" | tail -1)
mkdir -p "$R/bits/EFI/BOOT" "$R/bits/images/pxeboot" "$R/bits/LiveOS"
bsdtar -x -f "$NET" -C "$R/bits" EFI/BOOT/BOOTX64.EFI EFI/BOOT/grubx64.efi images/pxeboot/vmlinuz images/pxeboot/initrd.img images/install.img
for f in BOOTX64.EFI grubx64.efi; do
    want=$(q "[b['Sha256'] for b in j['Boot'] if b['Role']=='stick' and b['File']=='$f'][0]")
    echo "$want  $R/bits/EFI/BOOT/$f" | sha256sum -c --quiet || { echo "fetch-release: $f differs from the table - refusing" >&2; exit 1; }
done
for d in $(q "' '.join(x['Id'] for x in j['Desktops'])"); do
    url=$(q "[x['Url'] for x in j['Desktops'] if x['Id']=='$d'][0]"); sha=$(q "[x['IsoSha256'] for x in j['Desktops'] if x['Id']=='$d'][0]")
    img=$(q "[x['ImagePath'] for x in j['Desktops'] if x['Id']=='$d'][0]"); isha=$(q "[x['ImageSha256'] for x in j['Desktops'] if x['Id']=='$d'][0]")
    iso=$(get "$url" "$sha" | tail -1)
    if ! echo "$isha  $R/bits/LiveOS/$d.squashfs" | sha256sum -c --quiet 2>/dev/null; then
        bsdtar -x -O -f "$iso" "$img" > "$R/bits/LiveOS/$d.squashfs.part"
        mv "$R/bits/LiveOS/$d.squashfs.part" "$R/bits/LiveOS/$d.squashfs"
    fi
    echo "$isha  $R/bits/LiveOS/$d.squashfs" | sha256sum -c --quiet || { echo "fetch-release: $d image differs from the table - refusing" >&2; exit 1; }
    echo "fetch-release: $d image verified"
done
echo "fetch-release: $ID ready in $R/bits"
