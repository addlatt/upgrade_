#!/usr/bin/env python3
"""
v12-verdict.py - one row of docs/validation-results/v12-window.csv from the
run's own evidence: the window's lines in the stick's upgrade_/convert.log,
the %pre verifier's report (verify.json), the return check's row
(v0-handoff.csv on the stick) and what the guest had left afterwards
(after.json). Never by hand.

    v12-verdict.py <artifacts dir> <csv> <firmware>

pass-plumbing needs every link: the person's Start logged, the reopen task
registered before the arm, the arm, the live boot's report, the handoff
fired once, the window reopened and logged a result matching what came
back, and nothing left behind (no reopen or launch task, no armed state,
no bootsequence, the window's ProgramData folder gone after Close).
"""
import csv, json, re, sys, datetime, pathlib

A = pathlib.Path(sys.argv[1]); CSV = pathlib.Path(sys.argv[2]); FIRMWARE = sys.argv[3]
HEADER = ["timestamp", "window_version", "firmware", "secureboot", "launched_as", "renderer", "started", "reopen_registered",
          "armed", "handoff_result", "verify_identity", "verify_image", "reopened", "result_heading", "left_behind", "result", "notes"]


def read(name):
    p = A / name
    return p.read_text(encoding="utf-8-sig", errors="replace") if p.exists() else ""


log = read("convert.log")
# only this run: from the last "the verify flow on" header written by the window
lines = log.splitlines()
start = max((i for i, l in enumerate(lines) if "window " in l and "the verify flow on" in l), default=None)
if start is not None:
    # the Start line comes just before the header
    start = max((i for i, l in enumerate(lines[:start]) if "pressed Start the test" in l), default=start)
run = lines[start:] if start is not None else []
wl = [l for l in run if re.search(r"\bwindow \d+\.\d+\.\d+:", l)]


def has(s):
    return any(s in l for l in wl)


ver = next((m.group(1) for l in wl for m in [re.search(r"\bwindow (\d+\.\d+\.\d+):", l)] if m), "unknown")
renderer = "wgpu" if has("trying wgpu") else ("glow" if wl else "unknown")
started = "y" if has("pressed Start the test") else "n"
registered = "y" if has("reopens the window is registered") else "n"
armed = "y" if has("armed; the computer restarts") else "n"
after_line = next((l for l in wl if "after the restart:" in l), "")
m = re.search(r"after the restart: (.*) \(handoff (.*)\)", after_line)
heading, logged_handoff = (m.group(1), m.group(2)) if m else ("", "")
reopened = "y" if after_line else "n"

verify = None
if read("verify.json"):
    try: verify = json.loads(read("verify.json"))
    except Exception as e: verify = {"parse_error": str(e)}
ident = (verify or {}).get("identity", {}).get("result", "not-reached")
image = (verify or {}).get("payload", {}).get("result", "not-reached")

handoff = "no-row"
if read("v0-handoff.csv"):
    rows = [r for r in csv.DictReader(read("v0-handoff.csv").splitlines()) if r.get("result")]
    if rows: handoff = rows[-1]["result"]

after = {}
if read("after.json"):
    try: after = json.loads(read("after.json"))
    except Exception: after = {}
left = [k for k in ("reopen_task_left", "launch_task_left", "window_dir_left", "handoff_armed_left", "window_running") if after.get(k)]
if (after.get("bootsequence") or "").strip(): left.append("bootsequence")
sb = after.get("secure_boot", "unknown")

notes = []
stops = [l.split(": ", 1)[-1] for l in wl if "stopped" in l or "could not open" in l]
if stops: notes.append("window stops: " + "; ".join(stops)[:300])
if verify: notes.append(f"verify {verify.get('verify_version')} mode={verify.get('mode')} wifi='{verify.get('hardware', {}).get('wifi_detail', '')}' read_mbps={verify.get('payload', {}).get('read_mbps')}")
notes.append("launched elevated through a one-shot interactive task (no UAC click); autologon stood in for the sign-in")

if start is None or not wl:
    result = "no-window-lines"
elif started != "y":
    result = "not-started"
elif armed != "y":
    result = "stopped-before-arm" if stops else "arm-not-logged"
elif registered != "y":
    result = "armed-without-reopen"  # the window must never do this
elif reopened != "y":
    result = "not-reopened"
elif handoff != "fired-once" or logged_handoff != handoff:
    result = "handoff-mismatch"
elif ident != "pass" or image != "pass" or heading != "Linux ran on this computer":
    result = "result-mismatch"
elif left:
    result = "left-behind"
else:
    result = "pass-plumbing"

row = [datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"), ver, FIRMWARE, sb, "task-elevated", renderer,
       started, registered, armed, handoff, ident, image, reopened, heading, ",".join(left) or "none", result, " | ".join(notes)]
new = not CSV.exists()
with open(CSV, "a", newline="", encoding="utf-8") as f:
    w = csv.writer(f, quoting=csv.QUOTE_ALL, lineterminator="\n")
    if new: w.writerow(HEADER)
    w.writerow(row)
print(f"v12-verdict: {result} (window {ver}, renderer={renderer}, armed={armed}, handoff={handoff}, reopened={reopened}, heading='{heading}', left={left or 'none'}) -> {CSV}")
sys.exit(0 if result == "pass-plumbing" else 1)
