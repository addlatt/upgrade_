#!/usr/bin/env python3
"""
v9-verdict.py - one row of docs/validation-results/v9-erase.csv from a V9
arm's own evidence (RISKS R27). Never by hand.

    v9-verdict.py <artifacts/v9> <arm A|B|C> <csv> <firmware> [note]

Reads <dir>/arm-<X>/ (what `v9.sh pull` / `stick-pull` copied) and the
offline disk inspections v9-pre.json and v9-after-<X>.json.
  A refuse: verify.json identity=fail, no countdown.json, disks unchanged -> refused-before-countdown
  B cancel: countdown.json cancelled, outcome.json stopped at countdown and schema-valid,
            disks unchanged -> cancelled-untouched
  C erase:  countdown.json elapsed, outcome.json completed + schema-valid with crossed_utc =
            the countdown's end, system disk = EFI + /boot + root, home disk = one Linux
            filesystem, first Linux boot marker: password fingerprint = the job's hash,
            /home present on the second disk -> erased-installed
Anything else is named, never smoothed over.
"""
import csv, datetime, hashlib, json, pathlib, sys
from jsonschema import Draft7Validator, FormatChecker

A = pathlib.Path(sys.argv[1]); ARM = sys.argv[2].upper(); CSV = pathlib.Path(sys.argv[3]); FIRMWARE = sys.argv[4]
ROOT = pathlib.Path(__file__).resolve().parents[2]
D = A / ("arm-" + ARM)
HEADER = ["timestamp", "arm", "firmware", "prologue_version", "verify_version", "identity", "countdown", "outcome_status",
          "stopped_at", "commit_crossed", "outcome_valid", "disks_unchanged", "system_gpt_after", "home_gpt_after",
          "fedora_booted", "password_matches", "home_on_second_disk", "result", "notes"]

def load(p):
    try: return json.load(open(p, encoding="utf-8-sig"))
    except Exception: return None
def valid(doc, kind):
    if doc is None: return "n/a"
    s = json.load(open(ROOT / "schemas" / (kind + ".schema.json")))
    e = list(Draft7Validator(s, format_checker=FormatChecker()).iter_errors(doc))
    return "y" if not e else "n: " + "; ".join(x.message[:80] for x in e[:2])
def gpt_summary(ins, role):
    if not ins: return "n/a"
    g = ins["disks"][role]["gpt"]
    if not g: return "none"
    return "+".join("%s:%sMiB" % (p["type"].replace(" ", "_"), int(p["size_mib"])) for p in g["partitions"]) or "empty"
def same(a, b):
    if not a or not b: return "n/a"
    return "y" if all(a["disks"][r] == b["disks"][r] for r in ("system", "home")) else "n"

notes = [sys.argv[5]] if len(sys.argv) > 5 else []
v = load(D / "verify.json"); cd = load(D / "countdown.json"); o = load(D / "outcome.json"); job = load(D / "job.json") or load(A / "job.json")
pre = load(A / "v9-pre.json"); after = load(A / ("v9-after-%s.json" % ARM))
pro = load(D / "prologue.json")
identity = (v or {}).get("identity", {}).get("result", "n/a")
countdown = (cd or {}).get("result", "none")
status = (o or {}).get("status", "none"); stopped_at = (o or {}).get("stopped_at") or ""
crossed = str((o or {}).get("commit_line", {}).get("crossed", "n/a")).lower()
ov = valid(o, "outcome")
unchanged = same(pre, after)
sys_after, home_after = gpt_summary(after, "system"), gpt_summary(after, "home")

booted = pw_ok = home_ok = "n/a"
boots = D / "boots.log"
if boots.exists():
    lines = [l.strip() for l in open(boots, encoding="utf-8-sig", errors="replace") if l.startswith("linux-boot")]
    booted = "y" if lines else "n"
    if lines:
        f = dict(kv.split("=", 1) for kv in lines[-1].split(",") if "=" in kv)
        want = hashlib.sha256((job or {}).get("intent", {}).get("account", {}).get("password_hash", "").encode()).hexdigest()
        pw_ok = "y" if f.get("pw_sha256") == want else "n"
        home_ok = "y" if f.get("home_dir") == "present" and f.get("home_disk") not in (None, "none", f.get("root_disk")) else "n"
        notes.append("boot marker: user=%s home_disk=%s root_disk=%s" % (f.get("user"), f.get("home_disk"), f.get("root_disk")))

if ARM == "A":
    ok = identity == "fail" and countdown == "none" and unchanged == "y"
    result = "refused-before-countdown" if ok else "fail"
elif ARM == "B":
    ok = countdown == "cancelled" and status == "stopped" and stopped_at == "countdown" and ov == "y" and unchanged == "y"
    result = "cancelled-untouched" if ok else "fail"
else:
    elapsed_at = (cd or {}).get("ended_utc")
    same_time = elapsed_at and (o or {}).get("commit_line", {}).get("crossed_utc") == elapsed_at
    sys_ok = sys_after.startswith("EFI_System") and sys_after.count("Linux") >= 2 and "Microsoft" not in sys_after
    home_gpt_ok = home_after.count("Linux") == 1 and "+" not in home_after
    ok = countdown == "elapsed" and status == "completed" and ov == "y" and same_time and sys_ok and home_gpt_ok and booted == "y" and pw_ok == "y" and home_ok == "y"
    if not same_time: notes.append("crossed_utc %s vs countdown end %s" % ((o or {}).get("commit_line", {}).get("crossed_utc"), elapsed_at))
    result = "erased-installed" if ok else "fail"
if o and o.get("reason"): notes.append("reason: " + o["reason"])

row = {"timestamp": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"), "arm": ARM, "firmware": FIRMWARE,
       "prologue_version": ((pro or {}).get("prologue_version") or ("harness" if ARM == "A" else "n/a")),
       "verify_version": (v or {}).get("verify_version", "n/a"), "identity": identity, "countdown": countdown,
       "outcome_status": status, "stopped_at": stopped_at, "commit_crossed": crossed, "outcome_valid": ov,
       "disks_unchanged": unchanged, "system_gpt_after": sys_after, "home_gpt_after": home_after,
       "fedora_booted": booted, "password_matches": pw_ok, "home_on_second_disk": home_ok, "result": result, "notes": " | ".join(notes)}
new = not CSV.exists()
with open(CSV, "a", newline="") as f:
    w = csv.DictWriter(f, fieldnames=HEADER, quoting=csv.QUOTE_ALL)
    if new: w.writeheader()
    w.writerow(row)
print("v9-verdict: arm %s -> %s (%s)" % (ARM, result, ", ".join("%s=%s" % (k, row[k]) for k in ("identity", "countdown", "outcome_status", "disks_unchanged", "system_gpt_after", "home_gpt_after", "fedora_booted", "password_matches", "home_on_second_disk"))))
