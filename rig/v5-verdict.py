#!/usr/bin/env python3
"""
v5-verdict.py - one row of docs/validation-results/v5-controller-mode.csv
from a machine's own evidence: the scanner's JSON report and the -DumpMachine
capture that RUN-SCANNER.cmd left on the stick in one SATA mode. Never by
hand.

    v5-verdict.py --sata-mode-set raid|ahci|absent <report.json> <capture.json> [<csv>] [--note "..."]

The one operator input is the mode the firmware setup was set to before the
run (`raid` = Intel RST Premium / Optane / RAID, `ahci` = AHCI, `absent` =
the setup exposes no SATA-mode option and this is the only mode the machine
has). Everything else in the row is read from the two files, and the row's
`result` compares the operator's word against the PCI class code the
controller actually declared, so a mislabelled run shows up as
`mode-mismatch` rather than as evidence. The row is appended to the CSV
(default: docs/validation-results/v5-controller-mode.csv beside this repo).

Exit status: 0 for the two rows V5 needs (`fail-fired`, `ok-passed`), also 0
for `warn-rst-on-ahci` (the R7 branch, a correct verdict); 1 for anything the
check got wrong (`missed`, `false-fail`) or that cannot count
(`mode-mismatch`, `option-absent`, `error`).
"""
import argparse, csv, datetime, json, os, pathlib, re, sys

HARNESS = "v5-verdict 0.1.0"
HEADER = ["timestamp", "harness", "vendor", "model", "firmware", "os", "scanner_version",
          "sata_mode_set", "controller", "pci_id", "class_code", "compatible_ids", "driver_service",
          "check_status", "check_detail", "verdict", "result", "notes"]
CLASS_WORD = {"0104": "RAID", "0106": "AHCI", "0108": "NVMe", "0100": "SCSI", "0101": "IDE", "0180": "other-storage"}
REPO = pathlib.Path(__file__).resolve().parent.parent
DEFAULT_CSV = REPO / "docs" / "validation-results" / "v5-controller-mode.csv"


def evict_page_cache(path):
    # WSL's /mnt 9p cache can serve stale pages of files Windows rewrote
    # (proven 2026-08-30); a stick re-plugged at the same letter is exactly
    # that case. Harmless elsewhere.
    try:
        fd = os.open(path, os.O_RDONLY)
        os.posix_fadvise(fd, 0, 0, os.POSIX_FADV_DONTNEED)
        os.close(fd)
    except (OSError, AttributeError):
        pass


def load(path):
    evict_page_cache(path)
    with open(path, encoding="utf-8-sig") as f:
        return json.load(f)


def pci_id(device_id):
    m = re.search(r"VEN_([0-9A-Fa-f]{4})&DEV_([0-9A-Fa-f]{4})", device_id or "")
    return f"{m.group(1).lower()}:{m.group(2).lower()}" if m else ""


def class_code(compatible_ids):
    # Windows publishes the PCI class in CompatibleIDs as PCI\CC_ccss[pp];
    # take the class+subclass (4 hex digits) of the most specific entry.
    codes = []
    for c in compatible_ids or []:
        m = re.search(r"CC_([0-9A-Fa-f]{4})", c or "")
        if m and m.group(1).upper() not in codes:
            codes.append(m.group(1).upper())
    return codes


def storage_controllers(pnp):
    """Intel PCI devices that are mass-storage class (CC_01xx) or carry an
    Intel RST service. This is what the scanner's check reads; the row
    records it verbatim so a reader can see what Windows saw."""
    out = []
    for d in pnp:
        did = d.get("DeviceID") or ""
        if not did.startswith("PCI\\"):
            continue
        pid = pci_id(did)
        codes = class_code(d.get("CompatibleID"))
        svc = d.get("Service") or ""
        is_storage = any(c.startswith("01") for c in codes) or (d.get("PNPClass") in ("HDC", "SCSIAdapter"))
        if pid.startswith("8086:") and (is_storage or svc.lower().startswith("iastor")):
            out.append({"name": d.get("Name") or "", "pci_id": pid, "codes": codes, "service": svc,
                        "compat": [c for c in (d.get("CompatibleID") or []) if c]})
    return out


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--sata-mode-set", required=True, choices=["raid", "ahci", "absent"],
                    help="the SATA mode the firmware setup was set to for this run; 'absent' = no such option exists")
    ap.add_argument("report", help="upgrade-report-*.json from the run")
    ap.add_argument("capture", help="machine-capture*.json from the same run")
    ap.add_argument("csv", nargs="?", default=str(DEFAULT_CSV))
    ap.add_argument("--note", default="", help="operator's words, appended to notes with an 'operator:' prefix")
    a = ap.parse_args()

    notes, errors = [], []
    try:
        rep = load(a.report)
    except Exception as e:
        rep, errors = {}, errors + [f"report unreadable: {e}"]
    try:
        cap = load(a.capture)
    except Exception as e:
        cap, errors = {}, errors + [f"capture unreadable: {e}"]

    sysr = rep.get("System") or {}
    sysc = cap.get("Sys") or {}
    vendor = sysr.get("Vendor") or sysc.get("Vendor") or ""
    model = sysr.get("Model") or sysc.get("Model") or ""
    if sysr and sysc and (sysr.get("Vendor"), sysr.get("Model")) != (sysc.get("Vendor"), sysc.get("Model")):
        errors.append(f"report and capture are from different machines: report={sysr.get('Vendor')} {sysr.get('Model')}, "
                      f"capture={sysc.get('Vendor')} {sysc.get('Model')}")
    if cap.get("Synthetic"):
        errors.append("capture is marked Synthetic - a spoof is plumbing, not a V5 row (CLAUDE.md rule #5)")

    firmware = sysr.get("BiosVersion") or ""
    osver = " ".join(str(x) for x in (sysr.get("OsCaption"), sysr.get("OsBuild")) if x)
    scanner = rep.get("ScannerVersion") or ""
    if rep and not rep.get("RanAsAdmin", True):
        errors.append("scanner did not run elevated - storage enumeration is incomplete unelevated")

    check = next((c for c in rep.get("Checks") or [] if c.get("Title") == "Storage controller mode"), None)
    status = (check or {}).get("Status") or ("(no check emitted)" if rep else "")
    detail = (check or {}).get("Detail") or ""
    verdict = ((rep.get("Verdict") or {}).get("Level")) or ""

    ctrls = storage_controllers(cap.get("Pnp") or [])
    if cap and not ctrls:
        errors.append("no Intel storage-class PCI controller in the capture")
    seen_raid = any("0104" in c["codes"] for c in ctrls)
    seen_vmd_svc = any(c["service"].lower().startswith("iastorvd") for c in ctrls)
    seen_rst_svc = any(c["service"].lower().startswith("iastor") for c in ctrls)
    codes_seen = sorted({cc for c in ctrls for cc in c["codes"] if cc.startswith("01")})

    if errors:
        result = "error"
        notes += errors
    elif a.sata_mode_set == "absent":
        result = "mode-mismatch" if seen_raid else "option-absent"
        notes.append("firmware setup exposes no SATA-mode option (operator); only this mode exists on this machine")
    elif a.sata_mode_set == "raid" and not (seen_raid or seen_vmd_svc):
        result = "mode-mismatch"
        notes.append("set to RAID but no controller declares class 0104 and no iaStorVD service is bound - the setting did not take, or this is the wrong file")
    elif a.sata_mode_set == "ahci" and (seen_raid or seen_vmd_svc):
        result = "mode-mismatch"
        notes.append("set to AHCI but a controller declares class 0104 / iaStorVD is bound - the setting did not take, or this is the wrong file")
    elif seen_raid or seen_vmd_svc:
        result = "fail-fired" if (status == "fail" and verdict == "RED") else "missed"
        if result == "missed":
            notes.append(f"RAID/VMD is present in the enumeration but the check said '{status}' with verdict '{verdict}' - the check is wrong; fix it, never the row")
    else:
        result = {"ok": "ok-passed", "warn": "warn-rst-on-ahci", "fail": "false-fail"}.get(status, "error")
        if result == "false-fail":
            notes.append("no controller declares RAID class yet the check failed - over-refusal; fix the check")
        if result == "error":
            notes.append(f"unexpected check status '{status}'")

    notes.insert(0, f"scan={rep.get('ScannedUtc', '')} capture={cap.get('Captured', '')} controllers={len(ctrls)} "
                    f"classes={','.join(codes_seen) or 'none'} rst_service={'y' if seen_rst_svc else 'n'} vmd_service={'y' if seen_vmd_svc else 'n'}")
    if a.note:
        notes.append("operator: " + a.note.strip())

    j = lambda key: "; ".join(c[key] for c in ctrls)
    row = [rep.get("ScannedUtc") or datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
           HARNESS, vendor, model, firmware, osver, scanner, a.sata_mode_set,
           j("name"), j("pci_id"),
           "; ".join(" ".join(f"{cc} ({CLASS_WORD.get(cc, '?')})" for cc in c["codes"] if cc.startswith("01")) or "none" for c in ctrls),
           " | ".join(",".join(c["compat"]) for c in ctrls),
           j("service"), status, detail, verdict, result, " | ".join(notes)]

    out = pathlib.Path(a.csv)
    new = not out.exists()
    with open(out, "a", newline="", encoding="utf-8") as f:
        w = csv.writer(f, quoting=csv.QUOTE_ALL, lineterminator="\n")
        if new:
            w.writerow(HEADER)
        w.writerow(row)
    print(f"v5-verdict: {result} (set={a.sata_mode_set} classes={','.join(codes_seen) or 'none'} "
          f"service={j('service') or '-'} check={status} verdict={verdict}) -> {out}")
    sys.exit(0 if result in ("fail-fired", "ok-passed", "warn-rst-on-ahci") else 1)


if __name__ == "__main__":
    main()
