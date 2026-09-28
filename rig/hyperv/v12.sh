#!/usr/bin/env bash
#
# V12 - the window in front of the scripts (UPGRADE.exe), verify flow, on the
# Hyper-V rig (RISKS R31). The window is started in the signed-in session and
# driven with the guest's keyboard: Tab to "Start the test", Enter. From
# there the product runs itself: scan -> job -> kickstart -> its own reopen
# task -> Test-Handoff's arm -> restart -> the stick's live boot (verify.sh,
# upg.mode=verify) -> back to Windows -> autologon -> the return check and
# the window's reopen fire at the same sign-in -> the window shows the
# result. The verdict reads the stick's convert.log (the window's own lines),
# verify.json, the return check's row and what is left in the guest, and
# writes one row of docs/validation-results/v12-window.csv. Nothing is
# installed; the internal disk is not touched.
#
#   v12.sh stick         VM off: the kit's stick with NO boot-verify marker (the window writes it)
#   v12.sh windows       power on, wait for PS Direct and the autologon session
#   v12.sh start         launch F:\UPGRADE.exe in the signed-in session (a one-shot task, elevated:
#                        the UAC click is not on this path), screenshot, Tab + Enter, screenshot
#   v12.sh wait [secs]   poll until the window has logged "after the restart:" on the stick
#   v12.sh close         Tab + Enter on the result window's Close, then check what it left behind
#   v12.sh verdict       pull the evidence over PS Direct -> v12-verdict.py -> one row
#   v12.sh run           stick -> windows -> start -> wait -> close -> verdict
#
# Autologon stands in for the person signing in after the restart. A green
# row closes plumbing only (CLAUDE.md rule #5): the Aspire row is still owed.
set -euo pipefail
SELF=$(readlink -f "${BASH_SOURCE[0]}")
cd "$(dirname "$SELF")"
VMNAME=${VMNAME:-UPGRIGHV}
A=artifacts/v12
HV=/mnt/c/upgrade-rig/hv
CSV=../../docs/validation-results/v12-window.csv
FIRMWARE='Hyper-V UEFI Release v4.1'
LAUNCH_TASK='upgrade_ v12 launch'

VMPS1="$(wslpath -w vm.ps1)"
PS()  { powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$VMPS1" "$@" -Name "$VMNAME" < /dev/null; }
PSC() { powershell.exe -NoProfile -Command "$1" < /dev/null; }
vm_state() { PSC "(Get-VM $VMNAME).State" | tr -d '\r\n '; }
guest() { PS ps "$1" 2>&1 | tr -d '\r'; }
shot() { PS shot "C:\\upgrade-rig\\hv\\shots\\v12-$1.png" >/dev/null 2>&1 || true; mkdir -p "$A"; cp "$HV/shots/v12-$1.png" "$A/" 2>/dev/null || true; }
stick_letter() { guest '(Get-Volume -FileSystemLabel UPGV0 -ErrorAction SilentlyContinue | Select-Object -First 1).DriveLetter' | tr -d ' \n'; }
wait_windows() {
    t0=$(date +%s)
    while :; do
        if guest 'hostname' 2>/dev/null | grep -qx 'UPGRIGHV'; then echo "v12: Windows up (PS Direct) after $(( $(date +%s) - t0 )) s"; return 0; fi
        [ $(( $(date +%s) - t0 )) -ge "${1:-900}" ] && { shot windows-stuck; echo "v12: Windows did not answer within ${1:-900} s" >&2; return 2; }
        sleep 10
    done
}
wait_session() {
    t0=$(date +%s)
    while :; do
        if guest 'quser 2>&1' | grep -q 'rig .*Active'; then echo "v12: rig is signed in"; return 0; fi
        [ $(( $(date +%s) - t0 )) -ge 300 ] && { shot session-stuck; echo "v12: no signed-in session within 300 s" >&2; return 2; }
        sleep 10
    done
}

case "${1:-}" in
stick)
    s=$(vm_state); [ "$s" = Off ] || { echo "v12: VM must be Off (state: $s)" >&2; exit 1; }
    MODE=window ./v1.sh stick
    ;;
windows)
    PS start; wait_windows 900; wait_session
    ;;
start)
    mkdir -p "$A"; rm -f "$A"/*.png "$A"/*.txt "$A"/*.json "$A"/*.csv "$A"/*.log
    L=$(stick_letter); [ -n "$L" ] || { echo "v12: no UPGV0 volume in the guest" >&2; exit 1; }
    guest "if (-not (Test-Path ${L}:\\UPGRADE.exe)) { 'no UPGRADE.exe on the stick' }; & ${L}:\\UPGRADE.exe --version | Out-String" | tee "$A/version.txt"
    date -u +%Y-%m-%dT%H:%M:%SZ > "$A/started_utc.txt"
    # the signed-in session, not PS Direct's: a one-shot interactive task, removed at once
    guest "schtasks /Create /TN '$LAUNCH_TASK' /TR '${L}:\\UPGRADE.exe' /SC ONCE /ST 23:59 /RU rig /IT /RL HIGHEST /F | Out-Null; schtasks /Run /TN '$LAUNCH_TASK' | Out-Null; Start-Sleep 2; schtasks /Delete /TN '$LAUNCH_TASK' /F | Out-Null; 'launched'"
    sleep 12; shot 1-welcome
    PS key 9; sleep 1; shot 2-focused
    PS key 13; sleep 8; shot 3-running
    ;;
wait)
    limit=${2:-3600}; t0=$(date +%s)
    while :; do
        # the guest is away during the live boot; PS Direct simply fails then
        L=$(stick_letter 2>/dev/null || true)
        if [ -n "$L" ] && guest "Select-String -Path ${L}:\\upgrade_\\convert.log -Pattern 'after the restart:' -SimpleMatch -Quiet" 2>/dev/null | grep -q True; then
            echo "v12: the window logged its result after $(( $(date +%s) - t0 )) s"; sleep 5; shot 5-result; break
        fi
        if [ -n "$L" ] && guest "Select-String -Path ${L}:\\upgrade_\\convert.log -Pattern 'stopped' -SimpleMatch -Quiet" 2>/dev/null | grep -q True \
           && ! guest "Select-String -Path ${L}:\\upgrade_\\convert.log -Pattern 'armed;' -SimpleMatch -Quiet" 2>/dev/null | grep -q True; then
            echo "v12: the window stopped before arming"; shot 4-stopped; break
        fi
        e=$(( $(date +%s) - t0 ))
        [ $(( e % 120 )) -lt 15 ] && shot "wait-$e"
        [ "$e" -ge "$limit" ] && { shot wait-stuck; echo "v12: no result within $limit s" >&2; exit 2; }
        sleep 15
    done
    ;;
close)
    # the result screen's one button is Close; Tab reaches it. Test-Handoff's own
    # "done" popup may hold the focus first (Enter just dismisses it), so try up to 4 times
    for i in 1 2 3 4; do
        guest '[bool](Get-Process UPGRADE -ErrorAction SilentlyContinue)' | grep -q True || break
        PS key 9; sleep 1; PS key 13; sleep 5
    done
    sleep 8; shot 6-closed
    ;;
verdict)
    mkdir -p "$A"
    L=$(stick_letter)
    [ -n "$L" ] || { echo "v12: no UPGV0 volume in the guest" >&2; exit 1; }
    guest "Get-Content ${L}:\\upgrade_\\convert.log -Raw" > "$A/convert.log" || true
    guest "Get-Content ${L}:\\upgrade_\\report\\verify.json -Raw -ErrorAction SilentlyContinue" > "$A/verify.json" || true
    guest "Get-Content ${L}:\\v0-handoff.csv -Raw -ErrorAction SilentlyContinue" > "$A/v0-handoff.csv" || true
    guest '$sb=try { if (Confirm-SecureBootUEFI) {"on"} else {"off"} } catch {"unknown"}
[pscustomobject]@{
  secure_boot = $sb
  reopen_task_left = [bool](Get-ScheduledTask -TaskName "upgrade_ window reopen" -ErrorAction SilentlyContinue)
  launch_task_left = [bool](Get-ScheduledTask -TaskName "upgrade_ v12 launch" -ErrorAction SilentlyContinue)
  window_dir_left = (Test-Path C:\ProgramData\upgrade_\window)
  window_state_left = (Test-Path C:\ProgramData\upgrade_\window\state.json)
  handoff_armed_left = (Test-Path C:\ProgramData\upgrade_\v0\handoff-state.json)
  bootsequence = ((bcdedit /enum "{fwbootmgr}" 2>&1 | Select-String "bootsequence") -join " ")
  window_running = [bool](Get-Process UPGRADE -ErrorAction SilentlyContinue)
} | ConvertTo-Json' > "$A/after.json"
    for f in verify.json v0-handoff.csv; do
        if [ ! -s "$A/$f" ] || grep -q "Cannot find path" "$A/$f"; then rm -f "$A/$f"; fi
    done
    python3 v12-verdict.py "$A" "$CSV" "$FIRMWARE"
    ;;
run)
    "$SELF" stick; "$SELF" windows; "$SELF" start; "$SELF" wait; "$SELF" close; "$SELF" verdict
    ;;
*)
    sed -n '2,26p' "$SELF"; exit 1 ;;
esac
