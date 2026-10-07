#!/usr/bin/env bash
#
# port-check.sh - is the Rust port still saying what the PowerShell says?
# (RISKS R32, VALIDATION V13, docs/RUST-PORT.md)
#
#   ./port-check.sh            check everything, change nothing
#   ./port-check.sh --record   write the recorded files again from the
#                              PowerShell and Python sources, then check
#
# What it runs, in order:
#   1. the PowerShell self-tests and schemas/check.py (the originals)
#   2. the recorded files are fresh: data/tables.json (from data/*.ps1),
#      schemas/rust/tests/refused-cases.json (from check.py),
#      evaluate/scan/tests/golden.json (from the PowerShell scanner),
#      and the golden.json of the harvester, the job writer, the password
#      hasher and the kickstart generator (each from its own script)
#   3. every PowerShell self-test case has a Rust case under the same name
#   4. cargo test in every Rust crate of the port, and the differential
#      run against check.py
#
# powershell.exe is needed for the PowerShell parts (WSL on Windows). Without
# it those parts are named as SKIPPED and the script exits 3: the Rust tests
# still ran, against the files as recorded, but nothing proved they are fresh.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT"
RECORD=0; [ "${1:-}" = "--record" ] && RECORD=1
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
FAILED=0; SKIPPED=0
say()  { printf '%s\n' "$*"; }
pass() { say "    PASS  $*"; }
fail() { say "    FAIL  $*"; FAILED=$((FAILED + 1)); }
skip() { say "    SKIPPED (no powershell.exe)  $*"; SKIPPED=$((SKIPPED + 1)); }
ps() { powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$(wslpath -w "$1")" "${@:2}" 2>&1 | tr -d '\r'; }
HAVE_PS=0; command -v powershell.exe >/dev/null 2>&1 && HAVE_PS=1

# written again and compared: $1 = the committed file, $2 = the fresh copy
fresh() {
    if [ "$RECORD" = 1 ]; then cp "$2" "$1"; pass "recorded $1"
    elif cmp -s "$1" "$2"; then pass "$1 is fresh"
    else fail "$1 is stale: run ./port-check.sh --record and read the diff"; fi
}

say; say "  1. the originals"; say
if [ "$HAVE_PS" = 1 ]; then
    ps evaluate/windows/upgrade-scan.ps1 -SelfTest > "$TMP/scan-selftest.txt" || true
    if grep -q 'all checks passed' "$TMP/scan-selftest.txt"; then pass "upgrade-scan.ps1 -SelfTest ($(grep -c '  PASS  ' "$TMP/scan-selftest.txt") cases)"; else fail "upgrade-scan.ps1 -SelfTest"; fi
    ps evaluate/windows/Harvest-UpgradeState.ps1 -SelfTest > "$TMP/harvest-selftest.txt" || true
    if grep -q 'all checks passed' "$TMP/harvest-selftest.txt"; then pass "Harvest-UpgradeState.ps1 -SelfTest"; else fail "Harvest-UpgradeState.ps1 -SelfTest"; fi
else
    skip "upgrade-scan.ps1 -SelfTest"; skip "Harvest-UpgradeState.ps1 -SelfTest"
fi
if [ "$HAVE_PS" = 1 ]; then
    ps upgrade_/windows/New-Kickstart.ps1 -SelfTest > "$TMP/ks-selftest.txt" || true
    if grep -q 'all checks passed' "$TMP/ks-selftest.txt"; then pass "New-Kickstart.ps1 -SelfTest ($(grep -c '  PASS  ' "$TMP/ks-selftest.txt") cases)"; else fail "New-Kickstart.ps1 -SelfTest"; fi
else
    skip "New-Kickstart.ps1 -SelfTest"
fi
if [ "$HAVE_PS" = 1 ]; then
    ps evaluate/windows/New-Job.ps1 -SelfTest > "$TMP/job-selftest.txt" || true
    if grep -q 'all checks passed' "$TMP/job-selftest.txt"; then pass "New-Job.ps1 -SelfTest ($(grep -c '  PASS  ' "$TMP/job-selftest.txt") cases)"; else fail "New-Job.ps1 -SelfTest"; fi
    ps evaluate/windows/Read-Password.ps1 -SelfTest > "$TMP/password-selftest.txt" || true
    if grep -q 'all checks passed' "$TMP/password-selftest.txt"; then pass "Read-Password.ps1 -SelfTest ($(grep -c '  PASS  ' "$TMP/password-selftest.txt") cases)"; else fail "Read-Password.ps1 -SelfTest"; fi
else
    skip "New-Job.ps1 -SelfTest"; skip "Read-Password.ps1 -SelfTest"
fi
if python3 schemas/check.py --dump-cases "$TMP/refused-cases.json" --dump-mutations "$TMP/mutations.jsonl" > "$TMP/check.txt" 2>&1; then
    pass "schemas/check.py ($(grep -c '  PASS  ' "$TMP/check.txt") checks)"
else fail "schemas/check.py"; fi

say; say "  2. the recorded files"; say
[ -f "$TMP/refused-cases.json" ] && fresh schemas/rust/tests/refused-cases.json "$TMP/refused-cases.json"
if [ "$HAVE_PS" = 1 ]; then
    ps data/tools/export-tables.ps1 -Out "$(wslpath -w "$TMP")\\tables.json" > /dev/null
    fresh data/tables.json "$TMP/tables.json"
    ps evaluate/scan/tests/golden.ps1 -Out "$(wslpath -w "$TMP")\\golden.json" > /dev/null
    fresh evaluate/scan/tests/golden.json "$TMP/golden.json"
    ps upgrade_/kickstart/tests/golden.ps1 -Out "$(wslpath -w "$TMP")\\ks-golden.json" > /dev/null
    fresh upgrade_/kickstart/tests/golden.json "$TMP/ks-golden.json"
    ps evaluate/harvest/tests/golden.ps1 -Out "$(wslpath -w "$TMP")\\harvest-golden.json" > /dev/null
    fresh evaluate/harvest/tests/golden.json "$TMP/harvest-golden.json"
    ps evaluate/job/tests/golden.ps1 -Out "$(wslpath -w "$TMP")\\job-golden.json" > /dev/null
    fresh evaluate/job/tests/golden.json "$TMP/job-golden.json"
    ps evaluate/job/tests/password-golden.ps1 -Out "$(wslpath -w "$TMP")\\password-golden.json" > /dev/null
    fresh evaluate/job/tests/password-golden.json "$TMP/password-golden.json"
else
    skip "upgrade_/kickstart/tests/golden.json against New-Kickstart.ps1"
    skip "data/tables.json against data/*.ps1"; skip "evaluate/scan/tests/golden.json against the PowerShell scanner"
fi

say; say "  3. every self-test case has a Rust case"; say
same_names() {  # $1 = the self-test's output, $2 = the crate's cases.json, $3 = what to call it,
                # $4 = (optional) a file naming the self-test cases not ported yet
    if python3 - "$1" "$2" "${4:-}" <<'PY'
import json, sys
ran = {l.split('  PASS  ', 1)[1].rstrip('\n') for l in open(sys.argv[1], encoding='utf-8', errors='replace') if '  PASS  ' in l}
ours = {c['name'] for c in json.load(open(sys.argv[2], encoding='utf-8')) if c['origin'] != 'port'} - {'distro table: every kernel parses'}
if sys.argv[3]:
    owed = {l.rstrip('\n') for l in open(sys.argv[3], encoding='utf-8') if l.strip() and not l.startswith('#')}
    for n in sorted(owed & ours): print(f"          listed as not ported, but it is in cases.json: {n}")
    if owed & ours: sys.exit(1)
    ours |= owed
for n in sorted(ran - ours): print(f"          in the PowerShell self-test, not in cases.json: {n}")
for n in sorted(ours - ran): print(f"          in cases.json, not in the PowerShell self-test: {n}")
sys.exit(1 if ran != ours else 0)
PY
    then pass "$3: the PowerShell self-test and $2 name the same cases"
    else fail "$3: the PowerShell self-test and $2 disagree"; fi
}
if [ "$HAVE_PS" = 1 ]; then
    same_names "$TMP/scan-selftest.txt" evaluate/scan/tests/cases.json "scanner"
    same_names "$TMP/ks-selftest.txt" upgrade_/kickstart/tests/cases.json "kickstart"
    same_names "$TMP/harvest-selftest.txt" evaluate/harvest/tests/cases.json "harvester" evaluate/harvest/tests/windows-selftest.txt
    same_names "$TMP/job-selftest.txt" evaluate/job/tests/cases.json "job writer"
    same_names "$TMP/password-selftest.txt" evaluate/job/tests/password-cases.json "password hasher"
else
    skip "self-test case names against cases.json"
fi

say; say "  4. the Rust side"; say
for crate in schemas/rust evaluate/scan evaluate/harvest evaluate/job upgrade_/kickstart; do
    if (cd "$crate" && cargo test --locked --quiet) > "$TMP/cargo.txt" 2>&1; then pass "cargo test in $crate"
    else fail "cargo test in $crate"; sed 's/^/          /' "$TMP/cargo.txt" | tail -40; fi
done
if [ "$HAVE_PS" = 1 ]; then
    if (cd evaluate/harvest && ./windows-tests.sh) > "$TMP/wintests.txt" 2>&1 && grep -q "test result: ok. 12 passed" "$TMP/wintests.txt"; then pass "the harvester's 12 filesystem cases on this Windows machine"
    else fail "the harvester's filesystem cases on Windows"; tail -20 "$TMP/wintests.txt" | sed 's/^/          /'; fi
else
    skip "the harvester's filesystem cases (they run on Windows)"
fi
if (cd schemas/rust && UPGRADE_SCHEMA_MUTATIONS="$TMP/mutations.jsonl" cargo test --locked --quiet -- --ignored --nocapture) > "$TMP/diff.txt" 2>&1; then
    pass "schemas differential: $(grep -o '[0-9]* one-edit documents judged the same.*' "$TMP/diff.txt" || echo done)"
else fail "schemas differential against check.py"; sed 's/^/          /' "$TMP/diff.txt" | tail -30; fi

say
if [ "$FAILED" -gt 0 ]; then say "  $FAILED failed"; say; exit 1; fi
if [ "$SKIPPED" -gt 0 ]; then say "  nothing failed, but $SKIPPED checks were SKIPPED: freshness is unproven on this machine"; say; exit 3; fi
say "  all checks passed"; say
