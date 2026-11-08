#!/usr/bin/env python3
"""The reader's test disk, host side (settle-in/src/ntfsread.rs; RISKS R43).

guest/ntfs-fixture.ps1 left a fixed VHD (the disk's bytes, then a 512-byte
footer) and Windows' own list of its entries on OEMDRV:\\v3\\fixture\\. This
cuts the NTFS volume out of the VHD (the first MBR partition) and stores it
as its non-zero 4 KiB blocks only, so the test disk is small in git:

    b"UPGNTFS1", u64 LE volume size, then for each non-zero block:
    u32 LE block number, 4096 bytes

    ntfs-fixture.py VHD FIXTURE_JSON OUT_DIR
"""
import json, shutil, struct, sys

vhd, fixture, out = sys.argv[1:4]
d = open(vhd, "rb").read()
assert d[-512:-504] == b"conectix", "not a fixed VHD (no footer)"
d = d[:-512]
assert d[510:512] == b"\x55\xaa", "no MBR"
lba, sectors = struct.unpack_from("<II", d, 446 + 8)
vol = d[lba * 512:(lba + sectors) * 512]
assert vol[3:11] == b"NTFS    ", "the first partition is not NTFS"
B = 4096
with open(f"{out}/volume.blocks", "wb") as f:
    f.write(b"UPGNTFS1" + struct.pack("<Q", len(vol)))
    n = 0
    for i in range(0, len(vol), B):
        blk = vol[i:i + B].ljust(B, b"\0")
        if blk.count(0) != B:
            f.write(struct.pack("<I", i // B) + blk)
            n += 1
shutil.copyfile(fixture, f"{out}/fixture.json")
print(f"volume {len(vol)} bytes at LBA {lba}; {n} non-zero blocks ({n * B} bytes) -> {out}/volume.blocks")
