#!/usr/bin/env bash
#
# V3 / R19 orchestrator — the cryptsetup BITLK read bench on the Hyper-V rig.
# Transport: no SMB, no QMP. Guest work on the Fedora side goes through the
# OEMDRV run hook (guest/oemdrv-run.sh: the host writes OEMDRV:/run.sh, the
# guest runs it as root at boot and leaves OEMDRV:/run.log); Windows-side
# work goes over PowerShell Direct (vm.ps1 ps) and Copy-VMFile. The host
# reads the OEMDRV VHDX with qemu-img AFTER evicting the 9p page cache.
#
#   v3.sh oemdrv <run.sh> [extra files...]   build a FRESH OEMDRV-v3 VHDX carrying
#                                 run.sh + the hook/bootstrap + v3/key.txt (the
#                                 CURRENT recovery password from the host-side
#                                 key file — never committed, removed by the
#                                 guest script), detach any other OEMDRV, attach it
#   v3.sh rearm <run.sh>          put run.sh + key back on the EXISTING volume (keeps the Windows manifests)
#   v3.sh pull | log              OEMDRV VHDX -> artifacts/v3/oemdrv.img (cache-evicted) | print run.log
#   v3.sh start                   power on (GRUB default = Fedora) and screenshot
#   v3.sh login-bootstrap         ONE-TIME: log in on the Fedora console and run
#                                 v3-bootstrap.sh from OEMDRV (installs the hook)
#   v3.sh type "text"             type text as per-character VK codes (letters lower-case)
#   v3.sh wait-off [secs]         poll until the VM is Off
#   v3.sh shot TAG                screenshot -> artifacts/v3/TAG.png
#   v3.sh windows                 power on and pick Windows in GRUB (two Downs), wait for PS Direct
#   v3.sh plant CONFIG [plant args]   Copy-VMFile guest/v3-plant.ps1 in and run it (Windows must be
#                                 up): plants/hashes the corpus + C:\Users onto OEMDRV, FULL shutdown
#   v3.sh read [old]              = start: Fedora boots (old = second GRUB entry = previous kernel), the hook runs OEMDRV:/run.sh (v3-read.sh), powers off
#   v3.sh verdict                 pull + v3-verdict.py -> docs/validation-results/v3-bitlk-read.csv
#   v3.sh backup NAME             VM off: Windows-side copy of the guest VHDX -> vm/<VM>.NAME.vhdx (no 9p)
#   v3.sh mkvm DISKNAME            VMNAME=<new> : throwaway Gen 2 VM around vm/UPGRIGHV.DISKNAME.vhdx (SB off, vTPM)
#   v3.sh swap-in NAME            VM off: make vm/<VM>.NAME.vhdx the ONLY system disk (main detached), boot-first disk
#   v3.sh swap-back NAME          VM off: main disk back at SCSI 0, vm/<VM>.NAME.vhdx attached as a DATA disk (next free SCSI slot -
#                                 the DVDs hold 1 and 2; the reader finds it with DEV=auto)
#   v3.sh encrypt NAME <Method> usedspace|full [ShrinkGiB]   Windows up on the NAME disk: run guest/v3-encrypt.ps1,
#                                 capture RECOVERY_KEY= into the host key file for NAME (never printed), full shutdown
#   v3.sh run CONFIG [plant args] the whole cycle for one config: oemdrv -> windows -> plant -> read -> verdict
#
# Evidence rows are written by the harness (v3-verdict), never by hand.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"
VMNAME=${VMNAME:-UPGRIGHV}
LEG=${LEG:-}
A=artifacts/v3${LEG:+-$LEG}
HV=/mnt/c/upgrade-rig/hv
KEYFILE=${KEYFILE:-$HV/UPGRIGHV-bitlocker-recovery.txt}
OEM_VHDX="$HV/vm/oemdrv-v3${LEG:+-$LEG}.vhdx"
OEM_VHDX_WIN="C:\\upgrade-rig\\hv\\vm\\oemdrv-v3${LEG:+-$LEG}.vhdx"
OEM="$A/oemdrv.img"
MAIN_VHDX_WIN="C:\\upgrade-rig\\hv\\vm\\$VMNAME.vhdx"
CSV=../../docs/validation-results/v3-bitlk-read.csv
HARNESS_VERSION=0.1.0-hv
FIRMWARE='Hyper-V UEFI Release v4.1'
CONTEXT='installed Fedora 42 (alongside, sda5)'

VMPS1="$(wslpath -w vm.ps1)"
PS() { powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$VMPS1" "$@" -Name "$VMNAME" < /dev/null; }
PSC() { powershell.exe -NoProfile -Command "$1" < /dev/null; }
vm_state() { PSC "(Get-VM $VMNAME).State" | tr -d '\r\n '; }
need_off() { s=$(vm_state); [ "$s" = Off ] || { echo "v3: VM must be Off (state: $s)" >&2; exit 1; }; }
wait_secs() { t0=$(date +%s); until [ $(( $(date +%s) - t0 )) -ge "$1" ]; do sleep 1; done; }
evict()    { python3 -c 'import os,sys; fd=os.open(sys.argv[1],os.O_RDONLY); os.posix_fadvise(fd,0,0,os.POSIX_FADV_DONTNEED); os.close(fd)' "$1"; }
oem_pull() { evict "$OEM_VHDX"; qemu-img convert -f vhdx -O raw "$OEM_VHDX" "$OEM"; }
oem_push() { qemu-img convert -f raw -O vhdx "$OEM" "$OEM_VHDX"; }
# detach EVERY OEMDRV-labelled data disk (v1b's included) so by-label is unambiguous
oem_detach_all() { PSC "Get-VMHardDiskDrive $VMNAME | Where-Object { \$_.Path -like '*oemdrv*' } | Remove-VMHardDiskDrive"; }
oem_attach() { PSC "Add-VMHardDiskDrive -VMName $VMNAME -ControllerType SCSI -Path '$OEM_VHDX_WIN'"; }

# WMI TypeText garbles; type per character as virtual-key codes (2026-08-31).
str2vk() {
    local s="$1" c out=()
    for (( i=0; i<${#s}; i++ )); do
        c="${s:i:1}"
        case "$c" in
            [a-z]) out+=( $(( $(printf '%d' "'${c^^}") )) ) ;;
            [A-Z]) out+=( "s$(( $(printf '%d' "'$c") ))" ) ;;
            [0-9]) out+=( $(( 48 + c )) ) ;;
            '?') out+=(s191) ;; '*') out+=(s56) ;; ':') out+=(s186) ;; '_') out+=(s189) ;; '|') out+=(s220) ;; '"') out+=(s222) ;; "'") out+=(222) ;; '$') out+=(s52) ;; '(') out+=(s57) ;; ')') out+=(s48) ;; '>') out+=(s190) ;; '<') out+=(s188) ;; '!') out+=(s49) ;; '~') out+=(s192) ;; '\\') out+=(220) ;; '[') out+=(219) ;; ']') out+=(221) ;; '{') out+=(s219) ;; '}') out+=(s221) ;; '+') out+=(s187) ;; '&') out+=(s55) ;; '%') out+=(s53) ;; '@') out+=(s50) ;; '#') out+=(s51) ;; '^') out+=(s54) ;;
            ' ') out+=(32) ;; '/') out+=(191) ;; '-') out+=(189) ;; '.') out+=(190) ;; ',') out+=(188) ;; ';') out+=(186) ;; '=') out+=(187) ;;
            *) echo "str2vk: unsupported char '$c'" >&2; return 1 ;;
        esac
    done
    echo "${out[@]}"
}
type_line() { PS key $(str2vk "$1") 13; }

case "${1:-}" in
oemdrv)
    need_off
    [ -n "${2:-}" ] && [ -f "$2" ] || { echo "v3: oemdrv <run.sh>" >&2; exit 1; }
    mkdir -p "$A"
    oem_detach_all
    rm -f "$OEM"
    truncate -s 512M "$OEM"
    parted -s "$OEM" mklabel msdos mkpart primary fat32 1MiB 100%
    mkfs.fat -F 32 -n OEMDRV --offset 2048 "$OEM" >/dev/null
    mmd -i "$OEM@@1M" ::/v3
    mcopy -i "$OEM@@1M" "$2" ::/run.sh
    mcopy -i "$OEM@@1M" guest/oemdrv-run.sh guest/v3-bootstrap.sh ::/
    # the CURRENT recovery password only; stays host-side + on this VHDX until the guest removes it
    grep '^CURRENT' "$KEYFILE" | grep -oE '[0-9]{6}(-[0-9]{6}){7}' | head -1 > "$A/.key"
    [ -s "$A/.key" ] || { echo "v3: no CURRENT key in $KEYFILE" >&2; exit 1; }
    mcopy -i "$OEM@@1M" "$A/.key" ::/v3/key.txt; rm -f "$A/.key"
    if [ -n "${DEV:-}" ]; then printf '%s\n' "$DEV" > "$A/.dev"; mcopy -i "$OEM@@1M" "$A/.dev" ::/v3/dev; rm -f "$A/.dev"; fi
    shift 2; for f in "$@"; do mcopy -i "$OEM@@1M" "$f" ::/v3/; done
    oem_push
    oem_attach
    echo "v3: built FRESH $OEM_VHDX and attached it (other OEMDRV disks detached)"
    mdir -i "$OEM@@1M" ::/; mdir -i "$OEM@@1M" ::/v3
    ;;
rearm)
    # keep everything already on the volume (the Windows manifests); put run.sh + the key back
    need_off; [ -f "${2:-}" ] || { echo "v3: rearm <run.sh>" >&2; exit 1; }
    oem_pull
    mcopy -o -i "$OEM@@1M" "$2" ::/run.sh
    grep '^CURRENT' "$KEYFILE" | grep -oE '[0-9]{6}(-[0-9]{6}){7}' | head -1 > "$A/.key"
    mcopy -o -i "$OEM@@1M" "$A/.key" ::/v3/key.txt; rm -f "$A/.key"
    if [ -n "${DEV:-}" ]; then printf '%s\n' "$DEV" > "$A/.dev"; mcopy -o -i "$OEM@@1M" "$A/.dev" ::/v3/dev; rm -f "$A/.dev"; fi
    oem_detach_all; oem_push; oem_attach
    echo "v3: re-armed $OEM_VHDX with $2"; mdir -i "$OEM@@1M" ::/
    ;;
rearm-upgrade)
    # like rearm, but run.sh = the kernel-upgrade wrapper and the reader rides along as v3/read.sh
    need_off
    oem_pull
    mcopy -o -i "$OEM@@1M" guest/v3-upgrade-then-read.sh ::/run.sh
    mcopy -o -i "$OEM@@1M" guest/v3-read.sh ::/v3/read.sh
    mdel -i "$OEM@@1M" ::/v3/kernel-upgrade.done 2>/dev/null || true
    grep '^CURRENT' "$KEYFILE" | grep -oE '[0-9]{6}(-[0-9]{6}){7}' | head -1 > "$A/.key"
    mcopy -o -i "$OEM@@1M" "$A/.key" ::/v3/key.txt; rm -f "$A/.key"
    oem_detach_all; oem_push; oem_attach
    echo "v3: re-armed $OEM_VHDX with the kernel-upgrade wrapper + reader"
    ;;
pull) need_off; oem_pull; echo "v3: pulled OEMDRV -> $OEM"; mdir -i "$OEM@@1M" ::/ ;;
log)  need_off; oem_pull; mtype -i "$OEM@@1M" ::/run.log 2>/dev/null || echo "(no run.log yet)" ;;
start)
    need_off; mkdir -p "$A"
    PS start; wait_secs 12; PS shot "C:\\upgrade-rig\\hv\\shots\\v3-grub.png"; cp "$HV/shots/v3-grub.png" "$A/" || true
    ;;
login-bootstrap)
    # Fedora text console: login rig/rig, mount the OEMDRV partition (sdb1 —
    # the by-label path needs upper-case, which the VK map can't type), run
    # the bootstrap (sudo asks for the password once).
    type_line rig; wait_secs 2; type_line rig; wait_secs 3
    type_line 'sudo mount /dev/sdb1 /mnt'; wait_secs 2; type_line rig; wait_secs 2
    type_line 'sudo bash /mnt/v3-bootstrap.sh'
    wait_secs 3; PS shot "C:\\upgrade-rig\\hv\\shots\\v3-bootstrap.png"; cp "$HV/shots/v3-bootstrap.png" "$A/" || true
    ;;
type) type_line "$2" ;;
wait-off)
    limit=${2:-3600}; t0=$(date +%s)
    while :; do
        s=$(vm_state)
        [ "$s" = Off ] && { echo "v3: VM is Off after $(( $(date +%s) - t0 )) s"; exit 0; }
        [ $(( $(date +%s) - t0 )) -ge "$limit" ] && { echo "v3: still $s after $limit s" >&2; exit 2; }
        sleep 10
    done
    ;;
windows)
    need_off; mkdir -p "$A"
    PS start; wait_secs 12; PS shot "C:\\upgrade-rig\\hv\\shots\\v3-grub-win.png"
    PS key 40; PS key 40; PS key 13
    # Windows boots; PS Direct answers once the guest's VMBus session is up
    t0=$(date +%s)
    while :; do
        if PS ps 'hostname' 2>/dev/null | grep -q UPGRIG; then echo "v3: Windows up (PS Direct) after $(( $(date +%s) - t0 )) s"; break; fi
        [ $(( $(date +%s) - t0 )) -ge 600 ] && { PS shot "C:\\upgrade-rig\\hv\\shots\\v3-win-stuck.png"; echo "v3: Windows did not answer within 10 min" >&2; exit 2; }
        sleep 10
    done
    cp "$HV"/shots/v3-grub-win.png "$A/" 2>/dev/null || true
    ;;
plant)
    cfg=${2:?config label}; shift 2
    PS copy "$(wslpath -w guest/v3-plant.ps1)" 'C:\upgrade_\rig\v3-plant.ps1'
    PS ps "powershell.exe -NoProfile -ExecutionPolicy Bypass -File C:\upgrade_\rig\v3-plant.ps1 -Config $cfg $*"
    ;;
read)
    need_off; mkdir -p "$A"
    oem_pull; mdir -i "$OEM@@1M" ::/ | grep -q 'run *sh' || { echo "v3: no run.sh on OEMDRV - run 'v3.sh oemdrv guest/v3-read.sh' first" >&2; exit 1; }
    PS start; wait_secs 12; PS shot "C:\\upgrade-rig\\hv\\shots\\v3-grub-read.png"; cp "$HV/shots/v3-grub-read.png" "$A/" || true
    # 'read old' = the second GRUB entry (the previous kernel, e.g. the F42 GA 6.14 after a dnf upgrade)
    if [ "${2:-}" = old ]; then PS key 40; PS shot "C:\\upgrade-rig\\hv\\shots\\v3-grub-read-old.png"; PS key 13; fi
    ;;
verdict)
    need_off; oem_pull
    mtype -i "$OEM@@1M" ::/run.log > "$A/read-run.log" 2>/dev/null || true
    python3 v3-verdict.py "$A" "$CSV" "$HARNESS_VERSION" "$FIRMWARE" "$CONTEXT"
    ;;
backup)
    need_off
    PSC "Copy-Item -LiteralPath '$MAIN_VHDX_WIN' -Destination 'C:\\upgrade-rig\\hv\\vm\\$VMNAME.${2:?name}.vhdx'; Get-Item 'C:\\upgrade-rig\\hv\\vm\\$VMNAME.${2}.vhdx' | Select-Object Name, Length"
    ;;
mkvm)
    # a throwaway Gen 2 guest around an EXISTING disk (vm/<VM>.NAME.vhdx): SB off (MicrosoftWindows
    # template so Windows boots), fresh vTPM, no DVDs - the Windows side of a config build,
    # run in parallel with whatever UPGRIGHV is doing. VMNAME=<new name> ./v3.sh mkvm <disk name>
    n=${2:?disk name}
    PSC "\$v = New-VM -Name $VMNAME -Generation 2 -MemoryStartupBytes 4GB -VHDPath 'C:\\upgrade-rig\\hv\\vm\\UPGRIGHV.$n.vhdx' -SwitchName 'Default Switch' -Path 'C:\\upgrade-rig\\hv\\vm'; Set-VM -VM \$v -ProcessorCount 2 -AutomaticCheckpointsEnabled \$false -CheckpointType Standard -AutomaticStopAction ShutDown; Set-VMMemory -VM \$v -DynamicMemoryEnabled \$false; Set-VMFirmware -VM \$v -EnableSecureBoot Off -SecureBootTemplate MicrosoftWindows; Set-VMKeyProtector -VM \$v -NewLocalKeyProtector; Enable-VMTPM -VM \$v; Enable-VMIntegrationService -VM \$v -Name 'Guest Service Interface'; Get-VM $VMNAME | Select-Object Name, State, ProcessorCount, MemoryStartup"
    PS fw
    ;;
swap-in)
    need_off; n=${2:?name}
    PSC "Get-VMHardDiskDrive $VMNAME | Where-Object { \$_.Path -notlike '*oemdrv*' } | Remove-VMHardDiskDrive; Add-VMHardDiskDrive -VMName $VMNAME -ControllerType SCSI -ControllerNumber 0 -ControllerLocation 0 -Path 'C:\\upgrade-rig\\hv\\vm\\$VMNAME.$n.vhdx'"
    PS boot-first disk; PS disk list
    ;;
swap-back)
    need_off; n=${2:?name}
    PSC "Get-VMHardDiskDrive $VMNAME | Where-Object { \$_.Path -notlike '*oemdrv*' } | Remove-VMHardDiskDrive; Add-VMHardDiskDrive -VMName $VMNAME -ControllerType SCSI -ControllerNumber 0 -ControllerLocation 0 -Path '$MAIN_VHDX_WIN'; Add-VMHardDiskDrive -VMName $VMNAME -ControllerType SCSI -ControllerNumber 0 -Path 'C:\\upgrade-rig\\hv\\vm\\$VMNAME.$n.vhdx'"
    PS boot-first disk; PS disk list
    ;;
encrypt)
    n=${2:?name}; m=${3:?method}; mode=${4:?usedspace|full}; shrink=${5:-32}
    mkdir -p "$A"
    PS copy "$(wslpath -w guest/v3-encrypt.ps1)" 'C:\upgrade_\rig\v3-encrypt.ps1'
    uso=''; [ "$mode" = usedspace ] && uso='-UsedSpaceOnly'
    PS ps "powershell.exe -NoProfile -ExecutionPolicy Bypass -File C:\upgrade_\rig\v3-encrypt.ps1 -Method $m $uso -ShrinkGiB $shrink" > "$A/.encrypt-raw" 2>&1 || true
    # Enable-BitLocker prints the numeric password in its own text too (seen 2026-09-01): take the
    # key from the raw output ONCE, then redact every 48-digit pattern before anything is kept or shown
    k=$(grep -oE '[0-9]{6}(-[0-9]{6}){7}' "$A/.encrypt-raw" | head -1)
    sed -E 's/[0-9]{6}(-[0-9]{6}){7}/<redacted recovery password>/g' "$A/.encrypt-raw" > "$A/encrypt-$n.out"; rm -f "$A/.encrypt-raw"
    grep -v '^\s*$' "$A/encrypt-$n.out" | tail -12
    [ -n "$k" ] || { echo "v3: no recovery password in the encrypt output" >&2; exit 1; }
    kf="$HV/$VMNAME.$n-bitlocker-recovery.txt"
    printf '%s BitLocker C: recovery password (rig-only guest disk %s, throwaway)\nCURRENT (%s, %s %s): %s\n' "$VMNAME.$n" "$n" "$(date -u +%F)" "$m" "$mode" "$k" > "$kf"
    echo "v3: recovery password captured to $kf (use KEYFILE=$kf for the read)"
    ;;
run)
    cfg=${2:?config label}; shift 2
    "$0" oemdrv guest/v3-read.sh
    "$0" windows
    "$0" plant "$cfg" "$@"
    "$0" wait-off 1200
    "$0" read
    "$0" wait-off 3600
    "$0" verdict
    ;;
shot) PS shot "C:\\upgrade-rig\\hv\\shots\\${2}.png"; mkdir -p "$A"; cp "$HV/shots/${2}.png" "$A/" ;;
*) sed -n '3,25p' "$0"; exit 1 ;;
esac
