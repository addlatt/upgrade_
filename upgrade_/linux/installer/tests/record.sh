#!/usr/bin/env bash
# Records tests/golden.json: what verify.sh and outcome.sh did for every
# case in tests/cases.json, each run in the sandbox (tests/sandbox.py).
# The scripts are copied with two test-only substitutions, so the
# countdown and the refusal screen can be short: COUNT_SECS and the refusal
# screen's 60 s take UPG_TEST_COUNTDOWN and UPG_TEST_REFUSAL_SECS when set
# (the program honours the same two variables). Ran once, 2026-10-14, while
# the scripts were in the tree; stays as the record of how. The scripts
# left the tree in the commit that landed the program; to record again,
# take them from git (the commit before that one):
#   git show a2eaf85:upgrade_/linux/verify.sh  > <dir>/verify.sh
#   git show a2eaf85:upgrade_/linux/outcome.sh > <dir>/outcome.sh
#
#   tests/record.sh [<out dir>] [<dir holding verify.sh and outcome.sh>]
#       (defaults: target/sandbox/record, and ../.. where the scripts lived)
#
# Since upgrade-installer 0.1.2 (2026-10-17, R38: the console moved from
# tty6 to tty9 and the scripts never knew tty9) the golden is recorded from
# the program's own output, reviewed as a diff against the one before:
#   tests/record.sh --program <upgrade-installer> [<out dir>]
# Recorded so again for 0.1.3 (2026-10-17: the kickstart's chroot %post
# moved into `outcome`).
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROGRAM=
if [ "${1:-}" = --program ]; then PROGRAM=$(realpath "$2"); shift 2; fi
OUT=${1:-$HERE/../target/sandbox/record}
SCRIPTS=${2:-$HERE/../..}
if [ -z "$PROGRAM" ]; then
[ -f "$SCRIPTS/verify.sh" ] && [ -f "$SCRIPTS/outcome.sh" ] || { echo "record: no verify.sh and outcome.sh in $SCRIPTS (git show a2eaf85:upgrade_/linux/verify.sh)" >&2; exit 1; }
mkdir -p "$OUT"
sed -e 's/^    COUNT_SECS=120; CTTY=\/dev\/tty6$/    COUNT_SECS=${UPG_TEST_COUNTDOWN:-120}; CTTY=\/dev\/tty6/' \
    -e 's/^end = time.monotonic() + 60$/end = time.monotonic() + float(__import__("os").environ.get("UPG_TEST_REFUSAL_SECS", "60"))/' \
    "$SCRIPTS/verify.sh" > "$OUT/verify.sh"
[ "$(grep -c UPG_TEST "$OUT/verify.sh")" = 2 ] || { echo "record: the two substitutions did not apply" >&2; exit 1; }
cp "$SCRIPTS/outcome.sh" "$OUT/outcome.sh"
else
mkdir -p "$OUT"
fi
python3 - "$HERE/cases.json" "$OUT" "$HERE/golden.json" "$PROGRAM" <<'PY'
import json, subprocess, sys, os
cases, out, golden_path, program = json.load(open(sys.argv[1])), sys.argv[2], sys.argv[3], sys.argv[4]
here = os.path.dirname(os.path.abspath(sys.argv[1]))
golden = {}
for i, c in enumerate(cases, 1):
    d = os.path.join(out, "case-%02d" % i)
    if program:
        cmd = [program, "verify", "JOB", "LABEL"] if c["mode"] == "verify" else [program, "outcome", "JOB"]
    else:
        cmd = ["bash", os.path.join(out, "verify.sh"), "JOB", "LABEL"] if c["mode"] == "verify" else ["bash", os.path.join(out, "outcome.sh"), "JOB"]
    r = subprocess.run([sys.executable, os.path.join(here, "sandbox.py"), sys.argv[1], c["name"]] + cmd + ["--out", d], capture_output=True, text=True)
    if r.returncode != 0:
        print(r.stdout, r.stderr); raise SystemExit("record: sandbox failed on " + c["name"])
    res = json.load(open(os.path.join(d, "result.json")))
    res.pop("sandbox_stderr", None)
    golden[c["name"]] = res
    print("  %-7s exit %-4s %3d lines %3d calls %2d screens  %s" % (c["mode"], res["exit"], len(res["stdout"]), len(res["calls"]), len(res["screens"]), c["name"]))
json.dump(golden, open(golden_path, "w"), indent=1, ensure_ascii=False)
open(golden_path, "a").write("\n")
print("  written: %s (%d cases)" % (golden_path, len(cases)))
PY
