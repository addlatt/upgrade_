#!/usr/bin/env python3
"""
ubuntu-parts.py - one line per partition of a raw disk image: its number,
GPT type, first byte, size in bytes, and the sha256 of every byte in it
(RISKS R21, R40). The beside-Windows arm runs it before and after, so
"Windows' partitions were not touched" is a comparison of hashes, not a
reading of the installer's log.

    ubuntu-parts.py DISK.raw
"""
import hashlib, struct, sys

f = open(sys.argv[1], "rb")
f.seek(512); h = f.read(92)
if h[:8] != b"EFI PART": sys.exit("ubuntu-parts: no GPT")
lba, n, sz = struct.unpack_from("<Q", h, 72)[0], struct.unpack_from("<I", h, 80)[0], struct.unpack_from("<I", h, 84)[0]
f.seek(lba * 512); table = f.read(n * sz)
for i in range(n):
    e = table[i * sz:(i + 1) * sz]
    if e[:16] == bytes(16): continue
    a = struct.unpack_from("<IHH", e, 0)
    t = "%08x-%04x-%04x-%s-%s" % (a[0], a[1], a[2], e[8:10].hex(), e[10:16].hex())
    first, last = struct.unpack_from("<QQ", e, 32)
    left, d = (last - first + 1) * 512, hashlib.sha256()
    f.seek(first * 512)
    while left:
        b = f.read(min(left, 8 << 20))
        if not b: sys.exit("ubuntu-parts: partition %d runs past the end of the image" % (i + 1))
        d.update(b); left -= len(b)
    print("p%d %s %d %d %s" % (i + 1, t, first * 512, (last - first + 1) * 512, d.hexdigest()))
