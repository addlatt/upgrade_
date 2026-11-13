#!/usr/bin/env python3
"""
ubuntu-verdict.py - judge one Ubuntu arm from its own evidence and append its
row to docs/validation-results/ubuntu-install.csv (RISKS R40, R45).

    ubuntu-verdict.py ARM DIR CSV [NOTE]

DIR is what ubuntu-arms.sh gathered: the sha256 of each disk's whole content
before and after (read on the host with the guest off), the stick's upgrade_ folder after the run,
the log disk, the first MiB of the system disk, the job. A rig row closes
plumbing only (rule #5). Every reason for a fail goes in notes.
"""
import csv, datetime, hashlib, json, pathlib, struct, sys, tarfile

arm, d, out = sys.argv[1], pathlib.Path(sys.argv[2]), pathlib.Path(sys.argv[3])
note = sys.argv[4] if len(sys.argv) > 4 else ""
letter = arm[0]
COLS = ["timestamp", "arm", "firmware", "installer_version", "generator_version", "secure_boot", "network", "identity",
        "refusal_code", "refusals", "converted_marker", "armed_marker", "commit_record", "system_disk_unchanged",
        "decoy_disk_unchanged", "system_gpt_after", "ubuntu_booted", "password_matches", "root_on_job_disk",
        "graphical_login", "result", "notes"]

def text(p):
    p = d / p
    return p.read_text(errors="replace") if p.is_file() else ""
def js(p):
    try: return json.loads(text(p))
    except ValueError: return {}
def hashes(p):
    return dict(l.split()[:2] for l in text(p).splitlines() if len(l.split()) >= 2)

R = "stick/upgrade_/report/"
why, notes = [], []
def need(ok, reason):
    if not ok: why.append(reason)
    return ok

before, after = hashes("hashes-before.txt"), hashes("hashes-after.txt")
def same(n):
    return "y" if before.get(n) and before.get(n) == after.get(n) else "n" if before.get(n) and after.get(n) else "not-read"
sys_same, decoy_same = same("target"), same("decoy")

early_log = text(R + "early.log")
first = text("log/firstboot.txt")
def after_mark(s, mark):
    for l in s.splitlines():
        if mark in l: return l.split(mark, 1)[1].strip()
    return ""
head = after_mark(early_log, "== upgrade_ early check (")
installer_v = head.split("upgrade-installer ")[1].split(",")[0] if "upgrade-installer " in head else "not-run"
gen_v = head.split("upgrade-autoinstall ")[1].split(")")[0] if "upgrade-autoinstall " in head else "not-run"
firmware = after_mark(early_log, "== firmware: ") or after_mark(first, "firmware=") or "not-read"
sb_line = (after_mark(early_log, "== secure boot: ") or after_mark(first, "sb=")).lower()
guest = text("guest.txt")
# the guest's own word where our code ran; the host's setting where nothing of ours did
secure_boot = "on" if "enabled" in sb_line else "off" if "disabled" in sb_line else ("on (host setting)" if "secure_boot=On" in guest else "not-read")
network = "none" if "switch=[]" in guest else "connected" if "switch=[" in guest else "not-read"

refusal = js(R + "refusal.json")
ref_lines = [l for l in text(R + "refusals.log").splitlines() if l.strip()]
code = str(refusal.get("exit_code", "none"))
converted = "y" if (d / "stick/upgrade_/converted").is_file() else "n"
armed = "y" if (d / "stick/upgrade_/boot-install").is_file() else "n"
commit = "y" if (d / (R + "commit.json")).is_file() else "n"
identity = "pass" if "== decision: Install" in early_log else "fail" if "IDENTITY MISMATCH" in early_log else "not-reached" if early_log else "not-run"

def gpt(p):
    """The partition table in the disk's first MiB: type name and size."""
    b = (d / p).read_bytes() if (d / p).is_file() else b""
    if len(b) < 1024 or b[512:520] != b"EFI PART": return "none"
    names = {"c12a7328-f81f-11d2-ba4b-00a0c93ec93b": "EFI_System", "0fc63daf-8483-4772-8e79-3d69d8477de4": "Linux_filesystem",
             "e3c9e316-0b5c-4db8-817d-f92df00215ae": "Microsoft_reserved", "ebd0a0a2-b9e5-4433-87c0-68b6b72699c7": "Microsoft_basic_data"}
    lba, n, sz = struct.unpack_from("<Q", b, 512 + 72)[0], struct.unpack_from("<I", b, 512 + 80)[0], struct.unpack_from("<I", b, 512 + 84)[0]
    parts = []
    for i in range(n):
        e = b[lba * 512 + i * sz: lba * 512 + (i + 1) * sz]
        if len(e) < 48 or e[:16] == bytes(16): continue
        a = struct.unpack_from("<IHH", e, 0); t = "%08x-%04x-%04x-%s-%s" % (a[0], a[1], a[2], e[8:10].hex(), e[10:16].hex())
        s, l = struct.unpack_from("<QQ", e, 32)
        parts.append("%s:%dMiB" % (names.get(t, t), (l - s + 1) * 512 // 1048576))
    return "+".join(parts) or "empty"
gpt_after = gpt("target-first.bin")

def outcome_checks(keep):
    """outcome.json on the stick: there, valid against the contract, and saying what happened."""
    o = js("stick/upgrade_/outcome.json")
    if not need(bool(o), "no outcome.json on the stick"): return
    try:
        from jsonschema import Draft7Validator, FormatChecker
        schema = json.load(open(pathlib.Path(__file__).resolve().parents[2] / "schemas/outcome.schema.json"))
        errs = ["/".join(map(str, e.path)) + " " + e.message[:80] for e in Draft7Validator(schema, format_checker=FormatChecker()).iter_errors(o)]
    except ImportError:
        errs = ["jsonschema is not installed: outcome.json not checked"]
    need(not errs, "outcome.json does not pass the contract: " + "; ".join(errs[:3]))
    need(o.get("status") == "completed" and o.get("job_id") == job.get("job_id"), "outcome.json: status %s, job %s" % (o.get("status"), o.get("job_id")))
    need(o.get("path_taken") == ("keep-windows" if keep else "clean-slate"), "outcome.json path_taken is %s" % o.get("path_taken"))
    inst, bc = o.get("cutover", {}).get("install", {}), o.get("cutover", {}).get("boot_chain", {})
    need(inst.get("distro") == "ubuntu" and inst.get("release") == "26.04", "outcome.json install: %s %s" % (inst.get("distro"), inst.get("release")))
    need(bc.get("linux_first_in_bootorder") is True, "outcome.json: Linux is not first in the firmware's order")
    if keep:
        need(bc.get("bootmgfw_matches_snapshot") is True and bc.get("grub_lists_windows") is True and bc.get("windows_entry_present") is True,
             "outcome.json boot_chain: %s" % json.dumps(bc))
        need(o.get("windows", {}).get("kept") is True and bool(o.get("windows", {}).get("partition")), "outcome.json does not name the kept Windows partition")
    else:
        need(o.get("commit_line", {}).get("crossed") is True and o.get("cutover", {}).get("hardware", {}).get("human_gate") == "typed-in-the-window", "outcome.json: the commit line is not recorded as crossed in the window")
    notes.append("outcome.json valid: %s %s, kernel %s, fallback loader %s, settle-in installed=%s" % (inst.get("distro"), inst.get("release"), inst.get("kernel"), bc.get("fallback_loader"), o.get("cutover", {}).get("settle_in", {}).get("installed")))

booted = pw = root_ok = graphical = "n/a"
result = "fail"
job = js("job.json")

if letter == "A":
    need(code == "20", "refusal.json exit_code is %s, not 20" % code)
    need(len(ref_lines) >= 2 and all(" exit 20:" in l for l in ref_lines), "refusals.log has %d lines; two refusals show the machine restarted itself" % len(ref_lines))
    need(converted == "n" and armed == "y" and commit == "n", "markers: converted=%s boot-install=%s commit=%s" % (converted, armed, commit))
    need(sys_same == "y" and decoy_same == "y", "a disk changed: system unchanged=%s decoy unchanged=%s" % (sys_same, decoy_same))
    if not why: result = "refused-wrong-machine"
    notes.append("%d refusals on the stick, each after a restart the check made itself" % len(ref_lines))
elif letter == "F":
    need(not early_log and code == "none", "our check ran on a stick that had it taken out")
    need(converted == "n" and commit == "n", "markers: converted=%s commit=%s" % (converted, commit))
    need(sys_same == "y" and decoy_same == "y", "a disk changed: system unchanged=%s decoy unchanged=%s" % (sys_same, decoy_same))
    tgz, said = d / "log/fence-installer.tgz", ""
    if need(tgz.is_file(), "no installer log on the log disk: no evidence the installer ran and stopped"):
        with tarfile.open(tgz) as t:
            for m in t.getmembers():
                if m.isfile() and ("subiquity-server-debug" in m.name or "curtin" in m.name or m.name.endswith("traceback.txt")):
                    for l in t.extractfile(m).read().decode(errors="replace").splitlines():
                        if "UPGRADE-NO-DISK-CHOSEN" in l or "Failed to find matching" in l or "matching device" in l.lower():
                            said = l.strip()[:200]
        need(bool(said), "the installer's log does not show the disk match failing")
    if not why: result = "nothing-installed-without-the-check"
    notes.append("the installer's own log: " + said if said else "no match line found")
    notes.append("the guest was %s when the harness turned it off" % (text("end-state.txt").strip() or "?"))
elif letter == "C":
    late = js(R + "late.json")
    need(identity == "pass", "the early check did not decide to install")
    need(converted == "y" and armed == "n" and commit == "y", "markers: converted=%s boot-install=%s commit=%s" % (converted, armed, commit))
    need(bool(late), "no late.json: the late hook did not run")
    need(decoy_same == "y", "the decoy disk changed (unchanged=%s)" % decoy_same)
    need(sys_same == "n", "the system disk did not change")
    mib = int(job.get("identity", {}).get("system_disk", {}).get("size_bytes", 0)) // 1048576
    parts = gpt_after.split("+")
    need(len(parts) == 3 and parts[0] == "EFI_System:600MiB" and parts[1] == "Linux_filesystem:1024MiB" and parts[2].startswith("Linux_filesystem:")
         and mib - 1630 <= int(parts[2].split(":")[1][:-3] or 0) <= mib - 1624, "the system disk is not EFI 600 MiB + /boot 1 GiB + the rest: " + gpt_after)
    booted = "y" if "== firstboot" in first else "n"
    need(booted == "y", "no first-start record on the log disk")
    want = hashlib.sha256(job.get("intent", {}).get("account", {}).get("password_hash", "").encode()).hexdigest()
    pw = "y" if after_mark(first, "user_hash_sha256=") == want else "n"
    need(pw == "y", "the installed account's password hash is not the job's")
    # the disk under / is the size the job names
    root_src, rd = after_mark(first, "root_src=").replace("/dev/", ""), ""
    for l in first.splitlines():
        f = l.split()
        if len(f) >= 3 and f[2] == "disk": rd = f
        if root_src and len(f) >= 3 and f[0].lstrip("|-`└├─") == root_src: break
    root_ok = "y" if rd and rd[1] == str(job.get("identity", {}).get("system_disk", {}).get("size_bytes")) else "n"
    need(root_ok == "y", "/ is not on the disk the job names (root %s, disk %s)" % (root_src, rd))
    graphical = "y" if after_mark(first, "default_target=") == "graphical.target" and after_mark(first, "display_manager=") == "active" else "n"
    need(graphical == "y", "start_at desktop did not hold: %s, display manager %s" % (after_mark(first, "default_target="), after_mark(first, "display_manager=")))
    need("enabled" in after_mark(first, "sb=").lower(), "Secure Boot was not on at the first start")
    outcome_checks(False)
    if not why: result = "erased-installed"
    notes.append("early: %s" % after_mark(early_log, "== the stick's Ubuntu files: "))
    notes.append("first start: kernel %s, root %s, boot %s, esp %s" % (after_mark(first, "kernel="), root_src, after_mark(first, "boot_src="), after_mark(first, "esp_src=")))
elif letter == "K":
    late = js(R + "late.json")
    need(identity == "pass", "the early check did not decide to install")
    need(converted == "y" and armed == "n" and commit == "y", "markers: converted=%s boot-install=%s commit=%s" % (converted, armed, commit))
    need(bool(late), "no late.json: the late hook did not run")
    need(decoy_same == "y", "the decoy disk changed (unchanged=%s)" % decoy_same)
    def parts(p):
        return {l.split()[0]: l.split()[1:] for l in text(p).splitlines() if len(l.split()) == 5}
    pb, pa = parts("parts-before.txt"), parts("parts-after.txt")
    ESP, LINUX = "c12a7328-f81f-11d2-ba4b-00a0c93ec93b", "0fc63daf-8483-4772-8e79-3d69d8477de4"
    need(bool(pb), "the partitions were not hashed before the run")
    same, esp_n = [], ""
    for n, b in sorted(pb.items()):
        a = pa.get(n)
        if not need(a is not None and a[:3] == b[:3], "partition %s moved, changed size or type, or is gone" % n): continue
        if b[0] == ESP: esp_n = n
        elif need(a[3] == b[3], "partition %s is not byte for byte what it was" % n): same.append(n)
    new = sorted((int(v[1]), n, v) for n, v in pa.items() if n not in pb)
    need(len(new) == 2 and all(v[0] == LINUX for _, _, v in new) and int(new[0][2][2]) == 1024 * 1048576 and int(new[0][2][1]) + int(new[0][2][2]) == int(new[1][2][1]),
         "the new partitions are not /boot (1 GiB) then /: %r" % [(n, v[:3]) for _, n, v in new])
    eb, ea = set(text("esp-before.txt").splitlines()), set(text("esp-after.txt").splitlines())
    need(bool(eb) and eb <= ea, "files are gone from Windows' boot partition: %s" % sorted(eb - ea)[:5])
    lb, la = (d / "bootmgfw-before.efi"), (d / "bootmgfw-after.efi")
    need(lb.is_file() and la.is_file() and lb.read_bytes() == la.read_bytes(), "Windows' loader bootmgfw.efi changed or could not be read")
    snap = d / "stick/upgrade_/esp-snapshot"
    need((snap / "SHA256SUMS").is_file() and lb.is_file() and any(p.read_bytes() == lb.read_bytes() for p in snap.rglob("*") if p.is_file() and p.name.lower() == "bootmgfw.efi"),
         "the stick's safety copy does not hold Windows' loader as it was")
    sys_same = "n"
    booted = "y" if "== firstboot" in first else "n"
    need(booted == "y", "no first-start record on the log disk")
    want = hashlib.sha256(job.get("intent", {}).get("account", {}).get("password_hash", "").encode()).hexdigest()
    pw = "y" if after_mark(first, "user_hash_sha256=") == want else "n"
    need(pw == "y", "the installed account's password hash is not the job's")
    root_src, rd = after_mark(first, "root_src=").replace("/dev/", ""), ""
    for l in first.splitlines():
        f = l.split()
        if len(f) >= 3 and f[2] == "disk": rd = f
        if root_src and len(f) >= 3 and f[0].lstrip("|-`\u2514\u251c\u2500") == root_src: break
    root_ok = "y" if rd and rd[1] == str(job.get("identity", {}).get("system_disk", {}).get("size_bytes")) else "n"
    need(root_ok == "y", "/ is not on the disk the job names (root %s, disk %s)" % (root_src, rd))
    graphical = "y" if after_mark(first, "default_target=") == "graphical.target" and after_mark(first, "display_manager=") == "active" else "n"
    need(graphical == "y", "start_at desktop did not hold")
    need("enabled" in after_mark(first, "sb=").lower(), "Secure Boot was not on at the first start")
    need(after_mark(first, "kept_entries=") == "1" and after_mark(first, "osprober_entries=") == "0",
         "the start menu: 'Windows (kept)' entries %s, os-prober lines %s (one and none wanted, R21)" % (after_mark(first, "kept_entries=") or "?", after_mark(first, "osprober_entries=") or "?"))
    outcome_checks(True)
    if not why: result = "installed-beside-windows"
    notes.append("kept byte for byte: %s; Windows' boot partition %s in place, %d files before, all there after (%d now), bootmgfw.efi unchanged" % (" ".join(same), esp_n, len(eb), len(ea)))
    notes.append("new: " + " ".join("%s=%dMiB" % (n, int(v[2]) // 1048576) for _, n, v in new))
    notes.append("first start: kernel %s, root %s, boot %s, esp %s" % (after_mark(first, "kernel="), root_src, after_mark(first, "boot_src="), after_mark(first, "esp_src=")))
    notes.append("Windows was not started afterwards: Hyper-V's firmware cannot trust both signers at once")
elif letter == "G":
    detail = str(refusal.get("detail", ""))
    need(code == "21", "refusal.json exit_code is %s, not 21" % code)
    need("2 free spaces" in detail, "the refusal does not say two free spaces: %r" % detail[:160])
    need(len(ref_lines) >= 2 and all(" exit 21:" in l for l in ref_lines), "refusals.log has %d lines" % len(ref_lines))
    need(converted == "n" and armed == "y" and commit == "n", "markers: converted=%s boot-install=%s commit=%s" % (converted, armed, commit))
    need(sys_same == "y" and decoy_same == "y", "a disk changed: system unchanged=%s decoy unchanged=%s" % (sys_same, decoy_same))
    if not why: result = "refused-two-spaces"
    notes.append("a made-up disk, not Windows (a spoof): " + detail[:200])
    notes.append("%d refusals on the stick" % len(ref_lines))
elif letter == "D":
    need(code == "28", "refusal.json exit_code is %s, not 28" % code)
    need(bool(ref_lines) and all(" exit 28:" in l for l in ref_lines), "refusals.log: %r" % ref_lines[:3])
    need(converted == "y" and armed == "n", "markers: converted=%s boot-install=%s" % (converted, armed))
    need(sys_same == "y" and decoy_same == "y", "a disk changed: system unchanged=%s decoy unchanged=%s" % (sys_same, decoy_same))
    if not why: result = "not-installed-again"
    notes.append("%d refusals on the stick" % len(ref_lines))
else:
    why.append("unknown arm " + arm)

if after_mark(guest, "installer_sha256="): notes.append("upgrade-installer sha256 " + after_mark(guest, "installer_sha256="))
if note: notes.insert(0, note)
row = {"timestamp": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"), "arm": arm, "firmware": firmware,
       "installer_version": installer_v, "generator_version": gen_v, "secure_boot": secure_boot, "network": network, "identity": identity,
       "refusal_code": code, "refusals": str(len(ref_lines)), "converted_marker": converted, "armed_marker": armed, "commit_record": commit,
       "system_disk_unchanged": sys_same, "decoy_disk_unchanged": decoy_same, "system_gpt_after": gpt_after, "ubuntu_booted": booted,
       "password_matches": pw, "root_on_job_disk": root_ok, "graphical_login": graphical, "result": result,
       "notes": " | ".join(why + notes)}
new = not out.is_file()
with open(out, "a", newline="") as f:
    w = csv.DictWriter(f, fieldnames=COLS, quoting=csv.QUOTE_ALL, lineterminator="\n")
    if new: w.writeheader()
    w.writerow(row)
print("ubuntu-verdict: arm %s -> %s" % (arm, result))
for x in why: print("   why:", x)
sys.exit(0 if result != "fail" else 1)
