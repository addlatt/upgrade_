#!/usr/bin/env python3
"""
v2-verdict.py - one row of docs/validation-results/v2-install.csv from the
run's own evidence: the offline disk inspections (pre-install, post-install,
post-cycles: GPT + ESP manifest, v1b-inspect.py), outcome.json the converter
wrote to the stick (validated against schemas/outcome.schema.json), the
boot markers both OSes left on the stick (boots.log), and the V0 harness
row. Never by hand.

    v2-verdict.py <artifacts dir> <csv> <harness version> <firmware>
"""
import csv, json, sys, datetime, pathlib
A = pathlib.Path(sys.argv[1]); CSV = pathlib.Path(sys.argv[2]); HARNESS = sys.argv[3]; FIRMWARE = sys.argv[4]
ROOT = pathlib.Path(__file__).resolve().parents[2]
HEADER = ["timestamp", "harness", "firmware", "secureboot", "path", "desktop", "handoff_result", "install_done",
          "outcome_valid", "esp_size_mib", "esp_free_before", "esp_free_after", "esp_added_bytes", "bootmgfw_intact",
          "microsoft_files_changed", "fallback_loader", "snapshot_files", "windows_entry_present", "linux_first",
          "grub_lists_windows", "windows_boots", "linux_boots", "result", "notes"]
WIN_EXCLUDE = ("BCD", "BCD.LOG", "BCD.LOG1", "BCD.LOG2", "BOOTSTAT.DAT")
def windows_rewrites(name):
    # Windows rewrites these on every boot: the BCD store and its logs, the
    # boot status file, and BitLocker's TCG event log (FveTcg_N.log) -
    # seen changing across a plain Windows boot on the rig, 2026-09-10
    return name in WIN_EXCLUDE or (name.startswith("FveTcg_") and name.endswith(".log"))

def load(p):
    try: return json.load(open(A / p, encoding="utf-8-sig"))
    except Exception: return None

pre, post, cyc = load("pre-install.json"), load("post-install.json"), load("post-cycles.json")
outcome = load("outcome.json")
verify = load("verify.json")
notes = []
v0 = None
if (A / "v0-handoff.csv").exists():
    try:
        rows = [r for r in csv.DictReader(open(A / "v0-handoff.csv", encoding="utf-8-sig")) if r.get("result")]
        v0 = rows[-1] if rows else None
    except Exception: pass
handoff = v0["result"] if v0 else "no-row"
sb = v0["secureboot"] if v0 else "unknown"

# --- outcome.json: present, valid, completed ---------------------------------
outcome_valid = "n"; install_done = "n"; bc = {}
if outcome:
    try:
        from jsonschema import Draft7Validator, FormatChecker
        schema = json.load(open(ROOT / "schemas/outcome.schema.json"))
        errs = list(Draft7Validator(schema, format_checker=FormatChecker()).iter_errors(outcome))
        outcome_valid = "y" if not errs else "n"
        if errs: notes.append("outcome.json schema: " + "; ".join(f"{'/'.join(map(str,e.path))}: {e.message[:80]}" for e in errs[:3]))
    except ImportError:
        notes.append("jsonschema not installed; outcome.json not validated")
    install_done = "y" if outcome.get("status") == "completed" else "n"
    bc = (outcome.get("cutover") or {}).get("boot_chain") or {}
    notes.append(f"outcome: status={outcome.get('status')} path={outcome.get('path_taken')} kernel={((outcome.get('cutover') or {}).get('install') or {}).get('kernel')} reachable_via={(outcome.get('windows') or {}).get('reachable_via')}")
else:
    notes.append("no outcome.json on the stick")
path = (outcome or {}).get("path_taken") or "keep-windows"
desktop = "kde"
try:
    desktop = json.load(open(A / "job.json"))["intent"]["desktop"]
except Exception: pass

# --- ESP: before vs after -----------------------------------------------------
def esp(rec): return (rec or {}).get("esp") or {}
e0, e1, e2 = esp(pre), esp(post), esp(cyc)
esp_size = e0.get("size_mib", "")
free_b, free_a = e0.get("fat_bytes_free", ""), e1.get("fat_bytes_free", "")
m0, m1 = e0.get("manifest", {}), e1.get("manifest", {})
added = sum(v["size"] for k, v in m1.items() if k not in m0)
bootmgfw_ok = "n"
shas = {rec.get("esp", {}).get("bootmgfw_sha256") for rec in (pre, post, cyc) if rec}
shas.discard(None)
if e0.get("bootmgfw_sha256") and len(shas) == 1: bootmgfw_ok = "y"
ms_changed = sorted(k for k in m0 if k.startswith("/EFI/Microsoft/") and not windows_rewrites(k.split("/")[-1])
                    and (k not in m1 or m1[k]["sha256"] != m0[k]["sha256"]))
other_changed = sorted(k for k in m0 if not k.startswith("/EFI/Microsoft/") and (k not in m1 or m1[k]["sha256"] != m0[k]["sha256"]))
fallback = bc.get("fallback_loader", "unreported")
snap_files = ((verify or {}).get("esp_snapshot") or {}).get("files", 0)
if other_changed: notes.append("pre-existing non-Microsoft ESP files changed: " + ",".join(other_changed))
notes.append(f"install added {len([k for k in m1 if k not in m0])} files / {added} B to the ESP")

# --- boots ----------------------------------------------------------------------
wb = lb = 0
if (A / "boots.log").exists():
    seen_done = False
    for line in open(A / "boots.log", encoding="utf-8-sig", errors="replace"):
        line = line.strip()
        if line.startswith("install-done"): seen_done = True; continue
        if not seen_done: continue
        if line.startswith("linux-boot"): lb += 1
        if line.startswith("windows-boot"): wb += 1
notes.append(f"boots after install-done: windows={wb} linux={lb}")
if v0: notes.append("v0 row: " + v0["notes"][:120])

def yn(b): return "y" if b else "n"
win_present, lin_first, grub_win = yn(bc.get("windows_entry_present")), yn(bc.get("linux_first_in_bootorder")), yn(bc.get("grub_lists_windows"))

# the converter puts Fedora first in BootOrder on purpose, so the V0 harness's
# return check sees a permanent reorder: 'reordered' is the EXPECTED V0 result
# for this leg, 'fired-once' means the firmware ignored the new order
if handoff not in ("fired-once", "reordered") and not outcome: result = "handoff-failed"
elif not outcome or install_done != "y": result = "install-failed"
elif outcome_valid != "y": result = "outcome-invalid"
elif ms_changed or bootmgfw_ok != "y": result = "windows-files-changed"; notes.append("Microsoft files changed: " + ",".join(ms_changed))
elif e1 and free_a not in ("", None) and int(free_a) < 0: result = "esp-full"
elif wb < 1 or grub_win != "y": result = "windows-unbootable-via-grub"
elif lb < 1: result = "linux-unbootable"
elif fallback != "shim" or snap_files < 1: result = "fallback-loader-unrecorded"
elif wb < 2 or lb < 2: result = "cycles-incomplete"
else: result = "pass-plumbing"

row = [datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"), HARNESS, FIRMWARE, sb, path, desktop, handoff,
       install_done, outcome_valid, esp_size, free_b, free_a, added, bootmgfw_ok, ",".join(ms_changed) or "none", fallback, snap_files,
       win_present, lin_first, grub_win, wb, lb, result, " | ".join(notes)]
new = not CSV.exists()
with open(CSV, "a", newline="", encoding="utf-8") as f:
    w = csv.writer(f, quoting=csv.QUOTE_ALL, lineterminator="\n")
    if new: w.writerow(HEADER)
    w.writerow(row)
print(f"v2-verdict: {result} (install_done={install_done} outcome_valid={outcome_valid} bootmgfw={bootmgfw_ok} fallback={fallback} wb={wb} lb={lb}) -> {CSV}")
sys.exit(0 if result == "pass-plumbing" else 1)
