#!/bin/bash
# V3 wrapper for the OEMDRV run hook: first boot upgrades the guest's kernel
# (dnf, needs the guest's NAT network), records what it installed and reboots;
# the next boot runs the reader (OEMDRV:/v3/read.sh = guest/v3-read.sh, put
# there by v3.sh rearm-upgrade). Is the ntfs3 oops still there on the current
# Fedora 42 kernel? (settle-in runs on a freshly installed, then updated, system.)
OEM=/mnt/oemdrv
if [ ! -f "$OEM/v3/kernel-upgrade.done" ]; then
    { echo "== before: $(uname -r)"; rpm -q kernel-core; date -u
      dnf -y upgrade 'kernel*' ntfs-3g cryptsetup
      echo "dnf rc=$?"; rpm -q kernel-core ntfs-3g cryptsetup; date -u; } > "$OEM/v3/kernel-upgrade.log" 2>&1
    cp "$OEM/v3/kernel-upgrade.log" "$OEM/v3/kernel-upgrade.done"; sync
    echo reboot > "$OEM/next"
    exit 0
fi
exec bash "$OEM/v3/read.sh"
