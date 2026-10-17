//! The console sign-in hook, `settle-in console-hook`: what
//! `settle-in/linux/upgrade_-settle-in.sh` did until 2026-10-14; the one
//! line left in `/etc/profile.d/` only starts this. For people who chose
//! the text console: shows what the first startup did, once per person, in
//! the same words as the window (`settle-in summary --text`), and asks the
//! one question. ONLY at a real text console. A display manager also
//! starts the desktop through a login shell that reads the profile, before
//! any DISPLAY exists and with a terminal on standard input: rig run 2
//! (2026-09-27) waited there for an answer and the desktop never started
//! (a black screen). logind's own session type says which it is.

use std::io::Write;

/// The program as the polkit policy names it; `pkexec` needs the exact path.
pub const PROGRAM: &str = "/usr/local/libexec/upgrade_/settle-in";

/// What the sign-in looks like from inside the login shell.
pub struct Session {
    pub session_type: Option<String>,
    pub tty: Option<String>,
    pub stdin_tty: bool,
    pub stdout_tty: bool,
    pub display: String,
    pub wayland_display: String,
}

impl Session {
    pub fn read() -> Session {
        // SAFETY: isatty and ttyname take a descriptor and have no other preconditions;
        // ttyname's static buffer is read at once, on this thread, before any other call.
        let (stdin_tty, stdout_tty) = unsafe { (libc::isatty(0) == 1, libc::isatty(1) == 1) };
        let tty = unsafe {
            let p = libc::ttyname(0);
            if p.is_null() { None } else { Some(std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned()) }
        };
        Session {
            session_type: std::env::var("XDG_SESSION_TYPE").ok(),
            tty,
            stdin_tty,
            stdout_tty,
            display: std::env::var("DISPLAY").unwrap_or_default(),
            wayland_display: std::env::var("WAYLAND_DISPLAY").unwrap_or_default(),
        }
    }
}

/// The script's test, clause for clause: logind says `tty`, standard input is
/// `/dev/ttyN` (a virtual console, not a pseudo-terminal), both standard
/// streams are terminals, and no display is set.
pub fn is_text_console(s: &Session) -> bool {
    let on_console = s.tty.as_deref().and_then(|t| t.strip_prefix("/dev/tty")).is_some_and(|rest| rest.chars().next().is_some_and(|c| c.is_ascii_digit()));
    s.session_type.as_deref() == Some("tty") && on_console && s.stdin_tty && s.stdout_tty && s.display.is_empty() && s.wayland_display.is_empty()
}

/// `${XDG_STATE_HOME:-$HOME/.local/state}/upgrade_/settle-in-shown`; None when
/// neither is set (the script's path would have been relative; shown nothing).
pub fn mark_path(xdg_state_home: Option<&str>, home: Option<&str>) -> Option<String> {
    let base = match xdg_state_home {
        Some(x) if !x.is_empty() => x.to_string(),
        _ => format!("{}/.local/state", home.filter(|h| !h.is_empty())?),
    };
    Some(format!("{base}/upgrade_/settle-in-shown"))
}

/// `read -r answer` then `[ "$answer" = yes ]`: the shell strips the line end
/// and the blanks at both ends, and compares the rest exactly.
pub fn says_yes(line: &str) -> bool {
    line.trim_end_matches(['\n', '\r']).trim_matches([' ', '\t']) == "yes"
}

/// The hook's words, as the script printed them (approved 2026-09-27).
pub const QUESTION: &str = "  Remove the old Windows startup entry now? Type yes and press Enter (anything else skips): ";
pub const REMOVED: &str = "  Removed.";
pub const NOT_REMOVED: &str = "  Not removed: it did not work (the reason is in /var/lib/upgrade_/settle-in/report.json).";

/// Always exits 0: a sign-in hook must never fail the sign-in.
pub fn run(root: &str) -> i32 {
    if !is_text_console(&Session::read()) {
        return 0;
    }
    let Some(mark) = mark_path(std::env::var("XDG_STATE_HOME").ok().as_deref(), std::env::var("HOME").ok().as_deref()) else { return 0 };
    if std::path::Path::new(&mark).exists() {
        return 0;
    }
    let Ok(summary) = crate::public_summary(root) else { return 0 };
    let sections = crate::summary::sections(&summary);
    if let Some(dir) = std::path::Path::new(&mark).parent() {
        if std::fs::create_dir_all(dir).is_ok() {
            let _ = std::fs::write(&mark, "shown\n");
        }
    }
    println!();
    print!("{}", crate::summary::text(&sections));
    let has_button = sections["sections"].as_array().is_some_and(|a| a.iter().any(|s| s.get("button").and_then(serde_json::Value::as_str).is_some()));
    if has_button {
        println!();
        print!("{QUESTION}");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        let _ = std::io::stdin().read_line(&mut line);
        if says_yes(&line) {
            let ok = std::process::Command::new("pkexec").arg(PROGRAM).arg("remove-old-boot-entry").stdout(std::process::Stdio::null()).status().map(|s| s.success()).unwrap_or(false);
            println!("{}", if ok { REMOVED } else { NOT_REMOVED });
        }
    }
    println!();
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn console() -> Session {
        Session { session_type: Some("tty".into()), tty: Some("/dev/tty2".into()), stdin_tty: true, stdout_tty: true, display: String::new(), wayland_display: String::new() }
    }

    #[test]
    fn a_real_text_console_shows() {
        assert!(is_text_console(&console()));
        assert!(is_text_console(&Session { tty: Some("/dev/tty12".into()), ..console() }));
    }

    #[test]
    fn a_display_managers_login_shell_never_shows() {
        // rig run 2 (2026-09-27): SDDM's login shell, a terminal on stdin, no DISPLAY yet; logind says wayland or x11
        assert!(!is_text_console(&Session { session_type: Some("wayland".into()), ..console() }));
        assert!(!is_text_console(&Session { session_type: Some("x11".into()), ..console() }));
        assert!(!is_text_console(&Session { session_type: None, ..console() }));
        // a display already set, whatever logind says
        assert!(!is_text_console(&Session { display: ":0".into(), ..console() }));
        assert!(!is_text_console(&Session { wayland_display: "wayland-0".into(), ..console() }));
    }

    #[test]
    fn a_pseudo_terminal_or_a_pipe_never_shows() {
        // ssh and terminal windows are /dev/pts/N; logind may still say tty for ssh
        assert!(!is_text_console(&Session { tty: Some("/dev/pts/0".into()), ..console() }));
        assert!(!is_text_console(&Session { tty: Some("/dev/ttyS0".into()), ..console() }));
        assert!(!is_text_console(&Session { tty: None, stdin_tty: false, ..console() }));
        assert!(!is_text_console(&Session { stdout_tty: false, ..console() }));
    }

    #[test]
    fn the_mark_is_where_the_script_put_it() {
        assert_eq!(mark_path(Some("/var/state"), Some("/home/a")).as_deref(), Some("/var/state/upgrade_/settle-in-shown"));
        assert_eq!(mark_path(None, Some("/home/a")).as_deref(), Some("/home/a/.local/state/upgrade_/settle-in-shown"));
        assert_eq!(mark_path(Some(""), Some("/home/a")).as_deref(), Some("/home/a/.local/state/upgrade_/settle-in-shown"));
        assert_eq!(mark_path(None, None), None);
    }

    #[test]
    fn the_answer_is_the_shells_reading() {
        assert!(says_yes("yes\n"));
        assert!(says_yes("  yes \t\n"));
        assert!(!says_yes("YES\n"));
        assert!(!says_yes("yes please\n"));
        assert!(!says_yes("\n"));
        assert!(!says_yes(""));
    }
}
