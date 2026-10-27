"""The one click test's judge and its privacy gate (roadmap item 21, steps 4
and 6; R9, V13).

One judge for every stage: each finished stage of a run becomes one row of
docs/validation-results/oneclick-runs.csv, built from the run folder alone.
Nothing is typed: the harness (rig or physical) is the transport, the
machine and firmware are what the machine said about itself (facts.json),
and the row names the stage folder's SHA256SUMS by its own hash, so a row
points at exactly one set of evidence.

The privacy gate: before a row leaves the run folder, every value the run
knows to be personal (the test password, account names, Wi-Fi names, disk
serials, computer and SSH host names) is replaced by a placeholder, and the
rows are scanned again; a row that still holds one is refused, never
softened (CLAUDE.md: never commit a machine's or a person's details).

  oneclick.py promote RUN_DIR [--csv PATH] [--facts FACTS.json] [--dry-run]
"""

import csv
import hashlib
import io
import json
import os
import re

# the bench's own made-up names: no person's (the rig's account is "rig", rig/hyperv/v1-job.py)
BENCH_NAMES = {"rig"}
# the columns a person's detail can reach: the rest are the judge's own words and numbers
FREE_TEXT = ("machine", "firmware", "notes")

HEADER = ["timestamp", "manager", "harness", "machine", "firmware", "os_build", "run", "stage",
          "result", "class", "evidence_sha256", "notes"]


def class_of(harness, expected, result=""):
    """The project's words (validation-results/README.md): a stage that ended
    as its plan expects is pass on a real machine, pass-plumbing on the rig;
    a stand-in (the rig's fresh image for the walk-away) is stand-in, which
    closes nothing; anything else is fail."""
    if not expected:
        return "fail"
    if result.startswith("rig-standin"):
        return "stand-in"
    return "pass" if harness == "physical" else "pass-plumbing"


def find_facts(run_dir, explicit=None):
    """facts.json: given, or the run's own (recorded at its start), or a look
    stage's. Returns (facts, where)."""
    candidates = [explicit] if explicit else []
    candidates.append(os.path.join(run_dir, "facts.json"))
    stages = os.path.join(run_dir, "stages")
    if os.path.isdir(stages):
        candidates += [os.path.join(stages, d, "facts.json") for d in sorted(os.listdir(stages))]
    for p in candidates:
        if p and os.path.isfile(p):
            with open(p, encoding="utf-8-sig") as f:
                return json.load(f), p
    return {}, None


def personal_values(run_dir, secrets):
    """Every value this run knows to be someone's: (value, placeholder)."""
    found = []

    def add(v, ph):
        if isinstance(v, str) and len(v.strip()) >= 3 and v.strip().lower() not in BENCH_NAMES:
            found.append((v.strip(), ph))

    add(secrets.get("password"), "<password>")
    add(secrets.get("walkaway-account"), "<name>")
    for root, _, files in os.walk(os.path.join(run_dir, "stages")):
        if "job.json" not in files:
            continue
        try:
            with open(os.path.join(root, "job.json"), encoding="utf-8-sig") as f:
                job = json.load(f)
        except (OSError, ValueError):
            continue
        acct = (job.get("intent") or {}).get("account") or {}
        for k in ("windows_name", "linux_name", "full_name"):
            add(acct.get(k), "<name>")
        for p in ((job.get("harvest") or {}).get("wifi") or {}).get("profiles") or []:
            add(p.get("ssid"), "<wifi>")
            add(p.get("name"), "<wifi>")
        disk = (job.get("identity") or {}).get("system_disk") or {}
        add(disk.get("serial_number"), "<serial>")
        add(disk.get("unique_id"), "<disk-id>")
        for d in ((job.get("erase_consent") or {}).get("disks")) or []:
            add(d.get("serial_number"), "<serial>")
        add((job.get("stick") or {}).get("unique_id"), "<disk-id>")
        add((job.get("machine") or {}).get("computer_name"), "<computer>")
    host = os.environ.get("ONECLICK_HOSTNAME") or os.uname().nodename
    add(host, "<computer>")
    # the longest first, so a name inside a longer value is not half-replaced
    return sorted(set(found), key=lambda x: -len(x[0]))


def scrub(text, values):
    for v, ph in values:
        text = re.sub(re.escape(v), ph, text, flags=re.I)
    return text


def leaks(text, values):
    low = text.lower()
    return sorted({ph for v, ph in values if v.lower() in low})


def stage_rows(run_dir, run, entries, facts):
    harness = "physical" if run["transport"].startswith("ssh:") else "rig"
    machine = " ".join(x for x in (facts.get("manufacturer"), facts.get("model")) if x) or "unknown (no facts recorded)"
    firmware = " ".join(x for x in (facts.get("bios_vendor"), facts.get("bios_version")) if x) or "unknown"
    rows = []
    starts = {}
    for e in entries:
        if e["event"] == "stage-start":
            starts[e["index"]] = e.get("folder")
        if e["event"] != "stage-done":
            continue
        folder = starts.get(e["index"], "")
        sums = os.path.join(run_dir, "stages", folder, "SHA256SUMS")
        sha = ""
        if folder and os.path.isfile(sums):
            with open(sums, "rb") as f:
                sha = hashlib.sha256(f.read()).hexdigest()
        rows.append({
            "timestamp": e["utc"], "manager": run.get("manager", ""), "harness": harness,
            "machine": machine, "firmware": firmware, "os_build": str(facts.get("os_build", "")),
            "run": f"{os.path.basename(os.path.normpath(run_dir))}/{folder}", "stage": e["stage"],
            "result": e.get("word", ""), "class": class_of(harness, e.get("expected"), e.get("word", "")),
            "evidence_sha256": sha, "notes": clean_notes(e.get("notes", "")),
        })
    return rows


def clean_notes(text):
    """A stage's note for a row: markup (PowerShell's CLIXML) out, one line, short."""
    text = re.sub(r"#< CLIXML.*", "", text, flags=re.S)
    text = re.sub(r"<[^>]{0,200}>", " ", text)
    text = " ".join(text.split())
    return text[:300]


def scrub_rows(rows, values):
    for r in rows:
        for k in FREE_TEXT:
            r[k] = scrub(r[k], values)
    return rows


def render(rows):
    buf = io.StringIO()
    w = csv.DictWriter(buf, fieldnames=HEADER, quoting=csv.QUOTE_ALL, lineterminator="\n")
    for r in rows:
        w.writerow(r)
    return buf.getvalue()


def promote(run_dir, csv_path, facts_path=None, dry_run=False, out=print):
    with open(os.path.join(run_dir, "run.json"), encoding="utf-8") as f:
        run = json.load(f)
    secrets = {}
    sp = os.path.join(run_dir, "secrets.json")
    if os.path.exists(sp):
        with open(sp, encoding="utf-8") as f:
            secrets = json.load(f)
    with open(os.path.join(run_dir, "journal.jsonl"), encoding="utf-8") as f:
        entries = [json.loads(l) for l in f if l.strip()]
    facts, where = find_facts(run_dir, facts_path)
    rows = stage_rows(run_dir, run, entries, facts)
    if not rows:
        out("promote: no finished stage in this run; nothing to write")
        return 1
    if facts_path:
        for r in rows:
            r["notes"] = (r["notes"] + f" | facts from {os.path.basename(os.path.dirname(os.path.abspath(facts_path)))}").strip(" |")
    values = personal_values(run_dir, secrets)
    rows = scrub_rows(rows, values)
    text = render(rows)
    left = leaks(" ".join(r[k] for r in rows for k in FREE_TEXT), values)
    if left:
        out(f"promote: REFUSED - personal values still in the rows after scrubbing: {', '.join(left)}")
        return 2
    promoted = os.path.join(run_dir, ".promoted")
    if os.path.exists(promoted) and not dry_run:
        out(f"promote: REFUSED - this run's rows were already written ({open(promoted).read().strip()})")
        return 2
    if dry_run:
        out(text.rstrip("\n"))
        out(f"promote: dry run - {len(rows)} row(s), {len(values)} personal value(s) scrubbed; facts from {where}")
        return 0
    new = not os.path.exists(csv_path)
    if not new:
        with open(csv_path, encoding="utf-8") as f:
            head = f.readline().strip()
        if head != ",".join(f'"{h}"' for h in HEADER):
            out(f"promote: REFUSED - {csv_path} has another header")
            return 2
    with open(csv_path, "a", encoding="utf-8", newline="") as f:
        if new:
            f.write(",".join(f'"{h}"' for h in HEADER) + "\n")
        f.write(text)
    with open(csv_path, encoding="utf-8") as f:
        last = sum(1 for _ in f)
    with open(promoted, "w", encoding="utf-8") as f:
        f.write(f"{os.path.basename(csv_path)} lines {last - len(rows) + 1}-{last}\n")
    out(f"promote: {len(rows)} row(s) appended to {csv_path} (lines {last - len(rows) + 1}-{last}); {len(values)} personal value(s) scrubbed")
    return 0
