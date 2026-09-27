//! "Remove the old Windows startup entry" (decided 2026-09-26, the owner;
//! found on the Aspire's run 9: a stale "Windows Boot Manager" was left in
//! the firmware after the erase). A firmware entry is stale only when it
//! points at Windows' boot loader on a partition that no longer exists on
//! any disk. Offered on the erase and clean-slate paths only, and never
//! while any disk still has a Windows boot partition (keep-Windows keeps
//! its entry by design, R21/R22). This writes firmware variables, so it is
//! held to the writers' bar (rule #4): it refuses on anything it cannot
//! read, never removes the entry the machine started from, writes the new
//! boot order first and reads it back, and only then deletes the entry.

use crate::efi::{self, LoadOption};
use crate::gpt::{self, Partition};
use serde_json::{json, Value};

const WINDOWS_LOADER: &str = "\\efi\\microsoft\\boot\\bootmgfw.efi";

#[derive(Debug)]
pub struct Plan {
    pub stale: Vec<(u16, LoadOption)>,
    pub refusal: Option<String>,
}

pub struct Facts {
    pub clean_slate: bool,
    pub partitions: Result<Vec<Partition>, String>,
    pub windows_esp: Result<bool, String>,
    pub entries: Vec<(u16, LoadOption)>,
    pub boot_current: Option<u16>,
    pub boot_order: Option<Vec<u16>>,
}

fn is_windows_loader(lo: &LoadOption) -> bool {
    lo.path.as_deref().map(|p| p.to_ascii_lowercase().replace('/', "\\").ends_with(WINDOWS_LOADER)).unwrap_or(false)
}

/// Pure: which entries are stale, or why nothing may be removed.
pub fn plan(f: &Facts) -> Plan {
    let refuse = |why: &str| Plan { stale: Vec::new(), refusal: Some(why.to_string()) };
    if !f.clean_slate {
        return refuse("Windows was kept on this computer, so its startup entry stays");
    }
    let parts = match &f.partitions {
        Ok(p) => p,
        Err(e) => return refuse(&format!("the disks could not all be read ({})", e)),
    };
    match &f.windows_esp {
        Ok(false) => {}
        Ok(true) => return refuse("a disk in this computer still has a Windows startup partition"),
        Err(e) => return refuse(&format!("whether a disk still has Windows could not be checked ({})", e)),
    }
    let stale: Vec<(u16, LoadOption)> = f
        .entries
        .iter()
        .filter(|(_, lo)| is_windows_loader(lo) && lo.partition.map(|g| !parts.iter().any(|p| p.unique == g)).unwrap_or(false))
        .cloned()
        .collect();
    if stale.is_empty() {
        return Plan { stale, refusal: None };
    }
    let Some(cur) = f.boot_current else { return refuse("the entry this computer started from is not known") };
    if stale.iter().any(|(n, _)| *n == cur) {
        return refuse("this computer started from that entry");
    }
    let Some(order) = &f.boot_order else { return refuse("the startup order could not be read") };
    if !order.contains(&cur) {
        return refuse("the entry this computer started from is not in the startup order");
    }
    Plan { stale, refusal: None }
}

/// The partition device for a disk's partition number, from sysfs
/// (sda + 1 = sda1; nvme0n1 + 1 = nvme0n1p1 - read, not guessed).
fn part_name(root: &str, disk: &str, number: u32) -> Option<String> {
    let rd = std::fs::read_dir(format!("{}/sys/block/{}", root, disk)).ok()?;
    for e in rd.flatten() {
        let n = e.file_name().to_string_lossy().to_string();
        if n.starts_with(disk)
            && let Ok(t) = std::fs::read_to_string(e.path().join("partition"))
                && t.trim() == number.to_string() {
                    return Some(n);
                }
    }
    None
}

/// Case-insensitive walk to EFI/Microsoft/Boot/bootmgfw.efi under a mount point.
fn has_windows_loader(mount: &str) -> bool {
    let mut cur = std::path::PathBuf::from(mount);
    for part in ["efi", "microsoft", "boot", "bootmgfw.efi"] {
        let Ok(rd) = std::fs::read_dir(&cur) else { return false };
        match rd.flatten().find(|e| e.file_name().to_string_lossy().to_ascii_lowercase() == part) {
            Some(e) => cur = e.path(),
            None => return false,
        }
    }
    cur.is_file()
}

/// Is any EFI system partition on any disk a Windows one? A mounted ESP is
/// looked at where it is; an unmounted one is mounted read-only in /run for
/// the look and unmounted again.
pub fn windows_esp_present(root: &str, parts: &[Partition]) -> Result<bool, String> {
    let r = root.trim_end_matches('/');
    let mountinfo = std::fs::read_to_string(format!("{}/proc/self/mountinfo", r)).map_err(|e| format!("mountinfo: {}", e))?;
    for p in parts.iter().filter(|p| p.type_guid == gpt::ESP_TYPE) {
        let name = part_name(r, &p.disk, p.number).ok_or(format!("{} partition {} has no device", p.disk, p.number))?;
        let devno = std::fs::read_to_string(format!("{}/sys/block/{}/{}/dev", r, p.disk, name)).map_err(|e| format!("{}: {}", name, e))?;
        let devno = devno.trim();
        let mounted = mountinfo.lines().find_map(|l| {
            let f: Vec<&str> = l.split(' ').collect();
            if f.len() > 4 && f[2] == devno { Some(f[4].replace("\\040", " ")) } else { None }
        });
        let found = match mounted {
            Some(m) => has_windows_loader(&format!("{}{}", r, m)),
            None => probe_unmounted(&format!("{}/dev/{}", r, name))?,
        };
        if found {
            return Ok(true);
        }
    }
    Ok(false)
}

fn probe_unmounted(dev: &str) -> Result<bool, String> {
    let dir = "/run/upgrade_-esp-probe";
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {}", dir, e))?;
    let c = |s: &str| std::ffi::CString::new(s).unwrap();
    let (src, tgt, fs) = (c(dev), c(dir), c("vfat"));
    let flags = libc::MS_RDONLY | libc::MS_NOSUID | libc::MS_NODEV | libc::MS_NOEXEC;
    // SAFETY: NUL-terminated strings; a read-only mount of a FAT filesystem.
    let rc = unsafe { libc::mount(src.as_ptr(), tgt.as_ptr(), fs.as_ptr(), flags, std::ptr::null()) };
    if rc != 0 {
        return Err(format!("{} could not be opened to look for Windows ({})", dev, std::io::Error::last_os_error()));
    }
    let found = has_windows_loader(dir);
    // SAFETY: unmounting what was just mounted.
    unsafe { libc::umount2(tgt.as_ptr(), 0) };
    Ok(found)
}

pub fn facts(root: &str, job: &Value) -> Facts {
    let clean_slate = job.pointer("/intent/path").and_then(Value::as_str) == Some("clean-slate");
    let partitions = gpt::all_partitions(root);
    let windows_esp = match &partitions {
        Ok(p) => windows_esp_present(root, p),
        Err(e) => Err(e.clone()),
    };
    let cur = efi::read_var(root, "BootCurrent").filter(|d| d.len() >= 2).map(|d| u16::from_le_bytes([d[0], d[1]]));
    Facts { clean_slate, partitions, windows_esp, entries: efi::boot_entries(root), boot_current: cur, boot_order: efi::read_var(root, "BootOrder").map(|d| efi::u16_list(&d)) }
}

pub fn describe(p: &Plan) -> Value {
    json!({
        "offered": p.refusal.is_none() && !p.stale.is_empty(),
        "entries": p.stale.iter().map(|(n, lo)| json!({ "entry": format!("Boot{:04X}", n), "description": lo.description,
            "partition": lo.partition.map(|g| gpt::guid_text(&g)) })).collect::<Vec<_>>(),
        "why_not": p.refusal,
    })
}

/// The writer. Re-reads everything, then: new boot order first, read back;
/// only then the entries go. Returns what it did.
pub fn remove(root: &str, job: &Value) -> Value {
    let f = facts(root, job);
    let p = plan(&f);
    if let Some(why) = &p.refusal {
        return json!({ "result": "refused", "why": why });
    }
    if p.stale.is_empty() {
        return json!({ "result": "nothing-to-remove" });
    }
    let order = f.boot_order.clone().unwrap_or_default();
    let gone: Vec<u16> = p.stale.iter().map(|(n, _)| *n).collect();
    let new: Vec<u16> = order.iter().copied().filter(|n| !gone.contains(n)).collect();
    if new != order {
        let bytes: Vec<u8> = new.iter().flat_map(|n| n.to_le_bytes()).collect();
        if let Err(e) = efi::write_var(root, "BootOrder", &bytes) {
            return json!({ "result": "failed", "why": format!("the startup order could not be written; nothing was removed ({})", e) });
        }
        if efi::read_var(root, "BootOrder").map(|d| efi::u16_list(&d)) != Some(new.clone()) {
            return json!({ "result": "failed", "why": "the startup order did not read back as written; nothing was removed" });
        }
    }
    if efi::read_var(root, "BootNext").filter(|d| d.len() >= 2).map(|d| gone.contains(&u16::from_le_bytes([d[0], d[1]]))).unwrap_or(false) {
        let _ = efi::delete_var(root, "BootNext");
    }
    let mut removed = Vec::new();
    for (n, lo) in &p.stale {
        let name = format!("Boot{:04X}", n);
        match efi::delete_var(root, &name) {
            Ok(()) if efi::read_var(root, &name).is_none() => removed.push(json!({ "entry": name, "description": lo.description })),
            Ok(()) => return json!({ "result": "failed", "why": format!("{} is still there after it was deleted", name), "removed": removed }),
            Err(e) => return json!({ "result": "failed", "why": e, "removed": removed }),
        }
    }
    json!({ "result": "removed", "removed": removed, "boot_order": new.iter().map(|n| format!("Boot{:04X}", n)).collect::<Vec<_>>() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::efi::tests::load_option;
    use crate::gpt::{guid_bytes, tests::image, ESP_TYPE};

    const OLD: &str = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"; // Windows' ESP, erased
    const NEW: &str = "11111111-2222-3333-4444-555555555555"; // Fedora's ESP

    fn lo(desc: &str, g: &str, path: &str) -> LoadOption {
        efi::parse_load_option(&load_option(desc, guid_bytes(g), path)).unwrap()
    }
    fn facts_with(clean: bool, esp: Result<bool, String>) -> Facts {
        Facts {
            clean_slate: clean,
            partitions: Ok(vec![Partition { disk: "vda".into(), number: 1, type_guid: ESP_TYPE, unique: guid_bytes(NEW) }]),
            windows_esp: esp,
            entries: vec![(0, lo("Windows Boot Manager", OLD, "\\EFI\\Microsoft\\Boot\\bootmgfw.efi")), (3, lo("Fedora", NEW, "\\EFI\\fedora\\shimx64.efi"))],
            boot_current: Some(3),
            boot_order: Some(vec![3, 0]),
        }
    }

    #[test]
    fn a_windows_entry_on_a_vanished_partition_is_stale() {
        let p = plan(&facts_with(true, Ok(false)));
        assert_eq!(p.refusal, None);
        assert_eq!(p.stale.iter().map(|x| x.0).collect::<Vec<_>>(), vec![0]);
    }

    #[test]
    fn refusals() {
        assert!(plan(&facts_with(false, Ok(false))).refusal.unwrap().contains("Windows was kept"));
        assert!(plan(&facts_with(true, Ok(true))).refusal.unwrap().contains("still has a Windows"));
        assert!(plan(&facts_with(true, Err("x".into()))).refusal.is_some());
        let mut f = facts_with(true, Ok(false));
        f.partitions = Err("sdb unreadable".into());
        assert!(plan(&f).refusal.is_some());
        let mut f = facts_with(true, Ok(false));
        f.boot_current = Some(0);
        assert!(plan(&f).refusal.unwrap().contains("started from that entry"));
        let mut f = facts_with(true, Ok(false));
        f.boot_order = Some(vec![0]);
        assert!(plan(&f).refusal.is_some());
    }

    #[test]
    fn a_windows_entry_whose_partition_still_exists_is_not_stale() {
        let mut f = facts_with(true, Ok(false));
        f.entries[0] = (0, lo("Windows Boot Manager", NEW, "\\EFI\\Microsoft\\Boot\\bootmgfw.efi"));
        assert!(plan(&f).stale.is_empty());
    }

    /// A whole fake machine: efivars, one disk with Fedora's ESP mounted.
    fn machine(tag: &str, windows_on_esp: bool) -> String {
        let t = std::env::temp_dir().join(format!("settle-in-boot-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        let r = t.to_str().unwrap().to_string();
        let ev = format!("{}/sys/firmware/efi/efivars", r);
        std::fs::create_dir_all(&ev).unwrap();
        let put = |n: &str, d: &[u8]| {
            let mut b = 7u32.to_le_bytes().to_vec();
            b.extend_from_slice(d);
            std::fs::write(format!("{}/{}-{}", ev, n, efi::GLOBAL), b).unwrap();
        };
        put("Boot0000", &load_option("Windows Boot Manager", guid_bytes(OLD), "\\EFI\\Microsoft\\Boot\\bootmgfw.efi"));
        put("Boot0003", &load_option("Fedora", guid_bytes(NEW), "\\EFI\\fedora\\shimx64.efi"));
        put("BootCurrent", &3u16.to_le_bytes());
        put("BootOrder", &[3, 0, 0, 0]);
        put("BootNext", &0u16.to_le_bytes());
        std::fs::create_dir_all(format!("{}/sys/block/vda/vda1", r)).unwrap();
        std::fs::write(format!("{}/sys/block/vda/vda1/partition", r), "1\n").unwrap();
        std::fs::write(format!("{}/sys/block/vda/vda1/dev", r), "252:1\n").unwrap();
        std::fs::create_dir_all(format!("{}/dev", r)).unwrap();
        std::fs::write(format!("{}/dev/vda", r), image(&[(ESP_TYPE, guid_bytes(NEW))])).unwrap();
        std::fs::create_dir_all(format!("{}/proc/self", r)).unwrap();
        std::fs::write(format!("{}/proc/self/mountinfo", r), "40 1 252:1 / /boot/efi rw,relatime - vfat /dev/vda1 rw\n").unwrap();
        std::fs::create_dir_all(format!("{}/boot/efi/EFI/fedora", r)).unwrap();
        if windows_on_esp {
            std::fs::create_dir_all(format!("{}/boot/efi/EFI/Microsoft/Boot", r)).unwrap();
            std::fs::write(format!("{}/boot/efi/EFI/Microsoft/Boot/bootmgfw.efi", r), "MZ").unwrap();
        }
        r
    }

    #[test]
    fn removes_the_stale_entry_order_first() {
        let r = machine("rm", false);
        let job = json!({ "intent": { "path": "clean-slate" } });
        let d = describe(&plan(&facts(&r, &job)));
        assert_eq!(d["offered"], true);
        let out = remove(&r, &job);
        assert_eq!(out["result"], "removed", "{}", out);
        assert_eq!(efi::read_var(&r, "BootOrder").map(|d| efi::u16_list(&d)), Some(vec![3]));
        assert!(efi::read_var(&r, "Boot0000").is_none());
        assert!(efi::read_var(&r, "Boot0003").is_some());
        assert!(efi::read_var(&r, "BootNext").is_none());
        assert_eq!(remove(&r, &job)["result"], "nothing-to-remove");
        std::fs::remove_dir_all(&r).unwrap();
    }

    #[test]
    fn refuses_while_a_disk_has_windows() {
        let r = machine("win", true);
        let out = remove(&r, &json!({ "intent": { "path": "clean-slate" } }));
        assert_eq!(out["result"], "refused");
        assert!(efi::read_var(&r, "Boot0000").is_some());
        assert_eq!(remove(&r, &json!({ "intent": { "path": "keep-windows" } }))["result"], "refused");
        std::fs::remove_dir_all(&r).unwrap();
    }
}
