#!/usr/bin/env bash
# The Ubuntu arms (RISKS R40, R45; roadmap item 23): the product's own
# pieces on a throwaway Hyper-V guest, each arm judged from its own evidence
# and written as a row of docs/validation-results/ubuntu-install.csv by
# ubuntu-verdict.py, never by hand. A rig row closes plumbing only (rule #5).
#
# The guest, UPGRIGUBU: Secure Boot on (the UEFI CA template), the network
# adapter connected to nothing, a 40 GB system disk and a 60 GB decoy disk.
# The decoy is the larger on purpose: left to itself Ubuntu's installer
# takes "the largest disk" (R45). The stick is the ISO's files on FAT32,
# our grub.cfg (the kernel argument 'autoinstall'), the stick's own
# autoinstall.yaml (upgrade-autoinstall stick-file), upgrade-installer,
# job.json, SHA256SUMS and the boot-install marker.
#
#   ubuntu-arms.sh kit            build upgrade-installer (static) and upgrade-autoinstall; unpack and hash the ISO
#   ubuntu-arms.sh vm             create the guest and its disks
#   ubuntu-arms.sh arm A [NOTE]   wrong machine: a job naming a disk that is not there. Refused, restarted, nothing written
#   ubuntu-arms.sh arm F [NOTE]   the fence: the stick's own file with our check taken out. Nothing installed
#   ubuntu-arms.sh arm C [NOTE]   the erase: the job names the 40 GB disk (identity read from arm A's log)
#   ubuntu-arms.sh arm D [NOTE]   not again: after C, the stick first in the order. Refused, nothing written
#   ubuntu-arms.sh arm K [NOTE]   beside Windows: the system disk is a copy of the rig's shrunk Windows disk
#                                 (UPGRIGHV.pre-install.vhdx). Every partition of it is hashed before and after
#   ubuntu-arms.sh arm G [NOTE]   two free spaces: a made-up disk with a boot partition holding a file named as
#                                 Windows' loader, 30 GiB free, a data partition, 40 GiB free. Refused, nothing written
#   ubuntu-arms.sh clean          delete the guest and every disk (ubuntu.sh clean)
set -euo pipefail
cd "$(dirname "$0")"
VMNAME=UPGRIGUBU
REPO="$(cd ../.. && pwd)"
A="$PWD/artifacts/ubuntu"; ROOT="$A/iso-root"
HV=/mnt/c/upgrade-rig/hv; HVW='C:\upgrade-rig\hv'
BASE_IMG="$A/ubu-arms-stick.img"
CSV="$REPO/docs/validation-results/ubuntu-install.csv"
INSTALLER="$REPO/upgrade_/linux/installer/target/x86_64-unknown-linux-musl/release/upgrade-installer"
GEN="$REPO/target/debug/upgrade-autoinstall"
BOGUS_UID=60022480ffffffffffffffffffffffff
SYSTEM_BYTES=42949672960
VMPS1="$(wslpath -w vm.ps1)"
PS()  { powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$VMPS1" "$@" -Name "$VMNAME" < /dev/null | tr -d '\r'; }
PSC() { powershell.exe -NoProfile -Command "$1" < /dev/null | tr -d '\r'; }
evict() { python3 -c 'import os,sys; fd=os.open(sys.argv[1],os.O_RDONLY); os.posix_fadvise(fd,0,0,os.POSIX_FADV_DONTNEED); os.close(fd)' "$1" 2>/dev/null || true; }
state() { PSC "(Get-VM -Name $VMNAME -ErrorAction SilentlyContinue).State" | tr -d ' \n'; }
# What the guest can see of each disk, hashed: the disk file itself will not do, because
# Hyper-V rewrites a new file's own header when it opens it (arm A's first row, 2026-11-13).
# Arm K's system disk is hashed partition by partition instead (ubuntu-parts.py), and Windows'
# loader and the list of files on its boot partition are kept from before and after.
hashes() {  # $1 = before|after
    local n
    for n in ${SYSDISK:-target} decoy; do
        evict "$HV/vm/ubu-$n.vhdx"
        qemu-img convert -f vhdx -O raw "$HV/vm/ubu-$n.vhdx" "$A/content.raw"
        if [ "$n" = windows ]; then
            python3 ubuntu-parts.py "$A/content.raw" > "$D/parts-$1.txt"
            local esp; esp="$(awk '$2 == "c12a7328-f81f-11d2-ba4b-00a0c93ec93b" { print $3; exit }' "$D/parts-$1.txt")"
            mcopy -n -i "$A/content.raw@@$esp" ::/EFI/Microsoft/Boot/bootmgfw.efi "$D/bootmgfw-$1.efi" 2>/dev/null || true
            mdir -/ -b -i "$A/content.raw@@$esp" ::/ 2>/dev/null | sort > "$D/esp-$1.txt" || true
        else
            echo "$([ "$n" = decoy ] && echo decoy || echo target) $(sha256sum < "$A/content.raw" | cut -d' ' -f1)"
        fi
        rm -f "$A/content.raw"
    done
}
# the identity a Hyper-V guest sees for a disk file: 60022480, then the file's DiskIdentifier (its first six bytes as stored, then its last six)
wwn_of() { PSC "\$g = [guid](Get-VHD '$HVW\\vm\\$1').DiskIdentifier; \$b = \$g.ToByteArray(); '60022480' + (-join (\$b[0..5] + \$b[10..15] | ForEach-Object { \$_.ToString('x2') }))" | tr -d ' \n'; }
swap_system() {  # $1 = the disk file the guest gets as its system disk; the other system disks are taken out
    off
    PSC "Get-VMHardDiskDrive -VMName $VMNAME | Where-Object { (\$_.Path -like '*ubu-target.vhdx' -or \$_.Path -like '*ubu-windows.vhdx' -or \$_.Path -like '*ubu-gaps.vhdx') -and \$_.Path -notlike '*$1' } | Remove-VMHardDiskDrive
      if (-not (Get-VMHardDiskDrive -VMName $VMNAME | Where-Object { \$_.Path -like '*$1' })) { Add-VMHardDiskDrive -VMName $VMNAME -ControllerType SCSI -Path '$HVW\\vm\\$1' }"
}
off() { [ "$(state)" = Off ] || { PS kill >/dev/null; sleep 3; }; }

build_stick() {  # $1 = arm, $2 = job.json
    local arm="$1" job="$2" S="$A/arms-extra"
    [ -f "$A/SHA256SUMS.iso" ] || { echo "ubuntu-arms: run '$0 kit' first" >&2; exit 1; }
    rm -rf "$S"; mkdir -p "$S/upgrade_"
    cat > "$S/grub.cfg" <<'G'
set timeout=3
menuentry "Ubuntu, from this stick (upgrade_)" {
    set gfxpayload=keep
    linux  /casper/vmlinuz autoinstall ---
    initrd /casper/initrd
}
G
    "$GEN" stick-file /cdrom/upgrade_/upgrade-installer /cdrom/upgrade_/job.json > "$S/autoinstall.yaml"
    if [ "$arm" = F ]; then
        # the fence arm: the same file with our check taken out, and the bench's own
        # error hook so the installer's log of what it did reaches the log disk
        python3 - "$S/autoinstall.yaml" <<'P'
import sys
p = sys.argv[1]; l = open(p).read().split("\n")
i = l.index("  early-commands:"); del l[i:i + 2]
l[-1:] = ["  error-commands:", "    - \"mkdir -p /run/upglog && mount -L UPGLOG /run/upglog && tar czf /run/upglog/fence-installer.tgz /var/log/installer; lsblk -b -o NAME,SIZE,TYPE,FSTYPE > /run/upglog/fence-lsblk.txt; sync; umount /run/upglog\"", ""]
open(p, "w").write("\n".join(l))
P
    fi
    cp "$job" "$S/upgrade_/job.json"; cp "$INSTALLER" "$S/upgrade_/upgrade-installer"
    echo "armed by ubuntu-arms.sh arm $arm" > "$S/upgrade_/boot-install"
    echo "the rig's stick" > "$S/upgrade_/bench-ubuntu"
    echo "$(sha256sum < "$INSTALLER" | cut -c1-16)" > "$A/installer.sha256"
    cat > "$S/upgrade_/bench-firstboot.sh" <<'B'
#!/bin/sh
# rig only: what the first start of the installed system looks like, then off
L=/run/upglog; mkdir -p $L; mount -L UPGLOG $L || exit 0
{ echo "== firstboot $(date -u +%FT%TZ)"; echo "kernel=$(uname -r)"; head -2 /etc/os-release
  echo "firmware=$(cat /sys/class/dmi/id/bios_vendor) $(cat /sys/class/dmi/id/bios_version)"
  echo "root_src=$(findmnt -no SOURCE /)"; echo "boot_src=$(findmnt -no SOURCE /boot)"; echo "esp_src=$(findmnt -no SOURCE /boot/efi)"
  echo "sb=$(mokutil --sb-state 2>&1 | head -1)"
  echo "user_hash_sha256=$(getent shadow rig | cut -d: -f2 | tr -d '\n' | sha256sum | cut -d' ' -f1)"
  echo "default_target=$(systemctl get-default)"
  echo "kept_entries=$(grep -c "^menuentry 'Windows (kept)'" /boot/grub/grub.cfg)"; echo "osprober_entries=$(grep -c osprober /boot/grub/grub.cfg)"
  sleep 45; echo "display_manager=$(systemctl is-active display-manager)"
  echo "== lsblk"; lsblk -b -o NAME,SIZE,TYPE,FSTYPE,MOUNTPOINTS
  echo "== net"; ip -br addr; echo "== efibootmgr"; efibootmgr
} > $L/firstboot.txt 2>&1
sync; umount $L; systemctl poweroff
B
    { grep -v ' boot/grub/grub.cfg$' "$A/SHA256SUMS.iso"; echo "$(sha256sum < "$S/grub.cfg" | cut -d' ' -f1)  boot/grub/grub.cfg"; } > "$S/SHA256SUMS"
    if [ ! -f "$BASE_IMG" ]; then
        local mb=$(( $(du -sb "$ROOT" | cut -f1) / 1048576 + 400 ))
        echo "ubuntu-arms: the stick image, ${mb} MiB (once)"
        truncate -s "${mb}M" "$BASE_IMG"
        parted -s "$BASE_IMG" mklabel msdos mkpart primary fat32 1MiB 100% set 1 boot on
        mkfs.fat -F 32 -n UPGV0 --offset 2048 "$BASE_IMG" >/dev/null
        (cd "$ROOT" && mcopy -s -o -i "$BASE_IMG@@1M" ./* ./.disk ::/)
    fi
    local P="$BASE_IMG@@1M"
    mdeltree -i "$P" ::/upgrade_ 2>/dev/null || true
    mcopy -o -i "$P" "$S/grub.cfg" ::/boot/grub/grub.cfg
    mcopy -o -i "$P" "$S/autoinstall.yaml" "$S/SHA256SUMS" ::/
    mcopy -s -o -i "$P" "$S/upgrade_" ::/
    off
    rm -f "$HV/vm/ubu-stick.vhdx"; qemu-img convert -f raw -O vhdx "$BASE_IMG" "$HV/vm/ubu-stick.vhdx"
    evict "$BASE_IMG"
    echo "ubuntu-arms: stick built for arm $arm"
}

pull_stick() {  # $1 = arm dir
    local d="$1"; rm -rf "$d/stick"; mkdir -p "$d/stick"
    evict "$HV/vm/ubu-stick.vhdx"
    qemu-img convert -f vhdx -O raw "$HV/vm/ubu-stick.vhdx" "$d/stick.raw"
    mcopy -s -n -i "$d/stick.raw@@1M" ::/upgrade_ "$d/stick/" 2>/dev/null || true
    mcopy -n -i "$d/stick.raw@@1M" ::/autoinstall.yaml "$d/stick/" 2>/dev/null || true
    rm -f "$d/stick/upgrade_/upgrade-installer" "$d/stick.raw"
}

run_arm() {  # $1 = arm dir name, $2 = seconds
    ./ubuntu.sh run "$1" "$2" | tail -3
    state > "$A/$1/end-state.txt"
    [ "$(state)" = Off ] || ./ubuntu.sh shot "$1-end" >/dev/null 2>&1 || true
    off
}

case "${1:-}" in
kit)
    (cd "$REPO/upgrade_/linux/installer" && cargo build -q --release --target x86_64-unknown-linux-musl)
    (cd "$REPO" && cargo build -q -p upgrade-autoinstall)
    [ -d "$ROOT/casper" ] || ./ubuntu.sh root
    [ -f "$A/SHA256SUMS.iso" ] || (cd "$ROOT" && find casper pool dists boot/grub EFI/boot -type f -print0 | sort -z | xargs -0 sha256sum > "$A/SHA256SUMS.iso")
    wc -l "$A/SHA256SUMS.iso"; "$INSTALLER" 2>&1 | head -2 || true
    ;;
vm)
    truncate -s 4M "$A/empty"; [ -f "$HV/vm/ubu-stick.vhdx" ] || qemu-img convert -f raw -O vhdx "$A/empty" "$HV/vm/ubu-stick.vhdx"
    ./ubuntu.sh vm
    PSC "\$ErrorActionPreference='Stop'
      New-VHD -Path '$HVW\\vm\\ubu-decoy.vhdx' -SizeBytes 60GB -Dynamic | Out-Null
      Add-VMHardDiskDrive -VMName $VMNAME -ControllerType SCSI -Path '$HVW\\vm\\ubu-decoy.vhdx'
      Get-VMNetworkAdapter -VMName $VMNAME | ForEach-Object { 'adapter=' + \$_.Name + ' switch=[' + \$_.SwitchName + ']' }"
    PS disk list
    ;;
arm)
    ARM="${2:?A|F|C|D|K}"; NOTE="${3:-}"; T="arm-$ARM"; D="$A/$T"
    rm -rf "$D"; mkdir -p "$D"
    SYSDISK=target; [ "$ARM" = K ] && SYSDISK=windows; [ "$ARM" = G ] && SYSDISK=gaps
    HASH="$(openssl passwd -6 -salt upgrig rig)"
    case "$ARM" in
    A)  python3 ubuntu-job.py "$D/job.json" --uid=$BOGUS_UID --size=$SYSTEM_BYTES --hash="$HASH"
        build_stick A "$D/job.json"; SECS=540 ;;
    F)  python3 ubuntu-job.py "$D/job.json" --uid=$BOGUS_UID --size=$SYSTEM_BYTES --hash="$HASH"
        build_stick F "$D/job.json"; SECS=420 ;;
    C)  # the guest's own words for its 40 GB disk, from arm A's early log
        read -r CUID CSERIAL < <(python3 - "$A/arm-A/stick/upgrade_/report/early.log" $SYSTEM_BYTES <<'P'
import sys
for l in open(sys.argv[1]):
    f = l.split()
    if len(f) >= 4 and f[1] == sys.argv[2] and f[2] == "disk" and f[-1].startswith("0x"):
        print(f[-1][2:], f[-2] if len(f) >= 5 and f[-2] != "disk" else ""); break
P
)
        [ -n "${CUID:-}" ] || { echo "ubuntu-arms: arm A's early.log names no $SYSTEM_BYTES-byte disk; run arm A first" >&2; exit 1; }
        python3 ubuntu-job.py "$D/job.json" --uid="$CUID" --size=$SYSTEM_BYTES --serial="${CSERIAL:-}" --hash="$HASH"
        build_stick C "$D/job.json"; SECS=3600 ;;
    D)  [ -f "$A/arm-C/job.json" ] || { echo "ubuntu-arms: arm D runs after arm C, on the stick C left" >&2; exit 1; }
        cp "$A/arm-C/job.json" "$D/job.json"; off; SECS=540 ;;
    K)  if [ ! -f "$HV/vm/ubu-windows.vhdx" ]; then
            # in the foreground, and compared: a copy cut short is a full-length file with a garbage tail (2026-11-12)
            PSC "\$ErrorActionPreference='Stop'; Copy-Item '$HVW\\vm\\UPGRIGHV.pre-install.vhdx' '$HVW\\vm\\ubu-windows.vhdx'
              \$a = (Get-FileHash '$HVW\\vm\\UPGRIGHV.pre-install.vhdx').Hash; \$b = (Get-FileHash '$HVW\\vm\\ubu-windows.vhdx').Hash
              if (\$a -ne \$b) { throw 'the copy of the Windows disk does not match' }; 'windows disk copied: ' + \$b"
        fi
        WUID="$(wwn_of ubu-windows.vhdx)"; WSIZE="$(PSC "(Get-VHD '$HVW\\vm\\ubu-windows.vhdx').Size" | tr -d ' \n')"
        python3 ubuntu-job.py "$D/job.json" --keep --uid="$WUID" --size="$WSIZE" --hash="$HASH"
        build_stick K "$D/job.json"; SECS=3600 ;;
    G)  # a made-up disk (rule #5: a spoof): nothing on it is Windows but the name of one file
        off; rm -f "$A/gaps.raw" "$HV/vm/ubu-gaps.vhdx"; truncate -s 80G "$A/gaps.raw"
        parted -s "$A/gaps.raw" mklabel gpt mkpart ESP fat32 1MiB 101MiB set 1 esp on mkpart data ntfs 30821MiB 41061MiB set 2 msftdata on
        mkfs.fat -F 32 -n SYSTEM --offset 2048 "$A/gaps.raw" $((100 * 1024)) > /dev/null
        mmd -i "$A/gaps.raw@@1M" ::/EFI ::/EFI/Microsoft ::/EFI/Microsoft/Boot ::/EFI/Boot
        echo "not Windows: the two-spaces arm's stand-in" > "$A/fake-loader"
        mcopy -i "$A/gaps.raw@@1M" "$A/fake-loader" ::/EFI/Microsoft/Boot/bootmgfw.efi
        mcopy -i "$A/gaps.raw@@1M" "$A/fake-loader" ::/EFI/Boot/bootx64.efi
        qemu-img convert -f raw -O vhdx -o subformat=dynamic "$A/gaps.raw" "$HV/vm/ubu-gaps.vhdx"; rm -f "$A/gaps.raw" "$A/fake-loader"
        GUID="$(wwn_of ubu-gaps.vhdx)"; GSIZE="$(PSC "(Get-VHD '$HVW\\vm\\ubu-gaps.vhdx').Size" | tr -d ' \n')"
        python3 ubuntu-job.py "$D/job.json" --keep --uid="$GUID" --size="$GSIZE" --hash="$HASH"
        build_stick G "$D/job.json"; SECS=600 ;;
    *)  echo "ubuntu-arms: unknown arm $ARM" >&2; exit 1 ;;
    esac
    swap_system "ubu-$SYSDISK.vhdx"
    ./ubuntu.sh first stick > /dev/null
    PSC "Get-VMNetworkAdapter -VMName $VMNAME | ForEach-Object { 'adapter=' + \$_.Name + ' switch=[' + \$_.SwitchName + ']' }; (Get-VMFirmware -VMName $VMNAME | ForEach-Object { 'secure_boot=' + \$_.SecureBoot + ' template=' + \$_.SecureBootTemplate })" > "$D/guest.txt"
    echo "installer_sha256=$(cat "$A/installer.sha256" 2>/dev/null)" >> "$D/guest.txt"
    off; hashes before > "$D/hashes-before.txt"
    run_arm "$T" "$SECS"
    hashes after > "$D/hashes-after.txt"
    pull_stick "$D"
    ./ubuntu.sh log "$T" > /dev/null 2>&1 || true
    qemu-img dd -f vhdx -O raw bs=1M count=1 if="$HV/vm/ubu-$SYSDISK.vhdx" of="$D/target-first.bin" 2>/dev/null || true
    python3 ubuntu-verdict.py "$ARM" "$D" "$CSV" "$NOTE"
    ;;
clean)
    ./ubuntu.sh clean; rm -f "$HV/vm/ubu-decoy.vhdx" "$HV/vm/ubu-target.vhdx" "$HV/vm/ubu-windows.vhdx" "$HV/vm/ubu-gaps.vhdx" "$BASE_IMG"; rm -rf "$A/arms-extra" ;;
*) sed -n 2,22p "$0"; exit 1 ;;
esac
