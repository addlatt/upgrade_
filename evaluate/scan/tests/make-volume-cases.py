#!/usr/bin/env python3
"""Writes tests/volume-cases.json: made-up facts for `upgrade-scan diag
volume` (CLAUDE.md rule #5, the logic level), the shape of
src/diag/volume.rs's Facts. Every number and name is invented. The golden
was recorded from DIAG-VOLUME.cmd's inline PowerShell once
(tests/volume-golden.json, frozen); a new case gets its expected lines
from the Rust's own output, reviewed as a diff.
"""
import json, pathlib

CHKDSK_LOG = "Checking file system on C:\r\nThe type of the file system is NTFS.\r\n\r\nA disk check has been scheduled.\r\nWindows will now check the disk.\r\n\r\nStage 1: Examining basic file system structure ...\r\n  389376 file records processed.\r\nFile verification completed.\r\n\r\nWindows has scanned the file system and found no problems.\r\nNo further action is required."
LONG = "The file system structure on volume C: has now been repaired. " * 8

cases = [
    {"name": "a dirty volume with found.000, a boot-time check logged, NTFS and disk events, an SSD and an HDD",
     "facts": {"Now": "2026-10-14T16:01:02.0000000Z", "Computer": "ASPIRE-TEST",
               "FsutilDirty": ["Volume - C: is Dirty"], "Chkntfs": ["The type of the file system is NTFS.", "C: is dirty.  You may use the /C option to schedule chkdsk for this drive."],
               "BootExecute": ["autocheck autochk *"],
               "Found": [{"FullName": "C:\\found.000", "LastWriteTime": "2026-09-15T18:01:40", "Files": 7}, {"FullName": "C:\\found.001", "LastWriteTime": "2026-09-20T09:00:00", "Files": 0}],
               "Volume": {"DriveLetter": "C", "FileSystem": "NTFS", "HealthStatus": "Warning", "OperationalStatus": "Scan Needed", "Size": 255481835520, "SizeRemaining": 61203312640},
               "RepairScan": "ErrorsFound",
               "NtfsInfo": ["NTFS Volume Serial Number :       0x1234567890abcdef", "NTFS Version   :                  3.1", "Bytes Per Sector  :               512", "Total Clusters :                  62373495 (237.9 GB)"],
               "Wininit": [{"TimeLocal": "2026-09-15T18:03:10", "Provider": "Microsoft-Windows-Wininit", "Id": 1001, "Message": CHKDSK_LOG},
                           {"TimeLocal": "2026-09-14T08:00:00", "Provider": "Microsoft-Windows-Winlogon", "Id": 1001, "Message": "not Wininit; dropped"}],
               "Chkdsk": [{"TimeLocal": "2026-09-15T20:10:00", "Provider": "Chkdsk", "Id": 26212, "Message": "Chkdsk was executed in scan mode on a volume snapshot.\r\n\r\nChecking file system on C:\r\nStage 1 ..."},
                          {"TimeLocal": "2026-09-15T20:11:00", "Provider": "Chkdsk", "Id": 26226, "Message": "Chkdsk was executed in verify mode on a volume snapshot."}],
               "Ntfs": [{"TimeLocal": "2026-09-15T18:00:00", "Provider": "Microsoft-Windows-Ntfs", "Id": 55, "Message": "A corruption was discovered in the file system structure on volume C:.\r\n\r\nThe exact nature of the corruption is unknown.  The file system structures need to be scanned online."},
                        {"TimeLocal": "2026-09-15T17:59:00", "Provider": "Microsoft-Windows-Ntfs", "Id": 98, "Message": "Volume C: (\\Device\\HarddiskVolume3) is healthy.  No action is needed."},
                        {"TimeLocal": "2026-09-15T17:58:00", "Provider": "Microsoft-Windows-Ntfs", "Id": 130, "Message": LONG},
                        {"TimeLocal": "2026-09-15T17:57:00", "Provider": "Microsoft-Windows-Ntfs", "Id": 131, "Message": "short one-line message with no double spaces"}],
               "DiskEvents": [{"TimeLocal": "2026-09-15T17:50:00", "Provider": "disk", "Id": 7, "Message": "The device, \\Device\\Harddisk0\\DR0, has a bad block."},
                              {"TimeLocal": "2026-09-15T17:49:00", "Provider": "Service Control Manager", "Id": 7036, "Message": "not a disk provider; dropped"},
                              {"TimeLocal": "2026-09-15T17:48:00", "Provider": "storahci", "Id": 129, "Message": "Reset to device, \\Device\\RaidPort0, was issued.\r\n" + "x" * 250},
                              {"TimeLocal": "2026-09-15T17:47:00", "Provider": "volmgr", "Id": 46, "Message": "Crash dump initialization failed!"},
                              {"TimeLocal": "2026-09-15T17:46:00", "Provider": "partmgr", "Id": 58, "Message": "The disk signature of disk 1 is equal to the disk signature of disk 0."}],
               "PhysicalDisks": [{"DeviceId": "0", "FriendlyName": "KINGSTON SA400M8240G", "MediaType": "SSD", "HealthStatus": "Warning", "OperationalStatus": "OK", "Size": 240057409536},
                                 {"DeviceId": "1", "FriendlyName": "WDC WD10SPZX-21Z10T0", "MediaType": "HDD", "HealthStatus": "Healthy", "OperationalStatus": "OK", "Size": 1000204886016}],
               "HiberbootEnabled": 1, "Hiberfil": True, "LastBootUpTime": "2026-10-14T11:30:05"}},
    {"name": "a clean NVMe volume, nothing found, no events, Fast Startup off",
     "facts": {"Now": "2026-10-14T16:05:00.0000000Z", "Computer": "G16-TEST",
               "FsutilDirty": ["Volume - C: is NOT Dirty"], "Chkntfs": ["The type of the file system is NTFS.", "C: is not dirty."],
               "BootExecute": ["autocheck autochk *", "extra entry"],
               "Found": [],
               "Volume": {"DriveLetter": "C", "FileSystem": "NTFS", "HealthStatus": "Healthy", "OperationalStatus": "OK", "Size": 1022989434880, "SizeRemaining": 500000000000},
               "RepairScan": "NoErrorsFound",
               "NtfsInfo": ["NTFS Volume Serial Number :       0x0000000000000001"],
               "Wininit": [], "Chkdsk": [], "Ntfs": [], "DiskEvents": [],
               "PhysicalDisks": [{"DeviceId": "0", "FriendlyName": "SAMSUNG MZVL21T0HCLR-00B00", "MediaType": "SSD", "HealthStatus": "Healthy", "OperationalStatus": "OK", "Size": 1024209543168}],
               "HiberbootEnabled": 0, "Hiberfil": False, "LastBootUpTime": "2026-10-13T22:15:44"}},
    {"name": "not elevated: fsutil refuses, the scan is refused, no BootExecute value, more events than the cuts keep",
     "facts": {"Now": "2026-10-14T16:09:30.0000000Z", "Computer": "LAB-PC",
               "FsutilDirty": ["The FSUTIL utility requires that you have administrative privileges."], "Chkntfs": ["Access Denied as you do not have sufficient privileges.", "You have to invoke this utility running in elevated mode."],
               "BootExecute": None,
               "Found": [{"FullName": "C:\\found.002", "LastWriteTime": "2026-10-01T00:00:00", "Files": 1}],
               "Volume": {"DriveLetter": "C", "FileSystem": "NTFS", "HealthStatus": "Healthy", "OperationalStatus": "OK", "Size": 128000000000, "SizeRemaining": 1000},
               "RepairScan": None,
               "NtfsInfo": ["The FSUTIL utility requires that you have administrative privileges."],
               "Wininit": [{"TimeLocal": "2026-10-10T07:00:00", "Provider": "Microsoft-Windows-Wininit", "Id": 1001, "Message": "one line"}],
               "Chkdsk": [],
               "Ntfs": [{"TimeLocal": f"2026-10-0{1 + k // 24}T{k % 24:02d}:00:00", "Provider": "Microsoft-Windows-Ntfs", "Id": 98, "Message": f"Volume C: is healthy, mount number {k}."} for k in range(45)],
               "DiskEvents": [{"TimeLocal": f"2026-10-0{1 + k // 24}T{k % 24:02d}:30:00", "Provider": ["disk", "stornvme", "other"][k % 3], "Id": 100 + k, "Message": f"disk event {k}  with  double  spaces"} for k in range(60)],
               "PhysicalDisks": [],
               "HiberbootEnabled": None, "Hiberfil": True, "LastBootUpTime": "2026-10-14T06:00:00"}},
]

out = pathlib.Path(__file__).with_name("volume-cases.json")
out.write_text(json.dumps(cases, indent=1) + "\n", encoding="utf-8")
print(f"{out}: {len(cases)} cases")
