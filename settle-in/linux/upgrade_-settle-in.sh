# upgrade_ settle-in at a console sign-in (for people who chose the text
# console). Shows what the first startup did, once per person, in the same
# words as the window (they come from `settle-in summary --text`), and
# offers the one button as a question. Installed in /etc/profile.d/ by the
# installer adapter. POSIX sh: every login shell sources it.
# ONLY a real text console. A display manager also starts the desktop
# through a login shell that reads this file, before any DISPLAY exists, and
# with a terminal on standard input: rig run 2 (2026-09-27) waited here at
# `read` and the desktop never started (a black screen). logind's own
# session type says which it is.
_upg_tty=$(tty 2>/dev/null)
case "$_upg_tty" in /dev/tty[0-9]*) ;; *) _upg_tty= ;; esac
if [ "${XDG_SESSION_TYPE:-}" = tty ] && [ -n "$_upg_tty" ] && [ -t 0 ] && [ -t 1 ] && [ -z "${DISPLAY}${WAYLAND_DISPLAY}" ] \
    && [ -x /usr/local/libexec/upgrade_/settle-in ]; then
    _upg_mark="${XDG_STATE_HOME:-$HOME/.local/state}/upgrade_/settle-in-shown"
    if [ ! -e "$_upg_mark" ] && /usr/local/libexec/upgrade_/settle-in summary --text > /dev/null 2>&1; then
        mkdir -p "$(dirname "$_upg_mark")" && echo shown > "$_upg_mark"
        echo
        /usr/local/libexec/upgrade_/settle-in summary --text
        if /usr/local/libexec/upgrade_/settle-in summary | grep -q '"button"'; then
            echo
            # a draft awaiting approval: the console form of the button
            printf '  Remove the old Windows startup entry now? Type yes and press Enter (anything else skips): '
            read -r _upg_answer
            if [ "$_upg_answer" = yes ]; then
                if pkexec /usr/local/libexec/upgrade_/settle-in remove-old-boot-entry > /dev/null; then echo '  Removed.'
                else echo '  Not removed: it did not work (the reason is in /var/lib/upgrade_/settle-in/report.json).'; fi
            fi
        fi
        echo
    fi
    unset _upg_mark _upg_answer
fi
unset _upg_tty
