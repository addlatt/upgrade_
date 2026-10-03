#!/usr/bin/env python3
"""
v11w-verdict.py - one row of docs/validation-results/v11-walkaway.csv (the
walk-away way back to Windows: RISKS R33, VALIDATION V11 step 4). Never by hand.

    v11w-verdict.py <evidence dir> <csv> <machine> <firmware> [--observed KEY=VALUE ...] [note]

Reads what was copied off the stick: go-back.json (the job), go-back-gate.json
(the gate's record), go-back-unattend.xml (the answer file it wrote), and
go-back-installed.txt (written by SetupComplete.cmd, if Windows ran it).
What only a person at the machine can see is passed as --observed and the row
says so: started (how the stick was started), key_page (y/n), password_forced
(y/n), wifi_at_first_start (y/n), activation (Windows' own words), cancel
(tested-cancelled / not-tested).

A row passes ("windows-reached") only if: the gate crossed after a full
countdown, every job drive matched one disk by serial or world-wide name, the
answer file wipes exactly those disks and never the stick, Windows ran
SetupComplete (installed marker), no product key page, the password was
forced, the program itself started the stick (not a tester step), and the new
Windows was online at its first start (decided 2026-10-02). Anything less is
"fail", with every reason in the notes.
"""
import csv, datetime, json, pathlib, re, sys

D = pathlib.Path(sys.argv[1]); CSV = pathlib.Path(sys.argv[2]); MACHINE = sys.argv[3]; FIRMWARE = sys.argv[4]
rest = sys.argv[5:]
obs = dict(a.split("=", 1) for a in rest[rest.index("--observed") + 1:] if "=" in a) if "--observed" in rest else {}
notes = [a for a in rest if not a.startswith("--") and "=" not in a]
HEADER = ["timestamp", "machine", "firmware", "gate_version", "job_id", "started", "stick_wait_s", "gate_result",
          "drives_matched", "stick_spared", "countdown_s", "cancel", "wipes", "installed_marker", "key_page",
          "password_forced", "wifi_at_first_start", "activation", "result", "notes"]

def load(p):
    try: return json.load(open(p, encoding="utf-8-sig"))
    except Exception: return None

job = load(D / "go-back.json") or {}
gate = load(D / "go-back-gate.json") or {}
xml = (D / "go-back-unattend.xml").read_text() if (D / "go-back-unattend.xml").exists() else ""
found = gate.get("found") or []
roles = [d.get("role") for d in job.get("drives", [])]
matched = "y" if found and sorted(f["role"] for f in found) == sorted(roles) and all(f.get("matched_by") for f in found) else "n"
wiped = sorted(int(x) for x in re.findall(r"<DiskID>(\d+)</DiskID><WillWipeDisk>true", xml))
want = sorted(f["number"] for f in found)
stick = gate.get("stick_disk")
spared = "y" if stick is not None and stick not in wiped else "n"
wipes = "y" if wiped and wiped == want else "n"
installed = "y" if (D / "go-back-installed.txt").exists() else "n"
cd = (gate.get("countdown") or {}).get("elapsed_s")
crossed = gate.get("result") == "crossed" and (cd or 0) >= 119.5

why = []
if not crossed: why.append("the gate did not cross after a full countdown")
if matched != "y": why.append("not every job drive was matched")
if spared != "y": why.append("the stick was not spared")
if wipes != "y": why.append("the answer file does not wipe exactly the found drives")
if installed != "y": why.append("Windows did not run SetupComplete.cmd (no go-back-installed.txt)")
if obs.get("key_page") != "n": why.append("a product key page was shown, or not observed")
if obs.get("password_forced") != "y": why.append("the account was not forced to choose a password")
if obs.get("started", "") != "program": why.append("the stick was started by %s, not by the program's own restart" % obs.get("started", "?"))
if obs.get("wifi_at_first_start") != "y": why.append("Windows was not online at its first start")
result = "windows-reached" if not why else "fail"

row = {"timestamp": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"), "machine": MACHINE, "firmware": FIRMWARE,
       "gate_version": gate.get("gate_version", "n/a"), "job_id": job.get("job_id", "n/a"), "started": obs.get("started", "not-observed"),
       "stick_wait_s": gate.get("stick_wait_s", "n/a"), "gate_result": gate.get("result", "none"), "drives_matched": matched,
       "stick_spared": spared, "countdown_s": "%.1f" % cd if cd else "n/a", "cancel": obs.get("cancel", "not-tested"),
       "wipes": wipes, "installed_marker": installed, "key_page": obs.get("key_page", "not-observed"),
       "password_forced": obs.get("password_forced", "not-observed"), "wifi_at_first_start": obs.get("wifi_at_first_start", "not-observed"),
       "activation": obs.get("activation", "not-observed"), "result": result,
       "notes": " | ".join(notes + (["observed by the owner: " + ", ".join("%s=%s" % kv for kv in sorted(obs.items()))] if obs else []) + why)}
new = not CSV.exists()
with open(CSV, "a", newline="") as f:
    w = csv.DictWriter(f, fieldnames=HEADER, quoting=csv.QUOTE_ALL)
    if new: w.writeheader()
    w.writerow(row)
print("v11w-verdict: %s (%s)" % (result, "; ".join(why) if why else "every check passed"))
