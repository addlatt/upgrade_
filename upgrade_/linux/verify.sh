#!/bin/bash
# upgrade_ - the %pre verifier. Runs inside Anaconda's stage2 before anything
# is decided, from the kickstart New-Kickstart.ps1 generates:
#
#   exec /bin/bash /run/install/repo/upgrade_/verify.sh <job.json> <stick label>
#
# Three jobs, in this order, and it is the cutover's refusal gate
# (architecture.md, stage 2 steps 5 and 7):
#   1. IDENTITY - the disk the job names (serial / unique id / size) must be
#      attached to THIS machine, and its size must match exactly. A moved
#      stick must never partition a stranger's laptop: on a mismatch this
#      script exits non-zero, %pre is --erroronfail, and the installer stops.
#   2. HARDWARE - display connected and driving a mode, a Wi-Fi device
#      present and able to scan, amp firmware artifacts present when the job
#      lists any. Each is pass / fail / skipped; nothing here is a guess.
#   3. STORAGE - writes /tmp/upgrade_-storage.ks (the %include) with the
#      resolved Linux device names for the path the job chose.
#   4. COUNTDOWN (erase-and-install jobs only, RISKS R27, 0.4.0) - after
#      every check above has passed, two minutes on screen: "press any key
#      to cancel". A key: the report goes to the stick and the machine
#      restarts into Windows, untouched. No key: the moment it reaches zero
#      is the commit line - its time goes to the stick and the installer
#      erases the listed drives.
#
# Everything it learns goes to the stick, under upgrade_/report/, as
# verify.json plus the raw logs, and it syncs before it returns. With
# upg.mode=verify on the kernel command line (the reversible half of the
# vertical) it reboots after writing the report and the install never
# starts - the machine comes back to Windows with the report on the stick.
set -u
JOB=${1:?job.json path}
LABEL=${2:-UPGV0}
VERIFY_VERSION=0.5.0
STICK=/run/install/repo
REPORT=$STICK/upgrade_/report
STORAGE_KS=/tmp/upgrade_-storage.ks
LOG=/tmp/upgrade_-verify.log
MODE=install
grep -qw 'upg.mode=verify' /proc/cmdline && MODE=verify

exec > >(tee -a "$LOG") 2>&1
echo "== upgrade_ verify.sh $VERIFY_VERSION  mode=$MODE  $(date -u +%FT%TZ)"
echo "== cmdline: $(cat /proc/cmdline)"

jq_() { python3 -c 'import json,sys; j=json.load(open(sys.argv[1])); v=j
for k in sys.argv[2].split("."):
    v = v[int(k)] if isinstance(v, list) else v.get(k)
    if v is None: break
print("" if v is None else (json.dumps(v) if isinstance(v,(dict,list)) else str(v)))' "$JOB" "$1"; }

# --- a refusal the person can read (0.5.0, 2026-09-27; the owner approved the words) ----
# Before, a refusal was only an exit code, and Anaconda put its raw Python
# traceback on the screen (rig arm A, the Aspire's run 9). Now every refusal
# shows one plain screen on tty6 for 60 s (any key: now), writes
# report/refusal.json, and restarts the computer unchanged - nothing on the
# internal drives has been written at any refusal point. The exit code
# stays as the backstop if the restart itself fails (%pre is --erroronfail).
refuse() {
    local code=$1 plain=$2 detail=$3
    echo "!! $detail"
    echo "== REFUSED (exit $code): $plain"
    mount -o remount,rw "$STICK" 2>/dev/null; mkdir -p "$REPORT" 2>/dev/null
    python3 -c 'import json,sys,datetime; json.dump({"schema":"refusal/1","exit_code":int(sys.argv[2]),"reason":sys.argv[3],"detail":sys.argv[4],"created_utc":datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")}, open(sys.argv[1],"w"), indent=2)' \
        "$REPORT/refusal.json" "$code" "$plain" "$detail" 2>/dev/null
    cp "$LOG" "$REPORT/verify.log" 2>/dev/null; sync; sync
    if [ -c /dev/tty6 ] && chvt 6 2>/dev/null; then
        python3 - /dev/tty6 "$plain" <<'PYEOF2'
import os, select, sys, termios, textwrap, time
path, reason = sys.argv[1], sys.argv[2]
fd = os.open(path, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
old = termios.tcgetattr(fd); raw = termios.tcgetattr(fd)
raw[3] &= ~(termios.ICANON | termios.ECHO | termios.ISIG); raw[6][termios.VMIN] = 0; raw[6][termios.VTIME] = 0
termios.tcsetattr(fd, termios.TCSANOW, raw); termios.tcflush(fd, termios.TCIFLUSH)
def put(t):
    d = t.encode()
    while d:
        try: n = os.write(fd, d); d = d[n:]
        except BlockingIOError: time.sleep(0.01)
def screen(left):
    body = ["NOTHING WAS CHANGED ON THIS COMPUTER.", "",
            "The installer stopped before touching anything, because %s." % reason, "",
            "It restarts in %d seconds, exactly as it was before." % left,
            "Press any key to restart now.", "",
            "The details are saved on the USB stick, in upgrade_\\report."]
    lines = []
    for b in body: lines += (textwrap.wrap(b, 60) or [""])
    put("\033[2J\033[H\n\n" + "".join("   %s\n" % l for l in lines))
end = time.monotonic() + 60
try:
    while True:
        left = end - time.monotonic()
        if left <= 0: break
        screen(int(left) + (1 if left % 1 else 0))
        r, _, _ = select.select([fd], [], [], min(1.0, left))
        if r:
            try:
                if os.read(fd, 64): break
            except BlockingIOError: pass
finally:
    termios.tcsetattr(fd, termios.TCSANOW, old)
PYEOF2
    else
        echo "!! the refusal screen could not be shown on tty6; restarting in 60 s"
        sleep 60
    fi
    systemctl reboot 2>/dev/null || reboot -f 2>/dev/null || { echo b > /proc/sysrq-trigger; }
    sleep 60
    exit "$code"
}

# --- 0. the job ---------------------------------------------------------------
SCHEMA=$(jq_ schema)
if [ "$SCHEMA" != "job/1" ]; then refuse 10 "this USB stick was prepared by a different version of this tool" "job schema '$SCHEMA' is not job/1"; fi
JOB_ID=$(jq_ job_id); PATH_CHOSEN=$(jq_ intent.path)
J_SERIAL=$(jq_ identity.system_disk.serial_number)
J_UID=$(jq_ identity.system_disk.unique_id)
J_SIZE=$(jq_ identity.system_disk.size_bytes)
echo "== job $JOB_ID path=$PATH_CHOSEN disk serial='$J_SERIAL' unique_id='$J_UID' size=$J_SIZE"

# --- 1. identity: find the disk by id, then check its size exactly ------------
# Windows unique ids: NVMe 'eui.<hex>', SCSI/SAS a 32-hex WWN, USBSTOR paths.
# Linux exposes the same hex in /dev/disk/by-id names (nvme-eui.<hex>,
# wwn-0x<hex>, scsi-3<hex>) - matched as a case-insensitive hex substring.
# The serial, when Windows had one, is matched the same way as a second vote.
# Only an id that IS hex (eui.<hex>, a WWN, a bare hex string) is matched by
# its hex; a padded ATA text id ("ATA     HFS256G39TND-N210A   <serial>", as
# the Aspire's Windows reports it, 2026-09-12) would otherwise be stripped to
# meaningless hex fragments - the serial vote handles those disks.
norm() { echo "$1" | tr 'A-Z' 'a-z' | sed 's/^eui\.//; s/^0x//' | tr -d ' _.-'; }
# resolve_disk <unique_id> <serial>: the whole-disk device carrying that id
# (sets R_DISK and R_BY; both empty when nothing matches)
resolve_disk() {
    local uid_hex serial_norm link name tgt
    R_DISK=""; R_BY=""
    uid_hex=$(norm "$1"); echo "$uid_hex" | grep -qE '^[0-9a-f]{8,}$' || uid_hex=""
    serial_norm=$(echo "$2" | tr 'A-Z' 'a-z' | tr -d ' _.-')
    for link in /dev/disk/by-id/*; do
        [ -e "$link" ] || continue
        name=$(basename "$link" | tr 'A-Z' 'a-z')
        case "$name" in *-part[0-9]*) continue;; esac
        tgt=$(readlink -f "$link"); [ -b "$tgt" ] || continue
        case "$tgt" in *[0-9]p[0-9]*|/dev/sd[a-z]*[0-9]) continue;; esac
        if [ -n "$uid_hex" ] && [ ${#uid_hex} -ge 8 ] && echo "$name" | tr -d '_.-' | grep -q "$uid_hex"; then R_DISK=$tgt; R_BY="unique_id via $(basename "$link")"; return; fi
    done
    if [ -n "$serial_norm" ] && [ ${#serial_norm} -ge 6 ]; then
        for link in /dev/disk/by-id/*; do
            name=$(basename "$link" | tr 'A-Z' 'a-z' | tr -d '_.-')
            case "$name" in *part[0-9]*) continue;; esac
            if echo "$name" | grep -q "$serial_norm"; then R_DISK=$(readlink -f "$link"); R_BY="serial via $(basename "$link")"; return; fi
        done
    fi
}
resolve_disk "$J_UID" "$J_SERIAL"; DISK=$R_DISK; MATCHED_BY=$R_BY
IDENTITY=fail; DISK_SIZE=""
if [ -n "$DISK" ]; then
    DISK_SIZE=$(blockdev --getsize64 "$DISK" 2>/dev/null || echo "")
    echo "== candidate $DISK ($MATCHED_BY) size=$DISK_SIZE"
    if [ "$DISK_SIZE" = "$J_SIZE" ]; then IDENTITY=pass; else echo "!! size mismatch: job says $J_SIZE, disk is $DISK_SIZE"; fi
else
    echo "!! no attached disk carries the job's unique id or serial"
fi
# erase-and-install (R27): every drive the job names must be here, by id and
# exact size; disks[0] is the system drive above, disks[1] (if any) is home
ERASE=false; [ -n "$(jq_ erase_consent.statement)" ] && ERASE=true
HOME_DISK=""; HOME_BY=""; HOME_SIZE=""; NDISKS=0
if [ "$ERASE" = true ]; then
    NDISKS=$(python3 -c 'import json,sys; print(len(json.load(open(sys.argv[1]))["erase_consent"]["disks"]))' "$JOB" 2>/dev/null || echo 0)
    if [ "$(jq_ erase_consent.disks.0.role)" != system ] || [ "$(jq_ erase_consent.disks.0.unique_id)" != "$J_UID" ] || [ "$(jq_ erase_consent.disks.0.size_bytes)" != "$J_SIZE" ]; then
        echo "!! erase_consent.disks[0] is not the system disk the job identifies"; IDENTITY=fail
    fi
    if [ "$NDISKS" -ge 2 ]; then
        H_UID=$(jq_ erase_consent.disks.1.unique_id); H_SERIAL=$(jq_ erase_consent.disks.1.serial_number); H_SIZE_J=$(jq_ erase_consent.disks.1.size_bytes)
        resolve_disk "$H_UID" "$H_SERIAL"; HOME_DISK=$R_DISK; HOME_BY=$R_BY
        if [ -z "$HOME_DISK" ]; then echo "!! the home drive the job names (serial '$H_SERIAL') is not attached"; IDENTITY=fail
        else
            HOME_SIZE=$(blockdev --getsize64 "$HOME_DISK" 2>/dev/null || echo "")
            echo "== home candidate $HOME_DISK ($HOME_BY) size=$HOME_SIZE"
            [ "$HOME_SIZE" = "$H_SIZE_J" ] || { echo "!! home size mismatch: job says $H_SIZE_J, disk is $HOME_SIZE"; IDENTITY=fail; }
            [ "$HOME_DISK" != "$DISK" ] || { echo "!! the home drive resolved to the system drive"; IDENTITY=fail; }
        fi
    fi
    echo "== erase-and-install: $NDISKS drive(s); identity=$IDENTITY"
fi
lsblk -b -o NAME,SIZE,TYPE,FSTYPE,LABEL,PARTTYPENAME,SERIAL,WWN 2>/dev/null | tee /tmp/upgrade_-lsblk.txt
ls -l /dev/disk/by-id/ > /tmp/upgrade_-by-id.txt 2>&1

# --- 2. hardware ----------------------------------------------------------------
DISPLAY_RESULT=skipped; DISPLAY_DETAIL=""
for st in /sys/class/drm/card*-*/status; do
    [ -e "$st" ] || continue
    if [ "$(cat "$st")" = connected ]; then
        con=$(basename "$(dirname "$st")"); mode=$(head -1 "$(dirname "$st")/modes" 2>/dev/null)
        DISPLAY_DETAIL="$DISPLAY_DETAIL $con:${mode:-nomode}"
        [ -n "$mode" ] && DISPLAY_RESULT=pass
    fi
done
if [ "$DISPLAY_RESULT" = skipped ] && ls /sys/class/drm/card*-* >/dev/null 2>&1; then DISPLAY_RESULT=fail; DISPLAY_DETAIL="connectors present, none connected with a mode"; fi
[ -z "$DISPLAY_DETAIL" ] && DISPLAY_DETAIL="$(ls /sys/class/graphics/ 2>/dev/null | tr '\n' ' ')"
cat /sys/class/graphics/fb0/virtual_size 2>/dev/null > /tmp/upgrade_-fb0.txt

WIFI_RESULT=skipped; WIFI_DETAIL="no wifi device"
if command -v nmcli >/dev/null 2>&1; then
    wdev=$(nmcli -t -f DEVICE,TYPE device 2>/dev/null | awk -F: '$2=="wifi"{print $1; exit}')
    if [ -n "$wdev" ]; then
        nmcli device wifi rescan ifname "$wdev" >/dev/null 2>&1; sleep 4
        n=$(nmcli -t -f SSID device wifi list ifname "$wdev" 2>/dev/null | grep -c .)
        WIFI_DETAIL="$wdev sees $n networks"
        if [ "$n" -gt 0 ]; then WIFI_RESULT=pass; else WIFI_RESULT=fail; fi
    fi
fi
nmcli device 2>/dev/null > /tmp/upgrade_-nmcli.txt

AUDIO_RESULT=skipped; AUDIO_DETAIL="job lists no firmware artifacts"
nart=$(python3 -c 'import json,sys; print(len(json.load(open(sys.argv[1]))["harvest"]["firmware_artifacts"]))' "$JOB" 2>/dev/null || echo 0)
if [ "$nart" -gt 0 ]; then
    AUDIO_RESULT=pass; AUDIO_DETAIL=""
    for i in $(seq 0 $((nart-1))); do
        p=$(jq_ "harvest.firmware_artifacts.$i.path"); [ -f "$STICK/upgrade_/$p" ] || { AUDIO_RESULT=fail; AUDIO_DETAIL="$AUDIO_DETAIL missing:$p"; }
    done
fi
lspci -nn > /tmp/upgrade_-lspci.txt 2>&1 || true
dmesg > /tmp/upgrade_-dmesg.txt 2>&1 || true

# --- 2b. the desktop image: is what the kickstart will install actually on the
# stick, byte for byte? Read back against the stick's own SHA256SUMS - the
# cutover's step 6 (architecture.md), and the counterfeit-flash test (RISKS
# R17): a stick that lied about its capacity fails here, while Windows still
# exists. The read speed is recorded too - it is the honest basis for the
# time estimates the person is shown.
DESKTOP=$(jq_ intent.desktop)
IMG_REL="upgrade_/LiveOS/$DESKTOP.squashfs"
IMAGE_RESULT=fail; IMAGE_DETAIL=""; IMAGE_MBPS=""
if [ ! -f "$STICK/$IMG_REL" ]; then
    IMAGE_DETAIL="missing: $IMG_REL"
elif [ ! -f "$STICK/SHA256SUMS" ]; then
    IMAGE_DETAIL="no SHA256SUMS on the stick"
else
    want=$(grep -E "[[:space:]]\./$IMG_REL\$|[[:space:]]$IMG_REL\$" "$STICK/SHA256SUMS" | head -1 | cut -c1-64)
    if [ -z "$want" ]; then
        IMAGE_DETAIL="$IMG_REL not in SHA256SUMS"
    else
        # no `stat` in Anaconda's stage2 (coreutils-single; seen 2026-09-09) - python has the size
        bytes=$(python3 -c 'import os,sys; print(os.path.getsize(sys.argv[1]))' "$STICK/$IMG_REL"); t0=$(date +%s.%N)
        got=$(sha256sum "$STICK/$IMG_REL" | cut -c1-64)
        t1=$(date +%s.%N)
        IMAGE_MBPS=$(python3 -c 'import sys; b,t0,t1=float(sys.argv[1]),float(sys.argv[2]),float(sys.argv[3]); print(round(b/1e6/max(t1-t0,0.001),1))' "$bytes" "$t0" "$t1")
        if [ "$got" = "$want" ]; then IMAGE_RESULT=pass; IMAGE_DETAIL="$IMG_REL $bytes bytes sha256 ok, read at $IMAGE_MBPS MB/s"
        else IMAGE_DETAIL="$IMG_REL sha256 MISMATCH (want $want got $got)"; fi
    fi
fi
echo "== desktop image: $IMAGE_RESULT - $IMAGE_DETAIL"

# --- 2c. the ESP snapshot (architecture.md step 8; RISKS R21). Before anything
# touches the shared ESP: every file under EFI/Boot and EFI/Microsoft, with
# sha256, and the firmware's Boot#### entries, copied to the stick. What
# rollback restores Windows' fallback loader from, and what the boot-chain
# checklist compares against after the install. Runs in verify mode too (a
# rehearsal - it only reads the ESP and writes the stick).
SNAP_RESULT=skipped; SNAP_FILES=0; SNAP_DIR="$REPORT/../esp-snapshot"
if [ "$IDENTITY" = pass ] && [ "$PATH_CHOSEN" = keep-windows ]; then
    mount -o remount,rw "$STICK" 2>/dev/null || true
    rm -rf "$SNAP_DIR"; mkdir -p "$SNAP_DIR"
    dev=$(basename "$DISK"); SNAP_RESULT=fail
    for p in /sys/block/$dev/$dev*; do
        [ -d "$p" ] || continue
        part=/dev/$(basename "$p")
        [ "$(lsblk -no PARTTYPE "$part" 2>/dev/null | tr 'A-Z' 'a-z')" = "c12a7328-f81f-11d2-ba4b-00a0c93ec93b" ] || continue
        mkdir -p /tmp/upg-esp
        if mount -o ro "$part" /tmp/upg-esp 2>/dev/null; then
            if [ -f /tmp/upg-esp/EFI/Microsoft/Boot/bootmgfw.efi ]; then
                for d in EFI/Boot EFI/BOOT EFI/Microsoft; do
                    [ -d "/tmp/upg-esp/$d" ] && { mkdir -p "$SNAP_DIR/$(dirname "$d")"; cp -a "/tmp/upg-esp/$d" "$SNAP_DIR/$d"; }
                done
                (cd "$SNAP_DIR" && find . -type f ! -name SHA256SUMS ! -name boot-entries.txt | sort | xargs -r sha256sum > SHA256SUMS)
                SNAP_FILES=$(grep -c . "$SNAP_DIR/SHA256SUMS" || echo 0)
                efibootmgr -v > "$SNAP_DIR/boot-entries.txt" 2>&1 || true
                printf 'disk=%s esp=%s taken_utc=%s\n' "$DISK" "$part" "$(date -u +%FT%TZ)" > "$SNAP_DIR/SOURCE"
                SNAP_RESULT=pass
            fi
            umount /tmp/upg-esp
        fi
        [ "$SNAP_RESULT" = pass ] && break
    done
    sync
fi
echo "== esp snapshot: $SNAP_RESULT ($SNAP_FILES files) -> $SNAP_DIR"

# --- 3. storage %include, from the resolved disk --------------------------------
ESP=""; ESP_RESULT=skipped
if [ "$IDENTITY" = pass ]; then
    dev=$(basename "$DISK")
    for p in /sys/block/$dev/$dev*; do
        [ -d "$p" ] || continue
        part=/dev/$(basename "$p")
        if [ "$(lsblk -no PARTTYPE "$part" 2>/dev/null | tr 'A-Z' 'a-z')" = "c12a7328-f81f-11d2-ba4b-00a0c93ec93b" ]; then
            mkdir -p /tmp/upg-esp
            if mount -o ro "$part" /tmp/upg-esp 2>/dev/null; then
                [ -f /tmp/upg-esp/EFI/Microsoft/Boot/bootmgfw.efi ] && ESP=$part
                umount /tmp/upg-esp
            fi
            [ -n "$ESP" ] && break
        fi
    done
    if [ "$PATH_CHOSEN" = keep-windows ]; then
        if [ -n "$ESP" ]; then
            ESP_RESULT=pass
            # the rig's bench drives GRUB by keystroke and needs a longer menu
            # timeout; it says so with a marker file beside the kickstart
            BL_EXTRA=""; [ -f "$STICK/upgrade_/bench" ] && BL_EXTRA=" --timeout=20"
            cat > "$STORAGE_KS" <<EOF
# written by verify.sh: keep-windows on $DISK ($MATCHED_BY), ESP $ESP reused unformatted
ignoredisk --only-use=$dev
bootloader --location=mbr --boot-drive=$dev$BL_EXTRA
part /boot/efi --onpart=$(basename "$ESP") --noformat
part /boot --fstype=ext4 --size=1024 --ondisk=$dev
part /     --fstype=ext4 --size=4096 --grow --ondisk=$dev
EOF
        else
            ESP_RESULT=fail; echo "!! keep-windows: no EFI partition holding bootmgfw.efi on $DISK"
        fi
    elif [ "$ERASE" = true ]; then
        ESP_RESULT=skipped
        hdev=""; [ -n "$HOME_DISK" ] && hdev=$(basename "$HOME_DISK")
        drives=$dev; [ -n "$hdev" ] && drives="$dev,$hdev"
        {
            echo "# written by verify.sh: erase-and-install on $drives - the countdown before this is the commit line (R27)"
            echo "zerombr"
            echo "ignoredisk --only-use=$drives"
            echo "clearpart --all --initlabel --drives=$drives"
            echo "bootloader --location=mbr --boot-drive=$dev"
            echo "part /boot/efi --fstype=efi --size=600 --ondisk=$dev"
            echo "part /boot     --fstype=ext4 --size=1024 --ondisk=$dev"
            echo "part /         --fstype=ext4 --size=4096 --grow --ondisk=$dev"
            [ -n "$hdev" ] && echo "part /home     --fstype=ext4 --size=1024 --grow --ondisk=$hdev"
        } > "$STORAGE_KS"
    else
        ESP_RESULT=$([ -n "$ESP" ] && echo pass || echo skipped)
        cat > "$STORAGE_KS" <<EOF
# written by verify.sh: clean-slate on $DISK ($MATCHED_BY) - the wipe is the commit line
ignoredisk --only-use=$dev
clearpart --all --initlabel --drives=$dev
bootloader --location=mbr --boot-drive=$dev
part /boot/efi --fstype=efi --size=600 --ondisk=$dev
part /boot     --fstype=ext4 --size=1024 --ondisk=$dev
part /         --fstype=ext4 --grow --ondisk=$dev
EOF
    fi
    echo "== storage include:"; cat "$STORAGE_KS"
fi

# --- the report, on the stick ----------------------------------------------------
mount -o remount,rw "$STICK" 2>/dev/null || echo "!! could not remount the stick read-write"
mkdir -p "$REPORT"
python3 - "$REPORT/verify.json" <<EOF
import json, sys, datetime
r = {
  "schema": "verify/1", "verify_version": "$VERIFY_VERSION", "job_id": "$JOB_ID", "mode": "$MODE",
  "created_utc": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
  "kernel": "$(uname -r)", "cmdline": open("/proc/cmdline").read().strip(),
  "identity": {"result": "$IDENTITY", "disk": "$DISK", "matched_by": "$MATCHED_BY", "disk_size_bytes": "$DISK_SIZE", "job_size_bytes": "$J_SIZE"},
  "hardware": {"display": "$DISPLAY_RESULT", "display_detail": "$DISPLAY_DETAIL".strip(),
               "wifi": "$WIFI_RESULT", "wifi_detail": "$WIFI_DETAIL",
               "audio_firmware": "$AUDIO_RESULT", "audio_detail": "$AUDIO_DETAIL".strip()},
  "storage": {"path": "$PATH_CHOSEN", "esp": "$ESP", "esp_result": "$ESP_RESULT", "include_written": $([ -f "$STORAGE_KS" ] && echo True || echo False),
              "erase": $([ "$ERASE" = true ] && echo True || echo False), "home_disk": "$HOME_DISK", "home_matched_by": "$HOME_BY", "home_size_bytes": "$HOME_SIZE"},
  "esp_snapshot": {"result": "$SNAP_RESULT", "files": $SNAP_FILES, "path": "upgrade_/esp-snapshot"},
  "payload": {"desktop": "$DESKTOP", "image": "$IMG_REL", "result": "$IMAGE_RESULT", "detail": "$IMAGE_DETAIL", "read_mbps": "$IMAGE_MBPS"},
  "secure_boot": "$(od -An -t u1 /sys/firmware/efi/efivars/SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c 2>/dev/null | awk '{print $NF}')"
}
json.dump(r, open(sys.argv[1], "w"), indent=2)
EOF
cp "$STORAGE_KS" "$REPORT/storage.ks" 2>/dev/null || true
for f in verify lsblk by-id fb0 nmcli lspci dmesg; do cp "/tmp/upgrade_-$f.txt" "$REPORT/$f.txt" 2>/dev/null || true; done
cp "$LOG" "$REPORT/verify.log" 2>/dev/null || true
sync; sync
echo "== report written to $REPORT; identity=$IDENTITY display=$DISPLAY_RESULT wifi=$WIFI_RESULT audio=$AUDIO_RESULT esp=$ESP_RESULT image=$IMAGE_RESULT"

if [ "$MODE" = verify ]; then
    echo "== verify mode: rebooting to Windows (nothing installed, nothing changed on the internal disk)"
    sleep 2; sync
    systemctl reboot 2>/dev/null || reboot -f 2>/dev/null || { echo b > /proc/sysrq-trigger; }
    sleep 60
    exit 0
fi
# install mode: the refusal is the exit code - %pre is --erroronfail
[ "$IDENTITY" = pass ] || refuse 20 "this USB stick was prepared for a different computer, or one of its drives has changed since" "IDENTITY MISMATCH - refusing to install on this machine"
[ -f "$STORAGE_KS" ] || refuse 21 "the installer could not work out where to put Linux" "no storage include written"
[ "$ESP_RESULT" != fail ] || refuse 22 "Windows' startup files were not where the preparation found them" "keep-windows needs the Windows ESP"
[ "$IMAGE_RESULT" = pass ] || refuse 23 "the copy of Linux on this USB stick is damaged" "the desktop image on the stick did not verify (RISKS R17)"
[ "$PATH_CHOSEN" != keep-windows ] || [ "$SNAP_RESULT" = pass ] || refuse 24 "a safety copy of Windows' startup files could not be made" "the ESP snapshot failed - the ESP is not touched without it (RISKS R21)"
[ "$PATH_CHOSEN" = keep-windows ] || [ "$ERASE" = true ] || refuse 25 "this version cannot yet put your files back on an erased computer" "a clean slate with staged files needs the restore, which this version does not have"

# --- 4. the countdown: the last exit before the erase (RISKS R27, rule #3) ------
# Anaconda runs %pre before its own screens; tty6 is free in text mode. The
# countdown is written there and a key is read from there. If it cannot be
# shown, nothing is erased: an erase with no visible last exit is a refusal.
if [ "$ERASE" = true ]; then
    COUNT_SECS=120; CTTY=/dev/tty6
    what="THIS COMPUTER'S DRIVE"; [ -n "$HOME_DISK" ] && what="BOTH DRIVES"
    names="$(lsblk -dno MODEL "$DISK" 2>/dev/null | xargs)"; [ -n "$HOME_DISK" ] && names="$names and $(lsblk -dno MODEL "$HOME_DISK" 2>/dev/null | xargs)"
    countdown_record() {
        python3 - "$REPORT/countdown.json" "$1" "$COUNT_SECS" <<'PYEOF'
import json, sys, datetime
json.dump({"schema": "countdown/1", "result": sys.argv[2], "seconds": int(sys.argv[3]),
           "ended_utc": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")}, open(sys.argv[1], "w"), indent=2)
PYEOF
        cp "$LOG" "$REPORT/verify.log" 2>/dev/null || true; sync; sync
    }
    if [ ! -c "$CTTY" ] || ! chvt 6 2>/dev/null; then
        refuse 26 "the last-chance countdown could not be shown, and nothing is erased without it" "the countdown could not be shown on $CTTY (chvt: $(command -v chvt || echo missing)) - no erase without a visible last exit"
    fi
    echo "== countdown: $COUNT_SECS s on $CTTY, erasing $what ($names)"
    # Python, not bash `read -t -n`: on the rig (2026-09-26, V9 arm B) a key
    # pressed during the countdown left bash 5.2.37 blocked in read(2) on tty6
    # with its timeout dead - the countdown froze at 0:02 and neither cancelled
    # nor erased. Here the terminal goes raw ONCE for the whole countdown (no
    # canonical window between reads for a key to fall into), input waits in
    # select() on a non-blocking descriptor against a monotonic deadline, and
    # a console the kernel reports narrower than 40 columns (12, same run) is
    # set to 80x25. Exit 0 = elapsed, 1 = a key cancelled it, anything else =
    # it could not be shown, which is a refusal.
    python3 - "$CTTY" "$COUNT_SECS" "$what" "$names" <<'PYEOF'
import fcntl, os, select, struct, sys, termios, textwrap, time
path, secs, what, names = sys.argv[1], int(sys.argv[2]), sys.argv[3], sys.argv[4]
fd = os.open(path, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
old = termios.tcgetattr(fd)
raw = termios.tcgetattr(fd)
raw[3] &= ~(termios.ICANON | termios.ECHO | termios.ISIG)   # any key, Ctrl-C included, is a key
raw[6][termios.VMIN] = 0; raw[6][termios.VTIME] = 0
termios.tcsetattr(fd, termios.TCSANOW, raw)
termios.tcflush(fd, termios.TCIFLUSH)                          # keys pressed before the countdown do not count
rows, cols = struct.unpack("HHHH", fcntl.ioctl(fd, termios.TIOCGWINSZ, bytes(8)))[:2]
print("== countdown console %dx%d" % (cols, rows), file=sys.stderr)
if cols < 40:
    try:
        fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 25, 80, 0, 0))
        rows, cols = struct.unpack("HHHH", fcntl.ioctl(fd, termios.TIOCGWINSZ, bytes(8)))[:2]
        print("== countdown console set to %dx%d" % (cols, rows), file=sys.stderr)
    except OSError as ex:
        print("== countdown console could not be resized: %s" % ex, file=sys.stderr)
width = max(20, min(cols, 100) - 6)
def put(text):
    data = text.encode()
    while data:
        try: n = os.write(fd, data); data = data[n:]
        except BlockingIOError: time.sleep(0.01)
def screen(left):
    body = ["ERASING %s IN %d:%02d" % (what, left // 60, left % 60), "",
            "Windows and everything on this computer will be deleted."]
    if names: body += ["(%s)" % names]
    body += ["", "Press any key to CANCEL and restart into Windows."]
    lines = []
    for b in body: lines += (textwrap.wrap(b, width) or [""])
    put("\033[2J\033[H\n\n" + "".join("   %s\n" % l for l in lines))
result = "elapsed"
end = time.monotonic() + secs
try:
    while True:
        left = end - time.monotonic()
        if left <= 0: break
        whole = int(left) + (1 if left % 1 else 0)
        screen(whole)
        r, _, _ = select.select([fd], [], [], max(0.05, left - (whole - 1)))
        if r:
            try: data = os.read(fd, 64)
            except BlockingIOError: data = b""
            if data: result = "cancelled"; break
finally:
    termios.tcsetattr(fd, termios.TCSANOW, old)
print("== countdown %s after %.1f s" % (result, secs - max(0.0, end - time.monotonic())), file=sys.stderr)
sys.exit(0 if result == "elapsed" else 1)
PYEOF
    rc=$?
    RESULT=elapsed; [ "$rc" = 1 ] && RESULT=cancelled
    if [ "$rc" != 0 ] && [ "$rc" != 1 ]; then
        refuse 26 "the last-chance countdown could not be shown, and nothing is erased without it" "the countdown could not run (python exit $rc) - no erase without a visible last exit"
    fi
    if [ "$RESULT" = cancelled ]; then
        printf '\033[2J\033[H\n\n   CANCELLED. Nothing was erased. Restarting into Windows...\n' > "$CTTY"
        echo "== countdown CANCELLED by a key press - nothing erased; restarting into Windows"
        countdown_record cancelled
        sleep 3
        systemctl reboot 2>/dev/null || reboot -f 2>/dev/null || { echo b > /proc/sysrq-trigger; }
        sleep 60
        exit 27
    fi
    printf '\033[2J\033[H\n\n   Erasing and installing Fedora. This takes a while; you can walk away.\n' > "$CTTY"
    echo "== countdown ELAPSED - the commit line: the installer now erases $what"
    countdown_record elapsed
    chvt 1 2>/dev/null || true
fi
exit 0
