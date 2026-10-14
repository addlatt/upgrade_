#!/usr/bin/env python3
"""
measure-release.py - write one release's entry in data/releases.json from the
release's own files (RISKS R34, decided 2026-10-03, the owner: the kit's
Linux release is chosen from what the machine accepts). The facts in the
table are measured here, never typed by hand (rule #2).

    measure-release.py --id fedora-44 --name "Fedora 44" --family fedora \
        --installer kickstart --netinst URL ISO \
        --desktop kde "KDE Plasma" URL ISO --desktop gnome GNOME URL ISO \
        [--image-path LiveOS/squashfs.img] [--table data/releases.json]

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

def boot_entry(role, f):
    return {'Role': role, 'File': f['file'], 'Sha256': f['sha256'], 'Sbat': f['sbat'], 'SbatLevel': list(f['sbatlevel']), 'SignedBy': list(f['signed_by'])}

def main():
    a = argparse.ArgumentParser()
    a.add_argument('--id', required=True); a.add_argument('--name', required=True)
    a.add_argument('--family', required=True); a.add_argument('--installer', default='')
    a.add_argument('--netinst', nargs=2, metavar=('URL', 'ISO'), required=True)
    a.add_argument('--desktop', nargs=4, action='append', metavar=('ID', 'LABEL', 'URL', 'ISO'), default=[])
    a.add_argument('--image-path', default='LiveOS/squashfs.img')
    a.add_argument('--table', default=str(ROOT / 'data/releases.json'))
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
        netinst = {'Url': url, 'Sha256': iso_sha}
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
            desktops.append({'Id': did, 'Label': label, 'Url': durl, 'IsoSha256': dsha, 'ImagePath': o.image_path, 'ImageSha256': isha})
            shutil.rmtree(d, ignore_errors=True)
    entry = {'Id': o.id, 'Name': o.name, 'Family': o.family, 'Installer': o.installer, 'Measured': today, 'Note': o.note,
             'Netinst': netinst, 'Desktops': desktops, 'Boot': boot}
    # the entry replaces one with the same Id, or goes at the end; the rest of the file is untouched
    doc = json.load(open(o.table, encoding='utf-8'))
    rels = doc['Releases']
    at = next((i for i, r in enumerate(rels) if r.get('Id') == o.id), None)
    if at is None:
        rels.append(entry)
    else:
        rels[at] = entry
    with open(o.table, 'w', encoding='utf-8') as f:
        json.dump(doc, f, indent=2, ensure_ascii=False); f.write('\n')
    print('measure-release: wrote %s into %s' % (o.id, o.table), file=sys.stderr)

if __name__ == '__main__':
    main()
