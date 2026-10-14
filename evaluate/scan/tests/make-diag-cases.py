#!/usr/bin/env python3
"""Writes tests/diag-cases.json: made-up facts for `upgrade-scan diag`
(CLAUDE.md rule #5, the logic level). Every number is invented; the shapes
follow what the collectors read (src/diag/secureboot.rs, src/diag/smart.rs).
The byte fields (firmware variables, db/dbx, the stick's boot files, the ATA
SMART blocks) are hex. Run it again to add a case; the golden was recorded
from the original scripts once (tests/diag-golden.json, frozen) and a new
case gets its expected lines from the Rust's own output, reviewed as a diff.
"""
import json, pathlib

def pe(sections):
    """A minimal PE: MZ stub, COFF header, no optional header, one header
    and one raw block per section (name, bytes)."""
    out = bytearray(b'MZ' + b'\0' * 0x3a) + (0x40).to_bytes(4, 'little')
    pe_off = 0x40
    out += b'PE\0\0' + (0x8664).to_bytes(2, 'little') + len(sections).to_bytes(2, 'little') + bytes(12) + (0).to_bytes(2, 'little') + (0x22).to_bytes(2, 'little')
    table_off = pe_off + 24
    data_off = (table_off + 40 * len(sections) + 15) // 16 * 16
    headers, body, cur = bytearray(), bytearray(), data_off
    for name, data in sections:
        raw = data + b'\0' * ((-len(data)) % 16)
        headers += name.encode().ljust(8, b'\0') + len(data).to_bytes(4, 'little') + (0x1000).to_bytes(4, 'little') + len(raw).to_bytes(4, 'little') + cur.to_bytes(4, 'little') + bytes(12) + (0x40000040).to_bytes(4, 'little')
        body += raw
        cur += len(raw)
    out += headers
    out += b'\0' * (data_off - len(out))
    out += body
    return bytes(out)

def smart_block(entries):
    """The 512-byte ATA SMART attribute block: (id, flags, current, worst, raw48)."""
    b = bytearray(512)
    b[0] = 0x10
    for k, (id_, flags, cur, worst, raw) in enumerate(entries):
        i = 2 + 12 * k
        b[i], b[i + 1], b[i + 2], b[i + 3], b[i + 4] = id_, flags & 0xff, flags >> 8, cur, worst
        b[i + 5:i + 11] = raw.to_bytes(6, 'little')
    return bytes(b)

def thresholds(entries):
    b = bytearray(512)
    b[0] = 0x10
    for k, (id_, th) in enumerate(entries):
        i = 2 + 12 * k
        b[i], b[i + 1] = id_, th
    return bytes(b)

H = lambda b: b.hex()

GRUB_SBAT = (b"sbat,1,SBAT Version,sbat,1,https://github.com/rhboot/shim/blob/main/SBAT.md\n"
             b"grub,5,Free Software Foundation,grub,2.12,https://www.gnu.org/software/grub/\n"
             b"grub.fedora,5,The Fedora Project,grub2,2.12-66.fc44,https://src.fedoraproject.org/rpms/grub2\n")
SHIM_SBAT = (b"sbat,1,SBAT Version,sbat,1,https://github.com/rhboot/shim/blob/main/SBAT.md\n"
             b"shim,4,UEFI shim,shim,1,https://github.com/rhboot/shim\n"
             b"shim.fedora,2,The Fedora Project,shim,16.1-7,https://src.fedoraproject.org/rpms/shim\n")
SHIM_LEVEL = b"sbat,1,2025051000\nshim,4\ngrub,5\ngrub.debian,4\n\0"

sbat_level_rt = b"sbat,1,2025051000\nshim,4\ngrub,5\ngrub.debian,4\ngrub.peimage,2\n\0"
sbat_level_reg = list(b"sbat,1,2024010900\nshim,4\ngrub,3\ngrub.debian,4\0") + [32] * 18 + [0]
db_bytes = bytes(range(256)) * 3 + b"Microsoft Corporation UEFI CA 2011"
dbx_bytes = bytes([0x26, 0x16, 0xc4, 0xc1, 0x4c, 0x50, 0x92, 0x40]) * 400

REGISTRY_FULL = [
    {"Key": "HKLM\\SYSTEM\\CurrentControlSet\\Control\\SecureBoot", "Values": [{"Name": "AvailableUpdates", "Data": 0}]},
    {"Key": "HKLM\\SYSTEM\\CurrentControlSet\\Control\\SecureBoot\\SBAT", "Values": [{"Name": "UpdateStatus", "Data": 2}, {"Name": "SbatLevel", "Data": sbat_level_reg}]},
    {"Key": "HKLM\\SYSTEM\\CurrentControlSet\\Control\\SecureBoot\\Servicing", "Values": [
        {"Name": "UEFICA2023Status", "Data": "Updated"},
        {"Name": "ConfidenceLevel", "Data": "High Confidence"},
        {"Name": "LastTelemetrySendTime", "Data": [62, 191, 12, 89, 227, 91, 221, 1]},
        {"Name": "RefireRunDate", "Data": 134345952000000000},
        {"Name": "OEMModelSKU", "Data": ""},
        {"Name": "Negative", "Data": -1},
        {"Name": "Paths", "Data": ["C:\\one", "D:\\two"]},
        {"Name": "Empty", "Data": []},
    ]},
    {"Key": "HKLM\\SYSTEM\\CurrentControlSet\\Control\\SecureBoot\\Servicing\\DeviceAttributes", "Values": [
        {"Name": "OEMName", "Data": "Example Computer Co."},
        {"Name": "FirmwareReleaseDate", "Data": "05/28/2025"},
    ]},
    {"Key": "HKLM\\SYSTEM\\CurrentControlSet\\Control\\SecureBoot\\Servicing\\UploadedForCurrentBootCycle", "Values": []},
    {"Key": "HKLM\\SYSTEM\\CurrentControlSet\\Control\\SecureBoot\\State", "Values": [
        {"Name": "UEFISecureBootEnabled", "Data": 1},
        {"Name": "PolicyPublisher", "Data": "{77fa9abd-0359-4d32-bd60-28f4e78f784b}"},
        {"Name": "PolicyVersion", "Data": 1},
    ]},
]

cases = []

cases.append({"name": "secureboot: elevated, level 2025051000 applied, Fedora 44 files on the stick", "diag": "secureboot", "facts": {
    "Now": "2026-10-14T16:01:02.1234567Z", "Computer": "CASE-ONE", "Privilege": "ok",
    "Variables": [
        {"Name": "SbatLevel", "Status": "error 203 (203 = not present or not readable from Windows)", "Bytes": None},
        {"Name": "SbatLevelRT", "Status": "attributes 0x7", "Bytes": H(sbat_level_rt)},
    ],
    "Databases": [{"Name": "db", "Bytes": H(db_bytes), "Error": None}, {"Name": "dbx", "Bytes": H(dbx_bytes), "Error": None}],
    "ConfirmSecureBoot": "True",
    "Registry": REGISTRY_FULL,
    "Events": [
        {"TimeUtc": "2026-10-11T23:29:59.4653094Z", "Provider": "Microsoft-Windows-TPM-WMI", "Id": 1808, "Message": "This device has updated Secure Boot CA/keys.\r\n\r\nThis device signature information is:\tKEK\t\tdb  \n"},
        {"TimeUtc": "2026-10-11T23:24:57.3118572Z", "Provider": "Microsoft-Windows-TPM-WMI", "Id": 1038, "Message": "  Pre-attestation health checks confirm that the device is expected to pass attestation."},
        {"TimeUtc": "2026-10-03T15:40:00.0000000Z", "Provider": "Microsoft-Windows-Kernel-Boot", "Id": 1801, "Message": "Secure Boot servicing: the SBAT level was raised to 2025051000."},
        {"TimeUtc": "2026-09-30T08:00:00.0000001Z", "Provider": "Microsoft-Windows-Kernel-Boot", "Id": 1795, "Message": ""},
    ],
    "Files": [
        {"Name": "EFI\\BOOT\\grubx64.efi", "Bytes": H(pe([(".text", b"\x90" * 40), (".sbat", GRUB_SBAT)]))},
        {"Name": "EFI\\BOOT\\BOOTX64.EFI", "Bytes": H(pe([(".text", b"\xcc" * 24), (".sbat", SHIM_SBAT), (".sbatlevel", SHIM_LEVEL)]))},
    ],
}})

cases.append({"name": "secureboot: not elevated, nothing readable, one stick file without a .sbat section", "diag": "secureboot", "facts": {
    "Now": "2026-10-14T16:02:03.0000000Z", "Computer": "CASE-TWO", "Privilege": "AdjustTokenPrivileges 1300",
    "Variables": [
        {"Name": "SbatLevel", "Status": "error 1314 (203 = not present or not readable from Windows)", "Bytes": None},
        {"Name": "SbatLevelRT", "Status": "error 1314 (203 = not present or not readable from Windows)", "Bytes": None},
    ],
    "Databases": [{"Name": "db", "Bytes": None, "Error": "Unable to set proper privileges. Access was denied."}, {"Name": "dbx", "Bytes": None, "Error": "Unable to set proper privileges. Access was denied."}],
    "ConfirmSecureBoot": "Unable to set proper privileges. Access was denied.",
    "Registry": REGISTRY_FULL[:2] + REGISTRY_FULL[-1:],
    "Events": [],
    "Files": [
        {"Name": "EFI\\BOOT\\grubx64.efi", "Bytes": None},
        {"Name": "EFI\\BOOT\\BOOTX64.EFI", "Bytes": H(pe([(".text", b"\xcc" * 24), (".data", b"nothing to see")]))},
    ],
}})

cases.append({"name": "secureboot: Secure Boot off, no SBAT key, a variable with bytes past ASCII, no stick", "diag": "secureboot", "facts": {
    "Now": "2026-01-02T00:00:00.5000000Z", "Computer": "CASE-THREE", "Privilege": "ok",
    "Variables": [
        {"Name": "SbatLevel", "Status": "attributes 0x3", "Bytes": H(b"sbat,1,2023012900\nshim,2\ngrub,3\n\xff\xfe\0\0")},
        {"Name": "SbatLevelRT", "Status": "error 203 (203 = not present or not readable from Windows)", "Bytes": None},
    ],
    "Databases": [{"Name": "db", "Bytes": H(b"\x00" * 16), "Error": None}, {"Name": "dbx", "Bytes": None, "Error": "The system could not find the environment option that was entered."}],
    "ConfirmSecureBoot": "False",
    "Registry": [
        {"Key": "HKLM\\SYSTEM\\CurrentControlSet\\Control\\SecureBoot", "Values": []},
        {"Key": "HKLM\\SYSTEM\\CurrentControlSet\\Control\\SecureBoot\\State", "Values": [{"Name": "UEFISecureBootEnabled", "Data": 0}]},
    ],
    "Events": [{"TimeUtc": "2025-12-31T23:59:59.9999999Z", "Provider": "Microsoft-Windows-TPM-WMI", "Id": 1032, "Message": "Secure Boot update failed to update a Secure Boot variable with error Access denied."}],
    "Files": [{"Name": "EFI\\BOOT\\grubx64.efi", "Bytes": None}, {"Name": "EFI\\BOOT\\BOOTX64.EFI", "Bytes": None}],
}})

# the Aspire's shape (RISKS R18, 2026-09-13): a SATA SSD logging bad blocks beside a clean HDD
aspire_attrs = [
    (1, 0x000f, 100, 100, 0), (5, 0x0033, 100, 100, 7), (9, 0x0032, 100, 100, 8566), (12, 0x0032, 100, 100, 9375),
    (0, 0, 0, 0, 0),
    (168, 0x0032, 100, 100, 3), (184, 0x0033, 100, 100, 639), (187, 0x0032, 100, 100, 725), (188, 0x0032, 100, 100, 0),
    (194, 0x0022, 65, 50, 35 | (50 << 16) | (20 << 32)), (195, 0x001a, 1, 1, 59500000), (196, 0x0032, 100, 100, 7),
    (197, 0x0032, 100, 100, 0), (198, 0x0030, 100, 100, 0), (199, 0x003e, 100, 100, 0), (241, 0x0032, 100, 100, 12345678901), (250, 0x0032, 100, 100, 0x123456789abc),
]
hdd_attrs = [(1, 0x000f, 200, 200, 0), (3, 0x0027, 180, 180, 1500), (4, 0x0032, 99, 99, 1200), (5, 0x0033, 200, 200, 0), (9, 0x0032, 90, 90, 7777), (197, 0x0032, 200, 200, 0)]

def ev(t, id_, msg, provider="disk"):
    return {"TimeLocal": t, "Provider": provider, "Id": id_, "Message": msg}

cases.append({"name": "smart: a SATA SSD logging bad blocks beside a clean HDD (the Aspire's shape)", "diag": "smart", "facts": {
    "Now": "2026-09-13T15:16:48.1234567Z", "Computer": "CASE-FOUR",
    "PhysicalDisks": [
        {"DeviceId": "0", "FriendlyName": "SK hynix SC311 SATA 256GB", "BusType": "SATA", "MediaType": "SSD", "HealthStatus": "Healthy", "OperationalStatus": ["OK"], "Size": 256060514304, "FirmwareVersion": "30001P10", "SerialNumber": "MADEUP0001",
         "Reliability": {"Wear": 0, "Temperature": 35, "TemperatureMax": 50, "PowerOnHours": 8566, "ReadErrorsUncorrected": 0, "WriteErrorsUncorrected": 0, "ReadErrorsTotal": 725, "WriteErrorsTotal": 0, "ReadLatencyMax": 1234, "WriteLatencyMax": 567, "StartStopCycleCount": None, "LoadUnloadCycleCount": None}, "ReliabilityError": None},
        {"DeviceId": "1", "FriendlyName": "WDC WD10SPZX-21Z10T0", "BusType": "SATA", "MediaType": "HDD", "HealthStatus": "Healthy", "OperationalStatus": ["OK"], "Size": 1000204886016, "FirmwareVersion": "01.01A01", "SerialNumber": "MADEUP0002",
         "Reliability": None, "ReliabilityError": None},
    ],
    "DiskDrives": [
        {"Index": 0, "Model": "SK hynix SC311 SATA 256GB", "InterfaceType": "IDE", "Status": "OK", "PNPDeviceID": "SCSI\\DISK&VEN_SK_HYNIX&PROD_SC311_SATA_256GB\\4&1A2B3C4D&0&000000"},
        {"Index": 1, "Model": "WDC WD10SPZX-21Z10T0", "InterfaceType": "IDE", "Status": "OK", "PNPDeviceID": "SCSI\\DISK&VEN_WDC&PROD_WD10SPZX-21Z10T0\\4&1A2B3C4D&0&010000"},
    ],
    "FailurePredictStatus": {"Error": None, "Items": [
        {"InstanceName": "SCSI\\Disk&Ven_SK_hynix&Prod_SC311_SATA_256GB\\4&1a2b3c4d&0&000000_0", "PredictFailure": False, "Reason": 0},
        {"InstanceName": "SCSI\\Disk&Ven_WDC&Prod_WD10SPZX-21Z10T0\\4&1a2b3c4d&0&010000_0", "PredictFailure": False, "Reason": 0},
    ]},
    "FailurePredictData": {"Error": None, "Items": [
        {"InstanceName": "SCSI\\Disk&Ven_SK_hynix&Prod_SC311_SATA_256GB\\4&1a2b3c4d&0&000000_0", "VendorSpecific": H(smart_block(aspire_attrs))},
        {"InstanceName": "SCSI\\Disk&Ven_WDC&Prod_WD10SPZX-21Z10T0\\4&1a2b3c4d&0&010000_0", "VendorSpecific": H(smart_block(hdd_attrs))},
    ]},
    "FailurePredictThresholds": {"Error": None, "Items": [
        {"InstanceName": "SCSI\\Disk&Ven_SK_hynix&Prod_SC311_SATA_256GB\\4&1a2b3c4d&0&000000_0", "VendorSpecific": H(thresholds([(1, 50), (5, 10), (9, 0), (12, 0), (184, 97), (187, 0), (194, 0), (195, 0), (196, 10), (197, 0), (198, 0), (199, 0)]))},
        {"InstanceName": "SCSI\\Disk&Ven_WDC&Prod_WD10SPZX-21Z10T0\\4&1a2b3c4d&0&010000_0", "VendorSpecific": H(thresholds([(1, 51), (3, 21), (4, 0), (5, 140), (9, 0), (197, 0)]))},
    ]},
    "DiskEvents": [
        ev("2026-09-13T10:10:04.1200000", 7, "The device, \\Device\\Harddisk0\\DR0, has a bad block."),
        ev("2026-09-13T10:09:58.0000000", 7, "The device, \\Device\\Harddisk0\\DR0, has a bad block."),
        ev("2026-09-12T23:59:59.0000000", 153, "The IO operation at logical block address 0x1a2b3c for Disk 0 (PDO name: \\Device\\00000031) was retried."),
        ev("2026-09-12T12:00:00.0000000", 51, "An error was detected on device \\Device\\Harddisk0\\DR0 during a paging operation."),
        ev("2026-09-12T00:05:00.0000000", 7, "The device, \\Device\\Harddisk0\\DR0, has a bad block."),
        ev("2026-09-01T07:30:15.5000000", 157, "Disk 2 has been surprise removed."),
        ev("2026-08-27T18:45:00.0000000", 7, "The device,   \\Device\\Harddisk0\\DR0,\r\nhas a bad block."),
        ev("2026-08-27T18:44:59.0000000", 153, "The IO operation at logical block address 0x0 for Disk 0 (PDO name: \\Device\\00000031) was retried."),
    ],
    "StorageEvents": [
        ev("2026-09-13T10:10:04.5000000", 98, "Volume C: (\\Device\\HarddiskVolume3) needs to be taken offline to perform a Full Chkdsk. Please run CHKDSK /F.", "Microsoft-Windows-Ntfs"),
        ev("2026-09-13T10:09:00.0000000", 55, "A corruption was discovered in the file system structure on volume C:.\n\nThe exact nature of the corruption is unknown.  The file system structures need to be scanned online.", "Microsoft-Windows-Ntfs"),
        ev("2026-09-12T23:59:58.0000000", 129, "Reset to device, \\Device\\RaidPort0, was issued.", "storahci"),
        ev("2026-09-10T08:00:00.0000000", 55, "A corruption was discovered in the file system structure on volume C:.", "Microsoft-Windows-Ntfs"),
        ev("2026-09-01T07:30:16.0000000", 58, "The disk signature of disk 2 is equal to the disk signature of disk 1.", "partmgr"),
    ],
    "Volumes": [
        {"DriveLetter": "C", "FileSystemLabel": "Acer", "FileSystem": "NTFS", "HealthStatus": "Healthy", "OperationalStatus": "OK", "Size": 237203591168, "SizeRemaining": 61372792832},
        {"DriveLetter": "D", "FileSystemLabel": "Data", "FileSystem": "NTFS", "HealthStatus": "Healthy", "OperationalStatus": "OK", "Size": 1000202039296, "SizeRemaining": 999998001152},
    ],
}})

cases.append({"name": "smart: an NVMe drive, not elevated: counters refused, SMART not supported, nothing logged", "diag": "smart", "facts": {
    "Now": "2026-10-14T18:30:00.0000000Z", "Computer": "CASE-FIVE",
    "PhysicalDisks": [
        {"DeviceId": "0", "FriendlyName": "MTFDKBA1T0QFM-1BD1AABGB", "BusType": "NVMe", "MediaType": "SSD", "HealthStatus": "Healthy", "OperationalStatus": ["OK"], "Size": 1024209543168, "FirmwareVersion": "V3MA101", "SerialNumber": "0000_0000_0000_0001_MADE_UP00_0000_0000.",
         "Reliability": None, "ReliabilityError": "Access to a CIM resource was not available to the client."},
    ],
    "DiskDrives": [{"Index": 0, "Model": "MTFDKBA1T0QFM-1BD1AABGB", "InterfaceType": "SCSI", "Status": "OK", "PNPDeviceID": "SCSI\\DISK&VEN_NVME&PROD_MTFDKBA1T0QFM-1B\\5&89A5465&0&000000"}],
    "FailurePredictStatus": {"Error": "Not supported", "Items": []},
    "FailurePredictData": {"Error": "Not supported", "Items": []},
    "FailurePredictThresholds": {"Error": "Not supported", "Items": []},
    "DiskEvents": [],
    "StorageEvents": [],
    "Volumes": [{"DriveLetter": "C", "FileSystemLabel": "OS", "FileSystem": "NTFS", "HealthStatus": "Healthy", "OperationalStatus": "OK", "Size": 992585519104, "SizeRemaining": 5118668800}],
}})

# more events than the script shows: 45 disk events (it prints the newest 40, oldest first), 25 storage events (the newest 20)
many_disk, many_storage = [], []
for k in range(45):
    minute = 59 - k
    many_disk.append(ev(f"2026-10-01T{23 - k // 60:02d}:{minute:02d}:{(k * 7) % 60:02d}.{k:07d}", [7, 51, 153, 157][k % 4], f"Event number {k}: " + "x" * 200))
for k in range(25):
    many_storage.append(ev(f"2026-10-0{2 + k // 24}T{k % 24:02d}:30:00.0000000", 129 if k % 2 == 0 else 98, "A long message " * 20 + f"#{k}", ["stornvme", "Microsoft-Windows-Ntfs", "volmgr"][k % 3]))
cases.append({"name": "smart: more events than the script shows, a disk with no size, an empty threshold block, a status with no bool", "diag": "smart", "facts": {
    "Now": "2026-10-02T12:00:00.0000000Z", "Computer": "CASE-SIX",
    "PhysicalDisks": [
        {"DeviceId": "3", "FriendlyName": "Virtual Disk", "BusType": "Virtual", "MediaType": "Unspecified", "HealthStatus": "Warning", "OperationalStatus": ["Degraded", "Predictive Failure"], "Size": None, "FirmwareVersion": "", "SerialNumber": "",
         "Reliability": {"Wear": 12, "Temperature": None, "TemperatureMax": None, "PowerOnHours": 1, "ReadErrorsUncorrected": 2, "WriteErrorsUncorrected": 3, "ReadErrorsTotal": 4, "WriteErrorsTotal": 5, "ReadLatencyMax": 6, "WriteLatencyMax": 7, "StartStopCycleCount": 8, "LoadUnloadCycleCount": 9}, "ReliabilityError": None},
        {"DeviceId": "4", "FriendlyName": "Broken Disk", "BusType": "USB", "MediaType": "HDD", "HealthStatus": "Unhealthy", "OperationalStatus": ["Lost Communication"], "Size": 4000787030016, "FirmwareVersion": "0001", "SerialNumber": "X",
         "Reliability": None, "ReliabilityError": "The device is not ready."},
    ],
    "DiskDrives": [{"Index": None, "Model": "", "InterfaceType": "", "Status": "", "PNPDeviceID": ""}],
    "FailurePredictStatus": {"Error": None, "Items": [{"InstanceName": "X_0", "PredictFailure": True, "Reason": 4}]},
    "FailurePredictData": {"Error": None, "Items": [{"InstanceName": "X_0", "VendorSpecific": H(smart_block([(5, 0x0033, 1, 1, 0xffffffffffff), (0, 0, 0, 0, 0), (9, 0x0032, 0, 0, 65535), (12, 0x0032, 0, 0, 65536)]))}, {"InstanceName": "Y_0", "VendorSpecific": H(b"\x10\x00")}]},
    "FailurePredictThresholds": {"Error": None, "Items": [{"InstanceName": "X_0", "VendorSpecific": H(thresholds([]))}]},
    "DiskEvents": many_disk,
    "StorageEvents": many_storage,
    "Volumes": [],
}})

path = pathlib.Path(__file__).with_name("diag-cases.json")
path.write_text(json.dumps(cases, indent=1) + "\n")
print(f"{len(cases)} cases -> {path}")
