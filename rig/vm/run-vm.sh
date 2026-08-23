#!/usr/bin/env bash
#
# Every post-install boot of the rig guest. Never attaches the install
# cdroms.
#
#   run-vm.sh [--sb on|off] [--stick shell|shim|none] [--tpm] [--smb]
#             [--vmd] [--reset-vars]
#
#   --sb on|off    Secure Boot profile: which OVMF VARS copy is attached
#                  (vars-ms.fd = MS keys enrolled = SB on; default off).
#   --stick X      Attach artifacts/stick-X.img as a USB stick (usb-storage
#                  on xhci). Default none.
#   --tpm          Start swtpm (TPM 2.0) and attach it. State persists in
#                  artifacts/tpm - required for the BitLocker matrix runs,
#                  and once BitLocker is enabled, keep attaching it.
#   --smb          Share the repo checkout as \\10.0.2.4\qemu inside the
#                  guest (qemu spawns smbd; needs the samba package). Lets
#                  Test-Handoff.ps1 -Check write its CSV row straight into
#                  the host repo via -ResultsCsv.
#   --vmd          Use the patched QEMU (build-qemu-vmd.sh) and attach the
#                  spoofed Intel VMD device (8086:9a0b, class 0104).
#                  SPOOF ONLY - never evidence for a real-hardware clause.
#   --reset-vars   Re-copy the selected VARS profile from /usr/share/OVMF
#                  first (factory-reset that profile's UEFI variables).
#
# IMPORTANT: keep ONE invocation running through a whole arm -> reboot ->
# check cycle. OVMF re-enumerates Boot#### entries when the attached device
# set changes between boots, which Test-Handoff.ps1 would misreport as
# 'reordered'. Change flags only between cycles, never mid-cycle.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"

SB=off STICK=none TPM=0 SMB=0 VMD=0 RESET=0
while [ $# -gt 0 ]; do
    case "$1" in
        --sb)    SB=$2; shift 2 ;;
        --stick) STICK=$2; shift 2 ;;
        --tpm)   TPM=1; shift ;;
        --smb)   SMB=1; shift ;;
        --vmd)   VMD=1; shift ;;
        --reset-vars) RESET=1; shift ;;
        *) echo "run-vm: unknown flag $1" >&2; exit 1 ;;
    esac
done

case "$SB" in
    on)  VARS=artifacts/fw/vars-ms.fd;   SRC=/usr/share/OVMF/OVMF_VARS_4M.ms.fd ;;
    off) VARS=artifacts/fw/vars-nosb.fd; SRC=/usr/share/OVMF/OVMF_VARS_4M.fd ;;
    *) echo "run-vm: --sb must be on or off" >&2; exit 1 ;;
esac
[ "$RESET" = 1 ] && cp "$SRC" "$VARS"

QEMU_BIN=qemu-system-x86_64
if [ "$VMD" = 1 ]; then
    QEMU_BIN=artifacts/qemu-8.2.2/build/qemu-system-x86_64
    [ -x "$QEMU_BIN" ] || { echo "run-vm: patched qemu missing - run build-qemu-vmd.sh" >&2; exit 1; }
fi

NETDEV="user,id=n0"
if [ "$SMB" = 1 ]; then
    REPO=$(cd ../.. && pwd)
    NETDEV="user,id=n0,smb=$REPO"
fi

ARGS=(
    -machine q35,accel=kvm
    -cpu host,hv_relaxed,hv_vapic,hv_time,hv_spinlocks=0x1fff
    -smp 8 -m 8G -rtc base=localtime
    -drive if=pflash,format=raw,readonly=on,file=artifacts/fw/OVMF_CODE_4M.fd
    -drive if=pflash,format=raw,file="$VARS"
    -drive if=none,id=osdisk,format=qcow2,file=artifacts/win10.qcow2
    -device ide-hd,drive=osdisk,bus=ide.0
    -netdev "$NETDEV" -device e1000e,netdev=n0
    -device qemu-xhci,id=xhci
    -qmp unix:artifacts/qmp.sock,server,nowait
    -vga std -display gtk,gl=off
)

if [ "$STICK" != none ]; then
    IMG=artifacts/stick-$STICK.img
    [ -f "$IMG" ] || { echo "run-vm: $IMG missing - run make-stick.sh" >&2; exit 1; }
    ARGS+=(-drive if=none,id=stick,format=raw,file="$IMG"
           -device usb-storage,bus=xhci.0,drive=stick,removable=on)
fi

if [ "$TPM" = 1 ]; then
    mkdir -p artifacts/tpm
    # swtpm must be (re)started before every boot; its state dir persists so
    # the TPM keeps its seed across runs (BitLocker depends on that).
    pkill -f "tpmstate dir=$PWD/artifacts/tpm" 2>/dev/null || true
    swtpm socket --tpm2 --tpmstate dir="$PWD/artifacts/tpm" \
        --ctrl type=unixio,path="$PWD/artifacts/tpm/swtpm.sock" --daemon
    ARGS+=(-chardev socket,id=chrtpm,path="$PWD/artifacts/tpm/swtpm.sock"
           -tpmdev emulator,id=tpm0,chardev=chrtpm -device tpm-tis,tpmdev=tpm0)
fi

if [ "$VMD" = 1 ]; then
    ARGS+=(-device pci-testdev)
fi

if [ -w /dev/kvm ]; then
    exec "$QEMU_BIN" "${ARGS[@]}"
else
    echo "run-vm: /dev/kvm not writable from this shell - wrapping in 'sg kvm'."
    exec sg kvm -c "$(printf '%q ' "$QEMU_BIN" "${ARGS[@]}")"
fi
