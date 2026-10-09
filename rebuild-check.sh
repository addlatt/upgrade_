#!/usr/bin/env bash
# Reproducible builds (RISKS R14): a second checkout of the same commit,
# built the same way, must give the same bytes. This clones HEAD into a
# fresh directory, builds the five Windows programs there (build-rust.sh and
# the window's build.sh, the pinned toolchain, --locked, paths remapped) and
# compares their sha256 with this checkout's. One row per program goes to
# docs/validation-results/r14-rebuild.csv. Exit 1 if any differ.
#
#   ./rebuild-check.sh            build here too, then compare
#   ./rebuild-check.sh --kit DIR  compare with the exes in a kit folder (dist/kit/stick)
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REV=$(git -C "$ROOT" rev-parse --short HEAD)
[ -z "$(git -C "$ROOT" status --porcelain -- evaluate upgrade_ schemas settle-in Cargo.lock 2>/dev/null | grep -v '^??')" ] || { echo "rebuild-check: commit first (the clone builds HEAD, this tree has edits)" >&2; exit 1; }
EXES=(target/x86_64-pc-windows-gnu/release/upgrade-scan.exe target/x86_64-pc-windows-gnu/release/upgrade-harvest.exe target/x86_64-pc-windows-gnu/release/upgrade-job.exe target/x86_64-pc-windows-gnu/release/upgrade-prologue.exe target/x86_64-pc-windows-gnu/release/UPGRADE.exe)
if [ "${1:-}" = --kit ]; then
    KIT="$2"; here=()
    for e in "${EXES[@]}"; do here+=("$KIT/$(basename "$e")"); done
else
    # cargo does not count SOURCE_DATE_EPOCH as a reason to relink, and a deleted final exe
    # is only copied again from deps/ (runs 2 and 3, 2026-10-09: this tree's old binaries
    # were compared), so each package is cleaned for the Windows target first
    for c in upgrade-scan upgrade-harvest upgrade-job upgrade-prologue upgrade-window; do
        (cd "$ROOT" && cargo clean -p "$c" --release --target x86_64-pc-windows-gnu --quiet)
    done
    "$ROOT/build-rust.sh" >/dev/null; "$ROOT/upgrade_/windows/window/build.sh" >/dev/null
    here=(); for e in "${EXES[@]}"; do here+=("$ROOT/$e"); done
fi
TMP=$(mktemp -d "${TMPDIR:-/tmp}/upgrade_-rebuild.XXXXXX")
trap 'rm -rf "$TMP"' EXIT
git clone -q --no-local "$ROOT" "$TMP/src" && git -C "$TMP/src" checkout -q "$REV"
"$TMP/src/build-rust.sh" >/dev/null; "$TMP/src/upgrade_/windows/window/build.sh" >/dev/null
CSV="$ROOT/docs/validation-results/r14-rebuild.csv"
[ -f "$CSV" ] || echo 'timestamp,commit,program,sha256_here,sha256_fresh_clone,same,toolchain' > "$CSV"
fail=0; tc=$("$ROOT/build-rust.sh" --toolchain)
for i in "${!EXES[@]}"; do
    a=$(sha256sum "${here[$i]}" | cut -c1-64); b=$(sha256sum "$TMP/src/${EXES[$i]}" | cut -c1-64)
    same=y; [ "$a" = "$b" ] || { same=n; fail=1; }
    printf '"%s","%s","%s","%s","%s","%s","%s"\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$REV" "$(basename "${EXES[$i]}")" "$a" "$b" "$same" "$tc" >> "$CSV"
    echo "  $(basename "${EXES[$i]}"): $same  ($a)"
done
[ $fail = 0 ] && echo "rebuild-check: the same bytes from a fresh clone of $REV" || { echo "rebuild-check: DIFFERENT bytes (see $CSV)" >&2; exit 1; }
