#!/usr/bin/env bash
# The Ubuntu measuring bench (RISKS R40, roadmap item 23): does Ubuntu's
# installer run from the medium alone, with no network and nothing typed,
# Secure Boot on? A throwaway Hyper-V guest, UPGRIGUBU; no product code runs
# here, and nothing it finds is a row (a hand-run finding goes in the log).
#
#   ubuntu.sh root              unpack the ISO once (rig/hyperv/artifacts/ubuntu/iso-root)
#   ubuntu.sh stick ARM         build the FAT32 stick: the ISO's files, our grub.cfg
#                               (the kernel argument 'autoinstall'), autoinstall.yaml, the hooks
#   ubuntu.sh vm [DISK.vhdx]    create UPGRIGUBU: Secure Boot on (the UEFI CA template), the
#                               network adapter connected to nothing, an empty 40 GB disk or DISK
#   ubuntu.sh run TAG [SECS]    start, take a screenshot a minute, wait for the guest to turn itself off
#   ubuntu.sh log TAG           pull the log disk's files into artifacts/ubuntu/TAG/
#   ubuntu.sh fw | shot NAME | first stick|disk | detach-stick | attach-stick
#   ubuntu.sh clean             delete the guest and every disk this bench made
#
# ARM: erase (an empty disk, the whole disk), refuse (the early hook exits 7),
#      gap (install into the largest free space, beside what is there).
set -euo pipefail
cd "$(dirname "$0")"
VMNAME=UPGRIGUBU
ISO="${UBUNTU_ISO:-/home/addlatt/upgrade_/rig/vm/artifacts/ubuntu-26.04.1/ubuntu-26.04.1-desktop-amd64.iso}"
A="$PWD/artifacts/ubuntu"; ROOT="$A/iso-root"
HV=/mnt/c/upgrade-rig/hv; HVW='C:\upgrade-rig\hv'
STICK_IMG="$A/ubu-stick.img"; LOG_IMG="$A/ubu-log.img"
VMPS1="$(wslpath -w vm.ps1)"
PS()  { powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$VMPS1" "$@" -Name "$VMNAME" < /dev/null | tr -d '\r'; }
PSC() { powershell.exe -NoProfile -Command "$1" < /dev/null | tr -d '\r'; }
evict() { python3 -c 'import os,sys; fd=os.open(sys.argv[1],os.O_RDONLY); os.posix_fadvise(fd,0,0,os.POSIX_FADV_DONTNEED); os.close(fd)' "$1" 2>/dev/null || true; }
state() { PSC "(Get-VM -Name $VMNAME -ErrorAction SilentlyContinue).State" | tr -d ' \n'; }
mkdir -p "$A"

case "${1:-}" in
root)
    rm -rf "$ROOT"; mkdir -p "$ROOT"
    bsdtar -x -f "$ISO" -C "$ROOT"; chmod -R u+rwX "$ROOT"
    find "$ROOT" -type l -delete        # FAT has no links (the ISO's 'ubuntu -> .' and its like)
    du -sh "$ROOT"
    ;;
stick)
    ARM="${2:?erase|refuse|gap}"
    [ -d "$ROOT/casper" ] || { echo "ubuntu: run '$0 root' first" >&2; exit 1; }
    case "$ARM" in
        erase|refuse) STORAGE='layout: {name: direct}' ;;
        gap)          STORAGE='layout: {name: direct, mode: use_gap}' ;;
        *) echo "ubuntu: unknown arm $ARM" >&2; exit 1 ;;
    esac
    S="$A/stick-extra"; rm -rf "$S"; mkdir -p "$S/upg"
    HASH="$(openssl passwd -6 -salt upgrig rig)"
    cat > "$S/autoinstall.yaml" <<EOF
autoinstall:
  version: 1
  early-commands:
    - sh /cdrom/upg/early.sh $ARM
  locale: en_US.UTF-8
  keyboard: {layout: us}
  timezone: Etc/UTC
  identity: {realname: Rig, hostname: upgrigubu, username: rig, password: '$HASH'}
  storage:
    $STORAGE
  apt: {fallback: offline-install, geoip: false}
  codecs: {install: false}
  drivers: {install: false}
  shutdown: poweroff
  late-commands:
    - sh /cdrom/upg/late.sh $ARM
  error-commands:
    - sh /cdrom/upg/error.sh $ARM
EOF
    cat > "$S/upg/common.sh" <<'EOF'
L=/run/upglog
facts() {
    echo "== $1 $(date -u +%FT%TZ)"; cat /proc/cmdline
    mokutil --sb-state 2>&1; od -An -tu1 /sys/firmware/efi/efivars/SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032bdc 2>&1
    lsblk -o NAME,SIZE,TYPE,FSTYPE,LABEL,PARTTYPENAME,MOUNTPOINTS 2>&1
    sfdisk -d /dev/sda 2>&1; sfdisk -d /dev/sdb 2>&1; sfdisk -d /dev/sdc 2>&1
    ip -br addr 2>&1; efibootmgr -v 2>&1
}
EOF
    cat > "$S/upg/early.sh" <<'EOF'
#!/bin/sh
. /cdrom/upg/common.sh
mkdir -p $L; mount -L UPGLOG $L || exit 0
{ facts early; ls -l /autoinstall.yaml; } > $L/early-$1.txt 2>&1
sync; umount $L
[ "$1" = refuse ] && { echo "upg: the early check refuses (exit 7)"; exit 7; }
exit 0
EOF
    cat > "$S/upg/error.sh" <<'EOF'
#!/bin/sh
. /cdrom/upg/common.sh
mkdir -p $L; mount -L UPGLOG $L || exit 0
{ facts error; } > $L/error-$1.txt 2>&1
tar czf $L/error-$1-installer.tgz /var/log/installer 2>/dev/null
journalctl -b --no-pager > $L/error-$1-journal.txt 2>&1
sync; umount $L
# the bench has nobody to read the screen: leave a minute for a screenshot, then turn off
(sleep 90; systemctl poweroff) &
exit 0
EOF
    cat > "$S/upg/late.sh" <<'EOF'
#!/bin/sh
. /cdrom/upg/common.sh
mkdir -p $L; mount -L UPGLOG $L || exit 0
{ facts late; cat /target/etc/fstab; find /target/boot/efi -maxdepth 3 | sort; cat /target/etc/apt/sources.list.d/* 2>&1 | head -40; } > $L/late-$1.txt 2>&1
tar czf $L/late-$1-installer.tgz /var/log/installer 2>/dev/null
journalctl -b --no-pager > $L/late-$1-journal.txt 2>&1
cp /cdrom/upg/common.sh /cdrom/upg/firstboot.sh /target/usr/local/sbin/
cat > /target/etc/systemd/system/upg-firstboot.service <<UNIT
[Unit]
Description=upgrade_ rig: record the first start, then turn off
After=multi-user.target
[Service]
Type=oneshot
ExecStart=/bin/sh /usr/local/sbin/firstboot.sh $1
[Install]
WantedBy=multi-user.target
UNIT
ln -sf /etc/systemd/system/upg-firstboot.service /target/etc/systemd/system/multi-user.target.wants/upg-firstboot.service
sync; umount $L
exit 0
EOF
    cat > "$S/upg/firstboot.sh" <<'EOF'
#!/bin/sh
. /usr/local/sbin/common.sh
mkdir -p $L; mount -L UPGLOG $L || exit 0
{ facts firstboot; uname -r; cat /etc/os-release | head -4
  timeout 900 snap wait system seed.loaded; echo "seed.loaded=$?"; snap list 2>&1; snap changes 2>&1
  sleep 20; systemctl is-active display-manager; systemctl is-system-running; systemctl --failed --no-pager
  dpkg -l | grep -c '^ii'; } > $L/firstboot-$1.txt 2>&1
sync; sleep 60; umount $L
systemctl poweroff
EOF
    cat > "$S/grub.cfg" <<'EOF'
set timeout=3
menuentry "Ubuntu, autoinstall from this stick (rig)" {
    set gfxpayload=keep
    linux  /casper/vmlinuz autoinstall ---
    initrd /casper/initrd
}
EOF
    if [ ! -f "$STICK_IMG" ] || [ "${REBUILD:-}" = 1 ]; then
        MB=$(( $(du -sb "$ROOT" | cut -f1) / 1048576 + 400 ))
        echo "ubuntu: stick image ${MB} MiB"
        rm -f "$STICK_IMG"; truncate -s "${MB}M" "$STICK_IMG"
        parted -s "$STICK_IMG" mklabel msdos mkpart primary fat32 1MiB 100% set 1 boot on
        mkfs.fat -F 32 -n UPGV0 --offset 2048 "$STICK_IMG" >/dev/null
        (cd "$ROOT" && mcopy -s -o -i "$STICK_IMG@@1M" ./* ./.disk ::/)
    fi
    P="$STICK_IMG@@1M"
    mcopy -o -i "$P" "$S/grub.cfg" ::/boot/grub/grub.cfg
    mcopy -o -i "$P" "$S/autoinstall.yaml" ::/autoinstall.yaml
    mmd -i "$P" ::/upg 2>/dev/null || true
    mcopy -o -i "$P" "$S"/upg/* ::/upg/
    mdir -i "$P" ::/ | head -30
    [ "$(state)" = Off ] || [ -z "$(state)" ] || { echo "ubuntu: $VMNAME is running" >&2; exit 1; }
    rm -f "$HV/vm/ubu-stick.vhdx"; qemu-img convert -f raw -O vhdx "$STICK_IMG" "$HV/vm/ubu-stick.vhdx"
    evict "$STICK_IMG"; find "$ROOT" -type f -size +50M | while read -r f; do evict "$f"; done
    echo "ubuntu: stick built for arm $ARM"
    ;;
vm)
    DISK="${2:-}"
    rm -f "$LOG_IMG"; truncate -s 96M "$LOG_IMG"
    parted -s "$LOG_IMG" mklabel msdos mkpart primary fat32 1MiB 100%
    mkfs.fat -F 32 -n UPGLOG --offset 2048 "$LOG_IMG" >/dev/null
    rm -f "$HV/vm/ubu-log.vhdx"; qemu-img convert -f raw -O vhdx "$LOG_IMG" "$HV/vm/ubu-log.vhdx"
    if [ -z "$DISK" ]; then NEW="-NewVHDPath '$HVW\\vm\\ubu-target.vhdx' -NewVHDSizeBytes 40GB"; else NEW="-VHDPath '$DISK'"; fi
    PSC "\$ErrorActionPreference='Stop'
      if (Get-VM -Name $VMNAME -ErrorAction SilentlyContinue) { throw '$VMNAME exists: run clean first' }
      \$vm = New-VM -Name $VMNAME -Generation 2 -MemoryStartupBytes 4GB $NEW -Path '$HVW\\vm'
      Set-VM -VM \$vm -ProcessorCount 4 -AutomaticCheckpointsEnabled \$false -AutomaticStopAction ShutDown
      Set-VMMemory -VM \$vm -DynamicMemoryEnabled \$false
      Add-VMHardDiskDrive -VM \$vm -ControllerType SCSI -Path '$HVW\\vm\\ubu-stick.vhdx'
      Add-VMHardDiskDrive -VM \$vm -ControllerType SCSI -Path '$HVW\\vm\\ubu-log.vhdx'
      \$stick = Get-VMHardDiskDrive -VM \$vm | Where-Object { \$_.Path -like '*ubu-stick.vhdx' }
      Set-VMFirmware -VM \$vm -EnableSecureBoot On -SecureBootTemplate MicrosoftUEFICertificateAuthority -FirstBootDevice \$stick
      Get-VMNetworkAdapter -VM \$vm | Format-List Name, SwitchName, Status, Connected"
    PS fw; PS disk list
    ;;
run)
    TAG="${2:?tag}"; MAX="${3:-3600}"; mkdir -p "$A/$TAG"
    python3 evict-builds.py --wait "$A"
    PS fw > "$A/$TAG/fw-before.txt"; PS start; t0=$(date +%s); n=0
    while :; do
        sleep 60; n=$((n+1)); el=$(( $(date +%s) - t0 ))
        st="$(state)"
        if [ "$st" != Running ]; then echo "ubuntu: guest is $st after $el s"; break; fi
        PS shot "$HVW\\shots\\ubu-$TAG-$(printf %02d $n).png" >/dev/null 2>&1 || true
        cp "$HV/shots/ubu-$TAG-$(printf %02d $n).png" "$A/$TAG/" 2>/dev/null || true
        [ "$el" -ge "$MAX" ] && { echo "ubuntu: still running after $el s; left running"; break; }
    done
    PS fw > "$A/$TAG/fw-after.txt"; cat "$A/$TAG/fw-after.txt"
    ;;
log)
    TAG="${2:?tag}"; mkdir -p "$A/$TAG/log"
    [ "$(state)" = Off ] || { echo "ubuntu: turn the guest off first" >&2; exit 1; }
    evict "$HV/vm/ubu-log.vhdx"
    qemu-img convert -f vhdx -O raw "$HV/vm/ubu-log.vhdx" "$A/$TAG/log.img"
    mcopy -s -n -i "$A/$TAG/log.img@@1M" ::/ "$A/$TAG/log/" ; ls -la "$A/$TAG/log"
    ;;
fw)   PS fw; PS disk list ;;
shot) PS shot "$HVW\\shots\\ubu-$2.png" >/dev/null; cp "$HV/shots/ubu-$2.png" "$A/"; echo "$A/ubu-$2.png" ;;
first)
    case "$2" in stick) pat='*ubu-stick.vhdx' ;; disk) pat="${3:-*ubu-target.vhdx}" ;; esac
    PSC "Set-VMFirmware -VMName $VMNAME -FirstBootDevice (Get-VMHardDiskDrive -VMName $VMNAME | Where-Object { \$_.Path -like '$pat' })"; PS fw ;;
detach-stick) PSC "Get-VMHardDiskDrive -VMName $VMNAME | Where-Object { \$_.Path -like '*ubu-stick.vhdx' } | Remove-VMHardDiskDrive"; PS fw ;;
attach-stick) PSC "Add-VMHardDiskDrive -VMName $VMNAME -ControllerType SCSI -Path '$HVW\\vm\\ubu-stick.vhdx'"; PS fw ;;
clean)
    PSC "if (Get-VM -Name $VMNAME -ErrorAction SilentlyContinue) { Stop-VM -Name $VMNAME -TurnOff -Force -ErrorAction SilentlyContinue; Remove-VM -Name $VMNAME -Force }"
    rm -f "$HV"/vm/ubu-stick.vhdx "$HV"/vm/ubu-log.vhdx "$HV"/vm/ubu-target.vhdx "$HV"/vm/ubu-windows.vhdx
    rm -rf "$HV/vm/$VMNAME" "$STICK_IMG" "$LOG_IMG" "$ROOT"
    echo "ubuntu: guest and disks removed"
    ;;
*) sed -n 2,19p "$0"; exit 1 ;;
esac
