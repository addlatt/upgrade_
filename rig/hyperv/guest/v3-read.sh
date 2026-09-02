#!/bin/bash
# V3 / R19 - Fedora side (INSTALLED guest, root, via the OEMDRV run hook).
# Unlocks the Windows BitLocker partition with the recovery password from
# OEMDRV:/v3/key.txt (removed first thing; never echoed), mounts it read-only,
# and hashes the same trees the Windows side hashed (guest/v3-plant.ps1),
# writing manifests of the same shape next to Windows' ones on OEMDRV - once
# per NTFS driver (ntfs-3g/FUSE first, then the kernel's ntfs3, one corpus
# subtree at a time with a sync after each, so a kernel crash leaves the
# last completed stage on the volume). The previous boot's kernel log is
# saved first: a crash in the last attempt is evidence, not lost.
# Then powers off. The verdict is computed on the host (v3-verdict.py).
OEM=/mnt/oemdrv
V3=$OEM/v3
DEV=${V3_DEV:-/dev/sda3}
[ -f "$OEM/v3/dev" ] && DEV=$(tr -d "\r\n " < "$OEM/v3/dev")   # the host names the partition, or 'auto'
if [ "$DEV" = auto ]; then
    # a config disk attached as a data disk: the BitLocker partition NOT on the disk holding /
    rootdisk=$(lsblk -no PKNAME "$(findmnt -no SOURCE /)" | head -1)
    DEV=$(blkid -t TYPE=BitLocker -o device | grep -v "^/dev/$rootdisk" | head -1)
fi
MAP=winc
F=$V3/facts-linux.txt
fact() { printf '%s=%s\n' "$1" "$2" >> "$F"; sync; }
: > "$F"
rm -f "$OEM/run.sh"      # run once, whatever happens next
KEY=$(grep -oE '[0-9]{6}(-[0-9]{6}){7}' "$V3/key.txt" 2>/dev/null | head -1)
rm -f "$V3/key.txt"; sync
journalctl -b -1 -k --no-pager -q -o short-iso > "$V3/journal-prev-boot-kernel.txt" 2>/dev/null
journalctl -b -1 --no-pager -q -o short-iso -u oemdrv-run > "$V3/journal-prev-boot-hook.txt" 2>/dev/null
fact prev_boot_oops "$(grep -cE 'Oops|BUG:|general protection|kernel panic|Call Trace' "$V3/journal-prev-boot-kernel.txt")"
fact timestamp "$(date -u +%FT%TZ)"
fact kernel "$(uname -r)"
fact cryptsetup "$(cryptsetup --version | awk '{print $2}')"
fact ntfs3_module "$(modinfo -F name ntfs3 2>/dev/null || echo none)"
fact ntfs3g "$(rpm -q ntfs-3g 2>/dev/null)"
fact device "$DEV"
fact partition_bytes "$(blockdev --getsize64 $DEV 2>/dev/null)"
[ -n "$KEY" ] || { fact stage "no-key"; echo poweroff > $OEM/next; exit 1; }

echo "== bitlkDump"; cryptsetup bitlkDump "$DEV"
fact bitlk_cipher "$(cryptsetup bitlkDump $DEV | awk -F'\t' '/Cipher name/{n=$2}/Cipher mode/{m=$2}/Cipher key/{k=$2}END{print n"-"m"/"k}' | tr -d ' ')"
fact bitlk_volume_bytes "$(cryptsetup bitlkDump $DEV | awk '/Volume size/{print $3}')"
fact bitlk_protectors "$(cryptsetup bitlkDump $DEV | grep -c 'Protection:')"

echo "== open (recovery password on stdin)"
t0=$(date +%s)
msg=$(printf '%s\n' "$KEY" | cryptsetup open --type bitlk --readonly "$DEV" $MAP 2>&1); rc=$?
unset KEY
echo "$msg"; echo "[rc=$rc]"
fact open_rc "$rc"
fact open_message "$(echo "$msg" | tr '\n' ' ' | sed 's/[[:space:]]*$//')"
[ $rc -eq 0 ] && [ -e /dev/mapper/$MAP ] || { fact stage "unlock-failed"; echo poweroff > $OEM/next; exit 1; }
fact dm_size_sectors "$(cryptsetup status $MAP | awk '/^ *size:/{print $2}')"
fact dm_offset_sectors "$(cryptsetup status $MAP | awk '/offset:/{print $2}')"

# the same shape as the Windows manifest: sha256 <TAB> size <TAB> relpath (forward slashes, volume-relative)
cat > /run/v3-hash.py <<'PY'
import hashlib, os, stat, sys
top, rel0, out = sys.argv[1], sys.argv[2], sys.argv[3]
trace = open(sys.argv[4], 'w') if len(sys.argv) > 4 else None   # path of the file about to be read, fsynced: names a crash
rows = {}
errs = 0
KIND = {stat.S_IFIFO: 'fifo', stat.S_IFSOCK: 'sock', stat.S_IFCHR: 'chr', stat.S_IFBLK: 'blk'}
for d, dirs, files in os.walk(top, followlinks=False):
    dirs[:] = [x for x in dirs if not os.path.islink(os.path.join(d, x))]
    for f in files:
        p = os.path.join(d, f)
        if os.path.islink(p):
            continue
        rel = rel0 + os.path.relpath(p, top).replace(os.sep, '/')
        # lstat FIRST: a Windows 0-byte SYSTEM file is a FIFO to ntfs-3g and open() blocks forever (seen 2026-09-01)
        st = os.lstat(p)
        if not stat.S_ISREG(st.st_mode):
            rows[rel] = 'SPECIAL:%s\t%d\t%s' % (KIND.get(stat.S_IFMT(st.st_mode), 'other'), st.st_size, rel)
            continue
        if trace:
            trace.seek(0); trace.truncate(); trace.write(rel + '\n'); trace.flush(); os.fsync(trace.fileno())
        try:
            h = hashlib.sha256()
            n = 0
            with open(p, 'rb') as fh:
                for chunk in iter(lambda: fh.read(1 << 20), b''):
                    h.update(chunk); n += len(chunk)
            rows[rel] = '%s\t%d\t%s' % (h.hexdigest(), n, rel)
        except OSError as e:
            errs += 1
            rows[rel] = 'ERR:%s\t-1\t%s' % (type(e).__name__ + ':' + str(e.errno), rel)
with open(out, 'w', encoding='utf-8', newline='\n') as o:
    for k in sorted(rows):
        o.write(rows[k] + '\n')
print('%s: %d files, %d unreadable -> %s' % (top, len(rows), errs, out))
PY

mkdir -p /mnt/win
dmesg_before=$(dmesg | wc -l)
# passes: ntfs-3g (FUSE), the kernel ntfs3, and ntfs3 with readahead off on the dm device
# (the 2026-09-01 oops sits in page_cache_ra_unbounded - is readahead the trigger?)
for pass in ntfs-3g ntfs3 ntfs3ra0; do
    fs=${pass%ra0}; tag=$(echo $pass | tr -d '-')
    if [ "$pass" = ntfs3ra0 ]; then blockdev --setra 0 /dev/mapper/$MAP; else blockdev --setra 256 /dev/mapper/$MAP; fi
    fact "ra_${tag}" "$(blockdev --getra /dev/mapper/$MAP)"
    echo "== mount -t $fs -o ro ($pass)"
    msg=$(mount -t $fs -o ro /dev/mapper/$MAP /mnt/win 2>&1); rc=$?; echo "$msg"; echo "$msg"; echo "[rc=$rc]"
    fact "mount_${tag}_rc" "$rc"
    [ $rc -eq 0 ] || { fact "mount_${tag}_message" "$msg"; continue; }
    fact "mount_${tag}_type" "$(findmnt -no FSTYPE /mnt/win)"
    t1=$(date +%s)
    rm -f "$V3"/part-$tag-*.txt
    for sub in /mnt/win/v3corpus/*/; do
        s=$(basename "$sub")
        fact "stage" "$tag:corpus/$s"
        python3 /run/v3-hash.py "$sub" "v3corpus/$s/" "$V3/part-$tag-$s.txt" "$V3/trace-$tag-$s.txt"; echo "[rc=$?]"; sync
    done
    cat "$V3"/part-$tag-*.txt > "$V3/manifest-linux-$tag-corpus.txt"; sync
    fact "stage" "$tag:users"
    python3 /run/v3-hash.py /mnt/win/Users Users/ "$V3/manifest-linux-$tag-users.txt" "$V3/trace-$tag-users.txt"; echo "[rc=$?]"; sync
    fact "hash_${tag}_seconds" $(( $(date +%s) - t1 ))
    umount /mnt/win; fact "umount_${tag}_rc" "$?"
done
fact dmesg_new_lines "$(dmesg | tail -n +$((dmesg_before + 1)) | grep -ciE 'ntfs|I/O error|dm-|device-mapper|sda')"
dmesg | tail -n +$((dmesg_before + 1)) | grep -iE 'ntfs|I/O error|dm-|device-mapper|sda' > "$V3/dmesg-linux.txt"
cryptsetup close $MAP; fact close_rc "$?"
fact total_seconds $(( $(date +%s) - t0 ))
fact stage done
echo poweroff > $OEM/next
