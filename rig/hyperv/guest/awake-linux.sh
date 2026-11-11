#!/bin/bash
# rig/hyperv/guest/awake-linux.sh: settle-in's sleep lock inside the rig's
# Fedora (roadmap item 16, RISKS R33). Run as root by the OEMDRV run hook
# (guest/oemdrv-run.sh); every finding is one line, "AWAKE key=value", in
# OEMDRV:/run.log. The host half is rig/hyperv/awake.sh.
#
#   A  the walk-away's prepare, stopped at its consent step (the sentence is
#      wrong on purpose, so nothing is prepared): its record must say the
#      lock was held
#   B  `settle-in awake --for 240` holds the product's lock: logind lists it
#   C  a suspend asked of logind while it is held is turned down
#   D  logind is told to suspend after 1 idle minute; with the lock held the
#      guest stays awake for 150 s
#   E  settle-in is killed (-9): the lock and its helper are gone
#   F  the control for D: with no lock, the same guest, the same setting,
#      suspends by itself (the host presses a key to wake it)
#   G  the control for C: the same request with no lock suspends the guest
#
# logind counts idle time only for a signed-in session, so the host signs in
# on the console (rig/rig) while D starts.
B=/run/awake; mkdir -p $B; cp /mnt/oemdrv/v3/settle-in $B/settle-in; chmod +x $B/settle-in; S=$B/settle-in
say() { echo "AWAKE $*"; sync; }
lg() { busctl call org.freedesktop.login1 /org/freedesktop/login1 org.freedesktop.login1.Manager "$@" 2>&1 | tr '\n' ' '; }
# seconds this boot has spent suspended: CLOCK_BOOTTIME counts them, CLOCK_MONOTONIC does not
gap() { python3 -c 'import time; print(round(time.clock_gettime(time.CLOCK_BOOTTIME) - time.clock_gettime(time.CLOCK_MONOTONIC), 1))'; }
listed() { systemd-inhibit --list --no-legend 2>/dev/null | grep upgrade_ | tr -s ' ' | tr '\n' '|'; }

say os="$(. /etc/os-release; echo "$PRETTY_NAME")"
say kernel="$(uname -r)"
say systemd="$(systemctl --version | head -1)"
say settle_in="$($S --version)"
say power_states="$(cat /sys/power/state 2>&1)"
say can_suspend="$(lg CanSuspend)"
say sessions="$(loginctl list-sessions --no-legend 2>/dev/null | wc -l)"
say gap_at_start="$(gap)"

# A
$S go-back walkaway prepare --sentence "not the sentence" --account rig --serial none --size 1 --typed none > $B/prepare.out 2> $B/prepare.err
say prepare_rc=$?
say prepare_result="$(tr -d '\n' < $B/prepare.out | cut -c1-300)"
rec=$(ls -t /var/lib/upgrade_-go-back/walkaway-*.json 2>/dev/null | head -1)
say record_awake="$(python3 -c 'import json,sys; d=json.load(open(sys.argv[1])); print(json.dumps([s for s in d["steps"] if s.get("step")=="awake"]))' "$rec" 2>&1)"
say listed_after_prepare="$(listed)"

# B
$S awake --for 240 > $B/awake.out 2>&1 &
PID=$!
sleep 3
say hold="$(head -1 $B/awake.out)"
say listed="$(listed)"
say helper="$(pgrep -a -x tail | grep -c -- "--pid=$PID")"

# C
say suspend_asked="$(lg SuspendWithFlags t 1)"
say suspend_systemctl="$(systemctl suspend --check-inhibitors=yes 2>&1 | tr '\n' ' '; echo "rc=${PIPESTATUS[0]}")"
sleep 5
say gap_after_asking="$(gap)"

# D
mkdir -p /etc/systemd/logind.conf.d
printf '[Login]\nIdleAction=suspend\nIdleActionSec=60s\n' > /etc/systemd/logind.conf.d/zz-awake-test.conf
systemctl kill -s HUP systemd-logind
sleep 2
say idle_action="$(busctl get-property org.freedesktop.login1 /org/freedesktop/login1 org.freedesktop.login1.Manager IdleAction IdleActionUSec IdleHint 2>&1 | tr '\n' ' ')"
say listed_after_reload="$(listed)"
sleep 150
say sessions_while_held="$(loginctl list-sessions --no-legend 2>/dev/null | wc -l)"
say gap_after_150s_held="$(gap)"
say still_listed="$(listed)"
say logind_while_held="$(journalctl -b -u systemd-logind --no-pager -o cat 2>/dev/null | grep -i -E 'idle|suspend|sleep' | tail -5 | tr '\n' '|')"

# E
kill -9 $PID
sleep 3
say listed_after_kill="$(listed)"
say helper_after_kill="$(pgrep -a -x tail | grep -c -- "--pid=$PID")"

# F: the control for D. The lock is gone and the setting stands: logind suspends by itself
say sessions_at_control="$(loginctl list-sessions --no-legend 2>/dev/null | wc -l)"
say idle_hint_at_control="$(busctl get-property org.freedesktop.login1 /org/freedesktop/login1 org.freedesktop.login1.Manager IdleHint 2>&1)"
say control_start="$(date -u +%FT%TZ)"
c=not-suspended
for i in $(seq 1 15); do
    sleep 10
    g=$(gap)
    if python3 -c "import sys; sys.exit(0 if float('$g') > 5 else 1)"; then c="suspended within $((i * 10)) s, asleep $g s"; break; fi
done
say control_idle="$c"
say logind_control="$(journalctl -b -u systemd-logind --no-pager -o cat 2>/dev/null | grep -i -E 'idle|suspend|sleep' | tail -5 | tr '\n' '|')"
rm -f /etc/systemd/logind.conf.d/zz-awake-test.conf
systemctl kill -s HUP systemd-logind
sleep 2
# G: the control for C. The same request, with no lock: the guest suspends (the host wakes it)
g0=$(gap)
say control_asked="$(lg SuspendWithFlags t 1)"
sleep 8
say control_asked_asleep="$(python3 -c "print(round(float('$(gap)') - float('$g0'), 1))")"
say done=1
