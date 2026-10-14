# upgrade_ settle-in at a console sign-in. Installed in /etc/profile.d/ by
# the installer adapter; every login shell sources it, so it does one thing:
# start the program, which decides by itself whether this is a real text
# console (`settle-in console-hook`, settle-in/src/console_hook.rs; the
# checks lived in this file until 2026-10-14) and always returns 0.
if [ -x /usr/local/libexec/upgrade_/settle-in ]; then
    /usr/local/libexec/upgrade_/settle-in console-hook
fi
