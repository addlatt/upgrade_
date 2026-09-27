#!/usr/bin/env python3
"""
settle-in-verdict.py - one row of docs/validation-results/settle-in-first-start.csv
from a run's own evidence (RISKS R28, VALIDATION V10). Never by hand.

    settle-in-verdict.py <artifacts/v9/TAG> <csv> <machine> [--spoofed-wifi] [note]

Reads what `v9.sh stick-pull TAG` copied: outcome.json, job.json, and
settle-in/ (the bench marker's capture from the first Linux boot: report.json,
nm-parsed.txt, handoff-ls.txt, clock.txt, order.txt), plus
stick-credentials-dir.txt. Checks, each named in the row:

  installed   outcome.cutover.settle_in.installed (the adapter checked SHA256SUMS)
  handoff     credentials.wifi: moved == expected, removed_from_stick; the stick
              lists no wifi folder under artifacts/credentials
  clock       settle-in's clock result and why, as it said it (any result is
              recorded; "failed" fails the row)
  wifi        with --spoofed-wifi (v9-job.py's three made-up networks):
              NetworkManager's own parse of the files settle-in wrote -
              "RigSpoof Home" wpa-psk autoconnect, "Rig;Open" hidden, not
              autoconnect, open; "RigSpoof Work" (enterprise) not set up;
              every file mode 600
  deleted     report.wifi.passwords_deleted, and no wifi folder in the handoff
  order       settle-in finished before NetworkManager and chronyd started

A spoofed pass is pass-plumbing: it closes plumbing, never a real-hardware
clause (rule #5).
"""
import csv, datetime, json, pathlib, re, sys

D = pathlib.Path(sys.argv[1]); CSV = pathlib.Path(sys.argv[2]); MACHINE = sys.argv[3]
spoofed = "--spoofed-wifi" in sys.argv[4:]
notes = [a for a in sys.argv[4:] if not a.startswith("--")]
S = D / "settle-in"
HEADER = ["timestamp", "machine", "settle_in_version", "installed", "handoff", "clock_result", "clock_why",
          "installer_clock_error", "wifi_result", "wifi_nm_parse", "passwords_deleted", "ran_before_network", "result", "notes"]

def load(p):
    try: return json.load(open(p, encoding="utf-8-sig"))
    except Exception: return None
def text(p):
    try: return open(p, encoding="utf-8", errors="replace").read()
    except Exception: return ""

o = load(D / "outcome.json") or {}; rep = load(S / "report.json") or {}
fails = []

si = o.get("cutover", {}).get("settle_in", {})
installed = "y" if si.get("installed") else "n: %s" % si.get("why_not")
if not si.get("installed"): fails.append("not installed")

w = o.get("credentials", {}).get("wifi") or {}
stick_dir = text(D / "stick-credentials-dir.txt")
stick_has_wifi = bool(re.search(r"(?im)^wifi\s", stick_dir))
handoff = "moved %s/%s, stick %s" % (w.get("moved"), w.get("expected"), "still has wifi" if stick_has_wifi else "clear")
if not w or w.get("moved") != w.get("expected") or not w.get("removed_from_stick") or stick_has_wifi: fails.append("handoff")

c = rep.get("clock", {})
clock_result = c.get("result", "no report"); clock_why = c.get("why", "")
installer_err = c.get("install_records", {}).get("installer_clock_error", "")
if clock_result in ("failed", "attempting", "no report"): fails.append("clock " + clock_result)

wr = rep.get("wifi", {})
wifi_result = "%s (%s created)" % (wr.get("result", "none"), wr.get("created", 0))
nm = [l.split("|") for l in text(S / "nm-parsed.txt").splitlines() if l.strip()]
nm_by_ssid = {r[1]: r for r in nm if len(r) >= 6}
if spoofed:
    want = {"RigSpoof Home": ("no", "yes", "wpa-psk"), "Rig;Open": ("yes", "no", "")}
    bad = []
    for ssid, (hidden, auto, km) in want.items():
        r = nm_by_ssid.get(ssid)
        if not r: bad.append("%s missing" % ssid); continue
        if (r[2], r[3], r[4]) != (hidden, auto, km): bad.append("%s parsed %s" % (ssid, r[2:5]))
        if r[5] != "600": bad.append("%s mode %s" % (ssid, r[5]))
    if "RigSpoof Work" in nm_by_ssid: bad.append("enterprise network was set up")
    work = [n for n in wr.get("networks", []) if n.get("ssid") == "RigSpoof Work"]
    if not work or work[0].get("result") != "not-set-up": bad.append("enterprise network not listed")
    wifi_nm = "y (%d)" % len(nm) if not bad else "n: " + "; ".join(bad)
    if bad: fails.append("wifi")
else:
    wifi_nm = "n/a (%d parsed)" % len(nm)

deleted = "y" if wr.get("passwords_deleted") and "wifi" not in text(S / "handoff-ls.txt").split() else "n"
if wr.get("result") not in (None, "nothing-to-do") and deleted != "y": fails.append("passwords not deleted")

order = text(S / "order.txt").splitlines()
def first(pat):
    for i, l in enumerate(order):
        if re.search(pat, l): return i
    return None
si_done = first(r"upgrade_-settle-in.*(Finished|finished)|settle-in: report written")
nm_start = first(r"Starting NetworkManager|NetworkManager\[\d+\]")
ch_start = first(r"Starting chronyd|chronyd\[\d+\]")
if si_done is None: before = "n: settle-in not in the journal"; fails.append("order")
elif (nm_start is not None and nm_start < si_done) or (ch_start is not None and ch_start < si_done): before = "n"; fails.append("order")
else: before = "y"

result = ("pass-plumbing" if spoofed else "pass") if not fails else "fail"
if fails: notes.append("failed: " + ", ".join(fails))
row = [datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"), MACHINE, rep.get("settle_in_version", si.get("version", "")),
       installed, handoff, clock_result, clock_why, installer_err, wifi_result, wifi_nm, deleted, before, result, "; ".join(notes)]
new = not CSV.exists()
with open(CSV, "a", newline="") as f:
    wr_ = csv.writer(f)
    if new: wr_.writerow(HEADER)
    wr_.writerow(row)
for h, v in zip(HEADER, row): print("%-22s %s" % (h, v))
