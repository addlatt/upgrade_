#!/bin/bash
#
# collect-linux.sh - a real machine's evidence from the installed Linux, for the
# verdict scripts (v9-verdict.py, v2-verdict.py, settle-in-verdict.py).
#
#   sudo bash collect-linux.sh [--esp]
#
# A product stick carries no bench marker (outcome.sh writes upg-mark only when
# the stick says "bench"), so on a real machine the marker's own commands are
# run by hand, over the carried SSH key. This is that, kept in the tree so a
# physical row can be made the same way twice (first used 2026-10-04, the
# Aspire's run 11).
#
# Reads only. The stick is mounted read-only. Writes one new folder,
# ~/upgrade-evidence/<UTC>/, owned by the person:
#   var-lib-upgrade_/      the handoff folder (job.json, outcome.json, settle-in/report.json)
#   stick/                 the stick's records (no images, no binaries)
#   settle-in/             the marker's first-start captures (nm-parsed, sessions, clock, order, efibootmgr)
#   boot-line.txt          the marker's linux-boot line for THIS boot
#   first-mib-system.raw, first-mib-home.raw   each internal drive's partition table (no file data:
#                          the first partition starts at 1 MiB)
#   esp.raw                with --esp: the EFI System Partition, whole (boot files only; for
#                          rig/hyperv/physical/assemble-image.py and the offline inspector)
#   SHA256SUMS             of everything above, made here, checked again after the copy
set -u
U=${SUDO_USER:?run this with sudo}; T=$(date -u +%Y%m%dT%H%M%SZ); O=/home/$U/upgrade-evidence/$T; C=$O/settle-in
WANT_ESP=0; [ "${1:-}" = --esp ] && WANT_ESP=1
mkdir -p "$C" "$O/stick/EFI/BOOT"

cp -a /var/lib/upgrade_ "$O/var-lib-upgrade_" 2>/dev/null; ls -laR /var/lib/upgrade_ > "$O/var-lib-ls.txt" 2>&1
sshd -T 2>/dev/null | grep -i 'passwordauthentication\|pubkeyauthentication\|kbdinteractiveauthentication\|permitrootlogin' > "$O/sshd-T.txt"

# --- the bench marker's own lines (upgrade_/linux/outcome.sh, upg-mark) ---
cur=$(efibootmgr 2>/dev/null | awk '/BootCurrent/{print $2}')
sha=$(sha256sum /boot/efi/EFI/Microsoft/Boot/bootmgfw.efi 2>/dev/null | cut -c1-64)
u=$U; pw=$(getent shadow "$u" 2>/dev/null | cut -d: -f2 | tr -d '\n' | sha256sum | cut -c1-64)
tgt=$(systemctl get-default 2>/dev/null); dm=$(systemctl is-active display-manager 2>/dev/null)
hsrc=$(findmnt -no SOURCE /home 2>/dev/null); hdisk=$( [ -n "$hsrc" ] && lsblk -no PKNAME "$hsrc" 2>/dev/null | head -1 ); rdisk=$(lsblk -no PKNAME "$(findmnt -no SOURCE /)" 2>/dev/null | head -1)
cp /var/lib/upgrade_/settle-in/report.json "$C/report.json" 2>/dev/null
nmcli -t -f UUID,FILENAME connection show 2>/dev/null | while IFS=: read -r cu f; do
    case "$f" in */upgrade_-*) ;; *) continue;; esac
    printf '%s|%s|%s|%s|%s|%s\n' "$(basename "$f")" "$(nmcli -g 802-11-wireless.ssid connection show "$cu")" "$(nmcli -g 802-11-wireless.hidden connection show "$cu")" \
        "$(nmcli -g connection.autoconnect connection show "$cu")" "$(nmcli -g 802-11-wireless-security.key-mgmt connection show "$cu")" "$(stat -c %a "$f")" >> "$C/nm-parsed.txt"
done
journalctl -b -o short-monotonic --no-pager | grep -E 'Starting Wayland user session|Started plasma-kwin_wayland|pam_unix\(gdm-password:session\): session opened|Started org.gnome.Shell@(wayland|user)|sddm-helper exited|settle-in-window: ' > "$C/sessions.txt" 2>&1
nmcli -t -f NAME,UUID,TYPE,FILENAME connection show > "$C/nm-all.txt" 2>&1
ls -la /var/lib/upgrade_ /var/lib/upgrade_/artifacts/credentials > "$C/handoff-ls.txt" 2>&1
{ date -u +%s; cat /sys/class/rtc/rtc0/since_epoch; timedatectl show 2>&1; tail -1 /etc/adjtime; } > "$C/clock.txt" 2>&1
journalctl -b -o short-monotonic -u upgrade_-settle-in -u NetworkManager -u chronyd --no-pager 2>&1 | head -60 > "$C/order.txt"
journalctl -b -u upgrade_-settle-in --no-pager > "$C/settle-in.log" 2>&1
efibootmgr -v > "$C/efibootmgr-$T.txt" 2>&1
journalctl --list-boots --no-pager > "$O/list-boots.txt"
printf 'linux-boot,%s,%s,BootCurrent=%s,bootmgfw_sha256=%s,user=%s,pw_sha256=%s,home_dir=%s,home_disk=%s,root_disk=%s,default_target=%s,display_manager=%s\n' "$(date -u +%FT%TZ)" "$(uname -r)" "$cur" "$sha" "$u" "$pw" "$([ -d "/home/$u" ] && echo present || echo missing)" "${hdisk:-none}" "$rdisk" "$tgt" "$dm" > "$O/boot-line.txt"

# --- the drives: partition tables, and the ESP when asked ---
dd if="/dev/$rdisk" of="$O/first-mib-system.raw" bs=1M count=1 status=none
[ -n "${hdisk:-}" ] && [ "$hdisk" != "$rdisk" ] && dd if="/dev/$hdisk" of="$O/first-mib-home.raw" bs=1M count=1 status=none
lsblk -b -o NAME,SIZE,TYPE,TRAN,MODEL,SERIAL,PARTTYPENAME,FSTYPE,MOUNTPOINTS > "$O/lsblk.txt"
if [ "$WANT_ESP" = 1 ]; then
    esp=$(findmnt -no SOURCE /boot/efi); sync
    dd if="$esp" of="$O/esp.raw" bs=1M status=none && echo "$esp $(blockdev --getsize64 "$esp") bytes, starts at sector $(cat "/sys/class/block/$(basename "$esp")/start")" > "$O/esp.txt"
    grep -i 'windows\|menuentry ' /boot/grub2/grub.cfg > "$O/grub-menu.txt" 2>&1
fi

# --- the stick, read-only ---
M=$(mktemp -d)
if [ -e /dev/disk/by-label/UPGV0 ] && mount -o ro /dev/disk/by-label/UPGV0 "$M"; then
    (cd "$M" && find . -type f -printf '%s %TY-%Tm-%TdT%TH:%TM:%TS %p\n' | sort -k3) > "$O/stick-files.txt"
    ls -la "$M/upgrade_" > "$O/stick-upgrade-dir.txt"
    ls -la "$M/upgrade_/artifacts/credentials" > "$O/stick-credentials-dir.txt" 2>&1
    for f in KIT-MANIFEST.txt SHA256SUMS release.json machine-capture.json stick.json; do cp "$M/$f" "$O/stick/" 2>/dev/null; done
    cp "$M/EFI/BOOT/grubenv" "$M/EFI/BOOT/grub.cfg" "$O/stick/EFI/BOOT/" 2>/dev/null
    (cd "$M" && tar --exclude=upgrade_/LiveOS --exclude=upgrade_/settle-in --exclude=upgrade_/boot-chain -cf - upgrade_) | tar -C "$O/stick" -xf -
    umount "$M"
else echo "the stick UPGV0 is not in, or could not be read" > "$O/stick-not-read.txt"; fi
rmdir "$M" 2>/dev/null

(cd "$O" && find . -type f ! -name SHA256SUMS -exec sha256sum {} + > SHA256SUMS)
chown -R "$U": "/home/$U/upgrade-evidence"
echo "DONE $O"
