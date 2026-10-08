#!/usr/bin/env python3
"""Give WSL's page cache back to the host before a bench starts the VM
(rig/hyperv/README.md, "WSL's page cache starves Hyper-V"): posix_fadvise
DONTNEED over the build outputs, the registries and the kit."""
import os, sys
root = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..'))
home = os.path.expanduser('~')
dirs = [os.path.join(root, d) for d in ('evaluate/scan/target', 'evaluate/harvest/target', 'evaluate/job/target', 'upgrade_/prologue/target', 'upgrade_/kickstart/target', 'schemas/rust/target', 'upgrade_/windows/window/target', 'settle-in/target', 'settle-in/window/target', 'settle-in/gate/target', 'dist/kit', 'rig/hyperv/artifacts')] + [os.path.join(home, '.cargo/registry'), os.path.join(home, '.rustup')] + sys.argv[1:]
n = b = 0
for top in dirs:
    for d, _, fs in os.walk(top):
        for f in fs:
            p = os.path.join(d, f)
            try:
                fd = os.open(p, os.O_RDONLY); b += os.fstat(fd).st_size; os.posix_fadvise(fd, 0, 0, os.POSIX_FADV_DONTNEED); os.close(fd); n += 1
            except OSError:
                pass
print(f'evict-builds: {n} files, {b / 1e9:.1f} GB advised out of the cache')
