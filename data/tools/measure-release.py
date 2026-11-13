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

A casper ISO (Ubuntu's layout: one ISO that is live system, installer and
package pool at once) is measured with --casper and written to its own file
with --out, never into the table: an entry in the table is an offer to a
person, and a family is offered only once its rig row holds (RISKS R40).

    measure-release.py --id ubuntu-26.04.1 --name "Ubuntu 26.04.1 LTS" \
        --family ubuntu --installer autoinstall --casper URL ISO \
        --out data/measured/ubuntu-26.04.1.json

It records: the ISO's sha256 and size; the stick's boot files (EFI/boot's
shim, GRUB and MokManager) and the live kernel; the boot files the installed
system gets, read from the shim, GRUB and kernel packages in the ISO's pool
and from the kernel inside the base layer; every casper layer's size and
sha256, whether it is over FAT32's 4 GiB, and how many files the ISO splits
over several extents; the install sources; the snaps seeded in the base
layer; and the pool's size.
"""
import argparse, datetime, glob, hashlib, json, os, pathlib, shutil, struct, subprocess, sys, tempfile

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

FAT32_MAX = (1 << 32) - 1          # the largest file FAT32 can hold

def iso_split_files(iso):
    """The ISO 9660 names of files kept in several extents (the multi-extent flag, 0x80)."""
    split, seen = [], set()
    with open(iso, 'rb') as f:
        f.seek(16 * 2048); pvd = f.read(2048)
        assert pvd[1:6] == b'CD001' and pvd[0] == 1, 'not an ISO 9660 image'
        stack = [(struct.unpack_from('<I', pvd, 158)[0], struct.unpack_from('<I', pvd, 166)[0])]
        while stack:
            ext, size = stack.pop()
            if ext in seen: continue
            seen.add(ext)
            f.seek(ext * 2048); d = f.read(size)
            i = 0
            while i < len(d):
                ln = d[i]
                if ln == 0:                                   # records never cross a sector
                    i = (i // 2048 + 1) * 2048; continue
                flags, nlen = d[i + 25], d[i + 32]
                name = d[i + 33:i + 33 + nlen]
                if name not in (b'\0', b'\1'):
                    if flags & 0x02:
                        stack.append((struct.unpack_from('<I', d, i + 2)[0], struct.unpack_from('<I', d, i + 10)[0]))
                    elif flags & 0x80:
                        split.append(name.decode('ascii', 'replace'))
                i += ln
    return sorted(set(split))

def deb_unpack(deb, dest):
    """Unpack a .deb's data archive (ar, then tar in whatever compression) with bsdtar."""
    os.makedirs(dest)
    subprocess.run(['bsdtar', '-x', '-f', deb, '-C', dest], check=True)
    data = glob.glob(os.path.join(dest, 'data.tar*'))
    if len(data) != 1: raise SystemExit('measure-release: %s has no single data archive' % deb)
    root = os.path.join(dest, 'root'); os.makedirs(root)
    subprocess.run(['bsdtar', '-x', '-f', data[0], '-C', root], check=True)
    return root

def casper(o):
    import yaml
    url, iso = o.casper
    if not o.out: raise SystemExit('measure-release: --casper needs --out (a casper measurement never goes into the table)')
    print('measure-release: casper ISO %s: hashing and unpacking, this takes minutes' % iso, file=sys.stderr)
    listing = subprocess.run(['bsdtar', '-t', '-v', '-f', iso], check=True, capture_output=True, text=True).stdout
    files = {}
    for line in listing.splitlines():
        c = line.split(None, 8)
        if len(c) == 9 and not line.startswith('d'): files[c[8].split(' -> ')[0]] = int(c[4])
    boot, layers, pool = [], [], {n: s for n, s in files.items() if n.startswith('pool/') and n.endswith('.deb')}
    with tempfile.TemporaryDirectory() as tmp:
        for inner in ('EFI/boot/bootx64.efi', 'EFI/boot/grubx64.efi', 'EFI/boot/mmx64.efi'):
            boot.append(boot_entry('stick', pe_facts.facts(iso_extract(iso, inner, tmp))))
        live_kernel = pe_facts.facts(iso_extract(iso, 'casper/vmlinuz', tmp))
        boot.append(boot_entry('stick kernel', live_kernel))
        # the installed system's boot files come from packages in the ISO's own pool
        for prefix, wanted in (('shim-signed_', 'usr/lib/shim/shimx64.efi.signed.latest'),
                               ('grub-efi-amd64-signed_', 'usr/lib/grub/x86_64-efi-signed/grubx64.efi.signed'),
                               ('linux-image-', 'boot/vmlinuz-*')):
            debs = [n for n in pool if os.path.basename(n).startswith(prefix)]
            if len(debs) != 1:
                raise SystemExit('measure-release: the pool has %d packages named %s*; refusing to guess' % (len(debs), prefix))
            root = deb_unpack(iso_extract(iso, debs[0], tmp), os.path.join(tmp, 'deb-' + prefix))
            got = glob.glob(os.path.join(root, wanted))
            if len(got) != 1:
                raise SystemExit('measure-release: %s holds %d files matching %s; refusing to guess' % (debs[0], len(got), wanted))
            e = boot_entry('installed', pe_facts.facts(got[0])); e['Package'] = os.path.basename(debs[0])
            boot.append(e)
        sources = yaml.safe_load(open(iso_extract(iso, 'casper/install-sources.yaml', tmp)))
        for n in sorted(files):
            if n.startswith('casper/') and n.endswith('.squashfs'):
                h = hashlib.sha256()
                p = subprocess.Popen(['bsdtar', '-x', '-O', '-f', iso, n], stdout=subprocess.PIPE)
                for b in iter(lambda: p.stdout.read(1 << 20), b''): h.update(b)
                if p.wait() != 0: raise SystemExit('measure-release: could not read %s' % n)
                layers.append({'Path': n, 'Size': files[n], 'Sha256': h.hexdigest(), 'OverFat32': files[n] > FAT32_MAX})
        # the base layer: the kernel already inside it, and the snaps seeded in it
        base = iso_extract(iso, 'casper/' + next(s['path'] for s in sources['sources'] if s.get('default')), tmp)
        root = os.path.join(tmp, 'base')
        subprocess.run(['unsquashfs', '-q', '-n', '-d', root, base, 'boot', 'var/lib/snapd/seed'], check=True)
        subprocess.run(['chmod', '-R', 'u+rwX', root], check=False)
        kernels = [k for k in glob.glob(os.path.join(root, 'boot/vmlinuz-*')) if not os.path.islink(k)]
        base_kernels = [boot_entry('base layer kernel', pe_facts.facts(k)) for k in sorted(kernels)]
        seed = os.path.join(root, 'var/lib/snapd/seed/snaps')
        snaps = sorted(os.listdir(seed)) if os.path.isdir(seed) else []
    doc = {'_about': {'what': 'one casper ISO, measured from its own files by data/tools/measure-release.py --casper; never typed or edited by hand',
                      'offered': 'no: this is not data/releases.json, nothing reads it into a program, and no person is offered this release (RISKS R40)'},
           'Id': o.id, 'Name': o.name, 'Family': o.family, 'Installer': o.installer, 'Measured': datetime.date.today().isoformat(), 'Note': o.note,
           'Iso': {'Url': url, 'Sha256': sha256(iso), 'Size': os.path.getsize(iso), 'SplitFiles': iso_split_files(iso),
                   'LargestFile': max(files.values()), 'LargestFileName': max(files, key=files.get)},
           'Sources': [{'Id': s['id'], 'Default': bool(s.get('default')), 'Path': s['path'], 'Type': s.get('type', ''),
                        'Variations': {k: v.get('path', '') for k, v in (s.get('variations') or {}).items()}} for s in sources['sources']],
           'Kernel': sources.get('kernel', {}),
           'Layers': layers, 'Boot': boot + base_kernels,
           'InstalledKernelIsLiveKernel': any(b['Role'] == 'installed' and b['Sha256'] == live_kernel['sha256'] for b in boot),
           'SeededSnaps': snaps, 'Pool': {'Packages': len(pool), 'Bytes': sum(pool.values())}}
    os.makedirs(os.path.dirname(os.path.abspath(o.out)), exist_ok=True)
    with open(o.out, 'w', encoding='utf-8') as f:
        json.dump(doc, f, indent=2, ensure_ascii=False); f.write('\n')
    print('measure-release: wrote %s' % o.out, file=sys.stderr)

def main():
    a = argparse.ArgumentParser()
    a.add_argument('--id', required=True); a.add_argument('--name', required=True)
    a.add_argument('--family', required=True); a.add_argument('--installer', default='')
    a.add_argument('--netinst', nargs=2, metavar=('URL', 'ISO'))
    a.add_argument('--casper', nargs=2, metavar=('URL', 'ISO')); a.add_argument('--out', default='')
    a.add_argument('--desktop', nargs=4, action='append', metavar=('ID', 'LABEL', 'URL', 'ISO'), default=[])
    a.add_argument('--image-path', default='LiveOS/squashfs.img')
    a.add_argument('--table', default=str(ROOT / 'data/releases.json'))
    a.add_argument('--note', default='')
    o = a.parse_args()
    if o.casper: return casper(o)
    if not o.netinst: a.error('--netinst is required (or --casper for a casper ISO)')
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
