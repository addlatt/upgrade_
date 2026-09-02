#!/bin/bash
# One-time, run from the Fedora console (as root) with OEMDRV mounted at /mnt:
#   sudo bash /mnt/v3-bootstrap.sh
# Installs the OEMDRV run hook (guest/oemdrv-run.sh) as a boot-time unit and
# runs it once right away, so the same boot executes OEMDRV:/run.sh.
set -e
src=/mnt
install -m 0755 "$src/oemdrv-run.sh" /usr/local/sbin/oemdrv-run
cat > /etc/systemd/system/oemdrv-run.service <<'EOS'
[Unit]
Description=Hyper-V rig: run OEMDRV:/run.sh (host-driven guest work)
After=multi-user.target v1b-mark.service
[Service]
Type=oneshot
ExecStart=/usr/local/sbin/oemdrv-run
[Install]
WantedBy=multi-user.target
EOS
systemctl daemon-reload
systemctl enable oemdrv-run.service
echo "bootstrap: hook installed $(date -u +%FT%TZ)" > "$src/bootstrap.done"
# do NOT umount /mnt here: bash is still reading this script off it (seen
# 2026-09-01: "target is busy" aborted the run). The hook mounts by label at
# its own /mnt/oemdrv, so it runs fine alongside.
sync
echo "bootstrap: hook installed, running it now"
/usr/local/sbin/oemdrv-run
