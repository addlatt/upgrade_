#!/usr/bin/env python3
"""
v11w-root.py - a made-up Linux root for `settle-in go-back walkaway prepare --root`
on the rig (VALIDATION V11, RISKS R33). Nothing here is product code.

    v11w-root.py <out dir> <disk identifier GUID> <size bytes> <ssh public key file>

It describes a Fedora that owns ONE internal disk: the rig's walk-away target
VHDX. Hyper-V shows a VHDX to its guest with the SCSI world-wide name
60022480 + the DiskIdentifier's first two groups byte-swapped + its last 12
hex digits (read 2026-10-17 off two rig disks: fresh.vhdx 75A89D2F-B2E3-...-
3683792B2346 is 600224802F9DA875E3B23683792B2346 in the guest), so the job the
prepare writes names the disk the gate will find. Also: TPM 2.0 and UEFI (the
offer is Windows 11), Windows 11 Home recorded before, two spoofed saved
networks (one carried, one enterprise network not carried), New York time with
the hardware clock in UTC, SSH on with one key for the account rig.
A spoof closes plumbing only (rule #5).
"""
import os, sys, uuid, pathlib

out, guid, size, keyfile = pathlib.Path(sys.argv[1]), sys.argv[2], int(sys.argv[3]), sys.argv[4]
g = uuid.UUID(guid)
f = g.hex  # 32 hex digits
wwn = "60022480" + bytes.fromhex(f[0:8])[::-1].hex() + bytes.fromhex(f[8:12])[::-1].hex() + f[20:32]
def w(p, text, mode=None):
    p = out / p; p.parent.mkdir(parents=True, exist_ok=True); p.write_text(text)
    if mode: os.chmod(p, mode)
def ln(p, target):
    p = out / p; p.parent.mkdir(parents=True, exist_ok=True)
    if p.is_symlink() or p.exists(): p.unlink()
    os.symlink(target, p)
dev = "sys/devices/pci0000:00/0000:00:00.0/host0/target0:0:0/0:0:0:0"
w(f"{dev}/model", "Virtual Disk\n"); w(f"{dev}/vendor", "Msft\n"); w(f"{dev}/wwid", f"naa.{wwn}\n")
blk = f"{dev}/block/sda"
w(f"{blk}/dev", "8:0\n"); w(f"{blk}/size", f"{size // 512}\n"); w(f"{blk}/removable", "0\n"); w(f"{blk}/ro", "0\n"); w(f"{blk}/queue/rotational", "0\n")
ln(f"{blk}/device", "../../../0:0:0:0")
for n, d in (("1", "8:1"), ("2", "8:2")):
    w(f"{blk}/sda{n}/partition", n + "\n"); w(f"{blk}/sda{n}/dev", d + "\n")
ln("sys/block/sda", "../" + blk.split("sys/", 1)[1])
ln("sys/dev/block/8:0", "../../" + blk.split("sys/", 1)[1])
ln("sys/dev/block/8:2", "../../" + blk.split("sys/", 1)[1] + "/sda2")
ln("sys/dev/block/8:1", "../../" + blk.split("sys/", 1)[1] + "/sda1")
w("run/udev/data/b8:0", f"E:ID_BUS=scsi\nE:ID_MODEL=Virtual_Disk\nE:ID_WWN=0x{wwn}\n")
w("run/udev/data/b8:1", "E:ID_FS_TYPE=vfat\n"); w("run/udev/data/b8:2", "E:ID_FS_TYPE=ext4\n")
w("proc/self/mountinfo", "25 1 8:2 / / rw,relatime shared:1 - ext4 /dev/sda2 rw\n26 25 8:1 / /boot/efi rw,relatime shared:2 - vfat /dev/sda1 rw\n")
w("proc/swaps", "Filename\t\t\t\tType\t\tSize\t\tUsed\t\tPriority\n")
w("proc/meminfo", "MemTotal:        4029440 kB\n"); w("proc/cpuinfo", "model name\t: AMD Ryzen AI 9 HX 370 (rig spoof)\n")
(out / "sys/firmware/efi").mkdir(parents=True, exist_ok=True); w("sys/class/tpm/tpm0/tpm_version_major", "2\n")
w("var/lib/upgrade_-settle-in/summary.json", '{"windows_before": {"result": "read", "windows_version": "11", "edition_id": "Core", "edition": "Windows 11 Home", "note": "rig spoof"}}\n')
w("etc/NetworkManager/system-connections/RigSpoof Home.nmconnection",
  "[connection]\nid=RigSpoof Home\ntype=wifi\nautoconnect=true\n\n[wifi]\nmode=infrastructure\nssid=RigSpoof Home\n\n[wifi-security]\nkey-mgmt=wpa-psk\npsk=rig spoof pass 1\n", 0o600)
w("etc/NetworkManager/system-connections/RigSpoof Work.nmconnection",
  "[connection]\nid=RigSpoof Work\ntype=wifi\n\n[wifi]\nmode=infrastructure\nssid=RigSpoof Work\n\n[wifi-security]\nkey-mgmt=wpa-eap\n\n[802-1x]\neap=peap;\nidentity=someone\n", 0o600)
ln("etc/localtime", "../usr/share/zoneinfo/America/New_York")
w("etc/adjtime", "0.0 0 0.0\n0\nUTC\n")
w("etc/passwd", "root:x:0:0:root:/root:/bin/bash\nrig:x:1000:1000:rig:/home/rig:/bin/bash\n")
w("home/rig/.ssh/authorized_keys", pathlib.Path(keyfile).read_text().strip() + "\n", 0o600)
ln("etc/systemd/system/multi-user.target.wants/sshd.service", "/usr/lib/systemd/system/sshd.service")
print(f"v11w-root: {out} disk sda {size} B wwn 0x{wwn}")
