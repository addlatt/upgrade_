#!/usr/bin/env python3
"""Writes tests/storage-mode-cases.json: made-up inputs for the storage-mode
harness (`upgrade-prologue storage-mode`; CLAUDE.md rule #5, the logic
level). Two kinds of case:

- "fn" cases call one of the harness's pure functions with arguments and
  record what it returns;
- "flow" cases run one phase (start, resume, abort, notify) with every live
  read and write answered from the case: the state on disk, the facts, the
  Safe Mode key, the resume context, the clock, the stick, the scanner's
  report and capture for each leg, the popups' answers, the tools' output
  (bcdedit, shutdown), the task and the registry. What the phase does (its
  log lines, popups, notices, tool calls, saves, task and registry calls)
  and what it leaves on disk are the golden.

Every number and name is invented. The golden was recorded from
Test-StorageMode.ps1 once (tests/storage-mode-golden.json, frozen); a new
case gets its expected values from the Rust's own output, reviewed as a
diff.
"""
import json, pathlib

NOW = "2026-10-14T12:00:07.0000000Z"
STATE = "C:\\ProgramData\\upgrade_\\storage-mode"
STICK = "E:\\"

# the self-test's devices, the Aspire's shape and the G16's
ASPIRE_AHCI = {"Name": "Intel(R) 6th Generation Core Processor Family Platform I/O SATA AHCI Controller", "DeviceID": "PCI\\VEN_8086&DEV_9D03&SUBSYS_11931025&REV_21\\3&11583659&0&B8", "PNPClass": "HDC", "Service": "iaStorAC",
               "CompatibleID": ["PCI\\VEN_8086&DEV_9D03&REV_21", "PCI\\VEN_8086&DEV_9D03", "PCI\\VEN_8086&CC_010601", "PCI\\VEN_8086&CC_0106", "PCI\\VEN_8086", "PCI\\CC_010601", "PCI\\CC_0106"]}
RST_RAID = {"Name": "Intel Chipset SATA/PCIe RST Premium Controller", "DeviceID": "PCI\\VEN_8086&DEV_282A&SUBSYS_11931025&REV_21\\3&11583659&0&B8", "PNPClass": "HDC", "Service": "iaStorAC",
            "CompatibleID": ["PCI\\VEN_8086&DEV_282A&REV_21", "PCI\\VEN_8086&DEV_282A", "PCI\\VEN_8086&CC_010400", "PCI\\VEN_8086&CC_0104", "PCI\\VEN_8086", "PCI\\CC_010400", "PCI\\CC_0104"]}
VMD = {"Name": "Intel RST VMD Controller 9A0B", "DeviceID": "PCI\\VEN_8086&DEV_9A0B&SUBSYS_00000000&REV_00\\3&0&0A", "PNPClass": "SCSIAdapter", "Service": "iaStorVD", "CompatibleID": ["PCI\\CC_010400", "PCI\\CC_0104"]}
NVME = {"Name": "Standard NVM Express Controller", "DeviceID": "PCI\\VEN_1344&DEV_5413&SUBSYS_21001344&REV_03\\4&23CF4B8A&0&0011", "PNPClass": "SCSIAdapter", "Service": "stornvme", "CompatibleID": ["PCI\\CC_010802", "PCI\\CC_0108"]}
INTEL_NVME = {"Name": "Standard NVM Express Controller", "DeviceID": "PCI\\VEN_8086&DEV_F1A8&SUBSYS_390D8086&REV_03\\4&1&0&0011", "PNPClass": "SCSIAdapter", "Service": "stornvme", "CompatibleID": ["PCI\\VEN_8086&CC_010802", "PCI\\CC_010802", "PCI\\CC_0108"]}
USB = {"Name": "Intel(R) USB 3.0 eXtensible Host Controller", "DeviceID": "PCI\\VEN_8086&DEV_9D2F&SUBSYS_11931025&REV_21\\3&11583659&0&A0", "PNPClass": "USB", "Service": "USBXHCI", "CompatibleID": ["PCI\\VEN_8086&CC_0C0330", "PCI\\CC_0C0330", "PCI\\CC_0C03"]}
HV = {"Name": "Microsoft Hyper-V SCSI Controller", "DeviceID": "VMBUS\\{BA6163D9-04A1-4D29-B605-72E2FFB1DC7F}\\{...}", "PNPClass": "SCSIAdapter", "Service": "storvsc", "CompatibleID": [None]}
# an Intel RST service on a device whose CompatibleID is one string, and one whose class is only in PNPClass
RST_ONE_STRING = {"Name": "Intel(R) Rapid Storage Technology", "DeviceID": "PCI\\VEN_8086&DEV_A282&SUBSYS_00000000&REV_00\\3&0&B8", "PNPClass": "HDC", "Service": "iaStorAVC", "CompatibleID": "PCI\\VEN_8086&CC_0106"}
HDC_NO_CODES = {"Name": "Intel SATA Controller (no class codes published)", "DeviceID": "PCI\\VEN_8086&DEV_A382&SUBSYS_00000000&REV_00\\3&0&B9", "PNPClass": "HDC", "Service": "storahci", "CompatibleID": []}
LOWER_CC = {"Name": "Intel controller with lower-case ids", "DeviceID": "pci\\ven_8086&dev_a383&subsys_00000000&rev_00\\3&0&ba", "PNPClass": "SCSIAdapter", "Service": "IASTORVD", "CompatibleID": ["pci\\cc_010400"]}


def report(status, detail, level, version="0.3.0", scanned="2026-10-14T12:00:01.0000000Z", title="Storage controller mode", admin=True, vendor="Acer", model="Aspire A515-51G"):
    return {"ScannerVersion": version, "ScannedUtc": scanned,
            "System": {"Vendor": vendor, "Model": model, "BiosVersion": "V1.21", "OsCaption": "Microsoft Windows 11 Home", "OsBuild": 22631},
            "RanAsAdmin": admin, "Verdict": {"Level": level, "Summary": "made up"},
            "Checks": [{"Section": "System", "Title": "Memory", "Status": "ok", "Detail": "12 GB"},
                       {"Section": "Storage", "Title": title, "Status": status, "Detail": detail}]}


def capture(pnp, vendor="Acer", model="Aspire A515-51G", captured="2026-10-14"):
    return {"SchemaVersion": 1, "Captured": captured, "Label": f"{vendor} {model}", "Sys": {"Vendor": vendor, "Model": model, "RamGB": 12, "Firmware": "UEFI"}, "Pnp": pnp, "Expected": {}}


ASPIRE_FACTS = {"Vendor": "Acer", "Model": "Aspire A515-51G", "BiosVersion": "V1.21", "OsCaption": "Microsoft Windows 11 Home", "OsBuild": 22631, "SecureBoot": "on", "BitLocker": "off", "StickVolumeId": "\\\\?\\Volume{11111111-2222-3333-4444-555555555555}\\", "StickBus": "USB"}
VM_FACTS = {"Vendor": "Microsoft Corporation", "Model": "Virtual Machine", "BiosVersion": "Hyper-V UEFI Release v4.1", "OsCaption": "Microsoft Windows 10 Pro", "OsBuild": 19045, "SecureBoot": "off", "BitLocker": "unknown", "StickVolumeId": "\\\\?\\Volume{aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee}\\", "StickBus": "SAS"}
G16_FACTS = {"Vendor": "ASUSTeK COMPUTER INC.", "Model": "ROG Zephyrus G16", "BiosVersion": "GA605WI.308", "OsCaption": "Microsoft Windows 11 Home", "OsBuild": 26100, "SecureBoot": "on", "BitLocker": "off", "StickVolumeId": "\\\\?\\Volume{99999999-8888-7777-6666-555555555555}\\", "StickBus": "USB"}

GUID = "{6a3c1f2e-0b4d-4c8a-9e7f-1a2b3c4d5e6f}"
LAUNCHER_CTX = {"Utc": "2026-10-14T12:00:00.0000000Z", "RunAs": "ASPIRE\\person", "Interactive": True, "SessionId": 1, "ExplorerRunning": True, "Unattended": False, "UptimeSeconds": 900}
SYSTEM_CTX = {"Utc": "2026-10-14T12:00:00.0000000Z", "RunAs": "NT AUTHORITY\\SYSTEM", "Interactive": False, "SessionId": 0, "ExplorerRunning": False, "Unattended": True, "UptimeSeconds": 27}
SAFE_CTX = {"Utc": "2026-10-14T12:00:00.0000000Z", "RunAs": "NT AUTHORITY\\SYSTEM", "Interactive": False, "SessionId": 0, "ExplorerRunning": False, "Unattended": True, "UptimeSeconds": 41}

# bcdedit and shutdown as the tools answer; matched by program and the first argument
NATIVE_OK = [
    {"program": "bcdedit", "first": "/copy", "exit": 0, "text": f"The entry was successfully copied to {GUID}."},
    {"program": "bcdedit", "first": "/set", "exit": 0, "text": "The operation completed successfully."},
    {"program": "bcdedit", "first": "/enum", "second": GUID, "exit": 0, "text": f"Windows Boot Loader\n-------------------\nidentifier              {GUID}\ndevice                  partition=C:\npath                    \\WINDOWS\\system32\\winload.efi\ndescription             upgrade_ storage-mode test (Safe Mode, one boot)\nsafeboot                Minimal"},
    {"program": "bcdedit", "first": "/enum", "second": "{bootmgr}", "exit": 0, "text": f"Windows Boot Manager\n--------------------\nidentifier              {{bootmgr}}\ndevice                  partition=\\Device\\HarddiskVolume1\ndescription             Windows Boot Manager\nbootsequence            {GUID}\ndisplayorder            {{current}}"},
    {"program": "bcdedit", "first": "/displayorder", "exit": 0, "text": "The operation completed successfully."},
    {"program": "bcdedit", "first": "/bootsequence", "exit": 0, "text": "The operation completed successfully."},
    {"program": "bcdedit", "first": "/deletevalue", "exit": 0, "text": "The operation completed successfully."},
    {"program": "bcdedit", "first": "/delete", "exit": 0, "text": "The operation completed successfully."},
    {"program": "shutdown", "first": "/r", "second": "/fw", "exit": 0, "text": ""},
    {"program": "shutdown", "first": "/r", "exit": 0, "text": ""},
]
NATIVE_NO_SEQUENCE = [dict(n, text=n["text"].replace(f"bootsequence            {GUID}\n", "")) if n.get("second") == "{bootmgr}" else n for n in NATIVE_OK]


def with_(base, **over):
    out = [dict(n) for n in base]
    for n in out:
        key = n["first"] + (n.get("second") or "")
        if key in over:
            n.update(over[key])
    return out


def state(stage, legs, ask, safe_entry=True, bench=False, facts=ASPIRE_FACTS, resumes=(), safe_boots=(), fw=(), last=""):
    return {"HarnessVersion": "0.1.0", "StartedUtc": "2026-10-14T11:50:00.0000000Z", "Stage": stage, "Facts": facts, "StickVolumeId": facts["StickVolumeId"], "StickRootAtStart": STICK,
            "Bench": bench, "Legs": legs, "Resumes": list(resumes), "SafeBoots": list(safe_boots), "FwReboots": list(fw),
            "SafeEntry": {"Guid": GUID, "Enum": f"identifier {GUID} safeboot Minimal"} if safe_entry else None,
            "Ask": ask, "LastOutcome": last, "Cleanup": None, "Notice": None, "UpdatedUtc": "2026-10-14T11:50:30.0000000Z"}


def leg(n, asked, mode, ctrls, status, detail, verdict, run_as="ASPIRE\\person", session=1, unattended=False):
    return {"N": n, "Utc": f"2026-10-14T11:5{n}:00.0000000Z", "Asked": asked, "Mode": mode, "Controllers": ctrls,
            "Check": {"Status": status, "Detail": detail, "Verdict": verdict, "ScannerVersion": "0.3.0", "ScannedUtc": "2026-10-14T11:50:01.0000000Z"},
            "ReportFile": "upgrade-report-Aspire-A515-51G-20261014-1150.json", "CaptureFile": "machine-capture.json", "RunAs": run_as, "SessionId": session, "Unattended": unattended}


ASPIRE_CTRL = {"Name": ASPIRE_AHCI["Name"], "PciId": "8086:9d03", "Classes": ["0106"], "Service": "iaStorAC", "CompatibleIds": ASPIRE_AHCI["CompatibleID"]}
RAID_CTRL = {"Name": RST_RAID["Name"], "PciId": "8086:282a", "Classes": ["0104"], "Service": "iaStorAC", "CompatibleIds": RST_RAID["CompatibleID"]}
LEG1_AHCI = leg(1, "initial", "ahci", [ASPIRE_CTRL], "warn", "Intel RST driver present, controller not in RAID mode (iaStorAC)", "RED")
LEG2_RAID = leg(2, "raid", "raid", [RAID_CTRL], "fail", "Intel RST / VMD active (RST Premium, class 0104)", "RED", "NT AUTHORITY\\SYSTEM", 0, True)
LEG2_AHCI = leg(2, "raid", "ahci", [ASPIRE_CTRL], "warn", "Intel RST driver present, controller not in RAID mode (iaStorAC)", "RED", "NT AUTHORITY\\SYSTEM", 0, True)
FW1 = {"Utc": "2026-10-14T11:50:40.0000000Z", "Ask": "raid", "Method": "fw"}
RESUME1 = dict(SYSTEM_CTX, Utc="2026-10-14T11:55:00.0000000Z", StickWaitSeconds=2, Stage="armed-1")
SAFE1 = {"Utc": "2026-10-14T11:53:00.0000000Z", "RunAs": "ASPIRE\\person", "OptionValue": "1", "UptimeSeconds": "35", "Source": "runonce-signin"}

ASPIRE_REPORT_WARN = report("warn", "Intel RST driver present, controller not in RAID mode (iaStorAC)", "RED")
ASPIRE_REPORT_FAIL = report("fail", "Intel RST / VMD active (RST Premium, class 0104)", "RED")
LEG_AHCI = {"report": ASPIRE_REPORT_WARN, "report_file": "upgrade-report-Aspire-A515-51G-20261014-1200.json", "capture": capture([USB, ASPIRE_AHCI, HV])}
LEG_RAID = {"report": ASPIRE_REPORT_FAIL, "report_file": "upgrade-report-Aspire-A515-51G-20261014-1200.json", "capture": capture([USB, RST_RAID])}
LEG_G16 = {"report": report("ok", "standard AHCI / NVMe - visible to Linux installers", "YELLOW", vendor="ASUSTeK COMPUTER INC.", model="ROG Zephyrus G16"), "report_file": "upgrade-report-ROG-Zephyrus-G16-20261014-1200.json", "capture": capture([NVME, USB], "ASUSTeK COMPUTER INC.", "ROG Zephyrus G16")}
LEG_NO_REPORT = dict(LEG_AHCI, no_report=True)
LEG_NO_CAPTURE = dict(LEG_AHCI, no_capture=True)
LEG_VM = {"report": report("ok", "standard AHCI / NVMe - visible to Linux installers", "YELLOW", vendor="Microsoft Corporation", model="Virtual Machine"), "report_file": "upgrade-report-Virtual-Machine-20261014-1200.json", "capture": capture([HV], "Microsoft Corporation", "Virtual Machine")}


def flow(name, phase, **kw):
    c = {"name": name, "kind": "flow", "phase": phase, "args": {"stick": "E:", "bench": False, "no_prompt": False}, "state": None, "state_files": {}, "marker": None,
         "safe_mode": None, "elevated": True, "context": LAUNCHER_CTX, "now": NOW, "facts": ASPIRE_FACTS, "stick": STICK, "legs": [], "popups": [],
         "native": NATIVE_OK, "task_present": True, "register_task_error": None, "protect_error": None, "runonce": {},
         # the stick's upgrade_\storage-mode folder exists before the phase (a start made it earlier); every phase but start assumes so
         "stick_folder": phase != "start"}
    for k, v in kw.items():
        if k == "args":
            c["args"].update(v)
        else:
            c[k] = v
    return c


def fn(name, func, **args):
    return {"name": name, "kind": "fn", "fn": func, "args": args}


cases = [
    # ---- the pure functions (the self-test's cases, and more)
    fn("pci id: the Aspire's AHCI controller", "Get-SmPciId", DeviceId=ASPIRE_AHCI["DeviceID"]),
    fn("pci id: lower-case ids are lower-cased, not refused", "Get-SmPciId", DeviceId=LOWER_CC["DeviceID"]),
    fn("pci id: a VMBUS path has none", "Get-SmPciId", DeviceId=HV["DeviceID"]),
    fn("class codes: the Aspire's list, deduplicated", "Get-SmClassCodes", CompatibleIds=ASPIRE_AHCI["CompatibleID"]),
    fn("class codes: one string instead of a list", "Get-SmClassCodes", CompatibleIds=RST_ONE_STRING["CompatibleID"]),
    fn("class codes: lower-case entries are upper-cased", "Get-SmClassCodes", CompatibleIds=LOWER_CC["CompatibleID"]),
    fn("class codes: an empty list and a null entry", "Get-SmClassCodes", CompatibleIds=[None, ""]),
    fn("controllers: the Aspire in AHCI is one Intel HDC with class 0106 and iaStorAC", "Get-SmStorageControllers", Pnp=[USB, ASPIRE_AHCI, HV]),
    fn("controllers: a non-Intel NVMe controller is not counted (the G16)", "Get-SmStorageControllers", Pnp=[NVME, USB]),
    fn("controllers: an Intel USB controller is not storage", "Get-SmStorageControllers", Pnp=[USB]),
    fn("controllers: a Hyper-V VM has none", "Get-SmStorageControllers", Pnp=[HV]),
    fn("controllers: an RST service on a device with one compatible id string counts", "Get-SmStorageControllers", Pnp=[RST_ONE_STRING]),
    fn("controllers: an Intel HDC with no class codes counts by PNPClass", "Get-SmStorageControllers", Pnp=[HDC_NO_CODES]),
    fn("controllers: lower-case ids and an upper-case VMD service", "Get-SmStorageControllers", Pnp=[LOWER_CC]),
    fn("controllers: an Intel NVMe controller, a RAID one and the AHCI one together", "Get-SmStorageControllers", Pnp=[INTEL_NVME, RST_RAID, ASPIRE_AHCI, VMD]),
    fn("controllers: an empty device list", "Get-SmStorageControllers", Pnp=[]),
    fn("mode: class 0104 is raid (the pre-VMD RST Premium controller)", "mode-of", Pnp=[RST_RAID]),
    fn("mode: the VMD driver is raid even without a class code", "mode-of", Pnp=[VMD]),
    fn("mode: class 0106 with iaStorAC bound is ahci (the Aspire)", "mode-of", Pnp=[ASPIRE_AHCI]),
    fn("mode: an Intel NVMe controller alone is nvme", "mode-of", Pnp=[INTEL_NVME]),
    fn("mode: raid wins when an AHCI controller and a RAID one coexist", "mode-of", Pnp=[ASPIRE_AHCI, RST_RAID]),
    fn("mode: an upper-case VMD service is raid", "mode-of", Pnp=[LOWER_CC]),
    fn("mode: an Intel HDC with no class codes is other", "mode-of", Pnp=[HDC_NO_CODES]),
    fn("mode: no controllers is none", "mode-of", Pnp=[]),
    fn("ask: ahci asks for raid", "Get-SmNextMode", Current="ahci"),
    fn("ask: raid asks for ahci", "Get-SmNextMode", Current="raid"),
    fn("ask: nvme asks for raid", "Get-SmNextMode", Current="nvme"),
    fn("ask: none asks nothing", "Get-SmNextMode", Current="none"),
    fn("ask: other asks nothing", "Get-SmNextMode", Current="other"),
    fn("ask: AHCI in capitals is not ahci", "Get-SmNextMode", Current="AHCI"),
    fn("label: raid", "Get-SmModeLabel", Mode="raid"),
    fn("label: ahci", "Get-SmModeLabel", Mode="ahci"),
    fn("label: nvme is itself", "Get-SmModeLabel", Mode="nvme"),
    fn("label: empty is empty", "Get-SmModeLabel", Mode=""),
    fn("report: the storage line and verdict are read", "Get-SmReportCheck", Report=ASPIRE_REPORT_FAIL),
    fn("report: a report without the line says so instead of guessing", "Get-SmReportCheck", Report={"Checks": [], "Verdict": {"Level": "GREEN"}}),
    fn("report: the title matches without regard to case", "Get-SmReportCheck", Report=report("ok", "fine", "GREEN", title="STORAGE CONTROLLER MODE")),
    fn("report: an empty report has no verdict and no version", "Get-SmReportCheck", Report={}),
    fn("report: a null detail is an empty text", "Get-SmReportCheck", Report={"ScannerVersion": "0.3.0", "ScannedUtc": "x", "Verdict": {"Level": "RED"}, "Checks": [{"Title": "Storage controller mode", "Status": "fail", "Detail": None}]}),
    fn("bcdedit /copy: the GUID is parsed from the English line", "ConvertFrom-SmBcdCopy", Text=f"The entry was successfully copied to {GUID}."),
    fn("bcdedit /copy: any other wording still yields the GUID", "ConvertFrom-SmBcdCopy", Text="Der Eintrag wurde erfolgreich in {6A3C1F2E-0B4D-4C8A-9E7F-1A2B3C4D5E6F} kopiert."),
    fn("bcdedit /copy: no GUID yields null", "ConvertFrom-SmBcdCopy", Text="The boot configuration data store could not be opened."),
    fn("bcdedit /copy: the first of two GUIDs", "ConvertFrom-SmBcdCopy", Text="{11111111-1111-1111-1111-111111111111} then {22222222-2222-2222-2222-222222222222}"),
    fn("safe mode: OptionValue 1 is Safe Mode", "Test-SmSafeMode", OptionValue=1),
    fn("safe mode: OptionValue 2 is Safe Mode", "Test-SmSafeMode", OptionValue=2),
    fn("safe mode: absent is not", "Test-SmSafeMode", OptionValue=None),
    fn("safe mode: zero is still Safe Mode (the key exists)", "Test-SmSafeMode", OptionValue=0),
    fn("leg 2 same mode is mode-unchanged", "Get-SmLegOutcome", Leg=2, FirstMode="ahci", ThisMode="ahci"),
    fn("leg 2 different is changed", "Get-SmLegOutcome", Leg=2, FirstMode="ahci", ThisMode="raid"),
    fn("leg 3 back to the first mode is restored", "Get-SmLegOutcome", Leg=3, FirstMode="ahci", ThisMode="ahci"),
    fn("leg 3 else not-restored", "Get-SmLegOutcome", Leg=3, FirstMode="ahci", ThisMode="raid"),
    fn("leg 4 is unexpected", "Get-SmLegOutcome", Leg=4, FirstMode="ahci", ThisMode="ahci"),
    fn("leg 1 is unexpected", "Get-SmLegOutcome", Leg=1, FirstMode="ahci", ThisMode="ahci"),
    fn("leg 2: modes compare without regard to case", "Get-SmLegOutcome", Leg=2, FirstMode="ahci", ThisMode="AHCI"),
    fn("safe marker: one line becomes one Safe Mode boot record; junk lines are dropped", "ConvertFrom-SmSafeMarker", Lines=["2026-09-14T06:07:40Z|UPGRIGHV\\rig|1|341", "junk"]),
    fn("safe marker: two lines, one with five fields", "ConvertFrom-SmSafeMarker", Lines=["2026-09-14T06:07:40Z|UPGRIGHV\\rig|1|341", "2026-09-14T06:17:40Z|UPGRIGHV\\rig|1|12|extra", ""]),
    fn("safe marker: no lines", "ConvertFrom-SmSafeMarker", Lines=[]),
    fn("flow: three legs ending restored is restored", "Get-SmFlowResult", Stage="done:restored", Legs=3, LastOutcome="restored"),
    fn("flow: two legs unchanged is mode-unchanged", "Get-SmFlowResult", Stage="done:mode-unchanged", Legs=2, LastOutcome="mode-unchanged"),
    fn("flow: a stop names itself", "Get-SmFlowResult", Stage="stopped:no-intel-controller", Legs=1, LastOutcome=""),
    fn("flow: armed after one leg is in progress", "Get-SmFlowResult", Stage="armed-1", Legs=1, LastOutcome=""),
    fn("flow: two legs changed is in progress", "Get-SmFlowResult", Stage="armed-2", Legs=2, LastOutcome="changed"),
    fn("flow: three legs not restored", "Get-SmFlowResult", Stage="done:not-restored", Legs=3, LastOutcome="not-restored"),
    fn("flow: stopped with error", "Get-SmFlowResult", Stage="stopped:error", Legs=2, LastOutcome="changed"),
    fn("flow: 'stopped' without a colon is in progress", "Get-SmFlowResult", Stage="stopped", Legs=1, LastOutcome=""),
    fn("csv: every field quoted, quotes doubled, newlines flattened", "ConvertTo-SmCsvLine", Fields=["a", "b\"c", "d\ne"]),
    fn("csv: numbers, booleans, null and CRLF", "ConvertTo-SmCsvLine", Fields=[1, True, None, "x\r\n\r\ny"]),
    # ---- the start phase
    flow("start: the Aspire in AHCI, OK on the explanation, arms and restarts into the firmware", "start", legs=[LEG_AHCI], popups=[1]),
    flow("start: the Aspire in AHCI, Cancel on the explanation stops with nothing changed", "start", legs=[LEG_AHCI], popups=[2]),
    flow("start: the Aspire in AHCI, the explanation times out (-1) counts as OK", "start", legs=[LEG_AHCI], popups=[-1]),
    flow("start: the G16 has no Intel controller: stopped, a popup, nothing armed", "start", facts=G16_FACTS, legs=[LEG_G16], popups=[1]),
    flow("start: a VM on the bench with no prompt arms with raid asked and a plain restart", "start", args={"bench": True, "no_prompt": True}, facts=VM_FACTS, legs=[LEG_VM]),
    flow("start: a VM on the bench with the prompt shows the explanation first", "start", args={"bench": True}, facts=VM_FACTS, legs=[LEG_VM], popups=[1]),
    flow("start: the Aspire in RAID asks for AHCI", "start", legs=[LEG_RAID], popups=[1]),
    flow("start: the firmware refuses shutdown /fw (203): a plain restart and the setup key", "start", legs=[LEG_AHCI], popups=[1],
         native=with_(NATIVE_OK, **{"/r/fw": {"exit": 1, "text": "The system cannot find the path specified.(203)"}})),
    flow("start: BitLocker on refuses before anything is changed", "start", facts=dict(ASPIRE_FACTS, BitLocker="on"), task_present=False),
    flow("start: not elevated refuses", "start", elevated=False, task_present=False),
    flow("start: a test already in progress refuses; the catch cleans that one up", "start", state=state("armed-1", [LEG1_AHCI], "raid", fw=[FW1]), task_present=True, stick_folder=True),
    flow("start: the facts cannot be read", "start", facts={"error": "No MSFT_Volume objects found with property 'DriveLetter' equal to 'E'. Verify the value of the property and retry."}, task_present=False),
    flow("start: the scanner leaves no report: stopped with an error, nothing armed", "start", legs=[LEG_NO_REPORT], task_present=False),
    flow("start: the scanner leaves no capture", "start", legs=[LEG_NO_CAPTURE], task_present=False),
    flow("start: bcdedit /copy fails after the task is registered: cleanup removes the task", "start", legs=[LEG_AHCI], popups=[1],
         native=with_(NATIVE_OK, **{"/copy": {"exit": 1, "text": "The boot configuration data store could not be opened. Access is denied."}})),
    flow("start: bcdedit /copy prints no GUID", "start", legs=[LEG_AHCI], popups=[1], native=with_(NATIVE_OK, **{"/copy": {"exit": 0, "text": "The entry was successfully copied."}})),
    flow("start: bcdedit /set safeboot fails: the copy is deleted again", "start", legs=[LEG_AHCI], popups=[1], native=with_(NATIVE_OK, **{"/set": {"exit": 1, "text": "An error has occurred setting the element data."}})),
    flow("start: the copied entry does not show safeboot Minimal: deleted again", "start", legs=[LEG_AHCI], popups=[1], native=with_(NATIVE_OK, **{"/enum" + GUID: {"exit": 0, "text": f"identifier {GUID}\ndescription copy"}})),
    flow("start: bcdedit /displayorder /remove fails", "start", legs=[LEG_AHCI], popups=[1], native=with_(NATIVE_OK, **{"/displayorder": {"exit": 1, "text": "The parameter is incorrect."}})),
    flow("start: bcdedit /bootsequence fails", "start", legs=[LEG_AHCI], popups=[1], native=with_(NATIVE_OK, **{"/bootsequence": {"exit": 1, "text": "The parameter is incorrect."}})),
    flow("start: the bootsequence is not visible on {bootmgr} after setting it", "start", legs=[LEG_AHCI], popups=[1], native=NATIVE_NO_SEQUENCE),
    flow("start: the state directory still grants write access: refused before the task", "start", legs=[LEG_AHCI], popups=[1], protect_error="the state directory still grants write access to non-administrators; refusing to register a SYSTEM task over it", task_present=False),
    flow("start: the resume task cannot be registered", "start", legs=[LEG_AHCI], popups=[1], register_task_error="the resume task registered as 'ASPIRE\\person', not SYSTEM; removed again", task_present=False),
    flow("start: both shutdowns fail: stopped with an error after arming, everything undone", "start", legs=[LEG_AHCI], popups=[1],
         native=with_(NATIVE_OK, **{"/r/fw": {"exit": 1, "text": "Access is denied.(5)"}, "/r": {"exit": 1, "text": "Access is denied.(5)"}})),
    # ---- the resume phase
    flow("resume: in Safe Mode (option 1) as SYSTEM: the boot is recorded and the machine restarts", "resume", state=state("armed-1", [LEG1_AHCI], "raid", fw=[FW1]), safe_mode=1, context=SAFE_CTX),
    flow("resume: in Safe Mode with networking (option 2), as a person", "resume", state=state("armed-1", [LEG1_AHCI], "raid", fw=[FW1]), safe_mode=2, context=dict(LAUNCHER_CTX, UptimeSeconds=60)),
    flow("resume: nothing to resume (no state): the task goes", "resume", context=SYSTEM_CTX),
    flow("resume: leg 2 sees raid after ahci: changed, re-armed asking for ahci back", "resume", state=state("armed-1", [LEG1_AHCI], "raid", fw=[FW1]), context=SYSTEM_CTX, marker=["2026-10-14T11:53:00.0000000Z|ASPIRE\\person|1|35"], legs=[LEG_RAID]),
    flow("resume: leg 2 sees ahci again: mode-unchanged, finished, notice queued", "resume", state=state("armed-1", [LEG1_AHCI], "raid", fw=[FW1]), context=SYSTEM_CTX, marker=["2026-10-14T11:53:00.0000000Z|ASPIRE\\person|1|35"], legs=[LEG_AHCI]),
    flow("resume: leg 2 without a marker (nobody signed in at the Safe Mode screen)", "resume", state=state("armed-1", [LEG1_AHCI], "raid", fw=[FW1]), context=SYSTEM_CTX, legs=[LEG_AHCI]),
    flow("resume: leg 3 sees ahci back: restored", "resume", state=state("armed-2", [LEG1_AHCI, LEG2_RAID], "ahci", resumes=[RESUME1], safe_boots=[SAFE1], fw=[FW1, dict(FW1, Ask="ahci")], last="changed"), context=dict(SYSTEM_CTX, UptimeSeconds=25), marker=["2026-10-14T11:58:00.0000000Z|ASPIRE\\person|1|33"], legs=[LEG_AHCI]),
    flow("resume: leg 3 still raid: not-restored", "resume", state=state("armed-2", [LEG1_AHCI, LEG2_RAID], "ahci", resumes=[RESUME1], safe_boots=[SAFE1], fw=[FW1, dict(FW1, Ask="ahci")], last="changed"), context=dict(SYSTEM_CTX, UptimeSeconds=25), legs=[LEG_RAID]),
    flow("resume: leg 3 on the bench: a VM stays none both times", "resume", state=state("armed-1", [leg(1, "initial", "none", [], "ok", "standard AHCI / NVMe - visible to Linux installers", "YELLOW")], "raid", bench=True, facts=VM_FACTS, fw=[dict(FW1, Method="plain-bench")]), context=SYSTEM_CTX, facts=VM_FACTS, legs=[LEG_VM]),
    flow("resume: the stick is not there after the wait: a notice, the task stays", "resume", state=state("armed-1", [LEG1_AHCI], "raid", fw=[FW1]), context=SYSTEM_CTX, stick=None),
    flow("resume: the stick is not there and the resume is attended: a popup instead", "resume", state=state("armed-1", [LEG1_AHCI], "raid", fw=[FW1]), context=LAUNCHER_CTX, stick=None, popups=[1]),
    flow("resume: the state is at stage start, not armed: unexpected-stage, cleaned up", "resume", state=state("start", [LEG1_AHCI], None, safe_entry=False), context=SYSTEM_CTX),
    flow("resume: the state is already done: unexpected-stage", "resume", state=state("done:restored", [LEG1_AHCI, LEG2_RAID, LEG1_AHCI], "ahci"), context=SYSTEM_CTX),
    flow("resume: attended (a person, session 1): the finish shows a popup instead of a notice", "resume", state=state("armed-1", [LEG1_AHCI], "raid", fw=[FW1]), context=LAUNCHER_CTX, legs=[LEG_AHCI], popups=[1]),
    flow("resume: the scanner leaves no report: stopped with an error, cleaned up, notice queued", "resume", state=state("armed-1", [LEG1_AHCI], "raid", fw=[FW1]), context=SYSTEM_CTX, legs=[LEG_NO_REPORT]),
    flow("resume: the bootsequence was consumed (not on {bootmgr}): cleanup says not-set", "resume", state=state("armed-1", [LEG1_AHCI], "raid", fw=[FW1]), context=SYSTEM_CTX, legs=[LEG_AHCI], native=NATIVE_NO_SEQUENCE),
    flow("resume: the safe RunOnce is still there at cleanup and does not go", "resume", state=state("armed-1", [LEG1_AHCI], "raid", fw=[FW1]), context=SYSTEM_CTX, legs=[LEG_AHCI], runonce={"*upgrade_storage-mode-safe": "sticky"}),
    flow("resume: the second arm's shutdown /fw is refused: plain restart", "resume", state=state("armed-1", [LEG1_AHCI], "raid", fw=[FW1]), context=SYSTEM_CTX, legs=[LEG_RAID],
         native=with_(NATIVE_OK, **{"/r/fw": {"exit": 1, "text": "The system cannot find the path specified.(203)"}})),
    flow("resume: the state has no SafeEntry (lost) at leg 2 changed: a new copy is armed", "resume", state=state("armed-1", [LEG1_AHCI], "raid", safe_entry=False, fw=[FW1]), context=SYSTEM_CTX, legs=[LEG_RAID]),
    flow("resume: a notice that cannot be queued (registry refused) is not-queued", "resume", state=state("armed-1", [LEG1_AHCI], "raid", fw=[FW1]), context=SYSTEM_CTX, legs=[LEG_AHCI], runonce={"__refuse__": "upgrade_storage-mode-notice"}),
    # ---- abort and notify
    flow("abort: a test in progress is cleaned up and the state moved aside", "abort", state=state("armed-1", [LEG1_AHCI], "raid", fw=[FW1]), runonce={"*upgrade_storage-mode-safe": "x", "upgrade_storage-mode-notice": "y"}),
    flow("abort: nothing in progress removes the task and the RunOnce if present", "abort", task_present=True),
    flow("abort: nothing in progress and no task", "abort", task_present=False),
    flow("abort: not elevated refuses", "abort", elevated=False, state=state("armed-1", [LEG1_AHCI], "raid", fw=[FW1])),
    flow("abort: a done state is cleaned up again (nothing to remove)", "abort", state=state("done:restored", [LEG1_AHCI, LEG2_RAID, LEG1_AHCI], "ahci"), native=NATIVE_NO_SEQUENCE, task_present=False),
    flow("notify: the queued notice is shown and removed", "notify", state_files={"notice.json": {"title": "upgrade_ - storage-mode test", "text": "Storage-mode test finished: restored.\n\nBoth modes were scanned.", "buttons": 64, "queued_utc": "2026-10-14T11:59:00.0000000Z"}}, popups=[1]),
    flow("notify: no notice, nothing shown", "notify"),
]

out = pathlib.Path(__file__).with_name("storage-mode-cases.json")
out.write_text(json.dumps(cases, indent=1) + "\n", encoding="utf-8")
print(f"{out}: {len(cases)} cases ({sum(c['kind'] == 'fn' for c in cases)} functions, {sum(c['kind'] == 'flow' for c in cases)} flows)")
