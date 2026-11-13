//! The autoinstall generator: a `job.json` goes in, the answer file
//! Ubuntu's installer (subiquity) runs from comes out. Fedora's is
//! `upgrade_/kickstart`.
//!
//! Ubuntu's installer asks nothing once `autoinstall` is on the kernel
//! line, and left to its defaults it picks the disk itself: "the largest
//! disk" (RISKS R45, seen on the rig 2026-11-12). So there are two files:
//!
//! - [`stick_file`], the only answer file the stick carries. It names no
//!   account and a disk that cannot exist, so by itself it installs
//!   nothing. Its one job is to start our early check.
//! - [`autoinstall`], the real file. The early check (`upgrade-installer
//!   early`) writes it over the first one only after every check passed,
//!   with the disk it matched against the job, by path and by serial. The
//!   installer reads the file again after its early commands.
//!
//! It reads no hardware and decides nothing: the disk comes in as
//! [`Disk`], found by the caller.

use serde_json::Value;

pub const GENERATOR_VERSION: &str = "0.2.0";

/// A serial no disk has. The stick's own file asks for it, so an install
/// that runs without our early check finds no disk and stops.
pub const NO_DISK: &str = "UPGRADE-NO-DISK-CHOSEN";

/// Where the installer's live system mounts the stick.
pub const STICK: &str = "/cdrom";

/// One whole disk the job named and the early check found on this machine.
#[derive(Debug, Clone, PartialEq)]
pub struct Disk {
    /// the Linux device, `/dev/sda`
    pub path: String,
    /// udev's `ID_SERIAL`, or empty when the disk reports none
    pub serial: String,
}

/// A partition that is already there and stays exactly as it is.
#[derive(Debug, Clone, PartialEq)]
pub struct Kept {
    pub number: u32,
    /// where it starts and how long it is, in bytes
    pub offset: u64,
    pub size: u64,
    /// the GPT type, lower case
    pub type_guid: String,
}

/// A partition the install makes, in space that is free today.
#[derive(Debug, Clone, PartialEq)]
pub struct New {
    pub number: u32,
    pub offset: u64,
    pub size: u64,
}

/// Where the install goes.
#[derive(Debug, Clone, PartialEq)]
pub enum Storage {
    /// Erase `system` whole: a boot partition, `/boot`, `/`. A second drive,
    /// when the job names one, is erased whole for `/home`.
    Erase { system: Disk, home: Option<Disk> },
    /// Beside Windows: every partition on the disk is named and kept, the
    /// one in `esp` (Windows' boot partition) is mounted and never
    /// formatted, and `/boot` and `/` go at exact places in free space.
    Beside { disk: Disk, kept: Vec<Kept>, esp: u32, boot: New, root: New },
}

/// The GPT type of an EFI boot partition.
pub const ESP_TYPE: &str = "c12a7328-f81f-11d2-ba4b-00a0c93ec93b";

fn text(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}

/// A YAML scalar that is safe whatever the text holds: a JSON string is
/// one (double quotes, the same escapes).
fn q(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

fn login_name(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty() && b.len() <= 32 && (b[0].is_ascii_lowercase() || b[0] == b'_') && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_' || *c == b'-')
}

/// The refusal at the point of use, as the kickstart generator has one.
pub fn check(job: &Value) -> Result<(), String> {
    let at = |p: &str| text(job.pointer(p));
    let schema = at("/schema");
    if schema != "job/1" {
        return Err(format!("job schema '{schema}' is not job/1; refusing to generate"));
    }
    for k in ["job_id", "identity", "intent", "fork", "storage", "harvest", "stick"] {
        if job.get(k).is_none() {
            return Err(format!("job.json lacks '{k}'; refusing"));
        }
    }
    let distro = at("/intent/distro/name");
    if distro != "ubuntu" {
        return Err(format!("distro '{distro}' is not ubuntu; this generator writes Ubuntu's file only"));
    }
    let path = at("/intent/path");
    if path != "keep-windows" && path != "clean-slate" {
        return Err(format!("intent.path '{path}' is not a path"));
    }
    let desktop = at("/intent/desktop");
    if desktop != "gnome" {
        return Err(format!("intent.desktop '{desktop}' is not gnome; the Ubuntu this kit measured has the GNOME desktop only"));
    }
    let start_at = at("/intent/start_at");
    if start_at != "desktop" && start_at != "console" {
        return Err(format!("intent.start_at '{start_at}' is not desktop or console - what the computer starts at is the person's choice, never a default"));
    }
    if !at("/intent/account/password_hash").starts_with("$6$") {
        return Err("account.password_hash is not SHA-512 crypt; refusing".to_string());
    }
    let name = at("/intent/account/linux_name");
    if !login_name(&name) {
        return Err(format!("linux_name '{name}' is not a login name"));
    }
    if at("/intent/locale/lang").is_empty() || at("/intent/locale/timezone").is_empty() || at("/intent/locale/keymap").is_empty() {
        return Err("locale is incomplete".to_string());
    }
    Ok(())
}

fn disk_ok(d: &Disk) -> Result<(), String> {
    let p = d.path.strip_prefix("/dev/").unwrap_or("");
    if p.is_empty() || !p.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(format!("'{}' is not a disk device; refusing to name it", d.path));
    }
    if d.serial == NO_DISK {
        return Err("the disk's serial is the placeholder; refusing".to_string());
    }
    Ok(())
}

/// The stick's own answer file. `program` and `job` are paths as the live
/// system sees them. The program is copied off the stick first: a FAT
/// stick may be mounted without the right to run files.
pub fn stick_file(program: &str, job: &str) -> String {
    let mut l: Vec<String> = Vec::new();
    l.push(format!("# upgrade_ autoinstall, the stick's own file - upgrade-autoinstall {GENERATOR_VERSION}"));
    l.push("# By itself this file installs nothing: it names no account, and a disk".into());
    l.push("# that does not exist. It starts our check. Only when every check has".into());
    l.push("# passed does the check write the real file, for this machine's disk".into());
    l.push("# and no other (RISKS R45).".into());
    l.push("autoinstall:".into());
    l.push("  version: 1".into());
    l.push("  early-commands:".into());
    l.push(format!("    - {}", q(&format!("cp {program} /tmp/upgrade-installer && chmod 755 /tmp/upgrade-installer && /tmp/upgrade-installer early {job}"))));
    l.push("  storage:".into());
    l.push("    layout:".into());
    l.push("      name: direct".into());
    l.push(format!("      match: {{serial: {NO_DISK}}}"));
    l.join("\n") + "\n"
}

fn erase_disk(l: &mut Vec<String>, id: &str, d: &Disk) {
    l.push(format!("      - id: {id}"));
    l.push("        type: disk".into());
    l.push(format!("        path: {}", q(&d.path)));
    if !d.serial.is_empty() {
        l.push(format!("        serial: {}", q(&d.serial)));
    }
    l.push("        ptable: gpt".into());
    l.push("        wipe: superblock-recursive".into());
    l.push("        preserve: false".into());
    l.push("        grub_device: false".into());
}

fn part(l: &mut Vec<String>, id: &str, disk: &str, number: u32, size: &str, esp: bool) {
    l.push(format!("      - id: {id}"));
    l.push("        type: partition".into());
    l.push(format!("        device: {disk}"));
    l.push(format!("        number: {number}"));
    l.push(format!("        size: {size}"));
    if esp {
        l.push("        flag: boot".into());
    }
    l.push("        wipe: superblock".into());
    l.push("        preserve: false".into());
    l.push(format!("        grub_device: {esp}"));
}

fn format_mount(l: &mut Vec<String>, id: &str, fstype: &str, path: &str) {
    l.push(format!("      - id: {id}-fs"));
    l.push("        type: format".into());
    l.push(format!("        volume: {id}"));
    l.push(format!("        fstype: {fstype}"));
    l.push("        preserve: false".into());
    l.push(format!("      - id: {id}-mount"));
    l.push("        type: mount".into());
    l.push(format!("        device: {id}-fs"));
    l.push(format!("        path: {path}"));
}

/// The storage section: every disk and partition named, never "largest".
/// The sizes are Fedora's (`upgrade-installer verify`): 600 MiB for the
/// boot partition, 1 GiB for `/boot`, the rest for `/`.
fn storage(s: &Storage) -> Result<Vec<String>, String> {
    let mut l = vec!["  storage:".to_string(), "    config:".to_string()];
    match s {
        Storage::Erase { system, home } => {
            disk_ok(system)?;
            if let Some(h) = home {
                disk_ok(h)?;
                if h.path == system.path {
                    return Err("the home drive is the system drive; refusing".to_string());
                }
            }
            erase_disk(&mut l, "disk-system", system);
            part(&mut l, "esp", "disk-system", 1, "600M", true);
            part(&mut l, "boot", "disk-system", 2, "1G", false);
            part(&mut l, "root", "disk-system", 3, "-1", false);
            if let Some(h) = home {
                erase_disk(&mut l, "disk-home", h);
                part(&mut l, "home", "disk-home", 1, "-1", false);
            }
            format_mount(&mut l, "root", "ext4", "/");
            format_mount(&mut l, "boot", "ext4", "/boot");
            format_mount(&mut l, "esp", "fat32", "/boot/efi");
            if home.is_some() {
                format_mount(&mut l, "home", "ext4", "/home");
            }
        }
        Storage::Beside { disk, kept, esp, boot, root } => {
            disk_ok(disk)?;
            beside_ok(kept, *esp, boot, root)?;
            // version 2: a partition is where its offset says, not wherever the last one ended
            l.insert(1, "    version: 2".to_string());
            l.push("      - id: disk-system".into());
            l.push("        type: disk".into());
            l.push(format!("        path: {}", q(&disk.path)));
            if !disk.serial.is_empty() {
                l.push(format!("        serial: {}", q(&disk.serial)));
            }
            l.push("        ptable: gpt".into());
            l.push("        preserve: true".into());
            l.push("        grub_device: false".into());
            for k in kept {
                l.push(format!("      - id: kept-{}", k.number));
                l.push("        type: partition".into());
                l.push("        device: disk-system".into());
                l.push(format!("        number: {}", k.number));
                l.push(format!("        offset: {}", k.offset));
                l.push(format!("        size: {}", k.size));
                l.push(format!("        partition_type: {}", k.type_guid));
                if k.number == *esp {
                    l.push("        flag: boot".into());
                }
                l.push("        preserve: true".into());
                l.push(format!("        grub_device: {}", k.number == *esp));
            }
            for (id, n) in [("boot", boot), ("root", root)] {
                l.push(format!("      - id: {id}"));
                l.push("        type: partition".into());
                l.push("        device: disk-system".into());
                l.push(format!("        number: {}", n.number));
                l.push(format!("        offset: {}", n.offset));
                l.push(format!("        size: {}", n.size));
                l.push("        wipe: superblock".into());
                l.push("        preserve: false".into());
                l.push("        grub_device: false".into());
            }
            format_mount(&mut l, "root", "ext4", "/");
            format_mount(&mut l, "boot", "ext4", "/boot");
            // Windows' boot partition: mounted as it is, never formatted
            l.push(format!("      - id: kept-{esp}-fs"));
            l.push("        type: format".into());
            l.push(format!("        volume: kept-{esp}"));
            l.push("        fstype: fat32".into());
            l.push("        preserve: true".into());
            l.push(format!("      - id: kept-{esp}-mount"));
            l.push("        type: mount".into());
            l.push(format!("        device: kept-{esp}-fs"));
            l.push("        path: /boot/efi".into());
        }
    }
    Ok(l)
}

/// The list beside Windows is refused unless it is exact: the boot
/// partition among the kept ones, no number used twice, and nothing new
/// touching anything kept.
fn beside_ok(kept: &[Kept], esp: u32, boot: &New, root: &New) -> Result<(), String> {
    let Some(e) = kept.iter().find(|k| k.number == esp) else {
        return Err(format!("partition {esp}, Windows' boot partition, is not among the kept ones; refusing"));
    };
    if e.type_guid != ESP_TYPE {
        return Err(format!("partition {esp} is not an EFI boot partition; refusing"));
    }
    let mut spans: Vec<(u32, u64, u64)> = kept.iter().map(|k| (k.number, k.offset, k.size)).collect();
    spans.push((boot.number, boot.offset, boot.size));
    spans.push((root.number, root.offset, root.size));
    for (i, a) in spans.iter().enumerate() {
        if a.0 == 0 || a.0 > 128 || a.2 == 0 {
            return Err(format!("partition {} has no number or no size; refusing", a.0));
        }
        for b in &spans[i + 1..] {
            if a.0 == b.0 {
                return Err(format!("partition number {} is used twice; refusing", a.0));
            }
            if a.1 < b.1 + b.2 && b.1 < a.1 + a.2 {
                return Err(format!("partitions {} and {} overlap; refusing", a.0, b.0));
            }
        }
    }
    if kept.iter().any(|k| !k.type_guid.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') || k.type_guid.len() != 36) {
        return Err("a kept partition's type is not a GPT type; refusing".to_string());
    }
    Ok(())
}

/// The real answer file, for one machine. `program` is the installer
/// program as the early check left it (`/tmp/upgrade-installer`); `job` is
/// the job on the stick.
pub fn autoinstall(job: &Value, s: &Storage, program: &str, job_path: &str) -> Result<String, String> {
    check(job)?;
    let at = |p: &str| text(job.pointer(p));
    let erase = !at("/erase_consent/statement").is_empty();
    // the storage and the job must say the same thing, or nothing is written
    match s {
        Storage::Erase { .. } if !erase => return Err("an erase without the job's erase consent; refusing".to_string()),
        Storage::Erase { .. } if at("/intent/path") != "clean-slate" => return Err("an erase for a job that keeps Windows; refusing".to_string()),
        Storage::Beside { .. } if erase || at("/intent/path") != "keep-windows" => return Err("an install beside Windows for a job that does not keep Windows; refusing".to_string()),
        _ => {}
    }
    let mut l: Vec<String> = Vec::new();
    l.push(format!("# upgrade_ autoinstall - generated by upgrade-autoinstall {GENERATOR_VERSION} (Rust)"));
    l.push(format!("# job: {}   path: {}   machine: {} {}", at("/job_id"), at("/intent/path"), at("/identity/vendor"), at("/identity/model")).replace('\n', " "));
    l.push("# Written on this machine by upgrade-installer early, after it matched".into());
    l.push("# this machine's disk against the identity in the job. Do not edit.".into());
    l.push("autoinstall:".into());
    l.push("  version: 1".into());
    l.push(format!("  locale: {}", q(&at("/intent/locale/lang"))));
    l.push(format!("  keyboard: {{layout: {}}}", q(&at("/intent/locale/keymap"))));
    l.push(format!("  timezone: {}", q(&at("/intent/locale/timezone"))));
    l.push("  identity:".into());
    let full = at("/intent/account/full_name");
    let name = at("/intent/account/linux_name");
    l.push(format!("    realname: {}", q(if full.is_empty() { &name } else { &full })));
    l.push(format!("    username: {}", q(&name)));
    l.push(format!("    password: {}", q(&at("/intent/account/password_hash"))));
    l.push("    hostname: ubuntu".into());
    l.push("  source: {id: ubuntu-desktop-minimal}".into());
    l.push("  # offline: everything comes from the stick (architecture.md, \"Design constraints: offline\")".into());
    l.push("  apt: {fallback: offline-install, geoip: false}".into());
    l.push("  codecs: {install: false}".into());
    l.push("  drivers: {install: false}".into());
    l.push("  refresh-installer: {update: false}".into());
    l.extend(storage(s)?);
    l.push("  late-commands:".into());
    l.push(format!("    - {}", q(&format!("{program} late {job_path}"))));
    l.push("  error-commands:".into());
    l.push(format!("    - {}", q(&format!("{program} failed {job_path}"))));
    l.push("  shutdown: reboot".into());
    Ok(l.join("\n") + "\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn job() -> Value {
        json!({
            "schema": "job/1", "job_id": "j-1", "identity": {"vendor": "Acme", "model": "One"},
            "fork": {}, "storage": {}, "harvest": {}, "stick": {},
            "erase_consent": {"statement": "I confirm that everything on this computer will be deleted and nothing will be kept"},
            "intent": {"path": "clean-slate", "desktop": "gnome", "start_at": "desktop", "distro": {"name": "ubuntu", "release": "26.04.1"},
                "account": {"linux_name": "sam", "full_name": "Sam \"S\" O'Neil: yes", "password_hash": "$6$salt$hash"},
                "locale": {"lang": "en_US.UTF-8", "timezone": "Europe/Paris", "keymap": "us"}}
        })
    }
    fn sda() -> Disk {
        Disk { path: "/dev/sda".into(), serial: "360022480d990bd51d172abb88dc5d7c3".into() }
    }
    fn erase() -> Storage {
        Storage::Erase { system: sda(), home: None }
    }
    const MIB: u64 = 1048576;
    fn keep_job() -> Value {
        let mut j = job();
        j.as_object_mut().unwrap().remove("erase_consent");
        j["intent"]["path"] = json!("keep-windows");
        j
    }
    fn kept() -> Vec<Kept> {
        vec![
            Kept { number: 1, offset: MIB, size: 100 * MIB, type_guid: ESP_TYPE.into() },
            Kept { number: 2, offset: 101 * MIB, size: 16 * MIB, type_guid: "e3c9e316-0b5c-4db8-817d-f92df00215ae".into() },
            Kept { number: 3, offset: 117 * MIB, size: 49034 * MIB, type_guid: "ebd0a0a2-b9e5-4433-87c0-68b6b72699c7".into() },
        ]
    }
    fn beside() -> Storage {
        Storage::Beside { disk: sda(), kept: kept(), esp: 1, boot: New { number: 4, offset: 49151 * MIB, size: 1024 * MIB }, root: New { number: 5, offset: 50175 * MIB, size: 31744 * MIB } }
    }

    #[test]
    fn beside_windows_every_partition_is_named_and_nothing_kept_is_formatted() {
        let t = autoinstall(&keep_job(), &beside(), "p", "j").unwrap();
        assert!(t.contains("    version: 2\n    config:"));
        assert_eq!(t.matches("type: disk").count(), 1);
        assert_eq!(t.matches("type: partition").count(), 5);
        // the disk and Windows' three partitions are kept; only the two new ones are not
        assert_eq!(t.matches("preserve: true").count(), 5, "the disk, three partitions, the boot partition's filesystem");
        assert_eq!(t.matches("preserve: false").count(), 4, "two new partitions and their two filesystems");
        assert_eq!(t.matches("wipe:").count(), 2);
        assert!(!t.contains("superblock-recursive") && !t.contains("    layout:") && !t.contains("use_gap"));
        assert!(t.contains(&format!("offset: {}\n        size: {}\n        partition_type: ebd0a0a2", 117 * MIB, 49034 * MIB)));
        assert!(t.contains("      - id: kept-1-fs\n        type: format\n        volume: kept-1\n        fstype: fat32\n        preserve: true"));
        assert!(t.contains("device: kept-1-fs\n        path: /boot/efi"));
        assert_eq!(t.matches("grub_device: true").count(), 1);
    }

    #[test]
    fn beside_windows_is_refused_unless_exact() {
        let with = |f: &dyn Fn(&mut Vec<Kept>, &mut u32, &mut New, &mut New)| {
            let Storage::Beside { disk, mut kept, mut esp, mut boot, mut root } = beside() else { unreachable!() };
            f(&mut kept, &mut esp, &mut boot, &mut root);
            autoinstall(&keep_job(), &Storage::Beside { disk, kept, esp, boot, root }, "p", "j").unwrap_err()
        };
        assert!(with(&|_, _, b, _| b.offset = 49150 * MIB).contains("overlap"));
        assert!(with(&|_, _, _, r| r.offset = 50174 * MIB).contains("overlap"));
        assert!(with(&|_, _, b, _| b.number = 3).contains("used twice"));
        assert!(with(&|_, e, _, _| *e = 3).contains("not an EFI boot partition"));
        assert!(with(&|_, e, _, _| *e = 9).contains("not among the kept"));
        assert!(with(&|_, _, _, r| r.size = 0).contains("no size"));
        assert!(with(&|k, _, _, _| k[1].type_guid = "msftres".into()).contains("not a GPT type"));
        // the storage and the job must agree
        assert!(autoinstall(&job(), &beside(), "p", "j").unwrap_err().contains("does not keep Windows"));
        assert!(autoinstall(&keep_job(), &erase(), "p", "j").unwrap_err().contains("without the job's erase consent"));
        let mut both = job();
        both["intent"]["path"] = json!("keep-windows");
        assert!(autoinstall(&both, &erase(), "p", "j").unwrap_err().contains("keeps Windows"));
    }

    #[test]
    fn the_sticks_own_file_names_no_account_and_no_real_disk() {
        let t = stick_file("/cdrom/upgrade_/upgrade-installer", "/cdrom/upgrade_/job.json");
        assert!(t.contains("match: {serial: UPGRADE-NO-DISK-CHOSEN}"));
        assert!(t.contains("/tmp/upgrade-installer early /cdrom/upgrade_/job.json"));
        for never in ["identity", "password", "username", "/dev/", "late-commands", "use_gap", "largest"] {
            assert!(!t.contains(never), "the stick's file must not hold '{never}'");
        }
    }

    #[test]
    fn the_real_file_names_the_disk_by_path_and_serial_and_never_a_default() {
        let t = autoinstall(&job(), &erase(), "/tmp/upgrade-installer", "/cdrom/upgrade_/job.json").unwrap();
        assert!(t.contains("path: \"/dev/sda\""));
        assert!(t.contains("serial: \"360022480d990bd51d172abb88dc5d7c3\""));
        assert!(t.contains("fallback: offline-install"));
        assert!(t.contains("username: \"sam\""));
        assert!(t.contains(r#"realname: "Sam \"S\" O'Neil: yes""#));
        for never in ["    layout:", "largest", "use_gap", "match:", NO_DISK, "interactive-sections"] {
            assert!(!t.contains(never), "the real file must not hold '{never}'");
        }
        // one disk action, three partitions, three mounts
        assert_eq!(t.matches("type: disk").count(), 1);
        assert_eq!(t.matches("type: partition").count(), 3);
        assert_eq!(t.matches("type: mount").count(), 3);
        assert!(t.contains("path: /boot/efi") && t.contains("path: /boot\n") && t.contains("path: /\n"));
    }

    #[test]
    fn a_home_drive_is_a_second_named_disk() {
        let s = Storage::Erase { system: sda(), home: Some(Disk { path: "/dev/sdb".into(), serial: String::new() }) };
        let t = autoinstall(&job(), &s, "p", "j").unwrap();
        assert_eq!(t.matches("type: disk").count(), 2);
        assert!(t.contains("path: \"/dev/sdb\"") && t.contains("path: /home"));
        // a disk with no serial is named by path alone, never by a made-up one
        assert_eq!(t.matches("serial:").count(), 1);
        let same = Storage::Erase { system: sda(), home: Some(sda()) };
        assert!(autoinstall(&job(), &same, "p", "j").unwrap_err().contains("home drive is the system drive"));
    }

    #[test]
    fn refusals() {
        let edit = |ptr: &str, v: Value| {
            let mut j = job();
            *j.pointer_mut(ptr).unwrap() = v;
            autoinstall(&j, &erase(), "p", "j").unwrap_err()
        };
        assert!(edit("/schema", json!("job/2")).contains("not job/1"));
        assert!(edit("/intent/distro/name", json!("fedora")).contains("not ubuntu"));
        assert!(edit("/intent/desktop", json!("kde")).contains("GNOME desktop only"));
        assert!(edit("/intent/start_at", json!("")).contains("never a default"));
        assert!(edit("/intent/account/password_hash", json!("plain")).contains("SHA-512"));
        assert!(edit("/intent/account/linux_name", json!("Sam Smith")).contains("not a login name"));
        assert!(edit("/intent/locale/timezone", json!("")).contains("locale is incomplete"));
        assert!(edit("/erase_consent/statement", json!("")).contains("without the job's erase consent"));
        let mut j = job();
        j.as_object_mut().unwrap().remove("stick");
        assert!(autoinstall(&j, &erase(), "p", "j").unwrap_err().contains("lacks 'stick'"));
        for bad in ["sda", "/dev/", "/dev/sda; rm", "/dev/disk/by-id/x", ""] {
            let s = Storage::Erase { system: Disk { path: bad.into(), serial: String::new() }, home: None };
            assert!(autoinstall(&job(), &s, "p", "j").unwrap_err().contains("not a disk device"), "{bad}");
        }
        let s = Storage::Erase { system: Disk { path: "/dev/sda".into(), serial: NO_DISK.into() }, home: None };
        assert!(autoinstall(&job(), &s, "p", "j").unwrap_err().contains("placeholder"));
    }

    #[test]
    fn text_from_the_job_cannot_break_out_of_its_value() {
        let mut j = job();
        j["intent"]["locale"]["timezone"] = json!("UTC\n  storage:\n    layout: {name: direct}");
        j["identity"]["model"] = json!("x\nautoinstall: {}");
        let t = autoinstall(&j, &erase(), "p", "j").unwrap();
        // the job's text stays inside its quotes: no line of the file starts a key it brought
        assert_eq!(t.lines().filter(|l| l.trim_start().starts_with("storage:")).count(), 1);
        assert_eq!(t.lines().filter(|l| l.trim_start().starts_with("layout:")).count(), 0);
        assert_eq!(t.matches("\nautoinstall:").count(), 1);
        assert!(t.contains(r#"timezone: "UTC\n  storage:"#));
    }
}
