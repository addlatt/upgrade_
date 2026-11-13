//! Ubuntu's installer (subiquity) has three hooks, and these are ours
//! (roadmap item 23; RISKS R40, R45; docs/UBUNTU-DESIGN.md):
//!
//!   early   before the installer looks at any disk. The refusal gate, as
//!           `verify` is Anaconda's %pre. The stick's own answer file
//!           names a disk that does not exist; only when every check here
//!           has passed is the real file written over it, naming this
//!           machine's disk and no other. A refusal restarts the machine
//!           itself: the installer would wait at a button for ever.
//!   late    after the install, the new system at /target.
//!   failed  the installer's error hook: the record goes to the stick.
//!
//! What is read from the machine is collected into [`Facts`]; what is
//! decided is [`judge`], which reads nothing and is tested on made-up
//! facts (rule #5). Only the erase exists: a keep-Windows job is refused.

use crate::util::*;
use crate::verify::{blockdev_size, resolve_disk};
use serde_json::{json, Value};
use std::path::Path;
use upgrade_autoinstall::{Disk, Storage, STICK};

const REPORT: &str = "/cdrom/upgrade_/report";
const LOG: &str = "/tmp/upgrade_-early.log";
const ANSWER: &str = "/autoinstall.yaml";
const PROGRAM: &str = "/tmp/upgrade-installer";

/// One disk the job names, as this machine shows it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Found {
    /// the device, empty when no attached disk carries the job's identity
    pub dev: String,
    pub matched_by: String,
    pub size: String,
    pub serial: String,
}

/// Everything the early check read, and nothing it concluded.
#[derive(Debug, Clone)]
pub struct Facts {
    pub job: Value,
    pub mode_verify: bool,
    pub system: Found,
    /// the second drive of an erase job, when the job names one
    pub home: Option<Found>,
    /// the stick already converted a computer (upgrade_/converted)
    pub converted: Option<String>,
    /// the stick's Ubuntu files read back against its SHA256SUMS: what passed, or what is wrong
    pub payload: Result<String, String>,
}

/// What the early check does next.
#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    /// exit code, the plain words for the screen, the detail for the log
    Refuse(i32, String, String),
    /// verify mode: report and restart, nothing installed
    ReportOnly,
    Install(Storage),
}

fn refuse(code: i32, plain: &str, detail: &str) -> Decision {
    Decision::Refuse(code, plain.to_string(), detail.to_string())
}

/// The refusals, in order. The codes are `verify`'s where the reason is
/// the same (10, 20, 23, 25, 28); 26 and 27 are Ubuntu's own.
pub fn judge(f: &Facts) -> Decision {
    let j = &f.job;
    let schema = jq(j, "schema");
    if schema != "job/1" {
        return refuse(10, "this USB stick was prepared by a different version of this tool", &format!("job schema '{schema}' is not job/1"));
    }
    let distro = jq(j, "intent.distro.name");
    if distro != "ubuntu" {
        return refuse(10, "this USB stick was prepared by a different version of this tool", &format!("the job's distro is '{distro}', and this is Ubuntu's installer"));
    }
    if let Some(head) = &f.converted {
        return refuse(28, "this USB stick has already converted this computer, and it never installs twice. Take the stick out and restart", &format!("the stick carries upgrade_/converted ({head}) - refusing a second install (R35)"));
    }
    let (j_uid, j_size) = (jq(j, "identity.system_disk.unique_id"), jq(j, "identity.system_disk.size_bytes"));
    let mismatch = |detail: String| refuse(20, "this USB stick was prepared for a different computer, or one of its drives has changed since", &format!("IDENTITY MISMATCH - {detail}"));
    if f.system.dev.is_empty() {
        return mismatch("no attached disk carries the job's unique id or serial".into());
    }
    if f.system.size.is_empty() || f.system.size != j_size {
        return mismatch(format!("size mismatch: job says {j_size}, {} is {}", f.system.dev, f.system.size));
    }
    let erase = !jq(j, "erase_consent.statement").is_empty();
    let ndisks = if erase { jq_len(j, "erase_consent.disks") } else { 0 };
    if erase && (jq(j, "erase_consent.disks.0.role") != "system" || jq(j, "erase_consent.disks.0.unique_id") != j_uid || jq(j, "erase_consent.disks.0.size_bytes") != j_size) {
        return mismatch("erase_consent.disks[0] is not the system disk the job identifies".into());
    }
    if ndisks > 2 {
        return mismatch(format!("the job names {ndisks} drives to erase; this installer knows one or two"));
    }
    let mut home = None;
    if ndisks == 2 {
        let Some(h) = &f.home else {
            return mismatch("the job names a home drive and it was not looked for".into());
        };
        if h.dev.is_empty() {
            return mismatch(format!("the home drive the job names (serial '{}') is not attached", jq(j, "erase_consent.disks.1.serial_number")));
        }
        let want = jq(j, "erase_consent.disks.1.size_bytes");
        if h.size.is_empty() || h.size != want {
            return mismatch(format!("home size mismatch: job says {want}, {} is {}", h.dev, h.size));
        }
        if h.dev == f.system.dev {
            return mismatch("the home drive resolved to the system drive".into());
        }
        home = Some(Disk { path: h.dev.clone(), serial: h.serial.clone() });
    }
    if let Err(why) = &f.payload {
        return refuse(23, "the copy of Linux on this USB stick is damaged", &format!("the stick's Ubuntu files did not verify (RISKS R17): {why}"));
    }
    if f.mode_verify {
        return Decision::ReportOnly;
    }
    let path = jq(j, "intent.path");
    if path == "keep-windows" {
        return refuse(26, "this version cannot yet install Ubuntu beside Windows", "a keep-windows job: Ubuntu's storage list beside Windows is not written (docs/UBUNTU-DESIGN.md)");
    }
    if !erase {
        return refuse(25, "this version cannot yet put your files back on an erased computer", "a clean slate with staged files needs the restore, which this version does not have");
    }
    Decision::Install(Storage::Erase { system: Disk { path: f.system.dev.clone(), serial: f.system.serial.clone() }, home })
}

/// One `sha256sum` line: the hash and the path, forward slashes, no `./`.
fn manifest_line(l: &str) -> Option<(String, String)> {
    let (h, rest) = l.split_at_checked(64)?;
    if !h.bytes().all(|b| b.is_ascii_hexdigit()) || !rest.starts_with([' ', '\t']) {
        return None;
    }
    let p = rest.trim_start_matches([' ', '\t', '*']).trim_end().replace('\\', "/");
    Some((h.to_lowercase(), p.strip_prefix("./").unwrap_or(&p).to_string()))
}

/// Which of the manifest's files are Ubuntu's own (what the installer
/// starts from and installs), and what is wrong with the list itself.
pub fn payload_plan(manifest: &str, on_stick: &[String]) -> Result<Vec<(String, String)>, String> {
    let ours = |p: &str| ["casper/", "pool/", "dists/", "boot/grub/", "efi/boot/"].iter().any(|d| p.to_lowercase().starts_with(d));
    let listed: Vec<(String, String)> = manifest.lines().filter_map(manifest_line).filter(|(_, p)| ours(p)).collect();
    let has = |want: &str| listed.iter().any(|(_, p)| p.eq_ignore_ascii_case(want));
    for need in ["casper/vmlinuz", "casper/initrd", "casper/install-sources.yaml", "EFI/boot/bootx64.efi", "EFI/boot/grubx64.efi"] {
        if !has(need) {
            return Err(format!("SHA256SUMS does not name {need}"));
        }
    }
    if !listed.iter().any(|(_, p)| p.to_lowercase().ends_with(".squashfs")) {
        return Err("SHA256SUMS names no system image".into());
    }
    // a file the manifest never measured does not get installed from
    for f in on_stick {
        let l = f.to_lowercase();
        if (l.starts_with("casper/") || l.starts_with("pool/")) && !has(f) {
            return Err(format!("{f} is on the stick and not in SHA256SUMS"));
        }
    }
    Ok(listed)
}

fn walk(dir: &Path, rel: &str, out: &mut Vec<String>) {
    for name in sorted_entries(&dir.to_string_lossy()) {
        let (p, r) = (dir.join(&name), if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") });
        if p.is_dir() {
            walk(&p, &r, out);
        } else {
            out.push(r);
        }
    }
}

/// The stick's path for a manifest path, whatever case the stick spells it in.
fn on_stick_path(rel: &str) -> Option<String> {
    let mut at = STICK.to_string();
    for part in rel.split('/') {
        let hit = sorted_entries(&at).into_iter().find(|n| n.eq_ignore_ascii_case(part))?;
        at = format!("{at}/{hit}");
    }
    Some(at)
}

fn read_payload(log: &Log) -> Result<String, String> {
    let sums = format!("{STICK}/SHA256SUMS");
    if !exists(&sums) {
        return Err("no SHA256SUMS on the stick".into());
    }
    let mut files = Vec::new();
    for d in ["casper", "pool"] {
        walk(Path::new(&format!("{STICK}/{d}")), d, &mut files);
    }
    let plan = payload_plan(&read_to_string(&sums), &files)?;
    let (t0, mut bytes) = (std::time::Instant::now(), 0u64);
    for (want, rel) in &plan {
        let Some(p) = on_stick_path(rel) else {
            return Err(format!("missing: {rel}"));
        };
        bytes += std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
        let got = sha256_file(Path::new(&p)).unwrap_or_default();
        if &got != want {
            return Err(format!("{rel} sha256 MISMATCH (want {want} got {got})"));
        }
    }
    let secs = t0.elapsed().as_secs_f64().max(0.001);
    let ok = format!("{} files, {bytes} bytes sha256 ok, read at {:.1} MB/s", plan.len(), bytes as f64 / 1e6 / secs);
    log.say(&format!("== the stick's Ubuntu files: {ok}"));
    Ok(ok)
}

fn find(uid: &str, serial: &str) -> Found {
    let (dev, matched_by) = resolve_disk(uid, serial);
    if dev.is_empty() {
        return Found::default();
    }
    let size = blockdev_size(&dev);
    let udev = run("udevadm", &["info", "--query=property", "--name", &dev]).out;
    let serial = udev.lines().find_map(|l| l.strip_prefix("ID_SERIAL=")).unwrap_or("").to_string();
    Found { dev, matched_by, size, serial }
}

fn collect(job_path: &str, log: &Log) -> Facts {
    let job: Value = std::fs::read_to_string(job_path).ok().and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok()).unwrap_or(Value::Null);
    let cmdline = read_to_string("/proc/cmdline");
    log.say(&format!("== cmdline: {}", cmdline.trim_end()));
    let system = find(&jq(&job, "identity.system_disk.unique_id"), &jq(&job, "identity.system_disk.serial_number"));
    log.say(&format!("== job {} path={} system disk: {:?}", jq(&job, "job_id"), jq(&job, "intent.path"), system));
    let home = (jq_len(&job, "erase_consent.disks") >= 2).then(|| find(&jq(&job, "erase_consent.disks.1.unique_id"), &jq(&job, "erase_consent.disks.1.serial_number")));
    if let Some(h) = &home {
        log.say(&format!("== home disk: {h:?}"));
    }
    log.say_text(&run("lsblk", &["-b", "-o", "NAME,SIZE,TYPE,FSTYPE,LABEL,PARTTYPENAME,SERIAL,WWN"]).out);
    let marker = format!("{STICK}/upgrade_/converted");
    let converted = exists(&marker).then(|| std::fs::read(&marker).unwrap_or_default().into_iter().take(200).map(|b| b as char).collect::<String>().replace('\n', " "));
    // the long read comes last, and only for a stick that could still install
    let payload = if converted.is_some() || system.dev.is_empty() { Err("not read: the stick was refused before its files mattered".to_string()) } else { read_payload(log) };
    Facts { job, mode_verify: cmdline.split_whitespace().any(|w| w == "upg.mode=verify"), system, home, converted, payload }
}

fn stick_rw() -> bool {
    run("mount", &["-o", "remount,rw", STICK]).ok()
}

fn save(name: &str, text: &str) {
    let _ = std::fs::create_dir_all(REPORT);
    let _ = std::fs::write(format!("{REPORT}/{name}"), text);
}

/// The refusal a person can read, then the restart. The installer's own
/// failure screen has no way back to Windows, so this never returns to it.
fn refuse_and_restart(log: &Log, code: i32, plain: &str, detail: &str) -> ! {
    log.say(&format!("!! {detail}"));
    log.say(&format!("== REFUSED (exit {code}): {plain}"));
    stick_rw();
    save("refusal.json", &serde_json::to_string_pretty(&json!({"schema": "refusal/1", "exit_code": code, "reason": plain, "detail": detail, "installer": "subiquity", "created_utc": utc_now()})).unwrap_or_default());
    let _ = std::fs::copy(LOG, format!("{REPORT}/early.log"));
    sync();
    sync();
    let secs = test_secs("UPG_TEST_REFUSAL_SECS", 60) as u64;
    let body = format!("\n\n   NOTHING WAS CHANGED ON THIS COMPUTER.\n\n   The installer stopped before touching anything, because\n   {plain}.\n\n   It restarts in {secs} seconds, exactly as it was before.\n\n   The details are saved on the USB stick, in upgrade_\\report.\n");
    // every screen that is open says so: the text consoles, and the desktop's users
    for tty in ["/dev/tty1", "/dev/tty9", "/dev/console"] {
        let _ = std::fs::write(tty, format!("\x1b[2J\x1b[H{body}"));
    }
    run("chvt", &["9"]);
    run("wall", &[body.trim()]);
    sleep(&secs.to_string());
    reboot();
    sleep("60");
    std::process::exit(code);
}

pub fn early(job_path: &str) -> i32 {
    let log = Log { path: LOG.into() };
    log.say(&format!("== upgrade_ early check (upgrade-installer {}, Rust; upgrade-autoinstall {})  {}", env!("CARGO_PKG_VERSION"), upgrade_autoinstall::GENERATOR_VERSION, utc_now()));
    let facts = collect(job_path, &log);
    let decision = judge(&facts);
    log.say(&format!("== decision: {decision:?}"));
    let storage = match decision {
        Decision::Refuse(code, plain, detail) => refuse_and_restart(&log, code, &plain, &detail),
        Decision::ReportOnly => {
            stick_rw();
            save("early.json", &serde_json::to_string_pretty(&json!({"schema": "early/1", "mode": "verify", "job_id": jq(&facts.job, "job_id"), "system": facts.system.dev, "matched_by": facts.system.matched_by, "payload": facts.payload.clone().unwrap_or_default(), "created_utc": utc_now()})).unwrap_or_default());
            let _ = std::fs::copy(LOG, format!("{REPORT}/early.log"));
            sync();
            log.say("== verify mode: restarting to Windows (nothing installed, nothing changed on the internal disk)");
            reboot();
            sleep("60");
            // never let the installer go on from a verify start
            return 30;
        }
        Decision::Install(s) => s,
    };
    let text = match upgrade_autoinstall::autoinstall(&facts.job, &storage, PROGRAM, job_path) {
        Ok(t) => t,
        Err(why) => refuse_and_restart(&log, 27, "the installer could not work out where to put Linux", &format!("no answer file: {why}")),
    };
    // the stick disarms itself before anything is erased (R35), as verify does
    let (converted, armed) = (format!("{STICK}/upgrade_/converted"), format!("{STICK}/upgrade_/boot-install"));
    stick_rw();
    let _ = std::fs::write(&converted, format!("early (subiquity) job {} committed {}\n", jq(&facts.job, "job_id"), utc_now()));
    let _ = std::fs::remove_file(&armed);
    sync();
    sync();
    if !std::fs::metadata(&converted).map(|m| m.len() > 0).unwrap_or(false) || exists(&armed) {
        let _ = std::fs::remove_file(&converted);
        refuse_and_restart(&log, 29, "this USB stick could not be switched off as an installer, and without that it could install again by itself", "could not write upgrade_/converted or remove upgrade_/boot-install on the stick - no install without a disarmed stick (R35)");
    }
    log.say("== the stick is disarmed: upgrade_/converted written");
    // the real answer file, over the stick's placeholder: the installer reads it again after this hook
    if let Err(e) = std::fs::write(ANSWER, &text) {
        let _ = std::fs::remove_file(&converted);
        refuse_and_restart(&log, 27, "the installer could not work out where to put Linux", &format!("could not write {ANSWER}: {e}"));
    }
    save("autoinstall.yaml", &text);
    save("commit.json", &serde_json::to_string_pretty(&json!({"schema": "commit/1", "by": "the-window", "crossed_utc": utc_now()})).unwrap_or_default());
    save("early.json", &serde_json::to_string_pretty(&json!({"schema": "early/1", "mode": "install", "job_id": jq(&facts.job, "job_id"), "system": facts.system.dev, "matched_by": facts.system.matched_by, "system_serial": facts.system.serial, "home": facts.home.as_ref().map(|h| h.dev.clone()), "payload": facts.payload.clone().unwrap_or_default(), "secure_boot": run("mokutil", &["--sb-state"]).out.trim(), "created_utc": utc_now()})).unwrap_or_default());
    log.say(&format!("== {ANSWER} written for {} ({}); the install goes on", facts.system.dev, facts.system.matched_by));
    let _ = std::fs::copy(LOG, format!("{REPORT}/early.log"));
    sync();
    0
}

/// After the install: the record and the installer's own logs onto the
/// stick, and what the computer starts at. Never fails the install: the
/// system is already written.
pub fn late(job_path: &str) -> i32 {
    let job: Value = std::fs::read_to_string(job_path).ok().and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok()).unwrap_or(Value::Null);
    if jq(&job, "intent.start_at") == "console" {
        run("chroot", &["/target", "systemctl", "set-default", "multi-user.target"]);
    }
    // the bench's first-start marker, only on a stick the rig marked (as Fedora's bench marker)
    if exists(&format!("{STICK}/upgrade_/bench")) && exists(&format!("{STICK}/upgrade_/bench-firstboot.sh")) {
        let _ = std::fs::copy(format!("{STICK}/upgrade_/bench-firstboot.sh"), "/target/usr/local/sbin/upg-bench-firstboot.sh");
        let _ = std::fs::write("/target/etc/systemd/system/upg-bench-firstboot.service", "[Unit]\nDescription=upgrade_ rig: record the first start\nAfter=multi-user.target\n[Service]\nType=oneshot\nExecStart=/bin/sh /usr/local/sbin/upg-bench-firstboot.sh\n[Install]\nWantedBy=multi-user.target\n");
        let _ = std::os::unix::fs::symlink("/etc/systemd/system/upg-bench-firstboot.service", "/target/etc/systemd/system/multi-user.target.wants/upg-bench-firstboot.service");
    }
    stick_rw();
    let rec = json!({"schema": "late/1", "job_id": jq(&job, "job_id"), "created_utc": utc_now(),
        "lsblk": run("lsblk", &["-b", "-o", "NAME,SIZE,TYPE,FSTYPE,PARTTYPENAME,MOUNTPOINTS"]).out,
        "firmware_entries": run("efibootmgr", &["-v"]).merged(), "fstab": read_to_string("/target/etc/fstab"),
        "kernel": run("uname", &["-r"]).out.trim(), "secure_boot": run("mokutil", &["--sb-state"]).out.trim()});
    save("late.json", &serde_json::to_string_pretty(&rec).unwrap_or_default());
    run("tar", &["czf", &format!("{REPORT}/installer-logs.tgz", ), "/var/log/installer"]);
    sync();
    sync();
    0
}

/// The installer's error hook. The record goes to the stick; nothing is restarted
/// here, because a failure after the erase has no Windows to go back to.
pub fn failed(job_path: &str) -> i32 {
    let job: Value = std::fs::read_to_string(job_path).ok().and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok()).unwrap_or(Value::Null);
    stick_rw();
    save("failed.json", &serde_json::to_string_pretty(&json!({"schema": "failed/1", "job_id": jq(&job, "job_id"), "created_utc": utc_now(), "lsblk": run("lsblk", &["-b", "-o", "NAME,SIZE,TYPE,FSTYPE,PARTTYPENAME"]).out})).unwrap_or_default());
    run("tar", &["czf", &format!("{REPORT}/installer-logs.tgz"), "/var/log/installer"]);
    sync();
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    const UID: &str = "60022480d990bd51d172abb88dc5d7c3";
    fn job(path: &str, disks: usize) -> Value {
        let mut d = vec![json!({"role": "system", "unique_id": UID, "serial_number": "S1", "size_bytes": 42949672960u64})];
        if disks == 2 {
            d.push(json!({"role": "home", "unique_id": "600224801111", "serial_number": "H1", "size_bytes": 1000}));
        }
        let mut j = json!({"schema": "job/1", "job_id": "j", "identity": {"system_disk": {"unique_id": UID, "serial_number": "S1", "size_bytes": 42949672960u64}},
            "intent": {"path": path, "distro": {"name": "ubuntu", "release": "26.04.1"}}});
        if disks > 0 {
            j["erase_consent"] = json!({"statement": "I confirm that everything on this computer will be deleted and nothing will be kept", "disks": d});
        }
        j
    }
    fn sda() -> Found {
        Found { dev: "/dev/sda".into(), matched_by: "unique_id via wwn-0x60022480d990bd51d172abb88dc5d7c3".into(), size: "42949672960".into(), serial: "3600224".into() }
    }
    fn facts() -> Facts {
        Facts { job: job("clean-slate", 1), mode_verify: false, system: sda(), home: None, converted: None, payload: Ok("ok".into()) }
    }
    fn code(f: &Facts) -> i32 {
        match judge(f) {
            Decision::Refuse(c, _, _) => c,
            Decision::ReportOnly => -1,
            Decision::Install(_) => 0,
        }
    }

    #[test]
    fn the_erase_names_the_matched_disk_and_only_it() {
        assert_eq!(judge(&facts()), Decision::Install(Storage::Erase { system: Disk { path: "/dev/sda".into(), serial: "3600224".into() }, home: None }));
    }

    #[test]
    fn a_stick_on_another_computer_is_refused() {
        let mut f = facts();
        f.system = Found::default();
        assert_eq!(code(&f), 20);
        let mut f = facts();
        f.system.size = "42949672961".into();
        assert_eq!(code(&f), 20);
        let mut f = facts();
        f.system.size = String::new();
        assert_eq!(code(&f), 20);
        // the consent names another disk than the identity
        let mut f = facts();
        f.job["erase_consent"]["disks"][0]["unique_id"] = json!("6002248099");
        assert_eq!(code(&f), 20);
    }

    #[test]
    fn a_second_start_is_refused_before_anything_else_is_weighed() {
        let mut f = facts();
        f.converted = Some("early job j committed".into());
        assert_eq!(code(&f), 28);
        f.system = Found::default();
        assert_eq!(code(&f), 28);
    }

    #[test]
    fn the_other_refusals() {
        let mut f = facts();
        f.job["schema"] = json!("job/2");
        assert_eq!(code(&f), 10);
        let mut f = facts();
        f.job["intent"]["distro"]["name"] = json!("fedora");
        assert_eq!(code(&f), 10);
        let mut f = facts();
        f.job = Value::Null;
        assert_eq!(code(&f), 10);
        let mut f = facts();
        f.payload = Err("casper/minimal.squashfs sha256 MISMATCH".into());
        assert_eq!(code(&f), 23);
        let mut f = facts();
        f.job = job("keep-windows", 0);
        assert_eq!(code(&f), 26);
        let mut f = facts();
        f.job = job("clean-slate", 0);
        assert_eq!(code(&f), 25);
    }

    #[test]
    fn the_home_drive_must_be_found_at_its_size_and_be_another_disk() {
        let mut f = facts();
        f.job = job("clean-slate", 2);
        assert_eq!(code(&f), 20, "not looked for");
        f.home = Some(Found::default());
        assert_eq!(code(&f), 20, "not attached");
        f.home = Some(Found { dev: "/dev/sdb".into(), size: "999".into(), ..Found::default() });
        assert_eq!(code(&f), 20, "wrong size");
        f.home = Some(Found { dev: "/dev/sda".into(), size: "1000".into(), ..Found::default() });
        assert_eq!(code(&f), 20, "the system drive");
        f.home = Some(Found { dev: "/dev/sdb".into(), size: "1000".into(), ..Found::default() });
        assert_eq!(judge(&f), Decision::Install(Storage::Erase { system: Disk { path: "/dev/sda".into(), serial: "3600224".into() }, home: Some(Disk { path: "/dev/sdb".into(), serial: String::new() }) }));
    }

    #[test]
    fn verify_mode_reports_and_installs_nothing_but_still_refuses() {
        let mut f = facts();
        f.mode_verify = true;
        assert_eq!(judge(&f), Decision::ReportOnly);
        f.job = job("keep-windows", 0);
        assert_eq!(judge(&f), Decision::ReportOnly);
        f.system = Found::default();
        assert_eq!(code(&f), 20);
    }

    #[test]
    fn the_manifest_must_name_what_the_installer_starts_from_and_everything_it_installs_from() {
        let h = "a".repeat(64);
        let full: String = ["casper/vmlinuz", "casper/initrd", "casper/install-sources.yaml", "casper/minimal.squashfs", "EFI/boot/bootx64.efi", "EFI/boot/grubx64.efi", "pool/main/s/shim/shim.deb", "upgrade_/job.json"].iter().map(|p| format!("{h}  ./{p}\n")).collect();
        let stick: Vec<String> = ["casper/vmlinuz", "casper/initrd", "casper/install-sources.yaml", "casper/minimal.squashfs", "pool/main/s/shim/shim.deb"].iter().map(|s| s.to_string()).collect();
        let plan = payload_plan(&full, &stick).unwrap();
        assert_eq!(plan.len(), 7, "the kit's own files are not Ubuntu's");
        assert!(payload_plan(&full.replace("casper/initrd", "casper/other"), &stick).unwrap_err().contains("does not name casper/initrd"));
        assert!(payload_plan(&full.replace("minimal.squashfs", "minimal.img"), &stick[..3].to_vec()).unwrap_err().contains("no system image"));
        let mut extra = stick.clone();
        extra.push("casper/planted.squashfs".into());
        assert!(payload_plan(&full, &extra).unwrap_err().contains("casper/planted.squashfs is on the stick and not in SHA256SUMS"));
        // Windows wrote the stick: backslashes, a star, upper-case folders
        let win = full.replace("  ./", " *").replace('/', "\\").replace("EFI\\boot", "EFI\\BOOT");
        assert_eq!(payload_plan(&win, &stick).unwrap().len(), 7);
        assert!(payload_plan("", &stick).is_err());
    }
}
