//! The way back's first sign-in, on the freshly installed Windows (RISKS
//! R30, R33). Until gate 0.4.0 (2026-10-17) this was a batch file and a
//! PowerShell script settle-in wrote onto the stick; they are product code
//! (they run on the person's computer), so they are Rust now, in the one
//! Windows program the way back already carries (the stick, the installed
//! Linux, the walk-away stick). The batch file the answer file starts
//! (`upgrade_\go-back-first-logon.cmd`) only starts this.
//!
//!   upgrade-gate first-logon <stick, e.g. E:>   at the first sign-in (the answer file's FirstLogonCommands)
//!   upgrade-gate ssh-task                       the SYSTEM startup task first-logon registers
//!
//! What it does, decided before and kept exactly:
//! - Wi-Fi (decided 2026-10-02): every profile from Linux is added for all
//!   users, then the folder with the passwords leaves the stick. The Wi-Fi
//!   service may not be running yet at the first sign-in (the rig,
//!   2026-10-03), so it is started first and given 5 s.
//! - Remote access (decided 2026-10-04): only when Linux's SSH server
//!   started by itself and the person had plain public keys (settle-in
//!   decides that and writes `upgrade_\ssh\authorized_keys` only then).
//!   The OpenSSH server is a download from Windows Update, so a task as
//!   SYSTEM waits for the network, tries at every start until it has it,
//!   lets in only those keys, switches password sign-in off, and removes
//!   itself when done.
//! - No SetupComplete.cmd: Windows skips it on computers with a maker's key
//!   (the Aspire, 2026-10-02); first-sign-in commands run everywhere.
//!
//! Logs stay where they were: `<stick>\upgrade_\go-back-wifi.log`,
//! `<stick>\upgrade_\go-back-ssh.log` (the registering), and
//! `C:\ProgramData\upgrade_\go-back-ssh.log` (the task, a UTC time on every
//! line).

pub const TASK: &str = "upgrade_ go-back ssh";
pub const CAPABILITY: &str = "OpenSSH.Server~~~~0.0.1.0";
/// Our own firewall rule (netsh names rules by display name, and the
/// capability's own rule's display name is not ours to rely on).
pub const FIREWALL_RULE: &str = "upgrade_ OpenSSH Server (sshd)";
/// How many times the task asks Windows Update for the server, a minute apart, per start.
pub const TRIES: u32 = 60;
const MARK: &str = "# upgrade_: keys only";

/// A path under a folder, with the separator of the system it runs on (so
/// the tests run on Linux).
pub fn join(a: &str, b: &str) -> String {
    let sep = if cfg!(windows) { "\\" } else { "/" };
    let b = if cfg!(windows) { b.to_string() } else { b.replace('\\', "/") };
    format!("{}{}{}", a.trim_end_matches(['\\', '/']), sep, b)
}

/// The sshd service as the service manager reports it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Svc {
    pub running: bool,
    pub automatic: bool,
}

/// What first-logon and the task ask of Windows, besides files.
pub trait Host {
    /// A tool's exit code and everything it printed.
    fn run(&mut self, program: &str, args: &[&str]) -> (i32, String);
    /// None: no such service (the OpenSSH server is not installed).
    fn sshd(&mut self) -> Option<Svc>;
    /// Start sshd and wait until it runs (up to 30 s).
    fn start_sshd(&mut self) -> bool;
    /// Stop sshd and wait until it has stopped (up to 30 s).
    fn stop_sshd(&mut self) -> bool;
    fn sshd_automatic(&mut self) -> bool;
    fn sleep(&mut self, secs: u64);
    /// This program's own file, to copy where the task can find it after the stick is gone.
    fn own_exe(&mut self) -> Option<String>;
}

fn append(path: &str, text: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(text.as_bytes());
        if !text.is_empty() && !text.ends_with('\n') {
            let _ = f.write_all(b"\r\n");
        }
    }
}

/// The task's definition, for `schtasks /create /xml`: SYSTEM, at every
/// start, highest rights, on battery too, started late if a start was
/// missed, at most 2 h a run (what go-back-ssh.ps1 registered).
pub fn task_xml(exe: &str) -> String {
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-16\"?>\r\n\
<Task version=\"1.2\" xmlns=\"http://schemas.microsoft.com/windows/2004/02/mit/task\">\r\n\
  <RegistrationInfo><Description>upgrade_: the way back's SSH server, keys only (decided 2026-10-04); removes itself when done</Description></RegistrationInfo>\r\n\
  <Triggers><BootTrigger><Enabled>true</Enabled></BootTrigger></Triggers>\r\n\
  <Principals><Principal id=\"Author\"><UserId>S-1-5-18</UserId><RunLevel>HighestAvailable</RunLevel></Principal></Principals>\r\n\
  <Settings>\r\n\
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>\r\n\
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>\r\n\
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>\r\n\
    <StartWhenAvailable>true</StartWhenAvailable>\r\n\
    <ExecutionTimeLimit>PT2H</ExecutionTimeLimit>\r\n\
    <Enabled>true</Enabled>\r\n\
  </Settings>\r\n\
  <Actions Context=\"Author\"><Exec><Command>{}</Command><Arguments>ssh-task</Arguments></Exec></Actions>\r\n\
</Task>\r\n",
        esc(exe)
    )
}

/// UTF-16LE with a byte-order mark: the encoding Task Scheduler's own
/// exports use and schtasks reads.
pub fn utf16_bom(text: &str) -> Vec<u8> {
    let mut out = vec![0xFF, 0xFE];
    for u in text.encode_utf16() {
        out.extend_from_slice(&u.to_le_bytes());
    }
    out
}

/// sshd_config with password sign-in off: three lines on top, because sshd
/// keeps the first value it reads. None when our lines are already there.
pub fn keys_only(cfg: &[u8]) -> Option<Vec<u8>> {
    let text = String::from_utf8_lossy(cfg);
    if text.lines().any(|l| l.starts_with(MARK)) {
        return None;
    }
    let mut out = format!("{MARK}, carried from Linux\r\nPasswordAuthentication no\r\nKbdInteractiveAuthentication no\r\n").into_bytes();
    out.extend_from_slice(cfg);
    Some(out)
}

/// What the last line of the log says about password sign-in.
pub fn password_off(cfg: &[u8]) -> bool {
    String::from_utf8_lossy(cfg).lines().any(|l| l.trim_end() == "PasswordAuthentication no")
}

/// DISM's own success codes: done, and done but wants a restart.
pub fn dism_ok(code: i32) -> bool {
    code == 0 || code == 3010
}

/// The last line DISM printed that says something (its error, in the machine's language).
pub fn last_said(out: &str) -> String {
    out.lines().map(str::trim).filter(|l| !l.is_empty()).last().unwrap_or("").to_string()
}

/// The profiles in the stick's Wi-Fi folder, in name order.
pub fn wifi_profiles(dir: &str) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir).map(|d| d.flatten().filter(|e| e.path().is_file()).map(|e| e.file_name().to_string_lossy().to_string()).collect()).unwrap_or_default();
    v.retain(|n| n.to_ascii_lowercase().ends_with(".xml"));
    v.sort();
    v
}

/// `upgrade-gate first-logon <stick>`. `programdata`: C:\ProgramData.
pub fn first_logon(h: &mut dyn Host, stick: &str, programdata: &str) -> i32 {
    let up = join(stick, "upgrade_");
    let wifi_log = join(&up, "go-back-wifi.log");
    let wifi = join(&up, "wifi");
    if std::path::Path::new(&wifi).exists() {
        let (_, said) = h.run("net", &["start", "wlansvc"]);
        append(&wifi_log, &said);
        h.sleep(5);
        for f in wifi_profiles(&wifi) {
            let (_, said) = h.run("netsh", &["wlan", "add", "profile", &format!("filename={}", join(&wifi, &f)), "user=all"]);
            append(&wifi_log, &said);
        }
        let _ = std::fs::remove_dir_all(&wifi);
    }
    append(&wifi_log, if std::path::Path::new(&wifi).exists() { "wifi folder NOT removed" } else { "wifi folder removed" });

    let keys = join(&up, "ssh\\authorized_keys");
    if std::path::Path::new(&keys).is_file() {
        let ssh_log = join(&up, "go-back-ssh.log");
        let d = join(programdata, "upgrade_");
        let _ = std::fs::create_dir_all(&d);
        let machine_log = join(&d, "go-back-ssh.log");
        let say = |line: &str| {
            append(&machine_log, &format!("{} {}", crate::now_utc(), line));
            append(&ssh_log, line);
        };
        let copied = std::fs::copy(&keys, join(&d, "go-back-ssh-keys")).is_ok();
        let exe = join(&d, "upgrade-gate.exe");
        let have_exe = h.own_exe().is_some_and(|me| me.eq_ignore_ascii_case(&exe) || std::fs::copy(&me, &exe).is_ok());
        let xml = join(&d, "go-back-ssh-task.xml");
        let wrote = std::fs::write(&xml, utf16_bom(&task_xml(&exe))).is_ok();
        if !(copied && have_exe && wrote) {
            say(&format!("the task was not registered: keys copied {copied}, program copied {have_exe}, task file written {wrote}"));
            return 1;
        }
        let (code, said) = h.run("schtasks", &["/create", "/tn", TASK, "/xml", &xml, "/f"]);
        append(&ssh_log, &said);
        if code != 0 {
            say(&format!("the task was not registered (schtasks exit {code})"));
            return 1;
        }
        let (_, said) = h.run("schtasks", &["/run", "/tn", TASK]);
        append(&ssh_log, &said);
        say("task registered and started");
    }
    0
}

/// `upgrade-gate ssh-task`, as SYSTEM at every start until done.
pub fn ssh_task(h: &mut dyn Host, programdata: &str) -> i32 {
    let d = join(programdata, "upgrade_");
    let log = join(&d, "go-back-ssh.log");
    let say = |line: &str| append(&log, &format!("{} {}", crate::now_utc(), line));
    let drop_task = |h: &mut dyn Host| {
        h.run("schtasks", &["/delete", "/tn", TASK, "/f"]);
    };
    let keys = join(&d, "go-back-ssh-keys");
    if std::path::Path::new(&join(&d, "go-back-ssh.done")).exists() {
        drop_task(h);
        return 0;
    }
    if !std::path::Path::new(&keys).is_file() {
        say("no keys file; nothing to do");
        drop_task(h);
        return 0;
    }
    let mut have = false;
    for i in 0..TRIES {
        if h.sshd().is_some() {
            have = true;
            break;
        }
        let (code, said) = h.run("dism", &["/online", "/add-capability", &format!("/capabilityname:{CAPABILITY}"), "/norestart"]);
        if dism_ok(code) {
            have = true;
            say("the SSH server was installed");
            break;
        }
        say(&format!("not yet (try {}): dism exit {code}: {}", i + 1, last_said(&said)));
        h.sleep(60);
    }
    if !have {
        say("the SSH server could not be downloaded this time; trying again at the next start");
        return 1;
    }
    h.start_sshd(); // its first start writes C:\ProgramData\ssh\sshd_config
    let ssh = join(programdata, "ssh");
    let _ = std::fs::create_dir_all(&ssh);
    let k = join(&ssh, "administrators_authorized_keys");
    let _ = std::fs::copy(&keys, &k);
    h.run("icacls", &[&k, "/inheritance:r", "/grant", "*S-1-5-32-544:F", "/grant", "*S-1-5-18:F"]);
    let cfg = join(&ssh, "sshd_config");
    match std::fs::read(&cfg) {
        Ok(t) => {
            if let Some(new) = keys_only(&t) {
                if std::fs::write(&cfg, new).is_err() {
                    say("sshd_config could not be written; password sign-in was NOT switched off");
                }
            }
        }
        Err(_) => say("sshd_config was not found; password sign-in was NOT switched off"),
    }
    h.sshd_automatic();
    h.stop_sshd();
    h.start_sshd();
    let (shown, _) = h.run("netsh", &["advfirewall", "firewall", "show", "rule", &format!("name={FIREWALL_RULE}")]);
    if shown != 0 {
        h.run("netsh", &["advfirewall", "firewall", "add", "rule", &format!("name={FIREWALL_RULE}"), "dir=in", "action=allow", "protocol=TCP", "localport=22", "profile=any", "enable=yes"]);
    }
    let s = h.sshd();
    let n = std::fs::read_to_string(&k).map(|t| t.lines().filter(|l| !l.trim().is_empty()).count()).unwrap_or(0);
    let off = std::fs::read(&cfg).map(|t| password_off(&t)).unwrap_or(false);
    let word = |b: bool, yes: &'static str, no: &'static str| if b { yes } else { no };
    say(&format!("sshd: {}, start {}; keys {n}; password sign-in off: {}",
        s.map_or("not installed", |s| word(s.running, "Running", "Stopped")), s.map_or("none", |s| word(s.automatic, "Automatic", "Manual")), word(off, "True", "False")));
    if s.is_some_and(|s| s.running) {
        let _ = std::fs::write(join(&d, "go-back-ssh.done"), "done");
        let _ = std::fs::remove_file(&keys);
        drop_task(h);
    }
    0
}

/// The real Windows.
#[cfg(windows)]
pub struct Real;

#[cfg(windows)]
impl Host for Real {
    fn run(&mut self, program: &str, args: &[&str]) -> (i32, String) {
        match std::process::Command::new(program).args(args).output() {
            Ok(o) => (o.status.code().unwrap_or(-1), format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))),
            Err(e) => (-1, format!("{program}: {e}")),
        }
    }
    fn sshd(&mut self) -> Option<Svc> {
        crate::win::service("sshd")
    }
    fn start_sshd(&mut self) -> bool {
        crate::win::service_start("sshd")
    }
    fn stop_sshd(&mut self) -> bool {
        crate::win::service_stop("sshd")
    }
    fn sshd_automatic(&mut self) -> bool {
        crate::win::service_automatic("sshd")
    }
    fn sleep(&mut self, secs: u64) {
        std::thread::sleep(std::time::Duration::from_secs(secs));
    }
    fn own_exe(&mut self) -> Option<String> {
        std::env::current_exe().ok().map(|p| p.to_string_lossy().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A made-up Windows: what each tool answers, whether the server is
    /// installed, and every call in order.
    #[derive(Default)]
    struct Fake {
        calls: Vec<String>,
        sshd: Option<Svc>,
        dism_codes: Vec<i32>,
        schtasks_create: i32,
        rule_exists: bool,
        exe: Option<String>,
        sshd_config_on_start: Option<(PathBuf, String)>,
    }

    impl Host for Fake {
        fn run(&mut self, program: &str, args: &[&str]) -> (i32, String) {
            self.calls.push(format!("{program} {}", args.join(" ")));
            match (program, args.first().copied()) {
                ("dism", _) => {
                    let c = if self.dism_codes.is_empty() { 0 } else { self.dism_codes.remove(0) };
                    if dism_ok(c) {
                        self.sshd = Some(Svc { running: false, automatic: false });
                    }
                    (c, if dism_ok(c) { "The operation completed successfully.\r\n".into() } else { "\r\nError: 0x800f0954\r\n\r\nDISM failed. No operation was performed.\r\n\r\n".into() })
                }
                ("schtasks", Some("/create")) => (self.schtasks_create, "SUCCESS: The scheduled task \"upgrade_ go-back ssh\" has successfully been created.\r\n".into()),
                ("netsh", Some("advfirewall")) if args.contains(&"show") => (if self.rule_exists { 0 } else { 1 }, String::new()),
                _ => (0, format!("{program} ok\r\n")),
            }
        }
        fn sshd(&mut self) -> Option<Svc> {
            self.sshd
        }
        fn start_sshd(&mut self) -> bool {
            self.calls.push("start sshd".into());
            if let Some((p, t)) = self.sshd_config_on_start.take() {
                std::fs::create_dir_all(p.parent().unwrap()).unwrap();
                std::fs::write(p, t).unwrap();
            }
            self.sshd.as_mut().map(|s| s.running = true).is_some()
        }
        fn stop_sshd(&mut self) -> bool {
            self.calls.push("stop sshd".into());
            self.sshd.as_mut().map(|s| s.running = false).is_some()
        }
        fn sshd_automatic(&mut self) -> bool {
            self.calls.push("sshd automatic".into());
            self.sshd.as_mut().map(|s| s.automatic = true).is_some()
        }
        fn sleep(&mut self, secs: u64) {
            self.calls.push(format!("sleep {secs}"));
        }
        fn own_exe(&mut self) -> Option<String> {
            self.exe.clone()
        }
    }

    fn place(tag: &str) -> (PathBuf, String, String) {
        let d = std::env::temp_dir().join(format!("gate-firstlogon-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("stick/upgrade_")).unwrap();
        std::fs::create_dir_all(d.join("ProgramData")).unwrap();
        let s = d.join("stick").to_string_lossy().to_string();
        let p = d.join("ProgramData").to_string_lossy().to_string();
        (d, s, p)
    }

    const KEY: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIMQQyvVmEy6AQRORsQwKHQO9f1hMJtfxm1tm/jWBqmvP laptop";

    #[test]
    fn wifi_profiles_are_added_for_everyone_then_the_passwords_leave_the_stick() {
        let (d, s, p) = place("wifi");
        std::fs::create_dir_all(d.join("stick/upgrade_/wifi")).unwrap();
        std::fs::write(d.join("stick/upgrade_/wifi/02.xml"), "<WLANProfile>b</WLANProfile>").unwrap();
        std::fs::write(d.join("stick/upgrade_/wifi/01.xml"), "<WLANProfile>a</WLANProfile>").unwrap();
        std::fs::write(d.join("stick/upgrade_/wifi/notes.txt"), "x").unwrap();
        let mut h = Fake::default();
        assert_eq!(first_logon(&mut h, &s, &p), 0);
        let w = d.join("stick/upgrade_/wifi");
        assert_eq!(h.calls, vec![
            "net start wlansvc".to_string(),
            "sleep 5".into(),
            format!("netsh wlan add profile filename={}/01.xml user=all", w.display()),
            format!("netsh wlan add profile filename={}/02.xml user=all", w.display()),
        ]);
        assert!(!w.exists(), "the passwords leave the stick");
        let log = std::fs::read_to_string(d.join("stick/upgrade_/go-back-wifi.log")).unwrap();
        assert!(log.contains("net ok") && log.ends_with("wifi folder removed\r\n"));
        // no keys on the stick: no task, nothing in ProgramData
        assert!(!d.join("ProgramData/upgrade_").exists());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn no_wifi_folder_still_says_so_and_calls_nothing() {
        let (d, s, p) = place("nowifi");
        let mut h = Fake::default();
        assert_eq!(first_logon(&mut h, &s, &p), 0);
        assert!(h.calls.is_empty());
        assert_eq!(std::fs::read_to_string(d.join("stick/upgrade_/go-back-wifi.log")).unwrap(), "wifi folder removed\r\n");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn keys_on_the_stick_register_the_task_with_a_copy_of_this_program() {
        let (d, s, p) = place("register");
        std::fs::create_dir_all(d.join("stick/upgrade_/ssh")).unwrap();
        std::fs::write(d.join("stick/upgrade_/ssh/authorized_keys"), format!("{KEY}\r\n")).unwrap();
        let me = d.join("stick/upgrade_/upgrade-gate.exe");
        std::fs::write(&me, "MZ-gate").unwrap();
        let mut h = Fake { exe: Some(me.to_string_lossy().to_string()), ..Default::default() };
        assert_eq!(first_logon(&mut h, &s, &p), 0);
        let up = d.join("ProgramData/upgrade_");
        assert_eq!(std::fs::read_to_string(up.join("go-back-ssh-keys")).unwrap(), format!("{KEY}\r\n"));
        assert_eq!(std::fs::read_to_string(up.join("upgrade-gate.exe")).unwrap(), "MZ-gate");
        let xml = std::fs::read(up.join("go-back-ssh-task.xml")).unwrap();
        assert_eq!(&xml[..2], &[0xFF, 0xFE]);
        let xml_path = up.join("go-back-ssh-task.xml").to_string_lossy().to_string();
        assert_eq!(h.calls, vec![format!("schtasks /create /tn {TASK} /xml {xml_path} /f"), format!("schtasks /run /tn {TASK}")]);
        assert!(std::fs::read_to_string(up.join("go-back-ssh.log")).unwrap().ends_with(" task registered and started\r\n"));
        assert!(std::fs::read_to_string(d.join("stick/upgrade_/go-back-ssh.log")).unwrap().contains("task registered and started"));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_task_schtasks_refused_is_said_and_not_started() {
        let (d, s, p) = place("refused");
        std::fs::create_dir_all(d.join("stick/upgrade_/ssh")).unwrap();
        std::fs::write(d.join("stick/upgrade_/ssh/authorized_keys"), KEY).unwrap();
        std::fs::write(d.join("stick/upgrade_/upgrade-gate.exe"), "MZ").unwrap();
        let mut h = Fake { exe: Some(d.join("stick/upgrade_/upgrade-gate.exe").to_string_lossy().to_string()), schtasks_create: 1, ..Default::default() };
        assert_eq!(first_logon(&mut h, &s, &p), 1);
        assert_eq!(h.calls.len(), 1);
        assert!(std::fs::read_to_string(d.join("ProgramData/upgrade_/go-back-ssh.log")).unwrap().contains("the task was not registered (schtasks exit 1)"));
        // and without its own file to copy, it does not register a task that could not start
        let mut h = Fake::default();
        assert_eq!(first_logon(&mut h, &s, &p), 1);
        assert!(h.calls.is_empty());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn the_task_is_system_at_every_start_on_battery_too_for_two_hours_at_most() {
        let x = task_xml("C:\\ProgramData\\upgrade_\\upgrade-gate.exe");
        for want in ["<UserId>S-1-5-18</UserId>", "<RunLevel>HighestAvailable</RunLevel>", "<BootTrigger>", "<DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>",
            "<StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>", "<StartWhenAvailable>true</StartWhenAvailable>", "<ExecutionTimeLimit>PT2H</ExecutionTimeLimit>",
            "<Command>C:\\ProgramData\\upgrade_\\upgrade-gate.exe</Command><Arguments>ssh-task</Arguments>", "encoding=\"UTF-16\""] {
            assert!(x.contains(want), "{want}");
        }
        let b = utf16_bom("A<");
        assert_eq!(b, vec![0xFF, 0xFE, b'A', 0, b'<', 0]);
    }

    #[test]
    fn password_sign_in_goes_off_once_on_top() {
        let cfg = b"# This is the sshd server system-wide configuration file.\r\n#PasswordAuthentication yes\r\nSubsystem sftp sftp-server.exe\r\n";
        let new = keys_only(cfg).unwrap();
        let t = String::from_utf8(new.clone()).unwrap();
        assert!(t.starts_with("# upgrade_: keys only, carried from Linux\r\nPasswordAuthentication no\r\nKbdInteractiveAuthentication no\r\n# This is the sshd"));
        assert!(t.ends_with("sftp-server.exe\r\n"), "the rest is kept byte for byte");
        assert!(keys_only(&new).is_none(), "never twice");
        assert!(password_off(&new));
        assert!(!password_off(cfg));
        // bytes that are not text are kept as they were
        let odd = b"Banner caf\xe9\r\n";
        assert!(keys_only(odd).unwrap().ends_with(odd));
    }

    #[test]
    fn dism_success_and_its_words() {
        assert!(dism_ok(0) && dism_ok(3010) && !dism_ok(1) && !dism_ok(-2146498540));
        assert_eq!(last_said("\r\nError: 0x800f0954\r\n\r\nDISM failed. No operation was performed.\r\n\r\n"), "DISM failed. No operation was performed.");
        assert_eq!(last_said(""), "");
    }

    fn task_place(tag: &str) -> (PathBuf, String) {
        let (d, _, p) = place(tag);
        std::fs::create_dir_all(d.join("ProgramData/upgrade_")).unwrap();
        std::fs::write(d.join("ProgramData/upgrade_/go-back-ssh-keys"), format!("{KEY}\r\n")).unwrap();
        (d, p)
    }

    #[test]
    fn the_task_installs_the_server_lets_in_the_keys_only_and_removes_itself() {
        let (d, p) = task_place("task");
        let ssh = d.join("ProgramData/ssh");
        let mut h = Fake { dism_codes: vec![1, 1, 3010], sshd_config_on_start: Some((ssh.join("sshd_config"), "#PasswordAuthentication yes\r\n".into())), ..Default::default() };
        assert_eq!(ssh_task(&mut h, &p), 0);
        let dism = format!("dism /online /add-capability /capabilityname:{CAPABILITY} /norestart");
        let k = ssh.join("administrators_authorized_keys").to_string_lossy().to_string();
        assert_eq!(h.calls, vec![
            dism.clone(), "sleep 60".into(), dism.clone(), "sleep 60".into(), dism,
            "start sshd".into(),
            format!("icacls {k} /inheritance:r /grant *S-1-5-32-544:F /grant *S-1-5-18:F"),
            "sshd automatic".into(), "stop sshd".into(), "start sshd".into(),
            format!("netsh advfirewall firewall show rule name={FIREWALL_RULE}"),
            format!("netsh advfirewall firewall add rule name={FIREWALL_RULE} dir=in action=allow protocol=TCP localport=22 profile=any enable=yes"),
            format!("schtasks /delete /tn {TASK} /f"),
        ]);
        assert_eq!(std::fs::read_to_string(&k).unwrap(), format!("{KEY}\r\n"));
        assert!(std::fs::read_to_string(ssh.join("sshd_config")).unwrap().starts_with("# upgrade_: keys only"));
        let up = d.join("ProgramData/upgrade_");
        assert!(up.join("go-back-ssh.done").exists() && !up.join("go-back-ssh-keys").exists());
        let log = std::fs::read_to_string(up.join("go-back-ssh.log")).unwrap();
        assert!(log.contains("not yet (try 1): dism exit 1: DISM failed. No operation was performed.") && log.contains("not yet (try 2)"));
        assert!(log.contains("the SSH server was installed"));
        assert!(log.trim_end().ends_with("sshd: Running, start Automatic; keys 1; password sign-in off: True"), "{log}");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn an_installed_server_and_an_existing_rule_are_used_as_they_are() {
        let (d, p) = task_place("present");
        std::fs::create_dir_all(d.join("ProgramData/ssh")).unwrap();
        std::fs::write(d.join("ProgramData/ssh/sshd_config"), "# upgrade_: keys only, carried from Linux\r\nPasswordAuthentication no\r\n").unwrap();
        let mut h = Fake { sshd: Some(Svc { running: true, automatic: true }), rule_exists: true, ..Default::default() };
        assert_eq!(ssh_task(&mut h, &p), 0);
        assert!(!h.calls.iter().any(|c| c.starts_with("dism") || c.contains("add rule")));
        assert_eq!(std::fs::read_to_string(d.join("ProgramData/ssh/sshd_config")).unwrap().matches("keys only").count(), 1);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn no_network_all_day_means_try_again_at_the_next_start() {
        let (d, p) = task_place("offline");
        let mut h = Fake { dism_codes: vec![1; TRIES as usize], ..Default::default() };
        assert_eq!(ssh_task(&mut h, &p), 1);
        assert_eq!(h.calls.iter().filter(|c| c.starts_with("dism")).count(), TRIES as usize);
        assert!(!h.calls.iter().any(|c| c.starts_with("schtasks")), "the task stays for the next start");
        assert!(d.join("ProgramData/upgrade_/go-back-ssh-keys").exists());
        assert!(std::fs::read_to_string(d.join("ProgramData/upgrade_/go-back-ssh.log")).unwrap().contains("trying again at the next start"));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_server_that_does_not_run_keeps_the_task_and_the_keys() {
        let (d, p) = task_place("stopped");
        struct NoStart(Fake);
        impl Host for NoStart {
            fn run(&mut self, a: &str, b: &[&str]) -> (i32, String) { self.0.run(a, b) }
            fn sshd(&mut self) -> Option<Svc> { self.0.sshd }
            fn start_sshd(&mut self) -> bool { false }
            fn stop_sshd(&mut self) -> bool { true }
            fn sshd_automatic(&mut self) -> bool { true }
            fn sleep(&mut self, _: u64) {}
            fn own_exe(&mut self) -> Option<String> { None }
        }
        let mut h = NoStart(Fake { sshd: Some(Svc { running: false, automatic: true }), ..Default::default() });
        assert_eq!(ssh_task(&mut h, &p), 0);
        let up = d.join("ProgramData/upgrade_");
        assert!(!up.join("go-back-ssh.done").exists() && up.join("go-back-ssh-keys").exists());
        assert!(!h.0.calls.iter().any(|c| c.starts_with("schtasks")));
        let log = std::fs::read_to_string(up.join("go-back-ssh.log")).unwrap();
        assert!(log.contains("sshd: Stopped, start Automatic") && log.contains("sshd_config was not found; password sign-in was NOT switched off"));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn done_or_no_keys_removes_the_task_and_changes_nothing() {
        let (d, p) = task_place("done");
        std::fs::write(d.join("ProgramData/upgrade_/go-back-ssh.done"), "done").unwrap();
        let mut h = Fake::default();
        assert_eq!(ssh_task(&mut h, &p), 0);
        assert_eq!(h.calls, vec![format!("schtasks /delete /tn {TASK} /f")]);
        std::fs::remove_file(d.join("ProgramData/upgrade_/go-back-ssh.done")).unwrap();
        std::fs::remove_file(d.join("ProgramData/upgrade_/go-back-ssh-keys")).unwrap();
        let mut h = Fake::default();
        assert_eq!(ssh_task(&mut h, &p), 0);
        assert_eq!(h.calls, vec![format!("schtasks /delete /tn {TASK} /f")]);
        assert!(std::fs::read_to_string(d.join("ProgramData/upgrade_/go-back-ssh.log")).unwrap().contains("no keys file; nothing to do"));
        std::fs::remove_dir_all(&d).unwrap();
    }
}
