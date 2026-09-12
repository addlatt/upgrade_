#!/usr/bin/env python3
"""
rollback-verdict.py - one row of docs/validation-results/r21-rollback.csv
from the run's own evidence: the rollback's record on the stick
(upgrade_/rollback.json, written by Invoke-Rollback.ps1), the offline ESP
manifests before the conversion (pre-install.json: Windows' own fallback
loader), after the boot cycles (post-cycles.json) and after the rollback
(post-rollback.json), and the boot marker the bench wrote when Windows came
up with NO key pressed afterwards (boots.log). Never by hand.

    rollback-verdict.py <artifacts dir> <csv> <harness version> <firmware>
"""
import csv, json, sys, datetime, pathlib
A = pathlib.Path(sys.argv[1]); CSV = pathlib.Path(sys.argv[2]); HARNESS = sys.argv[3]; FIRMWARE = sys.argv[4]
HEADER = ["timestamp", "harness", "firmware", "record", "restored", "loader_matches_snapshot", "windows_first", "windows_direct_boot",
          "linux_partitions_intact", "efi_fedora_intact", "result", "notes"]
def load(p):
    try: return json.load(open(A / p, encoding="utf-8-sig"))
    except Exception: return None
def yn(b): return "y" if b else "n"
rec, pre, cyc, post = load("rollback.json"), load("pre-install.json"), load("post-cycles.json"), load("post-rollback.json")
notes = []
fb = (rec or {}).get("fallback_loader") or {}; bo = (rec or {}).get("boot_order") or {}
restored = yn(fb.get("restored")); win_first = yn(bo.get("windows_first"))
if rec: notes.append(f"rollback {rec.get('rollback_version')}: sha {str(fb.get('sha_before'))[:12]} -> {str(fb.get('sha_after'))[:12]} (snapshot {str(fb.get('snapshot_sha'))[:12]}); order {' '.join(bo.get('before', []))} -> {' '.join(bo.get('after', []))}")
def man(r): return ((r or {}).get("esp") or {}).get("manifest") or {}
def fbkey(m):
    for k in m:
        if k.lower() == "/efi/boot/bootx64.efi": return k
    return None
loader_ok = "unreported"
try:
    m0, m2 = man(pre), man(post)
    k0, k2 = fbkey(m0), fbkey(m2)
    loader_ok = yn(k0 and k2 and m0[k0]["sha256"] == m2[k2]["sha256"])
    notes.append(f"fallback loader after rollback {m2[k2]['sha256'][:12] if k2 else 'missing'} vs Windows' pre-conversion {m0[k0]['sha256'][:12] if k0 else 'missing'}")
except Exception as ex: notes.append(f"ESP comparison unavailable: {ex}")
parts_ok = "unreported"; fedora_ok = "unreported"
try:
    g1 = [(p["index"], p["type_guid"], p["start_lba"], p["end_lba"]) for p in cyc["gpt"]["partitions"]]
    g2 = [(p["index"], p["type_guid"], p["start_lba"], p["end_lba"]) for p in post["gpt"]["partitions"]]
    parts_ok = yn(g1 == g2)
    m1, m2 = man(cyc), man(post)
    f1 = {k: v["sha256"] for k, v in m1.items() if k.lower().startswith("/efi/fedora/")}
    f2 = {k: v["sha256"] for k, v in m2.items() if k.lower().startswith("/efi/fedora/")}
    fedora_ok = yn(f1 == f2 and len(f1) > 0)
    notes.append(f"GPT {len(g1)} partitions unchanged={g1 == g2}; EFI/fedora {len(f1)} files unchanged={f1 == f2}")
except Exception as ex: notes.append(f"GPT/fedora comparison unavailable: {ex}")
direct = "n"
try:
    for line in open(A / "boots.log", encoding="utf-8-sig", errors="replace"):
        if line.startswith("windows-boot") and "direct-after-rollback" in line: direct = "y"
except Exception: pass
if rec is None: result = "record-missing"
elif restored != "y": result = "not-restored"
elif loader_ok != "y": result = "loader-mismatch"
elif win_first != "y": result = "windows-not-first"
elif direct != "y": result = "windows-not-direct"
elif parts_ok != "y": result = "linux-touched"
elif fedora_ok != "y": result = "efi-fedora-touched"
else: result = "pass-plumbing"
row = [datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"), HARNESS, FIRMWARE, yn(rec), restored, loader_ok, win_first, direct, parts_ok, fedora_ok, result, " | ".join(notes)]
new = not CSV.exists()
with open(CSV, "a", newline="", encoding="utf-8") as f:
    w = csv.writer(f, quoting=csv.QUOTE_ALL, lineterminator="\n")
    if new: w.writerow(HEADER)
    w.writerow(row)
print(f"rollback-verdict: {result} (restored={restored} loader={loader_ok} windows_first={win_first} direct={direct} parts={parts_ok} fedora={fedora_ok}) -> {CSV}")
sys.exit(0 if result == "pass-plumbing" else 1)
