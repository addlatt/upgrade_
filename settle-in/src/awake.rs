//! Keep the computer awake while settle-in works (decided 2026-10-05, the
//! owner; RISKS R33, roadmap item 16). A desktop left alone goes to sleep:
//! GNOME did 15 minutes into Prepare (2026-10-04) and KDE 45 minutes in
//! (2026-10-28).
//!
//! The lock is logind's own (`systemd-inhibit`, the system's "do not sleep"
//! request): sleep, idle and the lid switch, blocked while the work runs.
//! `systemd-inhibit` holds the lock for as long as the command it was given
//! runs, and that command is `tail --pid=<us>`, which ends when this
//! process does. So the lock is given back when the guard is dropped, and
//! also when settle-in exits, crashes or is killed. Nothing is left behind.
//!
//! What it cannot do: a flat battery, the power button.

use std::process::{Child, Command, Stdio};

const PATH_ENV: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

/// What is asked for first, and what is asked for when that is turned down
/// (the lid switch needs more rights than sleep and idle do).
pub const WHAT: [&str; 2] = ["sleep:idle:handle-lid-switch", "sleep:idle"];

/// The lock, held until dropped.
pub struct Awake {
    child: Option<Child>,
    what: &'static str,
}

/// The command line that holds `what` for as long as process `pid` lives.
pub fn command_line(tool: &str, what: &str, why: &str, pid: u32) -> Vec<String> {
    vec![
        tool.to_string(),
        format!("--what={}", what),
        "--mode=block".to_string(),
        "--who=upgrade_".to_string(),
        format!("--why={}", why),
        "tail".to_string(),
        format!("--pid={}", pid),
        "-f".to_string(),
        "/dev/null".to_string(),
    ]
}

/// Ask the computer to stay awake while `why` goes on. Never fails the work:
/// where the lock cannot be taken, `held()` says so and the caller tells the
/// person.
pub fn hold(why: &str) -> Awake {
    hold_with("systemd-inhibit", why, PATH_ENV)
}

pub(crate) fn hold_with(tool: &str, why: &str, path: &str) -> Awake {
    use std::os::unix::process::CommandExt;
    for what in WHAT {
        let line = command_line(tool, what, why, std::process::id());
        // its own process group, so letting go ends the tool and its `tail` together
        let spawned = Command::new(&line[0]).args(&line[1..]).env("PATH", path).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).process_group(0).spawn();
        let Ok(mut child) = spawned else { continue };
        // a request that is turned down ends at once; one that is held keeps running
        std::thread::sleep(std::time::Duration::from_millis(300));
        match child.try_wait() {
            Ok(None) => return Awake { child: Some(child), what },
            _ => continue,
        }
    }
    Awake { child: None, what: "" }
}

impl Awake {
    /// True while the lock is held.
    pub fn held(&mut self) -> bool {
        matches!(self.child.as_mut().map(|c| c.try_wait()), Some(Ok(None)))
    }

    /// What is blocked: `sleep:idle:handle-lid-switch`, `sleep:idle`, or nothing.
    pub fn what(&self) -> &'static str {
        self.what
    }

    /// One line for the record and the person.
    pub fn words(&mut self) -> String {
        if !self.held() {
            return "this computer could not be asked to stay awake: keep it awake yourself until this finishes".to_string();
        }
        if self.what.contains("lid") { "this computer is kept awake while this runs".to_string() } else { "this computer is kept awake while this runs; keep the lid open".to_string() }
    }
}

impl Drop for Awake {
    fn drop(&mut self) {
        if let Some(mut c) = self.child.take() {
            // SAFETY: kill has no memory preconditions; the negative pid names the group made above.
            unsafe { libc::kill(-(c.id() as i32), libc::SIGTERM) };
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn the_lock_names_sleep_idle_and_the_lid_and_follows_this_process() {
        let l = command_line("systemd-inhibit", WHAT[0], "preparing", 4242);
        assert_eq!(l, ["systemd-inhibit", "--what=sleep:idle:handle-lid-switch", "--mode=block", "--who=upgrade_", "--why=preparing", "tail", "--pid=4242", "-f", "/dev/null"]);
    }

    /// A stand-in for systemd-inhibit: it turns down the lid switch when told
    /// to, and otherwise runs the command it was given, as the real one does.
    fn fake(tag: &str, refuse_lid: bool, refuse_all: bool) -> (String, String) {
        let dir = std::env::temp_dir().join(format!("settle-in-awake-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let tool = dir.join("systemd-inhibit");
        let body = format!(
            "#!/bin/sh\n{}{}echo \"$1\" > {}/asked\nwhile [ \"${{1#--}}\" != \"$1\" ]; do shift; done\n\"$@\" &\necho $! > {}/inner\nwait\n",
            if refuse_all { "exit 1\n" } else { "" },
            if refuse_lid { "case \"$1\" in *lid*) exit 1;; esac\n" } else { "" },
            dir.display(),
            dir.display()
        );
        std::fs::write(&tool, body).unwrap();
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
        (dir.to_string_lossy().into_owned(), format!("{}:/usr/bin:/bin", dir.display()))
    }

    fn alive(pid: &str) -> bool {
        std::fs::read_to_string(format!("/proc/{}/stat", pid.trim())).is_ok_and(|s| !s.contains(") Z "))
    }

    #[test]
    fn held_while_alive_and_nothing_left_running_after() {
        let (dir, path) = fake("held", false, false);
        let mut a = hold_with("systemd-inhibit", "a test", &path);
        assert!(a.held());
        assert_eq!(a.what(), "sleep:idle:handle-lid-switch");
        assert_eq!(a.words(), "this computer is kept awake while this runs");
        let inner = std::fs::read_to_string(format!("{}/inner", dir)).unwrap();
        assert!(alive(&inner), "the command under the lock is running");
        drop(a);
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(!alive(&inner), "letting go ends the command under the lock too");
    }

    #[test]
    fn a_lid_turned_down_still_holds_sleep_and_says_keep_the_lid_open() {
        let (dir, path) = fake("nolid", true, false);
        let mut a = hold_with("systemd-inhibit", "a test", &path);
        assert!(a.held());
        assert_eq!(a.what(), "sleep:idle");
        assert_eq!(std::fs::read_to_string(format!("{}/asked", dir)).unwrap().trim(), "--what=sleep:idle");
        assert!(a.words().ends_with("keep the lid open"));
    }

    #[test]
    fn no_lock_is_said_plainly_and_never_stops_the_work() {
        let (_, path) = fake("none", false, true);
        let mut a = hold_with("systemd-inhibit", "a test", &path);
        assert!(!a.held());
        assert_eq!(a.what(), "");
        assert!(a.words().contains("could not be asked to stay awake"));
        let mut b = hold_with("no-such-tool-anywhere", "a test", "/nonexistent");
        assert!(!b.held());
    }
}
