#!/usr/bin/env bash
#
# rig/hyperv/r39.sh: a Linux already on the Windows disk, on the rig (RISKS R39).
#
#   rig/hyperv/r39.sh [ARM ...]     (default: every arm, in order)
#
# The bench's Windows (UPGRIGHV, running, PowerShell Direct answering) gets
# the freshly built upgrade-scan.exe and upgrade-job.exe in C:\upgr39. Each
# arm plants a sign of a Linux on the disk Windows starts from, runs the two
# real programs, and takes the plant away (guest/r39-arm.ps1 says how):
#
#   none       nothing planted                     both programs silent about it
#   partition  a Linux-type partition              the scanner warns, the job writer refuses, each naming it
#   folder     EFI\fedora on the boot partition    the same
#   both       both                                the same, naming both
#   erase      both, and the erase sentence        the scanner warns; the job writer does NOT refuse for it
#   removed    both planted, then taken away       silent again
#
# One row per arm goes to docs/validation-results/r39-other-linux.csv; what
# the guest said goes to rig/hyperv/artifacts/r39/<date>/ (gitignored).
#
# What the rig cannot show (rule #5): a job WRITTEN on a disk with no Linux.
# The job writer refuses the rig's SCSI "stick" in every arm (Hyper-V has no
# USB, R16). The arms read whether R39's refusal stands beside that one.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
HARNESS=0.1.0
VM="${UPG_RIG_VM:-UPGRIGHV}"
CSV="$ROOT/docs/validation-results/r39-other-linux.csv"
OUT="$ROOT/rig/hyperv/artifacts/r39/$(date -u +%Y-%m-%d)"
HOSTTMP=/mnt/c/upgrade-rig/hv/tmp
mkdir -p "$OUT" "$HOSTTMP"
ARMS=("$@"); [ ${#ARMS[@]} -gt 0 ] || ARMS=(none partition folder both erase removed)
ZIG_DIR="$HOME/.local/opt/zig-linux-x86_64-0.13.0"
[ -x "$ZIG_DIR/zig" ] && export PATH="$ZIG_DIR:$PATH"
for c in evaluate/scan evaluate/job; do
    (cd "$ROOT/$c" && cargo test --locked --quiet >/dev/null 2>&1) || { echo "r39: tests fail in $c" >&2; exit 1; }
    (cd "$ROOT/$c" && cargo zigbuild --locked --release --quiet --target x86_64-pc-windows-gnu) || { echo "r39: $c does not build for Windows" >&2; exit 1; }
done
COMMIT=$(git -C "$ROOT" rev-parse --short HEAD)
[ -z "$(git -C "$ROOT" status --porcelain -- evaluate rig/hyperv/r39.sh rig/hyperv/guest/r39-arm.ps1)" ] || COMMIT="$COMMIT-dirty"
pwsh() { powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "$1" 2>&1 | tr -d '\r\0'; }
CRED="\$c = New-Object PSCredential('rig', (ConvertTo-SecureString 'rig' -AsPlainText -Force));"
pwsh "$CRED Invoke-Command -VMName $VM -Credential \$c -ScriptBlock { hostname }" | grep -q . || { echo "r39: $VM does not answer over PowerShell Direct (start it, wait for the desktop)" >&2; exit 1; }
for f in upgrade-scan upgrade-job upgrade-harvest; do
    [ -f "$ROOT/target/x86_64-pc-windows-gnu/release/$f.exe" ] || (cd "$ROOT/evaluate/${f#upgrade-}" && cargo zigbuild --locked --release --quiet --target x86_64-pc-windows-gnu)
    cp "$ROOT/target/x86_64-pc-windows-gnu/release/$f.exe" "$HOSTTMP/$f.exe"
    pwsh "Copy-VMFile -Name $VM -SourcePath 'C:\\upgrade-rig\\hv\\tmp\\$f.exe' -DestinationPath 'C:\\upgr39\\$f.exe' -FileSource Host -CreateFullPath -Force" >/dev/null
done
cp "$ROOT/rig/hyperv/guest/r39-arm.ps1" "$HOSTTMP/r39-arm.ps1"
[ -f "$CSV" ] || echo 'timestamp,harness,upgrade_scan,upgrade_job,commit,machine,arm,planted,expected,scan_line,job_refusal,job_read,job_written,layout_restored,result,notes' > "$CSV"
FAILED=0
for arm in "${ARMS[@]}"; do
    echo "r39: arm $arm"
    pwsh "$CRED Invoke-Command -VMName $VM -Credential \$c -FilePath 'C:\\upgrade-rig\\hv\\tmp\\r39-arm.ps1' -ArgumentList '$arm'" > "$OUT/$arm.out" || true
    grep -a '^R39JSON=' "$OUT/$arm.out" | tail -1 | cut -d= -f2- | base64 -d > "$OUT/$arm.json" 2>/dev/null || true
    python3 - "$arm" "$OUT/$arm.json" "$CSV" "$HARNESS" "$COMMIT" "$VM" <<'PY' || FAILED=$((FAILED + 1))
import csv, datetime, json, re, sys
arm, path, out, harness, commit, vm = sys.argv[1:7]
MARK = "a Linux is already on this computer's Windows disk"
plant = {'none': [], 'partition': ['partition'], 'folder': ['folder'], 'both': ['partition', 'folder'], 'erase': ['partition', 'folder'], 'removed': []}[arm]
expected = {'none': 'silent', 'removed': 'silent', 'erase': 'scanner warns; no refusal for it'}.get(arm, 'scanner warns; refused by name')
why, notes = [], []
try:
    d = json.load(open(path, encoding='utf-8-sig'))
except Exception as e:
    d = None; why.append(f'the guest gave no result ({e})')
row = dict(scan_line='', job_refusal='', job_read='', job_written='', layout_restored='')
ver = lambda s: (re.search(r'\d+\.\d+\.\d+', s or '') or [''])[0] if s else ''
if d:
    line = d.get('ScanLine'); detail = (line or {}).get('Detail', '')
    refusal = next((l.strip(' -') for l in (d.get('JobOut') or '').splitlines() if MARK in l), '')
    facts = d.get('Facts') or {}
    parts = [p for p in (facts.get('LinuxPartitions') or []) if p]; dirs = [f for f in (facts.get('LinuxBootFolders') or []) if f]
    row.update(scan_line=(f"{line.get('Status')}: {detail}" if line else 'none'), job_refusal=(refusal.split(';')[0] + ')' if refusal and ';' in refusal.split(')')[0] else refusal.split(' a second one')[0].rstrip('; ') if refusal else 'none'),
               job_read=f"partitions {[p.get('Number') for p in parts]}, folders {dirs}, error {facts.get('OtherLinuxError')}", job_written='y' if d.get('JobWritten') else 'n')
    n = d.get('PlantedPartition')
    names = ([f'partition {n} (Linux filesystem'] if 'partition' in plant else []) + (['EFI\\fedora'] if 'folder' in plant else [])
    if d.get('Facts') is None: why.append('the job writer gave no facts')
    if facts.get('OtherLinuxError'): why.append(f"the job writer could not read the disk: {facts.get('OtherLinuxError')}")
    if plant:
        if not line or line.get('Status') != 'warn': why.append('the scanner did not warn')
        for x in names:
            if x not in detail: why.append(f'the scanner did not name {x}')
        if ('partition' in plant) != bool(parts) or ('folder' in plant) != (dirs == ['fedora']): why.append('the job writer read something other than what was planted')
        if arm == 'erase':
            if refusal: why.append('the erase was refused for the old Linux')
        else:
            if not refusal: why.append("the job writer did not refuse for it")
            for x in names:
                if x not in refusal: why.append(f'the refusal did not name {x}')
    else:
        if line: why.append('the scanner warned with nothing planted')
        if refusal: why.append('the job writer refused with nothing planted')
        if parts or dirs: why.append('the job writer read a Linux with nothing planted')
    if d.get('JobWritten'): notes.append('a job was written')
    elif arm != 'erase' and not plant: notes.append("no job either way: the rig's stick is not on USB (R16) and PowerShell Direct is not the desktop")
    key = lambda l: (sorted(p.rsplit(':', 1)[0] for p in l['Partitions']), l['EspFolders'])
    same = key(d['LayoutBefore']) == key(d['LayoutAfter']); row['layout_restored'] = 'y' if same else 'n'
    if not same: why.append('the bench was not left as it was found')
    if plant and key(d['LayoutAtRun']) == key(d['LayoutBefore']): why.append('nothing was planted')
    if arm == 'removed': notes.append('both planted and taken away before the programs ran')
    others = [l.strip(' -')[:40] for l in (d.get('JobOut') or '').splitlines() if l.startswith('    - ') and MARK not in l]
    if others: notes.append(f'{len(others)} bench refusal(s) stand in every arm')
result = 'fail' if why else 'pass-plumbing'
with open(out, 'a', newline='') as f:
    csv.writer(f, lineterminator='\n').writerow([datetime.datetime.now(datetime.timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ'), harness, ver(d and d.get('ScanVersion')), ver(d and d.get('JobVersion')), commit,
        f'the rig ({vm}, Windows 10, Hyper-V)', arm, '+'.join(plant) or 'nothing', expected, row['scan_line'], row['job_refusal'], row['job_read'], row['job_written'], row['layout_restored'], result, '; '.join(why + notes)])
print(f"    {result}" + (': ' + '; '.join(why) if why else ''))
sys.exit(1 if why else 0)
PY
done
echo "r39: rows appended to ${CSV#$ROOT/}; guest output in ${OUT#$ROOT/}"
[ "$FAILED" = 0 ]
