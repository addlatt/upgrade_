#!/bin/bash
# Pull arm (V3, V8; R8, R19, R20) - Fedora side (INSTALLED guest, root, via
# the OEMDRV run hook). Rig tooling, not product.
#
# Runs the product's own pull on the Windows partition guest/pull-plant.ps1
# planted: writes the handoff folder settle-in reads (job.json from the
# harvester's folder map, outcome.json naming the BitLocker partition by its
# PARTUUID, the recovery key file in the job writer's format), makes a fresh
# person (pulltest) whose home is empty, then runs, as that person had
# started them through pkexec:
#   settle-in pull check   -> pull/check.json
#   settle-in pull run     -> pull/run.out  (progress lines, then the result)
# then hashes what landed in the home (pull/linux-home.tsv), reads the
# placeholders' and links' ntfs-3g attributes for the record (pull/xattr.txt,
# the fact V8's residue asks for), and powers off. The verdict is computed on
# the host (pull-verdict.py). The key never leaves root-only places and is
# removed from OEMDRV first thing.
OEM=/mnt/oemdrv
V3=$OEM/v3
P=$V3/pull
F=$P/facts-linux.txt
mkdir -p "$P"; : > "$F"
fact() { printf '%s=%s\n' "$1" "$2" >> "$F"; sync; }
rm -f "$OEM/run.sh"      # run once, whatever happens next
KEY=$(grep -oE '[0-9]{6}(-[0-9]{6}){7}' "$V3/key.txt" 2>/dev/null | head -1)
rm -f "$V3/key.txt"; sync
fact timestamp "$(date -u +%FT%TZ)"
fact kernel "$(uname -r)"
fact cryptsetup "$(cryptsetup --version | awk '{print $2}')"
fact ntfs3g "$(rpm -q ntfs-3g 2>/dev/null)"
SI=$V3/settle-in
install -m 0755 "$SI" /run/settle-in
fact settle_in "$(/run/settle-in --version)"
DEV=$(blkid -t TYPE=BitLocker -o device | head -1)
fact device "$DEV"
[ -n "$DEV" ] || { fact stage no-bitlocker-partition; echo poweroff > $OEM/next; exit 1; }
[ -n "$KEY" ] || { fact stage no-key; echo poweroff > $OEM/next; exit 1; }
PARTUUID=$(blkid -s PARTUUID -o value "$DEV")
fact partuuid "$PARTUUID"

# --- the handoff folder, as the installer leaves it on keep-Windows
H=/var/lib/upgrade_
rm -rf "$H/settle-in/pull.json" "$H/settle-in/pull-journal.tsv"
mkdir -p "$H/artifacts/credentials" "$H/settle-in"; chmod 700 "$H" "$H/artifacts" "$H/artifacts/credentials"
CONV=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1], encoding="utf-8-sig"))["bitlocker"]["conversion"])' "$P/expected.json")
fact windows_conversion_status "$CONV"
python3 - "$P/folder-map.json" "$H/job.json" <<'PY'
import json, sys
m = json.load(open(sys.argv[1], encoding="utf-8-sig"))
folders = [{"name": f["Name"], "path": f["Path"], "exists": f["Exists"], "is_onedrive": f["IsOneDrive"], "files": f["Files"], "bytes": f["Bytes"],
            "cloud_only_files": f["CloudOnlyFiles"], "truncated": f["Truncated"]} for f in m["UserFolders"]]
cloud = sum(f["cloud_only_files"] for f in folders)
job = {"schema": "job/1", "intent": {"path": "keep-windows"},
       "harvest": {"cloud_files": {"placeholders_found": cloud, "materialized": 0, "failed": 0, "result": "left-in-cloud" if cloud else "none-found"},
                   "folders": folders, "bitlocker": {"status": "on", "recovery_key_file": "artifacts/credentials/bitlocker-C.txt"}}}
json.dump(job, open(sys.argv[2], "w"), indent=1)
PY
printf '{"path_taken":"keep-windows","windows":{"kept":true,"partition":{"guid":"%s","size_bytes":1},"reachable_via":"grub"}}\n' "$PARTUUID" > "$H/outcome.json"
umask 077
printf '# rig: the job writer'"'"'s format (evaluate/job/src/recovery.rs)\nconversion-status: %s\n%s\n' "$CONV" "$KEY" > "$H/artifacts/credentials/bitlocker-C.txt"
# the rig's own copy, root-only on tmpfs, for the attribute record after the pull has spent the key
printf '%s\n' "$KEY" > /run/pull-key
unset KEY
umask 022
chmod 600 "$H/job.json" "$H/outcome.json"
cp "$H/job.json" "$P/job-as-written.json"

# --- a fresh person, an empty home
userdel -r pulltest >/dev/null 2>&1
useradd -m pulltest
U=$(id -u pulltest)
fact person_uid "$U"

# --- the product, as pkexec would start it
PKEXEC_UID=$U /run/settle-in pull check > "$P/check.json" 2> "$P/check.err"; fact check_rc $?
PKEXEC_UID=$U /run/settle-in pull run > "$P/run.out" 2> "$P/run.err"; fact run_rc $?
fact key_left_in_handoff "$( [ -e "$H/artifacts/credentials/bitlocker-C.txt" ] && echo yes || echo no )"
fact windows_still_mapped "$( [ -e /dev/mapper/upgrade_-pull-windows ] && echo yes || echo no )"
fact windows_still_mounted "$(findmnt -n /run/upgrade_/pull-windows >/dev/null && echo yes || echo no)"

# --- what landed: sha256 TAB size TAB owner TAB path (relative to the home)
( cd /home/pulltest && find . -type f -print0 | sort -z | while IFS= read -r -d '' f; do
    printf '%s\t%s\t%s\t%s\n' "$(sha256sum "$f" | cut -d' ' -f1)" "$(stat -c %s "$f")" "$(stat -c %U "$f")" "${f#./}"
  done ) > "$P/linux-home.tsv"
( cd /home/pulltest && find . -type l -o -type d -name LinkToWindows ) > "$P/linux-links.txt"
fact home_files "$(wc -l < "$P/linux-home.tsv")"
for b in /home/pulltest/Documents/Bookmarks\ from\ Windows\ -\ *.html; do
    [ -f "$b" ] && fact "bookmarks_file:$(basename "$b")" "$(grep -c '<DT><A HREF=' "$b")"
done

# --- the record V8's residue asks for: how ntfs-3g shows a placeholder, a downloaded one, a link
KEY=$(grep -oE '[0-9]{6}(-[0-9]{6}){7}' "$H/artifacts/credentials/bitlocker-C.txt" 2>/dev/null | head -1)
if [ -z "$KEY" ]; then KEY=$(cat /run/pull-key 2>/dev/null); fi
python3 - "$P/expected.json" > /run/pull-paths.txt <<'PY'
import json, sys
e = json.load(open(sys.argv[1], encoding="utf-8-sig"))
docs = e["documents"].replace("C:\\", "").replace("\\", "/")
for rel in e["placeholders"] + ["pull-test/Cloud/downloaded.pdf", "pull-test/LinkToWindows", "pull-test/letter-link.txt", "pull-test/desktop.ini", "pull-test/letter.txt"]:
    print(docs + "/" + rel)
PY
fact xattr_read "after the pull, by the rig (the product unmounted)"
echo "(the key was spent by the pull; the attributes are read only when the rig still holds one)" > "$P/xattr.txt"
if [ -n "$KEY" ]; then
    printf '%s\n' "$KEY" | cryptsetup open --type bitlk --readonly "$DEV" rigpull 2>>"$P/xattr.txt"
    mkdir -p /run/rigpull && mount -t ntfs-3g -o ro /dev/mapper/rigpull /run/rigpull 2>>"$P/xattr.txt"
    : > "$P/xattr.txt"
    while IFS= read -r rel; do
        python3 - "/run/rigpull/$rel" "$rel" >> "$P/xattr.txt" <<'PY'
import os, stat, sys
p, rel = sys.argv[1], sys.argv[2]
try:
    st = os.lstat(p)
    kind = "link" if stat.S_ISLNK(st.st_mode) else "dir" if stat.S_ISDIR(st.st_mode) else "file" if stat.S_ISREG(st.st_mode) else "other"
    def x(n):
        try: return os.getxattr(p, n, follow_symlinks=False).hex()
        except OSError as e: return "ERR:" + (e.strerror or str(e.errno))
    print("%s\tkind=%s size=%d blocks=%d attrib=%s reparse=%s" % (rel, kind, st.st_size, st.st_blocks, x("system.ntfs_attrib"), x("system.ntfs_reparse_data")[:24]))
except OSError as e:
    print("%s\tERR:%s" % (rel, e.strerror))
PY
    done < /run/pull-paths.txt
    umount /run/rigpull; cryptsetup close rigpull
fi
unset KEY
rm -f /run/pull-key
sync
echo poweroff > $OEM/next
