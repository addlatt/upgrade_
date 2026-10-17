# Rig only, never in the kit (R38, 2026-10-17). A %pre placed BEFORE the
# installer's own: it starts a watcher in the background and returns at once.
# The watcher records which processes hold tty1 to tty9 (and tty0, console)
# open, which units run the consoles' shells, and logind's settings, three
# times: as %pre starts, 3 s after the countdown starts (the program has
# switched to its console by then), and 30 s into the countdown. It never
# opens a console itself. The records go to the stick, upgrade_/report/
# r38-consoles-<when>.txt, for `v9.sh pull` / `stick-pull` to collect.
# v9.sh puts it in front of the job's ks.cfg (R38CAP=1 v9.sh job).
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
cap() {
    o=/tmp/r38-consoles-$1.txt
    {
        echo "== r38-consoles $1 $(date -u +%FT%TZ)"
        echo "== active console: $(cat /sys/class/tty/tty0/active 2>&1)"
        for n in 1 2 3 4 5 6 7 8 9 0; do echo "-- /dev/tty$n held by:"; holders /dev/tty$n; done
        echo "-- /dev/console held by:"; holders /dev/console
        echo "== units"
        systemctl list-units --all --no-pager --plain 'autovt@*' 'getty@*' 'anaconda-shell@*' 'serial-getty@*' 'anaconda*' 2>&1
        for u in $(systemctl list-units --all --plain --no-legend 'autovt@*' 'getty@*' 'anaconda-shell@*' 2>/dev/null | awk '{print $1}'); do
            echo "-- systemctl status $u"; systemctl status "$u" --no-pager -l 2>&1 | head -20
        done
        echo "== unit files"
        ls -l /etc/systemd/system/autovt@.service /usr/lib/systemd/system/autovt@.service /etc/systemd/system/getty.target.wants /usr/lib/systemd/system/anaconda-shell@.service 2>&1
        systemctl cat autovt@.service anaconda-shell@.service 2>&1
        echo "== logind"
        busctl get-property org.freedesktop.login1 /org/freedesktop/login1 org.freedesktop.login1.Manager NAutoVTs 2>&1
        cat /etc/systemd/logind.conf 2>&1 | grep -v '^#' | grep -v '^$'
        ls -l /etc/systemd/logind.conf.d /usr/lib/systemd/logind.conf.d 2>&1
        cat /etc/systemd/logind.conf.d/* /usr/lib/systemd/logind.conf.d/* 2>&1
        echo "== processes"
        ps -eo pid,ppid,tty,comm,args 2>&1
    } > "$o" 2>&1
}
save() {
    mount -o remount,rw /run/install/repo
    mkdir -p /run/install/repo/upgrade_/report
    cp /tmp/r38-consoles-*.txt /tmp/r38cap.log /run/install/repo/upgrade_/report/ 2>/dev/null
    sync
}
cap 1-pre-start
t0=$(date +%s)
until grep -q -e '== countdown: ' -e 'REFUSED' -e 'verify mode: rebooting' /tmp/upgrade_-verify.log 2>/dev/null; do
    [ $(( $(date +%s) - t0 )) -ge 900 ] && { echo "no countdown within 900 s"; cap 2-no-countdown; save; exit 0; }
    sleep 0.5
done
sleep 3; cap 2-countdown-3s; save
sleep 27; cap 3-countdown-30s; save
EOF
setsid bash /tmp/r38cap.sh < /dev/null > /tmp/r38cap.log 2>&1 &
%end
