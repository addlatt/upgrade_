#!/bin/bash
# Fedora-side run hook for the Hyper-V rig (installed by v3-bootstrap.sh into
# /usr/local/sbin/oemdrv-run, run by oemdrv-run.service on every boot).
#
# Transport contract with the host (rig/hyperv/v3.sh): the host writes a
# script to OEMDRV:/run.sh; this hook mounts the volume, runs the script as
# root with stdout+stderr captured to OEMDRV:/run.log, then reads
# OEMDRV:/next — 'poweroff' | 'reboot-windows' | anything else = stay up —
# and acts on it AFTER the volume is synced and unmounted. run.sh is copied
# off the volume before it runs so it can be renamed/removed on the volume
# (a script that must run only once removes itself: rm /mnt/oemdrv/run.sh).
dev=/dev/disk/by-label/OEMDRV
for i in $(seq 1 30); do [ -e "$dev" ] && break; sleep 1; done
[ -e "$dev" ] || exit 0
mkdir -p /mnt/oemdrv
mountpoint -q /mnt/oemdrv || mount "$dev" /mnt/oemdrv || exit 0
next=''
if [ -f /mnt/oemdrv/run.sh ]; then
    cp /mnt/oemdrv/run.sh /run/oemdrv-run.sh
    {
        echo "== oemdrv-run $(date -u +%FT%TZ) $(uname -r)"
        bash /run/oemdrv-run.sh
        echo "== run.sh rc=$?"
    } > /mnt/oemdrv/run.log 2>&1
    [ -f /mnt/oemdrv/next ] && next=$(tr -d '\r\n ' < /mnt/oemdrv/next)
    rm -f /mnt/oemdrv/next
fi
sync; umount /mnt/oemdrv
case "$next" in
    poweroff)       systemctl poweroff ;;
    reboot-windows) grub2-reboot 'Windows Boot Manager (on /dev/sda1)' 2>/dev/null || grub2-reboot 2; systemctl reboot ;;
    reboot)         systemctl reboot ;;
esac
exit 0
