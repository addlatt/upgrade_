#!/usr/bin/env bash
#
# port-check.sh - the Rust product's own checks, in one command.
# (RISKS R32, VALIDATION V13, docs/RUST-PORT.md)
#
#   ./port-check.sh            check everything, change nothing
#   ./port-check.sh --record   write the schema refusal cases again from
#                              their source, then check
#
# Decided (2026-10-11, the owner): the PowerShell scripts leave the product.
# The goldens the Rust was held to (recorded from the scripts while they
# existed) are frozen recordings now: their sha256 is in FROZEN-GOLDENS.sha256
# and this script fails if one changes. New cases get their expected values
# from the Rust's own tests, not from PowerShell.
#
# What it runs, in order:
#   1. the original that remains: schemas/check.py (Python; the contract's
#      oracle). The data tables are JSON since 2026-10-14 and the scanner's
#      own tests check their shape; nothing here needs PowerShell.
#   2. the frozen goldens are unchanged
#   3. cargo test in every crate of the product, the harvester's filesystem
#      cases on Windows, and the schema differential against check.py
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

fresh() {  # $1 = the committed file, $2 = the fresh copy
    if [ "$RECORD" = 1 ]; then cp "$2" "$1"; pass "recorded $1"
    elif cmp -s "$1" "$2"; then pass "$1 is fresh"
    else fail "$1 is stale: run ./port-check.sh --record and read the diff"; fi
}

say; say "  1. the original that remains"; say
if python3 schemas/check.py --dump-cases "$TMP/refused-cases.json" --dump-mutations "$TMP/mutations.jsonl" > "$TMP/check.txt" 2>&1; then
    pass "schemas/check.py ($(grep -c '  PASS  ' "$TMP/check.txt") checks)"
else fail "schemas/check.py"; fi
[ -f "$TMP/refused-cases.json" ] && fresh schemas/rust/tests/refused-cases.json "$TMP/refused-cases.json"
for f in data/devices/*.json data/distros.json data/releases.json; do
    python3 -c "import json,sys; json.load(open(sys.argv[1]))" "$f" > /dev/null 2>&1 || fail "$f is not JSON"
done
pass "data/devices/*.json, data/distros.json and data/releases.json parse (their shape: evaluate/scan's data tests)"

say; say "  2. the frozen goldens"; say
if sha256sum -c --quiet FROZEN-GOLDENS.sha256 > "$TMP/frozen.txt" 2>&1; then
    pass "every frozen golden is as recorded ($(wc -l < FROZEN-GOLDENS.sha256) files, FROZEN-GOLDENS.sha256)"
else fail "a frozen golden changed (FROZEN-GOLDENS.sha256): a recording is ground truth; if a case was added on purpose, record the new sha256 there and say so in the commit"; sed 's/^/          /' "$TMP/frozen.txt"; fi

say; say "  3. the Rust side"; say
for crate in schemas/rust evaluate/scan evaluate/harvest evaluate/job evaluate/stick upgrade_/kickstart upgrade_/prologue upgrade_/windows/window; do
    if (cd "$crate" && cargo test --locked --quiet) > "$TMP/cargo.txt" 2>&1; then pass "cargo test in $crate"
    else fail "cargo test in $crate"; sed 's/^/          /' "$TMP/cargo.txt" | tail -40; fi
done
for crate in settle-in settle-in/gate upgrade_/linux/installer; do
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
if [ "$SKIPPED" -gt 0 ]; then say "  nothing failed, but $SKIPPED checks were SKIPPED on this machine"; say; exit 3; fi
say "  all checks passed"; say
