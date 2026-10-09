#!/usr/bin/env python3
"""Give WSL's page cache back to the host before a bench starts the VM
(rig/hyperv/README.md, "WSL's page cache starves Hyper-V"): posix_fadvise
DONTNEED over the build outputs, the registries, the kit and the rig's own
files. With --wait, then poll the host until it shows at least 9 GB free
(the starts that failed had 7; the ones that worked had 9 or more), up to
three minutes, printing what it sees. Extra arguments are more directories."""
import os, subprocess, sys, time
root = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..'))
home = os.path.expanduser('~')
wait = '--wait' in sys.argv
args = [a for a in sys.argv[1:] if a != '--wait']
dirs = [os.path.join(root, d) for d in ('evaluate/scan/target', 'evaluate/harvest/target', 'evaluate/job/target', 'upgrade_/prologue/target', 'upgrade_/kickstart/target', 'schemas/rust/target', 'upgrade_/windows/window/target', 'settle-in/target', 'settle-in/window/target', 'settle-in/gate/target', 'dist/kit', 'rig/hyperv/artifacts')] + [os.path.join(home, '.cargo/registry'), os.path.join(home, '.rustup'), '/mnt/c/upgrade-rig/hv/vm'] + args
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
if wait:
    for _ in range(9):
        try:
            free = float(subprocess.run(['powershell.exe', '-NoProfile', '-Command', '[math]::Round((Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory/1MB,1)'], capture_output=True, text=True, timeout=60).stdout.strip())
        except Exception:
            free = -1.0
        print(f'evict-builds: host free {free} GB')
        if free >= 9:
            break
        time.sleep(20)
