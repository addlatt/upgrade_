#!/usr/bin/env python3
"""V1b / R21 offline inspector: read the rig guest's disk image WITHOUT booting
it and record what the alongside install is not allowed to break.

    v1b-inspect.py <image.qcow2|image.vhdx> <label> [outdir]

Writes <outdir>/<label>.json with:
  - the GPT (every partition: index, type GUID + name, start/end LBA, size)
  - the ESP: size, bytes free (per FAT), a manifest of every file with sha256
  - the sha256 of EFI/Microsoft/Boot/bootmgfw.efi pulled out for the CSV

Reads via `qemu-img dd` (no loop/nbd devices - the WSL2 kernel has none) and
lists the FAT volume with mtools. Pure read-only: nothing here writes to the
image. Run it before the shrink, after the shrink, after the install, and
after every boot cycle; `v1b.sh verdict` diffs the JSONs.
"""
import hashlib, json, os, struct, subprocess, sys, tempfile, uuid, datetime

GPT_TYPES = {
    'c12a7328-f81f-11d2-ba4b-00a0c93ec93b': 'EFI System',
    'e3c9e316-0b5c-4db8-817d-f92df00215ae': 'Microsoft reserved',
    'ebd0a0a2-b9e5-4433-87c0-68b6b72699c7': 'Microsoft basic data',
    'de94bba4-06d1-4d40-a16a-bfd50179d6ac': 'Windows recovery',
    '0fc63daf-8483-4772-8e79-3d69d8477de4': 'Linux filesystem',
    'bc13c2ff-59e6-4262-a352-b275fd6f7172': 'Linux extended boot',
    '0657fd6d-a4ab-43c4-84e5-0933c84b4f4f': 'Linux swap',
    '4f68bce3-e8cd-4db1-96e7-fbcaf984b709': 'Linux root (x86-64)',
}

def img_format(img):
    # the QEMU rig reads qcow2; the Hyper-V rig reads VHDX with the same code path
    return {'qcow2': 'qcow2', 'vhdx': 'vhdx'}.get(img.rsplit('.', 1)[-1].lower(), 'raw')

def qdd(img, out, skip_bytes, count_bytes, bs):
    assert skip_bytes % bs == 0 and count_bytes % bs == 0, (skip_bytes, count_bytes, bs)
    subprocess.run(['qemu-img', 'dd', '-f', img_format(img), '-O', 'raw', f'bs={bs}',
                    f'skip={skip_bytes // bs}', f'count={count_bytes // bs}',
                    f'if={img}', f'of={out}'], check=True)

def read_gpt(img, tmp):
    head = os.path.join(tmp, 'head.raw')
    qdd(img, head, 0, 1 << 20, 1 << 20)          # first MiB: MBR + GPT header + entries
    with open(head, 'rb') as f:
        data = f.read()
    hdr = data[512:1024]
    assert hdr[:8] == b'EFI PART', 'no GPT header at LBA1'
    (_, _, _, _, _, cur, bak, first, last, dguid, parts_lba, nparts, psize, _) = \
        struct.unpack('<8sIIIIQQQQ16sQIII', hdr[:92])
    parts = []
    base = parts_lba * 512
    for i in range(nparts):
        e = data[base + i * psize: base + (i + 1) * psize]
        tguid = uuid.UUID(bytes_le=e[:16])
        if tguid.int == 0:
            continue
        pguid = uuid.UUID(bytes_le=e[16:32])
        start, end, attrs = struct.unpack('<QQQ', e[32:56])
        name = e[56:128].decode('utf-16-le').rstrip('\0')
        parts.append({
            'index': i + 1, 'type_guid': str(tguid),
            'type': GPT_TYPES.get(str(tguid), 'unknown'),
            'part_guid': str(pguid), 'name': name,
            'start_lba': start, 'end_lba': end,
            'size_bytes': (end - start + 1) * 512,
            'size_mib': round((end - start + 1) * 512 / 2**20, 1),
        })
    return {'disk_guid': str(uuid.UUID(bytes_le=dguid)), 'first_usable_lba': first,
            'last_usable_lba': last, 'partitions': parts}

def walk_fat(esp, path='::/'):
    """Recursive listing of a FAT image via mdir; returns [(path, size)]."""
    out = subprocess.run(['mdir', '-i', esp, '-a', path], check=True,
                         capture_output=True, text=True).stdout
    files, dirs = [], []
    for line in out.splitlines():
        toks = line.split()
        if len(toks) < 4:
            continue
        if line.startswith(' Volume') or line.startswith('Directory') or 'bytes free' in line:
            continue
        idx = None
        for j, t in enumerate(toks):
            if len(t) == 10 and t[4] == '-' and t[7] == '-':
                idx = j; break
        if idx is None or idx < 1:
            continue
        kind = toks[idx - 1]
        longname = ' '.join(toks[idx + 2:]) if len(toks) > idx + 2 else None
        # tokens before the size column are the 8.3 name (1 or 2 tokens)
        short = toks[0] if idx <= 2 else toks[0] + '.' + toks[1]
        name = longname or short
        if name in ('.', '..'):
            continue
        full = path.rstrip('/') + '/' + name
        if kind == '<DIR>':
            dirs.append(full)
        else:
            files.append((full, int(kind)))
    for d in dirs:
        files += walk_fat(esp, d)
    return files

def fat_free(esp):
    out = subprocess.run(['mdir', '-i', esp, '::/'], check=True, capture_output=True, text=True).stdout
    for line in out.splitlines():
        if 'bytes free' in line:
            return int(line.split('bytes free')[0].replace(' ', '').replace(',', ''))
    return None

def evict_page_cache(path):
    # 2026-08-30, Hyper-V leg: WSL's 9p page cache served HOURS-stale pages of
    # a VHDX that Windows (VMMS) had rewritten - an offline inspection read a
    # pre-servicing bootmgfw.efi out of a disk whose live content was newer,
    # and a cp of the image baked the stale pages into the backup. Every read
    # of a Windows-written file through /mnt/c must evict the cache first.
    # Harmless on native-WSL files (QEMU rig qcow2).
    fd = os.open(path, os.O_RDONLY)
    try:
        os.posix_fadvise(fd, 0, 0, os.POSIX_FADV_DONTNEED)
    finally:
        os.close(fd)

def main():
    img, label = sys.argv[1], sys.argv[2]
    evict_page_cache(img)
    outdir = sys.argv[3] if len(sys.argv) > 3 else os.path.join(os.path.dirname(img), 'v1b')
    os.makedirs(outdir, exist_ok=True)
    rec = {'label': label, 'image': os.path.abspath(img),
           'timestamp': datetime.datetime.now(datetime.timezone.utc).isoformat()}
    with tempfile.TemporaryDirectory(dir=outdir) as tmp:
        gpt = read_gpt(img, tmp)
        rec['gpt'] = gpt
        esp = next((p for p in gpt['partitions'] if p['type'] == 'EFI System'), None)
        if esp is None:
            rec['esp'] = None
        else:
            espimg = os.path.join(tmp, 'esp.raw')
            qdd(img, espimg, esp['start_lba'] * 512, esp['size_bytes'], 512 if esp['start_lba'] % 2048 else 1 << 20)
            files = walk_fat(espimg)
            manifest = {}
            for path, size in sorted(files):
                dst = os.path.join(tmp, 'f')
                subprocess.run(['mcopy', '-n', '-o', '-i', espimg, path, dst], check=True,
                               capture_output=True)
                h = hashlib.sha256(open(dst, 'rb').read()).hexdigest()
                manifest[path[2:]] = {'size': size, 'sha256': h}
            used = sum(v['size'] for v in manifest.values())
            rec['esp'] = {
                'partition_index': esp['index'], 'size_bytes': esp['size_bytes'],
                'size_mib': esp['size_mib'], 'fat_bytes_free': fat_free(espimg),
                'file_bytes_used': used, 'file_count': len(manifest),
                'bootmgfw_sha256': manifest.get('/EFI/Microsoft/Boot/bootmgfw.efi', {}).get('sha256'),
                'manifest': manifest,
            }
    out = os.path.join(outdir, f'{label}.json')
    with open(out, 'w') as f:
        json.dump(rec, f, indent=1)
    print(f'v1b-inspect: wrote {out}')
    for p in gpt['partitions']:
        print(f"  p{p['index']} {p['type']:<22} {p['size_mib']:>10} MiB  LBA {p['start_lba']}-{p['end_lba']}  {p['name']}")
    if rec['esp']:
        e = rec['esp']
        print(f"  ESP: {e['size_mib']} MiB, {e['file_count']} files, {e['file_bytes_used']} B used, {e['fat_bytes_free']} B free")
        print(f"  bootmgfw.efi sha256: {e['bootmgfw_sha256']}")

if __name__ == '__main__':
    main()
