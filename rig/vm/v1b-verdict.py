#!/usr/bin/env python3
"""V1b / R21 verdict: turn the run's evidence into ONE CSV row.

    v1b-verdict.py <artifacts/v1b dir> <results.csv> <harness version> [firmware] [secureboot]

Inputs (all produced by the run, none typed by a human):
  01-post-shrink.json / 02-post-install.json / 03-post-cycles.json  (v1b-inspect.py)
  shrink.json                (guest v1b-shrink.ps1)
  oemdrv.img:/boots.log      (v1b-mark on both OSes; install-done from %post)
  oemdrv.img:/v1b-post.log   (kickstart %post: stock vs regenerated os-prober state)

Checks, each recorded separately so a fail names its clause:
  (a) esp_room          the ESP still has free space after the install, and
                        the bytes the install added are measured
  (b) bootmgfw_intact   sha256 identical before/after install and after cycles
  (c) windows_via_grub  >=1 windows-boot row after install-done whose
                        BootCurrent is NOT the Windows Boot Manager entry
  (d) linux_boots       >=1 linux-boot row
  (e) cycles            >=2 windows-boot and >=2 linux-boot rows after install
result: pass-plumbing only if all five hold; otherwise the first failing clause.
SB is off on this rig: this row can never satisfy R21's Secure Boot clause.
"""
import csv, json, os, re, subprocess, sys, datetime

A, CSV, VER = sys.argv[1], sys.argv[2], sys.argv[3]
FIRMWARE = sys.argv[4] if len(sys.argv) > 4 else 'QEMU q35 + OVMF 2024.02 (non-SMM)'
SECUREBOOT = sys.argv[5] if len(sys.argv) > 5 else 'off'
OEM = os.path.join(A, 'oemdrv.img') + '@@1M'

def j(name):
    p = os.path.join(A, name)
    return json.load(open(p, encoding='utf-8-sig')) if os.path.exists(p) else None  # PS 5.1 writes a BOM

def mtype(name):
    r = subprocess.run(['mtype', '-i', OEM, '::/' + name], capture_output=True, text=True)
    return r.stdout if r.returncode == 0 else ''

pre, post, cyc, shrink = j('01-post-shrink.json'), j('02-post-install.json'), j('03-post-cycles.json'), j('shrink.json')
boots = mtype('boots.log').splitlines()
postlog = mtype('v1b-post.log')
notes = []

# --- (a) ESP room --------------------------------------------------------------
esp_size_mib = pre['esp']['size_mib'] if pre else ''
free_before = pre['esp']['fat_bytes_free'] if pre else None
free_after = post['esp']['fat_bytes_free'] if post else None
added = None
if pre and post:
    added = post['esp']['file_bytes_used'] - pre['esp']['file_bytes_used']
    new_files = sorted(set(post['esp']['manifest']) - set(pre['esp']['manifest']))
    notes.append(f"install added {len(new_files)} files / {added} B to the ESP ({', '.join(sorted({f.split('/')[2] for f in new_files if f.count('/')>=3}))})")
esp_room = bool(post) and free_after is not None and free_after > 0 and added is not None and added >= 0
# arithmetic when the ESP is bigger (QEMU guest: 260 MiB); on a ~100 MiB ESP the (a) check exercises it for real
fits_100 = ''
if pre and added is not None:
    fits_100 = 'computed-yes' if pre['esp']['file_bytes_used'] + added <= 96 * 2**20 else 'computed-NO'

# --- (b') ANY pre-existing ESP file changed or removed by the install ------------------
# Windows rewrites its own BCD / BOOTSTAT / BCD.LOG* on boot; everything else that
# Windows Setup placed must survive byte-for-byte. EFI/Boot/bootx64.efi is the
# removable-media fallback loader (Windows' copy of bootmgfw.efi) - shim-x64
# overwrites it, which is a design input the converter must handle (back up /
# restore at rollback), so it gets its own result rather than a bare pass.
WINDOWS_VOLATILE = re.compile(r'^/EFI/Microsoft/(Boot|Recovery)/(BCD|BCD\.LOG\d?|BOOTSTAT\.DAT)$')
changed, removed = [], []
if pre and post:
    for k, v in pre['esp']['manifest'].items():
        if WINDOWS_VOLATILE.match(k):
            continue
        if k not in post['esp']['manifest']:
            removed.append(k)
        elif post['esp']['manifest'][k]['sha256'] != v['sha256']:
            changed.append(k)
preexisting_changed = ';'.join(sorted(changed) + [r + '(removed)' for r in sorted(removed)])
microsoft_touched = any(k.startswith('/EFI/Microsoft/') for k in changed + removed)
fallback_replaced = '/EFI/Boot/bootx64.efi' in changed or '/EFI/Boot/bootx64.efi' in removed
if preexisting_changed:
    notes.append('pre-existing ESP files modified: ' + preexisting_changed)

# --- (b) bootmgfw intact -----------------------------------------------------------
shas = {k: d['esp']['bootmgfw_sha256'] for k, d in (('pre', pre), ('post', post), ('cycles', cyc)) if d}
row_shas = set(re.findall(r'bootmgfw_sha256=([0-9a-f]{64})', '\n'.join(boots)))
bootmgfw_intact = bool(pre and post) and len(set(shas.values()) | row_shas) == 1
if shrink and shrink.get('bootmgfw_sha256') and pre and shrink['bootmgfw_sha256'] != pre['esp']['bootmgfw_sha256']:
    bootmgfw_intact = False; notes.append('guest-side and host-side pre-install hashes disagree')

# --- (c)(d)(e) boot rows ---------------------------------------------------------
after = []
seen_done = False
for line in boots:
    if line.startswith('install-done,'):
        seen_done = True; continue
    if seen_done:
        after.append(line)
if not seen_done:
    notes.append('no install-done marker in boots.log')
win_rows = [l for l in after if l.startswith('windows-boot,')]
lin_rows = [l for l in after if l.startswith('linux-boot,')]
# which Boot#### is Windows' own? efibootmgr -v output in the %post log names it.
win_entry = None
m = re.search(r'Boot([0-9A-F]{4})\*?\s+Windows Boot Manager', postlog)
if m:
    win_entry = m.group(1)
fed_entry = None
m = re.search(r'Boot([0-9A-F]{4})\*?\s+Fedora', postlog)
if m:
    fed_entry = m.group(1)
win_bc = [re.search(r'BootCurrent=([0-9A-Fa-f]{4}|\w+)', l).group(1) for l in win_rows if re.search(r'BootCurrent=(\S+?),', l)]
win_via_grub = [bc for bc in win_bc if bc.upper() == (fed_entry or '').upper()]
windows_via_grub = len(win_via_grub) >= 1
if win_rows and not windows_via_grub:
    notes.append(f'windows BootCurrent values {win_bc} (Windows entry {win_entry}, Fedora entry {fed_entry})')
linux_boots = len(lin_rows) >= 1
cycles = len(win_rows) >= 2 and len(lin_rows) >= 2

# --- os-prober stock state -------------------------------------------------------------
stock = re.search(r'stock grub.cfg: lines mentioning Windows: (\d+)', postlog)
regen = re.search(r'regenerated grub.cfg: lines mentioning Windows: (\d+)', postlog)
disable = re.search(r'^GRUB_DISABLE_OS_PROBER=(\S+)', postlog, re.M)
os_prober_stock = ''
if stock and regen:
    os_prober_stock = f"stock={'yes' if int(stock.group(1)) else 'no'}(GRUB_DISABLE_OS_PROBER={disable.group(1) if disable else 'unset'});after-enable={'yes' if int(regen.group(1)) else 'no'}"

# --- verdict --------------------------------------------------------------------------
if not post:
    result = 'install-failed'
elif not esp_room:
    result = 'esp-full'
elif not bootmgfw_intact or microsoft_touched:
    result = 'windows-files-changed'
elif not windows_via_grub:
    result = 'windows-unbootable-via-grub'
elif not linux_boots:
    result = 'linux-unbootable'
elif not cycles:
    result = 'cycles-incomplete'
elif fallback_replaced or preexisting_changed:
    result = 'fallback-loader-replaced'
else:
    result = 'pass-plumbing'
if shrink:
    notes.append(f"shrink: freed {shrink.get('freed_bytes')} B, cold shrinkable {shrink.get('shrinkable_cold')} B, capped={shrink.get('capped_by_immovable')}")
if shrink and shrink.get('bitlocker_volume_status') and shrink.get('bitlocker_volume_status') != 'none':
    notes.append(f"bitlocker C: {shrink.get('bitlocker_volume_status')}/{shrink.get('bitlocker_protection')} {shrink.get('bitlocker_method')} protectors={shrink.get('bitlocker_protectors')} at shrink time; suspended -RebootCount 1 for the installer boot per run-book")

row = {
    'timestamp': datetime.datetime.now(datetime.timezone.utc).isoformat(timespec='seconds'),
    'harness': VER, 'firmware': FIRMWARE, 'secureboot': SECUREBOOT,
    'esp_size_mib': esp_size_mib, 'esp_free_before': free_before, 'esp_free_after': free_after,
    'esp_added_bytes': added, 'fits_100mib_esp': fits_100,
    'bootmgfw_intact': 'y' if bootmgfw_intact else 'n',
    'preexisting_changed': preexisting_changed,
    'windows_via_grub': len(win_via_grub), 'windows_boots': len(win_rows), 'linux_boots': len(lin_rows),
    'os_prober_stock': os_prober_stock, 'result': result, 'notes': '; '.join(notes),
}
new = not os.path.exists(CSV)
with open(CSV, 'a', newline='') as f:
    w = csv.DictWriter(f, fieldnames=list(row), quoting=csv.QUOTE_ALL)
    if new:
        w.writeheader()
    w.writerow(row)
print(json.dumps(row, indent=1))
print(f'v1b-verdict: appended to {CSV}')
