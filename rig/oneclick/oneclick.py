#!/usr/bin/env python3
"""One click test: the test manager (roadmap item 21; R9, V13).

The G16 drives a machine under test through every stage over SSH, using the
product's own programs (decided 2026-10-27, the owner). The machine needs
nothing of the test's own except SSH set up once; every approval is typed in
full once, at kickoff.

  oneclick.py kickoff --plan PLAN.json --transport ssh:aspire|hyperv:UPGRIGHV
  oneclick.py run    RUN_DIR      (resume a run where its journal stopped)
  oneclick.py status RUN_DIR
  oneclick.py retry  RUN_DIR [--undone WHAT]   (try a stopped stage again, in the same run)
  oneclick.py ssh    RUN_DIR [COMMAND]         (reach the machine where the run last found it)

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
  - One stick, no swaps (the owner, 2026-11-15): the kit-stick stage writes
    the kit onto the stick while it sits in the machine, once at the start
    and once after the walk-away made it Windows' installer.
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
import shutil
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import transport as tr  # noqa: E402

VERSION = "0.4.1"
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


def stopped_since_retry(entries):
    """A run-stopped line written after the last retry (or ever, with none)."""
    retries = [n for n, e in enumerate(entries) if e["event"] == "stage-retry"]
    start = retries[-1] if retries else -1
    return any(e["event"] == "run-stopped" for e in entries[start + 1:])


def stage_states(entries):
    """index -> the last event for that stage ('stage-start', 'stage-done' ...)."""
    states = {}
    for e in entries:
        if e["event"].startswith("stage-"):
            states[e["index"]] = e
    return states


def next_action(plan, entries):
    """What the run does now, from its journal alone:
    ('run', i) | ('wait', i) | ('stop', why) | ('finished', None).
    'wait': the plan names a step only the owner's hands can do before stage
    i (a stick swapped), and `oneclick.py done` has not said it is done."""
    states = stage_states(entries)
    if any(e["event"] == "run-stopped" for e in entries):
        last = [e for e in entries if e["event"] == "run-stopped"][-1]
        # the one stop a run goes on from: a stage that was not built then and is now. It
        # changed nothing on the machine, so the chain stands exactly where it stood
        done = [e for e in entries if e["event"] == "stage-done"]
        tail = done[-1] if done else None
        rebuilt = tail and tail.get("word") == "not-built" and tail["stage"] in STAGES and entries.index(last) > entries.index(tail)
        # and one stopped for an approval it was not given, once `consent` gave it (nothing ran)
        approved = "needs an approval" in last.get("why", "") and any(
            e["event"] == "consent-given" and entries.index(e) > entries.index(last) for e in entries)
        # and one the owner chose to try again (`oneclick.py retry`, which refuses a stage that may
        # have changed the machine unless the owner says what was undone)
        retried = any(e["event"] == "stage-retry" and entries.index(e) > entries.index(last) for e in entries)
        if not (rebuilt or approved or retried):
            return ("stop", last.get("why", "stopped"))
    states = {i: e for i, e in states.items()
              if not (e["event"] == "stage-done" and e.get("word") == "not-built" and e["stage"] in STAGES)}
    for i, st in enumerate(plan["stages"]):
        e = states.get(i)
        if e is not None and e["event"] == "stage-retry":
            return ("run", i)
        if e is None:
            if st.get("owner_step") and not any(x["event"] == "owner-done" and x.get("index") == i for x in entries):
                return ("wait", i)
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
    """untouched: the stage stopped before it changed anything on the
    machine's drives (a job refused, a stick not found, the wrong system
    answering). Only such a stop may be tried again without the owner saying
    what was undone (`oneclick.py retry`)."""
    def __init__(self, word, expected, notes="", untouched=False):
        self.word, self.expected, self.notes, self.untouched = word, expected, notes, untouched


def untouched(word, notes):
    return Outcome(word, False, notes, untouched=True)


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
            return untouched("wrong-system", "look needs Windows to answer")
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
            return untouched("stick-not-found", f"{kv['STICKS']} volumes labelled UPGV0 (want exactly 1)")
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


def b64(text):
    import base64
    return base64.b64encode(text.encode("utf-8")).decode("ascii")


def sudo(ctx, script, timeout=600):
    """A root command on the machine's Linux: the test account's password
    on sudo's stdin (approved at kickoff, the owner, 2026-10-27). The
    password and the script travel base64, never as shell words."""
    pw = ctx.secrets.get("password") or ctx.secrets.get("rig_password", "")
    return ctx.link.run(f"echo {b64(pw + chr(10))} | base64 -d | sudo -S -p '' bash -c \"$(echo {b64(script)} | base64 -d)\"",
                        "linux", timeout)


def windows_script(name, *args):
    """A rig/hyperv/physical PowerShell script run as a script block: the
    machine's execution policy is never touched."""
    with open(os.path.join(REPO, "rig", "hyperv", "physical", name), encoding="utf-8") as f:
        body = f.read()
    return "& {\n" + body + "\n} " + " ".join(args)


def collect_windows(ctx, via, tag):
    r = ctx.link.run(windows_script("collect-windows.ps1", "-OutRoot", "C:\\upgrade_oneclick\\collect", "-Via", via) +
                     "; $d = Get-ChildItem C:\\upgrade_oneclick\\collect -Directory | Sort-Object Name | Select-Object -Last 1; "
                     "Write-Output \"COLLECTED=$($d.FullName)\"", "windows", timeout=900)
    ctx.save(f"collect-{tag}.out", r.out + r.err)
    m = re.search(r"^COLLECTED=(.+)$", r.out, re.M)
    if not m:
        return False
    return ctx.link.pull(m.group(1).strip(), os.path.join(ctx.stage_dir, f"windows-{tag}"), "windows").ok


def collect_linux(ctx, tag):
    """collect-linux.sh as root, then its folder as one tarball pulled back.
    (It also copies the stick's upgrade_/: the privacy step, roadmap item
    21 step 6, filters before anything leaves the run folder.)"""
    with open(os.path.join(REPO, "rig", "hyperv", "physical", "collect-linux.sh"), encoding="utf-8") as f:
        body = f.read()
    r = sudo(ctx, f"cat > /tmp/upg-collect.sh <<'UPGEOF'\n{body}\nUPGEOF\n"
                  "bash /tmp/upg-collect.sh --esp >/tmp/upg-collect.out 2>&1; rc=$?; "
                  "tar -C \"/home/$SUDO_USER\" -czf /tmp/upg-evidence.tgz upgrade-evidence && rm -rf \"/home/$SUDO_USER/upgrade-evidence\"; "
                  "chmod 644 /tmp/upg-evidence.tgz /tmp/upg-collect.out; echo COLLECT_RC=$rc", timeout=1800)
    ctx.save(f"collect-{tag}.out", r.out + r.err)
    ok = ctx.link.pull("/tmp/upg-evidence.tgz", os.path.join(ctx.stage_dir, f"linux-{tag}.tgz"), "linux").ok
    ctx.link.pull("/tmp/upg-collect.out", os.path.join(ctx.stage_dir, f"linux-{tag}-collect.log"), "linux")
    sudo(ctx, "rm -f /tmp/upg-evidence.tgz /tmp/upg-collect.out /tmp/upg-collect.sh")
    return ok


def boot_mark(ctx, os_kind):
    """Which boot this is: Linux's boot_id, Windows' last boot time."""
    if os_kind == "linux":
        r = ctx.link.run("cat /proc/sys/kernel/random/boot_id", "linux", 30)
    else:
        r = ctx.link.run("(Get-CimInstance Win32_OperatingSystem).LastBootUpTime.ToUniversalTime().ToString('o')", "windows", 30)
    return (os_kind, r.out.strip()) if r.ok and r.out.strip() else None


def wait_new_boot(ctx, what, before, limit_s, every_s=30):
    """After a restart was asked for: the system that answers from a NEW
    boot, or None on a hang. The old boot still answering is not an
    answer (a slow restart must never read as 'Linux came up first')."""
    def answered():
        who = ctx.link.probe()
        if who is None:
            return None
        mark = boot_mark(ctx, who)
        return who if mark is not None and mark != before else None
    return ctx.wait_for(what, answered, limit_s, every_s)


# GRUB's tools by either family's names: grub2-reboot and grub2-editenv on Fedora,
# grub-reboot and grub-editenv on Ubuntu (Fedora and Ubuntu in parallel, the owner, 2026-11-13)
GRUB_TOOLS = ("GR=$(command -v grub2-reboot || command -v grub-reboot); GE=$(command -v grub2-editenv || command -v grub-editenv); "
              "[ -n \"$GR\" ] && [ -n \"$GE\" ] || { echo NO_GRUB_TOOLS; exit 5; }; ")


def boot_windows_from_linux(ctx):
    """Linux -> Windows the way a person does it in the product: GRUB's
    'Windows (kept)' entry, chosen for one start (grub2-reboot). Choosing it
    sets GRUB's upg_windows_chosen flag, which the return step reads so it
    does not mistake the start for a firmware that ignores Linux.

    Not a firmware BootNext: the rig, 2026-10-27, showed the return step
    reads a firmware-chosen Windows start as InsydeH2O's symptom, puts shim in
    Windows' slot and restarts, on a firmware that honours Fedora's entry
    (R21: the detector cannot tell that from a person's F12 menu)."""
    r = sudo(ctx, GRUB_TOOLS + "\"$GR\" 'Windows (kept)' && \"$GE\" list | grep -q '^next_entry=Windows (kept)$' || { echo NO_GRUB_WINDOWS_ENTRY; exit 3; }; "
                  "echo NEXT_ENTRY=Windows-kept; systemd-run --on-active=5 systemctl reboot >/dev/null; echo REBOOTING")
    return "REBOOTING" in r.out, r.out + r.err


def first_mib(ctx, folder):
    """sha256 of the system drive's first MiB in a Windows collector's folder."""
    for root, _, files in os.walk(os.path.join(ctx.stage_dir, folder)):
        if "first-mib-system.raw" in files:
            with open(os.path.join(root, "first-mib-system.raw"), "rb") as f:
                return hashlib.sha256(f.read()).hexdigest()
    return None


def find_stick(ctx):
    r = ctx.link.run(FIND_STICK + "Write-Output \"STICK=$L\"", "windows", timeout=60)
    m = re.search(r"^STICK=(\w:)$", r.out, re.M)
    return m.group(1) if m else None


SIGNED_IN = ("$me = $env:USERNAME; $d = @(Get-CimInstance Win32_Process -Filter \"Name='explorer.exe'\" | "
             "ForEach-Object { (Invoke-CimMethod -InputObject $_ -MethodName GetOwner).User } | Where-Object { $_ -eq $me }); "
             "if ($d.Count -gt 0) { 'SIGNED_IN' } else { 'NOBODY' }")


def wait_signed_in(ctx, limit_s=21600):
    """The job writer reads the folders of whoever is signed in on the
    screen, and refuses when nobody is (it cannot tell whose folders they
    are). A person is at their desktop when they click; after the manager's
    restarts the screen sits at the sign-in. So the manager waits for the
    owner to sign in (the Aspire, 2026-10-28: refused at the sign-in screen,
    nothing changed), and the journal says what it waits for."""
    if signed_in(ctx):
        return True
    if ctx.plan.get("test_autologon") and getattr(ctx.link, "kind", "") == "ssh":
        how = sign_in_by_itself(ctx)
        if how == "signed-in":
            return True
        if how == "wrong-system":
            return False
    ctx.note("waiting for the owner to sign in on the machine's screen (the job writer needs the person at the desktop)")
    return bool(ctx.wait_for("someone signed in on the screen", lambda: signed_in(ctx), limit_s, every_s=30))


def signed_in(ctx):
    return "SIGNED_IN" in ctx.link.run(SIGNED_IN, "windows", 60).out


# --- the owner's hands, seen by the machine ----------------------------------
#
# The Aspire, 2026-10-28/29: twice the run waited for the owner to say "sticks
# swapped". A plan's owner_check names what the machine itself can see once
# the step is done; the manager polls it, wants it true twice in a row (a stick
# half in is not a swap), writes owner-done, and goes on by itself.

def kit_stick_in(ctx):
    """Windows answers, sees exactly one kit stick, and no walk-away stick
    (the one holding upgrade_\\go-back.json starts Windows Setup if left in)."""
    if ctx.link.probe() != "windows":
        return False
    r = ctx.link.run(FIND_STICK + "$w = @(Get-Volume | Where-Object { $_.DriveLetter -and (Test-Path \"$($_.DriveLetter):\\upgrade_\\go-back.json\") }); "
                     "if ($w.Count -eq 0) { 'KIT_ONLY' }", "windows", 60)
    return "KIT_ONLY" in r.out


def walkaway_stick_in(ctx):
    """Linux answers, the kit stick is out, and settle-in offers exactly one
    stick for the walk-away (with both in, it rightly refuses both)."""
    if ctx.link.probe() != "linux":
        return False
    r = sudo(ctx, f"test -e /dev/disk/by-label/UPGV0 && echo KIT_IN; {SETTLE_IN} go-back sticks --min-bytes {WALKAWAY_MIN_STICK}", 120)
    if "KIT_IN" in r.out:
        return False
    try:
        sticks = json.loads(r.out.strip().splitlines()[-1])
    except (ValueError, IndexError):
        return False
    return sum(1 for d in sticks if isinstance(d, dict) and d.get("offered")) == 1


def fetch_status(ctx):
    """The fetch program's own word on whether the stick carries the plan's
    Linux (`upgrade-fetch status`, the call the window makes)."""
    lx = ctx.plan.get("linux") or {}
    r = ctx.link.run(FIND_STICK + f"& \"$L\\upgrade-fetch.exe\" status --release {lx.get('release', '')} --desktop {lx.get('desktop', '')}; "
                     "Write-Output \"FETCH_RC=$LASTEXITCODE\"", "windows", 600)
    return bool(re.search(r"^FETCH_RC=0\s*$", r.out, re.M)), clean(r.out + r.err)


def linux_on_stick(ctx):
    """Windows answers and the one kit stick carries the plan's Linux, every
    file checked: the owner's fill through the window (UPGRADE.exe) is done."""
    return ctx.link.probe() == "windows" and fetch_status(ctx)[0]


OWNER_CHECKS = {"kit-stick-in": kit_stick_in, "walkaway-stick-in": walkaway_stick_in, "linux-on-stick": linux_on_stick}


def wait_owner_seen(ctx, st, limit_s):
    """True once the plan's owner_check holds twice in a row, 30 s apart."""
    check = OWNER_CHECKS[st["owner_check"]]
    seen = [0]

    def ready():
        seen[0] = seen[0] + 1 if check(ctx) else 0
        return seen[0] >= 2
    return bool(ctx.wait_for(f"the owner: {st['owner_step']}", ready, limit_s, every_s=30))


# --- the test's automatic sign-in --------------------------------------------
#
# The owner, 2026-10-28, after signing in by hand twice: "that kinda kills the
# thought of '1 click'". A person is at their desktop when they click; the
# test's restarts leave the screen at the sign-in. With the plan's
# test_autologon the manager signs Windows in the way Windows' own automatic
# sign-in does (Sysinternals Autologon's way: the password as the LSA secret
# DefaultPassword, never in the registry as text), restarts once, and says so
# in every row. It is switched off when the run finishes. A fresh walk-away
# Windows has an account with no password that Windows asks to choose at the
# first sign-in (settle-in/gate, /logonpasswordchg:yes): the test chooses the
# run's test password there, as the person would, and says so.

LSA = r"""
using System; using System.Runtime.InteropServices;
public static class UpgLsa {
  [StructLayout(LayoutKind.Sequential)] struct U { public ushort Length; public ushort MaximumLength; public IntPtr Buffer; }
  [StructLayout(LayoutKind.Sequential)] struct A { public int Length; public IntPtr RootDirectory; public IntPtr ObjectName; public uint Attributes; public IntPtr SecurityDescriptor; public IntPtr SecurityQualityOfService; }
  [DllImport("advapi32.dll")] static extern uint LsaOpenPolicy(IntPtr s, ref A a, uint access, out IntPtr h);
  [DllImport("advapi32.dll")] static extern uint LsaStorePrivateData(IntPtr h, ref U key, IntPtr data);
  [DllImport("advapi32.dll")] static extern uint LsaRetrievePrivateData(IntPtr h, ref U key, out IntPtr data);
  [DllImport("advapi32.dll")] static extern uint LsaClose(IntPtr h);
  [DllImport("advapi32.dll")] static extern uint LsaFreeMemory(IntPtr p);
  [DllImport("advapi32.dll")] static extern int LsaNtStatusToWinError(uint s);
  static U Str(string s) { U u = new U(); u.Buffer = Marshal.StringToHGlobalUni(s); u.Length = (ushort)(s.Length * 2); u.MaximumLength = (ushort)(s.Length * 2 + 2); return u; }
  static IntPtr Open() { A a = new A(); a.Length = Marshal.SizeOf(typeof(A)); IntPtr h; uint r = LsaOpenPolicy(IntPtr.Zero, ref a, 0x00000F0FFF, out h); if (r != 0) throw new Exception("LsaOpenPolicy " + LsaNtStatusToWinError(r)); return h; }
  public static int Store(string name, string value) {
    IntPtr h = Open(); U k = Str(name);
    try {
      if (value == null) { int e = LsaNtStatusToWinError(LsaStorePrivateData(h, ref k, IntPtr.Zero)); return e == 2 ? 0 : e; }
      U v = Str(value); IntPtr p = Marshal.AllocHGlobal(Marshal.SizeOf(typeof(U))); Marshal.StructureToPtr(v, p, false);
      try { return LsaNtStatusToWinError(LsaStorePrivateData(h, ref k, p)); } finally { Marshal.FreeHGlobal(p); Marshal.FreeHGlobal(v.Buffer); }
    } finally { Marshal.FreeHGlobal(k.Buffer); LsaClose(h); }
  }
  public static bool Has(string name) {
    IntPtr h = Open(); U k = Str(name); IntPtr d;
    try { uint r = LsaRetrievePrivateData(h, ref k, out d); if (r == 0 && d != IntPtr.Zero) { LsaFreeMemory(d); return true; } return false; }
    finally { Marshal.FreeHGlobal(k.Buffer); LsaClose(h); }
  }
}
"""

WINLOGON = "HKLM:\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Winlogon"

AUTOLOGON_ON = ("$ErrorActionPreference = 'Stop'; $ProgressPreference = 'SilentlyContinue'; $pw = [Console]::In.ReadLine(); $me = $env:USERNAME; "
                f"$k = '{WINLOGON}'; $cur = Get-ItemProperty $k; "
                "if ($cur.DefaultPassword) { 'AUTOLOGON=PLAINTEXT-PRESENT'; exit 3 }; "
                "if ($cur.AutoAdminLogon -eq '1' -and $cur.DefaultUserName -ne $me) { 'AUTOLOGON=SOMEONE-ELSE'; exit 3 }; "
                "'BEFORE=' + ([ordered]@{ AutoAdminLogon = $cur.AutoAdminLogon; DefaultUserName = $cur.DefaultUserName; DefaultDomainName = $cur.DefaultDomainName } | ConvertTo-Json -Compress); "
                "$u = [ADSI]\"WinNT://./$me,user\"; "
                "if ($u.PasswordExpired.Value -eq 1) { $u.SetPassword($pw); $u.PasswordExpired = 0; $u.SetInfo(); 'FIRST_PASSWORD=CHOSEN' } else { "
                "Add-Type -AssemblyName System.DirectoryServices.AccountManagement; "
                "$pc = New-Object System.DirectoryServices.AccountManagement.PrincipalContext('Machine'); "
                "if (-not $pc.ValidateCredentials($me, $pw)) { 'AUTOLOGON=WRONG-PASSWORD'; exit 4 } }; "
                "Add-Type -TypeDefinition $lsa; $e = [UpgLsa]::Store('DefaultPassword', $pw); if ($e -ne 0) { \"AUTOLOGON=LSA-$e\"; exit 5 }; "
                "Set-ItemProperty $k AutoAdminLogon '1'; Set-ItemProperty $k DefaultUserName $me; Set-ItemProperty $k DefaultDomainName $env:COMPUTERNAME; "
                "if ([UpgLsa]::Has('DefaultPassword') -and (Get-ItemProperty $k).AutoAdminLogon -eq '1') { 'AUTOLOGON=SET' }")


def autologon_off_script(before):
    """Winlogon's three values put back as they were, and the secret removed."""
    sets = []
    for name in ("AutoAdminLogon", "DefaultUserName", "DefaultDomainName"):
        v = (before or {}).get(name)
        sets.append(f"Remove-ItemProperty $k {name} -ErrorAction SilentlyContinue" if v is None
                    else f"Set-ItemProperty $k {name} ([Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{b64(v)}')))")
    return ("$ErrorActionPreference = 'Stop'; $ProgressPreference = 'SilentlyContinue'; "
            f"$k = '{WINLOGON}'; " + "; ".join(sets) + "; "
            "Add-Type -TypeDefinition $lsa; $e = [UpgLsa]::Store('DefaultPassword', $null); "
            "if ($e -eq 0 -and -not [UpgLsa]::Has('DefaultPassword')) { 'AUTOLOGON=OFF' } else { \"AUTOLOGON=LSA-$e\" }")


def with_lsa(script):
    return f"$lsa = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{b64(LSA)}')); " + script


def autologon_state(entries):
    """The automatic sign-in this run set and has not switched off: the
    journal's last autologon-set (with what was there before), or None."""
    on = None
    for e in entries:
        if e["event"] == "autologon-set":
            on = e
        elif e["event"] == "autologon-off":
            on = None
    return on


def autologon_on(ctx):
    """Windows' automatic sign-in for the SSH login's account. Returns
    'SET' or the word that refused it (nothing changed then)."""
    pw = ctx.secrets.get("windows-password")
    if not pw:
        return "NO-PASSWORD"
    r = ctx.link.run(with_lsa(AUTOLOGON_ON), "windows", 120, stdin=pw + "\n")
    word = (re.findall(r"^AUTOLOGON=(\S+)", r.out, re.M) or ["UNKNOWN"])[-1]
    if word != "SET":
        ctx.note(f"the test's automatic sign-in was not set: {word}", out=clean(r.out + r.err)[-200:])
        return word
    m = re.search(r"^BEFORE=(\{.*\})$", r.out, re.M)
    before = json.loads(m.group(1)) if m else {}
    if not autologon_state(ctx.journal.entries()):
        ctx.journal.write("autologon-set", before=before, first_password="FIRST_PASSWORD=CHOSEN" in r.out)
    if "FIRST_PASSWORD=CHOSEN" in r.out:
        ctx.note("the TEST chose the fresh Windows account's password at its first sign-in (the run's test password), as the person would")
    return "SET"


def autologon_off(ctx_or_link, journal, out=print):
    on = autologon_state(journal.entries())
    if not on:
        return True
    link = ctx_or_link
    if link.probe() != "windows":
        out("oneclick: the test's automatic sign-in is still set in the machine's Windows (Windows does not answer now): "
            "oneclick.py autologon-off RUN_DIR once it does")
        return False
    r = link.run(with_lsa(autologon_off_script(on.get("before"))), "windows", 120)
    if "AUTOLOGON=OFF" not in r.out:
        out(f"oneclick: switching the automatic sign-in off did not finish: {clean(r.out + r.err).strip()[-200:]}")
        return False
    journal.write("autologon-off")
    out("oneclick: the test's automatic sign-in is off again")
    return True


def sign_in_by_itself(ctx):
    """'signed-in', 'not-signed-in' (the owner can still sign in) or
    'wrong-system' (the restart reached something else: the stage stops)."""
    if autologon_on(ctx) != "SET":
        return "not-signed-in"
    before = boot_mark(ctx, "windows")
    ctx.note("restarting Windows once so the test's automatic sign-in signs it in")
    ctx.link.run("shutdown /r /t 5", "windows", 60)
    got = wait_new_boot(ctx, "Windows, restarted to sign itself in", before, 1800)
    if got != "windows":
        ctx.note(f"after the restart for the automatic sign-in, {got or 'nothing'} answered")
        return "wrong-system"
    if ctx.wait_for("Windows to sign itself in", lambda: signed_in(ctx), 600, every_s=20):
        ctx.note("the TEST signed Windows in by itself (automatic sign-in)")
        return "signed-in"
    ctx.note("the automatic sign-in did not reach the desktop within 10 minutes")
    return "not-signed-in"


DESKTOP_TASK = "upgrade_ oneclick desktop"
DESKTOP_DIR = "C:\\upgrade_oneclick"


def clean(text):
    """PowerShell's progress records over SSH (#< CLIXML ...) are noise, not the answer."""
    return re.sub(r"#< CLIXML.*?</Objs>", "", text, flags=re.S)


def on_desktop(link, command, until, timeout):
    """`command` (a cmd.exe line) run the way the window runs the product: in
    the signed-in person's own session, as administrator. An SSH login is a
    session of its own, with no desktop, and the job writer rightly refuses
    there: it compares its session's desktop owner with itself (the Aspire,
    2026-10-28). So a task with an interactive principal runs it, writing a
    log; this waits on the machine until the log holds `until` or the
    command's exit (DESKTOP_RC=n), then returns the log. The prologue's
    `until` is its restart line: the log stops there, the machine restarts."""
    log = f"{DESKTOP_DIR}\\desktop.log"
    # brackets: a compound command is logged whole, and "RC=0>>" is never read as handle 0's redirect
    body = f"@echo off\r\ncd /d {DESKTOP_DIR}\r\n({command}) > {log} 2>&1\r\n(echo DESKTOP_RC=%ERRORLEVEL%)>> {log}\r\n"
    ps = (f"$c = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{b64(body)}')); "
          f"New-Item -ItemType Directory -Force {DESKTOP_DIR} | Out-Null; Set-Content -Path {DESKTOP_DIR}\\desktop.cmd -Value $c -Encoding ASCII; "
          f"Remove-Item {log} -ErrorAction SilentlyContinue; "
          f"$a = New-ScheduledTaskAction -Execute cmd.exe -Argument '/c {DESKTOP_DIR}\\desktop.cmd'; "
          "$p = New-ScheduledTaskPrincipal -UserId $env:USERNAME -LogonType Interactive -RunLevel Highest; "
          "$s = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit (New-TimeSpan -Hours 3); "
          f"Register-ScheduledTask -TaskName '{DESKTOP_TASK}' -Action $a -Principal $p -Settings $s -Force | Out-Null; "
          f"Start-ScheduledTask -TaskName '{DESKTOP_TASK}'; $end = (Get-Date).AddSeconds({max(60, timeout - 60)}); $log = ''; "
          f"while ((Get-Date) -lt $end) {{ Start-Sleep 5; $log = [string](Get-Content {log} -Raw -ErrorAction SilentlyContinue); "
          f"if ($log -match '{re.escape(until)}|DESKTOP_RC=') {{ break }} }}; Write-Output $log"
          f" <# on the desktop: {command.split(' --')[0]} #>")
    r = link.run(ps, "windows", timeout)
    return tr.Result(r.rc, clean(r.out), clean(r.err))


def write_job(ctx, L, args, erase=False):
    """The job. A real machine: the product's job writer over SSH, with the
    sentences typed at kickoff. The rig: the stand-in (the writer refuses the
    rig's SCSI "stick", R16; rig/hyperv/prologue.sh convert says why).
    Returns None, or the Outcome that stops the stage."""
    link = ctx.link
    hashed = subprocess.run(["openssl", "passwd", "-6", "-stdin"], input=ctx.secrets["password"], capture_output=True, text=True).stdout.strip()
    if not hashed.startswith("$6$"):
        return untouched("manager-error", "openssl gave no SHA-512 crypt hash")
    if link.kind == "hyperv":
        pub = os.path.expanduser("~/.ssh/upgrade_rig.pub")
        env = dict(os.environ, ONECLICK_HASH=hashed, ONECLICK_SSH_PUB=pub)
        if any(s.get("stage") == "plant-files" for s in ctx.plan.get("stages", [])):
            env["ONECLICK_FOLDER_MAP"] = "1"   # the stand-in job carries the product harvester's own folder count
        cmd = ["./v9.sh", "job", f"--spoof-ssh={pub}", *args.get("rig_job_args", [])] if erase else ["./v1.sh", "job"]
        j = subprocess.run(cmd, cwd=os.path.join(REPO, "rig", "hyperv"), env=env, capture_output=True, text=True, timeout=1800)
        ctx.save("job-standin.log", j.stdout + j.stderr)
        if j.returncode != 0:
            return untouched("job-failed", "the rig's job stand-in failed: read job-standin.log")
    else:
        if not wait_signed_in(ctx, int(args.get("signin_limit_s", 21600))):
            return untouched("nobody-signed-in", "nobody signed in on the machine's screen within the limit; no job was written")
        hf = os.path.join(ctx.stage_dir, ".hash")
        with open(hf, "w") as f:
            f.write(hashed + "\n")
        link.push(hf, "C:\\upgrade_oneclick\\pw.txt", "windows")
        os.remove(hf)
        ack = ctx.secrets.get("data-loss")
        jw = on_desktop(link,
            f"{L}\\upgrade-job.exe write --stick {L} --out {L}\\upgrade_ --scan {L}\\upgrade_\\reports --desktop {args.get('desktop', ctx.plan.get('desktop', 'kde'))} "
            f"--start-at {args.get('start_at', 'desktop')} --if-cannot-keep stop --password-hash-file C:\\upgrade_oneclick\\pw.txt "
            f"--kickstart {L}\\upgrade_\\ks.cfg --stick-label UPGV0 --manifest {L}\\SHA256SUMS --autoinstall {L}\\autoinstall.yaml" +
            (f" --erase-everything \"{ctx.secrets['erase']}\"" if erase else "") +
            (f" --acknowledge-data-loss \"{ack}\"" if ack else ""), "DESKTOP_RC=", 1800)
        link.run("Remove-Item C:\\upgrade_oneclick\\pw.txt -Force -ErrorAction SilentlyContinue", "windows", timeout=60)
        ctx.save("job-writer.out", jw.out + jw.err)
        if not re.search(r"DESKTOP_RC=0\b", jw.out):
            return untouched("job-refused", (jw.out + jw.err).strip()[-300:])
    link.pull(f"{L}\\upgrade_\\job.json", os.path.join(ctx.stage_dir, "job.json"), "windows")
    return None


def arm(ctx, L, erase=False, markers=()):
    """The bench marker (boot captures and boots.log; never autoshutdown:
    the manager restarts nothing by power), any test markers, the Windows
    collector, then the prologue exactly as the window runs it. Returns
    None when the prologue reached its restart, else the stopping Outcome."""
    link = ctx.link
    extra = "".join(f"Set-Content -Path {L}\\upgrade_\\{n} -Value '{v}'; " for n, v in markers)
    link.run(f"Set-Content -Path {L}\\upgrade_\\bench -Value oneclick; Remove-Item {L}\\upgrade_\\autoshutdown -ErrorAction SilentlyContinue; " + extra +
             f"Add-Content -Path {L}\\upgrade_\\boots.log -Value ('windows-clock-before,' + (Get-Date).ToUniversalTime().ToString('o') + ',host-utc={utc()}')",
             "windows", timeout=60)
    if not collect_windows(ctx, "before-install", "before"):
        return untouched("collect-failed", "the Windows collector before the conversion")
    ack = ctx.secrets.get("data-loss")
    if link.kind == "ssh":   # the person's desktop, as the window runs it (on_desktop)
        consent = f"--erase-consent \"{ctx.secrets['erase']}\"" if erase else "--confirm-word CONVERT"
        conv = on_desktop(link, f"{L}\\upgrade-prologue.exe start --stick {L} {consent}" + (f" --acknowledge-data-loss \"{ack}\"" if ack else ""),
                          "restarting in 15 s", 7200)
    else:
        consent = f"--erase-consent '{ctx.secrets['erase']}'" if erase else "--confirm-word CONVERT"
        conv = link.run(f"& {L}\\upgrade-prologue.exe start --stick {L} {consent}" + (f" --acknowledge-data-loss '{ack}'" if ack else ""),
                        "windows", timeout=7200)
    ctx.save("convert.log", conv.out + conv.err)
    if "restarting in 15 s" not in conv.out:
        link.pull(f"{L}\\upgrade_\\outcome.json", os.path.join(ctx.stage_dir, "outcome-stopped.json"), "windows")
        return Outcome("prologue-stopped", False, (conv.out + conv.err).strip()[-300:])
    return None


def wait_after_arm(ctx, L, want, limit_s):
    """Through the restarts and the installer to the system `want`
    ('linux' after an install, 'windows' after a refusal):
    Windows may answer in between (an update, the disk check, the return);
    a stopped outcome ends the wait early. Returns 'linux' | 'windows' |
    'stopped' | 'returned' (Windows is back to stay after a completed
    install) | None (a hang)."""
    link = ctx.link
    settled = [None]

    def arrived():
        who = link.probe()
        if who == "linux" and want == "linux":
            return "linux"
        if who == "windows":
            # only this job's outcome counts: a stopped record left by an earlier try of the stage
            # ended the wait while the conversion was going (the Aspire, 2026-11-15)
            st = link.run(FIND_STICK + "$j = Get-Content \"$L\\upgrade_\\job.json\" -Raw | ConvertFrom-Json; "
                          "if (Test-Path \"$L\\upgrade_\\outcome.json\") { $o = Get-Content \"$L\\upgrade_\\outcome.json\" -Raw | ConvertFrom-Json; "
                          "if ($o.job_id -eq $j.job_id) { \"OUTCOME=$($o.status)\" } }; "
                          "if (Test-Path \"$L\\upgrade_\\prologue-return.json\") { $r = Get-Content \"$L\\upgrade_\\prologue-return.json\" -Raw | ConvertFrom-Json; "
                          "if ($r.job_id -eq $j.job_id) { if ($r.windows_slot) { 'RETURN=SLOT' } else { 'RETURN=PLAIN' } } }; "
                          "if (Test-Path \"$L\\upgrade_\\report\\refusal.json\") { 'REFUSED' }", "windows", timeout=60)
            # a refusal is ALSO a stopped outcome (the prologue's return writes one): when
            # Windows is what the stage waits for, it comes first (the rig, 2026-10-27)
            if want == "windows" and "REFUSED" in st.out:
                return "windows"
            if "OUTCOME=stopped" in st.out:
                return "stopped"
            # the install completed, the firmware started Windows, and the prologue's return step
            # ran and arranged no restart (its record carries no windows_slot): nothing will start
            # Linux now. The Aspire, 2026-11-14, sat like this while the manager waited out three
            # hours. Wanted twice in the same Windows boot: the return writes its record a moment
            # before it adds windows_slot and restarts
            if want == "linux" and "OUTCOME=completed" in st.out and "RETURN=PLAIN" in st.out:
                mark = boot_mark(ctx, "windows")
                if mark is not None and mark == settled[0]:
                    return "returned"
                settled[0] = mark
            else:
                settled[0] = None
        return None
    return ctx.wait_for(f"the conversion, until {want} answers", arrived, limit_s, every_s=60)


def installed_outcome(ctx):
    out = sudo(ctx, "cat /var/lib/upgrade_/outcome.json", 60)
    ctx.save("outcome.json", out.out)
    try:
        return json.loads(out.out)
    except ValueError:
        return None


class KeepWindows:
    """Stage 2, the default path: the job (keep Windows), the prologue's
    convert, the install beside Windows, settle-in's first start, then both
    systems started twice with no key pressed (V1b, R21, R36, V10)."""
    writes = True
    needs = ("password",)

    def run(self, ctx, args):
        link = ctx.link
        adopt = args.get("adopt_armed")
        if adopt:
            # the manager's own earlier try armed this conversion and then lost its wait to a
            # manager fault; nothing is written or armed again, the wait is taken up (said in the row)
            L = adopt["stick"]
            ctx.journal.write("note", text=f"keep-windows takes up the conversion armed by {adopt['try']}: {adopt['why']}")
        else:
            if link.probe() != "windows":
                return untouched("wrong-system", "keep-windows starts in Windows")
            L = find_stick(ctx)
            if not L:
                return untouched("stick-not-found", "no single volume labelled UPGV0")
            stop = write_job(ctx, L, args) or arm(ctx, L)
            if stop:
                return stop
        got = wait_after_arm(ctx, L, "linux", int(args.get("limit_s", 10800)))
        if got is None:
            return Outcome("hang-install", False, "no Linux within the limit (the journal has the heartbeats)")
        if got == "stopped":
            link.pull(f"{L}\\upgrade_\\outcome.json", os.path.join(ctx.stage_dir, "outcome-stopped.json"), "windows")
            return Outcome("prologue-stopped", False, "the prologue stopped after a restart: outcome-stopped.json")
        if got == "returned":
            link.pull(f"{L}\\upgrade_\\prologue-return.json", os.path.join(ctx.stage_dir, "prologue-return.json"), "windows")
            link.pull(f"{L}\\upgrade_\\outcome.json", os.path.join(ctx.stage_dir, "outcome-on-stick.json"), "windows")
            return Outcome("windows-came-back", False, "the install completed, the firmware started Windows, and the prologue's return arranged no "
                           "restart: nothing will start Linux (prologue-return.json, outcome-on-stick.json)")
        if not ctx.wait_for("settle-in's first start", lambda: "done" in sudo(
                ctx, "test -e /var/lib/upgrade_/settle-in/first-start.done && echo done", 60).out, 1200, every_s=30):
            return Outcome("first-start-missing", False, "Linux answered but settle-in's first start did not finish")
        if not collect_linux(ctx, "l1"):
            return Outcome("collect-failed", False, "the Linux collector after the install")
        # both systems, twice, no key
        for n in (1, 2):
            before = boot_mark(ctx, "linux")
            ok, why = boot_windows_from_linux(ctx)
            ctx.save(f"to-windows-{n}.out", why)
            if not ok:
                return Outcome("grub-windows-entry-missing", False, why.strip()[:200])
            got = wait_new_boot(ctx, f"Windows, cycle {n}", before, 1800)
            if got != "windows":
                return Outcome("windows-not-reached", False, f"cycle {n}: {got or 'nothing'} answered after the BootNext to Windows")
            link.run(FIND_STICK + f"Add-Content -Path \"$L\\upgrade_\\boots.log\" -Value ('windows-boot,' + (Get-Date).ToUniversalTime().ToString('o') + ',host-utc={utc()}')",
                     "windows", timeout=60)
            if not collect_windows(ctx, "direct", f"w{n}"):
                return Outcome("collect-failed", False, f"the Windows collector, cycle {n}")
            before = boot_mark(ctx, "windows")
            link.run("shutdown /r /t 5", "windows", timeout=60)
            got = wait_new_boot(ctx, f"Linux, cycle {n}", before, 1800)
            if got != "linux":
                return Outcome("linux-not-reached", False, f"cycle {n}: {got or 'nothing'} answered; the firmware's default should reach Linux")
            if not collect_linux(ctx, f"l{n + 1}"):
                return Outcome("collect-failed", False, f"the Linux collector, cycle {n}")
        o = installed_outcome(ctx)
        if o is None:
            return Outcome("outcome-unread", False, "the installed system's outcome.json did not read as JSON")
        if o.get("path_taken") != "keep-windows" or not (o.get("windows") or {}).get("kept"):
            return Outcome("outcome-wrong", False, f"path_taken {o.get('path_taken')}, windows.kept {(o.get('windows') or {}).get('kept')}")
        return Outcome("kept-both-start", True, "installed beside Windows; Windows and Linux each started twice with no key"
                       + (f"; the job and the arming are in {adopt['try']} (the manager's wait was taken up again: {adopt['why']})" if adopt else ""))


class CleanSlate:
    """Stage 5: erase and install (V9 arm C, R27). No countdown since
    2026-11-02 (the owner: the last click in the window is the commit line):
    every refusal passes, the installer records commit.json, erases the drives
    the job names, and the new Linux answers."""
    writes = True
    needs = ("password", "erase")

    def run(self, ctx, args):
        link = ctx.link
        if link.probe() != "windows":
            return untouched("wrong-system", "the clean slate starts in Windows")
        L = find_stick(ctx)
        if not L:
            return untouched("stick-not-found", "no single volume labelled UPGV0")
        stop = write_job(ctx, L, args, erase=True) or arm(ctx, L, erase=True)
        if stop:
            return stop
        got = wait_after_arm(ctx, L, "linux", int(args.get("limit_s", 10800)))
        if got is None:
            return Outcome("hang-install", False, "no Linux within the limit (the journal has the heartbeats)")
        if got == "stopped":
            return Outcome("prologue-stopped", False, "the prologue stopped after a restart")
        if got == "returned":
            link.pull(f"{L}\\upgrade_\\prologue-return.json", os.path.join(ctx.stage_dir, "prologue-return.json"), "windows")
            link.pull(f"{L}\\upgrade_\\outcome.json", os.path.join(ctx.stage_dir, "outcome-on-stick.json"), "windows")
            return Outcome("windows-came-back", False, "the install completed, the firmware started Windows, and the prologue's return arranged no "
                           "restart: nothing will start Linux (prologue-return.json, outcome-on-stick.json)")
        if not ctx.wait_for("settle-in's first start", lambda: "done" in sudo(
                ctx, "test -e /var/lib/upgrade_/settle-in/first-start.done && echo done", 60).out, 1200, every_s=30):
            return Outcome("first-start-missing", False, "Linux answered but settle-in's first start did not finish")
        if not collect_linux(ctx, "l1"):
            return Outcome("collect-failed", False, "the Linux collector after the install")
        o = installed_outcome(ctx)
        if o is None:
            return Outcome("outcome-unread", False, "the installed system's outcome.json did not read as JSON")
        if (o.get("windows") or {}).get("kept"):
            return Outcome("outcome-wrong", False, "the outcome says Windows was kept after an erase")
        gate = ((o.get("cutover") or {}).get("hardware") or {}).get("human_gate")
        if gate != "typed-in-the-window":
            return Outcome("outcome-wrong", False, f"the outcome's human_gate is {gate!r}, not typed-in-the-window")
        return Outcome("erased-installed", True, f"path_taken {o.get('path_taken')}; committed at the window's click, the installer erased what the job named")


class StickLeftIn:
    """Stages 3 and 6: the stick left in and started again (R35, V9 arm D).
    A one-time start from the stick (an entry made for it, removed after);
    it must not install again: the installed Linux answers, from a new boot,
    with its partition tables unchanged. Counted as a writer: interrupted, the
    installer may have started, so it is never run again blind."""
    writes = True
    needs = ("password",)

    def run(self, ctx, args):
        link = ctx.link
        if link.probe() != "linux":
            return untouched("wrong-system", "the stick-left-in test starts in the installed Linux")
        # the internal drives only: real disks, not the USB stick, not zram (Fedora's swap in
        # memory, new every boot: the rig, 2026-10-27, read it as "a partition table changed")
        tables = ("s=$(lsblk -no PKNAME \"$(readlink -f /dev/disk/by-label/UPGV0)\" | head -1); "
                  "for d in $(lsblk -dno NAME,TYPE,TRAN | awk '$2==\"disk\" && $3!=\"usb\" && $1!~/^zram/{print $1}'); do "
                  "[ \"$d\" = \"$s\" ] && continue; echo \"$d $(head -c 1048576 /dev/$d | sha256sum | cut -c1-64)\"; done")
        # proof the stick really started: its GRUB writes upg_fired=1 at every start; set it to 0
        # first, so a firmware that ignores the one-time entry is a fail, never a hollow pass
        fired = ("m=$(findmnt -no TARGET /dev/disk/by-label/UPGV0 | head -1); "
                 "if [ -z \"$m\" ]; then mkdir -p /mnt/upgtest && mount /dev/disk/by-label/UPGV0 /mnt/upgtest && m=/mnt/upgtest; fi; "
                 "\"$GE\" \"$m/EFI/BOOT/grubenv\" {op}; \"$GE\" \"$m/EFI/BOOT/grubenv\" list | grep '^upg_fired='; sync")
        z = sudo(ctx, GRUB_TOOLS + fired.format(op="set upg_fired=0"), 120)
        ctx.save("fired-before.txt", z.out + z.err)
        if "upg_fired=0" not in z.out:
            return untouched("stick-env-unwritable", "the stick's grubenv could not be set before the start")
        t0 = sudo(ctx, tables, 120)
        ctx.save("tables-before.txt", t0.out)
        before = boot_mark(ctx, "linux")
        r = sudo(ctx, "s=$(readlink -f /dev/disk/by-label/UPGV0); [ -n \"$s\" ] || { echo NO_STICK; exit 3; }; "
                      "d=/dev/$(lsblk -no PKNAME \"$s\" | head -1); p=$(cat /sys/class/block/$(basename $s)/partition); "
                      "efibootmgr -c -d $d -p $p -l '\\EFI\\BOOT\\BOOTX64.EFI' -L 'upgrade_ test stick' >/dev/null || { echo NO_ENTRY; exit 4; }; "
                      "n=$(efibootmgr | sed -n 's/^Boot\\([0-9A-F]\\{4\\}\\)\\*\\? upgrade_ test stick.*/\\1/p' | head -1); "
                      "efibootmgr -n $n >/dev/null && echo ENTRY=$n; systemd-run --on-active=5 systemctl reboot >/dev/null; echo REBOOTING", 120)
        ctx.save("to-stick.out", r.out + r.err)
        if "REBOOTING" not in r.out:
            return Outcome("stick-entry-failed", False, (r.out + r.err).strip()[:200])
        got = wait_new_boot(ctx, "the installed Linux after the stick's start", before, int(args.get("limit_s", 2700)))
        if got is None:
            return Outcome("hang-after-stick", False, "nothing answered after the start from the stick (an install would be silent too)")
        if got != "linux":
            return Outcome("wrong-system-after-stick", False, f"{got} answered after the start from the stick")
        m = re.search(r"ENTRY=([0-9A-F]{4})", r.out)
        if m:
            sudo(ctx, f"efibootmgr -b {m.group(1)} -B >/dev/null; echo REMOVED", 60)
        z = sudo(ctx, GRUB_TOOLS + fired.format(op="list >/dev/null"), 120)
        ctx.save("fired-after.txt", z.out + z.err)
        if "upg_fired=1" not in z.out:
            return Outcome("stick-not-started", False, "the stick's GRUB did not run: the firmware did not start the stick (upg_fired still 0)")
        t1 = sudo(ctx, tables, 120)
        ctx.save("tables-after.txt", t1.out)
        if not t0.out.strip() or t0.out != t1.out:
            return Outcome("installed-again", False, "a partition table changed after the stick started (or was not read)")
        collect_linux(ctx, "after-stick")
        return Outcome("not-installed-again", True, "the stick started, did not install, and the installed Linux came back unchanged")


BOGUS_HOME = {"role": "home", "serial_number": "NOSUCHDISK01", "unique_id": "60022480ffffffffffffffffffffffff",
              "size_bytes": 68719476736, "friendly_name": "a disk that is not attached", "health_status": "Healthy"}


class RefusedBeforeCountdown:
    """Stage 8 (V9 arm A), the rarest: an erase job that names a drive this
    computer does not have. It must be refused before the erase (by the
    prologue's re-validation or the installer's identity check), Windows must
    come back, and the system drive must be untouched."""
    writes = True
    needs = ("password", "erase")

    def run(self, ctx, args):
        link = ctx.link
        if link.probe() != "windows":
            return untouched("wrong-system", "the refusal test starts in Windows")
        L = find_stick(ctx)
        if not L:
            return untouched("stick-not-found", "no single volume labelled UPGV0")
        before = boot_mark(ctx, "windows")
        if link.kind == "hyperv":
            stop = write_job(ctx, L, dict(args, rig_job_args=["--bogus-home"]), erase=True)
        else:
            stop = write_job(ctx, L, args, erase=True)
            if not stop:
                jp = os.path.join(ctx.stage_dir, "job.json")
                with open(jp, encoding="utf-8-sig") as f:
                    job = json.load(f)
                job["erase_consent"]["disks"].append(dict(BOGUS_HOME))
                bad = os.path.join(ctx.stage_dir, "job-bogus-home.json")
                with open(bad, "w", encoding="utf-8") as f:
                    json.dump(job, f, indent=2)
                if not link.push(bad, f"{L}\\upgrade_\\job.json", "windows").ok:
                    return Outcome("push-failed", False, "the job naming a missing drive did not reach the stick")
        if stop:
            return stop
        stop = arm(ctx, L, erase=True)
        if stop and stop.word == "prologue-stopped":
            words = "the prologue's re-validation refused the job before any restart"
        elif stop:
            return stop
        else:
            got = wait_after_arm(ctx, L, "windows", int(args.get("limit_s", 3600)))
            if got not in ("windows", "stopped"):
                return Outcome("hang-refusal", False, "Windows did not come back after the job naming a missing drive")
            if boot_mark(ctx, "windows") == before:
                return Outcome("not-restarted", False, "the same Windows boot answered")
            words = "the installer refused before the erase"
        link.pull(f"{L}\\upgrade_\\report", os.path.join(ctx.stage_dir, "stick-report"), "windows")
        if os.path.exists(os.path.join(ctx.stage_dir, "stick-report", "commit.json")):
            return Outcome("committed", False, "the installer recorded the commit for a job naming a drive that is not attached")
        if not collect_windows(ctx, "direct", "after-refusal"):
            return Outcome("collect-failed", False, "the Windows collector after the refusal")
        a, b = first_mib(ctx, "windows-before"), first_mib(ctx, "windows-after-refusal")
        if not a or a != b:
            return Outcome("disk-changed", False, f"the system drive's first MiB before {a} after {b}")
        return Outcome("refused-before-countdown", True, words + "; the system drive is unchanged")


class WalkAway:
    """Stage 7 (V11, R33): from the erased computer's Linux, the walk-away
    back to a fresh Windows, through settle-in's own CLI with every word
    typed at kickoff, then Windows Setup and the first sign-in by
    themselves; the manager reaches the new Windows over the SSH key the
    walk-away carries. The rig cannot hold it (Hyper-V has no USB stick with
    a serial): there it is a stand-in, a fresh copy of the rig's Windows
    image, named so, never a walk-away row."""
    writes = True
    needs = ("password", "walkaway", "walkaway-stick", "walkaway-account")

    def run(self, ctx, args):
        link = ctx.link
        if link.kind == "hyperv":
            return self.rig_standin(ctx)
        if link.probe() != "linux":
            return untouched("wrong-system", "the walk-away starts in Linux")
        plan = sudo(ctx, f"{SETTLE_IN} go-back walkaway plan", 300)
        ctx.save("walkaway-plan.json", plan.out + plan.err)
        try:
            plan_v = json.loads(plan.out.strip().splitlines()[-1])
            drives, language = plan_v.get("drives"), plan_v.get("language") or "en-us"
        except (ValueError, IndexError, AttributeError):
            return untouched("walkaway-plan-unread", (plan.out + plan.err).strip()[-300:])
        # the plan's drives are the ones the walk-away erases (the window's drives_ok); the
        # sticks come from `go-back sticks`, as the window asks for them (the Aspire, 2026-10-28:
        # the manager read the plan's drives as sticks, and no stick could ever match)
        if not isinstance(drives, list) or not drives:
            return untouched("walkaway-no-drives", f"settle-in's plan names no drive to install Windows on: {drives}")
        # the manager's own mount of the kit stick (stick-left-in) is let go first: on one stick the
        # walk-away writes the kit stick itself, and settle-in refuses a stick in use (2026-11-15)
        sudo(ctx, "if mountpoint -q /mnt/upgtest; then umount /mnt/upgtest; fi", 60)
        st = sudo(ctx, f"{SETTLE_IN} go-back sticks --min-bytes {WALKAWAY_MIN_STICK}", 120)
        ctx.save("walkaway-sticks.json", st.out + st.err)
        try:
            sticks = json.loads(st.out.strip().splitlines()[-1])
        except (ValueError, IndexError):
            return untouched("walkaway-sticks-unread", (st.out + st.err).strip()[-300:])
        word = ctx.secrets["walkaway-stick"]
        picks = [d for d in sticks if isinstance(d, dict) and d.get("offered") and d.get("confirm_word") == word]
        if len(picks) != 1:
            return untouched("walkaway-stick-not-confirmed", f"{len(picks)} offered drives answer to the word typed at kickoff")
        d = picks[0]
        user = link.run("id -un", "linux", 30).out.strip()
        prep = sudo(ctx, f"{SETTLE_IN} go-back walkaway prepare --sentence '{ctx.secrets['walkaway']}' --account '{ctx.secrets['walkaway-account']}' "
                         f"--serial '{d['serial']}' --size {d['size_bytes']} --typed '{word}' --linux-user '{user}' --language '{language}'",
                    int(args.get("prepare_limit_s", 14400)))
        ctx.save("walkaway-prepare.out", prep.out + prep.err)
        if '"ready"' not in prep.out:
            return Outcome("walkaway-not-ready", False, (prep.out + prep.err).strip()[-300:])
        # what the firmware is about to be asked, written down before every walk-away restart: on
        # 2026-11-16 this restart left the Aspire before any system for 19 hours, with nothing kept to read
        cap = sudo(ctx, BEFORE_WALKAWAY_RESTART, 300)
        ctx.save("before-restart.txt", cap.out + cap.err)
        before = boot_mark(ctx, "linux")
        sudo(ctx, "systemd-run --on-active=5 systemctl reboot >/dev/null; echo REBOOTING")
        # Setup, the first sign-in, then Windows' SSH (installed from Windows Update by the walk-away's
        # task); the new account has the SSH login's own name (kickoff), so the same host reaches it
        got = wait_new_boot(ctx, "the fresh Windows after the walk-away", before, int(args.get("limit_s", 14400)), every_s=60)
        if got != "windows":
            return Outcome("hang-walkaway" if got is None else "walkaway-wrong-system", False, f"{got or 'nothing'} answered after the walk-away")
        found = link.run("$v = Get-Volume | Where-Object { $_.DriveLetter -and (Test-Path \"$($_.DriveLetter):\\upgrade_\\go-back.json\") } | Select-Object -First 1; "
                         "if ($v) { Write-Output \"WSTICK=$($v.DriveLetter):\" }", "windows", 60)
        m = re.search(r"WSTICK=(\w:)", found.out)
        if m:
            for n in ("go-back.json", "go-back-gate.json", "go-back-installed.txt", "go-back-wifi.log", "go-back-ssh.log"):
                link.pull(f"{m.group(1)}\\upgrade_\\{n}", os.path.join(ctx.stage_dir, "walkaway-stick", n), "windows")
        if not collect_windows(ctx, "direct", "after-walkaway"):
            return Outcome("collect-failed", False, "the Windows collector after the walk-away")
        return Outcome("windows-reached", True, "the walk-away gave a fresh Windows, reached over the carried SSH key"
                       + ("; one stick: the upgrade_ stick itself became the Windows installer" if d.get("upgrade_stick") else ""))

    def rig_standin(self, ctx):
        hv = os.path.join(REPO, "rig", "hyperv")
        logs = []
        for cmd in (["powershell.exe", "-NoProfile", "-Command", f"Stop-VM -Name '{ctx.link.vm}' -TurnOff -Force"], ["./v9.sh", "prepare"], ["./v9.sh", "windows"]):
            r = subprocess.run(cmd, cwd=hv, capture_output=True, text=True, timeout=3600, stdin=subprocess.DEVNULL)
            logs.append(f"$ {' '.join(cmd)}\n{r.stdout}{r.stderr}")
            if r.returncode != 0:
                ctx.save("rig-standin.log", "\n".join(logs))
                return Outcome("rig-standin-failed", False, f"{cmd[-1]} exit {r.returncode}")
        ctx.save("rig-standin.log", "\n".join(logs))
        if ctx.link.probe() != "windows":
            return Outcome("rig-standin-failed", False, "the fresh image's Windows did not answer")
        return Outcome("rig-standin-fresh-windows", True, "the rig cannot hold the walk-away (no USB stick with a serial): a fresh copy of the rig's Windows image stands in; not a walk-away row")


# read-only: the firmware's entries, BootNext and BootOrder (efibootmgr -v), every USB drive's partition
# table and first sector, the boot files on it (hashed from a read-only mount of the test's own), and
# that the restart that follows is a warm one (systemctl reboot, this boot's id and start time)
BEFORE_WALKAWAY_RESTART = (
    "echo \"== utc $(date -u +%FT%TZ)\"; echo '== efibootmgr -v'; efibootmgr -v; "
    "echo '== drives'; lsblk -o NAME,SIZE,TYPE,TRAN,FSTYPE,LABEL,PARTTYPE,PARTFLAGS,MODEL,SERIAL; "
    "for d in $(lsblk -dno NAME,TRAN | awk '$2==\"usb\"{print $1}'); do "
    "echo \"== sfdisk -d /dev/$d\"; sfdisk -d /dev/$d; echo \"== first sector of /dev/$d, sha256\"; head -c 512 /dev/$d | sha256sum; "
    "for p in $(lsblk -lno NAME,TYPE /dev/$d | awk '$2==\"part\"{print $1}'); do m=$(mktemp -d); "
    "if mount -o ro /dev/$p $m 2>/dev/null; then echo \"== /dev/$p: its root, and the files under EFI\"; ls -la $m; "
    "(cd $m && find . -maxdepth 4 -ipath './efi/*' -type f -exec sha256sum {} +); umount $m; else echo \"== /dev/$p: not mounted read-only\"; fi; rmdir $m; done; done; "
    "echo '== this boot (the restart that follows is warm: systemctl reboot)'; uptime -s; cat /proc/sys/kernel/random/boot_id; "
    "echo '== sleep locks'; systemd-inhibit --list --no-legend; echo CAPTURED")

SETTLE_IN = "/usr/local/libexec/upgrade_/settle-in"
# the window's own floor (settle-in/window/src/goback.rs refresh_sticks): Windows 11 takes about 7 GB, 16 GB sticks
WALKAWAY_MIN_STICK = 14_000_000_000


class Rollback:
    """Stage 4 (and the Aspire's stage 0, on the install it already has):
    settle-in's rollback from Linux, the way the person's click runs it, then
    one restart with no key and no BootNext. Windows must come up first, and
    the guard must stand down (R21, R22, V1b)."""
    writes = True
    needs = ("password",)

    def run(self, ctx, args):
        link = ctx.link
        if link.probe() != "linux":
            return untouched("wrong-system", "the rollback starts in Linux")
        dry = sudo(ctx, f"{SETTLE_IN} rollback --dry-run", 300)
        ctx.save("rollback-dry-run.json", dry.out + dry.err)
        if dry.rc != 0 or '"plan"' not in dry.out:
            return untouched("rollback-refused", (dry.out + dry.err).strip()[-300:])
        real = sudo(ctx, f"{SETTLE_IN} rollback", 600)
        ctx.save("rollback.json", real.out + real.err)
        if real.rc != 0 or '"done"' not in real.out:
            return Outcome("rollback-failed", False, (real.out + real.err).strip()[-300:])
        if not collect_linux(ctx, "after-rollback"):
            return Outcome("collect-failed", False, "the Linux collector after the rollback")
        before = boot_mark(ctx, "linux")
        sudo(ctx, "systemd-run --on-active=5 systemctl reboot >/dev/null; echo REBOOTING")
        got = wait_new_boot(ctx, "the first start after the rollback", before, 1800)
        if got is None:
            return Outcome("hang-after-rollback", False, "nothing answered after the restart")
        if got != "windows":
            return Outcome("windows-not-first", False, f"{got} answered first after the rollback, with no key pressed")
        if not collect_windows(ctx, "direct", "after-rollback"):
            return Outcome("collect-failed", False, "the Windows collector after the rollback")
        gone = ctx.wait_for("the guard to stand down", lambda: "ABSENT" in link.run(
            "if (Get-ScheduledTask -TaskName 'upgrade_ windows-slot guard' -ErrorAction SilentlyContinue) { 'PRESENT' } else { 'ABSENT' }",
            "windows", 60).out, 600, every_s=30)
        if not gone:
            return Outcome("guard-not-removed", False, "the guard task is still registered 10 minutes after Windows came back")
        return Outcome("rolled-back", True, "Windows started first with no key; the guard stood down")


# --- the file copy, and reclaim's refusals -----------------------------------
#
# The file copy (settle-in pull; V3, R8, R19, R20): known files are planted in
# Windows before the job is written (stage plant-files), so the product's own
# harvest counts them; after the install beside Windows, the pull runs through
# settle-in's own command and every planted file is hashed where it landed.
# Reclaim (R41, V15): only its check runs here. The check writes nothing and
# never arms; the move itself needs a USB stick the rig does not have and a
# healthy drive the Aspire does not have.

PLANT_DIR = "C:\\upgrade_oneclick\\plant"


def last_json(text):
    """The last whole JSON object a program printed (settle-in prints pretty
    JSON, after any progress lines)."""
    lines = text.splitlines()
    for i in range(len(lines) - 1, -1, -1):
        if lines[i].rstrip() == "{":
            try:
                return json.loads("\n".join(lines[i:]))
            except ValueError:
                continue
    for l in reversed(lines):
        if l.startswith("{") and l.rstrip().endswith("}"):
            try:
                return json.loads(l)
            except ValueError:
                continue
    return None


def stage_file(ctx, stage, name):
    """A file an earlier stage of this run kept (the latest such stage)."""
    import glob
    found = sorted(glob.glob(os.path.join(ctx.run_dir, "stages", f"*-{stage}", name)))
    return found[-1] if found else None


def csv_line(values):
    import csv
    import io
    buf = io.StringIO()
    csv.writer(buf, quoting=csv.QUOTE_MINIMAL, lineterminator="\n").writerow(values)
    return buf.getvalue()


def run_facts(ctx):
    """The machine's facts as promote reads them: the run's own, or a look
    stage's (the rig is off at the run's start: 2026-11-18, the first rows
    of the new stages named no machine)."""
    import rows
    try:
        return rows.find_facts(ctx.run_dir)[0]
    except (OSError, ValueError):
        return {}


STAGE_ROW_FILES = {"v3-pull.row.csv": ("v3-pull.csv", "pull"), "r41-reclaim.row.csv": ("r41-reclaim.csv", "reclaim")}


def stage_rows(run_dir, results_dir=None, out=print):
    """The lines the pull and reclaim-check stages wrote for their own CSVs
    (v3-pull.csv, r41-reclaim.csv), appended there once per run. A line that
    names no machine or firmware (manager 0.4.0 on the rig) is given the
    run's own facts, the ones promote uses; nothing else is touched."""
    import csv
    import io
    import rows
    results_dir = results_dir or os.path.join(REPO, "docs", "validation-results")
    marker = os.path.join(run_dir, ".stage-rows")
    if os.path.exists(marker):
        out(f"stage-rows: REFUSED - this run's stage rows were already written ({open(marker).read().strip()})")
        return 2
    facts = rows.find_facts(run_dir)[0]
    machine = " ".join(x for x in (facts.get("manufacturer"), facts.get("model")) if x)
    firmware = " ".join(x for x in (facts.get("bios_vendor"), facts.get("bios_version")) if x)
    said = []
    stages = os.path.join(run_dir, "stages")
    for name, (target, kind) in STAGE_ROW_FILES.items():
        lines = []
        for d in sorted(os.listdir(stages)):
            f = os.path.join(stages, d, name)
            if not os.path.isfile(f):
                continue
            for row in csv.reader(open(f, encoding="utf-8", newline="")):
                if kind == "pull":
                    row[2] = row[2] or firmware
                    if row[3].rstrip().endswith(","):
                        row[3] = row[3].rstrip() + " " + machine
                    row[-1] += f" | run {os.path.basename(os.path.normpath(run_dir))}/{d}"
                else:
                    row[2], row[3] = row[2] or machine, row[3] or firmware
                    row[-1] += f" | run {os.path.basename(os.path.normpath(run_dir))}/{d}"
                buf = io.StringIO()
                csv.writer(buf, quoting=csv.QUOTE_MINIMAL, lineterminator="\n").writerow(row)
                lines.append(buf.getvalue())
        if not lines:
            continue
        path = os.path.join(results_dir, target)
        with open(path, "a", encoding="utf-8", newline="") as f:
            f.write("".join(lines))
        with open(path, encoding="utf-8") as f:
            last = sum(1 for _ in f)
        said.append(f"{target} lines {last - len(lines) + 1}-{last}")
    if not said:
        out("stage-rows: no pull or reclaim-check stage wrote a line in this run")
        return 1
    with open(marker, "w", encoding="utf-8") as f:
        f.write("; ".join(said) + "\n")
    out("stage-rows: " + "; ".join(said))
    return 0


class PlantFiles:
    """Before the job is written: a known tree of files in the account's own
    Documents and Pictures (rig/hyperv/physical/plant-files.ps1), with what
    must arrive and what must not written down. Files in the test account's
    own folders only; an earlier plant is replaced, so it can run again."""
    writes = False
    needs = ()

    def run(self, ctx, args):
        link = ctx.link
        if link.probe() != "windows":
            return untouched("wrong-system", "the files are planted in Windows, before the job is written")
        link.run(f"New-Item -ItemType Directory -Force -Path '{PLANT_DIR}' | Out-Null", "windows", 60)
        ff = os.path.join(ctx.stage_dir, "firefox.jsonlz4")
        subprocess.run([sys.executable, os.path.join(REPO, "rig", "hyperv", "pull-firefox.py"), ff], capture_output=True, text=True)
        fx = ""
        if os.path.isfile(ff) and link.push(ff, PLANT_DIR + "\\firefox.jsonlz4", "windows").ok:
            fx = PLANT_DIR + "\\firefox.jsonlz4"
        r = link.run(windows_script("plant-files.ps1", "-Out", f"'{PLANT_DIR}'", "-Firefox", f"'{fx}'"), "windows", 900)
        ctx.save("plant.out", r.out + r.err)
        if "EXPECTED=" not in r.out:
            return untouched("plant-failed", (r.out + r.err).strip()[-300:])
        local = os.path.join(ctx.stage_dir, "expected.json")
        if not link.pull(PLANT_DIR + "\\expected.json", local, "windows").ok or not os.path.isfile(local):
            return untouched("plant-failed", "what was planted could not be read back")
        with open(local, encoding="utf-8-sig") as f:
            exp = json.load(f)
        marks = ", ".join(f"{k} {v}" for k, v in (exp.get("bookmarks") or {}).items()) or "none"
        return Outcome("files-planted", True, f"{len(exp['copy'])} files planted in the account's Documents and Pictures, "
                       f"{len(exp['not_copied'])} entries that must not be copied (links, Windows' own settings file); bookmarks planted: {marks}; "
                       "no OneDrive-only files (R43's residue stays owed)")


PULL_TOOLS = ("for t in ntfs-3g cryptsetup systemd-inhibit; do printf 'TOOL %s=%s\\n' $t \"$(command -v $t || echo MISSING)\"; done; "
              "echo \"NTFS3G=$( (rpm -q ntfs-3g 2>/dev/null | grep -v 'not installed') || dpkg-query -W -f '${Package} ${Version}' ntfs-3g 2>/dev/null || echo unknown)\"; "
              "echo \"CRYPTSETUP=$(cryptsetup --version 2>/dev/null | head -1)\"; echo \"KERNEL=$(uname -r)\"; . /etc/os-release; echo \"OS=$PRETTY_NAME\"")


class Pull:
    """After the install beside Windows: settle-in's own file copy, as the
    person's click runs it (check, then run), then every planted file hashed
    where it landed. A second run must find nothing to do."""
    writes = True
    needs = ("password",)

    def run(self, ctx, args):
        link = ctx.link
        if link.probe() != "linux":
            return untouched("wrong-system", "the file copy runs in the installed Linux")
        ep = stage_file(ctx, "plant-files", "expected.json")
        if not ep:
            return untouched("nothing-planted", "this run has no plant-files stage before the install, so there is nothing to hold the copy against")
        with open(ep, encoding="utf-8-sig") as f:
            exp = json.load(f)
        tools = sudo(ctx, PULL_TOOLS, 120)
        ctx.save("tools.txt", tools.out + tools.err)
        tv = kv_lines(tools.out)
        chk = sudo(ctx, f"{SETTLE_IN} pull check", int(args.get("check_limit_s", 900)))
        ctx.save("pull-check.out", chk.out + chk.err)
        c = last_json(chk.out) or {}
        if c.get("result") != "plan":
            self.row(ctx, tv, exp, c.get("result") or "unread", "not-reached", 0, "refused" if c.get("result") == "refused" else "fail",
                     "; ".join(c.get("why") or []) or (chk.out + chk.err).strip()[-200:])
            return untouched("pull-refused", "; ".join(c.get("why") or []) or (chk.out + chk.err).strip()[-300:])
        run = sudo(ctx, f"{SETTLE_IN} pull run", int(args.get("limit_s", 3600)))
        ctx.save("pull-run.out", run.out + run.err)
        r = last_json(run.out) or {}
        if r.get("result") != "done":
            why = "; ".join(r.get("why") or []) if isinstance(r.get("why"), list) else str(r.get("why") or "")
            self.row(ctx, tv, exp, "plan", r.get("result") or "unread", 0, "fail", why or (run.out + run.err).strip()[-200:])
            return Outcome("pull-" + (r.get("result") or "unread"), False, why or (run.out + run.err).strip()[-300:])
        tos = {f["name"]: f["to"] for f in r.get("folders", [])}
        look = "".join(f"echo '== {name}'; if [ -d {shq(to)}/pull-test ]; then cd {shq(to)} && find pull-test -type f -exec sha256sum {{}} + ; "
                       f"find pull-test -printf 'ENTRY %y %u %p\\n'; fi; " for name, to in sorted(tos.items()))
        docs_to = tos.get("Documents", "")
        look += (f"echo '== bookmarks'; ls -1 {shq(docs_to)} 2>/dev/null | grep '^Bookmarks from Windows' | sed 's/^/MARKS /'; "
                 "echo '== after'; echo \"MOUNTED=$(findmnt -n /run/upgrade_/pull-windows >/dev/null && echo yes || echo no)\"; "
                 "echo \"MAPPED=$([ -e /dev/mapper/upgrade_-pull-windows ] && echo yes || echo no)\"; "
                 "echo \"KEY_LEFT=$([ -e /var/lib/upgrade_/artifacts/credentials/bitlocker-C.txt ] && echo yes || echo no)\"; echo \"PERSON=$SUDO_USER\"")
        seen = sudo(ctx, look, 600)
        ctx.save("landed.txt", seen.out + seen.err)
        got, entries, owners, folder = {}, set(), set(), None
        for l in seen.out.splitlines():
            if l.startswith("== "):
                folder = l[3:].strip()
            elif l.startswith("ENTRY "):
                _, _kind, owner, path = l.split(" ", 3)
                entries.add((folder, path))
                owners.add(owner)
            elif re.match(r"^[0-9a-f]{64}  ", l):
                got[(folder, l[66:])] = l[:64]
        sv = kv_lines(seen.out)
        want = {(e["folder"], e["rel"]): e["sha256"] for e in exp["copy"]}
        missing = sorted(f"{k[0]}/{k[1]}" for k in want if k not in got)
        differ = sorted(f"{k[0]}/{k[1]}" for k in want if k in got and got[k] != want[k])
        extra = sorted(f"{k[0]}/{k[1]}" for k in got if k not in want)
        wrongly = sorted(n for n in exp.get("not_copied", []) if ("Documents", n) in entries)
        marks_seen = [l[6:] for l in seen.out.splitlines() if l.startswith("MARKS ")]
        marks_missing = sorted(b for b in (exp.get("bookmarks") or {}) if not any(b in m for m in marks_seen))
        strangers = sorted(o for o in owners if o != sv.get("PERSON"))
        again = sudo(ctx, f"{SETTLE_IN} pull run", 900)
        ctx.save("pull-again.out", again.out + again.err)
        a = last_json(again.out) or {}
        faults = []
        if missing:
            faults.append(f"{len(missing)} planted file(s) did not arrive (the first: {missing[0]})")
        if differ:
            faults.append(f"{len(differ)} arrived with other bytes (the first: {differ[0]})")
        if extra:
            faults.append(f"{len(extra)} file(s) arrived that were not planted to be copied (the first: {extra[0]})")
        if wrongly:
            faults.append("copied though it must not be: " + ", ".join(wrongly))
        if marks_missing:
            faults.append("no bookmarks file for " + ", ".join(marks_missing))
        if strangers:
            faults.append("copied files belong to " + ", ".join(strangers) + ", not the person")
        if sv.get("MOUNTED") != "no" or sv.get("MAPPED") != "no":
            faults.append("Windows' drive was left open after the copy")
        if a.get("result") != "nothing-to-do":
            faults.append(f"a second run said {a.get('result') or 'nothing readable'}, not nothing-to-do")
        n_ok = len(want) - len(missing) - len(differ)
        marks = " ".join(f"{b}={n}" for b, n in (exp.get("bookmarks") or {}).items())
        self.row(ctx, tv, exp, "plan", "done", n_ok, "fail" if faults else ("pass" if link.kind == "ssh" else "pass-plumbing"),
                 "; ".join(faults) or "no OneDrive-only files planted (R43's residue stays owed); a second run found nothing to do; the one click test manager's stage",
                 key_deleted="yes" if sv.get("KEY_LEFT") == "no" else "no", marks=marks)
        if faults:
            return Outcome("files-differ", False, "; ".join(faults))
        return Outcome("files-brought", True, f"settle-in's own copy brought all {len(want)} planted files into the person's folders, each identical (sha256); "
                       f"the {len(exp.get('not_copied', []))} links and settings files were not copied; bookmarks written for {marks or 'none'}; "
                       f"Windows' drive closed again; a second run found nothing to do; {tv.get('OS', '')}, {tv.get('NTFS3G', '')}; no OneDrive-only files were planted")

    def row(self, ctx, tv, exp, check, run, n_ok, result, notes, key_deleted="", marks=""):
        """This stage's own line for v3-pull.csv, kept in the stage folder."""
        facts = run_facts(ctx)
        machine = " ".join(x for x in (facts.get("manufacturer"), facts.get("model")) if x)
        firmware = " ".join(x for x in (facts.get("bios_vendor"), facts.get("bios_version")) if x)
        with open(os.path.join(REPO, "settle-in", "Cargo.toml"), encoding="utf-8") as f:
            sv = re.search(r'^version = "([^"]+)"', f.read(), re.M).group(1)
        ctx.save("v3-pull.row.csv", csv_line([
            utc()[:10], f"oneclick {VERSION} pull", firmware, f"installed {tv.get('OS', 'Linux')} (alongside), {machine}",
            f"settle-in {sv}", exp.get("windows_build", ""), tv.get("KERNEL", ""), tv.get("CRYPTSETUP", "").replace("cryptsetup ", "").split(" ")[0],
            tv.get("NTFS3G", ""), "0", len(exp["copy"]), n_ok, 0, 0, "none planted", marks, key_deleted, check, run, result, notes]))


def shq(text):
    """One shell word."""
    return "'" + str(text).replace("'", "'\"'\"'") + "'"


RECLAIM_STATE = ("echo \"BOOT=$(efibootmgr 2>/dev/null | grep -E '^(BootNext|BootOrder|Boot[0-9A-F]{4})' | sha256sum | cut -c1-64)\"; "
                 "D=/dev/$(lsblk -no PKNAME \"$(findmnt -no SOURCE /)\" | head -1); echo \"DISK=$D\"; "
                 "echo \"TABLE=$(sfdisk -d $D 2>/dev/null | sha256sum | cut -c1-64)\"; echo \"DISK_BYTES=$(blockdev --getsize64 $D)\"; "
                 "echo \"RECORDS=$(cat /var/lib/upgrade_/job.json /var/lib/upgrade_/outcome.json | sha256sum | cut -c1-64)\"; "
                 "echo \"WINMOUNT=$(findmnt -n /run/upgrade_/reclaim-windows >/dev/null && echo yes || echo no)\"")

# The planted faults (VALIDATION V15), each made by changing what the fault is about and
# nothing else, on the RIG only, and put back after the check. arm -> (what is planted,
# words the refusal must carry).
RECLAIM_PLANTS = {
    "wrong-guid": ("outcome", "d['windows']['partition']['guid'] = '0fc63daf-0000-4000-8000-00000000beef'", "is not on the disk Linux runs from"),
    "drive-health": ("job", "d['storage']['physical_disk']['health_status'] = 'Warning'", "did not report itself healthy"),
    "bitlocker-locked": ("job", "d['harvest']['bitlocker']['status'] = 'on'", "locked with BitLocker"),
    "other-profiles": ("windows", None, "other people's accounts are on Windows"),
}


class ReclaimCheck:
    """Reclaim's own check (settle-in reclaim check; R41, V15): what it would
    delete, or why it refuses. It writes nothing and arms nothing, and this
    stage holds it to that: the firmware's entries, the partition table and
    the install's records are the same after as before. With "arm" in the
    plan, one fault is planted first (the rig only) and put back after."""
    writes = False
    needs = ("password",)

    def run(self, ctx, args):
        link = ctx.link
        if link.probe() != "linux":
            return untouched("wrong-system", "reclaim is checked from the installed Linux")
        arm = args.get("arm", "as-found")
        want = args.get("expect", "refused")
        must = list(args.get("expect_why", []))
        if arm != "as-found":
            if arm not in RECLAIM_PLANTS:
                return untouched("manager-error", f"no planted fault is named {arm}")
            if link.kind != "hyperv":
                return untouched("plant-refused", "a fault is planted on the rig only, never on a real machine")
            must.append(RECLAIM_PLANTS[arm][2])
        before = kv_lines(sudo(ctx, RECLAIM_STATE, 120).out)
        planted = self.plant(ctx, arm) if arm != "as-found" else None
        if planted is False:
            self.unplant(ctx, arm)
            return untouched("plant-failed", f"the {arm} fault could not be planted: read plant.out")
        try:
            chk = sudo(ctx, f"{SETTLE_IN} reclaim check", int(args.get("limit_s", 900)))
        finally:
            if arm != "as-found":
                put_back = self.unplant(ctx, arm)
        ctx.save("reclaim-check.out", chk.out + chk.err)
        c = last_json(chk.out) or {}
        after = kv_lines(sudo(ctx, RECLAIM_STATE, 120).out)
        ctx.save("state.txt", "before " + json.dumps(before, sort_keys=True) + "\nafter  " + json.dumps(after, sort_keys=True) + "\n")
        why = [str(w) for w in (c.get("why") or [])]
        result = c.get("result") or "unread"
        faults = []
        for k in ("BOOT", "TABLE", "RECORDS"):
            if not before.get(k) or before.get(k) != after.get(k):
                faults.append({"BOOT": "the firmware's start entries", "TABLE": "the partition table", "RECORDS": "the install's records"}[k] + " changed across the check")
        if after.get("WINMOUNT") != "no":
            faults.append("Windows' drive was left mounted after the check")
        if arm != "as-found" and not put_back:
            faults.append("the planted fault could not be put back: read plant.out")
        if result != want:
            faults.append(f"the check said {result}, the plan expects {want}")
        absent = [m for m in must if not any(m in w for w in why)]
        if result == "refused" and absent:
            faults.append("the refusal does not carry: " + " / ".join(absent))
        if arm != "as-found":
            self.row(ctx, arm, before, c, result, faults)
        words = "; ".join(why)[:400] if why else ""
        if faults:
            return Outcome("reclaim-check-wrong", False, "; ".join(faults) + (f" | it said: {words}" if words else ""))
        if result == "refused":
            return Outcome("reclaim-refused", True, f"{'planted fault ' + arm if arm != 'as-found' else 'the computer as found'}: reclaim's check refused "
                           f"({len(why)} reason(s): {words}); nothing was armed, and the firmware's entries, the partition table and the install's records are unchanged")
        return Outcome("reclaim-plan-made", True, f"the computer as found: reclaim's check gave a plan (it would delete {c.get('deleted_bytes', '?')} bytes of Windows "
                       f"and move Linux {((c.get('move') or {}).get('shift_sectors', '?'))} sectors); nothing was armed or changed. The move itself is not run by this stage")

    def plant(self, ctx, arm):
        where, edit, _ = RECLAIM_PLANTS[arm]
        if where == "windows":
            script = ("set -e; G=$(python3 -c \"import json; print(json.load(open('/var/lib/upgrade_/outcome.json'))['windows']['partition']['guid'])\"); "
                      "mkdir -p /run/upgrade_-plant; mount -t ntfs-3g /dev/disk/by-partuuid/$G /run/upgrade_-plant; "
                      "mkdir '/run/upgrade_-plant/Users/someone-else'; echo planted > '/run/upgrade_-plant/Users/someone-else/a-file.txt'; "
                      "umount /run/upgrade_-plant; echo PLANTED=yes")
        else:
            f = f"/var/lib/upgrade_/{where}.json"
            script = (f"set -e; cp -p {f} {f}.upgrade_-plant-keep; python3 - <<'PY'\nimport json\nd = json.load(open('{f}'))\n{edit}\n"
                      f"json.dump(d, open('{f}', 'w'), indent=1)\nPY\necho PLANTED=yes")
        r = sudo(ctx, script, 300)
        ctx.save("plant.out", r.out + r.err)
        return "PLANTED=yes" in r.out

    def unplant(self, ctx, arm):
        where = RECLAIM_PLANTS[arm][0]
        if where == "windows":
            script = ("G=$(python3 -c \"import json; print(json.load(open('/var/lib/upgrade_/outcome.json'))['windows']['partition']['guid'])\"); "
                      "mkdir -p /run/upgrade_-plant; mountpoint -q /run/upgrade_-plant || mount -t ntfs-3g /dev/disk/by-partuuid/$G /run/upgrade_-plant; "
                      "rm -rf '/run/upgrade_-plant/Users/someone-else'; [ ! -e '/run/upgrade_-plant/Users/someone-else' ] && G1=gone; "
                      "umount /run/upgrade_-plant && rmdir /run/upgrade_-plant; echo \"PUT_BACK=${G1:-no}\"")
        else:
            f = f"/var/lib/upgrade_/{where}.json"
            script = f"if [ -e {f}.upgrade_-plant-keep ]; then mv -f {f}.upgrade_-plant-keep {f} && echo PUT_BACK=gone; else echo PUT_BACK=gone; fi"
        r = sudo(ctx, script, 300)
        with open(os.path.join(ctx.stage_dir, "plant.out"), "a", encoding="utf-8") as f:
            f.write("\n--- put back\n" + r.out + r.err)
        return "PUT_BACK=gone" in r.out

    def row(self, ctx, arm, before, c, result, faults):
        """This stage's own line for r41-reclaim.csv, kept in the stage folder."""
        facts = run_facts(ctx)
        machine = " ".join(x for x in (facts.get("manufacturer"), facts.get("model")) if x)
        firmware = " ".join(x for x in (facts.get("bios_vendor"), facts.get("bios_version")) if x)
        with open(os.path.join(REPO, "settle-in", "Cargo.toml"), encoding="utf-8") as f:
            sv = re.search(r'^version = "([^"]+)"', f.read(), re.M).group(1)
        ok = result == "refused" and not faults
        notes = ("settle-in reclaim check, the fault planted and put back by the one click test manager; nothing armed, firmware entries, table and records unchanged; "
                 + "; ".join(str(w) for w in (c.get("why") or []))) if ok else "; ".join(faults)
        ctx.save("r41-reclaim.row.csv", csv_line([
            utc(), f"oneclick {VERSION} reclaim-check", machine, firmware, sv, arm, before.get("DISK_BYTES", ""), "", str(64 << 20),
            result if result == "refused" else "not-started", ("pass" if ctx.link.kind == "ssh" else "pass-plumbing") if ok else "fail", notes[:600]]))


# --- one stick, no swaps -----------------------------------------------------
#
# Decided (2026-11-15, the owner): one stick on the way back too, and a test
# with no stick swaps. The kit used to be written on the manager's own
# computer and the stick carried over; a second stick took Windows' installer.
# Now the stick never leaves the machine: the product's stick writer
# (upgrade-stick.exe, R16) runs on the machine itself, from a copy of the kit
# pushed there, with the word it asks for typed at kickoff.

KIT_DIR = "C:\\upgrade_oneclick\\kit"
KIT_TAR = "C:\\upgrade_oneclick\\kit.tar"
KIT_CSV = "C:\\upgrade_oneclick\\r16-stick-writer.csv"
ONE_USB = ("$ProgressPreference = 'SilentlyContinue'; $u = @(Get-Disk | Where-Object BusType -eq 'USB'); "
           "if ($u.Count -ne 1) { Write-Output \"USB_DISKS=$($u.Count)\"; exit 3 }; $d = $u[0]; ")
# the word the stick writer asks for: the first volume's label, else the model name (evaluate/stick judge.rs confirm_word)
USB_FACTS = (ONE_USB + "$lab = @(Get-Partition -DiskNumber $d.Number -ErrorAction SilentlyContinue | Get-Volume -ErrorAction SilentlyContinue | "
             "Where-Object FileSystemLabel | ForEach-Object FileSystemLabel) | Select-Object -First 1; "
             "Write-Output \"TARGET=$($d.UniqueId)\"; Write-Output \"BYTES=$($d.Size)\"; Write-Output \"MODEL=$($d.FriendlyName.Trim())\"; Write-Output \"LABEL=$lab\"")
WALKAWAY_LABEL_SOURCE = ("settle-in/src/stickwrite.rs", r'const LABEL: &str = "([^"]+)"')


def walkaway_label(repo=REPO):
    """The label settle-in gives the stick it makes Windows' installer (read
    from the product's source: one home per fact)."""
    path, rx = WALKAWAY_LABEL_SOURCE
    with open(os.path.join(repo, path), encoding="utf-8") as f:
        m = re.search(rx, f.read())
    if not m:
        raise SystemExit(f"oneclick: the walk-away stick's label is not where it was ({path}); refusing to guess it")
    return m.group(1)


def kv_lines(text):
    return dict(re.findall(r"^(\w+)=(.*?)\s*$", clean(text), re.M))


def file_sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def psq(text):
    """A PowerShell single-quoted string."""
    return "'" + text.replace("'", "''") + "'"


class KitStick:
    """The kit onto the stick, on the machine itself: dist/kit is pushed over
    as one file (its hash checked on the machine before it is unpacked), then
    the product's stick writer erases the one USB stick and writes the kit,
    reading every file back. With the plan's "fetch", the Linux is then
    fetched onto it by the same call the window makes (upgrade-fetch get).

    Every kit-stick stage of a run must write the same kit (the journal's
    'kit' line holds its SHA256SUMS' hash): one kit per run.

    Not counted as a writer of the machine's drives: it erases the stick and
    nothing else, and an interrupted write is simply done again. What stops
    it from touching a drive is the stick writer's own refusals (R16), the
    size the owner saw at kickoff, and the word typed there.

    The rig cannot hold it (the writer rightly refuses Hyper-V's SCSI
    "stick"): there the rig's own stick builder stands in, named so."""
    writes = False
    needs = ()   # the word: stage_needs (which one depends on where the stage sits)

    def run(self, ctx, args):
        link = ctx.link
        kit = os.path.join(REPO, "dist", "kit")
        sums, writer = os.path.join(kit, "stick", "SHA256SUMS"), os.path.join(kit, "upgrade-stick.exe")
        if not (os.path.exists(sums) and (link.kind == "hyperv" or os.path.exists(writer))):
            return untouched("kit-not-built", f"no kit at {kit} (./make-kit.sh)")
        kit_id = file_sha256(sums)
        first = [e for e in ctx.journal.entries() if e["event"] == "kit"]
        if first and first[0].get("sums_sha256") != kit_id:
            return untouched("kit-changed", f"this run's kit was {first[0].get('sums_sha256', '')[:12]}; dist/kit is now {kit_id[:12]}: one kit per run")
        manifest = os.path.join(kit, "stick", "KIT-MANIFEST.txt")
        if os.path.exists(manifest):
            shutil.copy(manifest, os.path.join(ctx.stage_dir, "KIT-MANIFEST.txt"))
        if not first:
            ctx.journal.write("kit", sums_sha256=kit_id)
        if link.kind == "hyperv":
            return self.rig_standin(ctx, kit_id)
        if link.probe() != "windows":
            return untouched("wrong-system", "the stick writer runs on Windows")
        word = ctx.secrets["kit-stick-after-walkaway" if args.get("after_walkaway") else "kit-stick"]
        # the kit, as one file
        tar = os.path.join(ctx.stage_dir, "kit.tar")
        t = subprocess.run(["tar", "-C", kit, "-cf", tar, "stick", "upgrade-stick.exe"], capture_output=True, text=True)
        if t.returncode != 0:
            return untouched("manager-error", f"tar: {t.stderr.strip()[:200]}")
        tar_id, tar_bytes = file_sha256(tar), os.path.getsize(tar)
        link.run(f"Remove-Item {KIT_DIR}, {KIT_TAR}, {KIT_CSV} -Recurse -Force -ErrorAction SilentlyContinue; New-Item -ItemType Directory -Force {KIT_DIR} | Out-Null",
                 "windows", 300)
        p = link.push(tar, KIT_TAR, "windows", timeout=int(args.get("push_limit_s", 7200)))
        os.remove(tar)
        if not p.ok:
            return untouched("push-failed", f"the kit did not reach the machine: {p.err.strip()[:200]}")
        un = link.run(f"$h = (Get-FileHash {KIT_TAR} -Algorithm SHA256).Hash.ToLower(); Write-Output \"TAR=$h\"; "
                      f"if ($h -eq '{tar_id}') {{ tar.exe -xf {KIT_TAR} -C {KIT_DIR}; Write-Output \"UNPACKED=$LASTEXITCODE\" }}; "
                      f"Remove-Item {KIT_TAR} -Force", "windows", 1800)
        ctx.save("unpack.out", clean(un.out + un.err))
        kv = kv_lines(un.out)
        if kv.get("TAR") != tar_id:
            return untouched("kit-copy-differs", f"the kit's copy on the machine hashes {kv.get('TAR', '?')[:12]}, not {tar_id[:12]}; nothing was unpacked")
        if kv.get("UNPACKED") != "0":
            return untouched("kit-unpack-failed", clean(un.out + un.err).strip()[-200:])
        # the one USB stick, and the stick writer's own list of it
        facts = link.run(USB_FACTS, "windows", 120)
        lst = link.run(f"& {KIT_DIR}\\upgrade-stick.exe list", "windows", 300)
        ctx.save("stick-before.txt", clean(facts.out + facts.err + lst.out + lst.err))
        kv = kv_lines(facts.out)
        if "USB_DISKS" in kv or not kv.get("TARGET"):
            return untouched("stick-not-found", f"{kv.get('USB_DISKS', '?')} USB disks on the machine (want exactly 1)")
        want_bytes = ctx.secrets.get("kit-stick-bytes")
        if want_bytes and str(want_bytes) != kv.get("BYTES"):
            return untouched("stick-not-the-approved-one", f"the USB disk is {kv.get('BYTES')} bytes; the one approved at kickoff was {want_bytes}")
        if f"id: {kv['TARGET']}" not in clean(lst.out):
            return untouched("stick-not-listed", "the stick writer's own list does not show the USB disk by that id")
        w = link.run(f"& {KIT_DIR}\\upgrade-stick.exe write --target {psq(kv['TARGET'])} --expected-bytes {kv['BYTES']} --source {KIT_DIR}\\stick "
                     f"--boot-size-gb {int(args.get('boot_size_gb', 8))} --confirmed-label {psq(word)} --results-csv {KIT_CSV}; "
                     "Write-Output \"WRITER_RC=$LASTEXITCODE\"", "windows", int(args.get("write_limit_s", 7200)))
        ctx.save("stick-writer.out", clean(w.out + w.err))
        link.pull(KIT_CSV, os.path.join(ctx.stage_dir, "r16-stick-writer.csv"), "windows")
        rc = kv_lines(w.out).get("WRITER_RC")
        if rc == "2":
            return untouched("stick-writer-refused", clean(w.out + w.err).strip()[-300:])
        if rc != "0":
            return Outcome("stick-not-verified", False, clean(w.out + w.err).strip()[-300:])
        L = find_stick(ctx)
        if not L:
            return Outcome("stick-not-found-after-write", False, "no single volume labelled UPGV0 after the write")
        link.pull(f"{L}\\stick.json", os.path.join(ctx.stage_dir, "stick.json"), "windows")
        files = (re.findall(r"written and verified: (\d+) files", w.out) or ["?"])[-1]
        notes = (f"upgrade-stick wrote the kit (SHA256SUMS {kit_id[:12]}) onto the {kv.get('MODEL', 'stick')} in the machine itself, {files} files read back; "
                 f"the kit travelled as one file of {tar_bytes} bytes, its hash checked on the machine")
        if args.get("after_walkaway"):
            notes += "; the stick was Windows' installer from the walk-away: one stick, no swap"
        if not args.get("fetch"):
            return Outcome("kit-stick-written", True, notes)
        lx = ctx.plan.get("linux") or {}
        g = link.run(f"& {L}\\upgrade-fetch.exe get --release {lx.get('release', '')} --desktop {lx.get('desktop', '')}" +
                     (f" --from {args['fetch_from']}" if args.get("fetch_from") else "") + "; Write-Output \"FETCH_RC=$LASTEXITCODE\"",
                     "windows", int(args.get("fetch_limit_s", 14400)))
        ctx.save("fetch.out", clean(g.out + g.err))
        ok, said = fetch_status(ctx)
        ctx.save("fetch-status.out", said)
        if kv_lines(g.out).get("FETCH_RC") != "0" or not ok:
            return Outcome("linux-not-fetched", False, clean(g.out + g.err).strip()[-300:])
        return Outcome("kit-stick-written", True, notes + f"; the manager fetched {lx.get('release')} ({lx.get('desktop')}) onto it with the window's own call"
                       + (" from a local copy (--from)" if args.get("fetch_from") else ""))

    def rig_standin(self, ctx, kit_id):
        hv = os.path.join(REPO, "rig", "hyperv")
        logs = []
        for cmd in (["powershell.exe", "-NoProfile", "-Command",
                     f"if ((Get-VM -Name '{ctx.link.vm}').State -ne 'Off') {{ Stop-VM -Name '{ctx.link.vm}' -Force }}; (Get-VM -Name '{ctx.link.vm}').State"],
                    ["./v9.sh", "stick"], ["./v9.sh", "windows"]):
            r = subprocess.run(cmd, cwd=hv, capture_output=True, text=True, timeout=3600, stdin=subprocess.DEVNULL)
            logs.append(f"$ {' '.join(cmd)}\n{r.stdout}{r.stderr}")
            if r.returncode != 0:
                ctx.save("rig-standin.log", "\n".join(logs))
                return Outcome("rig-standin-failed", False, f"{cmd[-1]} exit {r.returncode}")
        ctx.save("rig-standin.log", "\n".join(logs))
        if ctx.link.probe() != "windows":
            return Outcome("rig-standin-failed", False, "Windows did not answer after the rig's stick was built again")
        return Outcome("rig-standin-kit-stick", True, f"the rig cannot hold the stick writer (it refuses Hyper-V's SCSI stick, R16): the rig's own "
                       f"stick builder made the stick again from the kit (SHA256SUMS {kit_id[:12]}); not a stick writer row")


STAGES = {"look": Look(), "keep-windows": KeepWindows(), "rollback": Rollback(), "rollback-existing": Rollback(),
          "stick-left-in": StickLeftIn(), "clean-slate": CleanSlate(), "stick-left-in-erased": StickLeftIn(),
          "walk-away": WalkAway(), "refused-before-countdown": RefusedBeforeCountdown(), "kit-stick": KitStick(),
          "plant-files": PlantFiles(), "pull": Pull(), "reclaim-check": ReclaimCheck()}


def stage_impl(name):
    return STAGES.get(name) or NotBuilt(name)


def stage_needs(st):
    """The approvals a plan stage needs: its own, and the sentences the plan
    names for this machine (the Aspire's scan is RED, so its job writer and
    prologue need R23's sentence; it is never added in silence). A plan may
    name only a sentence the product itself asks for."""
    extra = st.get("needs", [])
    unknown = [n for n in extra if n not in SENTENCE_SOURCES]
    if unknown:
        raise SystemExit(f"oneclick: stage '{st['stage']}' names approvals the manager does not know: {', '.join(unknown)}")
    own = tuple(stage_impl(st["stage"]).needs)
    if st["stage"] == "kit-stick":
        # the stick writer's word differs with what the stick is then: what it shows at kickoff,
        # or the label the walk-away leaves on it
        own += ("kit-stick-after-walkaway" if st.get("after_walkaway") else "kit-stick",)
    return own + tuple(n for n in extra if n not in own)


def plan_needs(stages):
    return sorted({n for st in stages for n in stage_needs(st)})


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


def record_facts(run_dir, link, journal):
    """What the machine says it is, once, at the run's start: every row names
    its machine and firmware from this, never from a typed name."""
    who = link.probe()
    if who == "windows":
        r = link.run("$b = Get-CimInstance Win32_BIOS; $c = Get-CimInstance Win32_ComputerSystem; $o = Get-CimInstance Win32_OperatingSystem; "
                     "[ordered]@{ manufacturer = $c.Manufacturer; model = $c.Model; bios_vendor = $b.Manufacturer; bios_version = $b.SMBIOSBIOSVersion; "
                     "os_caption = $o.Caption; os_build = $o.BuildNumber; read_from = 'windows' } | ConvertTo-Json", "windows", 120)
    elif who == "linux":
        r = link.run("python3 -c 'import json,os,platform; d=\"/sys/class/dmi/id/\"; g=lambda n: open(d+n).read().strip() if os.path.exists(d+n) else \"\"; "
                     "print(json.dumps(dict(manufacturer=g(\"sys_vendor\"), model=g(\"product_name\"), bios_vendor=g(\"bios_vendor\"), "
                     "bios_version=g(\"bios_version\"), os_caption=platform.platform(), os_build=\"\", read_from=\"linux\")))'", "linux", 120)
    else:
        journal.write("note", text="the machine's facts were not read at the start: nothing answered")
        return
    try:
        facts = json.loads(r.out)
    except ValueError:
        journal.write("note", text="the machine's facts did not read as JSON", out=r.out[-200:])
        return
    with open(os.path.join(run_dir, "facts.json"), "w", encoding="utf-8") as f:
        json.dump(facts, f, indent=2)
    journal.write("facts", firmware=f"{facts.get('bios_vendor', '')} {facts.get('bios_version', '')}".strip(), read_from=facts.get("read_from"))


class KeepAwake:
    """The manager's own computer stays awake for the run (the G16 went into
    Modern Standby 16:16Z-20:31Z on 2026-10-27, freezing WSL and the run).
    A Windows helper holds ES_SYSTEM_REQUIRED while a lock file in the run
    folder is fresh; a thread here refreshes it every minute, so a manager
    that dies lets the computer sleep again within 10 minutes. It stops idle
    sleep, not a closed lid or the power button."""

    def __init__(self, run_dir):
        self.lock = os.path.join(run_dir, ".awake")
        self.proc, self.thread, self.stop = None, None, None

    def __enter__(self):
        import threading
        if not shutil.which("powershell.exe"):
            return self
        with open(self.lock, "w") as f:
            f.write(utc())
        win = subprocess.run(["wslpath", "-w", self.lock], capture_output=True, text=True).stdout.strip()
        script = ("Add-Type -Name P -Namespace W -MemberDefinition '[DllImport(\"kernel32.dll\")] public static extern uint SetThreadExecutionState(uint f);'; "
                  "[W.P]::SetThreadExecutionState([uint32]2147483649) | Out-Null; "
                  f"while ((Test-Path '{win}') -and (((Get-Date) - (Get-Item '{win}').LastWriteTime).TotalMinutes -lt 10)) {{ Start-Sleep 30 }}")
        self.proc = subprocess.Popen(["powershell.exe", "-NoProfile", "-NonInteractive", "-EncodedCommand", tr.encoded(script)],
                                     stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        self.stop = threading.Event()

        def touch():
            while not self.stop.wait(60):
                try:
                    os.utime(self.lock)
                except OSError:
                    return
        self.thread = threading.Thread(target=touch, daemon=True)
        self.thread.start()
        return self

    def __exit__(self, *exc):
        if self.stop:
            self.stop.set()
        try:
            os.remove(self.lock)
        except OSError:
            pass
        return False


AWAKE_UNIT = "upgrade-oneclick-awake"


def hold_awake(ctx, quiet=False):
    """The plan's hold_awake: the machine's Linux kept from sleeping for this
    stage, by the TEST, never by the product. The walk-away's prepare runs
    an hour and settle-in holds no sleep lock: on 2026-10-28 KDE put the
    Aspire to sleep 45 minutes in and the manager lost it (the same finding
    as 2026-10-04, owed in settle-in). The owner chose (2026-10-28) to hold
    the lock from the test and say so in the journal and the row. A
    transient unit outlives the SSH call; the next restart ends it."""
    if ctx.link.probe() != "linux":
        if not quiet:
            ctx.note("hold_awake: Linux does not answer; no sleep lock was taken")
        return False
    r = sudo(ctx, f"systemctl stop {AWAKE_UNIT} 2>/dev/null; systemd-run --unit={AWAKE_UNIT} systemd-inhibit --what=sleep:idle --mode=block "
                  "--who=upgrade_oneclick --why='one click test: the stage runs long; settle-in holds no sleep lock' sleep 21600 >/dev/null && "
                  "sleep 1 && systemd-inhibit --list --no-legend | grep -q upgrade_oneclick && echo HELD", 60)
    held = "HELD" in r.out
    ctx.note("the TEST holds a sleep lock on Linux for this stage" if held else "hold_awake: the sleep lock was not taken", out=(r.out + r.err)[-200:])
    return held


def release_awake(ctx):
    if ctx.link.probe() == "linux":
        sudo(ctx, f"systemctl stop {AWAKE_UNIT} 2>/dev/null; echo RELEASED", 60)


WINDOWS_AWAKE = ("$q = powercfg /query SCHEME_CURRENT SUB_SLEEP STANDBYIDLE | Select-String 'Current AC Power Setting Index'; "
                 "$was = -1; if ($q) { try { $was = [Convert]::ToInt32((\"$q\" -replace '.*0x', '').Trim(), 16) } catch { } }; "
                 "powercfg /change standby-timeout-ac 0; powercfg /change hibernate-timeout-ac 0; Write-Output \"AWAKE_WAS=$was\"")


def windows_awake(ctx):
    """The plan's test_windows_awake: idle sleep on mains power switched off in
    the machine's Windows, by the TEST, never by the product. Each of the
    product's programs keeps the computer awake while it works (R33), and a
    person at the keyboard covers the gaps between them. The test has no
    person: on 2026-11-18 the Aspire had sat idle past its sleep timer when
    the fetch ended, slept at that moment, and the manager's next call got
    no answer. Said in the journal and in every row it was on for; put back
    when the run finishes. True when Windows answered and took it."""
    if ctx.link.probe() != "windows":
        return False
    r = ctx.link.run(WINDOWS_AWAKE, "windows", 120)
    was = kv_lines(r.out).get("AWAKE_WAS")
    if was is None:
        ctx.note("test_windows_awake: Windows did not take the setting", out=(r.out + r.err)[-200:])
        return False
    if was not in ("0", "-1"):
        ctx.journal.write("windows-awake", was_s=int(was))
    return True


def windows_awake_off(link, journal, out=print):
    """At the run's end: the idle sleep time the test found, put back (when
    Windows answers and a time was recorded for it)."""
    was = [e["was_s"] for e in journal.entries() if e["event"] == "windows-awake"]
    if not was or link.probe() != "windows":
        return
    r = link.run(f"powercfg /change standby-timeout-ac {max(1, was[-1] // 60)}; Write-Output AWAKE_BACK", "windows", 120)
    journal.write("windows-awake-off", minutes=max(1, was[-1] // 60), ok="AWAKE_BACK" in r.out)
    out(f"oneclick: Windows' idle sleep on mains power is back at {max(1, was[-1] // 60)} minutes" if "AWAKE_BACK" in r.out
        else "oneclick: Windows' idle sleep could not be put back (the test left it off)")


def run_chain(run_dir, link=None, sleep=time.sleep, clock=time.monotonic, out=print):
    if link is not None:   # a test's made-up machine: no helper on the real computer
        return _run_chain(run_dir, link, sleep, clock, out)
    with KeepAwake(run_dir):
        return _run_chain(run_dir, link, sleep, clock, out)


def _run_chain(run_dir, link=None, sleep=time.sleep, clock=time.monotonic, out=print):
    run, secrets = load(run_dir)
    plan = run["plan"]
    link = link or tr.from_spec(run["transport"], run_dir)
    journal = Journal(os.path.join(run_dir, "journal.jsonl"))
    if hasattr(link, "on_event"):   # Follow: a new address or a newly trusted key goes in the journal
        link.on_event = lambda event, **kv: journal.write(event, **kv)
    ctx = Context(run_dir, plan, secrets, link, journal, sleep, clock)
    if not journal.entries():
        journal.write("run-start", manager=VERSION, transport=link.describe(), machine=plan.get("machine", ""))
        record_facts(run_dir, link, journal)
    while True:
        action, arg = next_action(plan, journal.entries())
        if action == "finished":
            journal.write("run-finished")
            out("oneclick: every stage ran and ended as the plan expects")
            autologon_off(link, journal, out)
            windows_awake_off(link, journal, out)
            return 0
        if action == "wait":
            st = plan["stages"][arg]
            if not any(e["event"] == "owner-wait" and e.get("index") == arg for e in journal.entries()):
                journal.write("owner-wait", index=arg, stage=st["stage"], step=st["owner_step"])
            if st.get("owner_check") not in OWNER_CHECKS:
                out(f"oneclick: waiting for the owner before stage {arg + 1} {st['stage']}:\n  {st['owner_step']}\n"
                    f"then: oneclick.py done {run_dir} && oneclick.py run {run_dir}")
                return 3
            out(f"oneclick: waiting for you before stage {arg + 1} {st['stage']}:\n  {st['owner_step']}\n"
                "  (no need to say so: the manager sees it on the machine and goes on)")
            # an idle Linux sleeps while it waits for hands (KDE, 2026-10-28): the test holds it awake
            held = hold_awake(ctx, quiet=True)
            seen = wait_owner_seen(ctx, st, int(st.get("owner_limit_s", 21600)))
            if held:
                release_awake(ctx)
            if not seen:
                out(f"oneclick: still waiting for: {st['owner_step']}\n  oneclick.py run {run_dir} looks again")
                return 3
            journal.write("owner-done", index=arg, stage=st["stage"], by="seen on the machine", check=st["owner_check"])
            out(f"oneclick: seen on the machine: {st['owner_step']}")
            continue
        if action == "stop":
            if not stopped_since_retry(journal.entries()):
                journal.write("run-stopped", why=arg)
            out(f"oneclick: stopped - {arg}")
            last = [e for e in journal.entries() if e["event"] == "stage-done"]
            if last and not last[-1].get("expected"):
                out(f"  try it again in this run: oneclick.py retry {run_dir}" + ("" if last[-1].get("untouched") else " --undone 'WHAT WAS PUT BACK'"))
            if autologon_state(journal.entries()):
                out(f"  the test's automatic sign-in is still on in the machine's Windows (for the retry); off: oneclick.py autologon-off {run_dir}")
            return 1
        i = arg
        st = plan["stages"][i]
        tries = sum(1 for e in journal.entries() if e["event"] == "stage-start" and e["index"] == i)
        name = f"{i + 1:02d}-{st['stage']}" + (f"-try{tries + 1}" if tries else "")
        ctx.stage_dir = os.path.join(run_dir, "stages", name)
        impl = stage_impl(st["stage"])
        missing = [k for k in stage_needs(st) if k not in secrets]
        if missing:
            journal.write("run-stopped", why=f"stage {i} ({st['stage']}) needs an approval this run was not given: {', '.join(missing)} (oneclick.py consent RUN_DIR)")
            out(f"oneclick: stopped - stage {i + 1} {st['stage']} needs {', '.join(missing)}; give it with: oneclick.py consent {run_dir}")
            return 1
        os.makedirs(ctx.stage_dir, exist_ok=False)
        journal.write("stage-start", index=i, stage=st["stage"], folder=name)
        autologon_before = bool(autologon_state(journal.entries()))
        out(f"oneclick: stage {i + 1} {st['stage']} ...")
        held = hold_awake(ctx) if st.get("hold_awake") else False
        no_sleep = windows_awake(ctx) if plan.get("test_windows_awake") else False
        try:
            o = impl.run(ctx, st)
        except Exception as e:  # a manager bug is a stop, never a pass
            o = Outcome("manager-error", False, f"{type(e).__name__}: {e}")
        if held:
            release_awake(ctx)
            o.notes = (o.notes + "; " if o.notes else "") + "the TEST held a sleep lock on Linux for this stage (settle-in holds none: 2026-10-04, 2026-10-28)"
        if no_sleep:
            o.notes = (o.notes + "; " if o.notes else "") + "the TEST had idle sleep off in Windows for this stage (test_windows_awake; no person is at the keyboard between the product's programs)"
        if st.get("row_note"):   # what this run's plan says a hand did before the stage: in the row, never left out
            o.notes = (o.notes + "; " if o.notes else "") + st["row_note"]
        if autologon_before or autologon_state(journal.entries()):
            o.notes = (o.notes + "; " if o.notes else "") + "the TEST's automatic sign-in was on in Windows for this stage (test_autologon; the person signs in by hand)"
        n = sha256sums(ctx.stage_dir)
        journal.write("stage-done", index=i, stage=st["stage"], word=o.word, expected=bool(o.expected), notes=o.notes, files=n,
                      untouched=bool(o.untouched or not impl.writes))
        out(f"oneclick: stage {i + 1} {st['stage']}: {o.word}{'' if o.expected else ' (NOT expected)'} - {o.notes}")


def ask_consents(needed, link, secrets, ask=input, out=print):
    """Every approval a stage needs, typed in full (kickoff, or `consent`
    for one a newer stage added). Returns False when one was not given."""
    for key in needed:
        if key == "password":
            # kickoff may have set it: the plan's fixed test password, or an earlier run's
            secrets.setdefault("password", pysecrets.token_urlsafe(12))
            continue
        if key == "walkaway-account":
            # the fresh Windows gets the SSH login's own name, so the manager reaches it the same way
            if link.kind == "ssh":
                g = subprocess.run(["ssh", "-G", link.host], capture_output=True, text=True).stdout
                m = re.search(r"^user (\S+)$", g, re.M)
                secrets[key] = m.group(1) if m else ""
            else:
                secrets[key] = "rig"
            if not secrets[key]:
                out("oneclick: no SSH user for the machine; the walk-away cannot name its account. Nothing started.")
                return False
            continue
        if key == "walkaway-stick":
            if link.kind != "ssh":
                secrets[key] = ""   # the rig's stand-in uses no stick
                continue
            hint, who = "", link.probe()
            if who == "windows":
                hint = link.run("Get-PhysicalDisk | Where-Object BusType -eq 'USB' | ForEach-Object { \"  $($_.Model)  ($([math]::Round($_.Size/1e9,1)) GB)\" }", "windows", 60).out
            elif who == "linux":   # the word settle-in checks is Linux's own model name
                hint = link.run("lsblk -dno MODEL,SIZE,TRAN | awk '$NF==\"usb\" {$NF=\"\"; print \"  \" $0}'", "linux", 60).out
            out("\nType the walk-away stick's model word exactly, to approve writing Windows' installer onto it."
                "\nUSB drives on the machine now (the kit stick is one of them):\n" + (hint or "  (none listed)"))
            secrets[key] = ask("> ").strip()
            if not secrets[key]:
                out("oneclick: no word typed. Nothing started.")
                return False
            continue
        if key == "kit-stick":
            if link.kind != "ssh":
                secrets[key] = ""   # the rig's stand-in builds its own stick
                continue
            kv = kv_lines(link.run(USB_FACTS, "windows", 120).out) if link.probe() == "windows" else {}
            if not kv.get("TARGET"):
                out("oneclick: the kit-stick stage needs Windows answering and exactly one USB stick in the machine "
                    f"(found: {kv.get('USB_DISKS', 'no answer')}). Nothing started.")
                return False
            want, what = (kv["LABEL"], "label") if kv.get("LABEL") else (kv.get("MODEL", ""), "model name")
            out(f"\nThe one USB stick in the machine now: {kv.get('MODEL')}, {int(kv['BYTES']) / 1e9:.1f} GB" +
                (f", labelled {kv['LABEL']}" if kv.get("LABEL") else ", no label") +
                f".\nThe run ERASES it and writes the kit onto it. Type its {what} exactly to approve (the stick writer checks the same word):")
            if ask("> ").strip() != want or not want:
                out(f"oneclick: that is not the stick's {what}. Nothing started.")
                return False
            secrets[key], secrets["kit-stick-bytes"] = want, int(kv["BYTES"])
            continue
        if key == "kit-stick-after-walkaway":
            if link.kind != "ssh":
                secrets[key] = ""
                continue
            want = walkaway_label()
            out(f"\nAfter the walk-away the same stick is Windows' installer, labelled {want}. The run ERASES it again there, to write the kit back "
                f"for the last stages.\nType {want} to approve:")
            if ask("> ").strip() != want:
                out("oneclick: that is not the label. Nothing started.")
                return False
            secrets[key] = want
            continue
        s = sentence(key)
        out(f"\nType this sentence exactly, to approve the {key} step for every loop of this run:\n  {s}")
        if ask("> ") != s:
            out("oneclick: that is not the sentence. Nothing started.")
            return False
        secrets[key] = s
    return True


def kickoff(plan_path, spec, ask=input, out=print, root=None, password_from=None, ask_secret=None, follow=None):
    """password_from: an earlier run on the same machine, whose installed
    Linux this run goes on with (a tail run): its account keeps that run's
    password. Else the plan's test_password (the owner, 2026-10-28: the
    Aspire's test account is 'password' from its next install on), else a
    random one.

    test_autologon: the Windows account's password for the test's automatic
    sign-in. The plan's "windows_password": "test_password" when Windows'
    account has the test password (the Aspire's since 2026-10-29), else typed
    here, hidden.

    follow: an ssh run reads the machine's identity now, over the owner's own
    login, so it can follow the machine to new addresses and keys (default
    on for ssh; tr.seed_follow)."""
    with open(plan_path, encoding="utf-8") as f:
        plan = json.load(f)
    link = tr.from_spec(spec)
    secrets = {}
    if password_from:
        _, earlier = load(password_from)
        if not earlier.get("password"):
            out(f"oneclick: {password_from} holds no password. Nothing started.")
            return None
        secrets["password"] = earlier["password"]
    elif plan.get("test_password"):
        secrets["password"] = plan["test_password"]
    bad = [st["owner_check"] for st in plan["stages"] if "owner_check" in st and st["owner_check"] not in OWNER_CHECKS]
    if bad:
        raise SystemExit(f"oneclick: the plan names owner checks the manager does not know: {', '.join(bad)}")
    lx = plan.get("linux") or {}
    wants_linux = any(st.get("owner_check") == "linux-on-stick" or (st["stage"] == "kit-stick" and st.get("fetch")) for st in plan["stages"])
    if wants_linux and not all(re.fullmatch(r"[A-Za-z0-9._-]+", str(lx.get(k, ""))) for k in ("release", "desktop")):
        raise SystemExit("oneclick: the plan fetches a Linux (or waits for the window to) and does not name it: \"linux\": {\"release\": ..., \"desktop\": ...}")
    needed = plan_needs(plan["stages"])
    out(f"oneclick {VERSION}: {len(plan['stages'])} stages on {plan.get('machine', '?')} over {link.describe()}")
    for i, st in enumerate(plan["stages"]):
        built = "" if st["stage"] in STAGES else "   (not built yet: the run stops here)"
        out(f"  {i + 1:2d}. {st['stage']}{built}")
        if st.get("owner_step"):
            out(f"      (first waits for you: {st['owner_step']}" + (" The manager sees when it is done.)" if st.get("owner_check") else ")"))
    if not ask_consents(needed, link, secrets, ask, out):
        return None
    if plan.get("test_autologon") and link.kind == "ssh":
        if plan.get("windows_password") == "test_password":
            secrets["windows-password"] = secrets["password"]
        else:
            import getpass
            pw = (ask_secret or getpass.getpass)("\nThe Windows account's password, for the test's automatic sign-in (kept in the run folder only; hidden): ")
            if not pw:
                out("oneclick: no password typed. Nothing started.")
                return None
            secrets["windows-password"] = pw
        out("oneclick: the test signs Windows in by itself when a stage needs the desktop (automatic sign-in, off again when the run finishes)")
    run_dir = os.path.join(root or runs_root(), f"{plan.get('machine', 'machine')}-{stamp()}")
    os.makedirs(run_dir, exist_ok=False)
    if (follow if follow is not None else link.kind == "ssh") and not tr.seed_follow(link.host, run_dir, plan.get("machine", "machine"), out):
        out("  (the run reaches the machine as `ssh " + link.host + "` does; a new address or key stops it)")
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


def consent(run_dir, ask=input, out=print):
    """The approvals a run's stages need that it was not given at kickoff."""
    run, secrets = load(run_dir)
    link = tr.from_spec(run["transport"], run_dir)
    needed = [n for n in plan_needs(run["plan"]["stages"]) if n not in secrets]
    if not needed:
        out("oneclick: nothing is missing")
        return 0
    if not ask_consents(needed, link, secrets, ask, out):
        return 2
    p = os.path.join(run_dir, "secrets.json")
    tmp = p + ".new"
    fd = os.open(tmp, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, "w", encoding="utf-8") as f:
        json.dump(secrets, f)
    os.replace(tmp, p)
    Journal(os.path.join(run_dir, "journal.jsonl")).write("consent-given", keys=needed)
    out(f"oneclick: given: {', '.join(needed)}")
    return 0


def status(run_dir, out=print):
    run, _ = load(run_dir)
    entries = Journal(os.path.join(run_dir, "journal.jsonl")).entries()
    states = stage_states(entries)
    for i, st in enumerate(run["plan"]["stages"]):
        e = states.get(i)
        word = "-" if e is None else (e.get("word") or e["event"])
        out(f"  {i + 1:2d}. {st['stage']:<14} {word}")
    mj = os.path.join(run_dir, "machine.json")
    if os.path.exists(mj):
        with open(mj, encoding="utf-8") as f:
            m = json.load(f)
        out(f"  machine: last found at {m['address']} (seen at {', '.join(m.get('addresses', []))})")
    if autologon_state(entries):
        out("  the test's automatic sign-in is ON in the machine's Windows")
    action, arg = next_action(run["plan"], entries)
    if action == "wait":
        st = run["plan"]["stages"][arg]
        how = " (the manager watches for it)" if st.get("owner_check") in OWNER_CHECKS else f" (then: oneclick.py done {run_dir})"
        out(f"  next: WAITING for you before stage {arg + 1}: {st['owner_step']}{how}")
        return
    lock = os.path.join(run_dir, ".awake")
    live = os.path.exists(lock) and time.time() - os.path.getmtime(lock) < 180
    last = entries[-1] if entries else {}
    if live and last.get("event") in ("stage-start", "heartbeat", "note", "hang"):
        open_stage = [e for e in entries if e["event"] == "stage-start"][-1]
        out(f"  now: RUNNING stage {open_stage['index'] + 1} {open_stage['stage']} (the manager refreshed its lock {int(time.time() - os.path.getmtime(lock))} s ago)")
        return
    out(f"  next: {action}{' - ' + str(arg) if arg is not None else ''}")


def done(run_dir, out=print):
    """The owner did the step the run waits for (said by the owner, typed
    by whoever runs this): the chain may go on at its next `run`."""
    run, _ = load(run_dir)
    journal = Journal(os.path.join(run_dir, "journal.jsonl"))
    action, arg = next_action(run["plan"], journal.entries())
    if action != "wait":
        out("oneclick: nothing waits for the owner")
        return 2
    journal.write("owner-done", index=arg, stage=run["plan"]["stages"][arg]["stage"])
    out(f"oneclick: done: {run['plan']['stages'][arg]['owner_step']}")
    return 0


def retry(run_dir, undone=None, out=print):
    """Try the stage the run stopped at again, in the same run: the
    approvals typed at kickoff stand, the stage gets a new folder (-tryN),
    and the fail stays in the journal and the rows. A stage that stopped
    before changing anything goes again as it is; one that may have changed
    the machine (the prologue started, the walk-away prepared, a writer was
    interrupted) goes again only with `--undone`, the owner's words for what
    was put back (the Aspire, 2026-10-29: `walkaway undo`), written down."""
    run, _ = load(run_dir)
    journal = Journal(os.path.join(run_dir, "journal.jsonl"))
    entries = journal.entries()
    action, why = next_action(run["plan"], entries)
    if action != "stop":
        out(f"oneclick: nothing to try again: the run is not stopped ({action})")
        return 2
    marks = [e for e in entries if e["event"] in ("stage-start", "stage-done")]
    if not marks:
        out("oneclick: no stage has run yet")
        return 2
    e = stage_states(entries)[marks[-1]["index"]]
    i, stage = e["index"], e["stage"]
    if e["event"] == "stage-done" and e.get("expected"):
        out(f"oneclick: the run stopped for another reason, not a stage's result: {why}")
        return 2
    clean_stop = e["event"] == "stage-done" and e.get("untouched")
    word = e.get("word") or "interrupted"
    if not clean_stop and not (undone or "").strip():
        out(f"oneclick: stage {i + 1} {stage} ended '{word}' and may have changed the machine. Put it back first, then say what you did:\n"
            f"  oneclick.py retry {run_dir} --undone 'WHAT WAS PUT BACK'")
        return 2
    journal.write("stage-retry", index=i, stage=stage, after=word, untouched=bool(clean_stop), undone=(undone or "").strip())
    out(f"oneclick: stage {i + 1} {stage} goes again at the next run (after '{word}'" + ("" if clean_stop else f"; undone: {undone.strip()}") + ")")
    return 0


def ssh_to(run_dir, command, out=print):
    """The machine, reached the way the run reaches it (its address and
    trusted keys), for a look by hand. Nothing is recorded."""
    run, _ = load(run_dir)
    link = tr.from_spec(run["transport"], run_dir)
    if not command:
        if isinstance(link, tr.Follow):
            os.execvp("ssh", ["ssh", *link.opts, link.host])
        os.execvp("ssh", ["ssh", link.host])
    r = link.raw(" ".join(command), timeout=600)
    out(r.out + r.err, end="")
    return 255 if r.rc is None else r.rc


def main(argv=None):
    ap = argparse.ArgumentParser(prog="oneclick.py")
    sub = ap.add_subparsers(dest="cmd", required=True)
    k = sub.add_parser("kickoff")
    k.add_argument("--plan", required=True)
    k.add_argument("--transport", required=True)
    k.add_argument("--no-run", action="store_true", help="make the run folder only")
    k.add_argument("--password-from", help="an earlier run on this machine whose installed Linux this run goes on with")
    r = sub.add_parser("run")
    r.add_argument("run_dir")
    s = sub.add_parser("status")
    s.add_argument("run_dir")
    c = sub.add_parser("consent")
    c.add_argument("run_dir")
    dn = sub.add_parser("done", help="the owner did the step the run waits for")
    dn.add_argument("run_dir")
    rt = sub.add_parser("retry", help="try the stage the run stopped at again, in the same run")
    rt.add_argument("run_dir")
    rt.add_argument("--undone", help="what was put back, when the stage may have changed the machine")
    sh = sub.add_parser("ssh", help="reach the machine where the run last found it")
    sh.add_argument("run_dir")
    sh.add_argument("command", nargs=argparse.REMAINDER)
    ao = sub.add_parser("autologon-off", help="switch the test's automatic sign-in off in the machine's Windows")
    ao.add_argument("run_dir")
    pr = sub.add_parser("promote", help="the run's rows, scrubbed, into docs/validation-results/oneclick-runs.csv")
    pr.add_argument("run_dir")
    pr.add_argument("--csv", default=os.path.join(REPO, "docs", "validation-results", "oneclick-runs.csv"))
    pr.add_argument("--facts", help="another run's facts.json, for a run that recorded none (said in the notes)")
    pr.add_argument("--dry-run", action="store_true")
    sr = sub.add_parser("stage-rows", help="the pull and reclaim-check stages' own lines, into v3-pull.csv and r41-reclaim.csv")
    sr.add_argument("run_dir")
    a = ap.parse_args(argv)
    if a.cmd == "kickoff":
        d = kickoff(a.plan, a.transport, password_from=a.password_from)
        if d is None:
            return 2
        return 0 if a.no_run else run_chain(d)
    if a.cmd == "run":
        return run_chain(a.run_dir)
    if a.cmd == "consent":
        return consent(a.run_dir)
    if a.cmd == "done":
        return done(a.run_dir)
    if a.cmd == "retry":
        return retry(a.run_dir, a.undone)
    if a.cmd == "ssh":
        return ssh_to(a.run_dir, a.command)
    if a.cmd == "autologon-off":
        run, _ = load(a.run_dir)
        return 0 if autologon_off(tr.from_spec(run["transport"], a.run_dir), Journal(os.path.join(a.run_dir, "journal.jsonl"))) else 1
    if a.cmd == "stage-rows":
        return stage_rows(a.run_dir)
    if a.cmd == "promote":
        import rows
        return rows.promote(a.run_dir, a.csv, a.facts, a.dry_run)
    status(a.run_dir)
    return 0


if __name__ == "__main__":
    sys.exit(main())
