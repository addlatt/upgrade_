#!/usr/bin/env python3
"""One click test: the test manager (roadmap item 21; R9, V13).

The G16 drives a machine under test through every stage over SSH, using the
product's own programs (decided 2026-10-27, the owner). The machine needs
nothing of the test's own except SSH set up once; every approval is typed in
full once, at kickoff.

  oneclick.py kickoff --plan PLAN.json --transport ssh:aspire|hyperv:UPGRIGHV
  oneclick.py run    RUN_DIR      (resume a run where its journal stopped)
  oneclick.py status RUN_DIR

The rules it keeps:
  - Stages run in the plan's order, most likely in the wild first (the
    owner, 2026-10-27: a fail late in the chain matters less).
  - The journal line for a stage is written and flushed BEFORE the stage
    starts. A stage that writes to a disk and was interrupted is never
    re-run: the machine's state is unknown, so the run stops (rule #1).
  - Silence past a stage's limit is a hang, never a pass. While waiting,
    the manager asks the machine who it is every `heartbeat_s` and writes
    the answer down.
  - On the first unexpected result the chain stops and keeps everything
    (the owner, 2026-10-27).
  - A run folder is never overwritten; a stage tried again gets a new
    folder. Every stage folder ends with a SHA256SUMS.
  - The run folder lives under rig/hyperv/artifacts/runs/, which git
    ignores: it holds the test account's password and the machine's facts.
"""

import argparse
import datetime
import hashlib
import json
import os
import re
import secrets as pysecrets
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import transport as tr  # noqa: E402

VERSION = "0.1.0"
HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(HERE, "..", ".."))


def utc():
    return datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def stamp():
    return datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ")


def runs_root():
    """rig/hyperv/artifacts/runs in the main checkout: artifacts are
    gitignored, so a worktree has none of its own."""
    try:
        common = subprocess.run(["git", "-C", REPO, "rev-parse", "--path-format=absolute", "--git-common-dir"],
                                capture_output=True, text=True, check=True).stdout.strip()
        base = os.path.dirname(common)
    except (subprocess.CalledProcessError, OSError):
        base = REPO
    return os.environ.get("ONECLICK_RUNS", os.path.join(base, "rig", "hyperv", "artifacts", "runs"))


# --- the sentences, read from the product's own source (one home per fact) --

SENTENCE_SOURCES = {
    "data-loss": ("evaluate/job/src/lib.rs", r'pub const RISK_STATEMENT: &str = "([^"]+)"'),
    "erase": ("evaluate/job/src/lib.rs", r'pub const ERASE_STATEMENT: &str = "([^"]+)"'),
    "walkaway": ("settle-in/src/walkaway.rs", r'pub const SENTENCE: &str = "([^"]+)"'),
}


def sentence(key, repo=REPO):
    path, rx = SENTENCE_SOURCES[key]
    with open(os.path.join(repo, path), encoding="utf-8") as f:
        m = re.search(rx, f.read())
    if not m:
        raise SystemExit(f"oneclick: the {key} sentence is not where it was ({path}); refusing to guess it")
    return m.group(1)


# --- the journal -----------------------------------------------------------

class Journal:
    def __init__(self, path):
        self.path = path

    def write(self, event, **detail):
        line = json.dumps({"utc": utc(), "event": event, **detail}, sort_keys=True)
        with open(self.path, "a", encoding="utf-8") as f:
            f.write(line + "\n")
            f.flush()
            os.fsync(f.fileno())
        return line

    def entries(self):
        if not os.path.exists(self.path):
            return []
        with open(self.path, encoding="utf-8") as f:
            return [json.loads(line) for line in f if line.strip()]


def stage_states(entries):
    """index -> the last event for that stage ('stage-start', 'stage-done' ...)."""
    states = {}
    for e in entries:
        if e["event"].startswith("stage-"):
            states[e["index"]] = e
    return states


def next_action(plan, entries):
    """What the run does now, from its journal alone:
    ('run', i) | ('stop', why) | ('finished', None)."""
    if any(e["event"] == "run-stopped" for e in entries):
        last = [e for e in entries if e["event"] == "run-stopped"][-1]
        return ("stop", last.get("why", "stopped"))
    states = stage_states(entries)
    for i, st in enumerate(plan["stages"]):
        e = states.get(i)
        if e is None:
            return ("run", i)
        if e["event"] == "stage-done":
            if e.get("expected"):
                continue
            return ("stop", f"stage {i} ({st['stage']}) ended '{e.get('word')}', not what the plan expects")
        if e["event"] == "stage-start":
            if STAGES.get(st["stage"], NotBuilt).writes:
                return ("stop", f"stage {i} ({st['stage']}) writes to a disk and was interrupted: the machine's state is unknown")
            return ("run", i)
        return ("stop", f"stage {i} ({st['stage']}): {e['event']}")
    return ("finished", None)


# --- what every stage gets -------------------------------------------------

class Context:
    def __init__(self, run_dir, plan, secrets, link, journal, sleep=time.sleep, clock=time.monotonic):
        self.run_dir, self.plan, self.secrets, self.link, self.journal = run_dir, plan, secrets, link, journal
        self.sleep, self.clock = sleep, clock
        self.stage_dir = None

    def note(self, text, **kv):
        self.journal.write("note", text=text, **kv)

    def wait_for(self, what, predicate, limit_s, every_s=30):
        """Poll predicate() until it is truthy or limit_s passes. Asks the
        machine who it is every heartbeat_s and writes it down. Returns the
        predicate's value, or None on a hang."""
        beat = self.plan.get("heartbeat_s", 300)
        start = self.clock()
        last_beat = start - beat
        while True:
            v = predicate()
            if v:
                return v
            now = self.clock()
            if now - last_beat >= beat:
                self.journal.write("heartbeat", waiting_for=what, answered=self.link.probe(), elapsed_s=int(now - start))
                last_beat = now
            if now - start >= limit_s:
                self.journal.write("hang", waiting_for=what, limit_s=limit_s)
                return None
            self.sleep(every_s)

    def save(self, name, text):
        p = os.path.join(self.stage_dir, name)
        with open(p, "w", encoding="utf-8") as f:
            f.write(text)
        return p


class Outcome:
    def __init__(self, word, expected, notes=""):
        self.word, self.expected, self.notes = word, expected, notes


def sha256sums(folder):
    lines = []
    for root, _, files in os.walk(folder):
        for n in sorted(files):
            if n == "SHA256SUMS":
                continue
            p = os.path.join(root, n)
            h = hashlib.sha256()
            with open(p, "rb") as f:
                for chunk in iter(lambda: f.read(1 << 20), b""):
                    h.update(chunk)
            lines.append(f"{h.hexdigest()}  ./{os.path.relpath(p, folder)}")
    with open(os.path.join(folder, "SHA256SUMS"), "w", encoding="utf-8") as f:
        f.write("\n".join(sorted(lines, key=lambda l: l[66:])) + ("\n" if lines else ""))
    return len(lines)


# --- the stages ------------------------------------------------------------

class NotBuilt:
    """A stage the plan names that the manager cannot run yet. It stops the
    chain with its own word: nothing is skipped in silence."""
    writes = False
    needs = ()

    def __init__(self, name):
        self.name = name

    def run(self, ctx, args):
        return Outcome("not-built", False, f"stage '{self.name}' is not built yet")


FIND_STICK = ("$v = @(Get-Volume -FileSystemLabel UPGV0 -ErrorAction SilentlyContinue | Where-Object DriveLetter); "
              "if ($v.Count -ne 1) { Write-Output \"STICKS=$($v.Count)\"; exit 3 }; $L = \"$($v[0].DriveLetter):\"; ")


class Look:
    """Stage 1, what everyone does: the scan and the machine capture, read
    only. Leaves a capture behind (rule #5: every contact with a real
    machine stays testable)."""
    writes = False
    needs = ()

    def run(self, ctx, args):
        link = ctx.link
        if link.probe() != "windows":
            return Outcome("wrong-system", False, "look needs Windows to answer")
        facts = link.run(
            "$b = Get-CimInstance Win32_BIOS; $c = Get-CimInstance Win32_ComputerSystem; $o = Get-CimInstance Win32_OperatingSystem; "
            "$sb = try { Confirm-SecureBootUEFI } catch { 'unknown' }; "
            "[ordered]@{ manufacturer = $c.Manufacturer; model = $c.Model; bios_vendor = $b.Manufacturer; "
            "bios_version = $b.SMBIOSBIOSVersion; os_caption = $o.Caption; os_build = $o.BuildNumber; secure_boot = \"$sb\" } | ConvertTo-Json",
            "windows", timeout=120)
        if not facts.ok:
            return Outcome("facts-unread", False, facts.err.strip()[:300])
        ctx.save("facts.json", facts.out)
        scan = link.run(FIND_STICK +
                        "& \"$L\\upgrade-scan.exe\" scan --json --out \"$L\\upgrade_\\reports\" --kit \"$L\\\" | Out-Null; $rc = $LASTEXITCODE; "
                        "& \"$L\\upgrade-scan.exe\" dump-machine \"$L\\machine-capture.json\" | Out-Null; "
                        "$r = Get-ChildItem \"$L\\upgrade_\\reports\\upgrade-report-*.json\" | Sort-Object LastWriteTime | Select-Object -Last 1; "
                        "Write-Output \"SCAN_RC=$rc\"; Write-Output \"REPORT=$($r.FullName)\"; Write-Output \"STICK=$L\"",
                        "windows", timeout=int(args.get("limit_s", 900)))
        ctx.save("scan.out", scan.out + scan.err)
        kv = dict(re.findall(r"^(\w+)=(.*)$", scan.out, re.M))
        if "STICKS" in kv:
            return Outcome("stick-not-found", False, f"{kv['STICKS']} volumes labelled UPGV0 (want exactly 1)")
        if not kv.get("REPORT"):
            return Outcome("scan-failed", False, (scan.out + scan.err).strip()[-300:])
        for remote, local in ((kv["REPORT"], "report.json"), (kv["REPORT"][:-5] + ".txt", "report.txt"),
                              (kv["STICK"] + "\\machine-capture.json", "machine-capture.json")):
            p = link.pull(remote, os.path.join(ctx.stage_dir, local), "windows")
            if not p.ok:
                return Outcome("pull-failed", False, f"{remote}: {p.err.strip()[:200]}")
        with open(os.path.join(ctx.stage_dir, "report.json"), encoding="utf-8-sig") as f:
            level = json.load(f).get("Verdict", {}).get("Level", "")
        word = f"scanned-{level.lower() or 'unknown'}"
        want = [w.lower() for w in args.get("expect", ["GREEN", "YELLOW", "RED"])]
        return Outcome(word, level.lower() in want, f"scan exit {kv.get('SCAN_RC')}; verdict {level}")


STAGES = {"look": Look()}


def stage_impl(name):
    return STAGES.get(name) or NotBuilt(name)


# --- the run ---------------------------------------------------------------

def load(run_dir):
    with open(os.path.join(run_dir, "run.json"), encoding="utf-8") as f:
        run = json.load(f)
    sec_path = os.path.join(run_dir, "secrets.json")
    secrets = {}
    if os.path.exists(sec_path):
        with open(sec_path, encoding="utf-8") as f:
            secrets = json.load(f)
    return run, secrets


def run_chain(run_dir, link=None, sleep=time.sleep, clock=time.monotonic, out=print):
    run, secrets = load(run_dir)
    plan = run["plan"]
    link = link or tr.from_spec(run["transport"])
    journal = Journal(os.path.join(run_dir, "journal.jsonl"))
    ctx = Context(run_dir, plan, secrets, link, journal, sleep, clock)
    if not journal.entries():
        journal.write("run-start", manager=VERSION, transport=link.describe(), machine=plan.get("machine", ""))
    while True:
        action, arg = next_action(plan, journal.entries())
        if action == "finished":
            journal.write("run-finished")
            out("oneclick: every stage ran and ended as the plan expects")
            return 0
        if action == "stop":
            if not any(e["event"] == "run-stopped" for e in journal.entries()):
                journal.write("run-stopped", why=arg)
            out(f"oneclick: stopped - {arg}")
            return 1
        i = arg
        st = plan["stages"][i]
        tries = sum(1 for e in journal.entries() if e["event"] == "stage-start" and e["index"] == i)
        name = f"{i + 1:02d}-{st['stage']}" + (f"-try{tries + 1}" if tries else "")
        ctx.stage_dir = os.path.join(run_dir, "stages", name)
        os.makedirs(ctx.stage_dir, exist_ok=False)
        impl = stage_impl(st["stage"])
        journal.write("stage-start", index=i, stage=st["stage"], folder=name)
        out(f"oneclick: stage {i + 1} {st['stage']} ...")
        try:
            o = impl.run(ctx, st)
        except Exception as e:  # a manager bug is a stop, never a pass
            o = Outcome("manager-error", False, f"{type(e).__name__}: {e}")
        n = sha256sums(ctx.stage_dir)
        journal.write("stage-done", index=i, stage=st["stage"], word=o.word, expected=bool(o.expected), notes=o.notes, files=n)
        out(f"oneclick: stage {i + 1} {st['stage']}: {o.word}{'' if o.expected else ' (NOT expected)'} - {o.notes}")


def kickoff(plan_path, spec, ask=input, out=print, root=None):
    with open(plan_path, encoding="utf-8") as f:
        plan = json.load(f)
    link = tr.from_spec(spec)
    secrets = {}
    needed = sorted({n for st in plan["stages"] for n in stage_impl(st["stage"]).needs})
    out(f"oneclick {VERSION}: {len(plan['stages'])} stages on {plan.get('machine', '?')} over {link.describe()}")
    for i, st in enumerate(plan["stages"]):
        built = "" if st["stage"] in STAGES else "   (not built yet: the run stops here)"
        out(f"  {i + 1:2d}. {st['stage']}{built}")
    for key in needed:
        if key == "password":
            secrets["password"] = pysecrets.token_urlsafe(12)
            continue
        s = sentence(key)
        out(f"\nType this sentence exactly, to approve the {key} step for every loop of this run:\n  {s}")
        if ask("> ") != s:
            out("oneclick: that is not the sentence. Nothing started.")
            return None
        secrets[key] = s
    run_dir = os.path.join(root or runs_root(), f"{plan.get('machine', 'machine')}-{stamp()}")
    os.makedirs(run_dir, exist_ok=False)
    commit = subprocess.run(["git", "-C", REPO, "rev-parse", "--short", "HEAD"], capture_output=True, text=True).stdout.strip()
    with open(os.path.join(run_dir, "run.json"), "w", encoding="utf-8") as f:
        json.dump({"manager": VERSION, "commit": commit, "created": utc(), "transport": spec, "plan": plan}, f, indent=2)
    if secrets:
        p = os.path.join(run_dir, "secrets.json")
        fd = os.open(p, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(fd, "w", encoding="utf-8") as f:
            json.dump(secrets, f)
    out(f"oneclick: run folder {run_dir}")
    return run_dir


def status(run_dir, out=print):
    run, _ = load(run_dir)
    entries = Journal(os.path.join(run_dir, "journal.jsonl")).entries()
    states = stage_states(entries)
    for i, st in enumerate(run["plan"]["stages"]):
        e = states.get(i)
        word = "-" if e is None else (e.get("word") or e["event"])
        out(f"  {i + 1:2d}. {st['stage']:<14} {word}")
    action, arg = next_action(run["plan"], entries)
    out(f"  next: {action}{' - ' + str(arg) if arg is not None else ''}")


def main(argv=None):
    ap = argparse.ArgumentParser(prog="oneclick.py")
    sub = ap.add_subparsers(dest="cmd", required=True)
    k = sub.add_parser("kickoff")
    k.add_argument("--plan", required=True)
    k.add_argument("--transport", required=True)
    k.add_argument("--no-run", action="store_true", help="make the run folder only")
    r = sub.add_parser("run")
    r.add_argument("run_dir")
    s = sub.add_parser("status")
    s.add_argument("run_dir")
    a = ap.parse_args(argv)
    if a.cmd == "kickoff":
        d = kickoff(a.plan, a.transport)
        if d is None:
            return 2
        return 0 if a.no_run else run_chain(d)
    if a.cmd == "run":
        return run_chain(a.run_dir)
    status(a.run_dir)
    return 0


if __name__ == "__main__":
    sys.exit(main())
