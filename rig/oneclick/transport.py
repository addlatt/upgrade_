"""How the test manager reaches the machine under test (roadmap item 21).

Two channels, one interface, so the same stages run on the rig first and
on a real machine second (rule #5):

  Ssh     a real machine: Windows' OpenSSH (a cmd.exe login shell; every
          Windows command goes through PowerShell as -EncodedCommand, so no
          quoting survives two shells) or Linux's sshd. Key only, BatchMode.
  HyperV  the rig's Windows guest over PowerShell Direct, as rig/hyperv/vm.ps1
          does (rig/rig). It has no Linux channel yet: probe() answers only
          "windows" or None.

run() never raises on a failed command: a test manager records what the
machine said, and silence is an answer too (rc None).
"""

import base64
import datetime
import json
import os
import re
import shutil
import subprocess
import uuid


def encoded(script):
    """PowerShell's -EncodedCommand form: base64 of UTF-16LE."""
    return base64.b64encode(script.encode("utf-16-le")).decode("ascii")


# Windows caps a command line at 32,767 characters, and cmd.exe (Windows' SSH
# login shell) at 8,191: the collector script, encoded, is past both (the rig,
# 2026-10-27: "powershell.exe: Invalid argument"). A long script goes to the
# machine as a file and runs as a script block read from it - the execution
# policy applies to script files, not to script blocks, so it is never touched.
LONG = 6000


def long_script_wrapper(link, script, timeout, stdin=""):
    import tempfile
    name = f"C:\\upgrade_oneclick\\scripts\\{uuid.uuid4().hex}.ps1"
    fd, local = tempfile.mkstemp(suffix=".ps1")
    with os.fdopen(fd, "w", encoding="utf-8-sig") as f:
        f.write(script)
    try:
        link.run("New-Item -ItemType Directory -Force C:\\upgrade_oneclick\\scripts | Out-Null", "windows", 60)
        p = link.push(local, name, "windows")
        if not p.ok:
            return Result(p.rc, p.out, f"sending the script: {p.err}")
        return link.run(f"try {{ & ([scriptblock]::Create((Get-Content -Raw -Encoding UTF8 '{name}'))) }} "
                        f"finally {{ Remove-Item '{name}' -Force -ErrorAction SilentlyContinue }}", "windows", timeout,
                        **({"stdin": stdin} if stdin else {}))
    finally:
        os.remove(local)


class Result:
    def __init__(self, rc, out, err):
        self.rc, self.out, self.err = rc, out, err

    @property
    def ok(self):
        return self.rc == 0

    def __repr__(self):
        return f"Result(rc={self.rc}, out={self.out[:80]!r}, err={self.err[:80]!r})"


def _call(argv, timeout, stdin=None):
    try:
        p = subprocess.run(argv, input=stdin, capture_output=True, text=True, timeout=timeout,
                           encoding="utf-8", errors="replace")
        return Result(p.returncode, p.stdout.replace("\r", ""), p.stderr.replace("\r", ""))
    except subprocess.TimeoutExpired:
        return Result(None, "", f"timed out after {timeout} s")
    except OSError as e:
        return Result(None, "", str(e))


class Ssh:
    kind = "ssh"

    def __init__(self, host, extra=(), ssh="ssh", scp="scp", windows_client=False):
        self.host, self.ssh, self.scp, self.windows_client = host, ssh, scp, windows_client
        self.opts = ["-o", "BatchMode=yes", "-o", "ConnectTimeout=10",
                     "-o", "ServerAliveInterval=15", "-o", "ServerAliveCountMax=4", *extra]

    def describe(self):
        return f"ssh:{self.host}"

    def raw(self, remote, timeout=60, stdin=""):
        return _call([self.ssh, *self.opts, self.host, remote], timeout, stdin=stdin)

    def probe(self):
        """Which system answers: 'windows', 'linux' or None (silence).
        One call works in both shells: cmd expands %OS%, bash runs uname."""
        r = self.raw("echo %OS% $(uname -s)", timeout=25)
        if r.rc is None or r.rc == 255:
            return None
        if "Windows_NT" in r.out:
            return "windows"
        if "Linux" in r.out:
            return "linux"
        return None

    def run(self, script, os_kind, timeout=600, stdin=""):
        """stdin: what the command reads (a secret travels here, never on a
        command line another program on the machine could read)."""
        if os_kind == "windows":
            if len(encoded(script)) > LONG:
                return long_script_wrapper(self, script, timeout, stdin)
            return self.raw(f"powershell -NoProfile -NonInteractive -EncodedCommand {encoded(script)}", timeout, stdin)
        return self.raw(script, timeout, stdin)

    def pull(self, remote, local, os_kind, timeout=1800):
        """Copy a remote file or folder to local. Windows paths become
        /C:/x/y, the form Windows' OpenSSH scp accepts."""
        if os_kind == "windows":
            remote = "/" + remote.replace("\\", "/")
        os.makedirs(os.path.dirname(os.path.abspath(local)), exist_ok=True)
        dest = local
        if self.windows_client:  # Windows' scp.exe writes to a Windows path
            dest = subprocess.run(["wslpath", "-w", os.path.abspath(local)], capture_output=True, text=True).stdout.strip()
        return _call([self.scp, "-r", "-q", *self.opts, f"{self.host}:{remote}", dest], timeout, stdin="")

    def push(self, local, remote, os_kind, timeout=600):
        if os_kind == "windows":
            remote = "/" + remote.replace("\\", "/")
        src = local
        if self.windows_client:
            src = subprocess.run(["wslpath", "-w", os.path.abspath(local)], capture_output=True, text=True).stdout.strip()
        return _call([self.scp, "-q", *self.opts, src, f"{self.host}:{remote}"], timeout, stdin="")


# --- following a real machine across its systems ---------------------------
#
# The Aspire, 2026-10-28/29: its Windows and its Linux get different addresses
# from the router (192.168.0.211, .79, then .245 after the clean slate), and
# every install has a new SSH host key. Three times the owner ran a script by
# hand so `ssh aspire` reached the machine again. Follow does what those
# scripts did, the same way: host-key checks stay on, in the run's own
# known_hosts (the owner's ~/.ssh is never touched), and a NEW key is trusted
# only after the system behind it proves it is the same computer: the same
# model, and a network card or an internal drive with the same serial as the
# ones read at kickoff over the owner's trusted login. It must also accept the
# key the job carried (it is a login like any other).

IDENTITY_WINDOWS = (
    "$ProgressPreference = 'SilentlyContinue'; $c = Get-CimInstance Win32_ComputerSystem; "
    "$m = @(Get-NetAdapter -Physical -ErrorAction SilentlyContinue | ForEach-Object { $_.PermanentAddress, $_.MacAddress }); "
    "$d = @(Get-PhysicalDisk | Where-Object BusType -ne 'USB' | ForEach-Object { $_.SerialNumber }); "
    "[ordered]@{ os = 'windows'; model = $c.Model; macs = $m; disks = $d } | ConvertTo-Json -Compress")
IDENTITY_LINUX = (
    "python3 -c 'import json,os,subprocess as s\n"
    "n=\"/sys/class/net/\"; m=[]\n"
    "for i in os.listdir(n):\n"
    " if os.path.exists(n+i+\"/device\"):\n"
    "  m.append(open(n+i+\"/address\").read().strip())\n"
    "  p=s.run([\"ip\",\"-d\",\"-o\",\"link\",\"show\",i],capture_output=True,text=True).stdout.split()\n"
    "  m+= [p[p.index(\"permaddr\")+1]] if \"permaddr\" in p else []\n"
    "d=[l.split()[0] for l in s.run([\"lsblk\",\"-dno\",\"SERIAL,TRAN\"],capture_output=True,text=True).stdout.splitlines() if l.split() and l.split()[-1]!=\"usb\" and len(l.split())>1]\n"
    "g=lambda f: open(\"/sys/class/dmi/id/\"+f).read().strip() if os.path.exists(\"/sys/class/dmi/id/\"+f) else \"\"\n"
    "print(json.dumps(dict(os=\"linux\",model=g(\"product_name\"),macs=m,disks=d)))'")


def _norm(v):
    return re.sub(r"[^0-9A-Z]", "", str(v or "").upper())


def same_machine(known, seen):
    """(True, why) when `seen` is the computer `known` was read from: the
    same model, and at least one network card address or internal drive
    serial in common (normalised: Windows writes AA-BB-.., Linux aa:bb:..).
    Zeroed or empty values never count."""
    if not known or not seen:
        return False, "no identity to compare"
    if _norm(known.get("model")) != _norm(seen.get("model")) or not _norm(known.get("model")):
        return False, f"model {seen.get('model')!r}, not {known.get('model')!r}"
    def vals(d, k):
        return {x for x in (_norm(v) for v in (d.get(k) or [])) if len(x) >= 6 and set(x) != {"0"}}
    macs = vals(known, "macs") & vals(seen, "macs")
    disks = vals(known, "disks") & vals(seen, "disks")
    if not (macs or disks):
        return False, "the same model, but no network card or drive in common"
    return True, f"model {seen.get('model')}; {len(macs)} network card(s) and {len(disks)} drive(s) in common"


def read_identity(link):
    """The identity of whatever answers on `link` (read-only), or None."""
    who = link.probe()
    if who is None:
        return None
    r = link.run(IDENTITY_WINDOWS if who == "windows" else IDENTITY_LINUX, who, 90)
    for line in reversed(r.out.splitlines()):   # PowerShell's progress records may come after it
        if line.startswith("{") and '"model"' in line:
            try:
                return json.loads(line)
            except ValueError:
                return None
    return None


def sweep(prefix, port=22, timeout=1.0):
    """Every address prefix.1-254 with `port` open (the router's /24)."""
    import socket
    from concurrent.futures import ThreadPoolExecutor

    def open_(ip):
        try:
            socket.create_connection((ip, port), timeout=timeout).close()
            return ip
        except OSError:
            return None
    with ThreadPoolExecutor(64) as ex:
        return [ip for ip in ex.map(open_, [f"{prefix}.{n}" for n in range(1, 255)]) if ip]


def keyscan(ip, timeout=8):
    """{(type, key)} the SSH server at ip presents."""
    r = _call(["ssh-keyscan", "-T", "5", ip], timeout)
    return {(p[1], p[2]) for p in (l.split() for l in r.out.splitlines() if l and not l.startswith("#")) if len(p) >= 3}


class Follow(Ssh):
    """ssh:<host> inside a run: the machine is followed by address and
    host key, as above. State in the run folder: machine.json (the identity,
    the addresses seen, the one in use) and known_hosts (the keys trusted
    for this machine, under one alias)."""
    RELOCATE_EVERY_S = 120

    def __init__(self, host, run_dir, clock=None, **kw):
        import time as _t
        self.run_dir = run_dir
        self.state_path = os.path.join(run_dir, "machine.json")
        self.kh = os.path.join(run_dir, "known_hosts")
        with open(self.state_path, encoding="utf-8") as f:
            self.state = json.load(f)
        self.clock = clock or _t.monotonic
        self._last_relocate = None
        self.on_event = lambda event, **kv: None
        super().__init__(host, **kw)

    @property
    def opts(self):
        return ["-o", "BatchMode=yes", "-o", "ConnectTimeout=10", "-o", "ServerAliveInterval=15", "-o", "ServerAliveCountMax=4",
                "-o", "ProxyCommand=none", "-o", f"HostName={self.state['address']}", "-o", f"HostKeyAlias={self.state['alias']}",
                "-o", f"UserKnownHostsFile={self.kh}", "-o", "GlobalKnownHostsFile=/dev/null", "-o", "StrictHostKeyChecking=yes"]

    @opts.setter
    def opts(self, _):
        pass   # Ssh.__init__ sets its own; Follow's are computed

    def describe(self):
        return f"ssh:{self.host}"

    def _save(self):
        tmp = self.state_path + ".new"
        with open(tmp, "w", encoding="utf-8") as f:
            json.dump(self.state, f, indent=2)
        os.replace(tmp, self.state_path)

    def trusted(self):
        if not os.path.exists(self.kh):
            return set()
        with open(self.kh, encoding="utf-8") as f:
            return {(p[1], p[2]) for p in (l.split() for l in f) if len(p) >= 3}

    def raw(self, remote, timeout=60, stdin=""):
        r = super().raw(remote, timeout, stdin)
        if r.rc == 255 and self.relocate():
            r = super().raw(remote, timeout, stdin)
        return r

    def relocate(self):
        """After a failed connection: find the machine again. True when the
        address or the trusted keys changed (so the call is worth trying again)."""
        now = self.clock()
        if self._last_relocate is not None and now - self._last_relocate < self.RELOCATE_EVERY_S:
            return False
        self._last_relocate = now
        known = self.state.get("addresses", [])
        cands = list(dict.fromkeys([self.state["address"], *known, *self.sweep()]))
        trusted = self.trusted()
        unknown = []
        for ip in cands:
            keys = keyscan(ip)
            if not keys:
                continue
            if keys & trusted:
                if ip == self.state["address"]:
                    return False   # where it was, with a key we trust: it is just not answering yet
                return self._move(ip, "a key this run already trusts")
            unknown.append((ip, keys))
        for ip, keys in unknown:
            if self._try_new_key(ip):
                return True
        return False

    def sweep(self):
        return sweep(self.state["prefix"]) if self.state.get("prefix") else []

    def _move(self, ip, why):
        old = self.state["address"]
        self.state["address"] = ip
        if ip not in self.state.setdefault("addresses", []):
            self.state["addresses"].append(ip)
        self._save()
        self.on_event("machine-moved", **{"from": old, "to": ip, "why": why})
        return True

    def _try_new_key(self, ip):
        """Connect once to ip with a quarantine known_hosts (its key accepted
        for this one look), read who it is, and trust the key only if it is
        the same computer."""
        import tempfile
        fd, q = tempfile.mkstemp(prefix="oneclick-kh-", dir=self.run_dir)
        os.close(fd)
        try:
            look = Ssh(self.host, extra=["-o", "ProxyCommand=none", "-o", f"HostName={ip}", "-o", f"HostKeyAlias={self.state['alias']}",
                                         "-o", f"UserKnownHostsFile={q}", "-o", "GlobalKnownHostsFile=/dev/null",
                                         "-o", "StrictHostKeyChecking=accept-new"], ssh=self.ssh, scp=self.scp)
            seen = read_identity(look)
            ok, why = same_machine(self.state.get("identity"), seen)
            stamp_ = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
            with open(os.path.join(self.run_dir, f"follow-{stamp_}-{ip}.json"), "w", encoding="utf-8") as f:
                json.dump({"address": ip, "seen": seen, "same_machine": ok, "why": why}, f, indent=2)
            if not ok:
                self.on_event("host-key-refused", address=ip, why=why)
                return False
            with open(q, encoding="utf-8") as f:
                lines = [l for l in f if l.strip()]
            with open(self.kh, "a", encoding="utf-8") as f:
                f.writelines(lines)
            fps = _call(["ssh-keygen", "-l", "-f", q], 20).out.split("\n")
            self.on_event("host-key-trusted", address=ip, system=(seen or {}).get("os"), why=why,
                          fingerprints=[l.split()[1] for l in fps if len(l.split()) > 1])
            self._move(ip, f"a new system that proved it is the same computer ({why})")
            return True
        finally:
            os.remove(q)


def seed_follow(host, run_dir, machine, out=print):
    """At kickoff, over the owner's own trusted login: who the machine is,
    where it answers, and the keys the owner trusts for it, copied into the
    run. Returns True when the run can follow the machine."""
    plain = Ssh(host)
    who = plain.probe()
    if who is None:
        out("oneclick: the machine does not answer; the run will not follow it across addresses")
        return False
    conn = plain.raw("echo [%SSH_CONNECTION%] [$SSH_CONNECTION]", 25).out
    m = re.findall(r"\[(\S+) \d+ (\d+\.\d+\.\d+\.\d+) \d+\]", conn)
    if not m:
        out("oneclick: the machine did not say its address; the run will not follow it")
        return False
    ip = m[0][1]
    ident = read_identity(plain)
    if not ident:
        out("oneclick: the machine's identity did not read; the run will not follow it")
        return False
    g = _call(["ssh", "-G", host], 20).out
    opt = dict((l.split(" ", 1) + [""])[:2] for l in g.splitlines() if l)
    names = [opt.get("hostkeyalias") or opt.get("hostname") or host]
    files = [os.path.expanduser(f) for f in (opt.get("userknownhostsfile") or "~/.ssh/known_hosts").split()]
    owned = set()
    for f in files:
        if os.path.exists(f):
            for n in names:
                r = _call(["ssh-keygen", "-F", n, "-f", f], 20)
                owned |= {(p[1], p[2]) for p in (l.split() for l in r.out.splitlines() if l and not l.startswith("#")) if len(p) >= 3}
    now = keyscan(ip)
    keep = owned & now if now else owned
    if not keep:
        out("oneclick: none of the keys you trust for this machine is the one it shows now; the run will not follow it")
        return False
    alias = f"oneclick-{machine}"
    fd = os.open(os.path.join(run_dir, "known_hosts"), os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, "w", encoding="utf-8") as f:
        f.writelines(f"{alias} {t} {k}\n" for t, k in sorted(keep))
    state = {"alias": alias, "address": ip, "addresses": [ip], "prefix": ip.rsplit(".", 1)[0], "identity": ident, "read_from": who}
    with open(os.path.join(run_dir, "machine.json"), "w", encoding="utf-8") as f:
        json.dump(state, f, indent=2)
    out(f"oneclick: following the machine by its identity ({ident.get('model')}; {len(ident.get('macs') or [])} network address(es), "
        f"{len(ident.get('disks') or [])} drive serial(s)); it answers at {ip} now")
    return True


class HyperV:
    """The rig's guest. Windows: PowerShell Direct (rig/rig). Linux: SSH as
    'rig' with the rig key, which the job stand-in carries into Fedora
    (rig/hyperv/v1-job.py, ONECLICK_SSH_PUB). The guest's address comes from
    Hyper-V or, without the guest's integration daemon, from the host's
    neighbour table by the network card's MAC. WSL may not route to the
    Default Switch, so Windows' own ssh.exe is the fallback."""
    kind = "hyperv"
    WIN_SSH = "/mnt/c/Windows/System32/OpenSSH/ssh.exe"
    WIN_SCP = "/mnt/c/Windows/System32/OpenSSH/scp.exe"

    def __init__(self, vm, key=os.path.expanduser("~/.ssh/upgrade_rig"), user="rig"):
        self.vm, self.key, self.user = vm, key, user
        self._linux = None

    def describe(self):
        return f"hyperv:{self.vm}"

    def _host(self, script, timeout):
        return _call(["powershell.exe", "-NoProfile", "-NonInteractive", "-EncodedCommand", encoded("$ProgressPreference = 'SilentlyContinue'; " + script)], timeout, stdin="")

    def _session(self):
        return ("$sec = ConvertTo-SecureString 'rig' -AsPlainText -Force; "
                "$cred = New-Object System.Management.Automation.PSCredential ('rig', $sec); ")

    def guest_ip(self):
        r = self._host(
            f"$a = Get-VMNetworkAdapter -VMName '{self.vm}' | Select-Object -First 1; "
            "$ip = @($a.IPAddresses | Where-Object { $_ -match '^\\d+\\.\\d+\\.\\d+\\.\\d+$' })[0]; "
            "if (-not $ip) { $m = ($a.MacAddress -replace '(..)(?!$)', '$1-'); "
            "$ip = (Get-NetNeighbor -LinkLayerAddress $m -AddressFamily IPv4 -ErrorAction SilentlyContinue | Select-Object -First 1).IPAddress }; "
            "Write-Output $ip", 60)
        ip = r.out.strip().splitlines()[-1].strip() if r.ok and r.out.strip() else ""
        return ip or None

    def linux(self):
        """An Ssh link to the guest's Linux, or None when it does not answer."""
        ip = self.guest_ip()
        if not ip:
            return None
        # the rig's guest is reinstalled every run: its host key is never remembered
        common = ["-o", "StrictHostKeyChecking=no", "-o", "LogLevel=ERROR"]
        wsl = Ssh(f"{self.user}@{ip}", extra=["-i", self.key, *common, "-o", "UserKnownHostsFile=/dev/null"])
        if "Linux" in wsl.raw("uname -s", timeout=20).out:
            return wsl
        win = Ssh(f"{self.user}@{ip}", extra=["-i", self._windows_key(), *common, "-o", "UserKnownHostsFile=NUL"],
                  ssh=self.WIN_SSH, scp=self.WIN_SCP, windows_client=True)
        if "Linux" in win.raw("uname -s", timeout=20).out:
            return win
        return None

    def _windows_key(self):
        """The rig key where Windows' ssh.exe accepts it: in the user's
        profile, readable by the user alone."""
        prof = self._host("Write-Output $env:USERPROFILE", 30).out.strip().splitlines()[-1]
        dest_win = prof + "\\.ssh\\upgrade_rig_oneclick"
        dest = subprocess.run(["wslpath", "-u", dest_win], capture_output=True, text=True).stdout.strip()
        if not os.path.exists(dest):
            os.makedirs(os.path.dirname(dest), exist_ok=True)
            shutil.copyfile(self.key, dest)
            self._host(f"icacls '{dest_win}' /inheritance:r /grant:r \"$($env:USERNAME):R\" | Out-Null", 30)
        return dest_win

    def probe(self):
        r = self.run("$env:OS", "windows", timeout=60)
        if r.ok and "Windows_NT" in r.out:
            self._linux = None
            return "windows"
        self._linux = self.linux()
        return "linux" if self._linux else None

    def _lx(self):
        if self._linux is None:
            self._linux = self.linux()
        return self._linux

    def run(self, script, os_kind, timeout=600):
        if os_kind == "linux":
            lx = self._lx()
            return lx.run(script, "linux", timeout) if lx else Result(None, "", "the guest's Linux does not answer")
        if len(encoded(script)) > LONG:
            return long_script_wrapper(self, script, timeout)
        inner = encoded(script)
        outer = (self._session() +
                 f"Invoke-Command -VMName '{self.vm}' -Credential $cred -ErrorAction Stop -ScriptBlock {{ "
                 f"param($e) $s = [Text.Encoding]::Unicode.GetString([Convert]::FromBase64String($e)); "
                 f"& ([scriptblock]::Create($s)) }} -ArgumentList '{inner}'")
        return self._host(outer, timeout)

    def pull(self, remote, local, os_kind, timeout=1800):
        """Linux: scp. Windows: Copy-Item -FromSession to a host temp folder, then into local."""
        if os_kind == "linux":
            lx = self._lx()
            return lx.pull(remote, local, "linux", timeout) if lx else Result(None, "", "the guest's Linux does not answer")
        # Copy-Item -FromSession fails on the stick's FAT32 volume ("The parameter is incorrect",
        # 2026-10-27): stage the copy on the guest's own NTFS drive first
        stage = f"C:\\upgrade_oneclick\\out\\{uuid.uuid4().hex}"
        st = self.run(f"New-Item -ItemType Directory -Force '{stage}' | Out-Null; Copy-Item -Path '{remote}' -Destination '{stage}' -Recurse -ErrorAction Stop; 'STAGED'",
                      "windows", timeout)
        if "STAGED" not in st.out:
            return Result(1, "", f"staging {remote} in the guest: {st.err.strip()[-300:]}")
        remote = stage + "\\*"
        tmp = f"C:\\upgrade-rig\\oneclick-pull\\{uuid.uuid4().hex}"
        script = (self._session() +
                  f"New-Item -ItemType Directory -Force '{tmp}' | Out-Null; "
                  f"$s = New-PSSession -VMName '{self.vm}' -Credential $cred -ErrorAction Stop; "
                  f"try {{ Copy-Item -FromSession $s -Path '{remote}' -Destination '{tmp}' -Recurse -ErrorAction Stop }} "
                  f"finally {{ Remove-PSSession $s }}")
        r = self._host(script, timeout)
        self.run(f"Remove-Item '{stage}' -Recurse -Force -ErrorAction SilentlyContinue", "windows", 120)
        if not r.ok:
            return r
        src = "/mnt/c/" + tmp[3:].replace("\\", "/")
        name = os.listdir(src)[0] if os.path.isdir(src) and os.listdir(src) else None
        if name is None:
            return Result(1, "", f"nothing arrived from {remote}")
        os.makedirs(os.path.dirname(os.path.abspath(local)), exist_ok=True)
        shutil.move(os.path.join(src, name), local)
        shutil.rmtree(src, ignore_errors=True)
        return Result(0, "", "")

    def push(self, local, remote, os_kind, timeout=600):
        if os_kind == "linux":
            lx = self._lx()
            return lx.push(local, remote, "linux", timeout) if lx else Result(None, "", "the guest's Linux does not answer")
        src = subprocess.run(["wslpath", "-w", os.path.abspath(local)], capture_output=True, text=True).stdout.strip()
        return self._host(f"Copy-VMFile -Name '{self.vm}' -SourcePath '{src}' -DestinationPath '{remote}' "
                          "-FileSource Host -CreateFullPath -Force", timeout)


def from_spec(spec, run_dir=None):
    """'ssh:aspire' or 'hyperv:UPGRIGHV'. Inside a run whose kickoff read the
    machine's identity (machine.json), ssh follows the machine (Follow)."""
    kind, _, target = spec.partition(":")
    if kind == "ssh" and target:
        if run_dir and os.path.exists(os.path.join(run_dir, "machine.json")):
            return Follow(target, run_dir)
        return Ssh(target)
    if kind == "hyperv" and target:
        return HyperV(target)
    raise ValueError(f"unknown transport '{spec}' (want ssh:<host> or hyperv:<vm>)")
