#!/usr/bin/env python3
"""
record-golden.py - put the Rust scanner's own answers into golden.json for the
cases that changed on purpose (CLAUDE.md: a golden changes only on purpose, from
the Rust's own output, reviewed as a diff, its new sha256 recorded in the same
commit).

    UPG_RECORD_GOLDEN=/tmp/changed.json cargo test --test parity   (it fails, and writes the file)
    record-golden.py /tmp/changed.json                             (then read `git diff golden.json`)

Only the lines of the cases that differ are rewritten; every other line of the
recording stays byte for byte what the PowerShell scanner answered. Then put
the file's new sha256 in FROZEN-GOLDENS.sha256.
"""
import json, pathlib, sys
changed = json.load(open(sys.argv[1]))
p = pathlib.Path(__file__).with_name("golden.json")
lines = p.read_text(encoding="utf-8").split("\n")
done = set()
for i, l in enumerate(lines):
    for name, got in changed.items():
        key = json.dumps(name) + ": "
        if l.startswith(key):
            lines[i] = key + json.dumps(got, ensure_ascii=False, separators=(",", ":")) + ("," if l.rstrip().endswith(",") else "")
            done.add(name)
missing = set(changed) - done
if missing: sys.exit("record-golden: no line for %s" % sorted(missing))
p.write_text("\n".join(lines), encoding="utf-8")
print("record-golden: %d cases rewritten" % len(done))
