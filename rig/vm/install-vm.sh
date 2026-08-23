#!/usr/bin/env bash
#
# Install-phase boot: creates the OS disk and boots the Windows ISO with the
# unattend ISO attached. Secure Boot OFF (vars-nosb) so nothing interferes
# with setup. Expect ONE attended keypress at "Press any key to boot from
# CD or DVD..." - after that it is hands-off to the desktop (~20-30 min).
#
# Guest hardware is chosen so stock Windows 10 has inbox drivers for
# everything: AHCI disk (q35 ICH9 -> storahci), e1000e NIC, std VGA.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"

[ -f artifacts/win10.iso ]    || { echo "install-vm: run fetch-iso.sh first" >&2; exit 1; }
[ -f artifacts/unattend.iso ] || { echo "install-vm: run make-unattend.sh first" >&2; exit 1; }
[ -f artifacts/fw/vars-nosb.fd ] || { echo "install-vm: run setup.sh first" >&2; exit 1; }

if [ -f artifacts/win10.qcow2 ]; then
    echo "install-vm: artifacts/win10.qcow2 already exists."
    echo "            Delete it explicitly if you really want to reinstall."
    exit 1
fi
qemu-img create -f qcow2 artifacts/win10.qcow2 80G

QEMU=(qemu-system-x86_64
    -machine q35,accel=kvm
    -cpu host,hv_relaxed,hv_vapic,hv_time,hv_spinlocks=0x1fff
    -smp 8 -m 8G -rtc base=localtime
    -drive if=pflash,format=raw,readonly=on,file=artifacts/fw/OVMF_CODE_4M.fd
    -drive if=pflash,format=raw,file=artifacts/fw/vars-nosb.fd
    -drive if=none,id=osdisk,format=qcow2,file=artifacts/win10.qcow2
    -device ide-hd,drive=osdisk,bus=ide.0
    -drive if=none,id=inst,format=raw,readonly=on,file=artifacts/win10.iso
    -device ide-cd,drive=inst,bus=ide.1
    -drive if=none,id=ua,format=raw,readonly=on,file=artifacts/unattend.iso
    -device ide-cd,drive=ua,bus=ide.2
    -netdev user,id=n0 -device e1000e,netdev=n0
    -device qemu-xhci,id=xhci
    -qmp unix:artifacts/qmp.sock,server,nowait
    -vga std -display gtk,gl=off)

if [ -w /dev/kvm ]; then
    exec "${QEMU[@]}"
else
    echo "install-vm: /dev/kvm not writable from this shell - wrapping in 'sg kvm'."
    exec sg kvm -c "$(printf '%q ' "${QEMU[@]}")"
fi
