#!/usr/bin/env python3
"""
v9-job.py - the rig's erase-and-install job (VALIDATION V9, RISKS R27).

Hyper-V has no USB, so the product's job writer refuses the rig's SCSI
"stick" (as it must, R16); like v1-job.py this is the stand-in for steps
1-4 of RUN-ERASE-AND-INSTALL.cmd. It takes the keep-windows job v1-job.py
wrote from the guest's real facts and turns it into what New-Job.ps1
-EraseEverything writes: clean-slate, reason user-chose-fresh-start, the
typed sentence, and every internal disk by the identity the guest reported
(disks.json: Get-Disk rows) - the system disk first, the other non-stick
disk as home. Validated against schemas/job.schema.json before it is written.

    v9-job.py base-job.json disks.json out.json [--bogus-home] [--desktop=kde|gnome] [--start-at=desktop|console]

--bogus-home names a home disk that is not attached (arm A: the installer
must refuse before any countdown). Nothing here is product code.
"""
import json, sys, datetime, pathlib
from jsonschema import Draft7Validator, FormatChecker

root = pathlib.Path(__file__).resolve().parents[2]
job = json.load(open(sys.argv[1]))
disks = json.load(open(sys.argv[2], encoding="utf-8-sig"))
if isinstance(disks, dict): disks = [disks]
out = sys.argv[3]
bogus = "--bogus-home" in sys.argv[4:]
opts = dict(a.split("=", 1) for a in sys.argv[4:] if a.startswith("--") and "=" in a)

STATEMENT = "I confirm that everything on this computer will be deleted and nothing will be kept"
sysd = job["identity"]["system_disk"]
stick_uid = job["stick"]["unique_id"]
def row(d, role):
    return {"role": role, "serial_number": (d.get("serial") or "").replace(" ", ""), "unique_id": d["unique_id"],
            "size_bytes": int(d["size"]), "friendly_name": d.get("name") or "", "health_status": d.get("health") or "Unknown"}
system = next(d for d in disks if d["unique_id"] == sysd["unique_id"])
others = [d for d in disks if d["unique_id"] not in (sysd["unique_id"], stick_uid)]
if len(others) > 1: sys.exit("v9-job: more than one non-stick, non-system disk: %r" % others)
listed = [row(system, "system")]
if bogus:
    listed.append({"role": "home", "serial_number": "NOSUCHDISK01", "unique_id": "60022480ffffffffffffffffffffffff",
                   "size_bytes": 68719476736, "friendly_name": "a disk that is not attached", "health_status": "Healthy"})
elif others:
    h = row(others[0], "home")
    if h["health_status"] != "Healthy": sys.exit("v9-job: the home disk is %s, not Healthy" % h["health_status"])
    listed.append(h)

job["intent"]["desktop"] = opts.get("--desktop", "kde")
job["intent"]["start_at"] = opts.get("--start-at", "desktop")
job["intent"]["path"] = "clean-slate"
job["intent"]["path_reason"] = "user-chose-fresh-start"
job.pop("staged", None)
job["erase_consent"] = {"statement": STATEMENT,
                        "accepted_utc": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
                        "disks": listed}
schema = json.load(open(root / "schemas/job.schema.json"))
errs = sorted(Draft7Validator(schema, format_checker=FormatChecker()).iter_errors(job), key=lambda e: list(e.path))
if errs:
    for e in errs: print("schema:", "/".join(map(str, e.path)), e.message, file=sys.stderr)
    sys.exit(1)
json.dump(job, open(out, "w"), indent=2)
print("v9-job: wrote %s job_id=%s disks=%s" % (out, job["job_id"], ", ".join("%s=%s(%s)" % (d["role"], d["unique_id"], d["size_bytes"]) for d in listed)))
