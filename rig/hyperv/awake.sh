#!/usr/bin/env bash
#
# rig/hyperv/awake.sh: the product keeps the computer awake while it works,
# on the rig (roadmap item 16; RISKS R33).
#
#   rig/hyperv/awake.sh windows    the bench's Windows (UPGRIGHV, running)
#   rig/hyperv/awake.sh linux      a throwaway Fedora (made here, removed after)
#
# windows: guest/awake-win.ps1 runs the freshly built scanner and samples
# `powercfg /requests` while it runs: from a session with no desktop, and on
# the signed-in desktop. The guest has no sleep state, so the request being
# listed, and gone at exit, is all this side can show. Two rows.
#
# linux: a Gen 2 machine (UPGRIGAWAKE) around a differencing child of the V3
# bench's disk (vm/UPGRIGHV.vhdx: Fedora 42 with the OEMDRV run hook; the
# parent is never written), with settle-in (musl, as the kit builds it) on a
# fresh OEMDRV. guest/awake-linux.sh says what it does; the host signs in on
# the console so logind has an idle session to count, and presses a key when
# the guest's heartbeat stops (a suspended guest). Five rows.
#
# Rows go to docs/validation-results/r33-awake.csv. An arm whose control did
# not happen proves nothing and reads `not-shown`.
#
# What the rig cannot show (rule #5): a real idle sleep on Windows, a lid, a
# desktop's own power manager (GNOME's and KDE's are what slept on the
# Aspire), UPGRADE.exe's worker threads, the prologue's resume as SYSTEM.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT/rig/hyperv"
HARNESS=0.1.0
HV=/mnt/c/upgrade-rig/hv
CSV="$ROOT/docs/validation-results/r33-awake.csv"
OUT="$ROOT/rig/hyperv/artifacts/awake/$(date -u +%Y-%m-%d)"   # the UTC date
mkdir -p "$OUT" "$HV/tmp"
COMMIT=$(git -C "$ROOT" rev-parse --short HEAD)
[ -z "$(git -C "$ROOT" status --porcelain -- evaluate settle-in rig/hyperv/awake.sh rig/hyperv/guest/awake-win.ps1 rig/hyperv/guest/awake-linux.sh)" ] || COMMIT="$COMMIT-dirty"
pwsh() { powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "$1" < /dev/null 2>&1 | tr -d '\r\0'; }
VMPS() { powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$(wslpath -w vm.ps1)" "$@" < /dev/null 2>&1 | tr -d '\r'; }
wait_s() { pwsh "Start-Sleep $1" >/dev/null; }
[ -f "$CSV" ] || echo 'timestamp,harness,program,commit,machine,side,arm,expected,observed,control,result,notes' > "$CSV"
ZIG_DIR="$HOME/.local/opt/zig-linux-x86_64-0.13.0"
[ -x "$ZIG_DIR/zig" ] && export PATH="$ZIG_DIR:$PATH"

case "${1:-}" in
windows)
    VM="${UPG_RIG_VM:-UPGRIGHV}"
    (cd "$ROOT/evaluate/scan" && cargo test --locked --quiet >/dev/null 2>&1 && cargo zigbuild --locked --release --quiet --target x86_64-pc-windows-gnu) || { echo "awake: upgrade-scan does not test or build" >&2; exit 1; }
    CRED="\$c = New-Object PSCredential('rig', (ConvertTo-SecureString 'rig' -AsPlainText -Force));"
    pwsh "$CRED Invoke-Command -VMName $VM -Credential \$c -ScriptBlock { hostname }" | grep -q . || { echo "awake: $VM does not answer over PowerShell Direct" >&2; exit 1; }
    cp "$ROOT/target/x86_64-pc-windows-gnu/release/upgrade-scan.exe" guest/awake-win.ps1 "$HV/tmp/"
    pwsh "Copy-VMFile -Name $VM -SourcePath 'C:\\upgrade-rig\\hv\\tmp\\upgrade-scan.exe' -DestinationPath 'C:\\upgr39\\upgrade-scan.exe' -FileSource Host -CreateFullPath -Force" >/dev/null
    pwsh "$CRED Invoke-Command -VMName $VM -Credential \$c -FilePath 'C:\\upgrade-rig\\hv\\tmp\\awake-win.ps1'" > "$OUT/windows.out" || true
    grep -a '^AWAKEJSON=' "$OUT/windows.out" | tail -1 | cut -d= -f2- | base64 -d > "$OUT/windows.json" 2>/dev/null || true
    python3 - "$OUT/windows.json" "$CSV" "$HARNESS" "$COMMIT" "$VM" <<'PY'
import csv, datetime, json, re, sys
path, out, harness, commit, vm = sys.argv[1:6]
try: d = json.load(open(path, encoding='utf-8-sig'))
except Exception as e: d = None; err = str(e)
now = datetime.datetime.now(datetime.timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')
rows, bad = [], 0
for arm, key, want in (('no-desktop', 'NoDesktop', ['SYSTEM']), ('desktop', 'Desktop', ['DISPLAY', 'SYSTEM'])):
    why, notes = [], []
    if not d: why.append(f'the guest gave no result ({err})'); obs = ''
    else:
        a = d[key]; held, after = a.get('While') or [], a.get('After') or []
        obs = f"while it ran: {'+'.join(held) or 'nothing'} ({a.get('Samples')} samples); after it ended: {'+'.join(after) or 'nothing'}"
        for w in want:
            if w not in held: why.append(f'{w} was not requested while it ran')
        if after: why.append('a request was left after the program ended')
        if d.get('Before'): why.append('a request stood before the program started')
        if arm == 'desktop' and not d.get('DesktopRan'): why.append('the desktop run did not finish')
        if arm == 'no-desktop' and 'DISPLAY' not in held: notes.append('with no desktop Windows lists the system request only: the screen request is not taken')
        notes.append('this guest has no sleep state (powercfg /a), so no idle sleep was tried')
    ver = (re.search(r'\d+\.\d+\.\d+', (d or {}).get('ScanVersion', '')) or [''])[0]
    rows.append([now, harness, f'upgrade-scan {ver}', commit, f'the rig ({vm}, Windows 10, Hyper-V)', 'windows', arm, 'listed under ' + ' and '.join(want) + ' while it runs; nothing after', obs, 'none before it started' if d and not d.get('Before') else '', 'fail' if why else 'pass-plumbing', '; '.join(why + notes)])
    bad += bool(why); print(f"    {arm}: {'fail: ' + '; '.join(why) if why else 'pass-plumbing'}")
with open(out, 'a', newline='') as f: csv.writer(f, lineterminator='\n').writerows(rows)
sys.exit(1 if bad else 0)
PY
    ;;
linux)
    VM=UPGRIGAWAKE
    ( cd "$ROOT/settle-in" && cargo test --locked --quiet >/dev/null 2>&1 && cargo build --locked --release --quiet --target x86_64-unknown-linux-musl ) || { echo "awake: settle-in does not test or build" >&2; exit 1; }
    BIN="$ROOT/settle-in/target/x86_64-unknown-linux-musl/release/settle-in"
    IMG="$OUT/oemdrv.img"; rm -f "$IMG"; truncate -s 64M "$IMG"
    parted -s "$IMG" mklabel msdos mkpart primary fat32 1MiB 100%; mkfs.fat -F 32 -n OEMDRV --offset 2048 "$IMG" >/dev/null
    mmd -i "$IMG@@1M" ::/v3; mcopy -i "$IMG@@1M" guest/awake-linux.sh ::/run.sh; mcopy -i "$IMG@@1M" guest/oemdrv-run.sh ::/; mcopy -i "$IMG@@1M" "$BIN" ::/v3/settle-in
    printf 'poweroff\n' > "$OUT/next"; mcopy -i "$IMG@@1M" "$OUT/next" ::/next
    pwsh "if (Get-VM $VM -ErrorAction SilentlyContinue) { Stop-VM $VM -TurnOff -Force -ErrorAction SilentlyContinue; Remove-VM $VM -Force }; Remove-Item 'C:\\upgrade-rig\\hv\\vm\\$VM.diff.vhdx','C:\\upgrade-rig\\hv\\vm\\oemdrv-awake.vhdx' -Force -ErrorAction SilentlyContinue; exit 0" >/dev/null
    qemu-img convert -f raw -O vhdx -o subformat=dynamic "$IMG" "$HV/vm/oemdrv-awake.vhdx"
    pwsh "\$ErrorActionPreference = 'Stop'; \$d = 'C:\\upgrade-rig\\hv\\vm'
        New-VHD -Path \"\$d\\$VM.diff.vhdx\" -ParentPath \"\$d\\UPGRIGHV.vhdx\" -Differencing | Out-Null
        New-VM -Name $VM -Generation 2 -MemoryStartupBytes 2GB -VHDPath \"\$d\\$VM.diff.vhdx\" -Path \$d | Out-Null
        Set-VM $VM -AutomaticCheckpointsEnabled \$false -CheckpointType Disabled -ProcessorCount 2; Set-VMMemory $VM -DynamicMemoryEnabled \$false
        Set-VMFirmware $VM -EnableSecureBoot Off; Add-VMHardDiskDrive -VMName $VM -ControllerType SCSI -Path \"\$d\\oemdrv-awake.vhdx\"
        Set-VMFirmware $VM -FirstBootDevice (Get-VMHardDiskDrive $VM | Where-Object Path -like '*diff*')" | tail -3
    python3 evict-builds.py --wait | tail -1
    pwsh "Start-VM $VM" | tail -2
    t0=$(date +%s); el() { echo $(( $(date +%s) - t0 )); }
    # the console sign-in, once the lock is taken and D has begun (the script starts about 35 s in)
    while [ "$(el)" -lt 75 ]; do wait_s 5; done
    VMPS shot "C:\\upgrade-rig\\hv\\shots\\awake-login.png" -Name $VM >/dev/null || true
    VMPS type rig -Name $VM >/dev/null; VMPS key 13 -Name $VM >/dev/null; wait_s 3; VMPS type rig -Name $VM >/dev/null; VMPS key 13 -Name $VM >/dev/null
    echo "awake: signed in on the console at t+$(el) s"; wait_s 5
    VMPS shot "C:\\upgrade-rig\\hv\\shots\\awake-signed-in.png" -Name $VM >/dev/null || true
    WOKE=0; : > "$OUT/host.log"
    while :; do
        st=$(pwsh "\$v = Get-VM $VM; '' + \$v.State + ' ' + \$v.Heartbeat")
        echo "t+$(el) $st" >> "$OUT/host.log"
        case "$st" in Off*) break ;; esac
        if [ "$(el)" -ge 900 ]; then echo "awake: still running after 15 minutes; turned off" | tee -a "$OUT/host.log"; pwsh "Stop-VM $VM -TurnOff -Force" >/dev/null; break; fi
        # a suspended guest stops its heartbeat: one key press wakes it (never before D is over)
        case "$st" in Running\ Ok*) ;; *) if [ "$(el)" -ge 240 ]; then VMPS key 17 -Name $VM >/dev/null || true; WOKE=$((WOKE + 1)); echo "t+$(el) key pressed (heartbeat: $st)" >> "$OUT/host.log"; fi ;; esac
        wait_s 4
    done
    cp "$HV/shots/awake-login.png" "$HV/shots/awake-signed-in.png" "$OUT/" 2>/dev/null || true
    python3 -c 'import os,sys; fd=os.open(sys.argv[1],os.O_RDONLY); os.posix_fadvise(fd,0,0,os.POSIX_FADV_DONTNEED); os.close(fd)' "$HV/vm/oemdrv-awake.vhdx"
    qemu-img convert -f vhdx -O raw "$HV/vm/oemdrv-awake.vhdx" "$OUT/oemdrv-after.img"
    mtype -i "$OUT/oemdrv-after.img@@1M" ::/run.log > "$OUT/linux-run.log" 2>/dev/null || true
    rm -f "$OUT/oemdrv.img" "$OUT/oemdrv-after.img"
    pwsh "Remove-VM $VM -Force; Remove-Item 'C:\\upgrade-rig\\hv\\vm\\$VM.diff.vhdx','C:\\upgrade-rig\\hv\\vm\\oemdrv-awake.vhdx' -Force; Remove-Item 'C:\\upgrade-rig\\hv\\vm\\$VM' -Recurse -Force -ErrorAction SilentlyContinue; exit 0" >/dev/null
    python3 - "$OUT/linux-run.log" "$CSV" "$HARNESS" "$COMMIT" "$WOKE" <<'PY'
import csv, datetime, json, re, sys
path, out, harness, commit, woke = sys.argv[1:6]
k = {}
for l in open(path, errors='replace'):
    m = re.match(r'AWAKE (\w+)=(.*)', l.rstrip('\n'))
    if m: k[m.group(1)] = m.group(2).strip()
now = datetime.datetime.now(datetime.timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')
machine = f"the rig (a throwaway Hyper-V guest on the V3 bench's disk: {k.get('os', '?')}, {k.get('systemd', '?').split(' (')[0]}, sleep states '{k.get('power_states', '?')}')"
prog = k.get('settle_in', 'settle-in ?')
WHAT = 'sleep:idle:handle-lid-switch'
fl = lambda x: float(k.get(x, 'nan') or 'nan')
rows = []
def row(arm, expected, observed, control, why, shown=True, notes=''):
    result = 'fail' if why else ('pass-plumbing' if shown else 'not-shown')
    rows.append([now, harness, prog, commit, machine, 'linux', arm, expected, observed, control, result, '; '.join(why + ([notes] if notes else []))])
    print(f"    {arm}: {result}" + (': ' + '; '.join(why) if why else ''))
if 'done' not in k:
    row('all', 'the guest script runs to its end', f"last line: {list(k)[-1] if k else 'none'}", '', ['the guest script did not finish'])
else:
    # the record
    try: rec = json.loads(k.get('record_awake', '[]'))
    except Exception: rec = []
    why = []
    if not (rec and rec[0].get('ok') is True and rec[0].get('detail', {}).get('what') == WHAT): why.append("prepare's record does not say the lock was held")
    if '"stopped_at":"consent"' not in k.get('prepare_result', ''): why.append('prepare did not stop at its consent step')
    if k.get('listed_after_prepare'): why.append('a lock was left after prepare ended')
    row('prepare-record', f"the walk-away's prepare records an `awake` step, held, {WHAT}", f"record: {k.get('record_awake')}; prepare: {k.get('prepare_result')}", 'no lock listed once prepare ended' if not k.get('listed_after_prepare') else '', why, notes='prepare was stopped at consent on purpose (a wrong sentence): nothing was prepared')
    # listed
    why = []
    if WHAT not in k.get('listed', '') or ' block' not in k.get('listed', ''): why.append('logind does not list the lock as a block on sleep, idle and the lid')
    if k.get('helper') != '1': why.append('the helper that ties the lock to the process is not running')
    row('listed', 'logind lists upgrade_ blocking sleep, idle and the lid switch', f"{k.get('listed', '').strip('|')}; hold said {k.get('hold')}", '', why)
    # suspend refused
    why = []
    asked, sysctl = k.get('suspend_asked', ''), k.get('suspend_systemctl', '')
    if 'denied' not in asked.lower() and 'inhibit' not in asked.lower(): why.append('logind did not turn the suspend down')
    if 'inhibited by "upgrade_"' not in sysctl: why.append('systemctl did not name upgrade_ as what stops it')
    if not fl('gap_after_asking') < 1: why.append('the guest slept while the lock was held')
    ctl = fl('control_asked_asleep')
    row('suspend-refused', 'a suspend asked of logind is turned down while the lock is held', f"logind: {asked}; systemctl: {sysctl.split('. ')[0]}; asleep {k.get('gap_after_asking')} s", f"with no lock the same request put the guest to sleep for {k.get('control_asked_asleep')} s (answer: {k.get('control_asked') or 'none'})", why, shown=ctl > 1)
    # idle
    why = []
    if not fl('gap_after_150s_held') < 1: why.append('the guest slept while the lock was held')
    if WHAT not in k.get('still_listed', ''): why.append('the lock was gone before the 150 s were over')
    if 'suspend' not in k.get('idle_action', ''): why.append('logind did not take the 1-minute idle setting')
    ctl = k.get('control_idle', '')
    row('idle-1min', 'logind set to suspend after 1 idle minute; awake for 150 s with the lock held', f"asleep {k.get('gap_after_150s_held')} s over 150 s; sessions {k.get('sessions_while_held')}; logind said: {k.get('logind_while_held') or 'nothing'}", f"with no lock, same setting: {ctl} (idle hint {k.get('idle_hint_at_control')}; logind said: {(k.get('logind_control') or 'nothing').strip('|')})", why, shown=ctl.startswith('suspended'),
        notes=f"the host pressed a key {woke} time(s) to wake the guest" + ('' if ctl.startswith('suspended') else '; the control did not sleep, so staying awake here proves nothing'))
    # released
    why = []
    if k.get('listed_after_kill'): why.append('the lock outlived a killed settle-in')
    if k.get('helper_after_kill') != '0': why.append('the helper was left running')
    row('released-on-kill', 'settle-in killed (-9): the lock and its helper are gone within 3 s', f"listed after: {k.get('listed_after_kill') or 'nothing'}; helpers left: {k.get('helper_after_kill')}", f"listed before: {'yes' if WHAT in k.get('still_listed', '') else 'no'}", why)
with open(out, 'a', newline='') as f: csv.writer(f, lineterminator='\n').writerows(rows)
sys.exit(1 if any(r[10] == 'fail' for r in rows) else 0)
PY
    ;;
*) sed -n '3,6p' "$0"; exit 2 ;;
esac
echo "awake: rows appended to ${CSV#$ROOT/}; what the guest said is in ${OUT#$ROOT/}"
