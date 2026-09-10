#!/usr/bin/env bash
#
# V1, reversible half - the live boot through the handoff, on the Hyper-V rig.
#
# One stick (the kit make-kit.sh builds, plus upgrade_/{job.json,ks.cfg,
# boot-verify}), armed by the V0 harness exactly as a person would arm it
# (RUN-TEST.cmd's -Arm -Auto), then: firmware boots the stick -> shim ->
# GRUB records upg_fired and boots Anaconda's stage2 from the stick with
# upg.mode=verify -> %pre runs upgrade_/linux/verify.sh (identity against
# job.json, hardware, storage include) -> report on the stick -> reboot ->
# Windows returns -> the harness's return check writes the V0 row. The
# verdict reads both and writes one row of docs/validation-results/
# v1-live-boot.csv. Nothing is installed; the internal disk is not touched.
#
#   v1.sh stick        VM off: build artifacts/v1/stick.img from dist/kit/stick (+ verify.sh,
#                      boot-verify), convert to VHDX, attach on SCSI
#   v1.sh windows      power on, pick Windows in GRUB (three Downs), wait for PS Direct
#   v1.sh job          Windows up: read the guest's facts, write job.json (v1-job.py, schema-
#                      validated) + ks.cfg (New-Kickstart.ps1), Copy-VMFile both onto the stick
#   v1.sh arm          Windows up: run the stick's Test-Handoff.ps1 -Arm -Auto -SuspendBitLocker;
#                      the guest reboots itself 20 s later
#   v1.sh wait [secs]  poll until Windows is back AND the return check has written its row
#   v1.sh verdict      pull the V0 row and the stick's verify report over PS Direct -> row
#   v1.sh run          stick -> windows -> job -> arm -> wait -> verdict
#
# Secure Boot is OFF on this guest for this leg: Hyper-V's MicrosoftWindows
# template refuses Fedora's shim (rig/hyperv/README.md) - a VM row closes
# plumbing only. Evidence rows are written by v1-verdict.py, never by hand.
set -euo pipefail
SELF=$(readlink -f "${BASH_SOURCE[0]}")
cd "$(dirname "$SELF")"
VMNAME=${VMNAME:-UPGRIGHV}
A=artifacts/v1
HV=/mnt/c/upgrade-rig/hv
KIT=../../dist/kit/stick
STICK_IMG=$A/stick.img
STICK_VHDX="$HV/vm/v1-stick.vhdx"
STICK_VHDX_WIN='C:\upgrade-rig\hv\vm\v1-stick.vhdx'
MAIN_VHDX_WIN="C:\\upgrade-rig\\hv\\vm\\$VMNAME.vhdx"
GUEST_CSV='C:\upgrade_\v1\v0-handoff.csv'
CSV=../../docs/validation-results/v1-live-boot.csv
HARNESS_VERSION=0.2.0-hv
FIRMWARE='Hyper-V UEFI Release v4.1'
ROOT=$(cd ../.. && pwd)

VMPS1="$(wslpath -w vm.ps1)"
PS()  { powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$VMPS1" "$@" -Name "$VMNAME" < /dev/null; }
PSC() { powershell.exe -NoProfile -Command "$1" < /dev/null; }
vm_state() { PSC "(Get-VM $VMNAME).State" | tr -d '\r\n '; }
need_off() { s=$(vm_state); [ "$s" = Off ] || { echo "v1: VM must be Off (state: $s)" >&2; exit 1; }; }
guest() { PS ps "$1" 2>&1 | tr -d '\r'; }
evict() { python3 -c 'import os,sys; fd=os.open(sys.argv[1],os.O_RDONLY); os.posix_fadvise(fd,0,0,os.POSIX_FADV_DONTNEED); os.close(fd)' "$1"; }
stick_letter() { guest '(Get-Volume -FileSystemLabel UPGV0 -ErrorAction SilentlyContinue | Select-Object -First 1).DriveLetter' | tr -d ' \n'; }
wait_windows() {
    t0=$(date +%s)
    while :; do
        # exact match on its own line: a failed Invoke-Command's error text also
        # contains the VM name, which fooled a substring test (2026-09-08)
        if guest 'hostname' 2>/dev/null | grep -qx 'UPGRIGHV'; then echo "v1: Windows up (PS Direct) after $(( $(date +%s) - t0 )) s"; return 0; fi
        [ $(( $(date +%s) - t0 )) -ge "${1:-900}" ] && { PS shot "C:\\upgrade-rig\\hv\\shots\\v1-stuck.png"; echo "v1: Windows did not answer within ${1:-900} s" >&2; return 2; }
        sleep 10
    done
}

case "${1:-}" in
stick)
    need_off; mkdir -p "$A"
    [ -f "$KIT/images/install.img" ] || { echo "v1: run ./make-kit.sh first (needs images/ in the kit)" >&2; exit 1; }
    PSC "Get-VMHardDiskDrive $VMNAME | Where-Object { \$_.Path -eq '$STICK_VHDX_WIN' } | Remove-VMHardDiskDrive" >/dev/null
    rm -f "$STICK_IMG"
    # sized from the kit: its bytes plus room for the report and FAT overhead
    KIT_MB=$(( $(du -sb "$KIT" | cut -f1) / 1048576 + 320 ))
    echo "v1: stick image ${KIT_MB} MiB (kit $(du -sh "$KIT" | cut -f1))"
    truncate -s "${KIT_MB}M" "$STICK_IMG"
    parted -s "$STICK_IMG" mklabel msdos mkpart primary fat32 1MiB 100% set 1 boot on
    mkfs.fat -F 32 -n UPGV0 --offset 2048 "$STICK_IMG" >/dev/null
    P="$STICK_IMG@@1M"
    # the kit, verbatim (make-kit.sh verified it), then the V1 markers
    (cd "$KIT" && find . -type d | sed 's|^\./||' | grep -v '^\.$' | sort | while read -r d; do mmd -i "$OLDPWD/$P" "::/$d" >/dev/null 2>&1 || true; done)
    (cd "$KIT" && find . -type f | sed 's|^\./||' | while read -r f; do mcopy -o -i "$OLDPWD/$P" "$f" "::/$f"; done)
    mmd -i "$P" ::/upgrade_ >/dev/null 2>&1 || true
    # MODE=verify (default): the V1 reversible leg. MODE=install: the
    # conversion itself (v2.sh) - boot-install + the bench marker (GRUB
    # timeout, boot-marker unit) + autoshutdown after the first Linux boot.
    if [ "${MODE:-verify}" = install ]; then
        printf 'v2\n' > "$A/marker"; mcopy -o -i "$P" "$A/marker" ::/upgrade_/boot-install
        mcopy -o -i "$P" "$A/marker" ::/upgrade_/bench; mcopy -o -i "$P" "$A/marker" ::/upgrade_/autoshutdown
    else
        printf 'v1\n' > "$A/boot-verify"; mcopy -o -i "$P" "$A/boot-verify" ::/upgrade_/boot-verify
    fi
    mdir -i "$P" ::/ ; mdir -i "$P" ::/upgrade_ ; mdir -i "$P" ::/images
    rm -f "$STICK_VHDX"; qemu-img convert -f raw -O vhdx "$STICK_IMG" "$STICK_VHDX"
    # WSL keeps the gigabytes just written in its page cache and does not
    # hand them back to Windows; Hyper-V then cannot find memory to start
    # the guest ("Not enough memory in the system", 2026-09-09). Evict.
    for f in "$STICK_IMG" "$KIT"/upgrade_/LiveOS/*.squashfs "$KIT"/images/install.img; do evict "$f" 2>/dev/null || true; done
    PSC "Add-VMHardDiskDrive -VMName $VMNAME -ControllerType SCSI -Path '$STICK_VHDX_WIN'"
    # The machine this leg models has not been converted: Windows Boot Manager
    # is its firmware default. This guest has been dual-booting Fedora-first
    # since the V1b install, so the one-shot's fall-through would land in GRUB
    # (seen 2026-09-08). Put the Windows entry first for this leg.
    PSC "\$fw = Get-VMFirmware -VMName $VMNAME; \$win = \$fw.BootOrder | Where-Object { \$_.FirmwarePath -like '*bootmgfw.efi' } | Select-Object -First 1; \$rest = \$fw.BootOrder | Where-Object { \$_ -ne \$win }; if (\$win) { Set-VMFirmware -VMName $VMNAME -BootOrder (@(\$win) + \$rest); 'boot order: Windows Boot Manager first' } else { 'no Windows Boot Manager entry found' }"
    echo "v1: built and attached $STICK_VHDX"
    ;;
windows)
    PS start; sleep 10
    PS key 40; sleep 1; PS key 40; sleep 1; PS key 40; sleep 1
    PS shot "C:\\upgrade-rig\\hv\\shots\\v1-grub-selected.png" >/dev/null 2>&1 || true; cp "$HV/shots/v1-grub-selected.png" "$A/" 2>/dev/null || true
    PS key 13
    wait_windows 600
    ;;
job)
    mkdir -p "$A"
    guest '$cs=Get-CimInstance Win32_ComputerSystem; $sys=Get-CimInstance Win32_ComputerSystemProduct; $bios=Get-CimInstance Win32_BIOS; $os=Get-CimInstance Win32_OperatingSystem
$p=Get-Partition -DriveLetter C; $d=Get-Disk -Number $p.DiskNumber
$esp=Get-Partition -DiskNumber $d.Number | Where-Object { $_.GptType -eq "{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}" } | Select-Object -First 1
$espVol=$esp | Get-Volume -ErrorAction SilentlyContinue
$sb=try { if (Confirm-SecureBootUEFI) {"on"} else {"off"} } catch {"unknown"}
$bl=try { $v=Get-BitLockerVolume -MountPoint C: -ErrorAction Stop; if ($v.ProtectionStatus -eq "On") {"on"} else {"off"} } catch {"off"}
$shrink=$null; try { $s=Get-PartitionSupportedSize -DriveLetter C -ErrorAction Stop; $shrink=[math]::Round(($p.Size-$s.SizeMin)/1GB,1) } catch {}
$pd=Get-PhysicalDisk | Where-Object { "$($_.DeviceId)" -eq "$($d.Number)" } | Select-Object -First 1
$stick=Get-Volume -FileSystemLabel UPGV0 -ErrorAction SilentlyContinue | Get-Partition -ErrorAction SilentlyContinue | Get-Disk -ErrorAction SilentlyContinue | Select-Object -First 1
[pscustomobject]@{vendor=$cs.Manufacturer; model=$cs.Model; uuid="$($sys.UUID)"; bios_serial="$($bios.SerialNumber)"; bios_version="$($bios.SMBIOSBIOSVersion)"; os=$os.Caption; build=[int]$os.BuildNumber; sb=$sb; bitlocker=$bl; shrink_gb=$shrink; health="$($pd.HealthStatus)"
  disk=@{number=$d.Number; serial=("$($d.SerialNumber)" -replace "\s",""); unique_id="$($d.UniqueId)"; size=[long]$d.Size; style="$($d.PartitionStyle)"; name="$($d.FriendlyName)"}
  esp=@{size=[long]$esp.Size; free=[long]$(if ($espVol) { $espVol.SizeRemaining } else { 0 })}
  stick=@{unique_id="$($stick.UniqueId)"; size=[long]$stick.Size}} | ConvertTo-Json -Depth 4' > "$A/facts.json"
    python3 -c 'import json,sys; json.load(open(sys.argv[1]))' "$A/facts.json" || { echo "v1: facts.json is not JSON:"; cat "$A/facts.json"; exit 1; }
    HASH=$(openssl passwd -6 rig)
    SUID=$(python3 -c 'import json,sys; f=json.load(open(sys.argv[1])); print(f["stick"]["unique_id"])' "$A/facts.json")
    SSIZE=$(python3 -c 'import json,sys; f=json.load(open(sys.argv[1])); print(f["stick"]["size"])' "$A/facts.json")
    python3 v1-job.py "$A/facts.json" "$HASH" "$A/job.json" "$SUID" "$SSIZE"
    powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$(wslpath -w ../../upgrade_/windows/New-Kickstart.ps1)" -JobPath "$(wslpath -w "$A/job.json")" -OutFile "$(wslpath -w "$A/ks.cfg")" -StickLabel UPGV0 -Manifest "$(wslpath -w "$KIT/SHA256SUMS")" < /dev/null | tr -d '\r'
    L=$(stick_letter); [ -n "$L" ] || { echo "v1: no UPGV0 volume in the guest" >&2; exit 1; }
    PS copy "$(wslpath -w "$A/job.json")" "${L}:\\upgrade_\\job.json" | tr -d '\r'
    PS copy "$(wslpath -w "$A/ks.cfg")" "${L}:\\upgrade_\\ks.cfg" | tr -d '\r'
    printf 'REDACTED - rig placeholder, not a key\n' > "$A/bitlocker-C.txt"
    PS copy "$(wslpath -w "$A/bitlocker-C.txt")" "${L}:\\upgrade_\\artifacts\\credentials\\bitlocker-C.txt" | tr -d '\r'
    guest "Get-ChildItem ${L}:\\upgrade_ -Recurse | Select-Object FullName,Length | Format-Table -AutoSize"
    ;;
arm)
    L=$(stick_letter); [ -n "$L" ] || { echo "v1: no UPGV0 volume in the guest" >&2; exit 1; }
    guest 'New-Item -ItemType Directory -Force -Path C:\upgrade_\v1 | Out-Null; Remove-Item C:\upgrade_\v1\v0-handoff.csv -Force -ErrorAction SilentlyContinue; if (Test-Path C:\ProgramData\upgrade_\v0\handoff-state.json) { "stale armed state present - run -Check first" }'
    guest "powershell.exe -NoProfile -ExecutionPolicy Bypass -File ${L}:\\Test-Handoff.ps1 -Arm -Auto -Payload shim -PayloadDrive ${L}: -SuspendBitLocker -ResultsCsv $GUEST_CSV"
    echo "v1: armed; the guest reboots itself in ~20 s"
    ;;
wait)
    # Windows comes back, the logon task runs -Check -Auto, and its one human
    # question (was a key needed?) sits in a popup for up to 5 min before it
    # records 'unknown' and writes the row. The popup ignores the WMI
    # keyboard (tried 2026-09-08), so this just waits; 'unknown' is honest
    # for a rig with nobody at it, and it is not a V1 column.
    limit=${2:-1800}; t0=$(date +%s); sleep 60
    while :; do
        if guest "Test-Path $GUEST_CSV" 2>/dev/null | grep -q True; then echo "v1: return check wrote its row after $(( $(date +%s) - t0 )) s"; break; fi
        [ $(( $(date +%s) - t0 )) -ge "$limit" ] && { PS shot "C:\\upgrade-rig\\hv\\shots\\v1-wait-stuck.png"; cp "$HV/shots/v1-wait-stuck.png" "$A/" || true; echo "v1: no row within $limit s" >&2; exit 2; }
        sleep 15
    done
    PS shot "C:\\upgrade-rig\\hv\\shots\\v1-returned.png" >/dev/null 2>&1 || true; cp "$HV/shots/v1-returned.png" "$A/" 2>/dev/null || true
    ;;
verdict)
    mkdir -p "$A"
    guest "Get-Content $GUEST_CSV -Raw" > "$A/v0-handoff.csv" || true
    L=$(stick_letter)
    if [ -n "$L" ]; then
        guest "Get-Content ${L}:\\upgrade_\\report\\verify.json -Raw -ErrorAction SilentlyContinue" > "$A/verify.json" || true
        guest "Get-Content ${L}:\\upgrade_\\report\\verify.log -Raw -ErrorAction SilentlyContinue" > "$A/verify.log" || true
        guest "Get-Content ${L}:\\upgrade_\\report\\storage.ks -Raw -ErrorAction SilentlyContinue" > "$A/storage.ks" || true
        guest "Get-Content ${L}:\\upgrade_\\report\\lsblk.txt -Raw -ErrorAction SilentlyContinue" > "$A/lsblk.txt" || true
    fi
    for f in verify.json verify.log storage.ks lsblk.txt; do
        # a PS Direct error message is not a report file
        if [ ! -s "$A/$f" ] || grep -q "Cannot find path" "$A/$f"; then rm -f "$A/$f"; fi
    done
    python3 v1-verdict.py "$A" "$CSV" "$HARNESS_VERSION" "$FIRMWARE"
    ;;
grubtest)
    # Debug only: VM off -> boot the STICK first (no handoff involved), screenshot
    # the console every 3 s for 90 s, then put the firmware order back. Shows
    # what the stick's GRUB prints when it fails to start the installer.
    need_off; mkdir -p "$A/grubtest"; rm -f "$A/grubtest"/*.png
    PSC "\$d = Get-VMHardDiskDrive -VMName $VMNAME | Where-Object { \$_.Path -eq '$STICK_VHDX_WIN' }; Set-VMFirmware -VMName $VMNAME -FirstBootDevice \$d"
    PS start
    for i in $(seq 1 30); do sleep 3; PS shot "C:\\upgrade-rig\\hv\\shots\\v1-grubtest-$i.png" >/dev/null 2>&1 || true; cp "$HV/shots/v1-grubtest-$i.png" "$A/grubtest/" 2>/dev/null || true; done
    PS kill || true; sleep 3
    PSC "\$d = Get-VMHardDiskDrive -VMName $VMNAME | Where-Object { \$_.Path -eq '$MAIN_VHDX_WIN' }; Set-VMFirmware -VMName $VMNAME -FirstBootDevice \$d"
    echo "v1: grubtest shots in $A/grubtest (firmware order restored: system disk first)"
    ;;
run)
    "$SELF" stick; "$SELF" windows; "$SELF" job; "$SELF" arm; "$SELF" wait; "$SELF" verdict
    ;;
resume)
    # Windows already booting/up with the stick attached: the rest of run
    wait_windows 600; "$SELF" job; "$SELF" arm; "$SELF" wait; "$SELF" verdict
    ;;
*)
    sed -n '2,30p' "$SELF"; exit 1 ;;
esac
