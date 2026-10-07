#!/usr/bin/env bash
#
# Assemble the live-test USB kit into dist/kit/ - what actually goes on the
# stick for a physical V0 boot-handoff run - and verify every part of it
# before it leaves the tree.
#
#   ./make-kit.sh [--release ID] [--allow-dirty] [--to DIR]
#
# --release names an entry in data/releases.ps1 (default fedora-44). The kit
# carries that release's signed boot chain, installer and desktop images,
# fetched and checked against the table by rig/vm/fetch-release.sh, and says
# which release it is in release.json; the scanner and the job writer refuse
# it on a computer that cannot start it (decided 2026-10-03, the owner; R34).
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
#   3. every self-test passes on Windows PowerShell 5.1 - scanner,
#      harvester, handoff harness, materialization harness, stick writer,
#      kickstart generator, job writer, prologue - and schemas/check.py
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

ALLOW_DIRTY=0 TO= RELEASE=fedora-44
while [ $# -gt 0 ]; do
    case "$1" in
        --allow-dirty) ALLOW_DIRTY=1; shift ;;
        --to) TO=$2; shift 2 ;;
        --release) RELEASE=$2; shift 2 ;;
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
selftest "kickstart generator" "$ROOT/upgrade_/windows/New-Kickstart.ps1"
selftest "job writer" "$ROOT/evaluate/windows/New-Job.ps1"
selftest "password hasher" "$ROOT/evaluate/windows/Read-Password.ps1"
selftest "storage-mode harness (V5)" "$ROOT/evaluate/windows/Test-StorageMode.ps1"
selftest "prologue" "$ROOT/upgrade_/windows/Invoke-Prologue.ps1"
selftest "rollback" "$ROOT/upgrade_/windows/Invoke-Rollback.ps1"
bash -n "$ROOT/upgrade_/linux/verify.sh" || fail "verify.sh does not parse"
bash -n "$ROOT/upgrade_/linux/outcome.sh" || fail "outcome.sh does not parse"
# settle-in: the first-startup program, one self-contained file (Rust, static,
# decided 2026-09-27); its tests run before it is built into the kit
(cd "$ROOT/settle-in" && cargo test --locked --quiet >/dev/null 2>&1) || fail "settle-in tests fail (cd settle-in && cargo test)"
(cd "$ROOT/settle-in" && cargo build --locked --release --quiet --target x86_64-unknown-linux-musl) || fail "settle-in does not build"
SETTLE_IN_BIN="$ROOT/settle-in/target/x86_64-unknown-linux-musl/release/settle-in"
SETTLE_IN_VERSION=$("$SETTLE_IN_BIN" --version | awk '{print $2}')
(cd "$ROOT/settle-in/window" && cargo build --locked --release --quiet) || fail "the settle-in window does not build"
SETTLE_IN_WINDOW="$ROOT/settle-in/window/target/release/settle-in-window"
sh -n "$ROOT/settle-in/linux/upgrade_-settle-in.sh" || fail "the settle-in console hook does not parse"
# "Go back to Windows" carries wimlib-imagex (static, pinned source; built once into settle-in/target/wimlib/)
WIMLIB="$ROOT/settle-in/target/wimlib/wimlib-imagex"
[ -x "$WIMLIB" ] || "$ROOT/settle-in/tools/build-wimlib.sh" "$ROOT/settle-in/target/wimlib" >/dev/null || fail "wimlib-imagex does not build (settle-in/tools/build-wimlib.sh)"
# ... and, for the walk-away way back (R33, 2026-09-29), cabextract (Microsoft's
# catalog is an LZX cabinet; static, pinned source) and the gate (upgrade-gate.exe,
# Rust for Windows, started first in WinPE on the stick; its tests run first)
CABEXTRACT="$ROOT/settle-in/target/cabextract/cabextract"
[ -x "$CABEXTRACT" ] || "$ROOT/settle-in/tools/build-cabextract.sh" "$ROOT/settle-in/target/cabextract" >/dev/null || fail "cabextract does not build (settle-in/tools/build-cabextract.sh)"
GATE_EXE=$("$ROOT/settle-in/gate/build.sh") || fail "the gate (upgrade-gate.exe) does not build or its tests fail"
step "settle-in $SETTLE_IN_VERSION: tests pass, static build; window built"
grep -q 'boot-install' "$PAYLOAD/grub.cfg" || fail "grub.cfg lacks the boot-install branch"
# UPGRADE.exe: the window in front of the scripts (Rust, decided 2026-09-27);
# its tests run before it is built for Windows (upgrade_/windows/window/build.sh)
UPGRADE_EXE=$("$ROOT/upgrade_/windows/window/build.sh") || fail "the window (UPGRADE.exe) does not build or its tests fail"
# run a copy from Windows' temp folder: started from \\wsl.localhost it hung here
# and never ran (first kit build, 2026-09-27; likely Windows' prompt for a
# program on a network path), while copies in temp started every time
WINTMP=$(powershell.exe -NoProfile -Command '[IO.Path]::GetTempPath()' | tr -d '\r')
cp "$UPGRADE_EXE" "$(wslpath "$WINTMP")upgrade-window-probe.exe" || fail "could not copy UPGRADE.exe to Windows' temp folder"
timeout 60 powershell.exe -NoProfile -Command "\$p = Start-Process -FilePath '${WINTMP}upgrade-window-probe.exe' -ArgumentList '--version' -Wait -PassThru -WindowStyle Hidden; exit \$p.ExitCode" >/dev/null 2>&1 || fail "UPGRADE.exe does not start on Windows (--version)"
rm -f "$(wslpath "$WINTMP")upgrade-window-probe.exe"
step "UPGRADE.exe built, tests pass, starts on Windows"
# the Rust port's binaries (RISKS R32, the cut-over): beside the scripts on
# the stick until their ledger lines all read pass; the rig can be pointed at
# them with PROLOGUE=rust (rig/hyperv/prologue.sh, v9.sh)
RUST_EXES=$("$ROOT/build-rust.sh") || fail "the Rust binaries do not build (build-rust.sh)"
step "Rust binaries built: $(echo "$RUST_EXES" | xargs -n1 basename | tr '\n' ' ')"

# --- 4. parse-check under the PS 5.1 parser ----------------------------------
parsecheck() {
    local out
    out=$(PS -Command "\$t=\$null;\$e=\$null;[void][System.Management.Automation.Language.Parser]::ParseFile('$(wslpath -w "$1")',[ref]\$t,[ref]\$e); if (\$e.Count) { \$e | ForEach-Object { \$_.Message }; exit 1 } else { 'parse-ok' }")
    echo "$out" | grep -q 'parse-ok' || { echo "$out" >&2; fail "PS 5.1 parse error in $1"; }
    step "parses under PS 5.1: $(basename "$1")"
}
parsecheck "$SCANNER_DIST"
parsecheck "$HARNESS"
parsecheck "$ROOT/evaluate/windows/New-Job.ps1"
parsecheck "$HARVEST_SRC"
parsecheck "$ROOT/evaluate/windows/Read-Password.ps1"
parsecheck "$ROOT/upgrade_/windows/New-Kickstart.ps1"
parsecheck "$ROOT/upgrade_/windows/Invoke-Prologue.ps1"
parsecheck "$ROOT/upgrade_/windows/Invoke-Logged.ps1"

# --- .cmd launchers: an unescaped ( or ) in an echo inside an if-block closes
# the block early in cmd, and the lines after it run unconditionally. The
# data-loss launcher did exactly that on the Aspire (2026-09-17 and -20): it
# paused and exited right after writing the job (RISKS R18).
for c in "$PAYLOAD"/*.cmd "$ROOT"/evaluate/windows/usb-kit/*.cmd; do
    bad=$(awk '/^if .*\(\r?$/ {inb=1; next} /^\)/ {inb=0} inb && /echo.*[()]/ && !/\^[()]/ {print FILENAME":"NR": "$0}' "$c")
    [ -z "$bad" ] || fail "unescaped parenthesis inside an if-block (write ^( and ^)): $bad"
done
parsecheck "$ROOT/upgrade_/windows/Invoke-Rollback.ps1"
parsecheck "$ROOT/evaluate/windows/Test-StorageMode.ps1"
(cd "$ROOT" && python3 schemas/check.py >/dev/null) || fail "schemas/check.py failed - the contracts the prologue and outcome.sh write against are broken"
step "schemas check passed"

# --- 5. payload bits --------------------------------------------------------
[ -f "$BITS/Shell.efi" ] || fail "missing $BITS/Shell.efi - run rig/vm/fetch-payload-bits.sh"
# the release: its boot chain, installer and desktop images, each checked
# against data/releases.ps1 (fetch-release.sh refuses any other bytes)
"$ROOT/rig/vm/fetch-release.sh" "$RELEASE" >/dev/null || fail "release $RELEASE could not be fetched or does not match data/releases.ps1 (rig/vm/fetch-release.sh $RELEASE)"
REL="$ROOT/rig/vm/artifacts/releases/$RELEASE"
RBITS="$REL/bits"
RJ="$REL/release.json.table"
rq() { python3 -c "import json,sys; j=json.load(open('$RJ')); print($1)"; }
[ -n "$(rq "j.get('Installer') or ''")" ] || fail "release $RELEASE has no unattended installer in data/releases.ps1"
NETINST_SRC=$(rq "j['Netinst']['Url']")
# the shim payload's grub MUST be the install-media build (reads EFI/BOOT/grub.cfg)
grep -q 'save_env' <(strings -n 6 "$RBITS/EFI/BOOT/grubx64.efi") || fail "grubx64.efi lacks save_env - wrong GRUB build"
step "release $RELEASE ($(rq "j['Name']")): boot chain, installer and desktop images match data/releases.ps1"

# --- 6. the grubenv block ---------------------------------------------------
[ "$(stat -c %s "$PAYLOAD/grubenv")" = 1024 ] || fail "grubenv is not 1024 bytes"
[ "$(head -c 25 "$PAYLOAD/grubenv")" = "# GRUB Environment Block" ] || fail "grubenv lacks the GRUB header"
grep -q 'upg_fired' "$PAYLOAD/grub.cfg" || fail "grub.cfg does not set upg_fired"
grep -q 'boot-verify' "$PAYLOAD/grub.cfg" || fail "grub.cfg lacks the V1 boot-verify branch"
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
crlf "$PAYLOAD/RUN-VERIFY.cmd"      "$D/RUN-VERIFY.cmd"
cp "$UPGRADE_EXE"                   "$D/UPGRADE.exe"
for exe in $RUST_EXES; do cp "$exe" "$D/$(basename "$exe")"; done
crlf "$PAYLOAD/RUN-CONVERT.cmd"     "$D/RUN-CONVERT.cmd"
crlf "$PAYLOAD/RUN-PROBE.cmd"       "$D/RUN-PROBE.cmd"
crlf "$PAYLOAD/RUN-CONVERT-ACCEPTING-DATA-LOSS.cmd" "$D/RUN-CONVERT-ACCEPTING-DATA-LOSS.cmd"
crlf "$PAYLOAD/RUN-ERASE-AND-INSTALL.cmd" "$D/RUN-ERASE-AND-INSTALL.cmd"
crlf "$PAYLOAD/RUN-ERASE-AND-INSTALL-ACCEPTING-DATA-LOSS.cmd" "$D/RUN-ERASE-AND-INSTALL-ACCEPTING-DATA-LOSS.cmd"
cp "$ROOT/evaluate/windows/Read-Password.ps1" "$D/Read-Password.ps1"
crlf "$ROOT/evaluate/windows/usb-kit/DIAG-VOLUME.cmd" "$D/DIAG-VOLUME.cmd"
crlf "$ROOT/evaluate/windows/usb-kit/DIAG-SMART.cmd"  "$D/DIAG-SMART.cmd"
crlf "$ROOT/evaluate/windows/usb-kit/DIAG-SECUREBOOT.cmd" "$D/DIAG-SECUREBOOT.cmd"
cp "$ROOT/evaluate/windows/usb-kit/Diag-SecureBoot.ps1" "$D/Diag-SecureBoot.ps1"
crlf "$ROOT/evaluate/windows/usb-kit/RUN-STORAGE-MODE.cmd" "$D/RUN-STORAGE-MODE.cmd"
cp "$ROOT/evaluate/windows/Test-StorageMode.ps1" "$D/Test-StorageMode.ps1"
cp "$ROOT/evaluate/windows/usb-kit/Diag-Smart.ps1" "$D/Diag-Smart.ps1"
cp "$ROOT/upgrade_/windows/Invoke-Prologue.ps1" "$D/Invoke-Prologue.ps1"
cp "$ROOT/upgrade_/windows/Invoke-Logged.ps1"   "$D/Invoke-Logged.ps1"
crlf "$PAYLOAD/ROLLBACK.cmd"        "$D/ROLLBACK.cmd"
crlf "$PAYLOAD/CANCEL-CONVERSION.cmd" "$D/CANCEL-CONVERSION.cmd"
cp "$ROOT/upgrade_/windows/Invoke-Rollback.ps1" "$D/Invoke-Rollback.ps1"
cp "$ROOT/evaluate/windows/New-Job.ps1"        "$D/New-Job.ps1"
# the folder map (0.10.0): the job writer runs the harvester beside it, in its own process
cp "$HARVEST_SRC"                             "$D/Harvest-UpgradeState.ps1"
cp "$ROOT/upgrade_/windows/New-Kickstart.ps1"  "$D/New-Kickstart.ps1"
# signed payload: the product path, at the removable-media default location
cp "$RBITS/EFI/BOOT/BOOTX64.EFI" "$D/EFI/BOOT/BOOTX64.EFI"
cp "$RBITS/EFI/BOOT/grubx64.efi" "$D/EFI/BOOT/grubx64.efi"
cp "$PAYLOAD/grub.cfg"  "$D/EFI/BOOT/grub.cfg"
cp "$PAYLOAD/grubenv"   "$D/EFI/BOOT/grubenv"
# unsigned payload: the matrix rows (Test-Handoff.ps1 -Payload shell)
cp "$BITS/Shell.efi"    "$D/EFI/SHELL/SHELLX64.EFI"
cp "$PAYLOAD/startup.nsh" "$D/startup.nsh"
# V1: the unmodified Fedora installer boot files, and the %pre verifier.
# grub.cfg boots them only when upgrade_/boot-verify exists on the stick
# (the rig's v1.sh puts it there beside job.json + ks.cfg); without it the
# stick is the V0 marker stick, unchanged.
mkdir -p "$D/images/pxeboot" "$D/upgrade_"
cp "$RBITS/images/pxeboot/vmlinuz"    "$D/images/pxeboot/vmlinuz"
cp "$RBITS/images/pxeboot/initrd.img" "$D/images/pxeboot/initrd.img"
cp "$RBITS/images/install.img"        "$D/images/install.img"
sed 's/\r$//' "$ROOT/upgrade_/linux/verify.sh"  > "$D/upgrade_/verify.sh"
sed 's/\r$//' "$ROOT/upgrade_/linux/outcome.sh" > "$D/upgrade_/outcome.sh"
# settle-in and its service; outcome.sh installs them only if their checksums match SHA256SUMS
mkdir -p "$D/upgrade_/settle-in"
cp "$SETTLE_IN_BIN" "$D/upgrade_/settle-in/settle-in"
sed 's/\r$//' "$ROOT/settle-in/linux/upgrade_-settle-in.service" > "$D/upgrade_/settle-in/upgrade_-settle-in.service"
cp "$SETTLE_IN_WINDOW" "$D/upgrade_/settle-in/settle-in-window"
sed 's/\r$//' "$ROOT/settle-in/linux/upgrade_-settle-in.desktop" > "$D/upgrade_/settle-in/upgrade_-settle-in.desktop"
sed 's/\r$//' "$ROOT/settle-in/linux/upgrade_-settle-in.sh" > "$D/upgrade_/settle-in/upgrade_-settle-in.sh"
sed 's/\r$//' "$ROOT/settle-in/linux/org.upgrade.settle-in.policy" > "$D/upgrade_/settle-in/org.upgrade.settle-in.policy"
cp "$WIMLIB" "$D/upgrade_/settle-in/wimlib-imagex"
cp "$CABEXTRACT" "$D/upgrade_/settle-in/cabextract"
cp "$GATE_EXE" "$D/upgrade_/settle-in/upgrade-gate.exe"
sed 's/\r$//' "$ROOT/settle-in/linux/upgrade_-go-back-to-windows.desktop" > "$D/upgrade_/settle-in/upgrade_-go-back-to-windows.desktop"
# the desktops: Fedora's own live squashfs images, unmodified, one per
# desktop the intent capture offers (rig/vm/fetch-desktops.sh). The
# kickstart's liveimg line names one of them; verify.sh reads it back
# against SHA256SUMS in the live session before anything is installed.
mkdir -p "$D/upgrade_/LiveOS"
for d in gnome kde; do
    [ -f "$RBITS/LiveOS/$d.squashfs" ] || fail "release $RELEASE has no '$d' desktop image (the launchers offer gnome and kde)"
    cp "$RBITS/LiveOS/$d.squashfs" "$D/upgrade_/LiveOS/$d.squashfs"
done
# which release this stick carries (the job writer reads it and refuses one
# the scan found this computer cannot start), and the installed system's boot
# files' SBAT facts from the table (the scanner judges them; the images they
# came from are pinned by sha256 above)
python3 - "$RJ" "$D" <<'PY'
import json, os, sys
j = json.load(open(sys.argv[1])); d = sys.argv[2]
rel = {'id': j['Id'], 'name': j['Name'], 'family': j['Family'], 'release': j['Id'].rsplit('-', 1)[-1], 'measured': j['Measured']}
open(os.path.join(d, 'release.json'), 'w').write(json.dumps(rel, indent=2) + '\n')
bc = os.path.join(d, 'upgrade_', 'boot-chain'); os.makedirs(bc, exist_ok=True)
for b in j['Boot'] if isinstance(j['Boot'], list) else [j['Boot']]:
    if not b['Role'].startswith('installed '): continue
    name = b['Role'].split(' ', 1)[1] + '-' + os.path.splitext(b['File'])[0]
    open(os.path.join(bc, name + '.sbat'), 'w').write(b['Sbat'] + '\n')
    lv = b['SbatLevel'] if isinstance(b['SbatLevel'], list) else ([b['SbatLevel']] if b['SbatLevel'] else [])
    if lv: open(os.path.join(bc, name + '.sbatlevel'), 'w').write('\n'.join(lv) + '\n')
PY

# --- manifest + checksums -----------------------------------------------------
(cd "$D" && find . -type f ! -name SHA256SUMS ! -name KIT-MANIFEST.txt | sort | xargs sha256sum > SHA256SUMS)
crlf /dev/stdin "$D/KIT-MANIFEST.txt" <<MANIFEST
upgrade_ live-test kit  -  V0 boot-handoff stick (both payloads)
built:            $(date -u +%Y-%m-%dT%H:%M:%SZ)
commit:           $GIT_REV ($GIT_STATE)
harness:          Test-Handoff.ps1 $HARNESS_VERSION
window:           UPGRADE.exe $(grep -m1 '^version' "$ROOT/upgrade_/windows/window/Cargo.toml" | cut -d'"' -f2)  $(sha256sum "$UPGRADE_EXE" | cut -c1-64)   (verify flow; RUN-VERIFY.cmd stays as the fallback)
settle-in:        $SETTLE_IN_VERSION  $(sha256sum "$SETTLE_IN_BIN" | cut -c1-64)   -> upgrade_/settle-in/settle-in
scanner:          upgrade-scan.ps1 $SCANNER_VERSION (single-file build of evaluate/windows + data/)
release:          $RELEASE ($(rq "j['Name']"), measured $(rq "j['Measured']")) - data/releases.ps1; release.json on this stick
  Shell.efi       $(sha256sum "$BITS/Shell.efi" | cut -c1-64)   -> EFI/SHELL/SHELLX64.EFI
  shim            $(sha256sum "$RBITS/EFI/BOOT/BOOTX64.EFI" | cut -c1-64)   -> EFI/BOOT/BOOTX64.EFI
  grub            $(sha256sum "$RBITS/EFI/BOOT/grubx64.efi" | cut -c1-64)   -> EFI/BOOT/grubx64.efi
  install.img     $(sha256sum "$RBITS/images/install.img" | cut -c1-64)   -> images/install.img (+ pxeboot vmlinuz, initrd.img)
  from installer: $NETINST_SRC
$(rq "chr(10).join('  %-15s %s  <- %s' % (x['Id'] + ' image', x['ImageSha256'], x['Url']) for x in (j['Desktops'] if isinstance(j['Desktops'], list) else [j['Desktops']]))")
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
