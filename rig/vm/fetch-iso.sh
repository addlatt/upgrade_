#!/usr/bin/env bash
#
# Official Windows 10 22H2 x64 consumer ISO (contains Pro) via Fido
# (github.com/pbatard/Fido), run through the host's Windows PowerShell.
# The microsoft.com URL Fido returns expires in ~24 h - download immediately.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/artifacts"

if [ -f win10.iso ]; then
    echo "win10.iso already present - delete it to re-fetch."
    exit 0
fi

curl -Lo Fido.ps1 https://github.com/pbatard/Fido/raw/master/Fido.ps1
URL=$(powershell.exe -NoProfile -ExecutionPolicy Bypass \
      -File "$(wslpath -w "$PWD/Fido.ps1")" \
      -Win 10 -Rel 22H2 -Ed Pro -Lang English -Arch x64 -GetUrl | tr -d '\r' | tail -1)
case "$URL" in
    https://*) ;;
    *) echo "fetch-iso: Fido did not return a URL: $URL" >&2; exit 1 ;;
esac
echo "fetch-iso: downloading $URL"
curl -L -o win10.iso.part "$URL"
mv win10.iso.part win10.iso
ls -lh win10.iso
