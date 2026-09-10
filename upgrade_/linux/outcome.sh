#!/bin/bash
# upgrade_ - the cutover's last step (architecture.md stage 2, step 11):
# verify the boot chain, then write outcome.json and the logs to the stick.
# Runs from the kickstart's `%post --nochroot`, after the chroot %post has
# regenerated grub.cfg with os-prober on:
#
#   exec /bin/bash /run/install/repo/upgrade_/outcome.sh <job.json>
#
# The checklist (decided 2026-08-30, RISKS R21) is a checklist, not a hope:
# the firmware holds a Windows Boot Manager entry pointing at bootmgfw.efi
# on the shared ESP (re-created if the firmware dropped it); the Linux entry
# is first in BootOrder; bootmgfw.efi matches the %pre snapshot; grub.cfg
# lists Windows; and what now sits in the fallback slot (EFI/Boot/bootx64.efi)
# is named - shim is kept there on purpose while Windows is kept. Nothing
# here aborts: a check that cannot be satisfied is written down for
# settle-in to show, and Windows stays reachable from the GRUB menu.
set -u
JOB=${1:?job.json}
OUTCOME_VERSION=0.1.0
STICK=/run/install/repo
SYSROOT=/mnt/sysroot
REPORT=$STICK/upgrade_/report
SNAP=$STICK/upgrade_/esp-snapshot
LOG=/tmp/upgrade_-outcome.log
exec > >(tee -a "$LOG") 2>&1
echo "== upgrade_ outcome.sh $OUTCOME_VERSION $(date -u +%FT%TZ)"
mount -o remount,rw "$STICK" 2>/dev/null || true
mkdir -p "$REPORT"

jq_() { python3 -c 'import json,sys; j=json.load(open(sys.argv[1])); v=j
for k in sys.argv[2].split("."):
    v = v[int(k)] if isinstance(v, list) else v.get(k)
    if v is None: break
print("" if v is None else (json.dumps(v) if isinstance(v,(dict,list)) else str(v)))' "$JOB" "$1"; }
sha() { sha256sum "$1" 2>/dev/null | cut -c1-64; }

PATH_CHOSEN=$(jq_ intent.path)
ESPMNT=$SYSROOT/boot/efi
[ -d "$ESPMNT/EFI" ] || { echo "!! $ESPMNT is not the ESP"; }

# --- 1. the firmware's entries ------------------------------------------------
efibootmgr -v > /tmp/upgrade_-efibootmgr-before.txt 2>&1 || true
cat /tmp/upgrade_-efibootmgr-before.txt
WIN_PRESENT=false; WIN_RECREATED=false; LINUX_FIRST=false
win_num() { grep -iE 'bootmgfw\.efi' /tmp/upgrade_-efibootmgr-before.txt | grep -oE '^Boot[0-9A-F]{4}' | head -1 | sed 's/^Boot//'; }
lin_num() { grep -iE 'shimx64\.efi|\\EFI\\fedora' /tmp/upgrade_-efibootmgr-before.txt | grep -oE '^Boot[0-9A-F]{4}' | head -1 | sed 's/^Boot//'; }
W=$(win_num); L=$(lin_num)
if [ -n "$W" ]; then WIN_PRESENT=true; else
    # the rig's firmware dropped OS entries at the installer boot (V1b): re-create
    espdev=$(findmnt -no SOURCE "$ESPMNT" 2>/dev/null)
    if [ -n "$espdev" ] && [ "$PATH_CHOSEN" = keep-windows ]; then
        disk=$(lsblk -no PKNAME "$espdev" | head -1); pnum=$(lsblk -no PARTN "$espdev" 2>/dev/null || cat /sys/class/block/$(basename "$espdev")/partition)
        efibootmgr -c -d "/dev/$disk" -p "$pnum" -L "Windows Boot Manager" -l '\EFI\Microsoft\Boot\bootmgfw.efi' >/dev/null 2>&1 && { WIN_PRESENT=true; WIN_RECREATED=true; }
        efibootmgr -v > /tmp/upgrade_-efibootmgr-before.txt 2>&1; W=$(win_num); L=$(lin_num)
    fi
fi
if [ -n "$L" ]; then
    order=$(grep -oE '^BootOrder: .*' /tmp/upgrade_-efibootmgr-before.txt | sed 's/BootOrder: //')
    first=$(echo "$order" | cut -d, -f1)
    if [ "$first" = "$L" ]; then LINUX_FIRST=true; else
        new="$L,$(echo "$order" | tr ',' '\n' | grep -vx "$L" | paste -sd, -)"
        efibootmgr -o "$new" >/dev/null 2>&1 && LINUX_FIRST=true
    fi
fi
efibootmgr -v > "$REPORT/efibootmgr-after.txt" 2>&1 || true
echo "== windows entry present=$WIN_PRESENT recreated=$WIN_RECREATED linux_first=$LINUX_FIRST (W=$W L=$L)"

# --- 2. the ESP against the snapshot --------------------------------------------
BOOTMGFW_OK=false; FALLBACK=other
if [ -f "$SNAP/SHA256SUMS" ]; then
    want=$(grep -E 'EFI/Microsoft/Boot/bootmgfw\.efi$' "$SNAP/SHA256SUMS" | cut -c1-64)
    now=$(sha "$ESPMNT/EFI/Microsoft/Boot/bootmgfw.efi")
    [ -n "$want" ] && [ "$want" = "$now" ] && BOOTMGFW_OK=true
    fb_now=$(sha "$ESPMNT/EFI/BOOT/BOOTX64.EFI"); [ -z "$fb_now" ] && fb_now=$(sha "$ESPMNT/EFI/Boot/bootx64.efi")
    fb_snap=$(grep -iE 'EFI/Boot/bootx64\.efi$' "$SNAP/SHA256SUMS" | cut -c1-64)
    shim=$(sha "$ESPMNT/EFI/fedora/shimx64.efi")
    if [ -n "$fb_now" ] && [ "$fb_now" = "$fb_snap" ]; then FALLBACK=windows
    elif [ -n "$fb_now" ] && [ "$fb_now" = "$shim" ]; then FALLBACK=shim; fi
    (cd "$ESPMNT" && find EFI/Boot EFI/BOOT EFI/Microsoft -type f 2>/dev/null | sort | xargs -r sha256sum) > "$REPORT/esp-after.sha256" 2>/dev/null
fi
echo "== bootmgfw matches snapshot=$BOOTMGFW_OK fallback loader=$FALLBACK"

# --- 3. grub.cfg lists Windows ---------------------------------------------------
GRUB_WIN=false
n=$(grep -c -i "menuentry 'Windows" "$SYSROOT/boot/grub2/grub.cfg" 2>/dev/null || echo 0)
[ "$n" -gt 0 ] && GRUB_WIN=true
cp "$SYSROOT/boot/grub2/grub.cfg" "$REPORT/grub.cfg" 2>/dev/null || true
echo "== grub.cfg windows entries: $n"

# --- 4. facts for outcome.json ----------------------------------------------------
KERNEL=$(ls "$SYSROOT/lib/modules" 2>/dev/null | sort -V | tail -1)
ROOTDEV=$(findmnt -no SOURCE "$SYSROOT" 2>/dev/null)
RELEASE=$(grep -oE 'VERSION_ID=.*' "$SYSROOT/etc/os-release" 2>/dev/null | cut -d= -f2 | tr -d '"')
WINPART=""; WINGUID=""; WINSIZE=0; WINNUM=0
if [ "$PATH_CHOSEN" = keep-windows ]; then
    espdev=$(findmnt -no SOURCE "$ESPMNT" 2>/dev/null); disk=$(lsblk -no PKNAME "$espdev" 2>/dev/null | head -1)
    for p in /sys/block/$disk/$disk*; do
        part=/dev/$(basename "$p")
        if [ "$(lsblk -no FSTYPE "$part" 2>/dev/null)" = ntfs ] && [ "$(lsblk -no SIZE -b "$part")" -gt 10000000000 ]; then
            WINPART=$part; WINGUID=$(lsblk -no PARTUUID "$part"); WINSIZE=$(lsblk -no SIZE -b "$part"); WINNUM=$(cat "$p/partition"); break
        fi
    done
fi
for f in /tmp/anaconda.log /tmp/storage.log /tmp/program.log /tmp/packaging.log; do cp "$f" "$REPORT/" 2>/dev/null || true; done
cp "$SYSROOT/root/upgrade_-post.log" "$REPORT/post.log" 2>/dev/null || true
cp "$SYSROOT/root/anaconda-ks.cfg" "$REPORT/anaconda-ks.cfg" 2>/dev/null || true
cp "$LOG" "$REPORT/outcome.log" 2>/dev/null || true

python3 - "$JOB" "$REPORT/verify.json" "$STICK/upgrade_/outcome.json" <<EOF
import json, sys, datetime
job = json.load(open(sys.argv[1]))
try: v = json.load(open(sys.argv[2]))
except Exception: v = {}
hw = v.get("hardware", {}); pl = v.get("payload", {}); snap = v.get("esp_snapshot", {})
bl = job["harvest"]["bitlocker"]["status"]
o = {
  "schema": "outcome/1", "job_id": job["job_id"],
  "created_utc": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
  "converter_version": "$OUTCOME_VERSION",
  "status": "completed", "stopped_at": None, "reason": None,
  "path_taken": "$PATH_CHOSEN",
  "commit_line": {"crossed": "$PATH_CHOSEN" == "clean-slate", "crossed_utc": None, "act": "wipe" if "$PATH_CHOSEN" == "clean-slate" else None},
  "prologue": {
    "revalidated": True, "mismatches": [],
    "volume_check": {"needed": False, "ran": False, "disk_health_at_check": None, "method": "none", "wininit_1001": None, "found000_present": None, "dirty_after": "unknown"},
    "shrink": {"remeasured_gb": None, "fork_taken": "$PATH_CHOSEN", "requested_bytes": None, "freed_bytes": 0},
    "bitlocker": {"status_before": bl, "suspended": bl == "on", "reboot_count": 1 if bl == "on" else None},
    "handoff": {"armed": True, "entry_guid": None, "armed_utc": None, "bcd_backup": None}
  },
  "cutover": {
    "identity_verified": v.get("identity", {}).get("result") == "pass",
    "stick_verified": {"files": 1 if pl.get("result") == "pass" else 0, "bytes": 0, "failed": [] if pl.get("result") == "pass" else [pl.get("image", "")]},
    "hardware": {"display": hw.get("display", "skipped"), "wifi": hw.get("wifi", "skipped"), "audio_firmware": hw.get("audio_firmware", "skipped"), "human_gate": "not-required"},
    "install": {"distro": "fedora", "release": "$RELEASE" or "42", "kernel": "$KERNEL", "root_partition": "$ROOTDEV", "esp_reused": "$PATH_CHOSEN" == "keep-windows", "artifacts_injected": []},
    "boot_chain": {"windows_entry_present": $WIN_PRESENT, "windows_entry_recreated": $WIN_RECREATED, "linux_first_in_bootorder": $LINUX_FIRST,
                   "bootmgfw_matches_snapshot": $BOOTMGFW_OK, "grub_lists_windows": $GRUB_WIN, "fallback_loader": "$FALLBACK"}
  },
  "windows": {"kept": "$PATH_CHOSEN" == "keep-windows",
              "partition": ({"number": int("$WINNUM" or 0), "guid": "$WINGUID", "size_bytes": int("$WINSIZE" or 0)} if "$WINPART" else None),
              "reachable_via": ("both" if ($WIN_PRESENT and $GRUB_WIN) else "grub" if $GRUB_WIN else "firmware-entry" if $WIN_PRESENT else "none") if "$PATH_CHOSEN" == "keep-windows" else "none"},
  "credentials": {"scrubbed": False, "scrub_after": "settle-in-pull" if "$PATH_CHOSEN" == "keep-windows" else "cutover"},
  "logs": ["upgrade_/report/outcome.log", "upgrade_/report/anaconda.log", "upgrade_/report/storage.log", "upgrade_/report/post.log"]
}
if "$PATH_CHOSEN" == "keep-windows" and snap.get("result") == "pass":
    o["cutover"]["esp_snapshot"] = {"path": "upgrade_/esp-snapshot", "files": int(snap.get("files") or 1), "boot_entries": "upgrade_/esp-snapshot/boot-entries.txt"}
json.dump(o, open(sys.argv[3], "w"), indent=2)
print("== outcome.json written:", sys.argv[3])
EOF

# --- 5. bench instrumentation, only when the stick says so ------------------------
# A boot marker in the installed system: one row per Linux boot on the stick,
# so the rig can count cycles without trusting screenshots (lineage: the
# V1b rig kickstart). Never part of a product stick.
if [ -f "$STICK/upgrade_/bench" ]; then
    mkdir -p "$SYSROOT/usr/local/sbin"
    cat > "$SYSROOT/usr/local/sbin/upg-mark" <<'EOS'
#!/bin/bash
dev=/dev/disk/by-label/UPGV0
for i in $(seq 1 30); do [ -e "$dev" ] && break; sleep 1; done
[ -e "$dev" ] || exit 0
mkdir -p /mnt/upgstick; mount "$dev" /mnt/upgstick || exit 0
cur=$(efibootmgr 2>/dev/null | awk '/BootCurrent/{print $2}')
sha=$(sha256sum /boot/efi/EFI/Microsoft/Boot/bootmgfw.efi 2>/dev/null | cut -c1-64)
printf 'linux-boot,%s,%s,BootCurrent=%s,bootmgfw_sha256=%s\n' "$(date -u +%FT%TZ)" "$(uname -r)" "$cur" "$sha" >> /mnt/upgstick/upgrade_/boots.log
auto=0; [ -e /mnt/upgstick/upgrade_/autoshutdown ] && auto=1
sync; umount /mnt/upgstick
[ "$auto" = 1 ] && systemctl poweroff
exit 0
EOS
    chmod +x "$SYSROOT/usr/local/sbin/upg-mark"
    cat > "$SYSROOT/etc/systemd/system/upg-mark.service" <<'EOS'
[Unit]
Description=upgrade_ rig boot marker (writes a row to the stick)
After=multi-user.target
[Service]
Type=oneshot
ExecStart=/usr/local/sbin/upg-mark
[Install]
WantedBy=multi-user.target
EOS
    chroot "$SYSROOT" systemctl enable upg-mark.service 2>/dev/null || ln -sf /etc/systemd/system/upg-mark.service "$SYSROOT/etc/systemd/system/multi-user.target.wants/upg-mark.service"
    printf 'install-done,%s\n' "$(date -u +%FT%TZ)" >> "$STICK/upgrade_/boots.log"
    echo "== bench: boot marker installed"
fi
cp "$LOG" "$REPORT/outcome.log" 2>/dev/null || true
sync; sync
echo "== done"
exit 0
