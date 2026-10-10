#!/usr/bin/env bash
#
# V9 / RISKS R27 - the one-click erase and install, on the Hyper-V rig.
# The guest under test is a COPY of UPGRIGHV.fresh.vhdx (install-day
# Windows, Windows only) at SCSI 0:0 plus a new blank 64 GiB home disk at
# 0:4 (0:1-2 hold empty DVD drives); the real UPGRIGHV.vhdx is swapped out and untouched. Three arms, in
# this order (only the last one erases):
#
#   A  refuse   a job naming a home disk that is not attached, armed by the
#               V0 harness (the prologue would refuse it first, in Windows):
#               verify.sh must refuse before any countdown; both disks unchanged
#   B  cancel   armed by the product prologue (-EraseConsent); a key during the
#               countdown: back to Windows, the prologue's return writes a
#               stopped outcome (stopped_at countdown); both disks unchanged
#   C  erase    armed by the prologue; the countdown left alone: both disks
#               erased, Fedora on the system disk, /home on the home disk,
#               outcome.json completed with the commit line at the countdown's
#               end; the first Linux boot's marker records the account's
#               password fingerprint and where /home lives
#
#   v9.sh prepare          VM off: copy fresh.vhdx -> UPGRIGHV.erase.vhdx, new blank home.vhdx,
#                          attach both (main disk out), Windows Boot Manager first
#   v9.sh stick            MODE=prologue v1.sh stick (bench + autoshutdown, no boot-install)
#   v9.sh windows          power on, wait for PS Direct
#   v9.sh job [--bogus-home] [--spoof-wifi=DIR]   v1.sh job (base job, real facts) -> the guest's disks ->
#                          v9-job.py -> New-Kickstart.ps1 -> both onto the stick
#   v9.sh arm-harness      boot-install marker + v1.sh arm (Test-Handoff -Arm -Auto)
#   v9.sh convert          the product prologue: -Start -StickDrive X: -EraseConsent <sentence>
#   v9.sh watch TAG [s]    screenshots every 10 s into artifacts/v9/TAG/ until Windows answers,
#                          the VM is Off, or s seconds (default 900)
#   v9.sh key              one key press (space) - the cancel
#   v9.sh pull TAG         the records from the stick and the guest (Windows must be up)
#   v9.sh inspect LABEL    v9-inspect.py on both disks (VM off)
#   v9.sh stick-pull TAG   VM off: the stick's records read offline (after the erase there is no Windows)
#   v9.sh verdict ARM [NOTE]  v9-verdict.py (ARM B3 = arm B again) -> docs/validation-results/v9-erase.csv
#   v9.sh stick-first      VM off: the stick first in the firmware's boot order - what the Aspire's firmware
#                          did after its install (run 10, R35). Arm D: start, the stick must hand over to the
#                          installed Fedora and never install again (inspect before-D / after-D, verdict D)
#   v9.sh restore          VM off: main UPGRIGHV.vhdx back at 0:0, erase + home detached
#
# Secure Boot off (Hyper-V template clause). Rows are written by v9-verdict.py,
# never by hand. Rig traps obeyed as in prologue.sh.
set -euo pipefail
SELF=$(readlink -f "${BASH_SOURCE[0]}")
cd "$(dirname "$SELF")"
VMNAME=${VMNAME:-UPGRIGHV}
A=artifacts/v9
HV=/mnt/c/upgrade-rig/hv
MAIN_VHDX_WIN="C:\\upgrade-rig\\hv\\vm\\$VMNAME.vhdx"
FRESH_VHDX_WIN="C:\\upgrade-rig\\hv\\vm\\$VMNAME.fresh.vhdx"
ERA_VHDX="$HV/vm/$VMNAME.erase.vhdx";  ERA_VHDX_WIN="C:\\upgrade-rig\\hv\\vm\\$VMNAME.erase.vhdx"
HOME_VHDX="$HV/vm/$VMNAME.home.vhdx";  HOME_VHDX_WIN="C:\\upgrade-rig\\hv\\vm\\$VMNAME.home.vhdx"
STICK_VHDX="$HV/vm/v1-stick.vhdx"
GUEST_STATE='C:\ProgramData\upgrade_\prologue'
CSV=../../docs/validation-results/v9-erase.csv
FIRMWARE='Hyper-V UEFI Release v4.1'
ERASE_SENTENCE='I confirm that everything on this computer will be deleted and nothing will be kept'

VMPS1="$(wslpath -w vm.ps1)"
PS()  { powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$VMPS1" "$@" -Name "$VMNAME" < /dev/null; }
PSC() { powershell.exe -NoProfile -Command "$1" < /dev/null; }
vm_state() { PSC "(Get-VM $VMNAME).State" | tr -d '\r\n '; }
need_off() { s=$(vm_state); [ "$s" = Off ] || { echo "v9: VM must be Off (state: $s)" >&2; exit 1; }; }
guest() { PS ps "$1" 2>&1 | tr -d '\r'; }
evict() { python3 -c 'import os,sys; fd=os.open(sys.argv[1],os.O_RDONLY); os.posix_fadvise(fd,0,0,os.POSIX_FADV_DONTNEED); os.close(fd)' "$1"; }
stick_letter() { guest '(Get-Volume -FileSystemLabel UPGV0 -ErrorAction SilentlyContinue | Select-Object -First 1).DriveLetter' | tr -d ' \n'; }
windows_up() { guest 'hostname' 2>/dev/null | grep -qx 'UPGRIGHV'; }
wait_windows() { t0=$(date +%s); while :; do windows_up && { echo "v9: Windows up after $(( $(date +%s) - t0 )) s"; return 0; }; [ $(( $(date +%s) - t0 )) -ge "${1:-600}" ] && { echo "v9: no Windows within ${1:-600} s" >&2; return 2; }; sleep 10; done; }
pull() { guest "Get-Content '$1' -Raw -ErrorAction SilentlyContinue" > "$2" 2>/dev/null || true; if [ ! -s "$2" ] || grep -q "Cannot find path" "$2"; then rm -f "$2"; fi; }

case "${1:-}" in
prepare)
    need_off; mkdir -p "$A"
    PSC "Get-VMHardDiskDrive $VMNAME | Where-Object { \$_.Path -notlike '*stick*' } | Remove-VMHardDiskDrive"
    echo "v9: copying fresh.vhdx -> erase.vhdx, and a new blank 64 GiB home disk..."
    PSC "Copy-Item -Path '$FRESH_VHDX_WIN' -Destination '$ERA_VHDX_WIN' -Force; Remove-Item '$HOME_VHDX_WIN' -Force -ErrorAction SilentlyContinue; New-VHD -Path '$HOME_VHDX_WIN' -SizeBytes 64GB -Dynamic | Out-Null"
    evict "$ERA_VHDX" || true
    PSC "Add-VMHardDiskDrive -VMName $VMNAME -ControllerType SCSI -ControllerNumber 0 -ControllerLocation 0 -Path '$ERA_VHDX_WIN'; Add-VMHardDiskDrive -VMName $VMNAME -ControllerType SCSI -ControllerNumber 0 -ControllerLocation 4 -Path '$HOME_VHDX_WIN'"
    PSC "\$fw = Get-VMFirmware -VMName $VMNAME; \$win = \$fw.BootOrder | Where-Object { \$_.FirmwarePath -like '*bootmgfw.efi' } | Select-Object -First 1; \$rest = \$fw.BootOrder | Where-Object { \$_ -ne \$win }; if (\$win) { Set-VMFirmware -VMName $VMNAME -BootOrder (@(\$win) + \$rest) }; 'first: ' + (Get-VMFirmware -VMName $VMNAME).BootOrder[0].FirmwarePath"
    PS disk list
    ;;
stick) MODE=prologue ./v1.sh stick ;;
windows)
    for f in "$STICK_VHDX" "$ERA_VHDX" "$HOME_VHDX" artifacts/v1/stick.img ../../dist/kit/stick/upgrade_/LiveOS/*.squashfs ../../dist/kit/stick/images/install.img; do evict "$f" 2>/dev/null || true; done
    python3 evict-builds.py --wait   # and the build outputs, then wait for room (rig README, 2026-10-08)
    PS start; wait_windows 900 ;;
job)
    mkdir -p "$A"
    ./v1.sh job > "$A/v1-job.log"
    guest 'Get-Disk | ForEach-Object { $n = $_.Number; $p = Get-PhysicalDisk | Where-Object { "$($_.DeviceId)" -eq "$n" } | Select-Object -First 1
  [pscustomobject]@{ number = $n; unique_id = "$($_.UniqueId)"; serial = ("$($_.SerialNumber)" -replace "\s",""); size = [long]$_.Size; name = "$($_.FriendlyName)"; bus = "$($_.BusType)"; health = "$($p.HealthStatus)" } } | ConvertTo-Json -Depth 3' > "$A/disks.json"
    python3 v9-job.py artifacts/v1/job.json "$A/disks.json" "$A/job.json" "${@:2}"
    powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$(wslpath -w ../../upgrade_/windows/New-Kickstart.ps1)" -JobPath "$(wslpath -w "$A/job.json")" -OutFile "$(wslpath -w "$A/ks.cfg")" -StickLabel UPGV0 -Manifest "$(wslpath -w ../../dist/kit/stick/SHA256SUMS)" < /dev/null | tr -d '\r'
    L=$(stick_letter); [ -n "$L" ] || { echo "v9: no UPGV0 volume in the guest" >&2; exit 1; }
    PS copy "$(wslpath -w "$A/job.json")" "${L}:\\upgrade_\\job.json" | tr -d '\r'
    PS copy "$(wslpath -w "$A/ks.cfg")" "${L}:\\upgrade_\\ks.cfg" | tr -d '\r'
    # --spoof-wifi=DIR: the made-up networks' password file goes where the product's export puts one
    for a in "${@:2}"; do case "$a" in --spoof-wifi=*) PS copy "$(wslpath -w "${a#--spoof-wifi=}/01.xml")" "${L}:\\upgrade_\\artifacts\\credentials\\wifi\\01.xml" | tr -d '\r';; esac; done
    guest "Remove-Item ${L}:\\upgrade_\\report -Recurse -Force -ErrorAction SilentlyContinue; Remove-Item ${L}:\\upgrade_\\outcome.json,${L}:\\upgrade_\\prologue.json,${L}:\\upgrade_\\prologue-return.json -Force -ErrorAction SilentlyContinue; Get-ChildItem ${L}:\\upgrade_ | Select-Object Name,Length | Format-Table -AutoSize"
    ;;
arm-harness)
    L=$(stick_letter); [ -n "$L" ] || { echo "v9: no UPGV0 volume" >&2; exit 1; }
    guest "Set-Content -Path ${L}:\\upgrade_\\boot-install -Value 'v9 arm A (harness)'"
    ./v1.sh arm | tee "$A/arm-harness.log"
    ;;
convert)
    L=$(stick_letter); [ -n "$L" ] || { echo "v9: no UPGV0 volume" >&2; exit 1; }
    guest "Remove-Item -Recurse -Force '$GUEST_STATE' -ErrorAction SilentlyContinue"
    if [ "${PROLOGUE:-ps}" = rust ]; then
        guest "& ${L}:\\upgrade-prologue.exe start --stick ${L}: --erase-consent '$ERASE_SENTENCE'" | tee "$A/convert-${2:-x}.log"
    else
        guest "powershell.exe -NoProfile -ExecutionPolicy Bypass -File ${L}:\\Invoke-Prologue.ps1 -Start -StickDrive ${L}: -EraseConsent '$ERASE_SENTENCE'" | tee "$A/convert-${2:-x}.log"
    fi
    grep -q 'restarting in 15 s' "$A/convert-${2:-x}.log" || { echo "v9: the prologue did not reach a restart - read $A/convert-${2:-x}.log" >&2; exit 1; }
    ;;
watch)
    tag=${2:?tag}; limit=${3:-900}; mkdir -p "$A/$tag"; t0=$(date +%s); n=0
    while :; do
        el=$(( $(date +%s) - t0 ))
        PS shot "C:\\upgrade-rig\\hv\\shots\\v9-watch.png" >/dev/null 2>&1 || true
        cp "$HV/shots/v9-watch.png" "$A/$tag/$(printf '%04d' "$el").png" 2>/dev/null || true
        [ "$(vm_state)" = Off ] && { echo "v9: Off after $el s"; break; }
        [ "$el" -ge "$limit" ] && { echo "v9: watch ended at $el s"; break; }
        sleep 10; n=$((n+1))
    done
    ;;
key) PS key 32 ;;
pull)
    tag=${2:?tag}; mkdir -p "$A/$tag"; L=$(stick_letter)
    if [ -n "$L" ]; then
        for f in outcome.json prologue.json prologue-return.json job.json boots.log; do pull "${L}:\\upgrade_\\$f" "$A/$tag/$f"; done
        for f in verify.json verify.log countdown.json storage.ks prologue.log; do pull "${L}:\\upgrade_\\report\\$f" "$A/$tag/$f"; done
    fi
    for f in state-returned.json prologue.log; do pull "$GUEST_STATE\\$f" "$A/$tag/guest-$f"; done
    pull 'C:\upgrade_\v1\v0-handoff.csv' "$A/$tag/v0-handoff.csv"
    ls -la "$A/$tag"
    ;;
inspect) need_off; python3 v9-inspect.py "${2:?label}" "$A" "$ERA_VHDX" "$HOME_VHDX" ;;
stick-pull)
    need_off; tag=${2:?tag}; mkdir -p "$A/$tag"; evict "$STICK_VHDX" || true
    qemu-img convert -f vhdx -O raw "$STICK_VHDX" "$A/stick.raw"
    for f in outcome.json prologue.json boots.log job.json converted boot-install; do mcopy -o -i "$A/stick.raw@@1M" "::/upgrade_/$f" "$A/$tag/$f" 2>/dev/null || true; done
    mdir -b -i "$A/stick.raw@@1M" "::/upgrade_" > "$A/$tag/stick-upgrade-dir.txt" 2>&1 || true   # which markers the stick carries (R35)
    for f in verify.json verify.log countdown.json storage.ks outcome.log anaconda.log storage.log efibootmgr-after.txt; do mcopy -o -i "$A/stick.raw@@1M" "::/upgrade_/report/$f" "$A/$tag/$f" 2>/dev/null || true; done
    # settle-in's first startup, captured by the bench marker (2026-09-27)
    mkdir -p "$A/$tag/settle-in"; mcopy -s -o -i "$A/stick.raw@@1M" "::/upgrade_/settle-in-capture/*" "$A/$tag/settle-in/" 2>/dev/null || true
    mdir -i "$A/stick.raw@@1M" "::/upgrade_/artifacts/credentials" > "$A/$tag/stick-credentials-dir.txt" 2>&1 || true
    rm -f "$A/stick.raw"; ls -la "$A/$tag"
    ;;
stick-first)
    need_off
    PSC "\$d = Get-VMHardDiskDrive $VMNAME | Where-Object { \$_.Path -like '*stick*' }; Set-VMFirmware -VMName $VMNAME -FirstBootDevice \$d; 'first: ' + (Get-VMFirmware -VMName $VMNAME).BootOrder[0].Device.Path"
    ;;
verdict) python3 v9-verdict.py "$A" "${2:?arm}" "$CSV" "$FIRMWARE" ${3:+"$3"} ;;   # $3: a note (was dropped until 2026-09-29)
restore)
    need_off
    PSC "Get-VMHardDiskDrive $VMNAME | Where-Object { \$_.Path -notlike '*stick*' } | Remove-VMHardDiskDrive; Add-VMHardDiskDrive -VMName $VMNAME -ControllerType SCSI -ControllerNumber 0 -ControllerLocation 0 -Path '$MAIN_VHDX_WIN'"
    PS disk list
    ;;
*) sed -n '2,40p' "$SELF"; exit 1 ;;
esac
