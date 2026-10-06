#!/usr/bin/env python3
"""
v9-inspect.py - offline, read-only look at the V9 rig's two disks (RISKS R27).

    v9-inspect.py <label> <outdir> <system.vhdx> <home.vhdx>

For each disk: the GPT (via rig/vm/v1b-inspect.py's reader - qemu-img dd,
no loop devices) or null when the disk has none (a blank disk), and the
sha256 of its first MiB. Written to <outdir>/v9-<label>.json. "Untouched"
for the refusal and cancel arms is: the same JSON before and after.
"""
import hashlib, importlib.util, json, os, sys, tempfile, pathlib

here = pathlib.Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("v1b_inspect", here.parent / "vm" / "v1b-inspect.py")
v1b = importlib.util.module_from_spec(spec); spec.loader.exec_module(v1b)

label, outdir = sys.argv[1], pathlib.Path(sys.argv[2])
disks = {"system": sys.argv[3], "home": sys.argv[4]}
res = {"label": label, "disks": {}}
with tempfile.TemporaryDirectory() as tmp:
    for role, img in disks.items():
        if img == "-":          # no image of this disk (a real machine's before picture may lack one)
            res["disks"][role] = None
            continue
        head = os.path.join(tmp, "head-%s.raw" % role)
        v1b.qdd(img, head, 0, 1 << 20, 1 << 20)
        first = hashlib.sha256(open(head, "rb").read()).hexdigest()
        try: gpt = v1b.read_gpt(img, tmp)
        except AssertionError: gpt = None
        res["disks"][role] = {"image": os.path.basename(img), "first_mib_sha256": first, "gpt": gpt}
outdir.mkdir(parents=True, exist_ok=True)
json.dump(res, open(outdir / ("v9-%s.json" % label), "w"), indent=2)
for role, d in res["disks"].items():
    if d is None:
        print("v9-inspect %s %s: no image" % (label, role)); continue
    parts = d["gpt"]["partitions"] if d["gpt"] else []
    print("v9-inspect %s %s: %s" % (label, role, ", ".join("%s %s MiB" % (p["type"], p["size_mib"]) for p in parts) or "no GPT (first MiB %s)" % d["first_mib_sha256"][:12]))
