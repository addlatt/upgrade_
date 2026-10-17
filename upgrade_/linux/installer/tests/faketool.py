#!/usr/bin/env python3
"""The tools the installer's steps call, answering from the sandbox's world
(sandbox.py). Every call is appended to the world's calls.log, so a run
records not only what the step wrote but what it asked the machine. The
firmware's entries live in efi-state.json and change as efibootmgr would
change them."""
import json, os, subprocess, sys

WORLD = os.environ["WORLD"]
W = json.load(open(os.path.join(WORLD, "world.json")))
tool, args = sys.argv[1], sys.argv[2:]
with open(os.path.join(WORLD, "calls.log"), "a") as f:
    f.write(tool + " " + " ".join(args) + "\n")


def dev_of(path):
    return os.path.basename(path or "")


def find_disk(name):
    for d in W.get("disks", []):
        if d["dev"] == name:
            return d, None
        for p in d.get("parts", []):
            if f"{d['dev']}{p['n']}" == name:
                return d, p
    return None, None


def out(text):
    sys.stdout.write(text)
    sys.stdout.flush()


if tool == "blockdev":
    d, p = find_disk(dev_of(args[-1]))
    if d is None:
        sys.exit(1)
    out(str(p["size"] if p else d["size"]) + "\n")
elif tool == "lsblk":
    cols = []
    bytes_ = False
    device = None
    i = 0
    while i < len(args):
        a = args[i]
        if a == "-o":
            cols = args[i + 1].split(","); i += 2; continue
        if a.startswith("-") and "o" in a and a not in ("-b",):
            # -no COLS, -dno COLS, -bno
            cols = args[i + 1].split(","); bytes_ = bytes_ or "b" in a; i += 2; continue
        if a == "-b":
            bytes_ = True
        elif not a.startswith("-"):
            device = a
        i += 1
    if device is None:
        rows = []
        for d in W.get("disks", []):
            rows.append(f"{d['dev']} {d['size']} disk {d.get('fstype', '')} {d.get('label', '')} {d.get('serial', '')} {d.get('wwn', '')}")
            for p in d.get("parts", []):
                rows.append(f"{d['dev']}{p['n']} {p['size']} part {p.get('fstype', '')} {p.get('label', '')} {p.get('parttypename', '')}")
        out("NAME SIZE TYPE FSTYPE LABEL PARTTYPENAME SERIAL WWN\n" + "\n".join(rows) + "\n")
        sys.exit(0)
    d, p = find_disk(dev_of(device))
    if d is None:
        sys.exit(32)
    vals = []
    for c in cols:
        c = c.upper()
        if c == "PARTTYPE": vals.append(p.get("parttype", "") if p else "")
        elif c == "MODEL": vals.append(d.get("model", ""))
        elif c == "PKNAME": vals.append(d["dev"] if p else "")
        elif c == "PARTN": vals.append(str(p["n"]) if p else "")
        elif c == "PARTUUID": vals.append(p.get("partuuid", "") if p else "")
        elif c == "SIZE": vals.append(str(p["size"] if p else d["size"]))
        elif c == "FSTYPE": vals.append((p.get("fstype", "") if p else d.get("fstype", "")))
        elif c == "LABEL": vals.append(p.get("label", "") if p else "")
        else: vals.append("")
    out(" ".join(vals) + "\n")
elif tool == "mount":
    if "-o" in args and args[args.index("-o") + 1].startswith("remount"):
        sys.exit(32 if W.get("stick_remount_fails") else 0)
    if "-o" in args and args[args.index("-o") + 1] == "ro":
        part, target = args[-2], args[-1]
        src = os.path.join(WORLD, "esp", dev_of(part))
        if not os.path.isdir(src):
            sys.exit(32)
        sys.exit(subprocess.call(["/bin/mount", "--bind", src, target]))
    sys.exit(subprocess.call(["/bin/mount"] + args))
elif tool == "umount":
    sys.exit(subprocess.call(["/bin/umount"] + args))
elif tool == "nmcli":
    n = W.get("nmcli", {})
    if args[:4] == ["-t", "-f", "DEVICE,TYPE", "device"]:
        out("".join(f"{d}:{t}\n" for d, t in n.get("devices", [])))
    elif "rescan" in args:
        pass
    elif "SSID" in " ".join(args):
        out("".join(s + "\n" for s in n.get("ssids", [])))
    elif args == ["device"]:
        out("DEVICE TYPE STATE\n" + "".join(f"{d} {t} connected\n" for d, t in n.get("devices", [])))
    if not n and not W.get("nmcli_present", True):
        sys.exit(127)
elif tool == "lspci":
    out("".join(l + "\n" for l in W.get("lspci", ["00:00.0 Host bridge [0600]: Fake Host Bridge [1234:0001]"])))
elif tool == "dmesg":
    out("[    0.000000] Linux version fake\n")
elif tool == "uname":
    out(W.get("kernel", "6.14.0-63.fc44.x86_64") + "\n")
elif tool == "efibootmgr":
    path = os.path.join(WORLD, "efi-state.json")
    st = json.load(open(path))
    if "-c" in args:
        label = args[args.index("-L") + 1]
        loader = args[args.index("-l") + 1]
        disk = args[args.index("-d") + 1]
        pnum = args[args.index("-p") + 1]
        used = {e[0] for e in st["entries"]}
        num = next(f"{k:04X}" for k in range(0, 0xFFFF) if f"{k:04X}" not in used)
        st["entries"].append([num, label, f"HD({pnum},GPT,00000000-0000-0000-0000-000000000000,0x800,0x32000)/File({loader})"])
        st["order"] = [num] + [o for o in st["order"] if o != num]
        json.dump(st, open(path, "w"), indent=1)
        if W.get("efibootmgr_create_fails"):
            sys.exit(1)
    elif "-o" in args:
        st["order"] = args[args.index("-o") + 1].split(",")
        json.dump(st, open(path, "w"), indent=1)
        if W.get("efibootmgr_order_fails"):
            sys.exit(1)
    lines = [f"BootCurrent: {st.get('current', '')}", "Timeout: 0 seconds", "BootOrder: " + ",".join(st["order"])]
    for num, label, p in st["entries"]:
        lines.append(f"Boot{num}* {label}\t{p}")
    out("\n".join(lines) + "\n")
elif tool == "findmnt":
    target = args[-1]
    v = W.get("findmnt", {}).get(target)
    if v is None:
        sys.exit(1)
    out(v + "\n")
elif tool in ("chroot", "restorecon", "sync", "sleep"):
    pass
elif tool == "timedatectl":
    out(W.get("clock", {}).get("ntp", "") + "\n")
elif tool == "chronyc":
    out(W.get("clock", {}).get("chronyc", "") + "\n")
elif tool == "chvt":
    sys.exit(0 if W.get("chvt_ok", True) else 1)
elif tool == "systemctl":
    if args[:1] == ["reboot"] and W.get("systemctl_reboot_fails"):
        sys.exit(1)
elif tool == "reboot":
    pass
sys.exit(0)
