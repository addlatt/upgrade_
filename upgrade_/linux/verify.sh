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
#
# Everything it learns goes to the stick, under upgrade_/report/, as
# verify.json plus the raw logs, and it syncs before it returns. With
# upg.mode=verify on the kernel command line (the reversible half of the
# vertical) it reboots after writing the report and the install never
# starts - the machine comes back to Windows with the report on the stick.
set -u
JOB=${1:?job.json path}
LABEL=${2:-UPGV0}
VERIFY_VERSION=0.2.0
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

# --- 0. the job ---------------------------------------------------------------
SCHEMA=$(jq_ schema)
if [ "$SCHEMA" != "job/1" ]; then echo "!! job schema '$SCHEMA' is not job/1 - refusing"; exit 10; fi
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
norm() { echo "$1" | tr 'A-Z' 'a-z' | sed 's/^eui\.//; s/[^0-9a-f]//g'; }
uid_hex=$(norm "$J_UID"); serial_norm=$(echo "$J_SERIAL" | tr 'A-Z' 'a-z' | tr -d ' _.-')
DISK=""; MATCHED_BY=""
for link in /dev/disk/by-id/*; do
    [ -e "$link" ] || continue
    name=$(basename "$link" | tr 'A-Z' 'a-z')
    case "$name" in *-part[0-9]*) continue;; esac
    tgt=$(readlink -f "$link"); [ -b "$tgt" ] || continue
    case "$tgt" in *[0-9]p[0-9]*|/dev/sd[a-z]*[0-9]) continue;; esac
    if [ -n "$uid_hex" ] && [ ${#uid_hex} -ge 8 ] && echo "$name" | tr -d '_.-' | grep -q "$uid_hex"; then DISK=$tgt; MATCHED_BY="unique_id via $(basename "$link")"; break; fi
done
if [ -z "$DISK" ] && [ -n "$serial_norm" ] && [ ${#serial_norm} -ge 6 ]; then
    for link in /dev/disk/by-id/*; do
        name=$(basename "$link" | tr 'A-Z' 'a-z' | tr -d '_.-')
        case "$name" in *part[0-9]*) continue;; esac
        if echo "$name" | grep -q "$serial_norm"; then DISK=$(readlink -f "$link"); MATCHED_BY="serial via $(basename "$link")"; break; fi
    done
fi
IDENTITY=fail; DISK_SIZE=""
if [ -n "$DISK" ]; then
    DISK_SIZE=$(blockdev --getsize64 "$DISK" 2>/dev/null || echo "")
    echo "== candidate $DISK ($MATCHED_BY) size=$DISK_SIZE"
    if [ "$DISK_SIZE" = "$J_SIZE" ]; then IDENTITY=pass; else echo "!! size mismatch: job says $J_SIZE, disk is $DISK_SIZE"; fi
else
    echo "!! no attached disk carries the job's unique id or serial"
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
        bytes=$(stat -c %s "$STICK/$IMG_REL"); t0=$(date +%s.%N)
        got=$(sha256sum "$STICK/$IMG_REL" | cut -c1-64)
        t1=$(date +%s.%N)
        IMAGE_MBPS=$(python3 -c "print(round($bytes/1e6/max($t1-$t0,0.001),1))")
        if [ "$got" = "$want" ]; then IMAGE_RESULT=pass; IMAGE_DETAIL="$IMG_REL $bytes bytes sha256 ok, read at $IMAGE_MBPS MB/s"
        else IMAGE_DETAIL="$IMG_REL sha256 MISMATCH (want $want got $got)"; fi
    fi
fi
echo "== desktop image: $IMAGE_RESULT - $IMAGE_DETAIL"

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
            cat > "$STORAGE_KS" <<EOF
# written by verify.sh: keep-windows on $DISK ($MATCHED_BY), ESP $ESP reused unformatted
ignoredisk --only-use=$dev
bootloader --location=mbr --boot-drive=$dev
part /boot/efi --onpart=$(basename "$ESP") --noformat
part /boot --fstype=ext4 --size=1024 --ondisk=$dev
part /     --fstype=ext4 --size=4096 --grow --ondisk=$dev
EOF
        else
            ESP_RESULT=fail; echo "!! keep-windows: no EFI partition holding bootmgfw.efi on $DISK"
        fi
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
  "storage": {"path": "$PATH_CHOSEN", "esp": "$ESP", "esp_result": "$ESP_RESULT", "include_written": $([ -f "$STORAGE_KS" ] && echo True || echo False)},
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
[ "$IDENTITY" = pass ] || { echo "!! IDENTITY MISMATCH - refusing to install on this machine"; exit 20; }
[ -f "$STORAGE_KS" ] || { echo "!! no storage include written - refusing"; exit 21; }
[ "$ESP_RESULT" != fail ] || { echo "!! keep-windows needs the Windows ESP - refusing"; exit 22; }
[ "$IMAGE_RESULT" = pass ] || { echo "!! the desktop image on the stick did not verify - refusing (RISKS R17)"; exit 23; }
exit 0
