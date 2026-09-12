#!/usr/bin/env bash
#
# The prologue as product code, on the Hyper-V rig (RISKS R18 step 1b, the
# shrink, the handoff): the one-click flow RUN-CONVERT.cmd drives on a real
# machine, run against a copy of UPGRIGHV.fresh.vhdx - the UNSHRUNK
# install-day disk (C: 85.8 GB, 100 MiB ESP, Windows only, BitLocker off).
# Not pre-install.vhdx: that one was shrunk by hand for the V1b bench and
# already carries a 32 GiB gap, so a prologue run on it would never have
# to shrink anything. Fault injection: `fsutil dirty set C:` before the
# flow, so the prologue must confirm the flag by the online scan, read the
# disk-health guardrail, schedule the spot-fix, restart, read the check's
# outcome, re-measure by both paths, take the fork, shrink C:, arm the
# handoff and restart into the installer - then the install runs exactly
# as in v2.sh and the existing v2-verdict writes its row too.
#
#   prologue.sh prepare       VM off: copy fresh.vhdx -> UPGRIGHV.prologue.vhdx (Windows side),
#                             swap it in at SCSI 0:0 (main disk out), Windows Boot Manager first
#   prologue.sh stick         MODE=prologue v1.sh stick (bench + autoshutdown markers; NO boot-install -
#                             the prologue writes that itself when it arms)
#   prologue.sh windows       power on, wait for PS Direct (no GRUB on this disk)
#   prologue.sh dirty         fsutil dirty set C: (the fault), record fsutil's answer
#   prologue.sh convert       run the stick's RUN-CONVERT.cmd through cmd with CONVERT on stdin;
#                             the prologue restarts the guest for the disk check
#   prologue.sh wait-off [s]  wait until the guest powers itself off: check restart -> resume ->
#                             shrink -> arm -> installer -> first Linux boot -> autoshutdown
#                             (screenshots every 60 s into artifacts/prologue/progress/)
#   prologue.sh inspect LABEL offline GPT + ESP manifest of the prologue disk
#   prologue.sh cycle linux|windows TAG   as v2.sh; the windows cycle waits for the prologue's
#                             own return record (prologue-return.json) and pulls every record
#   prologue.sh verdict       prologue-verdict.py -> docs/validation-results/r18-prologue.csv,
#                             then v2-verdict.py -> v2-install.csv (the install row)
#   prologue.sh rollback      VM off, after the cycles: Windows via GRUB, ROLLBACK.cmd, record pulled,
#                             ESP inspected offline, then a keyless start must bring Windows up
#                             directly -> rollback-verdict.py -> r21-rollback.csv
#   prologue.sh restore       VM off: main UPGRIGHV.vhdx back, prologue disk detached
#   prologue.sh run           prepare -> stick -> windows -> inspect pre -> dirty -> convert -> wait-off
#                             -> inspect post-install -> cycle windows w1 -> cycle linux l1
#                             -> cycle windows w2 -> cycle linux l2 -> inspect post-cycles -> verdict
#
# Secure Boot off (Hyper-V template clause). Rows are written by the verdict
# scripts, never by hand. Rig traps obeyed: PS Direct "up" probe matches the
# hostname exactly; shell values never interpolated into Python; evict the
# page cache before every WSL read of a Windows-written file.
set -euo pipefail
SELF=$(readlink -f "${BASH_SOURCE[0]}")
cd "$(dirname "$SELF")"
VMNAME=${VMNAME:-UPGRIGHV}
A=artifacts/prologue
HV=/mnt/c/upgrade-rig/hv
MAIN_VHDX_WIN="C:\\upgrade-rig\\hv\\vm\\$VMNAME.vhdx"
FRESH_VHDX_WIN="C:\\upgrade-rig\\hv\\vm\\$VMNAME.fresh.vhdx"
PRO_VHDX="$HV/vm/$VMNAME.prologue.vhdx"
PRO_VHDX_WIN="C:\\upgrade-rig\\hv\\vm\\$VMNAME.prologue.vhdx"
GUEST_STATE='C:\ProgramData\upgrade_\prologue'
CSV=../../docs/validation-results/r18-prologue.csv
CSV_V2=../../docs/validation-results/v2-install.csv
HARNESS_VERSION=0.1.0-hv
FIRMWARE='Hyper-V UEFI Release v4.1'
WIN_DOWNS=${WIN_DOWNS:-2}

VMPS1="$(wslpath -w vm.ps1)"
PS()  { powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$VMPS1" "$@" -Name "$VMNAME" < /dev/null; }
PSC() { powershell.exe -NoProfile -Command "$1" < /dev/null; }
vm_state() { PSC "(Get-VM $VMNAME).State" | tr -d '\r\n '; }
need_off() { s=$(vm_state); [ "$s" = Off ] || { echo "prologue: VM must be Off (state: $s)" >&2; exit 1; }; }
guest() { PS ps "$1" 2>&1 | tr -d '\r'; }
evict() { python3 -c 'import os,sys; fd=os.open(sys.argv[1],os.O_RDONLY); os.posix_fadvise(fd,0,0,os.POSIX_FADV_DONTNEED); os.close(fd)' "$1"; }
stick_letter() { guest '(Get-Volume -FileSystemLabel UPGV0 -ErrorAction SilentlyContinue | Select-Object -First 1).DriveLetter' | tr -d ' \n'; }
shot() { PS shot "C:\\upgrade-rig\\hv\\shots\\prologue-$1.png" >/dev/null 2>&1 || true; cp "$HV/shots/prologue-$1.png" "$A/" 2>/dev/null || true; }
wait_off() {
    t0=$(date +%s); mkdir -p "$A/progress"; n=0
    while [ "$(vm_state)" != Off ]; do
        [ $(( $(date +%s) - t0 )) -ge "${1:-5400}" ] && { shot stuck; echo "prologue: not Off within ${1:-5400} s" >&2; return 2; }
        sleep 15; n=$((n+1))
        if [ $((n % 4)) -eq 0 ]; then PS shot "C:\\upgrade-rig\\hv\\shots\\prologue-progress.png" >/dev/null 2>&1 || true; cp "$HV/shots/prologue-progress.png" "$A/progress/$(printf '%04d' $((n/4))).png" 2>/dev/null || true; fi
    done
    echo "prologue: Off after $(( $(date +%s) - t0 )) s"
}
wait_windows() { t0=$(date +%s); while :; do guest 'hostname' 2>/dev/null | grep -qx 'UPGRIGHV' && { echo "prologue: Windows up after $(( $(date +%s) - t0 )) s"; return 0; }; [ $(( $(date +%s) - t0 )) -ge "${1:-600}" ] && { shot win-stuck; echo "prologue: no Windows within ${1:-600} s" >&2; return 2; }; sleep 10; done; }
pull() {  # $1 guest path, $2 local name
    guest "Get-Content '$1' -Raw -ErrorAction SilentlyContinue" > "$A/$2" 2>/dev/null || true
    if [ ! -s "$A/$2" ] || grep -q "Cannot find path" "$A/$2"; then rm -f "$A/$2"; fi
}
pull_all() {
    L=$(stick_letter)
    if [ -n "$L" ]; then
        for f in prologue.json prologue-return.json outcome.json boots.log job.json ks.cfg; do pull "${L}:\\upgrade_\\$f" "$f"; done
        for f in prologue.log verify.json verify.log outcome.log efibootmgr-after.txt; do pull "${L}:\\upgrade_\\report\\$f" "$f"; done
    fi
    for f in state.json state-stopped.json state-returned.json prologue.log; do pull "$GUEST_STATE\\$f" "guest-$f"; done
    guest "bcdedit /enum firmware | Select-String 'identifier|description|path'" > "$A/bcd-firmware-${1:-x}.txt" 2>/dev/null || true
}

case "${1:-}" in
prepare)
    need_off; mkdir -p "$A"
    [ -f "$HV/vm/$VMNAME.fresh.vhdx" ] || { echo "prologue: $VMNAME.fresh.vhdx missing" >&2; exit 1; }
    PSC "Get-VMHardDiskDrive $VMNAME | Where-Object { \$_.Path -notlike '*stick*' } | Remove-VMHardDiskDrive"
    echo "prologue: copying fresh.vhdx -> prologue.vhdx on the Windows side..."
    PSC "Copy-Item -Path '$FRESH_VHDX_WIN' -Destination '$PRO_VHDX_WIN' -Force"
    evict "$PRO_VHDX" || true
    PSC "Add-VMHardDiskDrive -VMName $VMNAME -ControllerType SCSI -ControllerNumber 0 -ControllerLocation 0 -Path '$PRO_VHDX_WIN'"
    PSC "\$fw = Get-VMFirmware -VMName $VMNAME; \$win = \$fw.BootOrder | Where-Object { \$_.FirmwarePath -like '*bootmgfw.efi' } | Select-Object -First 1; \$rest = \$fw.BootOrder | Where-Object { \$_ -ne \$win }; if (\$win) { Set-VMFirmware -VMName $VMNAME -BootOrder (@(\$win) + \$rest) }; 'first: ' + (Get-VMFirmware -VMName $VMNAME).BootOrder[0].FirmwarePath"
    PS disk list
    ;;
stick)   MODE=prologue ./v1.sh stick ;;
windows) PS start; wait_windows 900 ;;
dirty)
    mkdir -p "$A"
    guest 'fsutil dirty set C:; fsutil dirty query C:' | tee "$A/dirty.txt"
    grep -q 'is Dirty' "$A/dirty.txt" || { echo "prologue: the flag did not set" >&2; exit 1; }
    ;;
convert)
    mkdir -p "$A"
    L=$(stick_letter); [ -n "$L" ] || { echo "prologue: no UPGV0 volume in the guest" >&2; exit 1; }
    guest "Remove-Item -Recurse -Force '$GUEST_STATE' -ErrorAction SilentlyContinue; Remove-Item ${L}:\\upgrade_\\prologue.json,${L}:\\upgrade_\\prologue-return.json,${L}:\\upgrade_\\outcome.json -Force -ErrorAction SilentlyContinue"
    # Hyper-V has no USB: the product's job writer refuses this SCSI "stick" (bus
    # SAS) as it must (R16; seen 2026-09-12, artifacts/prologue/convert-runconvert-
    # refused.log). So steps 1-3 of RUN-CONVERT.cmd are the rig's schema-validated
    # job stand-in (v1.sh job -> v1-job.py + New-Kickstart.ps1, as V1/V2 did) and
    # step 5 - the prologue, the code under test - runs exactly as the launcher
    # runs it. The typed word is the launcher's; here it is passed straight.
    ./v1.sh job
    guest "powershell.exe -NoProfile -ExecutionPolicy Bypass -File ${L}:\\Invoke-Prologue.ps1 -Start -StickDrive ${L}: -ConfirmWord CONVERT" | tee "$A/convert.log"
    grep -q 'restarting in 15 s' "$A/convert.log" || { echo "prologue: the prologue did not reach a restart - read $A/convert.log" >&2; exit 1; }
    ;;
wait-off) wait_off "${2:-5400}" ;;
inspect)
    need_off; python3 ../vm/v1b-inspect.py "$PRO_VHDX" "${2:?label}" "$A" | tail -3
    ;;
cycle)
    need_off; which=${2:?linux|windows}; tag=${3:?tag}
    PS start; sleep 10; shot "grub-$tag"
    if [ "$which" = windows ]; then
        for i in $(seq 1 "$WIN_DOWNS"); do PS key 40; sleep 1; done
        shot "grub-selected-$tag"; PS key 13
        wait_windows 600
        # the prologue's return check runs at logon and leaves its record on the stick
        L=$(stick_letter); t0=$(date +%s)
        until guest "Test-Path ${L}:\\upgrade_\\prologue-return.json" 2>/dev/null | grep -q True; do [ $(( $(date +%s) - t0 )) -ge 420 ] && break; sleep 15; done
        guest "Add-Content -Path ${L}:\\upgrade_\\boots.log -Value ('windows-boot,' + (Get-Date).ToUniversalTime().ToString('o') + ',via-grub,BootCurrent=' + ((bcdedit /enum '{fwbootmgr}' | Select-String 'bootsequence|displayorder' | Select-Object -First 1) -replace '\\s+',' '))" >/dev/null 2>&1 || true
        pull_all "$tag"
        PS stop; wait_off 300
    else
        shot "grub-default-$tag"; wait_off 900
    fi
    ;;
pull) mkdir -p "$A"; pull_all "${2:-manual}"; ls -la "$A" ;;
rollback)
    # VM off with the converted disk in place (after the cycles): boot Windows through
    # GRUB, run the stick's ROLLBACK.cmd (ROLLBACK on stdin), pull its record, power off;
    # inspect the ESP offline; then start with NO key pressed - the firmware must bring
    # Windows up by itself - mark it, power off; verdict -> r21-rollback.csv
    need_off; PS start; sleep 10; shot grub-rollback
    for i in $(seq 1 "$WIN_DOWNS"); do PS key 40; sleep 1; done; PS key 13
    wait_windows 600; L=$(stick_letter); [ -n "$L" ] || { echo "prologue: no UPGV0 volume" >&2; exit 1; }
    guest "cmd /c \"echo ROLLBACK| ${L}:\\ROLLBACK.cmd\"" | tee "$A/rollback.log"
    pull "${L}:\\upgrade_\\rollback.json" rollback.json
    guest "bcdedit /enum '{fwbootmgr}'" > "$A/bcd-fwbootmgr-after-rollback.txt" 2>/dev/null || true
    PS stop; wait_off 300
    "$SELF" inspect post-rollback
    PS start; t0=$(date +%s)
    until guest 'hostname' 2>/dev/null | grep -qx 'UPGRIGHV'; do
        [ "$(vm_state)" = Off ] && { echo "prologue: the guest powered off instead of reaching Windows (GRUB default?)" >&2; break; }
        [ $(( $(date +%s) - t0 )) -ge 600 ] && { shot rollback-stuck; echo "prologue: no Windows within 600 s" >&2; break; }
        sleep 10
    done
    if guest 'hostname' 2>/dev/null | grep -qx 'UPGRIGHV'; then
        shot windows-direct
        guest "Add-Content -Path ${L}:\\upgrade_\\boots.log -Value ('windows-boot,' + (Get-Date).ToUniversalTime().ToString('o') + ',direct-after-rollback,BootCurrent=' + ((bcdedit /enum '{fwbootmgr}' | Select-String 'displayorder' | Select-Object -First 1) -replace '\\s+',' '))" >/dev/null 2>&1 || true
        pull "${L}:\\upgrade_\\boots.log" boots.log
        PS stop; wait_off 300
    fi
    python3 rollback-verdict.py "$A" ../../docs/validation-results/r21-rollback.csv "$HARNESS_VERSION" "$FIRMWARE"
    ;;
verdict)
    mkdir -p "$A"
    python3 prologue-verdict.py "$A" "$CSV" "$HARNESS_VERSION" "$FIRMWARE" || true
    python3 v2-verdict.py "$A" "$CSV_V2" "$HARNESS_VERSION-prologue" "$FIRMWARE"
    ;;
restore)
    need_off
    PSC "Get-VMHardDiskDrive $VMNAME | Where-Object { \$_.Path -notlike '*stick*' } | Remove-VMHardDiskDrive; Add-VMHardDiskDrive -VMName $VMNAME -ControllerType SCSI -ControllerNumber 0 -ControllerLocation 0 -Path '$MAIN_VHDX_WIN'"
    PS disk list
    ;;
run)
    "$SELF" prepare; "$SELF" stick; "$SELF" inspect pre-install; "$SELF" windows; "$SELF" dirty; "$SELF" convert
    "$SELF" wait-off 5400; "$SELF" inspect post-install
    "$SELF" cycle windows w1; "$SELF" cycle linux l1; "$SELF" cycle windows w2; "$SELF" cycle linux l2
    "$SELF" inspect post-cycles; "$SELF" verdict
    ;;
*) sed -n '2,42p' "$SELF"; exit 1 ;;
esac
