#!/usr/bin/env bash
#
# Produces dist/upgrade-scan.ps1 - a single file with no dependencies, so the
# released scanner can be downloaded and run without cloning anything.
#
# The data files stay separate in the source tree because that is what makes
# them contributable: adding a device should be a one-line diff to a table,
# not a patch against an 800-line script.
#
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SRC="$ROOT/evaluate/windows/upgrade-scan.ps1"
OUT_DIR="$ROOT/dist"
OUT="$OUT_DIR/upgrade-scan.ps1"

DATA_FILES=(
    "$ROOT/data/devices.ps1"
    "$ROOT/data/distros.ps1"
)

for f in "$SRC" "${DATA_FILES[@]}"; do
    [ -f "$f" ] || { echo "build: missing $f" >&2; exit 1; }
done

mkdir -p "$OUT_DIR"

skip=0
{
    while IFS= read -r line || [ -n "$line" ]; do
        case "$line" in
            *'#!INLINE-DATA-BEGIN!#'*)
                echo "# --- inlined by build.sh -----------------------------------------------"
                echo "# --- Do not edit here. Edit data/*.ps1 and rebuild. ---------------"
                for f in "${DATA_FILES[@]}"; do
                    cat "$f"
                    echo
                done
                skip=1
                ;;
            *'#!INLINE-DATA-END!#'*)
                skip=0
                ;;
            *)
                [ "$skip" -eq 1 ] || printf '%s\n' "$line"
                ;;
        esac
    done < "$SRC"
} > "$OUT"

# A built file that still tries to dot-source data/ would fail only on a user's
# machine, where we would never see it. Catch it here instead.
if grep -q 'PSScriptRoot.*data' "$OUT"; then
    echo "build: FAILED - dist still references the data directory" >&2
    exit 1
fi

lines=$(wc -l < "$OUT")
echo "build: wrote $OUT ($lines lines)"
