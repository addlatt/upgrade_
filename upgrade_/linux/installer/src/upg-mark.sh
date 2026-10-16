#!/bin/bash
dev=/dev/disk/by-label/UPGV0
for i in $(seq 1 30); do [ -e "$dev" ] && break; sleep 1; done
[ -e "$dev" ] || exit 0
mkdir -p /mnt/upgstick; mount "$dev" /mnt/upgstick || exit 0
# a command the bench left on the stick (rig/hyperv/prologue.sh rollback, 2026-10-12): the Linux-side
# rollback runs here, as the person's click would run it; its answer and exit code stay on the stick
auto_cmd=0
if [ -f /mnt/upgstick/upgrade_/bench-command ]; then
    cmd=$(tr -d '\r\n' < /mnt/upgstick/upgrade_/bench-command); rm -f /mnt/upgstick/upgrade_/bench-command
    case "$cmd" in
        rollback)
            /usr/local/libexec/upgrade_/settle-in rollback --stick /mnt/upgstick > /mnt/upgstick/upgrade_/bench-command.out 2>&1; rc=$?
            echo "exit=$rc" >> /mnt/upgstick/upgrade_/bench-command.out
            printf 'bench-command,%s,rollback,exit=%s\n' "$(date -u +%FT%TZ)" "$rc" >> /mnt/upgstick/upgrade_/boots.log; auto_cmd=1 ;;
    esac
fi
cur=$(efibootmgr 2>/dev/null | awk '/BootCurrent/{print $2}')
sha=$(sha256sum /boot/efi/EFI/Microsoft/Boot/bootmgfw.efi 2>/dev/null | cut -c1-64)
# V9 (R27): the account the job created - a fingerprint of its stored hash, never the hash itself - and where /home lives
u=$(cat /etc/upg-mark.user 2>/dev/null); pw=$(getent shadow "$u" 2>/dev/null | cut -d: -f2 | tr -d '\n' | sha256sum | cut -c1-64)
# wait for a display manager only when the system starts at the desktop: on the console path none
# starts, and a 120 s wait let the next restart cut the marker off before it wrote (rig run 4, 2026-09-27)
tgt=$(systemctl get-default 2>/dev/null); dm=inactive; [ "$tgt" = graphical.target ] && for i in $(seq 1 60); do dm=$(systemctl is-active display-manager 2>/dev/null); [ "$dm" = active ] && break; sleep 2; done
hsrc=$(findmnt -no SOURCE /home 2>/dev/null); hdisk=$( [ -n "$hsrc" ] && lsblk -no PKNAME "$hsrc" 2>/dev/null | head -1 ); rdisk=$(lsblk -no PKNAME "$(findmnt -no SOURCE /)" 2>/dev/null | head -1)
# settle-in's first startup (2026-09-27): its report, what NetworkManager itself parsed from the
# files it wrote, the clocks, and the order the services started - captured once, to the stick
if [ ! -e /mnt/upgstick/upgrade_/settle-in-capture/done ] && [ -e /var/lib/upgrade_/settle-in/first-start.done ]; then
    C=/mnt/upgstick/upgrade_/settle-in-capture; mkdir -p "$C"
    cp /var/lib/upgrade_/settle-in/report.json "$C/report.json" 2>/dev/null
    # cu, not u: u holds the account name the row below records (rig run 1, 2026-09-27, clobbered it)
    # GENERAL.* exists only for active connections (rig run 2): map uuid -> file from the list instead
    nmcli -t -f UUID,FILENAME connection show 2>/dev/null | while IFS=: read -r cu f; do
        case "$f" in */upgrade_-*) ;; *) continue;; esac
        printf '%s|%s|%s|%s|%s|%s\n' "$(basename "$f")" "$(nmcli -g 802-11-wireless.ssid connection show "$cu")" "$(nmcli -g 802-11-wireless.hidden connection show "$cu")" \
            "$(nmcli -g connection.autoconnect connection show "$cu")" "$(nmcli -g 802-11-wireless-security.key-mgmt connection show "$cu")" "$(stat -c %a "$f")" >> "$C/nm-parsed.txt"
    done
    # the desktop session: did a user session reach the window manager? (rig run 2's black screen)
    journalctl -b -o short-monotonic --no-pager | grep -E 'Starting Wayland user session|Started plasma-kwin_wayland|Started gnome-shell|org.gnome.Shell|sddm-helper exited' > "$C/sessions.txt" 2>&1
    nmcli -t -f NAME,UUID,TYPE,FILENAME connection show > "$C/nm-all.txt" 2>&1
    ls -la /var/lib/upgrade_ /var/lib/upgrade_/artifacts/credentials > "$C/handoff-ls.txt" 2>&1
    { date -u +%s; cat /sys/class/rtc/rtc0/since_epoch; timedatectl show 2>&1; tail -1 /etc/adjtime; } > "$C/clock.txt" 2>&1
    journalctl -b -o short-monotonic -u upgrade_-settle-in -u NetworkManager -u chronyd --no-pager 2>&1 | head -60 > "$C/order.txt"
    journalctl -b -u upgrade_-settle-in --no-pager > "$C/settle-in.log" 2>&1
    date -u +%FT%TZ > "$C/done"
fi
# every boot: the firmware's entries and settle-in's latest records (the button's effect shows on the next boot)
if [ -e /mnt/upgstick/upgrade_/settle-in-capture/done ]; then
    t=$(date -u +%Y%m%dT%H%M%SZ); C=/mnt/upgstick/upgrade_/settle-in-capture
    efibootmgr > "$C/efibootmgr-$t.txt" 2>&1
    # the previous boot's desktop sign-ins (the first boot's capture runs before anyone signs in)
    journalctl -b -1 -o short-monotonic --no-pager 2>/dev/null | grep -E 'Starting Wayland user session|Started plasma-kwin_wayland|pam_unix\(gdm-password:session\): session opened|Started org.gnome.Shell@(wayland|user)|sddm-helper exited|settle-in-window: ' > "$C/sessions-before-$t.txt" 2>&1
    cp /var/lib/upgrade_/settle-in/report.json "$C/report-$t.json" 2>/dev/null
    cp /var/lib/upgrade_-settle-in/summary.json "$C/summary-$t.json" 2>/dev/null
fi
printf 'linux-boot,%s,%s,BootCurrent=%s,bootmgfw_sha256=%s,user=%s,pw_sha256=%s,home_dir=%s,home_disk=%s,root_disk=%s,default_target=%s,display_manager=%s\n' "$(date -u +%FT%TZ)" "$(uname -r)" "$cur" "$sha" "$u" "$pw" "$([ -d "/home/$u" ] && echo present || echo missing)" "${hdisk:-none}" "$rdisk" "$tgt" "$dm" >> /mnt/upgstick/upgrade_/boots.log
auto=0; [ -e /mnt/upgstick/upgrade_/autoshutdown ] && auto=1
sync; umount /mnt/upgstick
[ "$auto" = 1 ] || [ "$auto_cmd" = 1 ] && systemctl poweroff
exit 0
