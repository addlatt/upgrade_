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


def boot_windows_from_linux(ctx):
    """Linux -> Windows with no key: a one-time BootNext to the firmware's
    Windows Boot Manager entry, then a restart."""
    r = sudo(ctx, "n=$(efibootmgr | sed -n 's/^Boot\\([0-9A-F]\\{4\\}\\)\\*\\? Windows Boot Manager.*/\\1/p' | head -1); "
                  "[ -n \"$n\" ] || { echo NO_WINDOWS_ENTRY; exit 3; }; efibootmgr -n $n >/dev/null && echo BOOTNEXT=$n; "
                  "systemd-run --on-active=5 systemctl reboot >/dev/null; echo REBOOTING")
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


def write_job(ctx, L, args, erase=False):
    """The job. A real machine: the product's job writer over SSH, with the
    sentences typed at kickoff. The rig: the stand-in (the writer refuses the
    rig's SCSI "stick", R16; rig/hyperv/prologue.sh convert says why).
    Returns None, or the Outcome that stops the stage."""
    link = ctx.link
    hashed = subprocess.run(["openssl", "passwd", "-6", "-stdin"], input=ctx.secrets["password"], capture_output=True, text=True).stdout.strip()
    if not hashed.startswith("$6$"):
        return Outcome("manager-error", False, "openssl gave no SHA-512 crypt hash")
    if link.kind == "hyperv":
        pub = os.path.expanduser("~/.ssh/upgrade_rig.pub")
        env = dict(os.environ, ONECLICK_HASH=hashed, ONECLICK_SSH_PUB=pub)
        cmd = ["./v9.sh", "job", f"--spoof-ssh={pub}", *args.get("rig_job_args", [])] if erase else ["./v1.sh", "job"]
        j = subprocess.run(cmd, cwd=os.path.join(REPO, "rig", "hyperv"), env=env, capture_output=True, text=True, timeout=1800)
        ctx.save("job-standin.log", j.stdout + j.stderr)
        if j.returncode != 0:
            return Outcome("job-failed", False, "the rig's job stand-in failed: read job-standin.log")
    else:
        hf = os.path.join(ctx.stage_dir, ".hash")
        with open(hf, "w") as f:
            f.write(hashed + "\n")
        link.push(hf, "C:\\upgrade_oneclick\\pw.txt", "windows")
        os.remove(hf)
        ack = ctx.secrets.get("data-loss")
        jw = link.run(
            f"& {L}\\upgrade-job.exe write --stick {L} --out {L}\\upgrade_ --scan {L}\\upgrade_\\reports --desktop {args.get('desktop', 'kde')} "
            f"--start-at {args.get('start_at', 'desktop')} --if-cannot-keep stop --password-hash-file C:\\upgrade_oneclick\\pw.txt "
            f"--kickstart {L}\\upgrade_\\ks.cfg --stick-label UPGV0 --manifest {L}\\SHA256SUMS" +
            (f" --erase-everything '{ctx.secrets['erase']}'" if erase else "") +
            (f" --acknowledge-data-loss '{ack}'" if ack else "") +
            "; $rc = $LASTEXITCODE; Remove-Item C:\\upgrade_oneclick\\pw.txt -Force; Write-Output \"JOB_RC=$rc\"", "windows", timeout=1800)
        ctx.save("job-writer.out", jw.out + jw.err)
        if "JOB_RC=0" not in jw.out:
            return Outcome("job-refused", False, (jw.out + jw.err).strip()[-300:])
    link.pull(f"{L}\\upgrade_\\job.json", os.path.join(ctx.stage_dir, "job.json"), "windows")
    return None


def arm(ctx, L, erase=False, markers=()):
    """The bench marker (boot captures and boots.log; never autoshutdown:
    the manager restarts nothing by power), any test markers, the Windows
    collector, then the prologue exactly as the window runs it. Returns
    None when the prologue reached its restart, else the stopping Outcome."""
    link = ctx.link
    extra = "".join(f"Set-Content -Path {L}\\upgrade_\\{n} -Value '{v}'; " for n, v in markers)
    link.run(f"Set-Content -Path {L}\\upgrade_\\bench -Value oneclick; Remove-Item {L}\\upgrade_\\autoshutdown,{L}\\upgrade_\\test-cancel-countdown -ErrorAction SilentlyContinue; " + extra +
             f"Add-Content -Path {L}\\upgrade_\\boots.log -Value ('windows-clock-before,' + (Get-Date).ToUniversalTime().ToString('o') + ',host-utc={utc()}')",
             "windows", timeout=60)
    if not collect_windows(ctx, "before-install", "before"):
        return Outcome("collect-failed", False, "the Windows collector before the conversion")
    ack = ctx.secrets.get("data-loss")
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
    ('linux' after an install, 'windows' after a cancel or a refusal):
    Windows may answer in between (an update, the disk check, the return);
    a stopped outcome ends the wait early. Returns 'linux' | 'windows' |
    'stopped' | None (a hang)."""
    link = ctx.link
    def arrived():
        who = link.probe()
        if who == "linux" and want == "linux":
            return "linux"
        if who == "windows":
            st = link.run(FIND_STICK + "if (Test-Path \"$L\\upgrade_\\outcome.json\") { (Get-Content \"$L\\upgrade_\\outcome.json\" -Raw | ConvertFrom-Json).status }; "
                          "if (Test-Path \"$L\\upgrade_\\report\\countdown.json\") { 'COUNTDOWN=' + (Get-Content \"$L\\upgrade_\\report\\countdown.json\" -Raw | ConvertFrom-Json).result }; "
                          "if (Test-Path \"$L\\upgrade_\\report\\refusal.json\") { 'REFUSED' }", "windows", timeout=60)
            if "stopped" in st.out:
                return "stopped"
            if want == "windows" and ("COUNTDOWN=cancelled" in st.out or "REFUSED" in st.out):
                return "windows"
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
        if link.probe() != "windows":
            return Outcome("wrong-system", False, "keep-windows starts in Windows")
        L = find_stick(ctx)
        if not L:
            return Outcome("stick-not-found", False, "no single volume labelled UPGV0")
        stop = write_job(ctx, L, args) or arm(ctx, L)
        if stop:
            return stop
        got = wait_after_arm(ctx, L, "linux", int(args.get("limit_s", 10800)))
        if got is None:
            return Outcome("hang-install", False, "no Linux within the limit (the journal has the heartbeats)")
        if got == "stopped":
            link.pull(f"{L}\\upgrade_\\outcome.json", os.path.join(ctx.stage_dir, "outcome-stopped.json"), "windows")
            return Outcome("prologue-stopped", False, "the prologue stopped after a restart: outcome-stopped.json")
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
                return Outcome("windows-entry-missing", False, why.strip()[:200])
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
        return Outcome("kept-both-start", True, "installed beside Windows; Windows and Linux each started twice with no key")


class CleanSlate:
    """Stage 5: erase and install (V9 arm C, R27). The countdown runs out by
    itself (no marker), the installer erases the drives the job names, and
    the new Linux answers."""
    writes = True
    needs = ("password", "erase")

    def run(self, ctx, args):
        link = ctx.link
        if link.probe() != "windows":
            return Outcome("wrong-system", False, "the clean slate starts in Windows")
        L = find_stick(ctx)
        if not L:
            return Outcome("stick-not-found", False, "no single volume labelled UPGV0")
        stop = write_job(ctx, L, args, erase=True) or arm(ctx, L, erase=True)
        if stop:
            return stop
        got = wait_after_arm(ctx, L, "linux", int(args.get("limit_s", 10800)))
        if got is None:
            return Outcome("hang-install", False, "no Linux within the limit (the journal has the heartbeats)")
        if got == "stopped":
            return Outcome("prologue-stopped", False, "the prologue stopped after a restart")
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
        return Outcome("erased-installed", True, f"path_taken {o.get('path_taken')}; the countdown ran out and the installer erased what the job named")


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
            return Outcome("wrong-system", False, "the stick-left-in test starts in the installed Linux")
        tables = "for d in $(lsblk -dno NAME,TRAN | awk '$2!=\"usb\"{print $1}'); do echo \"$d $(head -c 1048576 /dev/$d | sha256sum | cut -c1-64)\"; done"
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
        t1 = sudo(ctx, tables, 120)
        ctx.save("tables-after.txt", t1.out)
        if not t0.out.strip() or t0.out != t1.out:
            return Outcome("installed-again", False, "a partition table changed after the stick started (or was not read)")
        collect_linux(ctx, "after-stick")
        return Outcome("not-installed-again", True, "the stick started, did not install, and the installed Linux came back unchanged")


class CancelCountdown:
    """Stage 8 (V9 arm B, R27, R38): an erase armed for real, cancelled at
    the countdown by the test marker that can only cancel (upgrade-installer
    0.1.4). Windows must come back, the record must say test-marker, and the
    internal drives must be untouched."""
    writes = True
    needs = ("password", "erase")

    def run(self, ctx, args):
        link = ctx.link
        if link.probe() != "windows":
            return Outcome("wrong-system", False, "the cancel starts in Windows")
        L = find_stick(ctx)
        if not L:
            return Outcome("stick-not-found", False, "no single volume labelled UPGV0")
        before = boot_mark(ctx, "windows")
        stop = write_job(ctx, L, args, erase=True) or arm(ctx, L, erase=True, markers=[("test-cancel-countdown", str(args.get("cancel_after_s", 10)))])
        if stop:
            return stop
        got = wait_after_arm(ctx, L, "windows", int(args.get("limit_s", 3600)))
        if got != "windows":
            return Outcome("hang-cancel" if got is None else f"cancel-{got}", False, "Windows did not come back with a cancelled countdown")
        if boot_mark(ctx, "windows") == before:
            return Outcome("not-restarted", False, "the same Windows boot answered: the installer never ran")
        link.pull(f"{L}\\upgrade_\\report", os.path.join(ctx.stage_dir, "stick-report"), "windows")
        try:
            with open(os.path.join(ctx.stage_dir, "stick-report", "countdown.json"), encoding="utf-8-sig") as f:
                cd = json.load(f)
        except (OSError, ValueError):
            cd = {}
        if cd.get("result") != "cancelled" or cd.get("by") != "test-marker":
            return Outcome("cancel-unrecorded", False, f"countdown.json: {cd}")
        if not collect_windows(ctx, "direct", "after-cancel"):
            return Outcome("collect-failed", False, "the Windows collector after the cancel")
        a, b = first_mib(ctx, "windows-before"), first_mib(ctx, "windows-after-cancel")
        if not a or a != b:
            return Outcome("disk-changed", False, f"the system drive's first MiB before {a} after {b}")
        return Outcome("cancelled-untouched", True, "the countdown was cancelled by the test marker; Windows came back; the system drive's partition table is unchanged")


SETTLE_IN = "/usr/local/libexec/upgrade_/settle-in"


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
            return Outcome("wrong-system", False, "the rollback starts in Linux")
        dry = sudo(ctx, f"{SETTLE_IN} rollback --dry-run", 300)
        ctx.save("rollback-dry-run.json", dry.out + dry.err)
        if dry.rc != 0 or '"plan"' not in dry.out:
            return Outcome("rollback-refused", False, (dry.out + dry.err).strip()[-300:])
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


STAGES = {"look": Look(), "keep-windows": KeepWindows(), "rollback": Rollback(), "rollback-existing": Rollback(),
          "stick-left-in": StickLeftIn(), "clean-slate": CleanSlate(), "stick-left-in-erased": StickLeftIn(),
          "cancel-countdown": CancelCountdown()}


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
