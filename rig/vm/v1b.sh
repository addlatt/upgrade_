#!/usr/bin/env bash
#
# V1b / R21 orchestrator — the alongside-install bench on the QEMU rig.
#
#   v1b.sh oemdrv                 build artifacts/v1b/oemdrv.img (FAT, label OEMDRV,
#                                 carries v1b-ks.cfg; Anaconda auto-loads it)
#   v1b.sh autoshutdown on|off    put/remove the 'autoshutdown' flag on OEMDRV
#                                 (both OSes power off after writing their boot marker)
#   v1b.sh inspect LABEL          offline GPT + ESP manifest of win10.qcow2 -> artifacts/v1b/LABEL.json
#   v1b.sh install                boot the Fedora netinst with OEMDRV attached (SB off);
#                                 returns when the kickstart's `poweroff` exits QEMU
#   v1b.sh boot                   a normal post-install boot (run-vm.sh --smb --oemdrv)
#   v1b.sh log                    print OEMDRV:/boots.log
#   v1b.sh verdict                diff the inspections + count boot markers; append ONE
#                                 row to docs/validation-results/v1b-alongside.csv
#
# The VM never runs while inspect/oemdrv/verdict read the images. Everything
# generated stays in artifacts/ (gitignored) except the CSV row.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"
A=artifacts/v1b
OEM=$A/oemdrv.img
ISO=artifacts/payload-bits/fedora-netinst.iso
CSV=../../docs/validation-results/v1b-alongside.csv
HARNESS_VERSION=0.1.0

vm_running() { pgrep -f '^qemu-system-x86_64 -machine' >/dev/null; }
need_stopped() { vm_running && { echo "v1b: the guest is running - stop it first" >&2; exit 1; }; return 0; }

case "${1:-}" in
oemdrv)
    need_stopped
    mkdir -p "$A"
    rm -f "$OEM"
    truncate -s 256M "$OEM"
    parted -s "$OEM" mklabel msdos mkpart primary fat32 1MiB 100%
    mkfs.fat -F 32 -n OEMDRV --offset 2048 "$OEM" >/dev/null
    mcopy -i "$OEM@@1M" v1b-ks.cfg ::/ks.cfg
    echo "v1b: built $OEM with ks.cfg"; mdir -i "$OEM@@1M" ::/
    ;;
autoshutdown)
    need_stopped
    if [ "${2:-}" = on ]; then
        printf 'on\n' > "$A/.flag"; mcopy -o -i "$OEM@@1M" "$A/.flag" ::/autoshutdown; rm -f "$A/.flag"
    else
        mdel -i "$OEM@@1M" ::/autoshutdown 2>/dev/null || true
    fi
    echo "v1b: autoshutdown ${2:-off}"; mdir -i "$OEM@@1M" ::/
    ;;
inspect)
    need_stopped
    python3 v1b-inspect.py artifacts/win10.qcow2 "$2" "$A"
    ;;
install)
    need_stopped
    [ -f "$OEM" ] || { echo "v1b: run 'v1b.sh oemdrv' first" >&2; exit 1; }
    [ -f "$ISO" ] || { echo "v1b: $ISO missing - run fetch-payload-bits.sh" >&2; exit 1; }
    sha256sum -c <(grep fedora-netinst.iso artifacts/payload-bits/fedora-netinst.source.txt | sed 's#payload-bits/#artifacts/payload-bits/#')
    exec ./run-vm.sh --sb off --oemdrv --cdrom "$ISO"
    ;;
boot)
    exec ./run-vm.sh --sb off --smb --oemdrv
    ;;
log)
    mtype -i "$OEM@@1M" ::/boots.log 2>/dev/null || echo "(no boots.log yet)"
    ;;
cycle)
    # One power cycle, hands-free: boot, pick the GRUB entry over QMP, let the
    # OS write its boot marker and power itself off (autoshutdown must be on),
    # then show the row it wrote. $2 = windows|linux; $3 = tag for screenshots.
    need_stopped
    mdir -i "$OEM@@1M" ::/ | grep -qi autoshutdown || { echo "v1b: run 'v1b.sh autoshutdown on' first" >&2; exit 1; }
    tag=${3:-cycle}
    before=$(mtype -i "$OEM@@1M" ::/boots.log 2>/dev/null | wc -l)
    (nohup ./run-vm.sh --sb off --smb --oemdrv > "$A/boot-$tag.log" 2>&1 &)
    # ~4 s in, OVMF's BdsDxe line names the Boot#### it is starting (Fedora or Windows)
    t0=$(date +%s); until [ $(( $(date +%s) - t0 )) -ge 4 ]; do sleep 1; done
    python3 artifacts/qmp.py shot "$A/$tag-bds.png" >/dev/null || true
    t0=$(date +%s); until [ $(( $(date +%s) - t0 )) -ge 8 ]; do sleep 1; done
    python3 artifacts/qmp.py shot "$A/$tag-grub.png" >/dev/null || true
    if [ "$2" = windows ]; then
        python3 artifacts/qmp.py key down >/dev/null; python3 artifacts/qmp.py key down >/dev/null
        python3 artifacts/qmp.py shot "$A/$tag-grub-sel.png" >/dev/null || true
    fi
    python3 artifacts/qmp.py key ret >/dev/null || true
    t0=$(date +%s); until [ $(( $(date +%s) - t0 )) -ge 40 ]; do sleep 2; done
    python3 artifacts/qmp.py shot "$A/$tag-os.png" >/dev/null 2>&1 || true
    pid=$(pgrep -f '^qemu-system-x86_64 -machine' | head -1)
    t0=$(date +%s); until [ -z "$pid" ] || ! kill -0 "$pid" 2>/dev/null || [ $(( $(date +%s) - t0 )) -ge 360 ]; do sleep 3; done
    if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
        python3 artifacts/qmp.py shot "$A/$tag-stuck.png" >/dev/null 2>&1 || true
        echo "v1b: cycle $tag: guest did not power off within 6 min (see $A/$tag-stuck.png)"; exit 2
    fi
    after=$(mtype -i "$OEM@@1M" ::/boots.log 2>/dev/null | wc -l)
    echo "v1b: cycle $tag ($2): rows $before -> $after"
    mtype -i "$OEM@@1M" ::/boots.log | tail -n $(( after - before ))
    ;;
fwmenu)
    # Zero-touch firmware probe: boot with the given run-vm.sh flags, Esc into
    # OVMF setup, open Boot Manager, screenshot the Boot#### list, kill QEMU.
    # Nothing boots. $2 = screenshot tag, $3.. = run-vm.sh flags.
    need_stopped
    tag=$2; shift 2
    (nohup ./run-vm.sh "$@" > "$A/fwmenu-$tag.log" 2>&1 &)
    sleep 1; for _i in $(seq 1 14); do python3 artifacts/qmp.py key esc >/dev/null 2>&1 || true; sleep 0.3; done
    t0=$(date +%s); until [ $(( $(date +%s) - t0 )) -ge 3 ]; do sleep 1; done
    python3 artifacts/qmp.py key down >/dev/null; python3 artifacts/qmp.py key down >/dev/null; python3 artifacts/qmp.py key ret >/dev/null
    sleep 1.5; python3 artifacts/qmp.py shot "$A/fwmenu-$tag.png"
    pkill -f '^qemu-system-x86_64 -machine' || true
    t0=$(date +%s); until ! pgrep -f '^qemu-system-x86_64 -machine' >/dev/null || [ $(( $(date +%s) - t0 )) -ge 20 ]; do sleep 1; done
    echo "v1b: fwmenu $tag -> $A/fwmenu-$tag.png"
    ;;
verdict)
    need_stopped
    python3 v1b-verdict.py "$A" "$CSV" "$HARNESS_VERSION"
    ;;
*)
    sed -n '2,20p' "$0"; exit 1 ;;
esac
