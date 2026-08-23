#!/usr/bin/env bash
#
# Gather the EFI payload binaries (gitignored build inputs, per the repo's
# handoff-payload policy):
#   Shell.efi     - EDK2 UEFI Shell, unsigned payload (Ubuntu package)
#   shimx64.efi   - Fedora's signed shim  \  from a Fedora netinst ISO's
#   grubx64.efi   - Fedora's signed grub  /  EFI/BOOT (see note below)
#
# IMPORTANT: the shim payload's grub MUST come from install media, not from
# the plain grub2-efi-x64 RPM. The RPM binary has embedded prefix /EFI/fedora
# and never reads EFI/BOOT/grub.cfg - it drops to a grub prompt. The install
# media binary (grub2-efi-x64-cdboot build) reads ()/EFI/BOOT/grub.cfg, which
# is what the payload layout in handoff-payload/README.md relies on.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/artifacts"
mkdir -p payload-bits

# --- UEFI Shell (unsigned) - probe both packaging layouts ------------------
if [ ! -f payload-bits/Shell.efi ]; then
    found=
    for p in /usr/share/efi-shell-x64/shellx64.efi \
             /usr/share/edk2-shell/x64/Shell.efi; do
        if [ -f "$p" ]; then cp "$p" payload-bits/Shell.efi; found=1; break; fi
    done
    [ -n "$found" ] || { echo "fetch-payload-bits: no EFI shell found - run setup.sh (installs efi-shell-x64)" >&2; exit 1; }
fi

# --- Fedora signed chain from netinst media --------------------------------
if [ ! -f payload-bits/shimx64.efi ] || [ ! -f payload-bits/grubx64.efi ]; then
    if [ ! -f payload-bits/fedora-netinst.iso ]; then
        for base in \
            "${FEDORA_BASE:-}" \
            https://dl.fedoraproject.org/pub/fedora/linux/releases/42/Server/x86_64/iso \
            https://archives.fedoraproject.org/pub/archive/fedora/linux/releases/42/Server/x86_64/iso; do
            [ -n "$base" ] || continue
            iso=$(curl -sL "$base/" | grep -oE 'Fedora-Server-netinst-x86_64-[0-9._-]+\.iso' | head -1 || true)
            if [ -n "$iso" ]; then
                echo "fetch-payload-bits: downloading $base/$iso"
                curl -L -o payload-bits/fedora-netinst.iso.part "$base/$iso"
                mv payload-bits/fedora-netinst.iso.part payload-bits/fedora-netinst.iso
                echo "$base/$iso" > payload-bits/fedora-netinst.source.txt
                sha256sum payload-bits/fedora-netinst.iso >> payload-bits/fedora-netinst.source.txt
                break
            fi
        done
        [ -f payload-bits/fedora-netinst.iso ] || { echo "fetch-payload-bits: could not locate a Fedora netinst ISO; set FEDORA_BASE" >&2; exit 1; }
    fi
    bsdtar -x -f payload-bits/fedora-netinst.iso -C payload-bits \
        --include 'EFI/BOOT/BOOTX64.EFI' --include 'EFI/BOOT/grubx64.efi'
    mv payload-bits/EFI/BOOT/BOOTX64.EFI payload-bits/shimx64.efi
    mv payload-bits/EFI/BOOT/grubx64.efi payload-bits/grubx64.efi
    rmdir payload-bits/EFI/BOOT payload-bits/EFI 2>/dev/null || true
fi

echo "fetch-payload-bits: done."
ls -l payload-bits/*.efi
