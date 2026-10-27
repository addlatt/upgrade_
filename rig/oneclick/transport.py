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

    def __init__(self, host):
        self.host = host
        self.opts = ["-o", "BatchMode=yes", "-o", "ConnectTimeout=10",
                     "-o", "ServerAliveInterval=15", "-o", "ServerAliveCountMax=4"]

    def describe(self):
        return f"ssh:{self.host}"

    def raw(self, remote, timeout=60):
        return _call(["ssh", *self.opts, self.host, remote], timeout, stdin="")

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
        return _call(["scp", "-r", "-q", *self.opts, f"{self.host}:{remote}", local], timeout, stdin="")


class HyperV:
    kind = "hyperv"

    def __init__(self, vm):
        self.vm = vm

    def describe(self):
        return f"hyperv:{self.vm}"

    def _host(self, script, timeout):
        return _call(["powershell.exe", "-NoProfile", "-NonInteractive", "-EncodedCommand", encoded(script)], timeout, stdin="")

    def _session(self):
        return ("$sec = ConvertTo-SecureString 'rig' -AsPlainText -Force; "
                "$cred = New-Object System.Management.Automation.PSCredential ('rig', $sec); ")

    def probe(self):
        r = self.run("$env:OS", "windows", timeout=60)
        return "windows" if r.ok and "Windows_NT" in r.out else None

    def run(self, script, os_kind, timeout=600):
        if os_kind != "windows":
            return Result(None, "", "the rig transport has no Linux channel")
        inner = encoded(script)
        outer = (self._session() +
                 f"Invoke-Command -VMName '{self.vm}' -Credential $cred -ErrorAction Stop -ScriptBlock {{ "
                 f"param($e) $s = [Text.Encoding]::Unicode.GetString([Convert]::FromBase64String($e)); "
                 f"& ([scriptblock]::Create($s)) }} -ArgumentList '{inner}'")
        return self._host(outer, timeout)

    def pull(self, remote, local, os_kind, timeout=1800):
        """Copy-Item -FromSession to a host temp folder, then into local."""
        if os_kind != "windows":
            return Result(None, "", "the rig transport has no Linux channel")
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


def from_spec(spec):
    """'ssh:aspire' or 'hyperv:UPGRIGHV'."""
    kind, _, target = spec.partition(":")
    if kind == "ssh" and target:
        return Ssh(target)
    if kind == "hyperv" and target:
        return HyperV(target)
    raise ValueError(f"unknown transport '{spec}' (want ssh:<host> or hyperv:<vm>)")
