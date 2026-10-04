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
  D again:  after C, the stick first in the boot order and left in: the stick carries
            'converted', no 'boot-install', no new countdown, disks unchanged, Linux booted
            again -> not-installed-again (RISKS R35)
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
          "fedora_booted", "password_matches", "home_on_second_disk", "result", "notes", "graphical_login"]

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

booted = pw_ok = home_ok = gui_ok = "n/a"
boots = D / "boots.log"
if boots.exists():
    lines = [l.strip() for l in open(boots, encoding="utf-8-sig", errors="replace") if l.startswith("linux-boot")]
    booted = "y" if lines else "n"
    if lines:
        f = dict(kv.split("=", 1) for kv in lines[-1].split(",") if "=" in kv)
        want = hashlib.sha256((job or {}).get("intent", {}).get("account", {}).get("password_hash", "").encode()).hexdigest()
        pw_ok = "y" if f.get("pw_sha256") == want else "n"
        home_ok = "y" if f.get("home_dir") == "present" and f.get("home_disk") not in (None, "none", f.get("root_disk")) else "n"
        # one click ends at the desktop's sign-in (2026-09-26): graphical target AND the display manager running
        # what the person chose (intent.start_at, 2026-09-26): desktop = graphical + the display manager
        # running; console = multi-user and no display manager - the column says whether the choice held
        want_console = (job or {}).get("intent", {}).get("start_at") == "console"
        if "default_target" not in f: gui_ok = "not-recorded"
        elif want_console: gui_ok = "y" if f.get("default_target") == "multi-user.target" and f.get("display_manager") != "active" else "n"
        else: gui_ok = "y" if f.get("default_target") == "graphical.target" and f.get("display_manager") == "active" else "n"
        notes.append("first boot: default_target=%s display_manager=%s" % (f.get("default_target"), f.get("display_manager")))
        # a real person's account name never goes into a tracked row (the rig's account is "rig")
        notes.append("boot marker: user=%s home_disk=%s root_disk=%s" % (f.get("user") if f.get("user") == "rig" else "<name>", f.get("home_disk"), f.get("root_disk")))

# a re-run is named by its arm letter and a number (B3 = arm B again); judge it by the letter (2026-09-29)
KIND = ARM[:1]
if KIND == "D":
    # the stick started again after a finished install (RISKS R35; the Aspire's run 10):
    # it carries 'converted' and no 'boot-install', the countdown record is the install's own
    # (no new countdown ran), both disks are byte-for-byte as before this start (v9-before-<ARM>
    # vs v9-after-<ARM>), and Linux booted again (a second linux-boot line)
    listing = (D / "stick-upgrade-dir.txt").read_text(errors="replace") if (D / "stick-upgrade-dir.txt").exists() else ""
    conv = (D / "converted").exists(); armed = (D / "boot-install").exists() or "boot-install" in listing
    before = load(A / ("v9-before-%s.json" % ARM)); unchanged = same(before, after)
    nboots = len([l for l in open(boots, encoding="utf-8-sig", errors="replace") if l.startswith("linux-boot")]) if boots.exists() else 0
    elapsed_at = (cd or {}).get("ended_utc"); same_cd = bool(elapsed_at) and (o or {}).get("commit_line", {}).get("crossed_utc") == elapsed_at
    notes.append("stick: converted=%s boot-install=%s; countdown record still the install's=%s; linux boots=%d" % ("y" if conv else "n", "y" if armed else "n", "y" if same_cd else "n", nboots))
    ok = conv and not armed and unchanged == "y" and same_cd and countdown == "elapsed" and nboots >= 2
    result = "not-installed-again" if ok else "fail"
elif KIND == "A":
    ok = identity == "fail" and countdown == "none" and unchanged == "y"
    result = "refused-before-countdown" if ok else "fail"
elif KIND == "B":
    ok = countdown == "cancelled" and status == "stopped" and stopped_at == "countdown" and ov == "y" and unchanged == "y"
    result = "cancelled-untouched" if ok else "fail"
else:
    elapsed_at = (cd or {}).get("ended_utc")
    same_time = elapsed_at and (o or {}).get("commit_line", {}).get("crossed_utc") == elapsed_at
    sys_ok = sys_after.startswith("EFI_System") and sys_after.count("Linux") >= 2 and "Microsoft" not in sys_after
    home_gpt_ok = home_after.count("Linux") == 1 and "+" not in home_after
    ok = countdown == "elapsed" and status == "completed" and ov == "y" and same_time and sys_ok and home_gpt_ok and booted == "y" and pw_ok == "y" and home_ok == "y" and gui_ok == "y"
    if not same_time: notes.append("crossed_utc %s vs countdown end %s" % ((o or {}).get("commit_line", {}).get("crossed_utc"), elapsed_at))
    result = "erased-installed" if ok else "fail"
if o and o.get("reason"): notes.append("reason: " + o["reason"])

row = {"timestamp": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"), "arm": ARM, "firmware": FIRMWARE,
       "prologue_version": ((pro or {}).get("prologue_version") or ("harness" if KIND == "A" else "n/a")),
       "verify_version": (v or {}).get("verify_version", "n/a"), "identity": identity, "countdown": countdown,
       "outcome_status": status, "stopped_at": stopped_at, "commit_crossed": crossed, "outcome_valid": ov,
       "disks_unchanged": unchanged, "system_gpt_after": sys_after, "home_gpt_after": home_after,
       "fedora_booted": booted, "password_matches": pw_ok, "home_on_second_disk": home_ok, "graphical_login": gui_ok, "result": result, "notes": " | ".join(notes)}
new = not CSV.exists()
if not new:
    old = list(csv.DictReader(open(CSV)))
    if old and list(old[0].keys()) != HEADER:   # a column added later: earlier rows did not record it
        with open(CSV, "w", newline="") as f:
            w = csv.DictWriter(f, fieldnames=HEADER, quoting=csv.QUOTE_ALL); w.writeheader()
            for r in old: w.writerow({k: r.get(k, "not-recorded") for k in HEADER})
with open(CSV, "a", newline="") as f:
    w = csv.DictWriter(f, fieldnames=HEADER, quoting=csv.QUOTE_ALL)
    if new: w.writeheader()
    w.writerow(row)
print("v9-verdict: arm %s -> %s (%s)" % (ARM, result, ", ".join("%s=%s" % (k, row[k]) for k in ("identity", "countdown", "outcome_status", "disks_unchanged", "system_gpt_after", "home_gpt_after", "fedora_booted", "password_matches", "home_on_second_disk"))))
