#!/usr/bin/env bash
#
# rig/fetch/arms.sh: the release fetch's arms on this Windows machine
# (roadmap item 24, RISKS R44, VALIDATION V17).
#
#   rig/fetch/arms.sh [ARM ...]      (default: every arm, in order)
#
# 0.2.0 (2026-11-08): the sizes are asked with HEAD (the code review), so a
# --cut list counts GETs only.
#
# Builds upgrade-fetch.exe, copies it to Windows' temp folder (programs
# started from \\wsl.localhost hang; make-kit.sh, 2026-09-27), serves the
# rig's own Fedora ISOs from rig/vm/artifacts/releases/<id>/iso with
# rig/fetch/serve.py, and runs `upgrade-fetch rehearse`: the whole `get`, with
# Windows' own WinHTTP, into %TEMP%\upgrade_-fetch-rehearsal. Each arm puts
# one fault in the server and says what the program must do. One row per arm
# goes to docs/validation-results/v17-fetch.csv; the logs go to
# rig/fetch/artifacts/<date>/ (gitignored: they name this machine's folders).
#
# A rehearsal is plumbing (rule #5): the network path, the resume, the
# refusals and the files on an NTFS folder. A real stick (FAT32, a cheap
# stick's own speed and drops) and the real internet are other rows.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
# 0.3.0 (2026-11-13): Ubuntu's arms (roadmap item 23): one ISO taken whole. Until
# Ubuntu went into data/releases.json (2026-11-13) its arms handed the program a table
# (`rehearse --table`); now it reads the kit's own. Each file that lands is also compared
# with the ISO's own files as bsdtar reads them, not only with the program's record.
HARNESS=0.3.0
RELEASE=fedora-44
ISO="${UPG_RELEASE_ISOS:-$ROOT/rig/vm/artifacts/releases/$RELEASE/iso}"
[ -d "$ISO" ] || ISO="$(git -C "$ROOT" rev-parse --git-common-dir)/../rig/vm/artifacts/releases/$RELEASE/iso"
[ -f "$ISO/Fedora-Server-netinst-x86_64-44-1.7.iso" ] || { echo "arms: the rig's Fedora 44 ISOs are not at $ISO (rig/vm/fetch-release.sh $RELEASE)" >&2; exit 1; }
CSV="$ROOT/docs/validation-results/v17-fetch.csv"
OUT="$ROOT/rig/fetch/artifacts/$(date -u +%Y-%m-%d)"
mkdir -p "$OUT"
PORT=8044
ZIG_DIR="$HOME/.local/opt/zig-linux-x86_64-0.13.0"
[ -x "$ZIG_DIR/zig" ] && export PATH="$ZIG_DIR:$PATH"
(cd "$ROOT/evaluate/fetch" && cargo test --locked --quiet >/dev/null 2>&1) || { echo "arms: upgrade-fetch's tests fail" >&2; exit 1; }
(cd "$ROOT/evaluate/fetch" && cargo zigbuild --locked --release --quiet --target x86_64-pc-windows-gnu) || { echo "arms: upgrade-fetch does not build for Windows" >&2; exit 1; }
EXE="$ROOT/target/x86_64-pc-windows-gnu/release/upgrade-fetch.exe"
WINTMP=$(powershell.exe -NoProfile -Command '[IO.Path]::GetTempPath()' | tr -d '\r')
T=$(wslpath "$WINTMP")
cp "$EXE" "$T/upgrade-fetch.exe"
VERSION=$("$T/upgrade-fetch.exe" --version | tr -d '\r' | awk '{print $2}')
COMMIT=$(git -C "$ROOT" rev-parse --short HEAD)$(git -C "$ROOT" diff --quiet HEAD -- evaluate/fetch rig/fetch || echo "-dirty")
REH="$T/upgrade_-fetch-rehearsal"
[ -f "$CSV" ] || echo "timestamp,harness,upgrade_fetch,commit,machine,release,desktop,arm,fault,expected,exit,outcome,staging_left,stick_after,files_verified,result,notes" > "$CSV"

# name | server faults | desktop | expected: ready, already, or refused:<words the refusal must carry>
ARMS=(
  "fresh|--cut 300000000,999999999999,1500000000|kde|ready"
  "again||kde|already"
  "byte-changed|--flip 2000000000|gnome|refused:did not match its measured sha256"
  "no-length|--no-length|gnome|refused:did not say how big"
  "no-server|NOSERVER|kde|refused:could not reach"
  "range-ignored|--no-range --cut 500000000|kde|ready"
)
# the same four fields, then the release; these run only when named, and need the rig's Ubuntu ISO
UBUNTU=ubuntu-26.04.1
UISO="$(git -C "$ROOT" rev-parse --git-common-dir)/../rig/vm/artifacts/$UBUNTU"
ARMS_UBUNTU=(
  "ubuntu-fresh|--cut 300000000,2500000000|gnome|ready|$UBUNTU"
  "ubuntu-again||gnome|already|$UBUNTU"
  "ubuntu-byte-changed|--flip 2000000000|gnome|refused:did not match its measured sha256|$UBUNTU"
  "fedora-after-ubuntu||kde|ready|$RELEASE"
)
want_arms=("$@")
for w in "${want_arms[@]}"; do for u in "${ARMS_UBUNTU[@]}"; do [ "${u%%|*}" = "$w" ] && ARMS+=("$u"); done; done
iso_sums() {  # the ISO's own files, read by bsdtar, as sha256sum lines (once; kept beside the logs)
  # (the Ubuntu rig's own extracted copy if rig/hyperv/ubuntu-arms.sh kit made one, else a fresh one)
  local out="$ROOT/rig/fetch/artifacts/$UBUNTU.sha256" x="$ROOT/rig/hyperv/artifacts/ubuntu/iso-root" tmp=
  if [ ! -s "$out" ]; then
    [ -f "$x/casper/vmlinuz" ] || { tmp=$(mktemp -d); x="$tmp"; bsdtar -xf "$UISO"/*.iso -C "$x"; chmod -R u+rwX "$x"; }
    (cd "$x" && find . -type f | LC_ALL=C sort | xargs -d '\n' sha256sum) > "$out"
    [ -z "$tmp" ] || rm -rf "$tmp"
  fi
  echo "$out"
}

csvq() { local s=${1//\"/\"\"}; printf '"%s"' "$s"; }
for a in "${ARMS[@]}"; do
  IFS='|' read -r name faults desktop expected release <<< "$a"
  release=${release:-$RELEASE}; serve="$ISO"; table=()
  if [ "$release" = "$UBUNTU" ]; then
    ls "$UISO"/*.iso >/dev/null 2>&1 || { echo "arms: the rig's Ubuntu ISO is not at $UISO" >&2; exit 1; }
    serve="$UISO"
    : # in the kit's own table from 2026-11-13: no --table
  fi
  if [ ${#want_arms[@]} -gt 0 ] && [[ ! " ${want_arms[*]} " =~ " $name " ]]; then continue; fi
  { [ "$name" = fresh ] || [ "$name" = ubuntu-fresh ]; } && rm -rf "$REH"
  from="http://localhost:$PORT"
  pid=
  if [ "$faults" = NOSERVER ]; then
    from="http://localhost:$((PORT + 1))"
  else
    # shellcheck disable=SC2086
    python3 "$ROOT/rig/fetch/serve.py" "$serve" --port "$PORT" $faults > "$OUT/serve-$name.log" 2>&1 &
    pid=$!
    sleep 1
  fi
  set +e
  (cd "$T" && timeout 5400 ./upgrade-fetch.exe rehearse --release "$release" --desktop "$desktop" --from "$from" "${table[@]}") > "$OUT/fetch-$name.log" 2>&1
  code=$?
  set -e
  [ -n "$pid" ] && { kill "$pid" 2>/dev/null || true; wait "$pid" 2>/dev/null || true; }
  log=$(tr -d '\r' < "$OUT/fetch-$name.log")
  if echo "$log" | grep -q '^fetch: ready'; then outcome=ready
  elif echo "$log" | grep -q 'already carries it'; then outcome=already
  elif echo "$log" | grep -q '^REFUSED'; then outcome=refused
  else outcome=error; fi
  refusal=$(echo "$log" | grep -m1 '^REFUSED' | sed 's/^REFUSED: //' || true)
  staging=$([ -d "$REH/upgrade_/fetch" ] && echo y || echo n)
  after=$(python3 -c "import json,sys; j=json.load(open(sys.argv[1])); print(j['id']+'/'+j.get('desktop',''))" "$REH/release.json" 2>/dev/null || echo none)
  # every file SHA256SUMS names reads back as it says, checked here from Linux (not the program's own word)
  verified=$( (cd "$REH" && [ -s SHA256SUMS ] && sha256sum -c --quiet SHA256SUMS >/dev/null 2>&1 && echo y) || echo n)
  # Ubuntu on the stick: every file of the ISO but its own start menu is there, byte for byte as
  # bsdtar reads it from the ISO, and nothing else of a disc is. After Fedora replaced it: none is left.
  disc=
  if [ "$release" = "$UBUNTU" ] && [ "$expected" = ready ]; then
    want=$(grep -v ' \./boot/grub/grub\.cfg$' "$(iso_sums)" | sed 's#  \./EFI/boot/#  ./EFI/BOOT/#' | LC_ALL=C sort -k2)
    got=$(cd "$REH" && grep -E '  \./(casper|pool|dists|boot|\.disk|EFI/BOOT)/|  \./(md5sum\.txt|boot\.catalog)$' SHA256SUMS | grep -v -e ' \./EFI/BOOT/grub\.cfg$' -e ' \./EFI/BOOT/grubenv$' | LC_ALL=C sort -k2)
    if [ "$want" = "$got" ] && [ -n "$got" ]; then disc="; $(echo "$got" | wc -l) files, each the ISO's own as bsdtar reads it"; else disc="; THE FILES ARE NOT THE ISO'S"; verified=n; fi
  fi
  if [ "$name" = fedora-after-ubuntu ]; then
    left=$( (cd "$REH" && find casper pool dists boot .disk md5sum.txt boot.catalog -type f 2>/dev/null | wc -l) || true)
    disc="; ${left:-0} of Ubuntu's files left"; [ "${left:-0}" = 0 ] || verified=n
  fi
  ok=fail
  case "$expected" in
    ready) [ "$outcome" = ready ] && [ "$code" = 0 ] && [ "$staging" = n ] && [ "$verified" = y ] && [ "$after" = "$release/$desktop" ] && ok=pass-plumbing ;;
    already) [ "$outcome" = already ] && [ "$code" = 0 ] && ! grep -qE '^(GET|HEAD)' "$OUT/serve-$name.log" && ok=pass-plumbing ;;
    refused:*) [ "$outcome" = refused ] && [ "$code" != 0 ] && [ "$staging" = n ] && [[ "$refusal" == *"${expected#refused:}"* ]] && ok=pass-plumbing ;;
  esac
  gets=$(grep -c '^GET' "$OUT/serve-$name.log" 2>/dev/null || true)
  ranges=$(grep -c 'bytes=' "$OUT/serve-$name.log" 2>/dev/null || true)
  drops=$(echo "$log" | grep -c 'connection dropped' || true)
  notes="requests ${gets:-0}, ranged ${ranges:-0}; ${drops:-0} drops survived${disc}${refusal:+; refused: $refusal}"
  notes=$(echo "$notes" | sed -E 's#C:\\Users\\[^\\]+#C:\\Users\\<name>#g')
  echo "$(date -u +%Y-%m-%dT%H:%M:%SZ),$HARNESS,$VERSION,$COMMIT,$(csvq "rehearsal on the dev machine (WinHTTP, NTFS temp folder)"),$release,$desktop,$name,$(csvq "${faults:-none}"),$(csvq "$expected"),$code,$outcome,$staging,$after,$verified,$ok,$(csvq "$notes")" >> "$CSV"
  echo "arms: $name -> $outcome (exit $code), expected $expected: $ok"
done
