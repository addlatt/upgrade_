#!/usr/bin/env python3
"""Writes tests/cases.json: the made-up machines, sticks and installed
systems the installer's verify and outcome steps are run against in the
sandbox (tests/sandbox.py). Every name, id and number is invented; the jobs
start from schemas/examples. The golden was recorded from verify.sh and
outcome.sh once (tests/golden.json, frozen); a new case gets its expected
result from the Rust's own run, reviewed as a diff."""
import copy, json, pathlib

ROOT = pathlib.Path(__file__).resolve().parents[4]
EX = ROOT / "schemas/examples"
ESP = "c12a7328-f81f-11d2-ba4b-00a0c93ec93b"
MSR = "e3c9e316-0b5c-4db8-817d-f92df00215ae"
MSDATA = "ebd0a0a2-b9e5-4433-87c0-68b6b72699c7"
LINUXFS = "0fc63daf-8483-4772-8e79-3d69d8477de4"

KEEP = json.load(open(EX / "job.keep-windows.json"))
CLEAN = json.load(open(EX / "job.clean-slate.json"))
ERASE = json.load(open(EX / "job.erase-and-install.json"))

BOOTMGFW = "MZ-bootmgfw-windows-loader-bytes-0001"
BOOTX64_WIN = "MZ-bootx64-windows-fallback-bytes-0002"
SHIM = "MZ-shimx64-fedora-bytes-0003"
GRUB = "MZ-grubx64-fedora-bytes-0004"

ESP_FILES = {"EFI/Microsoft/Boot/bootmgfw.efi": BOOTMGFW, "EFI/Microsoft/Boot/BCD": "BCD-store-bytes", "EFI/Boot/bootx64.efi": BOOTX64_WIN}


def disk_sda(size=250059350016, esp=ESP_FILES, byid=("nvme-eui.0025385a91b12345", "nvme-Samsung_SSD_860_EVO_250GB_S3YANB0K123456")):
    return {"dev": "sda", "byid": list(byid), "size": size, "model": "Samsung SSD 860 EVO 250GB", "serial": "S3YANB0K123456",
            "parts": [{"n": 1, "parttype": ESP, "parttypename": "EFI System", "size": 104857600, "fstype": "vfat", "partuuid": "11111111-0001-4000-8000-000000000001", "esp": esp},
                      {"n": 2, "parttype": MSR, "parttypename": "Microsoft reserved", "size": 16777216, "partuuid": "11111111-0002-4000-8000-000000000002"},
                      {"n": 3, "parttype": MSDATA, "parttypename": "Microsoft basic data", "size": 180000000000, "fstype": "ntfs", "partuuid": "11111111-0003-4000-8000-000000000003"},
                      {"n": 4, "parttype": MSDATA, "parttypename": "Microsoft basic data", "size": 1073741824, "fstype": "ntfs", "partuuid": "11111111-0004-4000-8000-000000000004"},
                      {"n": 5, "parttype": LINUXFS, "parttypename": "Linux filesystem", "size": 68000000000, "fstype": "ext4", "partuuid": "11111111-0005-4000-8000-000000000005"}]}


def disk_vm(size=137438953472, byid=("wwn-0x60022480000000000000000000000001", "scsi-360022480000000000000000000000001"), dev="sdb"):
    return {"dev": dev, "byid": list(byid), "size": size, "model": "Msft Virtual Disk",
            "parts": [{"n": 1, "parttype": ESP, "parttypename": "EFI System", "size": 104857600, "fstype": "vfat", "partuuid": "22222222-0001-4000-8000-000000000001", "esp": ESP_FILES},
                      {"n": 2, "parttype": MSDATA, "parttypename": "Microsoft basic data", "size": 137000000000, "fstype": "ntfs", "partuuid": "22222222-0002-4000-8000-000000000002"}]}


def home_disk(dev="sdc", size=68719476736, byid=("wwn-0x60022480000000000000000000000002",)):
    return {"dev": dev, "byid": list(byid), "size": size, "model": "Msft Virtual Disk", "parts": []}


STICK_KEEP = {"files": {"upgrade_/LiveOS/kde.squashfs": "squashfs-image-bytes-kde-" + "x" * 500, "upgrade_/LiveOS/gnome.squashfs": "squashfs-image-bytes-gnome-" + "y" * 500, "KIT-MANIFEST.txt": "upgrade_ kit\ncommit 0000000\nrelease fedora-44\n"}}
DRM = [{"name": "card0-eDP-1", "status": "connected", "modes": ["1920x1080", "1280x720"]}, {"name": "card0-HDMI-A-1", "status": "disconnected", "modes": []}]
NM = {"devices": [["wlp2s0", "wifi"], ["enp1s0", "ethernet"]], "ssids": ["Home Net", "Neighbour", "CoffeeShop"]}


def vjob(base, **over):
    j = copy.deepcopy(base)
    for k, v in over.items():
        cur = j
        parts = k.split(".")
        for p in parts[:-1]:
            cur = cur[p]
        cur[parts[-1]] = v
    return j


def verify(name, job, disks, mode="install", **kw):
    c = {"name": name, "mode": "verify", "job": job, "cmdline": f"BOOT_IMAGE=/images/pxeboot/vmlinuz{' upg.mode=verify' if mode == 'verify' else ''} inst.ks=hd:LABEL=UPGV0:/ks.cfg quiet",
         "stick": copy.deepcopy(STICK_KEEP), "disks": disks, "drm": DRM, "graphics": ["fb0"], "nmcli": NM, "secureboot": 1, "chvt_ok": True, "keys": [], "countdown_secs": 2, "refusal_secs": 1, "tty": True}
    for k, v in kw.items():
        if k == "stick":
            c["stick"].update(v)
        else:
            c[k] = v
    return c


ERASE_JOB = vjob(ERASE, **{"identity.system_disk.unique_id": "60022480000000000000000000000001", "identity.system_disk.serial_number": ""})
ERASE_JOB["erase_consent"]["disks"][0]["unique_id"] = "60022480000000000000000000000001"
ERASE_JOB["erase_consent"]["disks"][0]["size_bytes"] = 137438953472
if len(ERASE_JOB["erase_consent"]["disks"]) < 2:
    ERASE_JOB["erase_consent"]["disks"].append({"role": "home", "serial_number": "", "unique_id": "60022480000000000000000000000002", "size_bytes": 68719476736, "friendly_name": "Msft Virtual Disk", "health_status": "Healthy"})
else:
    ERASE_JOB["erase_consent"]["disks"][1].update({"unique_id": "60022480000000000000000000000002", "size_bytes": 68719476736, "serial_number": ""})
ERASE_JOB["intent"]["desktop"] = "gnome"
ERASE_ONE = copy.deepcopy(ERASE_JOB)
ERASE_ONE["erase_consent"]["disks"] = ERASE_JOB["erase_consent"]["disks"][:1]
CLEAN_JOB = vjob(CLEAN, **{"identity.system_disk.unique_id": "eui.0025385A91B12345", "identity.system_disk.serial_number": "S3YANB0K123456", "identity.system_disk.size_bytes": 250059350016})
ATA_JOB = vjob(KEEP, **{"identity.system_disk.unique_id": "ATA     HFS256G39TND-N210A   TF123456", "identity.system_disk.serial_number": "TF123456"})
ATA_DISK = disk_sda(byid=("ata-HFS256G39TND-N210A_TF123456",))
FW_JOB = vjob(KEEP, **{"harvest.firmware_artifacts": [{"path": "artifacts/firmware/cs35l41/cs35l41-dsp1-spk-prot.wmfw", "sha256": "0" * 64, "size_bytes": 10}, {"path": "artifacts/firmware/cs35l41/cs35l41-dsp1-spk-prot.bin", "sha256": "0" * 64, "size_bytes": 10}]})
REFUSAL_KEY = [{"after": 0.6, "data": "\n"}]

cases = [
    verify("keep-windows, verify mode: identity, hardware, image and snapshot pass, the report, back to Windows", KEEP, [disk_sda()], mode="verify"),
    verify("keep-windows, install mode: every gate passes, the stick is disarmed", KEEP, [disk_sda()], stick={"markers": ["boot-install:v2"]}),
    verify("keep-windows, install: the stick already converted this computer (R35): refused 28 before any check", KEEP, [disk_sda()], stick={"markers": ["converted:verify 0.6.0 job x committed 2026-10-04T10:00:00Z"]}, keys=REFUSAL_KEY),
    verify("a job that is not job/1: refused 10", vjob(KEEP, schema="job/2"), [disk_sda()], keys=REFUSAL_KEY),
    verify("identity: the job's disk is not attached, verify mode reports fail and still returns to Windows", KEEP, [disk_vm()], mode="verify"),
    verify("identity: the job's disk is not attached, install mode: refused 20", KEEP, [disk_vm()], keys=REFUSAL_KEY),
    verify("identity: the disk is there but its size differs: refused 20", KEEP, [disk_sda(size=250059350017)], keys=REFUSAL_KEY),
    verify("identity: a padded ATA id is not hex; the serial finds the disk", ATA_JOB, [ATA_DISK], mode="verify"),
    verify("identity: a disk found by its serial only (the id is unknown to Linux)", vjob(KEEP, **{"identity.system_disk.unique_id": "{aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee}"}), [disk_sda()], mode="verify"),
    verify("identity: two disks, the second carries the id", KEEP, [disk_vm(), disk_sda()], mode="verify"),
    verify("image: the desktop image's sha256 does not match the stick's manifest (R17): refused 23", KEEP, [disk_sda()], stick={"wrong_sum_for": ["upgrade_/LiveOS/kde.squashfs"]}, keys=REFUSAL_KEY),
    verify("image: the desktop image is missing from the stick: refused 23", KEEP, [disk_sda()], stick={"files": {"upgrade_/LiveOS/gnome.squashfs": "only gnome", "KIT-MANIFEST.txt": "k"}}, keys=REFUSAL_KEY),
    verify("image: no SHA256SUMS on the stick: refused 23", KEEP, [disk_sda()], stick={"sums": False}, keys=REFUSAL_KEY),
    verify("image: the image is not in SHA256SUMS: refused 23", KEEP, [disk_sda()], stick={"not_in_sums": ["upgrade_/LiveOS/kde.squashfs"]}, keys=REFUSAL_KEY),
    verify("keep-windows: the ESP holds no Windows loader: no include, refused 21", KEEP, [disk_sda(esp={"EFI/Boot/bootx64.efi": BOOTX64_WIN})], keys=REFUSAL_KEY),
    verify("keep-windows: the ESP cannot be mounted: no include, refused 21", KEEP, [disk_sda(esp={})], keys=REFUSAL_KEY),
    verify("clean-slate without erase consent: the include is written, install refuses 25", CLEAN_JOB, [disk_sda()], keys=REFUSAL_KEY),
    verify("clean-slate, verify mode: the clean-slate include and the report", CLEAN_JOB, [disk_sda()], mode="verify"),
    verify("erase, two drives, the countdown runs out: the commit line, the stick disarmed", ERASE_JOB, [disk_vm(), home_disk()], stick={"markers": ["boot-install:v9", "bench:"], "files": {"upgrade_/LiveOS/gnome.squashfs": "g" * 600, "KIT-MANIFEST.txt": "k"}}),
    verify("erase, two drives, a key during the countdown: cancelled, exit 27, boot-install removed", ERASE_JOB, [disk_vm(), home_disk()], stick={"markers": ["boot-install:v9"], "files": {"upgrade_/LiveOS/gnome.squashfs": "g" * 600, "KIT-MANIFEST.txt": "k"}}, keys=[{"after": 1.0, "data": "q"}], countdown_secs=5),
    verify("erase, one drive, the countdown runs out", ERASE_ONE, [disk_vm()], stick={"markers": ["boot-install:v9"], "files": {"upgrade_/LiveOS/gnome.squashfs": "g" * 600, "KIT-MANIFEST.txt": "k"}}),
    verify("erase: the home drive the job names is not attached: identity fail, refused 20, no countdown", ERASE_JOB, [disk_vm()], stick={"files": {"upgrade_/LiveOS/gnome.squashfs": "g" * 600, "KIT-MANIFEST.txt": "k"}}, keys=REFUSAL_KEY),
    verify("erase: the home drive's size differs", ERASE_JOB, [disk_vm(), home_disk(size=68719476737)], stick={"files": {"upgrade_/LiveOS/gnome.squashfs": "g" * 600, "KIT-MANIFEST.txt": "k"}}, keys=REFUSAL_KEY),
    verify("erase: disks[0] is not the system disk the job identifies", vjob(ERASE_JOB, **{"erase_consent.disks": [dict(ERASE_JOB["erase_consent"]["disks"][0], unique_id="60022480000000000000000000000009")]}), [disk_vm()], stick={"files": {"upgrade_/LiveOS/gnome.squashfs": "g" * 600, "KIT-MANIFEST.txt": "k"}}, keys=REFUSAL_KEY),
    verify("erase: the countdown cannot be shown (no console switch): refused 26", ERASE_ONE, [disk_vm()], stick={"files": {"upgrade_/LiveOS/gnome.squashfs": "g" * 600, "KIT-MANIFEST.txt": "k"}}, chvt_ok=False, keys=REFUSAL_KEY),
    verify("erase: another program holds the countdown's console open (R38): refused 30, no countdown", ERASE_ONE, [disk_vm()], stick={"markers": ["boot-install:v9"], "files": {"upgrade_/LiveOS/gnome.squashfs": "g" * 600, "KIT-MANIFEST.txt": "k"}}, holder="before", keys=REFUSAL_KEY),
    verify("erase: another program opens the console during the countdown (R38): the countdown stops, refused 30, nothing erased", ERASE_ONE, [disk_vm()], stick={"markers": ["boot-install:v9"], "files": {"upgrade_/LiveOS/gnome.squashfs": "g" * 600, "KIT-MANIFEST.txt": "k"}}, holder="countdown", countdown_secs=5),
    # the one click test's cancel (roadmap item 21, 2026-10-27): a marker that can only cancel
    verify("erase: the stick's test marker cancels the countdown after its 2 s as a key would: exit 27, the record says test-marker, the marker and boot-install removed", ERASE_ONE, [disk_vm()], stick={"markers": ["boot-install:v9"], "files": {"upgrade_/LiveOS/gnome.squashfs": "g" * 600, "KIT-MANIFEST.txt": "k", "upgrade_/test-cancel-countdown": "2"}}, countdown_secs=5),
    verify("erase: an unreadable test marker still only cancels (5 s, inside the countdown), never an erase", ERASE_ONE, [disk_vm()], stick={"markers": ["boot-install:v9"], "files": {"upgrade_/LiveOS/gnome.squashfs": "g" * 600, "KIT-MANIFEST.txt": "k", "upgrade_/test-cancel-countdown": "soon"}}, countdown_secs=8),
    verify("hardware: no connectors, no wifi device, firmware artifacts listed and present; verify mode", FW_JOB, [disk_sda()], mode="verify", drm=[], graphics=[], nmcli={"devices": [["enp1s0", "ethernet"]], "ssids": []},
           stick={"files": {"upgrade_/LiveOS/kde.squashfs": "k" * 600, "KIT-MANIFEST.txt": "k", "upgrade_/artifacts/firmware/cs35l41/cs35l41-dsp1-spk-prot.wmfw": "fw1", "upgrade_/artifacts/firmware/cs35l41/cs35l41-dsp1-spk-prot.bin": "fw2"}}),
    verify("hardware: connectors but none driving a mode, wifi sees nothing, an artifact missing; install mode still goes on", FW_JOB, [disk_sda()], drm=[{"name": "card1-DP-1", "status": "disconnected", "modes": []}], nmcli={"devices": [["wlp2s0", "wifi"]], "ssids": []}, secureboot=0,
           stick={"markers": ["boot-install:v2"], "files": {"upgrade_/LiveOS/kde.squashfs": "k" * 600, "KIT-MANIFEST.txt": "k", "upgrade_/artifacts/firmware/cs35l41/cs35l41-dsp1-spk-prot.wmfw": "fw1"}}),
    verify("a refusal with no console to show it on: the message, the wait, the restart", KEEP, [disk_vm()], tty=False, secureboot=None),
    verify("the bench marker lengthens GRUB's menu timeout in the keep-windows include", KEEP, [disk_sda()], mode="verify", stick={"markers": ["bench:"]}),
]

# ---- outcome
VERIFY_JSON = {"schema": "verify/1", "verify_version": "0.6.0", "job_id": KEEP["job_id"], "mode": "install", "created_utc": "2026-10-14T20:00:00Z", "kernel": "6.14.0-63.fc44.x86_64", "cmdline": "x",
               "identity": {"result": "pass", "disk": "/dev/sda", "matched_by": "unique_id via nvme-eui.0025385a91b12345", "disk_size_bytes": "250059350016", "job_size_bytes": "250059350016"},
               "hardware": {"display": "pass", "display_detail": "card0-eDP-1:1920x1080", "wifi": "pass", "wifi_detail": "wlp2s0 sees 3 networks", "audio_firmware": "skipped", "audio_detail": "job lists no firmware artifacts"},
               "storage": {"path": "keep-windows", "esp": "/dev/sda1", "esp_result": "pass", "include_written": True, "erase": False, "home_disk": "", "home_matched_by": "", "home_size_bytes": ""},
               "esp_snapshot": {"result": "pass", "files": 3, "path": "upgrade_/esp-snapshot"},
               "payload": {"desktop": "kde", "image": "upgrade_/LiveOS/kde.squashfs", "result": "pass", "detail": "ok", "read_mbps": "22.6"}, "secure_boot": "1"}
import hashlib
h = lambda s: hashlib.sha256(s.encode()).hexdigest()
SNAPSHOT = {"SHA256SUMS": f"{h(BOOTX64_WIN)}  ./EFI/Boot/bootx64.efi\n{h('BCD-store-bytes')}  ./EFI/Microsoft/Boot/BCD\n{h(BOOTMGFW)}  ./EFI/Microsoft/Boot/bootmgfw.efi\n", "SOURCE": "disk=/dev/sda esp=/dev/sda1 taken_utc=2026-10-14T20:00:00Z\n", "boot-entries.txt": "BootOrder: 0001\n"}
SETTLE_IN = {"settle-in": "#!/bin/sh\necho 'settle-in 0.3.0-fake'\n", "upgrade_-settle-in.service": "[Unit]\nDescription=fake\n", "settle-in-window": "#!/bin/sh\necho window\n", "upgrade_-settle-in.desktop": "[Desktop Entry]\n", "upgrade_-settle-in.sh": "# hook\n", "org.upgrade.settle-in.policy": "<policyconfig/>\n",
             "wimlib-imagex": "#!/bin/sh\necho wimlib\n", "cabextract": "#!/bin/sh\necho cab\n", "upgrade-gate.exe": "MZ-gate", "upgrade_-go-back-to-windows.desktop": "[Desktop Entry]\nName=Go back\n"}
PROLOGUE_REC = {"schema": "prologue/1", "prologue_version": "0.12.2", "job_id": KEEP["job_id"], "stage": "armed", "updated_utc": "2026-10-14T19:00:00Z",
                "prologue": {"revalidated": True, "mismatches": [], "volume_check": {"needed": True, "ran": True, "disk_health_at_check": "Healthy", "method": "chkdsk-boot", "scan": None, "restarts": 1, "wininit_1001": None, "found000_present": False, "dirty_after": "clean", "trigger": "dirty-flag"},
                             "shrink": {"remeasured_gb": 70.5, "remeasured_by": "defrag", "remeasured_diskpart_gb": 70.0, "fork_taken": "keep-windows", "requested_bytes": 68000000000, "freed_bytes": 68000000000, "pagefile_disabled": True, "hibernation_disabled": True, "restore_points_deleted": 0, "usn_journal_deleted": 0},
                             "bitlocker": {"status_before": "off", "suspended": False, "reboot_count": None}, "handoff": {"armed": True, "marker": "boot-install", "entry_guid": "{guid}", "armed_utc": "2026-10-14T19:05:00Z", "bcd_backup": "x"}}}
SYSROOT = {"files": {"boot/efi/EFI/Microsoft/Boot/bootmgfw.efi": BOOTMGFW, "boot/efi/EFI/Microsoft/Boot/BCD": "BCD-store-bytes", "boot/efi/EFI/BOOT/BOOTX64.EFI": SHIM, "boot/efi/EFI/fedora/shimx64.efi": SHIM, "boot/efi/EFI/fedora/grubx64.efi": GRUB,
                     "boot/grub2/grub.cfg": "menuentry 'Fedora Linux' {\n}\nmenuentry 'Windows (kept)' --class windows {\n chainloader\n}\n", "lib/modules/6.14.0-63.fc44.x86_64/modules.dep": "", "lib/modules/6.13.9-200.fc42.x86_64/modules.dep": "",
                     "etc/os-release": "NAME=\"Fedora Linux\"\nVERSION_ID=44\n", "root/anaconda-ks.cfg": "# ks\n", "etc/default/grub": "GRUB_TIMEOUT=5\nGRUB_DISABLE_SUBMENU=true\nGRUB_ENABLE_BLSCFG=true\n"},
           "links": {"etc/systemd/system/default.target": "/usr/lib/systemd/system/graphical.target", "etc/systemd/system/display-manager.service": "/usr/lib/systemd/system/sddm.service"}}
EFI = {"entries": [["0001", "Windows Boot Manager", "HD(1,GPT,11111111-0001-4000-8000-000000000001,0x800,0x32000)/File(\\EFI\\Microsoft\\Boot\\bootmgfw.efi)"], ["0002", "Fedora", "HD(1,GPT,11111111-0001-4000-8000-000000000001,0x800,0x32000)/File(\\EFI\\fedora\\shimx64.efi)"], ["0003", "UEFI: USB Stick", "PciRoot(0x0)/Pci(0x14,0x0)/USB(2,0)"]], "order": ["0001", "0002", "0003"], "current": "0003"}
WIFI_FILES = {"upgrade_/artifacts/credentials/wifi/Home Net.xml": "<WLANProfile>home</WLANProfile>", "upgrade_/artifacts/credentials/wifi/Office.xml": "<WLANProfile>office</WLANProfile>"}


def outcome(name, job, **kw):
    c = {"name": name, "mode": "outcome", "job": job, "stick": {"files": dict(STICK_KEEP["files"], **WIFI_FILES, **{"upgrade_/prologue.json": json.dumps(PROLOGUE_REC, indent=2)}), "markers": ["converted:verify 0.6.0 job x committed 2026-10-14T20:00:00Z"], "report": {"verify.json": copy.deepcopy(VERIFY_JSON)}, "esp_snapshot": copy.deepcopy(SNAPSHOT), "settle_in": copy.deepcopy(SETTLE_IN)},
         "disks": [disk_sda()], "sysroot": copy.deepcopy(SYSROOT), "efibootmgr": copy.deepcopy(EFI), "findmnt": {"/mnt/sysroot": "/dev/sda5", "/mnt/sysroot/boot/efi": "/dev/sda1"}, "findmnt_uuid": {"/mnt/sysroot/boot/efi": "A1B2-C3D4"}, "plasma_setup_present": True,
         "clock": {"rtc": "1760472000", "ntp": "yes", "chronyc": "Reference ID    : C0A80001 (192.168.0.1)\nLeap status     : Normal"}, "kernel": "6.14.0-63.fc44.x86_64", "tty": False}
    for k, v in kw.items():
        if k == "stick":
            for kk, vv in v.items():
                if isinstance(vv, dict) and isinstance(c["stick"].get(kk), dict) and kk not in ("report", "esp_snapshot"):
                    c["stick"][kk].update(copy.deepcopy(vv))
                else:
                    c["stick"][kk] = copy.deepcopy(vv)
        else:
            c[k] = v
    return c


ERASE_VERIFY = dict(VERIFY_JSON, job_id=ERASE_JOB["job_id"], storage={"path": "clean-slate", "esp": "", "esp_result": "skipped", "include_written": True, "erase": True, "home_disk": "/dev/sdc", "home_matched_by": "unique_id via wwn-0x60022480000000000000000000000002", "home_size_bytes": "68719476736"}, esp_snapshot={"result": "skipped", "files": 0, "path": "upgrade_/esp-snapshot"})
cases += [
    outcome("keep-windows: the checklist passes, settle-in and the way back installed, the Wi-Fi files moved, the prologue's record taken", KEEP),
    outcome("keep-windows: the firmware dropped the Windows entry: re-created, Linux first", KEEP, efibootmgr={"entries": [EFI["entries"][1], EFI["entries"][2]], "order": ["0003", "0002"], "current": "0003"}),
    outcome("keep-windows: Linux already first, no reorder", KEEP, efibootmgr=dict(EFI, order=["0002", "0001", "0003"])),
    outcome("keep-windows: bootmgfw differs from the snapshot, grub lists no Windows, shim sits in the fallback slot, no verify.json", KEEP, stick={"report": {}, "esp_snapshot": dict(SNAPSHOT, SHA256SUMS=f"{h(SHIM)}  ./EFI/Boot/bootx64.efi\n{h('other')}  ./EFI/Microsoft/Boot/bootmgfw.efi\n")},
            sysroot={"files": dict(SYSROOT["files"], **{"boot/grub2/grub.cfg": "menuentry 'Fedora Linux' {\n}\n"}), "links": SYSROOT["links"]}, grub_mkconfig_fails=True),
    outcome("keep-windows: settle-in incomplete on the stick: not installed, the reason said", KEEP, stick={"settle_in": {k: v for k, v in SETTLE_IN.items() if k != "settle-in-window"}}),
    outcome("keep-windows: a settle-in file does not match the manifest: not installed", KEEP, stick={"wrong_sum_for": ["upgrade_-settle-in.service"]}),
    outcome("keep-windows: the way back's files do not match: settle-in installed, go-back not", KEEP, stick={"wrong_sum_for": ["cabextract"]}),
    outcome("keep-windows: the prologue's record is for another job: placeholders kept; the bench marker installs the boot marker; the stick has no Wi-Fi files", KEEP,
            stick={"files": dict(STICK_KEEP["files"], **{"upgrade_/prologue.json": json.dumps(dict(PROLOGUE_REC, job_id="other-job"), indent=2)}), "markers": ["converted:x", "bench:"]}),
    outcome("keep-windows: no prologue record, no Wi-Fi expected, a text console system", vjob(KEEP, **{"harvest.wifi.profiles": [], "intent.start_at": "console"}), stick={"files": dict(STICK_KEEP["files"])},
            sysroot={"files": SYSROOT["files"], "links": {"etc/systemd/system/default.target": "/usr/lib/systemd/system/multi-user.target"}}),
    outcome("erase-and-install: the countdown's end is the commit line, the credentials scrubbed, no Windows partition", ERASE_JOB,
            stick={"files": dict(STICK_KEEP["files"], **WIFI_FILES), "report": {"verify.json": ERASE_VERIFY, "countdown.json": {"schema": "countdown/1", "result": "elapsed", "seconds": 120, "ended_utc": "2026-10-14T20:02:00Z"}}, "esp_snapshot": None},
            disks=[disk_vm(dev="sda"), home_disk(dev="sdc")], findmnt={"/mnt/sysroot": "/dev/sda3", "/mnt/sysroot/boot/efi": "/dev/sda1"}, efibootmgr={"entries": [EFI["entries"][1]], "order": ["0002"], "current": "0002"}),
    outcome("erase-and-install: no countdown record: said, the outcome written anyway", ERASE_JOB, stick={"files": dict(STICK_KEEP["files"]), "report": {"verify.json": ERASE_VERIFY}, "esp_snapshot": None}, disks=[disk_vm(dev="sda")], efibootmgr={"entries": [EFI["entries"][1]], "order": ["0002"], "current": "0002"}),
    outcome("clean-slate without erase: the commit line crossed by the wipe", CLEAN_JOB, stick={"files": dict(STICK_KEEP["files"]), "report": {"verify.json": dict(VERIFY_JSON, job_id=CLEAN_JOB["job_id"])}, "esp_snapshot": None}, efibootmgr={"entries": [EFI["entries"][1]], "order": ["0002"], "current": "0002"}),
    outcome("the clock: no rtc, time service not synchronized, chronyc silent", KEEP, clock={"rtc": "", "ntp": "no", "chronyc": ""}),
    # the installed system's last settings, the chroot %post's shell until upgrade-installer 0.1.3 (kickstart 0.11.0)
    outcome("post, keep-windows, desktop: os-prober said false and would find Windows; turned off, our entry the only Windows one", KEEP, os_prober_finds_windows=True,
            sysroot={"files": dict(SYSROOT["files"], **{"etc/default/grub": "GRUB_TIMEOUT=5\nGRUB_DISABLE_OS_PROBER=false\n"}), "links": SYSROOT["links"]}),
    outcome("post, keep-windows, console: the start target set to multi-user, the graphical link replaced", vjob(KEEP, **{"intent.start_at": "console"})),
    outcome("post, keep-windows: the ESP's UUID cannot be read: the entry written without it, said in post.log", KEEP, findmnt_uuid={}),
    outcome("post, keep-windows: no /etc/default/grub at all: written with os-prober off", KEEP, sysroot={"files": {k: v for k, v in SYSROOT["files"].items() if k != "etc/default/grub"}, "links": SYSROOT["links"]}),
    outcome("post, erase, console: no GRUB changes, the start target multi-user, Plasma Setup absent", vjob(ERASE_JOB, **{"intent.start_at": "console"}), plasma_setup_present=False,
            stick={"files": dict(STICK_KEEP["files"]), "report": {"verify.json": ERASE_VERIFY, "countdown.json": {"schema": "countdown/1", "result": "elapsed", "seconds": 120, "ended_utc": "2026-10-14T20:02:00Z"}}, "esp_snapshot": None},
            disks=[disk_vm(dev="sda")], findmnt={"/mnt/sysroot": "/dev/sda3", "/mnt/sysroot/boot/efi": "/dev/sda1"}, efibootmgr={"entries": [EFI["entries"][1]], "order": ["0002"], "current": "0002"},
            sysroot={"files": {k: v for k, v in SYSROOT["files"].items() if k != "boot/grub2/grub.cfg"}, "links": SYSROOT["links"]}),
    outcome("post, erase, desktop: no GRUB changes, the start target graphical over a multi-user link", ERASE_JOB,
            stick={"files": dict(STICK_KEEP["files"]), "report": {"verify.json": ERASE_VERIFY, "countdown.json": {"schema": "countdown/1", "result": "elapsed", "seconds": 120, "ended_utc": "2026-10-14T20:02:00Z"}}, "esp_snapshot": None},
            disks=[disk_vm(dev="sda")], findmnt={"/mnt/sysroot": "/dev/sda3", "/mnt/sysroot/boot/efi": "/dev/sda1"}, efibootmgr={"entries": [EFI["entries"][1]], "order": ["0002"], "current": "0002"},
            sysroot={"files": {k: v for k, v in SYSROOT["files"].items() if k != "boot/grub2/grub.cfg"}, "links": {"etc/systemd/system/default.target": "/usr/lib/systemd/system/multi-user.target"}}),
]

out = pathlib.Path(__file__).with_name("cases.json")
out.write_text(json.dumps(cases, indent=1) + "\n", encoding="utf-8")
print(f"{out}: {len(cases)} cases ({sum(c['mode'] == 'verify' for c in cases)} verify, {sum(c['mode'] == 'outcome' for c in cases)} outcome)")
