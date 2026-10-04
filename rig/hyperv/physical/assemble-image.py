#!/usr/bin/env python3
"""
assemble-image.py - a disk image the offline inspector can read, from what a
real machine gave: the drive's first MiB and its EFI System Partition.

    assemble-image.py <first-mib.raw> <esp.raw> <out.raw>

The image is sparse: the first MiB at byte 0, the ESP at the offset the GPT in
that first MiB names for it, zeros everywhere else. rig/vm/v1b-inspect.py reads
exactly those two regions (the GPT's primary copy and the ESP), so it runs
unchanged:  v1b-inspect.py <out.raw> <label> <outdir>

Refuses when the GPT names no ESP, or when esp.raw is not the length the GPT
gives for it: a wrong offset would make an inspection of the wrong bytes.
"""
import struct, sys, uuid
first, esp, out = sys.argv[1:4]
ESP = uuid.UUID('c12a7328-f81f-11d2-ba4b-00a0c93ec93b')
head = open(first, 'rb').read()
assert len(head) == 1 << 20, 'the first MiB must be exactly 1 MiB'
hdr = head[512:1024]
assert hdr[:8] == b'EFI PART', 'no GPT header in the first MiB'
(_, _, _, _, _, _, _, _, last, _, parts_lba, nparts, psize, _) = struct.unpack('<8sIIIIQQQQ16sQIII', hdr[:92])
found = None
for i in range(nparts):
    e = head[parts_lba * 512 + i * psize: parts_lba * 512 + (i + 1) * psize]
    if uuid.UUID(bytes_le=e[:16]) == ESP:
        start, end = struct.unpack('<QQ', e[32:48]); found = (start, end); break
assert found, 'the GPT names no EFI System Partition'
start, end = found; want = (end - start + 1) * 512
import os
have = os.path.getsize(esp)
assert have == want, 'esp.raw is %d bytes; the GPT says the ESP is %d' % (have, want)
with open(out, 'wb') as o:
    o.write(head)
    o.seek(start * 512)
    with open(esp, 'rb') as f:
        while True:
            b = f.read(1 << 20)
            if not b: break
            o.write(b)
    o.truncate((last + 34) * 512)      # the drive's own length, sparse
print('assemble-image: ESP %d MiB at sector %d -> %s' % (want >> 20, start, out))
