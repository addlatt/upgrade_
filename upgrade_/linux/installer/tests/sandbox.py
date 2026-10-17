#!/usr/bin/env python3
"""The faked machine the installer's verify and outcome steps run in, for
their golden and for the Rust's parity test (CLAUDE.md rule #5, the logic
level, and RUST-PORT.md "How one piece is ported").

The two steps run inside Anaconda's stage 2 and read the kernel's own
paths: /proc/cmdline, /sys/class/drm, /sys/block, /sys/firmware/efi,
/dev/disk/by-id, the stick at /run/install/repo, the installed system at
/mnt/sysroot, the console /dev/tty9 (tty6 before upgrade-installer 0.1.2,
R38), /proc's list of processes, and tools (lsblk, blockdev, mount,
nmcli, efibootmgr, findmnt, chroot, chvt, systemctl ...). This driver
builds a world from a case (tests/cases.json), then runs one command in a
user and process namespace (unshare -Urm, and -pf --mount-proc so the
command sees only the world's processes) where every one of those paths is a bind
mount of the world's copy, the tools are fakes answering from the world
(tests/faketool.py), and /dev/tty9 is a pseudo-terminal this driver holds
the other end of (it records what was drawn and presses the case's keys).
A case's "holder" starts another program holding the console open, at
once or when the countdown starts (R38).
Nothing on this computer is read or changed.

    sandbox.py <cases.json> <case name> <command...> --out <dir>
        <command> is the thing under test; the words JOB, LABEL stand for
        the stick's job.json path and label. Writes <dir>/result.json: the
        command's output and exit code, every file it left on the stick,
        in the installed system and under /tmp, the tools it called, the
        screens it drew, and the firmware's entries after it, all with
        times and read speeds normalised.

The recorder (tests/record.sh) runs the scripts this way once; the Rust
test (tests/parity.rs) runs the program the same way and compares.
"""
import json, os, pty, re, select, shutil, struct, subprocess, sys, termios, fcntl, threading, time, hashlib, stat

HERE = os.path.dirname(os.path.abspath(__file__))
SECURE_BOOT_VAR = "SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c"
ESP_TYPE = "c12a7328-f81f-11d2-ba4b-00a0c93ec93b"
MSDATA_TYPE = "ebd0a0a2-b9e5-4433-87c0-68b6b72699c7"


def sha(data):
    return hashlib.sha256(data).hexdigest()


def write(path, data, mode=None):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    if isinstance(data, str):
        data = data.encode()
    with open(path, "wb") as f:
        f.write(data)
    if mode is not None:
        os.chmod(path, mode)


def build_world(case, world):
    """Everything the command may read, laid out as the kernel would show it."""
    shutil.rmtree(world, ignore_errors=True)
    os.makedirs(world)
    # the stick
    stick = os.path.join(world, "stick")
    files = {"upgrade_/job.json": json.dumps(case["job"], indent=2) + "\n"}
    files.update(case.get("stick", {}).get("files", {}))
    for rel, content in files.items():
        write(os.path.join(stick, rel), content)
    sums = case.get("stick", {}).get("sums")
    if sums is not False:
        lines = []
        for rel in sorted(files):
            if rel in case.get("stick", {}).get("not_in_sums", []) or rel.endswith("job.json"):
                continue
            data = files[rel].encode() if isinstance(files[rel], str) else files[rel]
            if rel in case.get("stick", {}).get("wrong_sum_for", []):
                data = b"not these bytes"
            lines.append(f"{sha(data)}  ./{rel}")
        write(os.path.join(stick, "SHA256SUMS"), "\n".join(lines) + "\n")
    for m in case.get("stick", {}).get("markers", []):
        name, _, text = m.partition(":")
        write(os.path.join(stick, "upgrade_", name), text + ("\n" if text else ""))
    for d in ("upgrade_/report",):
        os.makedirs(os.path.join(stick, d), exist_ok=True)
    for rel, content in case.get("stick", {}).get("report", {}).items():
        write(os.path.join(stick, "upgrade_/report", rel), json.dumps(content, indent=2) if isinstance(content, (dict, list)) else content)
    snap = case.get("stick", {}).get("esp_snapshot")
    if snap:
        for rel, content in snap.items():
            write(os.path.join(stick, "upgrade_/esp-snapshot", rel), content)
    for rel, content in case.get("stick", {}).get("settle_in", {}).items():
        write(os.path.join(stick, "upgrade_/settle-in", rel), content, 0o755)
    if case.get("stick", {}).get("settle_in"):
        # the kit's manifest names them
        with open(os.path.join(stick, "SHA256SUMS"), "a") as f:
            for rel, content in case["stick"]["settle_in"].items():
                data = content.encode() if isinstance(content, str) else content
                if rel in case["stick"].get("wrong_sum_for", []):
                    data = b"not these bytes"
                f.write(f"{sha(data)}  ./upgrade_/settle-in/{rel}\n")
    # the installed system
    sysroot = os.path.join(world, "sysroot")
    os.makedirs(sysroot)
    for rel, content in case.get("sysroot", {}).get("files", {}).items():
        write(os.path.join(sysroot, rel), content)
    for link, target in case.get("sysroot", {}).get("links", {}).items():
        p = os.path.join(sysroot, link)
        os.makedirs(os.path.dirname(p), exist_ok=True)
        os.symlink(target, p)
    # the kernel's views
    write(os.path.join(world, "proc-cmdline"), case.get("cmdline", "BOOT_IMAGE=/images/pxeboot/vmlinuz inst.ks=hd:LABEL=UPGV0:/ks.cfg") + "\n")
    drm = os.path.join(world, "sys/class/drm")
    os.makedirs(drm)
    for c in case.get("drm", []):
        write(os.path.join(drm, c["name"], "status"), c["status"] + "\n")
        write(os.path.join(drm, c["name"], "modes"), "".join(m + "\n" for m in c.get("modes", [])))
    gfx = os.path.join(world, "sys/class/graphics")
    os.makedirs(gfx)
    for g in case.get("graphics", []):
        write(os.path.join(gfx, g, "virtual_size"), case.get("fb_size", "1920,1080") + "\n")
    blk = os.path.join(world, "sys/block")
    os.makedirs(blk)
    byid = os.path.join(world, "dev/disk/by-id")
    os.makedirs(byid)
    for d in case.get("disks", []):
        os.makedirs(os.path.join(blk, d["dev"]))
        for p in d.get("parts", []):
            pd = os.path.join(blk, d["dev"], f"{d['dev']}{p['n']}")
            os.makedirs(pd)
            write(os.path.join(pd, "partition"), f"{p['n']}\n")
        for name in d.get("byid", []):
            os.symlink(f"../../{d['dev']}", os.path.join(byid, name))
            for p in d.get("parts", []):
                os.symlink(f"../../{d['dev']}{p['n']}", os.path.join(byid, f"{name}-part{p['n']}"))
        for p in d.get("parts", []):
            for rel, content in p.get("esp", {}).items():
                write(os.path.join(world, "esp", f"{d['dev']}{p['n']}", rel), content)
    fw = os.path.join(world, "sys/firmware")
    os.makedirs(os.path.join(fw, "efi/efivars"))
    if case.get("secureboot") is not None:
        write(os.path.join(fw, "efi/efivars", SECURE_BOOT_VAR), bytes([7, 0, 0, 0, int(case["secureboot"])]))
    rtc = os.path.join(world, "sys/class/rtc/rtc0")
    os.makedirs(rtc)
    if case.get("clock", {}).get("rtc"):
        write(os.path.join(rtc, "since_epoch"), str(case["clock"]["rtc"]) + "\n")
    # what the tools answer, and the firmware's entries (mutable)
    write(os.path.join(world, "world.json"), json.dumps(case, indent=1))
    write(os.path.join(world, "efi-state.json"), json.dumps(case.get("efibootmgr", {"entries": [], "order": [], "current": ""}), indent=1))
    write(os.path.join(world, "calls.log"), "")
    write(os.path.join(world, "no-tty"), "")
    # the fake tools
    bindir = os.path.join(world, "bin")
    os.makedirs(bindir)
    tools = ["blockdev", "lsblk", "mount", "umount", "nmcli", "lspci", "dmesg", "uname", "efibootmgr", "findmnt", "chroot", "restorecon", "timedatectl", "chronyc", "chvt", "systemctl", "reboot", "sleep", "sync"]
    for t in tools:
        write(os.path.join(bindir, t), f"#!/bin/sh\nexec python3 {HERE}/faketool.py {t} \"$@\"\n", 0o755)
    return world


def inner_script(world, cmd, out, tty_path, holder=None):
    binds = [
        (f"{world}/proc-cmdline", "/proc/cmdline"),
        (f"{world}/sys/class/drm", "/sys/class/drm"),
        (f"{world}/sys/class/graphics", "/sys/class/graphics"),
        (f"{world}/sys/block", "/sys/block"),
        (f"{world}/sys/firmware", "/sys/firmware"),
        (f"{world}/sys/class/rtc", "/sys/class/rtc"),
        (f"{world}/dev/disk/by-id", "/dev/disk/by-id"),
    ]
    lines = ["set -e", "mount -t tmpfs none /tmp", "mount -t tmpfs none /run", "mkdir -p /run/install/repo", f"mount --bind {world}/stick /run/install/repo",
             "mount -t tmpfs none /mnt", "mkdir -p /mnt/sysroot", f"mount --bind {world}/sysroot /mnt/sysroot"]
    for src, dst in binds:
        lines.append(f"[ -e {dst} ] && mount --bind {src} {dst} || true")
    if tty_path:
        lines.append(f"mount --bind {tty_path} /dev/tty9")
    else:
        # no console: a plain file where the character device would be
        lines.append(f"mount --bind {world}/no-tty /dev/tty9")
    # another program reading the console (R38): tail opens it itself, so it is
    # named tail whenever it holds it
    if holder == "before":
        lines.append("/usr/bin/tail -f /dev/tty9 > /dev/null 2>&1 &")
    elif holder == "countdown":
        lines.append("( until /bin/grep -q '== countdown: ' /tmp/upgrade_-verify.log 2>/dev/null; do /bin/sleep 0.1; done; /bin/sleep 0.3; exec /usr/bin/tail -f /dev/tty9 > /dev/null 2>&1 ) &")
    lines += [f"export PATH={world}/bin:$PATH WORLD={world}", "cd /", "set +e",
              " ".join(cmd) + f" > {out}/stdout.txt 2>&1", f"echo $? > {out}/exit.txt", "set -e",
              f"mkdir -p {out}/tmp && cp -a /tmp/. {out}/tmp/ 2>/dev/null || true"]
    return "\n".join(lines) + "\n"


TIME = re.compile(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?Z?")
RULES = [
    (TIME, "<time>"),
    (re.compile(r"read at [0-9.]+ MB/s"), "read at <mbps> MB/s"),
    (re.compile(r'"read_mbps": "[0-9.]+"'), '"read_mbps": "<mbps>"'),
    (re.compile(r"after [0-9.]+ s"), "after <secs> s"),
    (re.compile(r"installer=\d+"), "installer=<epoch>"),
    (re.compile(r'"installer_utc_epoch": \d+'), '"installer_utc_epoch": <epoch>'),
    (re.compile(r"/dev/pts/\d+"), "/dev/pts/N"),
    (re.compile(r"\(pid \d+\)"), "(pid N)"),
    # the program names itself after the script's header
    (re.compile(r"(== upgrade_ (?:verify|outcome)\.sh \d+\.\d+\.\d+) \(upgrade-installer [^)]*\)"), r"\1"),
    # `ls -l`: a link's date and time (or year) column
    (re.compile(r"(lrwxrwxrwx \d+ \S+ \S+ +\d+) [A-Z][a-z]{2} +\d+ +(?:\d\d:\d\d|\d{4})"), r"\1 <date>"),
]


def norm(text):
    for rx, rep in RULES:
        text = rx.sub(rep, text)
    return text


# outcome.sh's grep -c on a grub.cfg with no Windows entry prints the count
# AND exits 1, so `|| echo 0` prints a second 0, bash complains about the
# comparison, and the count echoes twice: the script's own noise, dropped
# here on both sides (RUST-PORT.md, "Differences kept on purpose").
def drop_noise(lines):
    out = []
    skip_zero = False
    for l in lines:
        if "integer expression expected" in l or (": line " in l and ": [: " in l):
            continue
        # the program's own diagnostics of the console (2026-10-14), lines the script never wrote
        if l.startswith("== countdown key read: ") or l.startswith("== countdown ignored the console") or " again " in l and "time(s): it had been hung up" in l:
            continue
        if skip_zero and l == "0":
            skip_zero = False
            continue
        skip_zero = l == "== grub.cfg windows entries: 0"
        out.append(l)
    return out


def norm_value(v):
    if isinstance(v, str):
        return norm(v)
    if isinstance(v, list):
        return [norm_value(x) for x in v]
    if isinstance(v, dict):
        return {k: ("<mbps>" if k == "read_mbps" and x else "<epoch>" if k == "installer_utc_epoch" else norm_value(x)) for k, x in v.items() if k != "verify_program"}
    return v


def tree(root, skip=()):
    out = {}
    for dirpath, dirs, files in os.walk(root):
        dirs.sort()
        for f in sorted(files):
            p = os.path.join(dirpath, f)
            rel = os.path.relpath(p, root)
            if any(rel == s or rel.startswith(s + "/") for s in skip):
                continue
            st = os.lstat(p)
            if stat.S_ISLNK(st.st_mode):
                out[rel] = {"link": os.readlink(p)}
                continue
            data = open(p, "rb").read()
            entry = {"mode": oct(st.st_mode & 0o777)}
            if rel.endswith(".json"):
                try:
                    entry["json"] = norm_value(json.loads(data.decode()))
                except Exception:
                    entry["text"] = norm(data.decode(errors="replace"))
            elif len(data) > 4000 and not rel.endswith((".log", ".txt", ".ks", ".cfg", ".sha256", "SHA256SUMS")):
                entry["sha256"] = sha(data)
            elif rel.endswith(".log"):
                entry["text"] = "\n".join(drop_noise(norm(data.decode(errors="replace")).splitlines()))
            else:
                entry["text"] = norm(data.decode(errors="replace"))
            out[rel] = entry
    return out


def screens(raw):
    """What was drawn on the console: each clear-and-redraw as one screen,
    repeats dropped, the countdown's seconds masked."""
    text = raw.decode(errors="replace")
    parts = [p for p in re.split(r"\x1b\[2J\x1b\[H", text) if p.strip()]
    out = []
    for p in parts:
        s = re.sub(r"IN \d+:\d\d", "IN M:SS", p.replace("\r", ""))
        s = re.sub(r"restarts in \d+ seconds", "restarts in N seconds", s)
        if not out or out[-1] != s:
            out.append(s)
    return out


def run(case, cmd, out):
    out = os.path.abspath(out)
    if out.startswith("/tmp/"):
        raise SystemExit("sandbox: the output folder cannot be under /tmp (the run gets a private /tmp)")
    world = build_world(case, os.path.join(out, "world"))
    os.makedirs(out, exist_ok=True)
    tty_path = None
    master = None
    if case.get("tty", True):
        master, slave = pty.openpty()
        tty_path = os.ttyname(slave)
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", case.get("rows", 25), case.get("cols", 80), 0, 0))
    job = "/run/install/repo/upgrade_/job.json"
    cmd = [job if w == "JOB" else case.get("label", "UPGV0") if w == "LABEL" else w for w in cmd]
    inner = os.path.join(out, "inner.sh")
    write(inner, inner_script(world, cmd, out, tty_path, case.get("holder")))
    env = dict(os.environ, UPG_TEST_COUNTDOWN=str(case.get("countdown_secs", 2)), UPG_TEST_REFUSAL_SECS=str(case.get("refusal_secs", 1)), LC_ALL="C", LANG="C")
    # the world's own path (a tool's location in a message, say) reads the same wherever the world was built
    RULES.insert(0, (re.compile(re.escape(world)), "<world>"))
    captured = bytearray()
    stop = threading.Event()

    def reader():
        while not stop.is_set():
            r, _, _ = select.select([master], [], [], 0.1)
            if r:
                try:
                    data = os.read(master, 4096)
                except OSError:
                    break
                if not data:
                    break
                captured.extend(data)

    t = None
    if master is not None:
        t = threading.Thread(target=reader, daemon=True)
        t.start()
    t0 = time.monotonic()
    proc = subprocess.Popen(["unshare", "-Urmpf", "--mount-proc", "bash", inner], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    keys = sorted(case.get("keys", []), key=lambda k: k["after"])
    while proc.poll() is None:
        now = time.monotonic() - t0
        while keys and keys[0]["after"] <= now:
            if master is not None:
                os.write(master, keys.pop(0)["data"].encode())
            else:
                keys.pop(0)
        time.sleep(0.05)
        if now > case.get("timeout", 120):
            proc.kill()
            break
    err = proc.stderr.read().decode(errors="replace")
    stop.set()
    if t:
        t.join(timeout=1)
    if master is not None:
        time.sleep(0.1)
        try:
            while select.select([master], [], [], 0)[0]:
                captured.extend(os.read(master, 4096))
        except OSError:
            pass
        os.close(master)
        os.close(slave)
    exit_code = int(open(os.path.join(out, "exit.txt")).read().strip()) if os.path.exists(os.path.join(out, "exit.txt")) else None
    stdout = open(os.path.join(out, "stdout.txt"), errors="replace").read() if os.path.exists(os.path.join(out, "stdout.txt")) else ""
    result = {
        "exit": exit_code,
        "stdout": drop_noise(norm(stdout).splitlines()),
        "stick": tree(os.path.join(world, "stick"), skip=("upgrade_/LiveOS", "upgrade_/settle-in", "upgrade_/job.json")),
        "sysroot": tree(os.path.join(world, "sysroot")),
        "tmp": tree(os.path.join(out, "tmp")),
        "calls": norm(open(os.path.join(world, "calls.log")).read()).splitlines(),
        "screens": screens(bytes(captured)),
        "efi": json.load(open(os.path.join(world, "efi-state.json"))),
        "sandbox_stderr": norm(err).splitlines()[-5:],
    }
    write(os.path.join(out, "result.json"), json.dumps(result, indent=1, ensure_ascii=False) + "\n")
    return result


def main():
    args = sys.argv[1:]
    out = args[args.index("--out") + 1]
    cmd = args[2:args.index("--out")]
    cases = json.load(open(args[0]))
    case = next(c for c in cases if c["name"] == args[1])
    r = run(case, cmd, out)
    print(f"sandbox: exit {r['exit']}, {len(r['stdout'])} lines, {len(r['calls'])} tool calls, {len(r['screens'])} screens -> {out}/result.json")


if __name__ == "__main__":
    main()
