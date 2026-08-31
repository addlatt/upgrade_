#!/usr/bin/env bash
#
# V1b / R21 orchestrator — the alongside-install bench on the HYPER-V rig
# (the ~100 MiB ESP row). Mirrors rig/vm/v1b.sh with Hyper-V transport:
# no QMP, no SMB — guest work goes over PowerShell Direct (vm.ps1 ps) and
# Copy-VMFile; OEMDRV is a VHDX converted from a raw FAT image; the
# inspector/verdict are rig/vm's, unchanged logic, reading the VHDX via
# qemu-img. Everything runs with the VM OFF except start/shot/keys.
#
#   v1b.sh oemdrv                 build a FRESH OEMDRV image (ks.cfg = rig/vm/v1b-ks.cfg),
#                                 convert to VHDX and attach it on SCSI
#   v1b.sh autoshutdown on|off    toggle the flag on OEMDRV (detach/edit/re-attach)
#   v1b.sh inspect LABEL          offline GPT+ESP manifest of the guest VHDX -> artifacts/v1b/LABEL.json
#   v1b.sh pull                   OEMDRV VHDX -> artifacts/v1b/oemdrv.img (read-only copy)
#   v1b.sh log                    pull + print boots.log
#   v1b.sh install                DVD=netinst, boot-first dvd, start, pick "Install Fedora 42"
#   v1b.sh wait-off [secs]        poll until the VM is Off (default 3600 s)
#   v1b.sh cycle windows|linux TAG   one hands-free power cycle through GRUB
#   v1b.sh shot TAG               screenshot -> artifacts/v1b/TAG.png
#   v1b.sh fwrec TAG              record vm.ps1 fw output -> artifacts/v1b/fw-TAG.txt
#   v1b.sh verdict                pull + rig/vm/v1b-verdict.py with the Hyper-V firmware string
#
# Evidence rows are written by the verdict script, never by hand (CLAUDE.md).
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"
A=artifacts/v1b
HV=/mnt/c/upgrade-rig/hv
MAIN_VHDX="$HV/vm/UPGRIGHV.vhdx"
OEM_VHDX="$HV/vm/oemdrv.vhdx"
OEM_VHDX_WIN='C:\upgrade-rig\hv\vm\oemdrv.vhdx'
ISO_WIN='C:\upgrade-rig\hv\iso\fedora-netinst.iso'
OEM="$A/oemdrv.img"
KS=../vm/v1b-ks.cfg
CSV=../../docs/validation-results/v1b-alongside.csv
HARNESS_VERSION=0.1.1-hv
FIRMWARE='Hyper-V UEFI Release v4.1'

VMPS1="$(wslpath -w vm.ps1)"
PS() { powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$VMPS1" "$@" < /dev/null; }
PSC() { powershell.exe -NoProfile -Command "$1" < /dev/null; }
vm_state() { PSC '(Get-VM UPGRIGHV).State' | tr -d '\r\n '; }
need_off() { s=$(vm_state); [ "$s" = Off ] || { echo "v1b-hv: VM must be Off (state: $s)" >&2; exit 1; }; }
wait_secs() { t0=$(date +%s); until [ $(( $(date +%s) - t0 )) -ge "$1" ]; do sleep 1; done; }

oem_detach() { PSC "Get-VMHardDiskDrive UPGRIGHV | Where-Object { \$_.Path -eq '$OEM_VHDX_WIN' } | Remove-VMHardDiskDrive"; }
oem_attach() { PSC "Add-VMHardDiskDrive -VMName UPGRIGHV -ControllerType SCSI -Path '$OEM_VHDX_WIN'"; }
# every WSL read of a Windows-written file must evict the 9p page cache first
# (2026-08-30: it served hours-stale VHDX pages; see rig READMEs / RISKS R21)
evict()      { python3 -c 'import os,sys; fd=os.open(sys.argv[1],os.O_RDONLY); os.posix_fadvise(fd,0,0,os.POSIX_FADV_DONTNEED); os.close(fd)' "$1"; }
oem_pull()   { evict "$OEM_VHDX"; qemu-img convert -f vhdx -O raw "$OEM_VHDX" "$OEM"; }
oem_push()   { qemu-img convert -f raw -O vhdx "$OEM" "$OEM_VHDX"; }

case "${1:-}" in
oemdrv)
    need_off
    mkdir -p "$A"
    oem_detach
    rm -f "$OEM"
    truncate -s 256M "$OEM"
    parted -s "$OEM" mklabel msdos mkpart primary fat32 1MiB 100%
    mkfs.fat -F 32 -n OEMDRV --offset 2048 "$OEM" >/dev/null
    mcopy -i "$OEM@@1M" "$KS" ::/ks.cfg
    oem_push
    oem_attach
    echo "v1b-hv: built FRESH $OEM_VHDX (carries ks.cfg) and attached it"
    mdir -i "$OEM@@1M" ::/
    ;;
autoshutdown)
    need_off
    oem_pull
    if [ "${2:-}" = on ]; then
        printf 'on\n' > "$A/.flag"; mcopy -o -i "$OEM@@1M" "$A/.flag" ::/autoshutdown; rm -f "$A/.flag"
    else
        mdel -i "$OEM@@1M" ::/autoshutdown 2>/dev/null || true
    fi
    oem_detach
    oem_push
    oem_attach
    echo "v1b-hv: autoshutdown ${2:-off}"; mdir -i "$OEM@@1M" ::/
    ;;
inspect)
    need_off
    python3 ../vm/v1b-inspect.py "$MAIN_VHDX" "$2" "$A"
    ;;
pull)
    need_off; oem_pull; echo "v1b-hv: pulled OEMDRV -> $OEM"
    ;;
log)
    need_off; oem_pull
    mtype -i "$OEM@@1M" ::/boots.log 2>/dev/null || echo "(no boots.log yet)"
    ;;
install)
    need_off
    [ -f "$OEM_VHDX" ] || { echo "v1b-hv: run 'v1b.sh oemdrv' first" >&2; exit 1; }
    PS dvd "$ISO_WIN" 0
    PS boot-first dvd
    PS start
    # the netinst ISO's GRUB: default entry is "Test this media & install";
    # Up selects "Install Fedora 42", Enter boots it. Screenshot both moments.
    wait_secs 8;  PS shot 'C:\upgrade-rig\hv\shots\v1b-iso-menu.png'
    PS key 38
    PS shot 'C:\upgrade-rig\hv\shots\v1b-iso-menu-sel.png'
    PS key 13
    wait_secs 4;  PS shot 'C:\upgrade-rig\hv\shots\v1b-iso-boot.png'
    cp "$HV"/shots/v1b-iso-*.png "$A/" 2>/dev/null || true
    echo "v1b-hv: installer boot started; kickstart runs hands-off and powers off when done"
    echo "v1b-hv: poll with 'v1b.sh wait-off' and screenshot with 'v1b.sh shot TAG'"
    ;;
wait-off)
    limit=${2:-3600}
    t0=$(date +%s)
    while :; do
        s=$(vm_state)
        [ "$s" = Off ] && { echo "v1b-hv: VM is Off after $(( $(date +%s) - t0 )) s"; exit 0; }
        [ $(( $(date +%s) - t0 )) -ge "$limit" ] && { echo "v1b-hv: still $s after $limit s" >&2; exit 2; }
        sleep 15
    done
    ;;
cycle)
    # One hands-free power cycle. $2 = windows|linux; $3 = tag for screenshots.
    # GRUB (timeout 30 s): default entry is Fedora; Windows is two Down presses
    # away (Fedora / rescue / Windows Boot Manager). The booted OS writes its
    # own marker row to OEMDRV:/boots.log and powers off (autoshutdown flag).
    need_off
    oem_pull
    mdir -i "$OEM@@1M" ::/ | grep -qi autoshutdown || { echo "v1b-hv: run 'v1b.sh autoshutdown on' first" >&2; exit 1; }
    tag=${3:-cycle}
    before=$(mtype -i "$OEM@@1M" ::/boots.log 2>/dev/null | wc -l)
    mkdir -p "$A"
    PS start
    wait_secs 10; PS shot "C:\\upgrade-rig\\hv\\shots\\v1b-$tag-grub.png"
    if [ "$2" = windows ]; then
        PS key 40; PS key 40
        PS shot "C:\\upgrade-rig\\hv\\shots\\v1b-$tag-grub-sel.png"
    fi
    PS key 13
    wait_secs 30; PS shot "C:\\upgrade-rig\\hv\\shots\\v1b-$tag-os1.png" || true
    if [ "$2" = windows ]; then
        wait_secs 45; PS shot "C:\\upgrade-rig\\hv\\shots\\v1b-$tag-os2.png" || true
    fi
    t0=$(date +%s)
    while :; do
        s=$(vm_state)
        [ "$s" = Off ] && break
        if [ $(( $(date +%s) - t0 )) -ge 600 ]; then
            PS shot "C:\\upgrade-rig\\hv\\shots\\v1b-$tag-stuck.png" || true
            cp "$HV"/shots/v1b-$tag-*.png "$A/" 2>/dev/null || true
            echo "v1b-hv: cycle $tag: guest did not power off within 10 min (see $A/v1b-$tag-stuck.png)"; exit 2
        fi
        sleep 10
    done
    cp "$HV"/shots/v1b-$tag-*.png "$A/" 2>/dev/null || true
    oem_pull
    after=$(mtype -i "$OEM@@1M" ::/boots.log 2>/dev/null | wc -l)
    echo "v1b-hv: cycle $tag ($2): rows $before -> $after"
    mtype -i "$OEM@@1M" ::/boots.log | tail -n $(( after - before ))
    ;;
shot)
    PS shot "C:\\upgrade-rig\\hv\\shots\\${2}.png"
    mkdir -p "$A"; cp "$HV/shots/${2}.png" "$A/" 2>/dev/null || true
    ;;
fwrec)
    mkdir -p "$A"
    PS fw | tee "$A/fw-${2:-now}.txt"
    ;;
verdict)
    need_off
    oem_pull
    python3 ../vm/v1b-verdict.py "$A" "$CSV" "$HARNESS_VERSION" "$FIRMWARE" off
    ;;
*)
    sed -n '3,25p' "$0"; exit 1 ;;
esac
