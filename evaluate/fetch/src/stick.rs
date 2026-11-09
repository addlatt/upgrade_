//! Which stick this is, and what it already holds (roadmap item 24).
//!
//! `upgrade-fetch` writes only into the drive it runs from: the stick the
//! kit was put on. There is no argument that names another drive. Before it
//! writes it checks that this drive looks like the kit's stick and is not
//! in the middle of a conversion.

use crate::plan::{release_owned, Plan};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// What Windows says about the drive's root (GetDriveTypeW).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DriveKind {
    Removable,
    Fixed,
    Remote,
    CdRom,
    RamDisk,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct StickFacts {
    /// the drive the program runs from, as `E:`; empty when it runs from a
    /// network path or a folder that is not a drive's root
    pub drive: String,
    pub kind: DriveKind,
    pub label: String,
    pub file_system: String,
    /// `%SystemDrive%`, as `C:`
    pub system_drive: String,
}

pub const LABEL: &str = "UPGV0";

/// Why this program will not write here, or None.
pub fn identity_refusal(f: &StickFacts, root: &Path) -> Option<String> {
    if f.drive.len() != 2 || !f.drive.ends_with(':') {
        return Some("upgrade-fetch must run from the root of its USB stick (it runs from somewhere else)".into());
    }
    match f.kind {
        DriveKind::Removable | DriveKind::Fixed => {}
        k => return Some(format!("{} is not a USB stick ({k:?}); upgrade-fetch writes only to the stick it runs from", f.drive)),
    }
    if f.drive.eq_ignore_ascii_case(&f.system_drive) {
        return Some(format!("{} is the drive Windows runs from, not the kit's stick", f.drive));
    }
    if f.label != LABEL {
        return Some(format!("the drive {} is labelled '{}', not {LABEL}: it is not a stick the kit writer made", f.drive, f.label));
    }
    for need in ["SHA256SUMS", "KIT-MANIFEST.txt", "UPGRADE.exe"] {
        if !root.join(need).is_file() {
            return Some(format!("{need} is missing from {}: this is not a whole kit stick", f.drive));
        }
    }
    let u = root.join("upgrade_");
    if u.join("converted").exists() {
        return Some("this stick has already converted a computer; it keeps the Linux it installed (RISKS R35)".into());
    }
    for (m, why) in [("boot-install", "is set to install"), ("boot-reclaim", "is set to free Windows' space"), ("boot-verify", "is set for a test start")] {
        if u.join(m).exists() {
            return Some(format!("this stick {why} at the next start ({m}); its Linux is not changed now"));
        }
    }
    None
}

/// `upgrade_\LiveOS\kde.squashfs` under `root`, on this OS.
pub fn under(root: &Path, rel: &str) -> PathBuf {
    let mut p = root.to_path_buf();
    for part in rel.trim_start_matches(".\\").split('\\').filter(|x| !x.is_empty()) {
        p.push(part);
    }
    p
}

/// SHA256SUMS as (path with backslashes, sha256) in file order. `./` and `/` are read as make-kit.sh writes them.
pub fn read_sums(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = vec![];
    for l in text.lines() {
        let l = l.trim_end_matches('\r');
        if l.len() < 67 || !l[..64].bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        let path = l[64..].trim_start_matches([' ', '*']).trim_start_matches("./").replace('/', "\\");
        if path.is_empty() {
            continue;
        }
        let h = l[..64].to_ascii_lowercase();
        match out.iter_mut().find(|(p, _)| p.eq_ignore_ascii_case(&path)) {
            Some(e) => e.1 = h,
            None => out.push((path, h)),
        }
    }
    out
}

/// SHA256SUMS with every line the release owns replaced by `new`, sorted by
/// path, in sha256sum's own form (`<sha256>  ./path`).
pub fn merge_sums(old: &str, new: &[(String, String)]) -> String {
    let mut all: Vec<(String, String)> = read_sums(old).into_iter().filter(|(p, _)| !release_owned(p)).collect();
    all.extend(new.iter().cloned());
    all.sort_by(|a, b| a.0.replace('\\', "/").cmp(&b.0.replace('\\', "/")));
    all.iter().map(|(p, h)| format!("{h}  ./{}\n", p.replace('\\', "/"))).collect()
}

/// The release files the stick holds now (every release-owned path SHA256SUMS names, and any left on disk).
pub fn owned_on_stick(root: &Path, sums: &str) -> Vec<String> {
    let mut v: Vec<String> = read_sums(sums).into_iter().map(|(p, _)| p).filter(|p| release_owned(p)).collect();
    for dir in ["upgrade_\\LiveOS", "upgrade_\\boot-chain"] {
        if let Ok(rd) = std::fs::read_dir(under(root, dir)) {
            for e in rd.flatten() {
                let rel = format!("{dir}\\{}", e.file_name().to_string_lossy());
                if release_owned(&rel) && !v.iter().any(|x| x.eq_ignore_ascii_case(&rel)) {
                    v.push(rel);
                }
            }
        }
    }
    for f in ["EFI\\BOOT\\BOOTX64.EFI", "EFI\\BOOT\\grubx64.efi", "images\\pxeboot\\vmlinuz", "images\\pxeboot\\initrd.img", "images\\install.img", "release.json"] {
        if under(root, f).exists() && !v.iter().any(|x| x.eq_ignore_ascii_case(f)) {
            v.push(f.into());
        }
    }
    v
}

/// Does the stick already hold this release and desktop, whole? It reads
/// release.json and SHA256SUMS and that every file is there; it does not
/// read the 4 GB back (the stick's own %pre does, RISKS R17).
pub fn holds(root: &Path, plan: &Plan) -> Result<(), String> {
    let rel: Value = std::fs::read(root.join("release.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).ok_or("this stick carries no Linux yet")?;
    let (id, desktop) = (rel["id"].as_str().unwrap_or(""), rel["desktop"].as_str().unwrap_or(""));
    if id != plan.id || desktop != plan.desktop {
        return Err(format!("this stick carries {} ({}), not {} ({})", rel["name"].as_str().unwrap_or(id), if desktop.is_empty() { "a kit-built stick" } else { desktop }, plan.name, plan.desktop_label));
    }
    let sums = read_sums(&std::fs::read_to_string(root.join("SHA256SUMS")).unwrap_or_default());
    let sum_of = |p: &str| sums.iter().find(|(q, _)| q.eq_ignore_ascii_case(p)).map(|(_, h)| h.clone());
    for x in plan.parts.iter().flat_map(|p| &p.extracts) {
        let Some(h) = sum_of(&x.dest) else { return Err(format!("{} is not in the stick's SHA256SUMS", x.dest)) };
        if let Some(want) = &x.sha256 {
            if &h != want {
                return Err(format!("{} in SHA256SUMS is not the measured file", x.dest));
            }
        }
        match std::fs::metadata(under(root, &x.dest)) {
            Ok(m) if m.len() > 0 => {}
            _ => return Err(format!("{} is missing from the stick", x.dest)),
        }
    }
    for (name, _) in &plan.boot_chain {
        if !under(root, name).is_file() {
            return Err(format!("{name} is missing from the stick"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> StickFacts {
        StickFacts { drive: "E:".into(), kind: DriveKind::Removable, label: "UPGV0".into(), file_system: "FAT32".into(), system_drive: "C:".into() }
    }

    fn kit() -> PathBuf {
        let d = std::env::temp_dir().join(format!("upg-fetch-stick-{}-{}", std::process::id(), rand_tag()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("upgrade_")).unwrap();
        for f in ["SHA256SUMS", "KIT-MANIFEST.txt", "UPGRADE.exe"] {
            std::fs::write(d.join(f), "x").unwrap();
        }
        d
    }

    fn rand_tag() -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        N.fetch_add(1, Ordering::SeqCst)
    }

    #[test]
    fn only_its_own_kit_stick_is_written() {
        let d = kit();
        assert_eq!(identity_refusal(&facts(), &d), None);
        assert_eq!(identity_refusal(&StickFacts { kind: DriveKind::Fixed, ..facts() }, &d), None, "a USB drive Windows calls fixed, or the rig's disk");
        let cases: Vec<(StickFacts, &str)> = vec![
            (StickFacts { drive: String::new(), ..facts() }, "root of its USB stick"),
            (StickFacts { kind: DriveKind::Remote, ..facts() }, "not a USB stick"),
            (StickFacts { kind: DriveKind::CdRom, ..facts() }, "not a USB stick"),
            (StickFacts { drive: "C:".into(), ..facts() }, "the drive Windows runs from"),
            (StickFacts { label: "BACKUP".into(), ..facts() }, "labelled 'BACKUP'"),
        ];
        for (f, want) in cases {
            let why = identity_refusal(&f, &d).unwrap_or_default();
            assert!(why.contains(want), "{f:?}: {why}");
        }
        std::fs::remove_file(d.join("KIT-MANIFEST.txt")).unwrap();
        assert!(identity_refusal(&facts(), &d).unwrap().contains("KIT-MANIFEST.txt is missing"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_stick_mid_conversion_or_converted_is_left_alone() {
        for (m, want) in [("converted", "already converted"), ("boot-install", "set to install"), ("boot-reclaim", "free Windows' space"), ("boot-verify", "test start")] {
            let d = kit();
            std::fs::write(d.join("upgrade_").join(m), "").unwrap();
            assert!(identity_refusal(&facts(), &d).unwrap_or_default().contains(want), "{m}");
            let _ = std::fs::remove_dir_all(&d);
        }
    }

    #[test]
    fn sums_keep_the_kit_lines_and_replace_the_release_lines() {
        let a = "a".repeat(64);
        let b = "b".repeat(64);
        let c = "c".repeat(64);
        let old = format!("{a}  ./UPGRADE.exe\n{b}  ./upgrade_/LiveOS/gnome.squashfs\n{b}  ./EFI/BOOT/BOOTX64.EFI\n{a}  ./EFI/BOOT/grub.cfg\n");
        let new = vec![("upgrade_\\LiveOS\\kde.squashfs".to_string(), c.clone()), ("EFI\\BOOT\\BOOTX64.EFI".to_string(), c.clone())];
        let m = merge_sums(&old, &new);
        assert_eq!(m, format!("{c}  ./EFI/BOOT/BOOTX64.EFI\n{a}  ./EFI/BOOT/grub.cfg\n{a}  ./UPGRADE.exe\n{c}  ./upgrade_/LiveOS/kde.squashfs\n"));
        assert_eq!(read_sums(&m).len(), 4);
        assert_eq!(read_sums(&m)[3], ("upgrade_\\LiveOS\\kde.squashfs".to_string(), c));
    }
}
