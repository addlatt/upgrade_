#!/usr/bin/env bash
#
# Build the two V0 payload stick images as MBR-partitioned FAT32 raw images.
#
# MBR (not a partitionless "superfloppy") on purpose: `bcdedit /set {guid}
# device partition=E:` stores the MBR disk signature + partition offset in the
# firmware entry's device path. A superfloppy has no MBR, so the entry Windows
# writes may be a shape OVMF cannot re-resolve at boot - which would fabricate
# a false 'ignored'. MBR also matches the physical-stick layout that
# upgrade_/windows/handoff-payload/README.md prescribes (New-Partition ->
# single partition), so the VM leg exercises the same shape as the physical
# matrix. Built with parted + mkfs.fat --offset + mtools; no loop devices.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"
NSH=../../upgrade_/windows/handoff-payload/startup.nsh
BITS=artifacts/payload-bits

[ -f "$BITS/Shell.efi" ] || { echo "make-stick: run fetch-payload-bits.sh first" >&2; exit 1; }

build_stick() {  # $1 = output image, $2 = variant: shell|shim
    local img=$1 variant=$2
    rm -f "$img"
    truncate -s 128M "$img"
    parted -s "$img" mklabel msdos mkpart primary fat32 1MiB 100% set 1 boot on
    mkfs.fat -F 32 -n UPGV0 --offset 2048 "$img" >/dev/null
    local P="$img@@1M"
    mmd -i "$P" ::/EFI ::/EFI/BOOT
    if [ "$variant" = shell ]; then
        mcopy -i "$P" "$BITS/Shell.efi" ::/EFI/BOOT/BOOTX64.EFI
        # the repo's canonical startup.nsh, copied verbatim
        mcopy -i "$P" "$NSH" ::/startup.nsh
    else
        [ -f "$BITS/shimx64.efi" ] || { echo "make-stick: shim bits missing - run fetch-payload-bits.sh" >&2; exit 1; }
        mcopy -i "$P" "$BITS/shimx64.efi" ::/EFI/BOOT/BOOTX64.EFI
        mcopy -i "$P" "$BITS/grubx64.efi" ::/EFI/BOOT/grubx64.efi
        printf 'reboot\n' > "$BITS/grub.cfg"
        mcopy -i "$P" "$BITS/grub.cfg" ::/EFI/BOOT/grub.cfg
    fi
    echo "make-stick: built $img ($variant)"
}

build_stick artifacts/stick-shell.img shell
if [ -f "$BITS/shimx64.efi" ]; then
    build_stick artifacts/stick-shim.img shim
else
    echo "make-stick: skipping shim stick (bits not fetched yet)"
fi
