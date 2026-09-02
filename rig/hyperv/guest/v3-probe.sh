#!/bin/bash
# V3 / R19 step 1 — does cryptsetup recognise and open the guest's BitLocker
# C: from the INSTALLED Fedora (settle-in's context)? Runs as root through the
# OEMDRV run hook (guest/oemdrv-run.sh); everything it prints lands in
# OEMDRV:/run.log. The recovery password is read from OEMDRV:/v3/key.txt and
# that file is removed before anything else happens; the key is never echoed
# (no set -x in this script — deliberately).
OEM=/mnt/oemdrv
DEV=${V3_DEV:-/dev/sda3}
MAP=winc
KEY=$(grep -oE '[0-9]{6}(-[0-9]{6}){7}' "$OEM/v3/key.txt" | head -1)
rm -f "$OEM/v3/key.txt"; sync
[ -n "$KEY" ] || { echo "probe: no key on OEMDRV"; echo poweroff > $OEM/next; exit 1; }
run() { echo; echo "\$ $*"; "$@"; echo "[rc=$?]"; }
run uname -r
run cryptsetup --version
run rpm -q cryptsetup ntfs-3g ntfsprogs kernel-core
run lsblk -o NAME,SIZE,TYPE,FSTYPE,LABEL,PARTTYPENAME,PARTUUID /dev/sda
run blkid "$DEV"
run cryptsetup bitlkDump "$DEV"
run modinfo -F name ntfs3

echo; echo "== unlock method A: passphrase on stdin (cryptsetup strips the trailing newline)"
printf '%s\n' "$KEY" | cryptsetup open --type bitlk --readonly "$DEV" $MAP; echo "[rc=$?]"
if [ -e /dev/mapper/$MAP ]; then run cryptsetup status $MAP; run cryptsetup close $MAP; fi

echo; echo "== unlock method B: --key-file holding the 48 digits with dashes, NO trailing newline"
printf '%s' "$KEY" > /run/v3.key; chmod 600 /run/v3.key
cryptsetup open --type bitlk --readonly --key-file /run/v3.key "$DEV" $MAP; echo "[rc=$?]"
if [ -e /dev/mapper/$MAP ]; then run cryptsetup close $MAP; fi

echo; echo "== unlock method C: --key-file WITH a trailing newline (the trap to document)"
printf '%s\n' "$KEY" > /run/v3.key
cryptsetup open --type bitlk --readonly --key-file /run/v3.key "$DEV" $MAP; echo "[rc=$?]"
if [ -e /dev/mapper/$MAP ]; then run cryptsetup close $MAP; fi

echo; echo "== unlock method D: bitlkOpen alias, stdin"
printf '%s\n' "$KEY" | cryptsetup bitlkOpen --readonly "$DEV" $MAP; echo "[rc=$?]"
if [ -e /dev/mapper/$MAP ]; then run cryptsetup close $MAP; fi

echo; echo "== unlock method E: a WRONG key (last digit changed) must be refused"
WRONG="${KEY%?}$(( (${KEY: -1} + 1) % 10 ))"
printf '%s\n' "$WRONG" | cryptsetup open --type bitlk --readonly "$DEV" $MAP; echo "[rc=$?]"
if [ -e /dev/mapper/$MAP ]; then echo "!! WRONG KEY OPENED THE VOLUME"; run cryptsetup close $MAP; fi
unset WRONG

echo; echo "== open for the mount tests (method A)"
printf '%s\n' "$KEY" | cryptsetup open --type bitlk --readonly "$DEV" $MAP; echo "[rc=$?]"
unset KEY; rm -f /run/v3.key
[ -e /dev/mapper/$MAP ] || { echo "probe: no mapping, stopping"; echo poweroff > $OEM/next; exit 1; }
run cryptsetup status $MAP
run blkid /dev/mapper/$MAP
run ntfsinfo -m /dev/mapper/$MAP
mkdir -p /mnt/win
for fs in ntfs3 ntfs-3g ntfs; do
    echo; echo "== mount -t $fs -o ro"
    mount -t $fs -o ro /dev/mapper/$MAP /mnt/win; rc=$?; echo "[rc=$rc]"
    if [ $rc -eq 0 ]; then
        run findmnt -no FSTYPE,OPTIONS /mnt/win
        run ls /mnt/win
        run ls /mnt/win/Users
        run sha256sum /mnt/win/Windows/System32/ntoskrnl.exe /mnt/win/Windows/System32/config/SOFTWARE /mnt/win/Windows/explorer.exe
        run stat -c '%s %n' /mnt/win/Windows/System32/ntoskrnl.exe /mnt/win/Windows/explorer.exe
        run dmesg -T --since -2min 2>/dev/null
        run umount /mnt/win
    else
        dmesg -T | tail -5
    fi
done
run cryptsetup close $MAP
run journalctl -b -p warning --no-pager -q -o short-iso
echo poweroff > $OEM/next
