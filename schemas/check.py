#!/usr/bin/env python3
"""
schemas/check.py - the contract's own test.

Validates every example under examples/ against its schema, then feeds each
schema a set of documents that MUST be refused - a RED verdict in a job, an
unknown schema version, a keep-windows outcome that claims to have crossed
the commit line - and fails if any of them is accepted. Run it before any
change to schemas/ lands:

    python3 schemas/check.py

The reader rule the schemas encode: a module that meets a `schema` value it
does not understand refuses, not guesses. That is asserted here as a
negative case, so a future edit cannot quietly loosen it.
"""
import copy
import json
import sys
from pathlib import Path

try:
    import jsonschema
    from jsonschema import Draft7Validator, FormatChecker
except ImportError:
    print("check: python3-jsonschema is required (pip install jsonschema)", file=sys.stderr)
    sys.exit(2)

HERE = Path(__file__).resolve().parent
SCHEMAS = {
    "job": HERE / "job.schema.json",
    "outcome": HERE / "outcome.schema.json",
}


def load(p):
    with open(p, encoding="utf-8") as f:
        return json.load(f)


def validator(kind):
    schema = load(SCHEMAS[kind])
    Draft7Validator.check_schema(schema)
    return Draft7Validator(schema, format_checker=FormatChecker())


def errors(v, doc):
    return sorted(v.iter_errors(doc), key=lambda e: list(e.path))


def set_path(doc, path, value):
    d = copy.deepcopy(doc)
    cur = d
    for k in path[:-1]:
        cur = cur[k]
    cur[path[-1]] = value
    return d


def del_path(doc, path):
    d = copy.deepcopy(doc)
    cur = d
    for k in path[:-1]:
        cur = cur[k]
    del cur[path[-1]]
    return d


failed = 0


def report(ok, name, detail=""):
    global failed
    if ok:
        print(f"    PASS  {name}")
    else:
        failed += 1
        print(f"    FAIL  {name}")
        if detail:
            for line in detail.splitlines():
                print(f"          {line}")


def main():
    print()
    print("  upgrade_ schemas check")
    print()
    v = {k: validator(k) for k in SCHEMAS}

    # --- every example validates ------------------------------------------
    examples = sorted((HERE / "examples").glob("*.json"))
    if not examples:
        report(False, "examples/ has documents")
    for ex in examples:
        kind = ex.name.split(".")[0]
        errs = errors(v[kind], load(ex))
        report(not errs, f"example validates: {ex.name}",
               "\n".join(f"{'/'.join(map(str, e.path)) or '<root>'}: {e.message}" for e in errs[:6]))

    # --- a job and its outcome agree on job_id ------------------------------
    job = load(HERE / "examples/job.keep-windows.json")
    out = load(HERE / "examples/outcome.keep-windows.json")
    report(job["job_id"] == out["job_id"], "example pair: keep-windows job and outcome share job_id")
    job2 = load(HERE / "examples/job.clean-slate.json")
    job3 = load(HERE / "examples/job.erase-and-install.json")
    out3 = load(HERE / "examples/outcome.erase-and-install.json")
    out2 = load(HERE / "examples/outcome.clean-slate.json")
    report(job2["job_id"] == out2["job_id"], "example pair: clean-slate job and outcome share job_id")

    jack = load(HERE / "examples/job.acknowledged-data-loss.json")
    oack = load(HERE / "examples/outcome.acknowledged-data-loss.json")
    report(jack["job_id"] == oack["job_id"], "example pair: acknowledged-data-loss job and outcome share job_id")

    # --- documents that MUST be refused --------------------------------------
    negatives = [
        # R23 (decided 2026-09-13): the acknowledged-data-loss path lifts exactly two refusals, with the statement verbatim
        ("job", "RED verdict without the acknowledgement is refused", set_path(job, ["scan", "verdict"], "RED")),
        ("job", "acknowledgement with a paraphrased statement is refused",
         set_path(jack, ["risk_acknowledgement", "statement"], "I understand the risks and could lose data")),
        ("job", "acknowledgement with no overrides is refused", set_path(jack, ["risk_acknowledgement", "overrides"], [])),
        ("job", "acknowledgement that names identity as an override is refused",
         set_path(jack, ["risk_acknowledgement", "overrides"], ["identity"])),
        ("job", "keep-windows on a Warning disk without disk-health acknowledged is refused",
         set_path(set_path(jack, ["storage", "physical_disk", "health_status"], "Warning"), ["risk_acknowledgement", "overrides"], ["volume-health"])),
        ("outcome", "a repair on a Warning disk without the acknowledgement is refused",
         del_path(oack, ["risk_acknowledgement"])),
        ("outcome", "an acknowledgement in the outcome with an unknown override is refused",
         set_path(oack, ["risk_acknowledgement", "overrides"], ["disk-health", "bitlocker"])),
        # R24: the resume evidence names its account; anything but SYSTEM|user is refused
        ("outcome", "a resume run_as outside SYSTEM|user is refused", set_path(out, ["prologue", "resumes", 0, "run_as"], "other")),
        ("outcome", "a resume without unattended is refused", del_path(out, ["prologue", "resumes", 0, "unattended"])),
        # R25 (2026-09-26): the Windows Update record is exact in shape
        ("outcome", "a windows_update record with a negative restart count is refused",
         set_path(out, ["prologue", "windows_update"], {"checks": 1, "pending_seen": True, "restarts": -1})),
        ("outcome", "a windows_update record with an unknown field is refused",
         set_path(out, ["prologue", "windows_update"], {"checks": 1, "pending_seen": False, "restarts": 0, "ignored": True})),
        # the reader rule: an unknown version is a refusal
        ("job", "unknown schema version is refused", set_path(job, ["schema"], "job/2")),
        ("outcome", "unknown outcome version is refused", set_path(out, ["schema"], "outcome/2")),
        # rule #1: no RED job, ever
        ("job", "RED verdict is refused", set_path(job, ["scan", "verdict"], "RED")),
        # a job needs an elevated run
        ("job", "unelevated run is refused", set_path(job, ["evaluate", "ran_as_admin"], False)),
        # the handoff is UEFI-only
        ("job", "legacy BIOS is refused", set_path(job, ["identity", "firmware_mode"], "BIOS")),
        # R18: the fork must be chosen
        ("job", "missing fork is refused", del_path(job, ["fork"])),
        ("job", "fork with an unknown branch is refused", set_path(job, ["fork", "if_cannot_keep"], "ask-later")),
        ("job", "flagged volume without consent to the disk check is refused",
         set_path(job, ["fork", "volume_check_consented"], False)),
        # R18 (2026-09-20): restore-point consent is optional (a job written before it existed reads as no consent) but typed
        ("job", "restore_points_consented that is not a boolean is refused",
         set_path(job, ["fork", "restore_points_consented"], "yes")),
        # R18 (2026-09-22): the change-journal consent, the same shape
        ("job", "usn_journal_consented that is not a boolean is refused",
         set_path(job, ["fork", "usn_journal_consented"], "yes")),
        # R18 (2026-09-17): Windows' own "repair queued" is a required fact, and it needs the same consent
        ("job", "volume_health without repair_queued is refused",
         del_path(job, ["storage", "volume_health", "repair_queued"])),
        ("job", "last_unmovable_file that is not a string or null is refused",
         set_path(job, ["storage", "last_unmovable_file"], 259)),
        ("job", "a queued repair without consent to the disk check is refused",
         set_path(set_path(job2, ["storage", "volume_health", "repair_queued"], True), ["fork", "volume_check_consented"], False)),
        # keep-windows needs a Healthy disk and an ESP with room
        ("job", "keep-windows on a Warning disk is refused",
         set_path(job, ["storage", "physical_disk", "health_status"], "Warning")),
        ("job", "keep-windows with a full ESP is refused",
         set_path(job, ["storage", "esp", "fits_alongside_install"], False)),
        # V8: no un-materialized placeholder survives into a job
        ("job", "cloud files with failures is refused", set_path(job, ["harvest", "cloud_files", "failed"], 3)),
        ("job", "cloud files 'refused' is not a writable result",
         set_path(job, ["harvest", "cloud_files", "result"], "refused")),
        # the one-click erase and install (2026-09-26, R27)
        ("job", "an erase job that also stages files is refused",
         set_path(job3, ["staged"], {"files": 1, "bytes": 1, "manifest": "staging/SHA256SUMS"})),
        ("job", "clean-slate with neither staged files nor the erase consent is refused", del_path(job3, ["erase_consent"])),
        ("job", "an erase consent with a paraphrased sentence is refused",
         set_path(job3, ["erase_consent", "statement"], "I confirm everything will be deleted")),
        ("job", "an erase consent on a keep-windows job is refused",
         set_path(job, ["erase_consent"], job3["erase_consent"])),
        ("job", "a fresh-start reason without the erase consent is refused",
         set_path(job2, ["intent", "path_reason"], "user-chose-fresh-start")),
        ("job", "an erase consent under another reason is refused",
         set_path(job3, ["intent", "path_reason"], "forced-no-room")),
        ("job", "a home drive that is not Healthy is refused",
         set_path(job3, ["erase_consent", "disks", 1, "health_status"], "Warning")),
        ("job", "a third drive to erase is refused",
         set_path(job3, ["erase_consent", "disks"], job3["erase_consent"]["disks"] + [job3["erase_consent"]["disks"][1]])),
        ("job", "a first drive that is not the system drive is refused",
         set_path(job3, ["erase_consent", "disks", 0, "role"], "home")),
        ("job", "a drive to erase without a unique id is refused",
         set_path(job3, ["erase_consent", "disks", 1, "unique_id"], "")),
        # the stick fit (2026-09-26, R26): required, and a clean slate must fit
        ("job", "a job without the stick fit is refused", del_path(job, ["harvest", "stick_fit"])),
        ("job", "clean-slate whose folders do not fit the stick is refused",
         set_path(job2, ["harvest", "stick_fit", "fits"], False)),
        ("job", "a negative gap is refused", set_path(job, ["harvest", "stick_fit", "gap_bytes"], -1)),
        # R6: truncated sizing is a refusal at evaluate
        ("job", "truncated folder sizing is refused",
         set_path(job, ["harvest", "folders", 1, "truncated"], True)),
        # decided 2026-09-07: undetermined BitLocker state refuses
        ("job", "unknown BitLocker state is refused", set_path(job, ["harvest", "bitlocker", "status"], "unknown")),
        ("job", "BitLocker on without a recovery key file is refused",
         set_path(job, ["harvest", "bitlocker", "recovery_key_file"], None)),
        # the password never travels in clear
        ("job", "a non-sha512-crypt password field is refused",
         set_path(job, ["intent", "account", "password_hash"], "hunter2")),
        # credentials and artifacts stay inside the job directory
        ("job", "a path that escapes the job directory is refused",
         set_path(job, ["harvest", "bitlocker", "recovery_key_file"], "../../Users/x/key.txt")),
        ("job", "an absolute path is refused",
         set_path(job, ["harvest", "bitlocker", "recovery_key_file"], "/etc/key.txt")),
        # clean-slate must carry the staged manifest
        ("job", "clean-slate without staged files is refused", del_path(job2, ["staged"])),
        # the software inventory is names, not free-form: an entry without a name is refused
        ("job", "a software entry without a name is refused",
         set_path(job, ["harvest", "software", "desktop", 0], {"version": "1.0"})),
        ("job", "a job without the software inventory is refused",
         del_path(job, ["harvest", "software"])),
        # unknown fields are a version skew signal, not noise
        ("job", "an unknown top-level field is refused", set_path(job, ["extra"], 1)),
        # the commit line is a fact, not a guess
        ("outcome", "keep-windows claiming the line was crossed is refused",
         set_path(out, ["commit_line", "crossed"], True)),
        ("outcome", "keep-windows with Windows not kept is refused", set_path(out, ["windows", "kept"], False)),
        ("outcome", "keep-windows scrubbing credentials before the pull is refused",
         set_path(out, ["credentials", "scrub_after"], "cutover")),
        ("outcome", "completed without a cutover block is refused", del_path(out, ["cutover"])),
        ("outcome", "stopped without a reason is refused",
         set_path(set_path(out, ["status"], "stopped"), ["stopped_at"], "shrink")),
        ("outcome", "a stopped run that crossed the line is refused",
         set_path(load(HERE / "examples/outcome.stopped-unhealthy-disk.json"), ["commit_line", "crossed"], True)),
        ("outcome", "clean-slate completed without crossing the line is refused",
         set_path(out2, ["commit_line", "crossed"], False)),
        ("outcome", "a repair on a non-Healthy disk is refused",
         set_path(out, ["prologue", "volume_check", "disk_health_at_check"], "Warning")),
        ("outcome", "a repair that names no method is refused",
         set_path(out, ["prologue", "volume_check", "method"], "none")),
        ("outcome", "a repair that took no restart is not refused by shape but a negative restart count is",
         set_path(out, ["prologue", "volume_check", "restarts"], -1)),
        ("outcome", "a remeasurement from an unknown path is refused",
         set_path(out, ["prologue", "shrink", "remeasured_by"], "guess")),
        ("outcome", "a staged block without the measured write speed is refused",
         set_path(out2, ["prologue", "staged"], {"files": 1, "bytes": 1, "failed": 0, "manifest": "upgrade_/staging/SHA256SUMS"})),
        # the one-click erase and install (2026-09-26, R27)
        ("outcome", "a completed erase with neither a countdown nor restored files is refused",
         del_path(out3, ["cutover", "countdown"])),
        ("outcome", "a completed erase whose countdown was cancelled is refused",
         set_path(out3, ["cutover", "countdown", "result"], "cancelled")),
        ("outcome", "a cancelled countdown that claims the line was crossed is refused",
         set_path(set_path(set_path(set_path(out3, ["status"], "stopped"), ["stopped_at"], "countdown"), ["reason"], "a key was pressed"), ["cutover", "countdown", "result"], "cancelled")),
        ("outcome", "an erase consent with another sentence is refused",
         set_path(out3, ["erase_consent", "statement"], "delete it all")),
        ("outcome", "an unknown stage name is refused",
         set_path(set_path(set_path(out, ["status"], "failed"), ["stopped_at"], "defrag"), ["reason"], "x")),
    ]
    for kind, name, doc in negatives:
        errs = errors(v[kind], doc)
        report(bool(errs), f"refused: {name}", "document was ACCEPTED")

    print()
    if failed:
        print(f"  {failed} failed")
        print()
        sys.exit(1)
    print("  all checks passed")
    print()


if __name__ == "__main__":
    main()
