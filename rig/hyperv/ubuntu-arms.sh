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
#   (arm D, not again: the early check's refusal of a second start. Run on the first stick only; see below)
#   ubuntu-arms.sh arm K [NOTE]   beside Windows: the system disk is a copy of the rig's shrunk Windows disk
#                                 (UPGRIGHV.pre-install.vhdx). Every partition of it is hashed before and after
#   ubuntu-arms.sh arm G [NOTE]   two free spaces: a made-up disk with a boot partition holding a file named as
#                                 Windows' loader, 30 GiB free, a data partition, 40 GiB free. Refused, nothing written
#   ubuntu-arms.sh arm H [NOTE]   hand over: after C, the stick first in the order. Its menu starts the installed
#                                 Ubuntu, not the installer
#
# From 2026-11-13 (the row at line 10) the stick is laid out as make-kit.sh and upgrade-fetch lay the
# product's out: the kit's own start menu for Ubuntu (handoff-payload/grub-ubuntu.cfg), Fedora's menu
# and the mark's block beside it in EFI/BOOT, release.json, settle-in. Arm D (the early check's own
# refusal of a second start) cannot be reached from that menu and is not run on it; its row stands.
#   ubuntu-arms.sh arm W [NOTE]   the Windows side: a copy of the rig's install-day Windows starts, the product's
#                                 prologue (upgrade-prologue.exe start, the erase sentence) reads the job and the
#                                 stick's answer file, arms the handoff and restarts; the stick's Ubuntu erases and
#                                 installs. Secure Boot OFF for this arm only (this firmware trusts one signer at a
#                                 time). The job is the rig's stand-in (v1.sh job, v9-job.py): Hyper-V has no USB,
#                                 so the product's job writer refuses the rig's stick, as it must (R16)
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
PAYLOAD="$REPO/upgrade_/windows/handoff-payload"
SETTLE_IN="$REPO/settle-in/target/x86_64-unknown-linux-musl/release/settle-in"
SETTLE_IN_WINDOW="$REPO/settle-in/window/target/release/settle-in-window"
BOGUS_UID=60022480ffffffffffffffffffffffff
SYSTEM_BYTES=42949672960
VMPS1="$(wslpath -w vm.ps1)"
PS()  { powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$VMPS1" "$@" -Name "$VMNAME" < /dev/null | tr -d '\r'; }
PSC() { powershell.exe -NoProfile -Command "$1" < /dev/null | tr -d '\r'; }
evict() { python3 -c 'import os,sys; fd=os.open(sys.argv[1],os.O_RDONLY); os.posix_fadvise(fd,0,0,os.POSIX_FADV_DONTNEED); os.close(fd)' "$1" 2>/dev/null || true; }
guest() { PS ps "$1" 2>&1; }
stick_letter() { guest '(Get-Volume -FileSystemLabel UPGV0 -ErrorAction SilentlyContinue | Select-Object -First 1).DriveLetter' | tr -d ' \n'; }
WINEXE="$REPO/target/x86_64-pc-windows-gnu/release"
ERASE_SENTENCE='I confirm that everything on this computer will be deleted and nothing will be kept'
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
    # the product's own menu instead (2026-11-13), and below the rest of what a kit stick carries
    cp "$PAYLOAD/grub-ubuntu.cfg" "$S/grub.cfg"
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
    # arm W: the prologue arms the stick itself
    [ "$arm" = W ] || echo "armed by ubuntu-arms.sh arm $arm" > "$S/upgrade_/boot-install"
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
  echo "boot_id=$(cat /proc/sys/kernel/random/boot_id)"; echo "boots=$(journalctl --list-boots -q | wc -l)"
  echo "ssh_unit=$(systemctl is-active ssh.service)"; echo "ssh_enabled=$(systemctl is-enabled ssh.service 2>&1)"; echo "ssh_socket=$(systemctl is-active ssh.socket 2>&1)"; echo "ssh_keys_sha256=$(sha256sum < /home/rig/.ssh/authorized_keys 2>/dev/null | cut -d' ' -f1)"
  echo "ssh_password_signin=$(sshd -T 2>/dev/null | grep -i '^passwordauthentication' | cut -d' ' -f2)"; echo "cryptsetup=$(command -v cryptsetup)"; echo "downloaders=$(for t in curl wget; do command -v $t; done | tr '\n' ' ')"
  echo "si_done=$(test -f /var/lib/upgrade_/settle-in/first-start.done && echo y || echo n)"; echo "si_unit=$(systemctl is-active upgrade_-settle-in.service)"
  cp /var/lib/upgrade_/settle-in/report.json $L/settle-in-report.json 2>/dev/null
  sleep 45; echo "display_manager=$(systemctl is-active display-manager)"
  echo "== lsblk"; lsblk -b -o NAME,SIZE,TYPE,FSTYPE,MOUNTPOINTS
  echo "== net"; ip -br addr; echo "== efibootmgr"; efibootmgr
} > $L/firstboot.txt 2>&1
sync; umount $L; systemctl poweroff
B
    { grep -v ' boot/grub/grub.cfg$' "$A/SHA256SUMS.iso"; echo "$(sha256sum < "$S/grub.cfg" | cut -d' ' -f1)  boot/grub/grub.cfg"; } > "$S/SHA256SUMS"
    if true; then
        mkdir -p "$S/upgrade_/settle-in" "$S/efi"
        cp "$PAYLOAD/grub.cfg" "$PAYLOAD/grubenv" "$S/efi/"
        cp "$SETTLE_IN" "$S/upgrade_/settle-in/settle-in"; cp "$SETTLE_IN_WINDOW" "$S/upgrade_/settle-in/settle-in-window"
        for f in upgrade_-settle-in.service upgrade_-settle-in.desktop upgrade_-settle-in.sh org.upgrade.settle-in.policy upgrade_-go-back-to-windows.desktop upgrade_-bring-files.desktop; do
            sed 's/\r$//' "$REPO/settle-in/linux/$f" > "$S/upgrade_/settle-in/$f"
        done
        python3 - "$REPO/data/releases.json" > "$S/release.json" <<'P'
import json, sys
r = next(x for x in json.load(open(sys.argv[1]))["Releases"] if x["Id"] == "ubuntu-26.04.1")
print(json.dumps({"id": r["Id"], "name": r["Name"], "family": r["Family"], "release": r["Id"].rsplit("-", 1)[-1], "measured": r["Measured"], "desktop": "gnome"}, indent=2))
P
        { echo "$(sha256sum < "$S/efi/grub.cfg" | cut -d' ' -f1)  ./EFI/BOOT/grub.cfg"; echo "$(sha256sum < "$S/efi/grubenv" | cut -d' ' -f1)  ./EFI/BOOT/grubenv"
          echo "$(sha256sum < "$S/release.json" | cut -d' ' -f1)  ./release.json"
          (cd "$S" && find upgrade_/settle-in -type f | sort | while read -r f; do echo "$(sha256sum < "$f" | cut -d' ' -f1)  ./$f"; done); } >> "$S/SHA256SUMS"
    fi
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
    mcopy -o -i "$P" "$S/efi/grub.cfg" "$S/efi/grubenv" ::/EFI/boot/; mcopy -o -i "$P" "$S/release.json" ::/
    mcopy -s -o -i "$P" "$S/upgrade_" ::/
    mdel -i "$P" ::/upgrade-prologue.exe ::/upgrade-job.exe 2>/dev/null || true
    if [ "$arm" = W ]; then
        [ -f "$WINEXE/upgrade-prologue.exe" ] && [ -f "$WINEXE/upgrade-job.exe" ] || { echo "ubuntu-arms: run ./build-rust.sh first (arm W needs the Windows programs)" >&2; exit 1; }
        mcopy -o -i "$P" "$WINEXE/upgrade-prologue.exe" "$WINEXE/upgrade-job.exe" ::/
    fi
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
    mcopy -n -i "$d/stick.raw@@1M" ::/EFI/boot/grubenv "$d/stick/grubenv" 2>/dev/null || true
    rm -f "$d/stick/upgrade_/upgrade-installer" "$d/stick.raw"
}

run_arm() {  # $1 = arm dir name, $2 = seconds
    ./ubuntu.sh run "$1" "$2" | tail -3
    state > "$A/$1/end-state.txt"
    [ "$(state)" = Off ] || ./ubuntu.sh shot "$1-end" >/dev/null 2>&1 || true
    off
}

windows_side() {  # $1 = arm dir name, $2 = seconds. Windows up, the job, the prologue, then wait for the guest to turn itself off
    local T="$1" L t0 n=0 made=""
    python3 evict-builds.py --wait "$A"
    PS fw > "$A/$T/fw-before.txt"; PS start; t0=$(date +%s)
    until guest 'hostname' 2>/dev/null | grep -qx 'UPGRIGHV'; do
        [ $(( $(date +%s) - t0 )) -lt 900 ] || { echo "ubuntu-arms: Windows did not answer within 900 s" | tee "$A/$T/windows-side.txt"; ./ubuntu.sh shot "$T-nowindows" >/dev/null 2>&1 || true; off; return 0; }
        sleep 10
    done
    echo "ubuntu-arms: Windows up after $(( $(date +%s) - t0 )) s"
    guest '"windows_sb=" + $(try { Confirm-SecureBootUEFI } catch { "unknown" }); "windows_caption=" + (Get-CimInstance Win32_OperatingSystem).Caption' > "$A/$T/windows-side.txt"
    L=$(stick_letter); [ -n "$L" ] || { echo "ubuntu-arms: Windows sees no UPGV0 volume" | tee -a "$A/$T/windows-side.txt"; off; return 0; }
    guest "'answer_file_sha256=' + (Get-FileHash ${L}:\\autoinstall.yaml).Hash.ToLower()" >> "$A/$T/windows-side.txt"
    # v1.sh job reads the guest's real facts and runs the product's settings harvest from the stick. It also wants a
    # kit's file list for Fedora's kickstart, which this stick does not use: the stick's own list stands in, and the kickstart is deleted
    [ -f "$REPO/dist/kit/stick/SHA256SUMS" ] || { mkdir -p "$REPO/dist/kit/stick"; cp "$A/arms-extra/SHA256SUMS" "$REPO/dist/kit/stick/SHA256SUMS"; made=y; }
    VMNAME=$VMNAME ./v1.sh job > "$A/$T/v1-job.log" 2>&1 || echo "v1.sh job failed: read v1-job.log" >> "$A/$T/windows-side.txt"
    [ -z "$made" ] || { rm -f "$REPO/dist/kit/stick/SHA256SUMS"; rmdir "$REPO/dist/kit/stick" "$REPO/dist/kit" "$REPO/dist" 2>/dev/null || true; }
    python3 - artifacts/v1/facts.json "$A/$T/disks.json" <<'P' || true
import json, sys
f = json.load(open(sys.argv[1])); d = f["disk"]
json.dump([{"number": d["number"], "unique_id": d["unique_id"], "serial": d["serial"], "size": d["size"], "name": d["name"], "health": f.get("health") or "Healthy"}], open(sys.argv[2], "w"))
P
    python3 v9-job.py artifacts/v1/job.json "$A/$T/disks.json" "$A/$T/job-erase.json" --desktop=gnome > "$A/$T/v9-job.log" 2>&1 || true
    python3 - "$A/$T/job-erase.json" "$A/$T/job.json" "$REPO/schemas/job.schema.json" <<'P' || { echo "no job could be written" >> "$A/$T/windows-side.txt"; off; return 0; }
import json, sys
from jsonschema import Draft7Validator, FormatChecker
j = json.load(open(sys.argv[1])); j["intent"]["distro"] = {"name": "ubuntu", "release": "26.04.1"}
e = list(Draft7Validator(json.load(open(sys.argv[3])), format_checker=FormatChecker()).iter_errors(j))
if e: sys.exit("schema: " + e[0].message)
json.dump(j, open(sys.argv[2], "w"), indent=2)
P
    PS copy "$(wslpath -w "$A/$T/job.json")" "${L}:\\upgrade_\\job.json" > /dev/null
    guest "Remove-Item ${L}:\\upgrade_\\ks.cfg -Force -ErrorAction SilentlyContinue; Remove-Item -Recurse -Force 'C:\\ProgramData\\upgrade_\\prologue' -ErrorAction SilentlyContinue; 'stick_upgrade_dir=' + ((Get-ChildItem ${L}:\\upgrade_ -Name) -join ',')" >> "$A/$T/windows-side.txt"
    guest "& ${L}:\\upgrade-prologue.exe start --stick ${L}: --erase-consent '$ERASE_SENTENCE'" > "$A/$T/prologue-start.log" 2>&1 || true
    tail -5 "$A/$T/prologue-start.log"
    if ! grep -q 'restarting in' "$A/$T/prologue-start.log"; then
        echo "the prologue did not reach a restart" >> "$A/$T/windows-side.txt"; off; return 0
    fi
    echo "prologue_restart=y" >> "$A/$T/windows-side.txt"
    while :; do
        sleep 60; n=$((n+1))
        [ "$(state)" = Running ] || { echo "ubuntu-arms: guest is $(state) after $(( $(date +%s) - t0 )) s"; break; }
        PS shot "$HVW\\shots\\ubu-$T-$(printf %02d $n).png" >/dev/null 2>&1 || true
        cp "$HV/shots/ubu-$T-$(printf %02d $n).png" "$A/$T/" 2>/dev/null || true
        [ $(( $(date +%s) - t0 )) -lt "$2" ] || { echo "ubuntu-arms: still running after $2 s"; ./ubuntu.sh shot "$T-end" >/dev/null 2>&1 || true; break; }
    done
    state > "$A/$T/end-state.txt"; PS fw > "$A/$T/fw-after.txt"; off
}

case "${1:-}" in
kit)
    (cd "$REPO/upgrade_/linux/installer" && cargo build -q --release --target x86_64-unknown-linux-musl)
    (cd "$REPO" && cargo build -q -p upgrade-autoinstall)
    # settle-in, as make-kit.sh builds it
    (cd "$REPO/settle-in" && cargo build -q --locked --release --target x86_64-unknown-linux-musl)
    (cd "$REPO/settle-in/window" && cargo build -q --locked --release)
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
    ARM="${2:?A|F|C|K|G|H|W}"; NOTE="${3:-}"; T="arm-$ARM"; D="$A/$T"
    [ "$ARM" != D ] || { echo "ubuntu-arms: arm D is not run on the product's stick (its menu never starts the installer again); see arm H" >&2; exit 1; }
    rm -rf "$D"; mkdir -p "$D"
    SYSDISK=target; [ "$ARM" = K ] && SYSDISK=windows; [ "$ARM" = G ] && SYSDISK=gaps; [ "$ARM" = W ] && SYSDISK=windows
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
        # remote access as a Windows that had it on would hand it over (a made-up key: a spoof)
        rm -f "$D/spoof-key" "$D/spoof-key.pub"; ssh-keygen -q -t ed25519 -N '' -C 'rig spoof' -f "$D/spoof-key"; rm -f "$D/spoof-key"
        python3 ubuntu-job.py "$D/job.json" --uid="$CUID" --size=$SYSTEM_BYTES --serial="${CSERIAL:-}" --hash="$HASH" --spoof-ssh="$D/spoof-key.pub"
        build_stick C "$D/job.json"; SECS=3600 ;;
    H)  [ -f "$A/arm-C/job.json" ] && [ -f "$A/arm-C/log/firstboot.txt" ] || { echo "ubuntu-arms: arm H runs after arm C, on the stick and the disk C left" >&2; exit 1; }
        cp "$A/arm-C/job.json" "$D/job.json"; cp "$A/arm-C/log/firstboot.txt" "$D/firstboot-C.txt"; off; SECS=600 ;;
    K)  # always a fresh copy: a disk an earlier run put Linux on has no free space, and the check refuses it (line 13)
        off; PSC "Get-VMHardDiskDrive -VMName $VMNAME | Where-Object { \$_.Path -like '*ubu-windows.vhdx' } | Remove-VMHardDiskDrive; Remove-Item '$HVW\\vm\\ubu-windows.vhdx' -ErrorAction SilentlyContinue; exit 0"
        if [ ! -f "$HV/vm/ubu-windows.vhdx" ]; then
            # in the foreground, and compared: a copy cut short is a full-length file with a garbage tail (2026-11-12)
            PSC "\$ErrorActionPreference='Stop'; Copy-Item '$HVW\\vm\\UPGRIGHV.pre-install.vhdx' '$HVW\\vm\\ubu-windows.vhdx'
              \$a = (Get-FileHash '$HVW\\vm\\UPGRIGHV.pre-install.vhdx').Hash; \$b = (Get-FileHash '$HVW\\vm\\ubu-windows.vhdx').Hash
              if (\$a -ne \$b) { throw 'the copy of the Windows disk does not match' }; 'windows disk copied: ' + \$b"
        fi
        WUID="$(wwn_of ubu-windows.vhdx)"; WSIZE="$(PSC "(Get-VHD '$HVW\\vm\\ubu-windows.vhdx').Size" | tr -d ' \n')"
        python3 ubuntu-job.py "$D/job.json" --keep --uid="$WUID" --size="$WSIZE" --hash="$HASH"
        build_stick K "$D/job.json"; SECS=3600 ;;
    W)  # the rig's install-day Windows, a fresh copy every time
        off; PSC "Get-VMHardDiskDrive -VMName $VMNAME | Where-Object { \$_.Path -like '*ubu-windows.vhdx' } | Remove-VMHardDiskDrive; Remove-Item '$HVW\\vm\\ubu-windows.vhdx' -ErrorAction SilentlyContinue; exit 0"
        PSC "\$ErrorActionPreference='Stop'; Copy-Item '$HVW\\vm\\UPGRIGHV.fresh.vhdx' '$HVW\\vm\\ubu-windows.vhdx'
          \$a = (Get-FileHash '$HVW\\vm\\UPGRIGHV.fresh.vhdx').Hash; \$b = (Get-FileHash '$HVW\\vm\\ubu-windows.vhdx').Hash
          if (\$a -ne \$b) { throw 'the copy of the Windows disk does not match' }; 'windows disk copied: ' + \$b"
        python3 ubuntu-job.py "$D/placeholder.json" --uid=$BOGUS_UID --size=$SYSTEM_BYTES --hash="$HASH"
        build_stick W "$D/placeholder.json"; SECS=3600 ;;
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
    if [ "$ARM" = W ]; then
        ./ubuntu.sh first disk '*ubu-windows.vhdx' > /dev/null
        PSC "Set-VMFirmware -VMName $VMNAME -EnableSecureBoot Off; Enable-VMIntegrationService -VMName $VMNAME -Name 'Guest Service Interface'"
    else
        ./ubuntu.sh first stick > /dev/null
    fi
    PSC "Get-VMNetworkAdapter -VMName $VMNAME | ForEach-Object { 'adapter=' + \$_.Name + ' switch=[' + \$_.SwitchName + ']' }; (Get-VMFirmware -VMName $VMNAME | ForEach-Object { 'secure_boot=' + \$_.SecureBoot + ' template=' + \$_.SecureBootTemplate })" > "$D/guest.txt"
    echo "installer_sha256=$(cat "$A/installer.sha256" 2>/dev/null)" >> "$D/guest.txt"
    echo "stick_location=$(PSC "(Get-VMHardDiskDrive -VMName $VMNAME | Where-Object { \$_.Path -like '*ubu-stick.vhdx' }).ControllerLocation" | tr -d ' \n')" >> "$D/guest.txt"
    off; hashes before > "$D/hashes-before.txt"
    # an empty log disk for every arm: a first-start record on it is this arm's or nobody's
    rm -f "$A/ubu-log.img"; truncate -s 96M "$A/ubu-log.img"; parted -s "$A/ubu-log.img" mklabel msdos mkpart primary fat32 1MiB 100%
    mkfs.fat -F 32 -n UPGLOG --offset 2048 "$A/ubu-log.img" > /dev/null
    rm -f "$HV/vm/ubu-log.vhdx"; qemu-img convert -f raw -O vhdx "$A/ubu-log.img" "$HV/vm/ubu-log.vhdx"
    if [ "$ARM" = W ]; then
        windows_side "$T" "$SECS"
        PSC "Set-VMFirmware -VMName $VMNAME -EnableSecureBoot On -SecureBootTemplate MicrosoftUEFICertificateAuthority"
    else
        run_arm "$T" "$SECS"
    fi
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
