#!/usr/bin/env bash
#
# Assemble the live-test USB kit into dist/kit/ - what actually goes on the
# stick for a physical V0 boot-handoff run - and verify every part of it
# before it leaves the tree.
#
#   ./make-kit.sh [--allow-dirty] [--to DIR]
#
# One stick layout comes out, dist/kit/stick/ (copy its CONTENTS to the root
# of a FAT32 stick). It carries BOTH payloads - Fedora's signed shim+grub at
# EFI/BOOT/BOOTX64.EFI (the product path, Secure Boot on) and the unsigned
# UEFI Shell at EFI/SHELL/SHELLX64.EFI (the matrix rows) - plus the one-click
# RUN-TEST.cmd, the matrix launchers (RUN-SCANNER / ARM-HANDOFF /
# CHECK-HANDOFF), the single-file scanner, the harness, README-STICK.txt,
# SHA256SUMS and KIT-MANIFEST.txt naming the commit and versions.
#
# What is verified, in order - any failure stops the build:
#   1. the tree is committed (a row must map to a commit; --allow-dirty to
#      override, and the manifest then says so)
#   2. ./build.sh reproduces the committed dist/upgrade-scan.ps1 (R9: the
#      shipped scanner matches source)
#   3. the three self-tests pass on Windows PowerShell 5.1 - scanner,
#      harvester, handoff harness
#   4. every shipped .ps1 parses under the PS 5.1 parser
#   5. the EFI payload bits exist and are the ones the rig fetched
#      (rig/vm/fetch-payload-bits.sh; gitignored build inputs)
#   6. the grubenv block is exactly 1024 bytes with GRUB's header
#   7. after layout, SHA256SUMS re-verifies; with --to, the copy re-verifies
#
# This script never writes to a device. --to copies into a DIRECTORY (a
# mounted stick, or a staging folder). The device writer is
# evaluate/windows/Write-UpgradeStick.ps1 (R16): point it at dist/kit/stick
# with -Source and it partitions, formats, copies and re-verifies - after
# refusing everything that is not the one USB stick pointed at.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT"
OUT="$ROOT/dist/kit"
BITS="$ROOT/rig/vm/artifacts/payload-bits"
PAYLOAD="$ROOT/upgrade_/windows/handoff-payload"
HARNESS="$ROOT/upgrade_/windows/Test-Handoff.ps1"
SCANNER_SRC="$ROOT/evaluate/windows/upgrade-scan.ps1"
HARVEST_SRC="$ROOT/evaluate/windows/Harvest-UpgradeState.ps1"
SCANNER_DIST="$ROOT/dist/upgrade-scan.ps1"
RUN_SCANNER="$ROOT/evaluate/windows/usb-kit/RUN-SCANNER.cmd"

ALLOW_DIRTY=0 TO=
while [ $# -gt 0 ]; do
    case "$1" in
        --allow-dirty) ALLOW_DIRTY=1; shift ;;
        --to) TO=$2; shift 2 ;;
        *) echo "make-kit: unknown flag $1" >&2; exit 1 ;;
    esac
done

fail() { echo "make-kit: FAILED - $*" >&2; exit 1; }
step() { echo "make-kit: $*"; }

# --- 1. committed tree ------------------------------------------------------
GIT_REV=$(git rev-parse --short HEAD)
DIRTY=$(git status --porcelain --untracked-files=no | grep -v '^?? ' || true)
if [ -n "$DIRTY" ]; then
    if [ "$ALLOW_DIRTY" = 1 ]; then
        step "WARNING: tree has uncommitted changes; manifest will say dirty"
        GIT_STATE="dirty (uncommitted changes present at build time)"
    else
        fail "uncommitted changes in the tree - commit first so the evidence row maps to a commit (or --allow-dirty):
$DIRTY"
    fi
else
    GIT_STATE="clean"
fi

# --- 2. dist matches source -------------------------------------------------
step "rebuilding dist/upgrade-scan.ps1"
./build.sh >/dev/null
git diff --quiet -- dist/upgrade-scan.ps1 || fail "dist/upgrade-scan.ps1 differs from a fresh build - commit the rebuilt dist first (R9)"

# --- 3. self-tests on Windows PowerShell 5.1 ---------------------------------
command -v powershell.exe >/dev/null || fail "powershell.exe not reachable - the kit must be verified against Windows PowerShell 5.1"
PS() { powershell.exe -NoProfile -ExecutionPolicy Bypass "$@" 2>&1 | tr -d '\r'; }
selftest() {  # $1 = label, $2 = script path (WSL)
    local out
    out=$(PS -File "$(wslpath -w "$2")" -SelfTest) || true
    if echo "$out" | grep -q 'all checks passed'; then
        step "self-test passed: $1"
    else
        echo "$out" | tail -20 >&2
        fail "self-test did not pass: $1"
    fi
}
selftest "scanner"   "$SCANNER_SRC"
selftest "harvester" "$HARVEST_SRC"
selftest "handoff harness" "$HARNESS"
selftest "V8 materialization harness" "$ROOT/evaluate/windows/Test-Materialize.ps1"
selftest "stick writer" "$ROOT/evaluate/windows/Write-UpgradeStick.ps1"

# --- 4. parse-check under the PS 5.1 parser ----------------------------------
parsecheck() {
    local out
    out=$(PS -Command "\$t=\$null;\$e=\$null;[void][System.Management.Automation.Language.Parser]::ParseFile('$(wslpath -w "$1")',[ref]\$t,[ref]\$e); if (\$e.Count) { \$e | ForEach-Object { \$_.Message }; exit 1 } else { 'parse-ok' }")
    echo "$out" | grep -q 'parse-ok' || { echo "$out" >&2; fail "PS 5.1 parse error in $1"; }
    step "parses under PS 5.1: $(basename "$1")"
}
parsecheck "$SCANNER_DIST"
parsecheck "$HARNESS"

# --- 5. payload bits --------------------------------------------------------
for f in Shell.efi shimx64.efi grubx64.efi; do
    [ -f "$BITS/$f" ] || fail "missing $BITS/$f - run rig/vm/fetch-payload-bits.sh"
done
NETINST_SRC=$(sed -n 1p "$BITS/fedora-netinst.source.txt" 2>/dev/null || echo "unknown")
# the shim payload's grub MUST be the install-media build (reads EFI/BOOT/grub.cfg)
grep -q 'save_env' <(strings -n 6 "$BITS/grubx64.efi") || fail "grubx64.efi lacks save_env - wrong GRUB build"

# --- 6. the grubenv block ---------------------------------------------------
[ "$(stat -c %s "$PAYLOAD/grubenv")" = 1024 ] || fail "grubenv is not 1024 bytes"
[ "$(head -c 25 "$PAYLOAD/grubenv")" = "# GRUB Environment Block" ] || fail "grubenv lacks the GRUB header"
grep -q 'upg_fired' "$PAYLOAD/grub.cfg" || fail "grub.cfg does not set upg_fired"
grep -q "GrubFiredVar = 'upg_fired'" "$HARNESS" || fail "harness and grub.cfg disagree on the marker variable"
grep -q "FiredMarker = 'fired.txt'" "$HARNESS" && grep -q 'fired.txt' "$PAYLOAD/startup.nsh" || fail "harness and startup.nsh disagree on the marker file"

HARNESS_VERSION=$(grep -oP "^\\\$HarnessVersion = '\K[^']+" "$HARNESS")
SCANNER_VERSION=$(grep -oP "^\\\$UpgVersion = '\K[^']+" "$SCANNER_SRC")

# --- layout: ONE stick, both payloads --------------------------------------
rm -rf "$OUT"
D="$OUT/stick"
mkdir -p "$D/EFI/BOOT" "$D/EFI/SHELL"
crlf() { sed 's/\r$//; s/$/\r/' "$1" > "$2"; }
cp "$HARNESS" "$D/Test-Handoff.ps1"
cp "$SCANNER_DIST" "$D/upgrade-scan.ps1"
crlf "$RUN_SCANNER"                 "$D/RUN-SCANNER.cmd"
crlf "$PAYLOAD/RUN-TEST.cmd"        "$D/RUN-TEST.cmd"
crlf "$PAYLOAD/ARM-HANDOFF.cmd"     "$D/ARM-HANDOFF.cmd"
crlf "$PAYLOAD/CHECK-HANDOFF.cmd"   "$D/CHECK-HANDOFF.cmd"
crlf "$PAYLOAD/README-STICK.txt"    "$D/README-STICK.txt"
# signed payload: the product path, at the removable-media default location
cp "$BITS/shimx64.efi"  "$D/EFI/BOOT/BOOTX64.EFI"
cp "$BITS/grubx64.efi"  "$D/EFI/BOOT/grubx64.efi"
cp "$PAYLOAD/grub.cfg"  "$D/EFI/BOOT/grub.cfg"
cp "$PAYLOAD/grubenv"   "$D/EFI/BOOT/grubenv"
# unsigned payload: the matrix rows (Test-Handoff.ps1 -Payload shell)
cp "$BITS/Shell.efi"    "$D/EFI/SHELL/SHELLX64.EFI"
cp "$PAYLOAD/startup.nsh" "$D/startup.nsh"

# --- manifest + checksums -----------------------------------------------------
(cd "$D" && find . -type f ! -name SHA256SUMS ! -name KIT-MANIFEST.txt | sort | xargs sha256sum > SHA256SUMS)
crlf /dev/stdin "$D/KIT-MANIFEST.txt" <<MANIFEST
upgrade_ live-test kit  -  V0 boot-handoff stick (both payloads)
built:            $(date -u +%Y-%m-%dT%H:%M:%SZ)
commit:           $GIT_REV ($GIT_STATE)
harness:          Test-Handoff.ps1 $HARNESS_VERSION
scanner:          upgrade-scan.ps1 $SCANNER_VERSION (single-file build of evaluate/windows + data/)
payload bits:     rig/vm/artifacts/payload-bits (gitignored inputs; fetch-payload-bits.sh)
  Shell.efi       $(sha256sum "$BITS/Shell.efi" | cut -c1-64)   -> EFI/SHELL/SHELLX64.EFI
  shimx64.efi     $(sha256sum "$BITS/shimx64.efi" | cut -c1-64)   -> EFI/BOOT/BOOTX64.EFI
  grubx64.efi     $(sha256sum "$BITS/grubx64.efi" | cut -c1-64)   -> EFI/BOOT/grubx64.efi
  from netinst:   $NETINST_SRC
verified at build: dist matches source; scanner/harvester/harness self-tests
                  passed on Windows PowerShell 5.1; shipped .ps1 parse under
                  the PS 5.1 parser; grubenv block 1024 B with GRUB header.
files (sha256):   SHA256SUMS in this folder - re-check with: sha256sum -c SHA256SUMS
MANIFEST
(cd "$D" && sha256sum -c --quiet SHA256SUMS) || fail "checksum re-verify failed in $D"
step "wrote $D (commit $GIT_REV, harness $HARNESS_VERSION)"

# --- optional copy to a directory (a mounted stick) ---------------------------
if [ -n "$TO" ]; then
    [ -d "$TO" ] || fail "--to $TO is not a directory (this script never writes to a device; mount the stick and point at its root)"
    cp -r "$D"/. "$TO"/
    sync
    (cd "$TO" && sha256sum -c --quiet SHA256SUMS) || fail "the copy at $TO does not verify against SHA256SUMS"
    step "copied the stick layout to $TO and re-verified every file"
fi
