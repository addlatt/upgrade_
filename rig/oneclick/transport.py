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
import os
import shutil
import subprocess
import uuid


def encoded(script):
    """PowerShell's -EncodedCommand form: base64 of UTF-16LE."""
    return base64.b64encode(script.encode("utf-16-le")).decode("ascii")


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

    def raw(self, remote, timeout=60):
        return _call([self.ssh, *self.opts, self.host, remote], timeout, stdin="")

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

    def run(self, script, os_kind, timeout=600):
        if os_kind == "windows":
            return self.raw(f"powershell -NoProfile -NonInteractive -EncodedCommand {encoded(script)}", timeout)
        return self.raw(script, timeout)

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
        return _call(["powershell.exe", "-NoProfile", "-NonInteractive", "-EncodedCommand", encoded(script)], timeout, stdin="")

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
        common = ["-o", "StrictHostKeyChecking=no", "-o", "UserKnownHostsFile=/dev/null", "-o", "LogLevel=ERROR"]
        wsl = Ssh(f"{self.user}@{ip}", extra=["-i", self.key, *common])
        if "Linux" in wsl.raw("uname -s", timeout=20).out:
            return wsl
        wkey = self._windows_key()
        win = Ssh(f"{self.user}@{ip}", extra=["-i", wkey, *common, "-o", "UserKnownHostsFile=NUL"],
                  ssh=self.WIN_SSH, scp=self.WIN_SCP, windows_client=True)
        win.opts = [o for o in win.opts if o != "UserKnownHostsFile=/dev/null"]
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
        tmp = f"C:\\upgrade-rig\\oneclick-pull\\{uuid.uuid4().hex}"
        script = (self._session() +
                  f"New-Item -ItemType Directory -Force '{tmp}' | Out-Null; "
                  f"$s = New-PSSession -VMName '{self.vm}' -Credential $cred -ErrorAction Stop; "
                  f"try {{ Copy-Item -FromSession $s -Path '{remote}' -Destination '{tmp}' -Recurse -ErrorAction Stop }} "
                  f"finally {{ Remove-PSSession $s }}")
        r = self._host(script, timeout)
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


def from_spec(spec):
    """'ssh:aspire' or 'hyperv:UPGRIGHV'."""
    kind, _, target = spec.partition(":")
    if kind == "ssh" and target:
        return Ssh(target)
    if kind == "hyperv" and target:
        return HyperV(target)
    raise ValueError(f"unknown transport '{spec}' (want ssh:<host> or hyperv:<vm>)")
