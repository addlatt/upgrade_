#!/usr/bin/env python3
"""
v1-job.py - the rig's stand-in for the job.json writer (which evaluate does
not have yet). Takes the facts the guest reported over PowerShell Direct
(facts.json), fills the keep-windows example from schemas/examples/ with
them, validates the result against schemas/job.schema.json and writes it.
Nothing here is product code: the point is a job that is TRUE for the rig
guest - its real disk identity, ESP, BitLocker state - so the verifier on
the stick has something real to match against.

    v1-job.py facts.json password_hash out.json [stick_unique_id stick_size]
"""
import json, sys, uuid, datetime, pathlib
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[2] / "schemas"))
from jsonschema import Draft7Validator, FormatChecker

here = pathlib.Path(__file__).resolve().parent
root = here.parents[1]
facts = json.load(open(sys.argv[1]))
pw_hash = sys.argv[2]
out = sys.argv[3]
stick_uid = sys.argv[4] if len(sys.argv) > 4 else "UNKNOWN-STICK"
stick_size = int(sys.argv[5]) if len(sys.argv) > 5 else 2306867200

job = json.load(open(root / "schemas/examples/job.keep-windows.json"))
job["job_id"] = str(uuid.uuid4())
job["created_utc"] = datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
job["identity"] = {
    "vendor": facts["vendor"], "model": facts["model"], "system_uuid": facts["uuid"],
    "bios_serial": facts["bios_serial"], "bios_version": facts["bios_version"],
    "firmware_mode": "UEFI", "secure_boot": facts["sb"],
    "os_caption": facts["os"], "os_build": int(facts["build"]),
    "system_disk": {"number": int(facts["disk"]["number"]), "serial_number": facts["disk"]["serial"] or "",
                    "unique_id": facts["disk"]["unique_id"], "friendly_name": facts["disk"]["name"],
                    "size_bytes": int(facts["disk"]["size"]), "partition_style": facts["disk"]["style"]},
}
job["scan"]["verdict"] = "YELLOW"
job["intent"]["account"] = {"windows_name": "rig", "full_name": None, "linux_name": "rig", "password_hash": pw_hash}
job["intent"]["locale"] = {"lang": "en_US.UTF-8", "timezone": "UTC", "keymap": "us"}
job["intent"]["desktop"] = "kde"
job["fork"] = {"if_cannot_keep": "stop", "volume_check_consented": True}
job["storage"]["shrinkable_gb"] = float(facts.get("shrink_gb") or 0) or None
job["storage"]["shrink_source"] = "storage-api" if job["storage"]["shrinkable_gb"] else None
job["storage"]["shrink_error"] = None
job["storage"]["volume_health"] = {"dirty": "clean", "scan": None}
job["storage"]["physical_disk"] = {"health_status": facts.get("health", "Healthy"), "operational_status": "OK"}
job["storage"]["esp"] = {"size_bytes": int(facts["esp"]["size"]), "free_bytes": int(facts["esp"]["free"]),
                         "fits_alongside_install": int(facts["esp"]["free"]) >= 32 * 1024 * 1024}
job["harvest"]["folders"] = [f for f in job["harvest"]["folders"] if f["name"] == "Documents"]
job["harvest"]["folders"][0].update({"path": "C:\\Users\\rig\\Documents", "is_onedrive": False, "files": 0, "bytes": 0, "cloud_only_files": 0})
job["harvest"]["cloud_files"] = {"placeholders_found": 0, "materialized": 0, "failed": 0, "result": "none-found"}
job["harvest"]["browsers"] = []
job["harvest"]["wifi"] = {"profiles": [], "secrets_file": None}
bl = facts.get("bitlocker", "off")
job["harvest"]["bitlocker"] = {"status": bl, "recovery_key_file": "artifacts/credentials/bitlocker-C.txt" if bl == "on" else None}
job["harvest"]["firmware_artifacts"] = []
job["stick"] = {"unique_id": stick_uid, "serial_number": "", "size_bytes": stick_size, "friendly_name": "Msft Virtual Disk",
                "label": "UPGV0", "manifest": "SHA256SUMS"}

schema = json.load(open(root / "schemas/job.schema.json"))
errs = sorted(Draft7Validator(schema, format_checker=FormatChecker()).iter_errors(job), key=lambda e: list(e.path))
if errs:
    for e in errs: print("schema:", "/".join(map(str, e.path)), e.message, file=sys.stderr)
    sys.exit(1)
json.dump(job, open(out, "w"), indent=2)
print(f"v1-job: wrote {out} job_id={job['job_id']} disk={job['identity']['system_disk']['unique_id']} size={job['identity']['system_disk']['size_bytes']}")
