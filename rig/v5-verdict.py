#!/usr/bin/env python3
"""
v5-verdict.py - one row of docs/validation-results/v5-controller-mode.csv
from a machine's own evidence: the scanner's JSON report and the -DumpMachine
capture that RUN-SCANNER.cmd left on the stick in one SATA mode. Never by
hand.

    v5-verdict.py --from-run <upgrade_/storage-mode dir> [<csv>] [--note "..."]
    v5-verdict.py --sata-mode-set raid|ahci|absent <report.json> <capture.json> [<csv>] [--note "..."]

--from-run is the one-click path (RUN-STORAGE-MODE.cmd / Test-StorageMode.ps1):
it reads the harness's record storage-mode.json and writes one row per leg,
the mode-as-set being what the harness ASKED the person to set on the setup
screen ('initial' for leg 1) - no operator word at all. The flow columns
(flow_result, resume_run_as, safe_boot, fw_reboot) come from the record.

The two-file form is for a plain RUN-SCANNER.cmd run. Its one operator input
is the mode the firmware setup was set to before the
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

HARNESS = "v5-verdict 0.2.0"
HEADER = ["timestamp", "harness", "vendor", "model", "firmware", "os", "scanner_version",
          "leg", "sata_mode_set", "controller", "pci_id", "class_code", "compatible_ids", "driver_service",
          "check_status", "check_detail", "verdict", "result", "flow_result", "resume_run_as", "safe_boot", "fw_reboot", "notes"]
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


def one_row(report_path, capture_path, mode_set, leg="", flow=None, note="", writer=None):
    """Derive one row from a report + capture pair (plus the harness's flow
    record when there is one). Returns (row, result)."""
    notes, errors = [], []
    try:
        rep = load(report_path)
    except Exception as e:
        rep, errors = {}, errors + [f"report unreadable: {e}"]
    try:
        cap = load(capture_path)
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
    seen_raid = any("0104" in c["codes"] for c in ctrls)
    seen_vmd_svc = any(c["service"].lower().startswith("iastorvd") for c in ctrls)
    seen_rst_svc = any(c["service"].lower().startswith("iastor") for c in ctrls)
    codes_seen = sorted({cc for c in ctrls for cc in c["codes"] if cc.startswith("01")})

    if errors:
        result = "error"
        notes += errors
    elif cap and not ctrls:
        result = "no-intel-controller"
        notes.append("no Intel storage-class PCI controller in the capture - the SATA-mode test does not apply to this machine (AMD, or a VM); the check's own status is recorded, the row is not V5 evidence")
    elif mode_set == "absent":
        result = "mode-mismatch" if seen_raid else "option-absent"
        notes.append("firmware setup exposes no SATA-mode option (operator); only this mode exists on this machine")
    elif mode_set == "raid" and not (seen_raid or seen_vmd_svc):
        result = "mode-mismatch"
        notes.append("asked/set to RAID but no controller declares class 0104 and no iaStorVD service is bound - the setting did not take, or this is the wrong file")
    elif mode_set == "ahci" and (seen_raid or seen_vmd_svc):
        result = "mode-mismatch"
        notes.append("asked/set to AHCI but a controller declares class 0104 / iaStorVD is bound - the setting did not take, or this is the wrong file")
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
    flow = flow or {}
    if flow:
        notes.append(flow.get("note", ""))
    if note:
        notes.append("operator: " + note.strip())

    j = lambda key: "; ".join(c[key] for c in ctrls)
    row = [rep.get("ScannedUtc") or datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
           HARNESS, vendor, model, firmware, osver, scanner, leg, mode_set,
           j("name"), j("pci_id"),
           "; ".join(" ".join(f"{cc} ({CLASS_WORD.get(cc, '?')})" for cc in c["codes"] if cc.startswith("01")) or "none" for c in ctrls),
           " | ".join(",".join(c["compat"]) for c in ctrls),
           j("service"), status, detail, verdict, result,
           flow.get("flow_result", "single-run"), flow.get("resume_run_as", ""), flow.get("safe_boot", ""), flow.get("fw_reboot", ""),
           " | ".join(n for n in notes if n)]
    return row, result


def run_rows(run_dir, note):
    """The one-click path: every leg in the harness's record."""
    rec = load(os.path.join(run_dir, "storage-mode.json"))
    legs = rec.get("legs") or []
    if not legs:
        sys.exit("v5-verdict: the record has no legs")
    flow_result = rec.get("flow_result", "")
    resumes = rec.get("resumes") or []
    safe_boots = rec.get("safe_boots") or []
    fw = rec.get("fw_reboots") or []
    cleanup = rec.get("cleanup") or {}
    out = []
    for i, leg in enumerate(legs):
        n = int(leg.get("N") or i + 1)
        d = os.path.join(run_dir, f"leg{n}")
        rp = os.path.join(d, leg.get("ReportFile") or "")
        cp = os.path.join(d, leg.get("CaptureFile") or "machine-capture.json")
        asked = (leg.get("Asked") or "initial").lower()
        # the resume that produced this leg (legs 2+), the Safe Mode boot before it, the setup restart before that
        res = resumes[n - 2] if n >= 2 and len(resumes) >= n - 1 else None
        sb = safe_boots[n - 2] if n >= 2 and len(safe_boots) >= n - 1 else None
        fwr = fw[n - 2] if n >= 2 and len(fw) >= n - 1 else None
        run_as = "SYSTEM" if res and re.search(r"(?i)(^|\\)SYSTEM$", str(res.get("RunAs", ""))) else ("user" if res else "launcher")
        if res and res.get("Unattended") is not None:
            run_as += " unattended" if res.get("Unattended") else " attended"
        flow = {"flow_result": flow_result, "resume_run_as": run_as,
                "safe_boot": ("y (%s, session %s)" % ("SYSTEM" if re.search(r"(?i)SYSTEM$", str(sb.get("RunAs", ""))) else "user", sb.get("SessionId"))) if sb else ("n" if n >= 2 else ""),
                "fw_reboot": (fwr.get("Method") or "") if fwr else "",
                "note": f"harness={rec.get('harness_version', '')} stage={rec.get('stage', '')} leg_mode_seen={leg.get('Mode', '')} "
                        f"cleanup={','.join(f'{k}={v}' for k, v in cleanup.items()) or 'none'}" + (f" bench=y" if (rec.get('facts') or {}).get('Bench') or 'Virtual' in str((rec.get('facts') or {}).get('Model', '')) else "")}
        row, result = one_row(rp, cp, asked if asked in ("raid", "ahci", "initial") else "initial", leg=str(n), flow=flow, note=note if i == len(legs) - 1 else "")
        out.append((row, result))
    return out


def append(csv_path, rows):
    out = pathlib.Path(csv_path)
    new = not out.exists()
    with open(out, "a", newline="", encoding="utf-8") as f:
        w = csv.writer(f, quoting=csv.QUOTE_ALL, lineterminator="\n")
        if new:
            w.writerow(HEADER)
        for r in rows:
            w.writerow(r)
    return out


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--from-run", metavar="DIR", help="the stick's upgrade_/storage-mode folder (one-click path): one row per leg")
    ap.add_argument("--sata-mode-set", choices=["raid", "ahci", "absent"],
                    help="two-file path: the SATA mode the firmware setup was set to for this run; 'absent' = no such option exists")
    ap.add_argument("files", nargs="*", help="two-file path: <report.json> <capture.json> [<csv>]; one-click path: [<csv>]")
    ap.add_argument("--note", default="", help="operator's words, appended to notes with an 'operator:' prefix")
    a = ap.parse_args()

    if a.from_run:
        csv_path = a.files[0] if a.files else str(DEFAULT_CSV)
        pairs = run_rows(a.from_run, a.note)
        out = append(csv_path, [r for r, _ in pairs])
        results = [res for _, res in pairs]
        for r, res in pairs:
            print(f"v5-verdict: leg {r[7]} set={r[8]} classes={r[11] or '-'} service={r[13] or '-'} check={r[14]} verdict={r[16]} -> {res}")
        print(f"v5-verdict: flow={pairs[0][0][18]} -> {out}")
        sys.exit(0 if all(res in ("fail-fired", "ok-passed", "warn-rst-on-ahci") for res in results) else 1)

    if not a.sata_mode_set or len(a.files) < 2:
        ap.error("either --from-run DIR, or --sata-mode-set MODE <report.json> <capture.json>")
    csv_path = a.files[2] if len(a.files) > 2 else str(DEFAULT_CSV)
    row, result = one_row(a.files[0], a.files[1], a.sata_mode_set, note=a.note)
    out = append(csv_path, [row])
    print(f"v5-verdict: {result} (set={a.sata_mode_set} classes={row[11] or '-'} service={row[13] or '-'} check={row[14]} verdict={row[16]}) -> {out}")
    sys.exit(0 if result in ("fail-fired", "ok-passed", "warn-rst-on-ahci") else 1)


if __name__ == "__main__":
    main()
