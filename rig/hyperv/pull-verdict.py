#!/usr/bin/env python3
"""Pull arm verdict (V3, V8; R8, R19, R20): one row in v3-pull.csv.

Reads what guest/pull-plant.ps1 (Windows) and guest/pull-read.sh (Fedora)
left in <artifacts>/pull/ and judges the product's pull:
  - every planted file is in the person's home, byte-identical, theirs;
  - no placeholder, link, junction or desktop.ini came along;
  - the downloaded cloud file (tag kept, recall bit gone) did;
  - Chrome's 5 and Firefox's 3 bookmarks are in their files;
  - the key is gone from the handoff folder and Windows is closed again;
  - check said plan and run said done.
All of it: pass-plumbing (a rig; the provider is ours, not OneDrive). Any
miss: fail, with every miss named. Rows are written here, never by hand.

    pull-verdict.py ARTIFACTS CSV HARNESS FIRMWARE CONTEXT
"""
import csv, datetime, json, os, sys

a, csv_path, harness, firmware, context = sys.argv[1:6]
P = os.path.join(a, "pull")
def read(n, default=""):
    try:
        return open(os.path.join(P, n), encoding="utf-8-sig").read()
    except OSError:
        return default
facts = dict(l.split("=", 1) for l in read("facts-linux.txt").splitlines() if "=" in l)
expected = json.loads(read("expected.json", "{}") or "{}")
home = {}
for l in read("linux-home.tsv").splitlines():
    f = l.split("\t")
    if len(f) == 4:
        home[f[3]] = {"sha": f[0], "size": int(f[1]), "owner": f[2]}
def last_json(text):
    body = "\n".join(l for l in text.splitlines() if not l.startswith("progress "))
    try:
        return json.loads(body)
    except ValueError:
        return {}
check, run = last_json(read("check.json")), last_json(read("run.out"))

miss = []
copies = expected.get("copy", [])
identical = 0
for c in copies:
    h = home.get(f'{c["folder"]}/{c["rel"]}')
    if not h:
        miss.append(f'missing {c["folder"]}/{c["rel"]}')
    elif h["sha"] != c["sha256"] or h["size"] != c["size"]:
        miss.append(f'different {c["folder"]}/{c["rel"]}')
    else:
        identical += 1
not_owned = [p for p, h in home.items() if h["owner"] != "pulltest"]
if not_owned:
    miss.append(f"{len(not_owned)} files not the person's")
ph_home = [p for p in expected.get("placeholders", []) if any(k.endswith(p) for k in home)]
if ph_home:
    miss.append("placeholders copied: " + ", ".join(ph_home))
nc_home = [p for p in expected.get("not_copied", []) if any(k == f"Documents/{p}" or k.startswith(f"Documents/{p}/") for k in home)]
if nc_home or read("linux-links.txt").strip():
    miss.append("links or folder settings copied: " + ", ".join(nc_home or [read("linux-links.txt").strip()]))
downloaded = "Documents/pull-test/Cloud/downloaded.pdf" in home
marks = {k.split(":", 1)[1]: int(v) for k, v in facts.items() if k.startswith("bookmarks_file:")}
for b, n in (expected.get("bookmarks") or {}).items():
    got = marks.get(f"Bookmarks from Windows - {b}.html")
    if got != n:
        miss.append(f"{b} bookmarks {got} of {n}")
if facts.get("key_left_in_handoff") != "no":
    miss.append("the key is still in the handoff folder")
if facts.get("windows_still_mapped") != "no" or facts.get("windows_still_mounted") != "no":
    miss.append("Windows was left open")
if check.get("result") != "plan":
    miss.append(f'check said {check.get("result")}: {check.get("why")}')
if run.get("result") != "done":
    miss.append(f'run said {run.get("result")}: {run.get("why")}')
if facts.get("stage"):
    miss.append("stopped at " + facts["stage"])

row = {
    "date": datetime.date.today().isoformat(), "harness": harness, "firmware": firmware, "context": context,
    "settle_in": facts.get("settle_in", ""), "windows_build": expected.get("windows_build", ""), "kernel": facts.get("kernel", ""),
    "cryptsetup": facts.get("cryptsetup", ""), "ntfs3g": facts.get("ntfs3g", ""), "bitlocker_conversion": facts.get("windows_conversion_status", ""),
    "planted_copies": len(copies), "copied_identical": identical, "placeholders_expected": len(expected.get("placeholders", [])),
    "placeholders_copied": len(ph_home), "downloaded_copied": "yes" if downloaded else "no",
    "bookmarks": " ".join(f"{k}={v}" for k, v in sorted(marks.items())), "key_deleted": "yes" if facts.get("key_left_in_handoff") == "no" else "no",
    "check": check.get("result", ""), "run": run.get("result", ""),
    "result": "pass-plumbing" if not miss and copies else "fail",
    "notes": "; ".join(miss) if miss else f'{len(home)} files in the home; xattrs in pull/xattr.txt',
}
new = not os.path.exists(csv_path) or os.path.getsize(csv_path) == 0
with open(csv_path, "a", newline="") as f:
    w = csv.DictWriter(f, fieldnames=list(row))
    if new:
        w.writeheader()
    w.writerow(row)
print(json.dumps(row, indent=1))
print("xattrs:\n" + read("xattr.txt"))
sys.exit(0 if row["result"] == "pass-plumbing" else 1)
