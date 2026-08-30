#!/usr/bin/env bash
# Wrap the Hyper-V variant of autounattend.xml into an ISO on the Windows
# side (Hyper-V cannot mount files under \\wsl.localhost).
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"
OUT=/mnt/c/upgrade-rig/hv/iso/unattend-hv.iso
mkdir -p "$(dirname "$OUT")"
genisoimage -quiet -o "$OUT" -V UNATTEND -J -r autounattend.xml
echo "make-unattend: wrote $OUT"
