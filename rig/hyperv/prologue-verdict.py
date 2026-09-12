#!/usr/bin/env python3
"""
prologue-verdict.py - one row of docs/validation-results/r18-prologue.csv
from the run's own evidence: the prologue's record on the stick
(upgrade_/prologue.json, its `prologue` block being what outcome.json must
carry), its return record (prologue-return.json, the handoff classified by
the prologue itself on the way back), outcome.json (validated against the
schema, and its prologue block compared with the record), the bench's
fault-injection note (dirty.txt) and the offline GPT inspections before and
after (did C: shrink by exactly what the prologue says it freed). Never by
hand.

    prologue-verdict.py <artifacts dir> <csv> <harness version> <firmware>
"""
import csv, json, sys, datetime, pathlib
A = pathlib.Path(sys.argv[1]); CSV = pathlib.Path(sys.argv[2]); HARNESS = sys.argv[3]; FIRMWARE = sys.argv[4]
ROOT = pathlib.Path(__file__).resolve().parents[2]
HEADER = ["timestamp", "harness", "firmware", "secureboot", "dirty_injected", "revalidated", "scan", "disk_health", "method", "restarts",
          "wininit_1001", "found000", "dirty_after", "remeasured_gb", "remeasured_by", "diskpart_gb", "fork_taken", "requested_bytes",
          "freed_bytes", "partition_shrunk", "hibernation_off", "pagefile_off", "bitlocker_before", "bitlocker_suspended", "armed",
          "handoff_result", "install_done", "outcome_valid", "record_in_outcome", "result", "notes"]

def load(p):
    try: return json.load(open(A / p, encoding="utf-8-sig"))
    except Exception: return None
def yn(b): return "y" if b else "n"

rec, ret, outcome = load("prologue.json"), load("prologue-return.json"), load("outcome.json")
pre, post = load("pre-install.json"), load("post-install.json")
notes = []
dirty_injected = "n"
try:
    if "is Dirty" in open(A / "dirty.txt", encoding="utf-8", errors="replace").read(): dirty_injected = "y"
except Exception: pass

P = (rec or {}).get("prologue") or {}
vc, sh, bl, ho = P.get("volume_check", {}), P.get("shrink", {}), P.get("bitlocker", {}), P.get("handoff", {})
state = (rec or {}).get("state") or {}
sb = (ret or {}).get("secure_boot", "unknown")
rh = (ret or {}).get("handoff") or {}
handoff = rh.get("Result") or rh.get("result") or "no-record"   # the record is the prologue's state (PascalCase); run 2 was misjudged by a lower-case read
if rec: notes.append(f"prologue {rec.get('prologue_version')} stage={rec.get('stage')} restarts={state.get('Restarts')}")
if vc.get("scan") is not None: notes.append(f"scan='{vc.get('scan')}' chkntfs={state.get('VolumeCheck', {}).get('Chkntfs')}")
if vc.get("wininit_1001"): notes.append("wininit 1001: " + " ".join(vc["wininit_1001"].split())[:200])
plan = (state.get("Shrink") or {}).get("Plan") or {}
if plan: notes.append(f"plan target={plan.get('TargetBytes')} shrinkable={plan.get('ShrinkableBytes')} reason='{plan.get('Reason')}' api_error='{(state.get('Shrink') or {}).get('ApiError')}' diskpart_error='{(state.get('Shrink') or {}).get('DiskpartError')}'")

# the partition table's own word on the shrink
shrunk = "unreported"
try:
    def c_size(rec_):
        parts = [p for p in rec_["gpt"]["partitions"] if p["type_guid"] == "ebd0a0a2-b9e5-4433-87c0-68b6b72699c7"]
        return max(p["size_bytes"] for p in parts)
    before, after = c_size(pre), c_size(post)
    delta = before - after
    shrunk = yn(delta == int(sh.get("freed_bytes") or 0) and delta > 0)
    notes.append(f"GPT: C: {before} -> {after} B (delta {delta}; prologue says freed {sh.get('freed_bytes')})")
except Exception as ex:
    notes.append(f"GPT comparison unavailable: {ex}")

# outcome.json: valid, completed, and carrying the prologue's own block
outcome_valid = "n"; install_done = "n"; record_in_outcome = "n"; stopped_at = None
if outcome:
    try:
        from jsonschema import Draft7Validator, FormatChecker
        schema = json.load(open(ROOT / "schemas/outcome.schema.json"))
        errs = list(Draft7Validator(schema, format_checker=FormatChecker()).iter_errors(outcome))
        outcome_valid = yn(not errs)
        if errs: notes.append("outcome.json schema: " + "; ".join(f"{'/'.join(map(str,e.path))}: {e.message[:80]}" for e in errs[:3]))
    except ImportError: notes.append("jsonschema not installed; outcome.json not validated")
    install_done = yn(outcome.get("status") == "completed")
    if outcome.get("status") != "completed": stopped_at = outcome.get("stopped_at"); notes.append(f"outcome stopped at {stopped_at}: {str(outcome.get('reason'))[:160]}")
    record_in_outcome = yn(rec is not None and outcome.get("prologue") == P)
    if rec and outcome.get("prologue") != P:
        diff = [k for k in set(list(P) + list(outcome.get("prologue", {}))) if P.get(k) != outcome.get("prologue", {}).get(k)]
        notes.append("outcome.prologue differs from the record in: " + ",".join(sorted(diff)))
else:
    notes.append("no outcome.json on the stick")

needed, ran = bool(vc.get("needed")), bool(vc.get("ran"))
if rec is None: result = "prologue-not-run"
elif stopped_at: result = f"stopped-{stopped_at}"
elif dirty_injected == "y" and not needed: result = "flag-not-confirmed"
elif needed and not ran: result = "check-not-run"
elif needed and vc.get("dirty_after") != "clean": result = "flag-persists"
elif sh.get("remeasured_gb") is None: result = "not-remeasured"
elif sh.get("fork_taken") != "keep-windows": result = f"fork-{sh.get('fork_taken')}"
elif int(sh.get("freed_bytes") or 0) < int(sh.get("requested_bytes") or 0) or shrunk == "n": result = "shrink-short"
elif not ho.get("armed"): result = "not-armed"
elif handoff not in ("fired-once", "reordered"): result = "handoff-failed"
elif install_done != "y": result = "install-failed"
elif outcome_valid != "y": result = "outcome-invalid"
elif record_in_outcome != "y": result = "record-mismatch"
else: result = "pass-plumbing"

row = [datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"), HARNESS, FIRMWARE, sb, dirty_injected,
       yn(P.get("revalidated")) if rec else "", vc.get("scan", ""), vc.get("disk_health_at_check", ""), vc.get("method", ""), vc.get("restarts", ""),
       yn(vc.get("wininit_1001")), vc.get("found000_present", ""), vc.get("dirty_after", ""), sh.get("remeasured_gb", ""), sh.get("remeasured_by", ""),
       sh.get("remeasured_diskpart_gb", ""), sh.get("fork_taken", ""), sh.get("requested_bytes", ""), sh.get("freed_bytes", ""), shrunk,
       yn(sh.get("hibernation_disabled")), yn(sh.get("pagefile_disabled")), bl.get("status_before", ""), yn(bl.get("suspended")), yn(ho.get("armed")),
       handoff, install_done, outcome_valid, record_in_outcome, result, " | ".join(notes)]
new = not CSV.exists()
with open(CSV, "a", newline="", encoding="utf-8") as f:
    w = csv.writer(f, quoting=csv.QUOTE_ALL, lineterminator="\n")
    if new: w.writerow(HEADER)
    w.writerow(row)
print(f"prologue-verdict: {result} (needed={needed} ran={ran} method={vc.get('method')} dirty_after={vc.get('dirty_after')} remeasured={sh.get('remeasured_gb')} fork={sh.get('fork_taken')} freed={sh.get('freed_bytes')} shrunk={shrunk} handoff={handoff} install={install_done} record={record_in_outcome}) -> {CSV}")
sys.exit(0 if result == "pass-plumbing" else 1)
