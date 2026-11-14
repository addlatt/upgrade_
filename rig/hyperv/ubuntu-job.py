#!/usr/bin/env python3
"""
ubuntu-job.py - the rig's Ubuntu erase job (RISKS R40, R45; roadmap item 23).

The Ubuntu guest has no Windows, so no job writer ran on it. This takes the
contract's own example erase job (schemas/examples/job.erase-and-install.json)
and makes it this guest's: Ubuntu, the GNOME desktop, the rig account with a
real password hash, and the system disk by the identity the guest's own
early check logged (its unique id and exact size). Validated against
schemas/job.schema.json before it is written. Nothing here is product code.

    ubuntu-job.py out.json [--keep] --uid=HEX --size=BYTES [--serial=TEXT] --hash='$6$...' [--spoof-ssh=PUBKEY]

--spoof-ssh makes harvest.ssh up the way evaluate writes it for a Windows that
had remote access on (one key), as v9-job.py does: the answer file then asks
for the SSH server and settle-in puts the key in. A spoof: plumbing only.

A made-up --uid is the wrong-machine arm. --keep makes the keep-windows job
instead (from schemas/examples/job.keep-windows.json): no erase consent,
25 GB wanted for Linux, and the boot partition said to have room, as the
scanner would have judged on this disk.
"""
import json, sys, datetime, pathlib, uuid
from jsonschema import Draft7Validator, FormatChecker

root = pathlib.Path(__file__).resolve().parents[2]
out = sys.argv[1]
opts = dict(a.split("=", 1) for a in sys.argv[2:] if a.startswith("--") and "=" in a)
keep = "--keep" in sys.argv[2:]
job = json.load(open(root / ("schemas/examples/job.keep-windows.json" if keep else "schemas/examples/job.erase-and-install.json")))
now = datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")

uid, size, serial = opts["--uid"], int(opts["--size"]), opts.get("--serial", "")
job["job_id"] = str(uuid.uuid4())
job["created_utc"] = now
job["identity"]["system_disk"].update({"unique_id": uid, "size_bytes": size, "serial_number": serial})
job["intent"].update({"path": "keep-windows" if keep else "clean-slate", "path_reason": "default" if keep else "user-chose-fresh-start", "desktop": "gnome", "start_at": "desktop",
                      "distro": {"name": "ubuntu", "release": "26.04.1"}})
job["intent"]["account"].update({"windows_name": "rig", "linux_name": "rig", "full_name": "Rig", "password_hash": opts["--hash"]})
job["intent"]["locale"] = {"lang": "en_US.UTF-8", "timezone": "Etc/UTC", "keymap": "us"}
if "--spoof-ssh" in opts:
    k = pathlib.Path(opts["--spoof-ssh"]).read_text().strip()
    job["harvest"]["ssh"] = {"result": "carried", "keys": [k], "sources": ["(rig spoof) C:\\ProgramData\\ssh\\administrators_authorized_keys"], "why": None}
if keep:
    job.pop("erase_consent", None)
    job["storage"]["linux_min_gb"] = 25
    job["storage"]["esp"]["fits_alongside_install"] = True
else:
    job["erase_consent"]["accepted_utc"] = now
    job["erase_consent"]["disks"] = [{"role": "system", "serial_number": serial, "unique_id": uid, "size_bytes": size,
                                      "friendly_name": "Msft Virtual Disk", "health_status": "Healthy"}]
schema = json.load(open(root / "schemas/job.schema.json"))
errs = sorted(Draft7Validator(schema, format_checker=FormatChecker()).iter_errors(job), key=lambda e: list(e.path))
if errs:
    for e in errs: print("schema:", "/".join(map(str, e.path)), e.message, file=sys.stderr)
    sys.exit(1)
json.dump(job, open(out, "w"), indent=2)
print("ubuntu-job: wrote %s job_id=%s system=%s (%d)" % (out, job["job_id"], uid, size))
