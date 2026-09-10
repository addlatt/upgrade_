#!/usr/bin/env bash
#
# The conversion itself, keep-windows path, on the Hyper-V rig - the first
# step of the destructive half (architecture.md "Build order"). Same stick and
# same arming as the V1 leg (v1.sh), with /upgrade_/boot-install instead of
# boot-verify: the installer runs the generated kickstart for real - %pre
# verifies identity, reads the image back, snapshots the ESP and writes the
# storage include; Anaconda installs the desktop squashfs into the space
# behind the shrunk C:, reusing the Windows ESP unformatted; %post turns
# os-prober on, runs the boot-chain checklist and writes outcome.json to the
# stick. The guest under test is a COPY of UPGRIGHV.pre-install.vhdx (post-
# shrink, pre-install, BitLocker suspended - the V1b starting state); the
# real UPGRIGHV.vhdx is swapped out and untouched.
#
#   v2.sh prepare      VM off: swap UPGRIGHV.install.vhdx in at SCSI 0:0 (main disk out), Windows first
#   v2.sh stick        MODE=install v1.sh stick (boot-install + bench + autoshutdown markers)
#   v2.sh windows|job|arm   = v1.sh (job.json + ks.cfg onto the stick, arm the one-shot)
#   v2.sh wait-off [s] wait until the guest powers itself off: install -> reboot -> first
#                      Linux boot -> boot marker -> autoshutdown
#   v2.sh inspect LABEL  offline GPT + ESP manifest of the install disk (rig/vm/v1b-inspect.py)
#   v2.sh cycle linux|windows TAG   one power cycle through GRUB: linux = default entry, marker,
#                      autoshutdown; windows = WIN_DOWNS Downs + Enter, the harness's return check
#                      writes its row, then the bench pulls outcome.json/boots.log and stops the VM
#   v2.sh verdict      v2-verdict.py -> docs/validation-results/v2-install.csv
#   v2.sh restore      VM off: main UPGRIGHV.vhdx back at SCSI 0:0, install disk detached
#   v2.sh run          prepare -> stick -> windows -> job -> arm -> wait-off -> inspect post-install
#                      -> cycle windows w1 -> cycle linux l1 -> cycle windows w2 -> cycle linux l2
#                      -> inspect post-cycles -> verdict
#
# Secure Boot off (Hyper-V template clause). Rows are written by v2-verdict.py.
set -euo pipefail
SELF=$(readlink -f "${BASH_SOURCE[0]}")
cd "$(dirname "$SELF")"
VMNAME=${VMNAME:-UPGRIGHV}
A=artifacts/v2
HV=/mnt/c/upgrade-rig/hv
MAIN_VHDX_WIN="C:\\upgrade-rig\\hv\\vm\\$VMNAME.vhdx"
INST_VHDX="$HV/vm/$VMNAME.install.vhdx"
INST_VHDX_WIN="C:\\upgrade-rig\\hv\\vm\\$VMNAME.install.vhdx"
STICK_VHDX_WIN='C:\upgrade-rig\hv\vm\v1-stick.vhdx'
GUEST_CSV='C:\upgrade_\v1\v0-handoff.csv'
CSV=../../docs/validation-results/v2-install.csv
HARNESS_VERSION=0.1.0-hv
FIRMWARE='Hyper-V UEFI Release v4.1'
WIN_DOWNS=${WIN_DOWNS:-2}

VMPS1="$(wslpath -w vm.ps1)"
PS()  { powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$VMPS1" "$@" -Name "$VMNAME" < /dev/null; }
PSC() { powershell.exe -NoProfile -Command "$1" < /dev/null; }
vm_state() { PSC "(Get-VM $VMNAME).State" | tr -d '\r\n '; }
need_off() { s=$(vm_state); [ "$s" = Off ] || { echo "v2: VM must be Off (state: $s)" >&2; exit 1; }; }
guest() { PS ps "$1" 2>&1 | tr -d '\r'; }
stick_letter() { guest '(Get-Volume -FileSystemLabel UPGV0 -ErrorAction SilentlyContinue | Select-Object -First 1).DriveLetter' | tr -d ' \n'; }
shot() { PS shot "C:\\upgrade-rig\\hv\\shots\\v2-$1.png" >/dev/null 2>&1 || true; cp "$HV/shots/v2-$1.png" "$A/" 2>/dev/null || true; }
wait_off() { t0=$(date +%s); while [ "$(vm_state)" != Off ]; do [ $(( $(date +%s) - t0 )) -ge "${1:-3600}" ] && { shot stuck; echo "v2: not Off within ${1:-3600} s" >&2; return 2; }; sleep 15; done; echo "v2: Off after $(( $(date +%s) - t0 )) s"; }
wait_windows() { t0=$(date +%s); while :; do guest 'hostname' 2>/dev/null | grep -qx 'UPGRIGHV' && { echo "v2: Windows up after $(( $(date +%s) - t0 )) s"; return 0; }; [ $(( $(date +%s) - t0 )) -ge "${1:-600}" ] && { shot win-stuck; echo "v2: no Windows within ${1:-600} s" >&2; return 2; }; sleep 10; done; }

case "${1:-}" in
prepare)
    need_off; mkdir -p "$A"
    [ -f "$INST_VHDX" ] || { echo "v2: $INST_VHDX missing - copy UPGRIGHV.pre-install.vhdx to it first" >&2; exit 1; }
    PSC "Get-VMHardDiskDrive $VMNAME | Where-Object { \$_.Path -notlike '*stick*' } | Remove-VMHardDiskDrive; Add-VMHardDiskDrive -VMName $VMNAME -ControllerType SCSI -ControllerNumber 0 -ControllerLocation 0 -Path '$INST_VHDX_WIN'"
    PSC "\$fw = Get-VMFirmware -VMName $VMNAME; \$win = \$fw.BootOrder | Where-Object { \$_.FirmwarePath -like '*bootmgfw.efi' } | Select-Object -First 1; \$rest = \$fw.BootOrder | Where-Object { \$_ -ne \$win }; if (\$win) { Set-VMFirmware -VMName $VMNAME -BootOrder (@(\$win) + \$rest) }; 'first: ' + (Get-VMFirmware -VMName $VMNAME).BootOrder[0].FirmwarePath"
    PS disk list
    ;;
stick)   MODE=install ./v1.sh stick ;;
windows) ./v1.sh windows ;;
job)     ./v1.sh job ;;
arm)     ./v1.sh arm ;;
wait-off) wait_off "${2:-3600}" ;;
inspect)
    need_off; python3 ../vm/v1b-inspect.py "$INST_VHDX" "${2:?label}" "$A" | tail -3
    ;;
cycle)
    need_off; which=${2:?linux|windows}; tag=${3:?tag}
    PS start; sleep 10; shot "grub-$tag"
    if [ "$which" = windows ]; then
        for i in $(seq 1 "$WIN_DOWNS"); do PS key 40; sleep 1; done
        shot "grub-selected-$tag"; PS key 13
        wait_windows 600
        # the harness's return check (armed once, at the conversion) may still
        # be pending on the FIRST Windows boot; give it time, then pull evidence
        t0=$(date +%s); until guest "Test-Path $GUEST_CSV" 2>/dev/null | grep -q True; do [ $(( $(date +%s) - t0 )) -ge 420 ] && break; sleep 15; done
        L=$(stick_letter)
        guest "Get-Content $GUEST_CSV -Raw" > "$A/v0-handoff.csv" 2>/dev/null || true
        for f in outcome.json boots.log; do guest "Get-Content ${L}:\\upgrade_\\$f -Raw -ErrorAction SilentlyContinue" > "$A/$f" 2>/dev/null || true; done
        for f in verify.json verify.log outcome.log efibootmgr-after.txt; do guest "Get-Content ${L}:\\upgrade_\\report\\$f -Raw -ErrorAction SilentlyContinue" > "$A/$f" 2>/dev/null || true; done
        for f in outcome.json boots.log verify.json verify.log outcome.log efibootmgr-after.txt v0-handoff.csv; do if [ ! -s "$A/$f" ] || grep -q "Cannot find path" "$A/$f"; then rm -f "$A/$f"; fi; done
        guest "bcdedit /enum firmware | Select-String 'identifier|description|path'" > "$A/bcd-firmware-$tag.txt" 2>/dev/null || true
        guest "Add-Content -Path ${L}:\\upgrade_\\boots.log -Value ('windows-boot,' + (Get-Date).ToUniversalTime().ToString('o') + ',via-grub')" >/dev/null 2>&1 || true
        PS stop; wait_off 300
    else
        shot "grub-default-$tag"; wait_off 900
    fi
    ;;
verdict)
    mkdir -p "$A"
    python3 v2-verdict.py "$A" "$CSV" "$HARNESS_VERSION" "$FIRMWARE"
    ;;
restore)
    need_off
    PSC "Get-VMHardDiskDrive $VMNAME | Where-Object { \$_.Path -notlike '*stick*' } | Remove-VMHardDiskDrive; Add-VMHardDiskDrive -VMName $VMNAME -ControllerType SCSI -ControllerNumber 0 -ControllerLocation 0 -Path '$MAIN_VHDX_WIN'"
    PS disk list
    ;;
run)
    "$SELF" prepare; "$SELF" stick; "$SELF" windows; "$SELF" job; "$SELF" arm
    "$SELF" wait-off 3600; "$SELF" inspect post-install
    "$SELF" cycle windows w1; "$SELF" cycle linux l1; "$SELF" cycle windows w2; "$SELF" cycle linux l2
    "$SELF" inspect post-cycles; "$SELF" verdict
    ;;
*) sed -n '2,32p' "$SELF"; exit 1 ;;
esac
