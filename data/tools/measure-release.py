#!/usr/bin/env python3
"""
measure-release.py - write one release's entry in data/releases.ps1 from the
release's own files (RISKS R34, decided 2026-10-03, the owner: the kit's
Linux release is chosen from what the machine accepts). The facts in the
table are measured here, never typed by hand (rule #2).

    measure-release.py --id fedora-44 --name "Fedora 44" --family fedora \
        --installer kickstart --netinst URL ISO \
        --desktop kde "KDE Plasma" URL ISO --desktop gnome GNOME URL ISO \
        [--image-path LiveOS/squashfs.img] [--table data/releases.ps1]

For the installer ISO: its sha256, and the facts of EFI/BOOT/BOOTX64.EFI
(shim) and EFI/BOOT/grubx64.efi (the install-media GRUB, which reads the
stick's EFI/BOOT/grub.cfg). For each desktop ISO: its sha256, the sha256 of
the live image inside it (what the stick carries and the kickstart
installs), and the facts of the boot files the installed system will start
from (/boot/efi/EFI/fedora/shimx64.efi and grubx64.efi in that image).
Facts per boot file (data/tools/pe_facts.py): sha256, the .sbat text, a
shim's built-in levels, and the certificate authorities it is signed under.

The entry replaces the block between '# BEGIN <id>' and '# END <id>' in the
table, or is added before the table's closing marker.
"""
import argparse, datetime, hashlib, json, os, pathlib, shutil, subprocess, sys, tempfile

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import pe_facts

ROOT = HERE.parents[1]
EROFS = os.environ.get('UPG_EROFS_BIN', str(ROOT / 'rig/vm/artifacts/tools/erofs/usr/bin'))

def sha256(path):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for b in iter(lambda: f.read(1 << 20), b''): h.update(b)
    return h.hexdigest()

def iso_extract(iso, inner, dest):
    subprocess.run(['bsdtar', '-x', '-f', iso, '-C', dest, inner], check=True)
    return os.path.join(dest, inner)

def image_files(img, wanted, tmp):
    """Pull files out of a live image (EROFS or squashfs) into tmp; returns {path: local}."""
    kind = open(img, 'rb').read(1100)
    out = {}
    if kind[1024:1028] == b'\xe2\xe1\xf5\xe0':           # EROFS superblock magic at 1024
        root = os.path.join(tmp, 'root')
        subprocess.run([os.path.join(EROFS, 'fsck.erofs'), '--extract=' + root, '--no-preserve', img],
                       check=True, stdout=subprocess.DEVNULL)
    elif kind[:4] == b'hsqs':
        root = os.path.join(tmp, 'root')
        subprocess.run(['unsquashfs', '-q', '-n', '-d', root, img] + [w.lstrip('/') for w in wanted], check=True)
    else:
        raise SystemExit('measure-release: %s is neither EROFS nor squashfs' % img)
    subprocess.run(['chmod', '-R', 'u+rwX', root], check=False)   # unpacked 0500 folders would block the clean-up
    for w in wanted:
        p = os.path.join(root, w.lstrip('/'))
        if os.path.exists(p): out[w] = p
    return out

def ps(s):
    return "'" + str(s).replace("'", "''") + "'"

def boot_entry(role, f):
    lv = ', '.join(ps(x) for x in f['sbatlevel'])
    by = ', '.join(ps(x) for x in f['signed_by'])
    return ("        @{ Role = %s; File = %s; Sha256 = %s\n"
            "           Sbat = %s\n"
            "           SbatLevel = @(%s); SignedBy = @(%s) }\n") % (ps(role), ps(f['file']), ps(f['sha256']), ps(f['sbat']), lv, by)

def main():
    a = argparse.ArgumentParser()
    a.add_argument('--id', required=True); a.add_argument('--name', required=True)
    a.add_argument('--family', required=True); a.add_argument('--installer', default='')
    a.add_argument('--netinst', nargs=2, metavar=('URL', 'ISO'), required=True)
    a.add_argument('--desktop', nargs=4, action='append', metavar=('ID', 'LABEL', 'URL', 'ISO'), default=[])
    a.add_argument('--image-path', default='LiveOS/squashfs.img')
    a.add_argument('--table', default=str(ROOT / 'data/releases.ps1'))
    a.add_argument('--note', default='')
    o = a.parse_args()
    today = datetime.date.today().isoformat()
    boot, desktops = [], []
    with tempfile.TemporaryDirectory() as tmp:
        url, iso = o.netinst
        print('measure-release: installer %s' % iso, file=sys.stderr)
        iso_sha = sha256(iso)
        for inner in ('EFI/BOOT/BOOTX64.EFI', 'EFI/BOOT/grubx64.efi'):
            boot.append(boot_entry('stick', pe_facts.facts(iso_extract(iso, inner, tmp))))
        netinst = '@{ Url = %s; Sha256 = %s }' % (ps(url), ps(iso_sha))
        for did, label, durl, diso in o.desktop:
            print('measure-release: desktop %s (%s): unpacking its image, this takes minutes' % (did, diso), file=sys.stderr)
            d = os.path.join(tmp, did); os.makedirs(d)
            dsha = sha256(diso)
            img = iso_extract(diso, o.image_path, d)
            isha = sha256(img)
            got = image_files(img, ['/boot/efi/EFI/fedora/shimx64.efi', '/boot/efi/EFI/fedora/grubx64.efi'], d)
            if len(got) != 2:
                raise SystemExit('measure-release: %s lacks the installed boot files (found %s); refusing to guess' % (did, list(got)))
            for w in ('/boot/efi/EFI/fedora/shimx64.efi', '/boot/efi/EFI/fedora/grubx64.efi'):
                boot.append(boot_entry('installed ' + did, pe_facts.facts(got[w])))
            desktops.append('        @{ Id = %s; Label = %s; Url = %s; IsoSha256 = %s; ImagePath = %s; ImageSha256 = %s }\n'
                            % (ps(did), ps(label), ps(durl), ps(dsha), ps(o.image_path), ps(isha)))
            shutil.rmtree(d, ignore_errors=True)
    block = ("    # BEGIN %s (measured %s by data/tools/measure-release.py from the files themselves; do not edit by hand)\n"
             "    @{ Id = %s; Name = %s; Family = %s; Installer = %s; Measured = %s; Note = %s\n"
             "       Netinst = %s\n"
             "       Desktops = @(\n%s       )\n"
             "       Boot = @(\n%s       ) }\n"
             "    # END %s\n") % (o.id, today, ps(o.id), ps(o.name), ps(o.family), ps(o.installer), ps(today), ps(o.note),
                                  netinst, ''.join(desktops), ''.join(boot), o.id)
    t = open(o.table).read()
    b, e = '    # BEGIN %s ' % o.id, '    # END %s\n' % o.id
    if b in t:
        i = t.index(b); j = t.index(e, i) + len(e)
        t = t[:i] + block + t[j:]
    else:
        m = '    # (releases end)\n'
        assert m in t, 'no "# (releases end)" marker in the table'
        t = t.replace(m, block + m)
    open(o.table, 'w').write(t)
    print('measure-release: wrote %s into %s' % (o.id, o.table), file=sys.stderr)

if __name__ == '__main__':
    main()
