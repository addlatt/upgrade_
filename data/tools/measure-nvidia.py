#!/usr/bin/env python3
"""
measure-nvidia.py - write data/devices/nvidia.json from the NVIDIA driver
packages an Ubuntu ISO carries (RISKS R47). The facts in the table are
measured here, never typed by hand (rule #2).

    measure-nvidia.py --release ubuntu-26.04.1 ISO [--table data/devices/nvidia.json]

Every `nvidia-driver-NNN` package on the ISO lists, in its own `Modaliases`
field, the cards it drives. That list is what Ubuntu's own tool
(`ubuntu-drivers`, which the installer runs when drivers are switched on)
matches a card against, so the table says what the installer will find on
the stick for a card. Per package: its file name, its sha256, its version,
and the PCI device ids it lists. And whether the ISO carries the kernel
module for it already built and signed for the kernel the release installs
(`linux-modules-nvidia-NNN-<kernel>`), which is what lets it load under
Secure Boot with no key step.

The ISO must be the one data/releases.json pins for that release: its
sha256 is checked against the entry before anything is read.
"""
import argparse, datetime, hashlib, json, os, pathlib, re, subprocess, sys, tempfile

ROOT = pathlib.Path(__file__).resolve().parents[2]


def sha256(path):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for block in iter(lambda: f.read(1 << 20), b''):
            h.update(block)
    return h.hexdigest()


def control(deb, tmp):
    """The control fields of a .deb (ar, then a tar in whatever compression), read with bsdtar."""
    d = tempfile.mkdtemp(dir=tmp)
    subprocess.run(['bsdtar', '-x', '-f', deb, '-C', d], check=True)
    tar = [n for n in os.listdir(d) if n.startswith('control.tar')]
    if len(tar) != 1:
        sys.exit(f'{deb}: no control archive')
    text = subprocess.run(['bsdtar', '-x', '-O', '-f', os.path.join(d, tar[0]), './control'], check=True, capture_output=True, text=True).stdout
    fields, key = {}, None
    for line in text.splitlines():
        if line[:1] in (' ', '\t') and key:
            fields[key] += ' ' + line.strip()
        elif ':' in line:
            key, value = line.split(':', 1)
            fields[key] = value.strip()
    return fields


def main():
    p = argparse.ArgumentParser()
    p.add_argument('--release', required=True)
    p.add_argument('iso')
    p.add_argument('--table', default=str(ROOT / 'data/devices/nvidia.json'))
    p.add_argument('--releases', default=str(ROOT / 'data/releases.json'))
    a = p.parse_args()

    rel = [r for r in json.load(open(a.releases))['Releases'] if r['Id'] == a.release]
    if not rel or 'Iso' not in rel[0]:
        sys.exit(f'{a.release} is not a release with one ISO in {a.releases}')
    rel = rel[0]
    have = sha256(a.iso)
    if have != rel['Iso']['Sha256']:
        sys.exit(f'{a.iso} is not the ISO {a.release} pins: sha256 {have}')
    kernels = sorted({k['Release'] for k in rel['Kernels'] if k['Role'].startswith('installed')})
    if len(kernels) != 1:
        sys.exit(f'{a.release} installs {len(kernels)} kernels; expected one')
    kernel = kernels[0]

    names = subprocess.run(['bsdtar', '-t', '-f', a.iso], check=True, capture_output=True, text=True).stdout.splitlines()
    drivers = sorted(n for n in names if re.fullmatch(r'pool/.*/nvidia-driver-\d+_[^/]*\.deb', n))
    if not drivers:
        sys.exit('no nvidia-driver package on this ISO')
    branches = []
    with tempfile.TemporaryDirectory() as tmp:
        subprocess.run(['bsdtar', '-x', '-f', a.iso, '-C', tmp] + drivers, check=True)
        for n in drivers:
            deb = os.path.join(tmp, n)
            f = control(deb, tmp)
            branch = re.fullmatch(r'nvidia-driver-(\d+)', f['Package']).group(1)
            ids = sorted({m.lower() for m in re.findall(r'pci:v000010DEd0000([0-9A-F]{4})sv', f.get('Modaliases', ''))})
            if not ids:
                sys.exit(f'{n}: no NVIDIA ids in its Modaliases')
            built = [x for x in names if re.fullmatch(rf'pool/.*/linux-modules-nvidia-{branch}-{re.escape(kernel)}_[^/]*\.deb', x)]
            branches.append({
                'Branch': branch,
                'Version': f['Version'],
                'Package': os.path.basename(n),
                'PackageSha256': sha256(deb),
                'SignedModuleFor': kernel if built else '',
                'Ids': ' '.join(ids),
            })
    branches.sort(key=lambda b: int(b['Branch']))

    about = {
        'table': 'which NVIDIA driver a card gets: per driver branch, the PCI device ids (vendor 10de) its package lists',
        'read': 'data/README.md',
        'rule': 'Measured from the driver packages on the release\'s own ISO by data/tools/measure-nvidia.py and pinned by sha256. Never type or edit one by hand: run the tool. A card gets the newest branch that lists it; a card no branch lists gets no NVIDIA driver from the stick.',
    }
    lines = ['{', '  "_about": ' + json.dumps(about) + ',',
             '  "Measured": ' + json.dumps({'Release': a.release, 'IsoSha256': have, 'Date': datetime.date.today().isoformat()}) + ',',
             '  "Branches": [']
    lines += ['    ' + json.dumps(b) + (',' if i < len(branches) - 1 else '') for i, b in enumerate(branches)]
    lines += ['  ]', '}']
    pathlib.Path(a.table).write_text('\n'.join(lines) + '\n')
    for b in branches:
        print(f"  {b['Branch']}: {b['Version']}, {len(b['Ids'].split())} cards, signed module for {b['SignedModuleFor'] or 'no installed kernel'}")
    print(f'  wrote {a.table}')


if __name__ == '__main__':
    main()
