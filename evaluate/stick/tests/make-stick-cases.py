#!/usr/bin/env python3
"""Writes tests/stick-cases.json: made-up disks for `upgrade-stick` (CLAUDE.md
rule #5, the logic level): the self-test's matrix of Write-UpgradeStick.ps1
(the system disk pointed at, two sticks and a USB hard drive, a USB SSD
enclosure, cloned serials, wrong size, off-by-one bytes, SD bus, SAS bus,
offline, Windows To Go) and more, as "fn" cases on the pure functions and
"flow" cases through the script's main block (list, plan, write, with the
live reads and the destructive half answered from the case). Every name
and number is invented. The golden was recorded from the script once
(tests/stick-golden.json, frozen); a new case gets its expected values
from the Rust's own output, reviewed as a diff.
"""
import json, pathlib

NOW = "2026-10-14T20:00:00.0000000Z"


def disk(**over):
    d = {"Number": 0, "FriendlyName": "X", "UniqueId": "ID", "SerialNumber": "", "BusType": "USB", "MediaType": "Unspecified",
         "SizeBytes": 8053063680, "PartitionStyle": "MBR", "IsSystem": False, "IsBoot": False, "IsOffline": False, "IsReadOnly": False,
         "Volumes": [], "HoldsSystemLetter": False}
    d.update(over)
    return d


def vol(letter, label, fs, size=1):
    return {"DriveLetter": letter, "Label": label, "FileSystem": fs, "SizeBytes": size}


SYS = disk(Number=0, FriendlyName="Samsung NVMe", UniqueId="eui.SYS", BusType="NVMe", MediaType="SSD", SizeBytes=1024209543168, IsSystem=True, IsBoot=True, HoldsSystemLetter=True, Volumes=[vol("C", "OS", "NTFS")])
STICK = disk(Number=1, FriendlyName="General UDisk", UniqueId="USB-STICK-A", Volumes=[vol("D", "UPGV0", "FAT32", 8036253696)])
STICK2 = disk(Number=2, FriendlyName="SanDisk Ultra", UniqueId="USB-STICK-B", SizeBytes=32010000000)
CLONE = disk(Number=3, FriendlyName="General UDisk", UniqueId="USB-STICK-A", Volumes=[])
USB_HDD = disk(Number=4, FriendlyName="WD Elements 2TB", UniqueId="USB-HDD", MediaType="HDD", SizeBytes=2000398934016)
USB_SSD = disk(Number=5, FriendlyName="Samsung T7", UniqueId="USB-SSD", MediaType="SSD", SizeBytes=1000204886016)
SD = disk(Number=6, FriendlyName="SD Card", UniqueId="SD-1", BusType="SD")
SAS = disk(Number=7, FriendlyName="Msft Virtual Disk", UniqueId="VHDX-1", BusType="SAS", SizeBytes=8053063680)
OFF = disk(Number=8, FriendlyName="Kingston", UniqueId="USB-OFF", IsOffline=True)
PF = disk(Number=9, FriendlyName="Kingston", UniqueId="USB-PF", HoldsSystemLetter=True, Volumes=[vol("C", "", "NTFS")])
RO = disk(Number=10, FriendlyName="Verbatim", UniqueId="USB-RO", IsReadOnly=True)
TWO_VOLS = disk(Number=11, FriendlyName="Lexar JumpDrive S75 With a Very Long Name", UniqueId="USB-TWO", SizeBytes=64023257088, Volumes=[vol("E", "", "FAT32", 4294967296), vol(None, "UPGDATA", "exFAT", 59000000000)])
LOWER = disk(Number=12, FriendlyName="General UDisk", UniqueId="usb-stick-a", Volumes=[vol("F", "upgv0", "FAT32")])
G16_SYS = disk(Number=0, FriendlyName="SAMSUNG MZVL21T0HCLR-00B00", UniqueId="eui.000000000000000100A07524480C0000", SerialNumber="S6UQNX0T000000", BusType="NVMe", MediaType="SSD", SizeBytes=1024209543168, PartitionStyle="GPT", IsSystem=True, IsBoot=True, HoldsSystemLetter=True,
               Volumes=[vol(None, "SYSTEM", "FAT32", 272629760), vol(None, None, None, 16777216), vol("C", "", "NTFS", 1022989434880), vol(None, None, "NTFS", 1023410176)])
G16_STICK = disk(Number=1, FriendlyName="General UDisk", UniqueId="USBSTOR\\DISK&VEN_GENERAL&PROD_UDISK&REV_5.00\\7&00000000&0&_&0:host", SerialNumber="", SizeBytes=8053063680, Volumes=[vol("D", "UPGRADE", "FAT32", 4294967296), vol("E", "UPGDATA", "exFAT", 3758096384)])
RIG = [disk(Number=0, FriendlyName="Msft Virtual Disk", UniqueId="60022480D07C0B9D81C3C65FD528F6E7", BusType="SAS", SizeBytes=85899345920, PartitionStyle="GPT", IsSystem=True, IsBoot=True, HoldsSystemLetter=True, Volumes=[vol(None, "", "FAT32", 104857600), vol("C", "", "NTFS", 85000000000)]),
       disk(Number=1, FriendlyName="Msft Virtual Disk", UniqueId="600224802F9DA875E3B23683792B2346", BusType="SAS", SizeBytes=536870912, Volumes=[vol("E", "OEMDRV", "FAT32", 536870912)]),
       disk(Number=2, FriendlyName="Msft Virtual Disk", UniqueId="60022480AAAAAAAAAAAAAAAAAAAAAAAA", BusType="SAS", SizeBytes=8053063680, Volumes=[vol("F", "UPGV0", "FAT32", 8053063680)])]

G16 = {"Manufacturer": "ASUSTeK COMPUTER INC.", "Model": "ROG Zephyrus G16 GA605WV_GA605WV", "Caption": "Microsoft Windows 11 Home", "BuildNumber": "26200"}
VM = {"Manufacturer": "Microsoft Corporation", "Model": "Virtual Machine", "Caption": "Microsoft Windows 10 Pro", "BuildNumber": "19045"}
WRITE_OK = {"Written": True, "Verified": True, "Files": 31, "Failed": [], "BootLetter": "D", "DataLetter": "E", "Notes": [],
            "Stick": {"unique_id": G16_STICK["UniqueId"], "serial_number": "", "size_bytes": 8053063680, "friendly_name": "General UDisk", "label": "UPGRADE", "manifest": "SHA256SUMS"}}


def fn(name, func, **args):
    return {"name": name, "kind": "fn", "fn": func, "args": args}


def flow(name, phase, **kw):
    c = {"name": name, "kind": "flow", "phase": phase, "target": "", "expected_bytes": 0, "expected_gb": 0, "source": "C:\\kit\\stick", "label": "UPGRADE", "boot_size_gb": 4, "confirmed_label": "",
         "disks": [G16_SYS, G16_STICK], "machine": G16, "elevated": False, "answer": "", "source_has_sums": True, "write": WRITE_OK, "now": NOW}
    c.update(kw)
    return c


R = lambda disks, target, **k: dict(Disks=disks, Target=target, ExpectedBytes=k.get("eb", 0), ExpectedGB=k.get("eg", 0))

cases = [
    fn("one stick beside the system disk, pointed at, right size: selected", "Resolve-UpgStickTarget", **R([SYS, STICK], "USB-STICK-A", eg=8)),
    fn("the system disk is never a candidate even when pointed at", "Resolve-UpgStickTarget", **R([SYS, STICK], "eui.SYS", eg=1024)),
    fn("two sticks + a USB hard drive attached: the pointed-at stick is selected", "Resolve-UpgStickTarget", **R([SYS, STICK, STICK2, USB_HDD], "USB-STICK-B", eg=32)),
    fn("the USB hard drive pointed at is refused for its media type", "Resolve-UpgStickTarget", **R([SYS, STICK, USB_HDD], "USB-HDD", eg=2000)),
    fn("a USB SSD enclosure is refused the same way", "Resolve-UpgStickTarget", **R([SYS, USB_SSD], "USB-SSD", eg=1000)),
    fn("right stick, wrong expected size: refused", "Resolve-UpgStickTarget", **R([SYS, STICK], "USB-STICK-A", eg=32)),
    fn("exact bytes from a job match exactly", "Resolve-UpgStickTarget", **R([STICK], "USB-STICK-A", eb=8053063680)),
    fn("exact bytes off by one byte: refused", "Resolve-UpgStickTarget", **R([STICK], "USB-STICK-A", eb=8053063681)),
    fn("no expected size at all: refused", "Resolve-UpgStickTarget", **R([STICK], "USB-STICK-A")),
    fn("two sticks with the same unique id (cloned serials): ambiguous, refused", "Resolve-UpgStickTarget", **R([SYS, STICK, CLONE], "USB-STICK-A", eg=8)),
    fn("the target is not attached: refused, says so", "Resolve-UpgStickTarget", **R([SYS, STICK], "USB-STICK-B", eg=32)),
    fn("an SD card is not USB: refused", "Resolve-UpgStickTarget", **R([SD], "SD-1", eg=8)),
    fn("a VHDX on the SAS bus (the rig stand-in) is refused for its bus", "Resolve-UpgStickTarget", **R([SAS], "VHDX-1", eg=8)),
    fn("an offline stick is refused", "Resolve-UpgStickTarget", **R([OFF], "USB-OFF", eg=8)),
    fn("a read-only stick is refused", "Resolve-UpgStickTarget", **R([RO], "USB-RO", eg=8)),
    fn("a USB disk holding the letter Windows runs from is refused (Windows To Go)", "Resolve-UpgStickTarget", **R([PF], "USB-PF", eg=8)),
    fn("empty target: refused", "Resolve-UpgStickTarget", **R([STICK], "", eg=8)),
    fn("a blank target of spaces: refused as no target", "Resolve-UpgStickTarget", **R([STICK], "   ", eg=8)),
    fn("every non-target disk gets 'not the device pointed at' among its reasons", "Resolve-UpgStickTarget", **R([SYS, STICK, STICK2], "USB-STICK-B", eg=32)),
    fn("the target id matches without regard to case", "Resolve-UpgStickTarget", **R([SYS, LOWER], "USB-STICK-A", eg=8)),
    fn("a lower-case bus word is still USB; a lower-case media word is still a drive", "Resolve-UpgStickTarget", Disks=[disk(UniqueId="U1", BusType="usb", MediaType="hdd")], Target="U1", ExpectedBytes=0, ExpectedGB=8),
    fn("a 7.2 GB stick is within 10 % of 8 GB; a 7.1 GB one is not", "Resolve-UpgStickTarget", Disks=[disk(UniqueId="A", SizeBytes=7200000000), disk(Number=1, UniqueId="B", SizeBytes=7100000000)], Target="B", ExpectedBytes=0, ExpectedGB=8),
    fn("a typed 8.1 GB prints as 8.1 in the refusal", "Resolve-UpgStickTarget", Disks=[STICK2], Target="USB-STICK-B", ExpectedBytes=0, ExpectedGB=8.1),
    fn("exact bytes win over a typed size when both are given", "Resolve-UpgStickTarget", **R([STICK], "USB-STICK-A", eb=8053063680, eg=32)),
    fn("no disks at all", "Resolve-UpgStickTarget", **R([], "USB-STICK-A", eg=8)),
    fn("size: 8 GB packaging matches a 8053063680-byte stick", "Test-UpgStickSize", SizeBytes=8053063680, ExpectedBytes=0, ExpectedGB=8),
    fn("size: 8 GB packaging does not match a 16 GB stick", "Test-UpgStickSize", SizeBytes=15931539456, ExpectedBytes=0, ExpectedGB=8),
    fn("size: 64 GB does not match a 2 TB drive", "Test-UpgStickSize", SizeBytes=2000398934016, ExpectedBytes=0, ExpectedGB=64),
    fn("size: exactly 10 % under is still a match", "Test-UpgStickSize", SizeBytes=7200000000, ExpectedBytes=0, ExpectedGB=8),
    fn("size: a negative expected size is no size", "Test-UpgStickSize", SizeBytes=8053063680, ExpectedBytes=-5, ExpectedGB=-1),
    fn("refusals: the system disk's five reasons, in order, plus the size", "Get-UpgDiskRefusals", Disk=SYS, Target="eui.SYS", ExpectedBytes=0, ExpectedGB=8),
    fn("refusals: an offline read-only stick of the wrong size, not pointed at", "Get-UpgDiskRefusals", Disk=disk(UniqueId="Z", IsOffline=True, IsReadOnly=True), Target="Y", ExpectedBytes=1, ExpectedGB=0),
    fn("refusals: none for the right stick", "Get-UpgDiskRefusals", Disk=STICK, Target="USB-STICK-A", ExpectedBytes=8053063680, ExpectedGB=0),
    fn("confirm word: the current label when there is one", "Get-UpgConfirmWord", Disk=STICK),
    fn("confirm word: the model name when the stick is blank", "Get-UpgConfirmWord", Disk=STICK2),
    fn("confirm word: the first labelled volume, skipping an unlabelled one", "Get-UpgConfirmWord", Disk=TWO_VOLS),
    fn("confirm word: a model name with spaces around it is trimmed", "Get-UpgConfirmWord", Disk=disk(FriendlyName="  Kingston DataTraveler  ")),
    fn("sha256sums: parses sha256sum -c lines into path -> hex", "ConvertFrom-UpgSha256Sums", Lines=["ABCDEF0123456789abcdef0123456789abcdef0123456789abcdef0123456789  ./EFI/BOOT/BOOTX64.EFI", "not a sum line"]),
    fn("sha256sums: a binary-mode line, a bare path, a repeated path in another case", "ConvertFrom-UpgSha256Sums", Lines=["a" * 64 + " *./a/b.txt", "b" * 64 + "  c.txt", "c" * 64 + "  ./A/B.TXT", ""]),
    fn("disk line: the G16's internal drive", "Format-DiskLine", Disk=G16_SYS),
    fn("disk line: a stick with two volumes", "Format-DiskLine", Disk=G16_STICK),
    fn("disk line: a long name, a lettered and an unlettered volume", "Format-DiskLine", Disk=TWO_VOLS),
    fn("disk line: a 2 TB USB drive with no volumes", "Format-DiskLine", Disk=USB_HDD),
    fn("disk line: the rig's 512 MiB OEMDRV volume", "Format-DiskLine", Disk=RIG[1]),
    # ---- flows
    flow("list: the G16 with a written stick", "list"),
    flow("list: the rig's three virtual disks", "list", disks=RIG, machine=VM),
    flow("list: no disks", "list", disks=[]),
    flow("plan: the G16's stick pointed at with 8.1 GB: selected, nothing written", "plan", target=G16_STICK["UniqueId"], expected_gb=8.1),
    flow("plan: the G16's stick pointed at with 32 GB: refused", "plan", target=G16_STICK["UniqueId"], expected_gb=32),
    flow("plan: the internal NVMe pointed at: refused on five counts", "plan", target=G16_SYS["UniqueId"], expected_gb=1024),
    flow("plan: a device that is not attached", "plan", target="USBSTOR\\DISK&VEN_X&PROD_Y\\0", expected_gb=8),
    flow("plan: the rig, every VHDX refused for its bus", "plan", disks=RIG, machine=VM, elevated=True, target=RIG[2]["UniqueId"], expected_gb=8),
    flow("plan: exact bytes from a job", "plan", target=G16_STICK["UniqueId"], expected_bytes=8053063680),
    flow("write: refused for size: logged, exit 2", "write", target=G16_STICK["UniqueId"], expected_gb=32, elevated=True),
    flow("write: not elevated: the throw", "write", target=G16_STICK["UniqueId"], expected_gb=8.1, elevated=False),
    flow("write: the source has no SHA256SUMS: the throw", "write", target=G16_STICK["UniqueId"], expected_gb=8.1, elevated=True, source_has_sums=False),
    flow("write: the typed label matches: written and verified", "write", target=G16_STICK["UniqueId"], expected_gb=8.1, elevated=True, answer="UPGRADE"),
    flow("write: the typed label is wrong: refused, exit 2", "write", target=G16_STICK["UniqueId"], expected_gb=8.1, elevated=True, answer="upgrade"),
    flow("write: the label passed in by the caller", "write", target=G16_STICK["UniqueId"], expected_gb=8.1, elevated=True, confirmed_label="UPGRADE"),
    flow("write: a blank stick confirms by its model name", "write", disks=[G16_SYS, dict(G16_STICK, Volumes=[])], target=G16_STICK["UniqueId"], expected_gb=8.1, elevated=True, answer="General UDisk"),
    flow("write: the write fails after the confirmation: the error logged, exit 1", "write", target=G16_STICK["UniqueId"], expected_gb=8.1, elevated=True, confirmed_label="UPGRADE", write={"error": "The disk has already been initialized."}),
    flow("write: written but a file failed verification: not verified, exit 1", "write", target=G16_STICK["UniqueId"], expected_gb=8.1, elevated=True, confirmed_label="UPGRADE", write=dict(WRITE_OK, Verified=False, Files=30, Failed=["EFI\\BOOT\\grubx64.efi"], Notes=["no room for a staging partition"], DataLetter=None)),
    flow("write: a capped boot partition is noted", "write", target=G16_STICK["UniqueId"], expected_gb=8.1, elevated=True, confirmed_label="UPGRADE", label="UPGV0", boot_size_gb=8, write=dict(WRITE_OK, Notes=["boot partition capped to the disk"])),
]

out = pathlib.Path(__file__).with_name("stick-cases.json")
out.write_text(json.dumps(cases, indent=1) + "\n", encoding="utf-8")
print(f"{out}: {len(cases)} cases ({sum(c['kind'] == 'fn' for c in cases)} functions, {sum(c['kind'] == 'flow' for c in cases)} flows)")
