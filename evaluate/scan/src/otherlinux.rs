//! A Linux already on the Windows disk (RISKS R39). A computer converted
//! once and rolled back, or one that had Linux before, still holds the old
//! Linux: its partitions, and its folder on the boot partition. A second
//! Linux put beside it leaves two sets of boot files, and which system
//! starts is no longer known. Found on the Aspire, 2026-10-28.
//!
//! Two signs are read, both from the disk Windows starts from:
//! a partition whose type says Linux, and a folder on the boot partition
//! (the ESP) that holds a Linux boot loader. One sign is enough. The scanner
//! reports it and the job writer refuses a keep-Windows job beside it; the
//! clean slate erases it with everything else.
//!
//! Not read: a Linux on another drive that keeps its boot files on that
//! drive, and firmware entries left with nothing behind them.

use crate::facts::{DiskFacts, EspFacts, LinuxPartition};

/// The partition types Linux makes for itself (the UEFI specification's
/// GPT type list and systemd's Discoverable Partitions Specification).
const LINUX_TYPES: [(&str, &str); 14] = [
    ("0fc63daf-8483-4772-8e79-3d69d8477de4", "Linux filesystem"),
    ("e6d6d379-f507-44c2-a23c-238f2a3df928", "Linux LVM"),
    ("0657fd6d-a4ab-43c4-84e5-0933c84b4f4f", "Linux swap"),
    ("ca7d7ccb-63ed-4c53-861c-1742536059cc", "Linux encrypted (LUKS)"),
    ("a19d880f-05fc-4d3b-a006-743f0f84911e", "Linux RAID"),
    ("933ac7e1-2eb4-4f13-b844-0e14e2aef915", "Linux home"),
    ("bc13c2ff-59e6-4262-a352-b275fd6f7172", "Linux boot"),
    ("4f68bce3-e8cd-4db1-96e7-fbcaf984b709", "Linux root"),
    ("44479540-f297-41b2-9af7-d131d5f0458a", "Linux root"),
    ("b921b045-1df0-41c3-af44-4c6f280d3fae", "Linux root"),
    ("69dad710-2ce4-4e3c-b16c-21a1d49abed3", "Linux root"),
    ("3b8f8425-20e0-4f3b-907f-1a25a76f98e8", "Linux server data"),
    ("4d21b016-b534-45c2-a9fb-5c16e091fd2d", "Linux variable data"),
    ("8da63339-0007-60c0-c436-083ac8230908", "Linux reserved"),
];

/// The folders under `EFI` on the boot partition that Linux systems are
/// known to use. A folder not named here still counts when a Linux boot
/// loader is found in it (`boot_loader`).
const LINUX_FOLDERS: [&str; 30] = [
    "fedora", "ubuntu", "debian", "centos", "redhat", "rhel", "almalinux", "rocky", "opensuse", "sled", "sles", "arch", "manjaro", "endeavouros", "cachyos", "garuda", "pop", "neon",
    "elementary", "zorin", "kali", "gentoo", "void", "nixos", "bazzite", "grub", "systemd", "linux", "refind", "qubes",
];

/// The kind of Linux partition a GPT type names, or nothing.
pub fn partition_kind(gpt_type: &str) -> Option<&'static str> {
    let g = gpt_type.trim().trim_start_matches('{').trim_end_matches('}').to_ascii_lowercase();
    LINUX_TYPES.iter().find(|(t, _)| *t == g).map(|(_, k)| *k)
}

/// True for a file name that is a Linux boot loader: shim, GRUB, systemd-boot, rEFInd, ELILO.
pub fn boot_loader(file: &str) -> bool {
    let f = file.to_ascii_lowercase();
    f == "grub.cfg" || (f.ends_with(".efi") && ["shim", "grub", "systemd-boot", "refind", "elilo", "mm"].iter().any(|p| f.starts_with(p)))
}

/// True when a folder under `EFI` belongs to a Linux: by its name, or by a
/// boot loader inside it. Windows' own folder and the shared fallback folder
/// (`Boot`) never count: the product itself may put shim in Windows' slot.
pub fn boot_folder(name: &str, files: &[String]) -> bool {
    let n = name.to_ascii_lowercase();
    if n == "microsoft" || n == "boot" {
        return false;
    }
    LINUX_FOLDERS.contains(&n.as_str()) || files.iter().any(|f| boot_loader(f))
}

/// The folders under `<esp>\EFI` that belong to a Linux (R39): by name, or
/// by a boot loader inside. Read-only. An error when `EFI` cannot be listed.
pub fn boot_folders(esp_root: &str) -> Result<Vec<String>, String> {
    let efi = std::path::Path::new(esp_root).join("EFI");
    let mut out = Vec::new();
    for e in std::fs::read_dir(&efi).map_err(|e| format!("{}: {e}", efi.display()))? {
        let e = e.map_err(|e| e.to_string())?;
        if !e.path().is_dir() {
            continue;
        }
        let name = e.file_name().to_string_lossy().into_owned();
        let files: Vec<String> = std::fs::read_dir(e.path()).map(|d| d.flatten().map(|f| f.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
        if boot_folder(&name, &files) {
            out.push(name);
        }
    }
    out.sort();
    Ok(out)
}

/// What was found, in words a person can check against Disk Management:
/// one line per sign. Empty means no Linux was found.
pub fn found(partitions: &[LinuxPartition], folders: &[String]) -> Vec<String> {
    let mut out: Vec<String> = partitions
        .iter()
        .map(|p| format!("partition {} ({}, {:.1} GB)", p.number.map_or("?".to_string(), |n| n.to_string()), p.kind.as_deref().unwrap_or("Linux"), p.size.unwrap_or(0.0) / 1e9))
        .collect();
    out.extend(folders.iter().map(|f| format!("the boot folder EFI\\{f}")));
    out
}

/// The scanner's line (R39), added only when a Linux was found: a computer
/// with none reads exactly as it did before.
pub fn check(scan: &mut crate::Scan, disk: &DiskFacts, esp: Option<&EspFacts>) {
    let folders = esp.map(|e| e.linux_boot_folders.clone()).unwrap_or_default();
    let found = found(&disk.linux_partitions, &folders);
    if found.is_empty() {
        return;
    }
    scan.add("Storage", "Linux already here", crate::Status::Warn, format!("a Linux is already on the Windows disk: {}", found.join("; ")))
        .note("This computer already holds a Linux beside Windows (from an earlier conversion, or one installed by hand). Putting a second Linux beside it would leave two sets of boot files, and which system starts would no longer be known. Linux is not installed beside Windows on this computer as it is.")
        .remedy("The clean-slate path erases the old Linux along with everything else. To keep Windows, the old Linux has to be removed first; do not delete partitions unless you know they hold nothing you need.");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(n: i64, kind: &str, size: f64) -> LinuxPartition {
        LinuxPartition { number: Some(n), kind: Some(kind.to_string()), size: Some(size) }
    }

    #[test]
    fn linux_partition_types_are_named_whatever_the_case_or_braces() {
        assert_eq!(partition_kind("{0FC63DAF-8483-4772-8E79-3D69D8477DE4}"), Some("Linux filesystem"));
        assert_eq!(partition_kind("e6d6d379-f507-44c2-a23c-238f2a3df928"), Some("Linux LVM"));
        assert_eq!(partition_kind("{bc13c2ff-59e6-4262-a352-b275fd6f7172}"), Some("Linux boot"));
    }

    #[test]
    fn windows_own_partitions_are_not_linux() {
        // the ESP, Microsoft reserved, basic data (C:), Windows recovery
        for g in ["{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}", "{e3c9e316-0b5c-4db8-817d-f92df00215ae}", "{ebd0a0a2-b9e5-4433-87c0-68b6b72699c7}", "{de94bba4-06d1-4d40-a16a-bfd50179d6ac}", ""] {
            assert_eq!(partition_kind(g), None, "{g}");
        }
    }

    #[test]
    fn a_boot_folder_counts_by_name_or_by_what_is_in_it() {
        assert!(boot_folder("fedora", &[]));
        assert!(boot_folder("Ubuntu", &[]));
        assert!(boot_folder("MyDistro", &["shimx64.efi".into(), "BOOTX64.CSV".into()]));
        assert!(boot_folder("something", &["grubx64.efi".into()]));
        assert!(boot_folder("loader", &["systemd-bootx64.efi".into()]));
    }

    #[test]
    fn windows_the_fallback_and_vendor_folders_are_left_alone() {
        // shim in Windows' slot is the product's own doing (the slot, R21): never a second Linux
        assert!(!boot_folder("Microsoft", &["bootmgfw.efi".into(), "shimx64.efi".into()]));
        assert!(!boot_folder("Boot", &["bootx64.efi".into(), "grubx64.efi".into()]));
        assert!(!boot_folder("Dell", &["BiosUpdate.efi".into()]));
        assert!(!boot_folder("HP", &[]));
        assert!(!boot_folder("OEM", &["memtest.efi".into()]));
    }

    #[test]
    fn the_boot_partition_is_read_folder_by_folder() {
        let root = std::env::temp_dir().join(format!("upgrade-scan-otherlinux-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (dir, file) in [("EFI/Microsoft/Boot", "bootmgfw.efi"), ("EFI/Boot", "bootx64.efi"), ("EFI/fedora", "shimx64.efi"), ("EFI/Mystery", "grubx64.efi"), ("EFI/Insyde", "isflash.efi"), ("upgrade_", "rolled-back.json")] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
            std::fs::write(root.join(dir).join(file), b"x").unwrap();
        }
        assert_eq!(boot_folders(&root.to_string_lossy()).unwrap(), ["Mystery", "fedora"]);
        // a boot partition that cannot be listed is an error, never "no Linux"
        assert!(boot_folders(&root.join("nowhere").to_string_lossy()).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_aspire_of_2026_10_28_is_found() {
        // its 2026-10-11 Fedora after the rollback: two partitions (1.07 and 25.77 GB) and EFI\fedora
        let f = found(&[part(5, "Linux boot", 1.07e9), part(6, "Linux filesystem", 25.77e9)], &["fedora".to_string()]);
        assert_eq!(f, ["partition 5 (Linux boot, 1.1 GB)", "partition 6 (Linux filesystem, 25.8 GB)", "the boot folder EFI\\fedora"]);
    }

    #[test]
    fn the_scanner_says_nothing_when_there_is_no_linux_and_warns_when_there_is() {
        let mut sc = crate::Scan::new();
        check(&mut sc, &DiskFacts::default(), Some(&EspFacts::default()));
        check(&mut sc, &DiskFacts::default(), None);
        assert!(sc.checks.is_empty());
        let esp = EspFacts { linux_boot_folders: vec!["fedora".into()], ..Default::default() };
        check(&mut sc, &DiskFacts::default(), Some(&esp));
        assert_eq!(sc.checks.len(), 1);
        assert_eq!(sc.checks[0].title, "Linux already here");
        assert_eq!(sc.checks[0].status, crate::Status::Warn);
        let mut sc = crate::Scan::new();
        let disk = DiskFacts { linux_partitions: vec![part(6, "Linux filesystem", 25.77e9)], ..Default::default() };
        check(&mut sc, &disk, None);
        assert!(sc.checks[0].detail.contains("partition 6 (Linux filesystem, 25.8 GB)"));
    }
}
