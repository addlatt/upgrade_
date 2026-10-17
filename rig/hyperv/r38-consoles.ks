# Rig only, never in the kit (R38, 2026-10-17). A %pre placed BEFORE the
# installer's own: it starts a watcher in the background and returns at once.
# The watcher records which processes hold tty1 to tty9 (and tty0, console)
# open, which units run the consoles' shells, and logind's settings, three
# times: as %pre starts, 3 s after the countdown starts (the program has
# switched to its console by then), and 30 s into the countdown. It never
# opens a console itself. The records go to the stick, upgrade_/report/
# r38-consoles-<when>.txt, for `v9.sh pull` / `stick-pull` to collect.
# v9.sh puts it in front of the job's ks.cfg (R38CAP=1 v9.sh job).
# Every command has a 10 s limit and every record is saved at once, with
# r38cap.log saying how far it got: the first two versions (2026-10-17, arms
# B and B2) left nothing on the stick; the watcher was most likely ended
# with %pre (see below).
%pre --log=/tmp/upgrade_-r38-capture-pre.log
cat > /tmp/r38cap.sh <<'EOF'
#!/bin/bash
holders() { # $1 a device path: every process with it open, by readlink of /proc/*/fd
    for p in /proc/[0-9]*; do
        for f in "$p"/fd/*; do
            [ "$(readlink "$f" 2>/dev/null)" = "$1" ] || continue
            echo "   pid ${p#/proc/} ppid $(awk '/^PPid/{print $2}' "$p/status" 2>/dev/null) comm $(cat "$p/comm" 2>/dev/null) fd ${f##*/} cmd $(tr '\0' ' ' < "$p/cmdline" 2>/dev/null) unit $(sed -n 's/^0::.*\///p' "$p/cgroup" 2>/dev/null)"
        done
    done
}
# the installer image has no `timeout` (arm C, 2026-10-17: every call answered 127): run plainly then
T() { if command -v timeout > /dev/null; then timeout 10 "$@"; else "$@"; fi; }
note() { echo "$(date -u +%FT%TZ) $*" >> /tmp/r38cap.log; }
cap() {
    note "cap $1 begins"
    o=/tmp/r38-consoles-$1.txt
    {
        echo "== r38-consoles $1 $(date -u +%FT%TZ)"
        echo "== active console: $(cat /sys/class/tty/tty0/active 2>&1)"
        for n in 1 2 3 4 5 6 7 8 9 0; do echo "-- /dev/tty$n held by:"; holders /dev/tty$n; done
        echo "-- /dev/console held by:"; holders /dev/console
        echo "== units"
        T systemctl list-units --all --no-pager --plain 'autovt@*' 'getty@*' 'anaconda-shell@*' 'serial-getty@*' 'anaconda*' 2>&1
        for u in $(T systemctl list-units --all --plain --no-legend 'autovt@*' 'getty@*' 'anaconda-shell@*' 2>/dev/null | awk '{print $1}'); do
            echo "-- systemctl status $u"; T systemctl status "$u" --no-pager -l 2>&1 | head -20
        done
        echo "== unit files"
        ls -l /etc/systemd/system/autovt@.service /usr/lib/systemd/system/autovt@.service /etc/systemd/system/getty.target.wants /usr/lib/systemd/system/anaconda-shell@.service 2>&1
        T systemctl cat autovt@.service anaconda-shell@.service 2>&1
        echo "== logind"
        T busctl get-property org.freedesktop.login1 /org/freedesktop/login1 org.freedesktop.login1.Manager NAutoVTs 2>&1
        cat /etc/systemd/logind.conf 2>&1 | grep -v '^#' | grep -v '^$'
        ls -l /etc/systemd/logind.conf.d /usr/lib/systemd/logind.conf.d 2>&1
        cat /etc/systemd/logind.conf.d/* /usr/lib/systemd/logind.conf.d/* 2>&1
        echo "== processes"
        T ps -eo pid,ppid,tty,comm,args 2>&1
    } > "$o" 2>&1
    note "cap $1 written"
}
save() {
    T mount -o remount,rw /run/install/repo; note "remount rw: $?"

    mkdir -p /run/install/repo/upgrade_/report
    cp /tmp/r38-consoles-*.txt /tmp/r38cap.log /run/install/repo/upgrade_/report/ 2>/dev/null
    sync
}
note "watcher started"
save
cap 1-pre-start; save
t0=$(date +%s)
until grep -q -e '== countdown: ' -e 'REFUSED' -e 'verify mode: rebooting' /tmp/upgrade_-verify.log 2>/dev/null; do
    [ $(( $(date +%s) - t0 )) -ge 900 ] && { echo "no countdown within 900 s"; cap 2-no-countdown; save; exit 0; }
    sleep 0.5
done
sleep 3; cap 2-countdown-3s; save
sleep 27; cap 3-countdown-30s; save
EOF
# a process left behind by %pre is ended with it when %pre runs as a systemd
# service (the cgroup is cleaned up; setsid does not leave it): the watcher
# gets a unit of its own. And %pre itself says, on the stick, which unit ran it
{ echo "$(date -u +%FT%TZ) %pre ran; its cgroup: $(cat /proc/self/cgroup)"; echo "parent: $(cat /proc/$PPID/comm 2>&1) $(tr '\0' ' ' < /proc/$PPID/cmdline 2>&1)"; } > /tmp/r38-pre.txt
if systemd-run --unit=upgrade-r38cap --collect --quiet bash /tmp/r38cap.sh >> /tmp/r38-pre.txt 2>&1; then echo "watcher: systemd-run upgrade-r38cap" >> /tmp/r38-pre.txt
else echo "watcher: systemd-run failed, setsid instead" >> /tmp/r38-pre.txt; setsid bash /tmp/r38cap.sh < /dev/null > /dev/null 2>&1 & fi
mount -o remount,rw /run/install/repo && mkdir -p /run/install/repo/upgrade_/report && cp /tmp/r38-pre.txt /run/install/repo/upgrade_/report/ && sync
true
%end
