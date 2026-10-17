//! Rollback from the Linux side (decided 2026-10-12, the owner; architecture.md,
//! "Rollback is a mode of upgrade_"): on a computer converted with Windows kept,
//! Windows starts first again, the way it did before the conversion. Deletes
//! nothing: Linux stays on the disk and in the firmware's menu.
//!
//! Why here and not in Windows: on the Acer Aspire (InsydeH2O) a `bcdedit`
//! change to the firmware's boot order did not survive the restart, while the
//! same order written from Linux through efivarfs did (`r21-rollback.csv` line
//! 6, 2026-10-12). The person starts the rollback from Linux anyway.
//!
//! What it does, every refusal before the first write:
//!   1. Windows' own boot loader back in its slot (`bootmgfw-kept.efi` ->
//!      `bootmgfw.efi`) when shim sits there (architecture step 12);
//!   2. `EFI/Boot/bootx64.efi` restored from the snapshot the stick holds
//!      (`upgrade_/esp-snapshot`, taken by `%pre` before the install), the
//!      previous copy saved to the stick;
//!   3. a marker on the ESP (`upgrade_/rolled-back.json`) that the guard task
//!      in the kept Windows reads at its next start: it stands down and removes
//!      itself instead of putting shim back (RISKS R22);
//!   4. the firmware's `BootOrder`: Windows Boot Manager first, read back;
//!   5. the record, `upgrade_/rollback.json`, on the stick and in the handoff folder.
//!
//! The judging half is pure (tests below); the live half reads sysfs,
//! efivarfs, the ESP and the stick.

use crate::efi::{self, LoadOption};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const VERSION: &str = "0.1.0";
pub const SLOT_REL: &str = "EFI/Microsoft/Boot/bootmgfw.efi";
pub const KEPT_REL: &str = "EFI/Microsoft/Boot/bootmgfw-kept.efi";
pub const FALLBACK_REL: &str = "EFI/Boot/bootx64.efi";
/// On the ESP. The guard in the kept Windows (`upgrade-prologue guard`) reads
/// the same path, as `upgrade_\rolled-back.json`.
pub const MARKER_REL: &str = "upgrade_/rolled-back.json";
/// On the stick: the ESP as `%pre` saw it, with `SHA256SUMS`.
pub const SNAPSHOT_REL: &str = "upgrade_/esp-snapshot";
pub const RECORD_REL: &str = "upgrade_/rollback.json";
pub const BACKUP_REL: &str = "upgrade_/rollback/bootx64.efi.before";
pub const STICK_LABEL: &str = "UPGV0";

/// What the ESP holds now: sha256 of each file, None when absent; the
/// partition's own GUID as GPT stores it (to match the firmware's entry).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Esp {
    pub slot: Option<String>,
    pub kept: Option<String>,
    pub fallback: Option<String>,
    pub partition: Option<[u8; 16]>,
}

/// The disk the ESP lives on, as Linux sees it (for the identity check).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Disk {
    pub size_bytes: u64,
    pub serial: Option<String>,
}

/// `SHA256SUMS` lines -> lower-case forward-slash keys without `./` (FAT is
/// case-insensitive), the same reading as the Windows side's.
pub fn sums(text: &str) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    for l in text.lines() {
        let l = l.trim();
        if l.len() < 66 || !l.as_bytes()[..64].iter().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        let path = l[64..].trim_start_matches([' ', '*']).trim();
        let key = path.trim_start_matches("./").replace('\\', "/").to_lowercase();
        m.insert(key, l[..64].to_lowercase());
    }
    m
}

/// The firmware entries that start Windows Boot Manager, by its path.
pub fn windows_entries(entries: &[(u16, LoadOption)]) -> Vec<(u16, Option<[u8; 16]>)> {
    entries
        .iter()
        .filter(|(_, lo)| lo.path.as_deref().map(|p| p.replace('/', "\\").to_lowercase().ends_with("\\efi\\microsoft\\boot\\bootmgfw.efi")).unwrap_or(false))
        .map(|(n, lo)| (*n, lo.partition))
        .collect()
}

fn s<'a>(v: &'a Value, p: &str) -> &'a str {
    v.pointer(p).and_then(Value::as_str).unwrap_or("")
}

fn norm(x: &str) -> String {
    x.trim().to_lowercase()
}

/// Every refusal, then the plan. Nothing here touches the machine.
#[allow(clippy::too_many_arguments)]
pub fn plan(job: &Value, outcome: &Value, stick_job: &Value, sums: &BTreeMap<String, String>, esp: &Esp, disk: &Disk, entries: &[(u16, LoadOption)], order: Option<&[u16]>) -> Value {
    let mut r: Vec<String> = Vec::new();
    if s(job, "/schema") != "job/1" {
        r.push("the job is not readable, so whether Windows was kept is not known".into());
    } else if s(job, "/intent/path") != "keep-windows" {
        r.push(format!("the conversion's path was '{}', not keep-windows: there is no kept Windows to go back to", s(job, "/intent/path")));
    }
    if s(outcome, "/schema") != "outcome/1" {
        r.push("no outcome.json in the handoff folder: nothing says a conversion finished here".into());
    } else {
        if s(outcome, "/path_taken") != "keep-windows" {
            r.push(format!("the outcome says the path taken was '{}', not keep-windows", s(outcome, "/path_taken")));
        }
        if outcome.pointer("/windows/kept") != Some(&json!(true)) {
            r.push("the outcome says Windows was not kept".into());
        }
        if outcome.pointer("/cutover/esp_snapshot").is_none() {
            r.push("the outcome names no ESP snapshot".into());
        }
    }
    let (jid, sid) = (s(job, "/job_id"), s(stick_job, "/job_id"));
    if jid.is_empty() || sid.is_empty() || jid != sid {
        r.push(format!("the stick holds another conversion's records (job '{}', this computer's '{}'): it is not the stick this computer was converted with", sid, jid));
    }
    let snap_fallback = sums.get(&FALLBACK_REL.to_lowercase()).cloned();
    let snap_bootmgfw = sums.get(&SLOT_REL.to_lowercase()).cloned();
    if snap_fallback.is_none() {
        r.push(format!("the snapshot has no checksum for {FALLBACK_REL}: nothing to restore from"));
    }
    if snap_bootmgfw.is_none() {
        r.push(format!("the snapshot has no checksum for {SLOT_REL}: Windows' own boot loader is not in it"));
    }
    // identity: a moved stick must never touch a stranger's disk (the Windows side compares the same two facts)
    let want_size = job.pointer("/identity/system_disk/size_bytes").and_then(Value::as_u64).unwrap_or(0);
    if want_size == 0 || disk.size_bytes != want_size {
        r.push(format!("identity: the job names a system disk of {} bytes, this disk has {}", want_size, disk.size_bytes));
    }
    let want_serial = norm(s(job, "/identity/system_disk/serial_number"));
    if let Some(have) = &disk.serial
        && !want_serial.is_empty() && norm(have) != want_serial {
            r.push(format!("identity: the job names disk serial '{}', this disk says '{}'", want_serial, have.trim()));
        }
    if esp.slot.is_none() {
        r.push(format!("the ESP has no {SLOT_REL}: nothing to put first"));
    }
    if esp.kept.is_some() && esp.kept == esp.slot {
        r.push("the kept copy and the slot hold the same bytes: the arrangement is not the one the conversion made".into());
    }
    let wins = windows_entries(entries);
    let matching: Vec<u16> = match esp.partition {
        Some(p) => wins.iter().filter(|(_, part)| *part == Some(p)).map(|(n, _)| *n).collect(),
        None => wins.iter().map(|(n, _)| *n).collect(),
    };
    let win = match matching.as_slice() {
        [one] => Some(*one),
        [] => {
            r.push("the firmware has no entry for Windows Boot Manager on this ESP; this program does not make one".into());
            None
        }
        many => {
            r.push(format!("the firmware has {} entries for Windows Boot Manager on this ESP; which is Windows' own is not certain", many.len()));
            None
        }
    };
    let Some(order) = order else {
        r.push("the firmware's startup order (BootOrder) could not be read".into());
        return json!({ "refusals": r });
    };
    if !r.is_empty() {
        return json!({ "refusals": r });
    }
    let win = win.unwrap_or_default();
    let mut after = vec![win];
    after.extend(order.iter().copied().filter(|n| *n != win));
    json!({
        "refusals": [],
        "windows_entry": format!("Boot{:04X}", win),
        "windows_entry_num": win,
        "order_before": order.iter().map(|n| format!("Boot{:04X}", n)).collect::<Vec<_>>(),
        "order_after": after.iter().map(|n| format!("Boot{:04X}", n)).collect::<Vec<_>>(),
        "order_after_num": after,
        "order_changes": after != order,
        "slot": if esp.kept.is_some() { "from-kept" } else { "already-windows" },
        "restore_fallback": esp.fallback.as_deref() != snap_fallback.as_deref(),
        "snapshot_fallback_sha": snap_fallback,
        "snapshot_bootmgfw_sha": snap_bootmgfw,
    })
}

/// The record, the same shape the Windows-side rollback writes (`rollback/1`),
/// so one verdict reads both. `guard_removed` is null here: the guard removes
/// itself at the next Windows start and says so in the marker.
#[allow(clippy::too_many_arguments)]
pub fn record(job_id: &str, p: &Value, fallback_before: Option<&str>, fallback_after: Option<&str>, slot_before: Option<&str>, slot_after: Option<&str>, utc: &str, program: &str) -> Value {
    json!({
        "schema": "rollback/1",
        "rollback_version": VERSION,
        "rollback_program": program,
        "side": "linux",
        "job_id": job_id,
        "created_utc": utc,
        "fallback_loader": {
            "restored": p["restore_fallback"] == json!(true),
            "sha_before": fallback_before,
            "sha_after": fallback_after,
            "snapshot_sha": p["snapshot_fallback_sha"],
            "backup": BACKUP_REL,
        },
        "boot_order": {
            "before": p["order_before"],
            "after": p["order_after"],
            "windows_entry": p["windows_entry"],
            "windows_first": true,
            "written_by": "efivarfs",
        },
        "windows_slot": {
            "slot_sha_before": slot_before,
            "slot_sha_after": slot_after,
            "kept_name_removed": p["slot"] == json!("from-kept"),
            "guard_removed": Value::Null,
            "guard": "stands down at the next Windows start; the marker on the ESP (upgrade_/rolled-back.json) records it",
        },
        "linux_left_in_place": true,
    })
}

/// The marker the guard reads.
pub fn marker(job_id: &str, slot_after: Option<&str>, utc: &str, program: &str) -> Value {
    json!({ "schema": "rolled-back/1", "program": program, "job_id": job_id, "rolled_back_utc": utc, "slot_sha_after": slot_after,
            "guard": { "removed": Value::Null, "utc": Value::Null } })
}

/// Plain words for the screen (DRAFT, awaiting the owner's approval).
pub fn words() -> Value {
    json!({
        "intro": {
            "heading": "Windows is still on this computer",
            "lines": [
                "This computer was converted with Windows kept. Windows is still here, with everything that was in it.",
                "This makes Windows start first again, the way it did before: the computer starts straight into Windows, with no menu.",
                "Nothing is deleted. Linux stays on the disk. You can start it from the computer's own startup menu (the key the maker names, often F12 or Esc), and you can come back to it any time.",
                "The USB stick this computer was converted with must be plugged in: it holds the copy of the startup files from before.",
            ],
        },
        "word": {
            "heading": "Make Windows start first",
            "prompt": "Type WINDOWS (in capitals) to continue. Anything else stops.",
            "button": "Make Windows start first",
        },
        "done": {
            "heading": "Done",
            "lines": [
                "The next start boots Windows directly. Linux is still in the computer's startup menu; its space is untouched.",
                "The small helper this program left in Windows removes itself the first time Windows starts.",
            ],
            "restart": "Restart now",
        },
        "refused": "Nothing was changed. ",
        "failed": "The rollback did not complete. ",
    })
}

/// The live half: Linux only.
pub mod live {
    use super::*;
    use crate::gpt;
    use sha2::{Digest, Sha256};
    use std::path::{Path, PathBuf};

    pub struct Mounted {
        pub dir: String,
        pub by_us: bool,
    }

    fn read(p: &str) -> Option<String> {
        std::fs::read_to_string(p).ok().map(|t| t.trim().to_string())
    }

    pub fn sha(path: &Path) -> Option<String> {
        let mut f = std::fs::File::open(path).ok()?;
        let mut h = Sha256::new();
        std::io::copy(&mut f, &mut h).ok()?;
        Some(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
    }

    fn udev(root: &str, devnum: &str) -> BTreeMap<String, String> {
        let mut m = BTreeMap::new();
        if let Some(t) = read(&format!("{}/run/udev/data/b{}", root.trim_end_matches('/'), devnum)) {
            for l in t.lines() {
                if let Some((k, v)) = l.strip_prefix("E:").and_then(|kv| kv.split_once('=')) {
                    m.insert(k.to_string(), v.to_string());
                }
            }
        }
        m
    }

    /// Mount points by major:minor, from the kernel's own table.
    fn mounts(root: &str) -> Vec<(String, String, String)> {
        read(&format!("{}/proc/self/mountinfo", root.trim_end_matches('/'))).unwrap_or_default().lines().filter_map(|l| {
            let f: Vec<&str> = l.split(' ').collect();
            let dash = f.iter().position(|x| *x == "-")?;
            Some((f.get(2)?.to_string(), f.get(4)?.replace("\\040", " "), f.get(dash + 1)?.to_string()))
        }).collect()
    }

    fn mount_rw(dev: &str, target: &str, fstype: &str) -> Result<(), String> {
        std::fs::create_dir_all(target).map_err(|e| format!("{}: {}", target, e))?;
        let src = std::ffi::CString::new(dev).map_err(|e| e.to_string())?;
        let tgt = std::ffi::CString::new(target).map_err(|e| e.to_string())?;
        let fs = std::ffi::CString::new(fstype).map_err(|e| e.to_string())?;
        // SAFETY: NUL-terminated strings; a read-write mount of a FAT filesystem.
        let rc = unsafe { libc::mount(src.as_ptr(), tgt.as_ptr(), fs.as_ptr(), 0, std::ptr::null()) };
        if rc != 0 {
            return Err(format!("mounting {} at {}: {}", dev, target, std::io::Error::last_os_error()));
        }
        Ok(())
    }

    fn unmount(target: &str) {
        if let Ok(t) = std::ffi::CString::new(target) {
            // SAFETY: unmounting what this program mounted.
            unsafe { libc::umount2(t.as_ptr(), 0) };
        }
    }

    /// A partition's mount point, or a fresh read-write mount of it.
    fn mounted_or_mount(root: &str, part_sys: &str, dev: &str, at: &str) -> Result<Mounted, String> {
        let devnum = read(&format!("{}/dev", part_sys)).unwrap_or_default();
        if let Some((_, m, _)) = mounts(root).into_iter().find(|(d, _, _)| *d == devnum) {
            return Ok(Mounted { dir: m, by_us: false });
        }
        let fstype = udev(root, &devnum).get("ID_FS_TYPE").cloned().unwrap_or_else(|| "vfat".into());
        mount_rw(dev, at, &fstype)?;
        Ok(Mounted { dir: at.to_string(), by_us: true })
    }

    /// The kit stick: the partition labelled UPGV0 (by udev), mounted where
    /// it is or by us.
    pub fn find_stick(root: &str) -> Result<Mounted, String> {
        let r = root.trim_end_matches('/');
        let rd = std::fs::read_dir(format!("{}/sys/block", r)).map_err(|e| format!("the disk list could not be read ({})", e))?;
        for disk in rd.flatten() {
            let dn = disk.file_name().to_string_lossy().to_string();
            let sys = format!("{}/sys/block/{}", r, dn);
            let Ok(parts) = std::fs::read_dir(&sys) else { continue };
            for p in parts.flatten() {
                let pn = p.file_name().to_string_lossy().to_string();
                let psys = format!("{}/{}", sys, pn);
                if !pn.starts_with(&dn) || !Path::new(&format!("{}/partition", psys)).exists() {
                    continue;
                }
                let devnum = read(&format!("{}/dev", psys)).unwrap_or_default();
                if udev(r, &devnum).get("ID_FS_LABEL").map(String::as_str) == Some(STICK_LABEL) {
                    return mounted_or_mount(root, &psys, &format!("{}/dev/{}", r, pn), &format!("{}/run/upgrade_/stick", r));
                }
            }
        }
        Err(format!("no USB stick labelled {} is plugged in: the rollback needs the stick this computer was converted with", STICK_LABEL))
    }

    /// The ESP that holds Windows' boot folder: its mount, its GPT GUID and the disk it is on.
    pub fn find_esp(root: &str) -> Result<(Mounted, [u8; 16], Disk), String> {
        let r = root.trim_end_matches('/');
        let parts = gpt::all_partitions(root)?;
        let mut tried = Vec::new();
        for p in parts.iter().filter(|p| p.type_guid == gpt::ESP_TYPE) {
            let sep = if p.disk.ends_with(|c: char| c.is_ascii_digit()) { "p" } else { "" };
            let pn = format!("{}{}{}", p.disk, sep, p.number);
            let psys = format!("{}/sys/block/{}/{}", r, p.disk, pn);
            let m = mounted_or_mount(root, &psys, &format!("{}/dev/{}", r, pn), &format!("{}/run/upgrade_/esp", r))?;
            let has = find_ci(Path::new(&m.dir), SLOT_REL).is_some() || find_ci(Path::new(&m.dir), KEPT_REL).is_some();
            if has {
                let sectors = read(&format!("{}/sys/block/{}/size", r, p.disk)).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
                let devnum = read(&format!("{}/sys/block/{}/dev", r, p.disk)).unwrap_or_default();
                let u = udev(root, &devnum);
                let disk = Disk { size_bytes: sectors * 512, serial: u.get("ID_SERIAL_SHORT").or(u.get("ID_SERIAL")).cloned() };
                return Ok((m, p.unique, disk));
            }
            if m.by_us {
                unmount(&m.dir);
            }
            tried.push(pn);
        }
        Err(format!("no EFI System Partition holds Windows' boot folder (looked at: {})", if tried.is_empty() { "none".to_string() } else { tried.join(", ") }))
    }

    /// A path under `base`, matched case-insensitively component by component (FAT).
    pub fn find_ci(base: &Path, rel: &str) -> Option<PathBuf> {
        let mut cur = base.to_path_buf();
        for comp in rel.split(['/', '\\']).filter(|c| !c.is_empty()) {
            let rd = std::fs::read_dir(&cur).ok()?;
            let hit = rd.flatten().find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(comp))?;
            cur = hit.path();
        }
        Some(cur)
    }

    fn copy_checked(from: &Path, to: &Path, want: Option<&str>, what: &str) -> Result<String, String> {
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {}", what, e))?;
        }
        std::fs::copy(from, to).map_err(|e| format!("{}: {}", what, e))?;
        let got = sha(to).ok_or_else(|| format!("{}: the copy could not be read back", what))?;
        if let Some(w) = want
            && !got.eq_ignore_ascii_case(w) {
                return Err(format!("{}: the copy reads back as {} instead of {}", what, &got[..12], &w[..12.min(w.len())]));
            }
        Ok(got)
    }

    fn read_json(p: &Path) -> Value {
        std::fs::read_to_string(p).ok().and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok()).unwrap_or(Value::Null)
    }

    fn save(path: &Path, v: &Value) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {}", path.display(), e))?;
        }
        std::fs::write(path, serde_json::to_string_pretty(v).unwrap_or_default() + "\n").map_err(|e| format!("{}: {}", path.display(), e))
    }

    /// The rollback. `stick` names the stick's mount when the caller knows it
    /// (the rig); otherwise it is found by label. `dry` stops after the plan.
    /// Returns the result as JSON: `refused` (nothing touched), `failed`
    /// (what was done is listed), or the record itself with `result: done`.
    pub fn run(root: &str, stick: Option<&str>, dry: bool, now_utc: &str) -> Value {
        let program = format!("settle-in {} (Linux)", env!("CARGO_PKG_VERSION"));
        let handoff = format!("{}/var/lib/upgrade_", root.trim_end_matches('/'));
        let job = read_json(Path::new(&format!("{}/job.json", handoff)));
        let outcome = read_json(Path::new(&format!("{}/outcome.json", handoff)));
        let stick_m = match stick {
            Some(d) => Mounted { dir: d.to_string(), by_us: false },
            None => match find_stick(root) {
                Ok(m) => m,
                Err(e) => return json!({ "result": "refused", "why": [e] }),
            },
        };
        let stick_dir = Path::new(&stick_m.dir);
        let stick_job = read_json(&stick_dir.join("upgrade_").join("job.json"));
        let sums_text = find_ci(stick_dir, &format!("{}/SHA256SUMS", SNAPSHOT_REL)).and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
        let sums = super::sums(&sums_text);
        let (esp_m, esp_guid, disk) = match find_esp(root) {
            Ok(x) => x,
            Err(e) => {
                if stick_m.by_us { unmount(&stick_m.dir); }
                return json!({ "result": "refused", "why": [e] });
            }
        };
        let esp_dir = Path::new(&esp_m.dir);
        let at = |rel: &str| find_ci(esp_dir, rel);
        let esp = Esp {
            slot: at(SLOT_REL).and_then(|p| sha(&p)),
            kept: at(KEPT_REL).and_then(|p| sha(&p)),
            fallback: at(FALLBACK_REL).and_then(|p| sha(&p)),
            partition: Some(esp_guid),
        };
        let entries = efi::boot_entries(root);
        let order = efi::read_var(root, "BootOrder").map(|d| efi::u16_list(&d));
        let mut p = super::plan(&job, &outcome, &stick_job, &sums, &esp, &disk, &entries, order.as_deref());
        if sums_text.is_empty() {
            p["refusals"].as_array_mut().map(|r| r.push(json!(format!("the stick holds no {}/SHA256SUMS: it was not the stick this computer was converted with", SNAPSHOT_REL))));
        }
        let snap_file = find_ci(stick_dir, &format!("{}/{}", SNAPSHOT_REL, FALLBACK_REL));
        if p["restore_fallback"] == json!(true) && snap_file.is_none() {
            p["refusals"].as_array_mut().map(|r| r.push(json!(format!("the snapshot on the stick has no {} file to restore from", FALLBACK_REL))));
        }
        let finish = |v: Value| {
            if esp_m.by_us { unmount(&esp_m.dir); }
            if stick_m.by_us { unmount(&stick_m.dir); }
            v
        };
        if let Some(r) = p["refusals"].as_array().filter(|r| !r.is_empty()) {
            return finish(json!({ "result": "refused", "why": r, "esp": esp_m.dir, "stick": stick_m.dir }));
        }
        if dry {
            return finish(json!({ "result": "plan", "plan": p, "esp": esp_m.dir, "stick": stick_m.dir, "esp_state": { "slot": esp.slot, "kept": esp.kept, "fallback": esp.fallback } }));
        }
        let mut done: Vec<Value> = Vec::new();
        let fail = |done: &Vec<Value>, why: String| json!({ "result": "failed", "why": why, "done": done, "esp": esp_m.dir, "stick": stick_m.dir });
        let job_id = s(&job, "/job_id").to_string();
        // 1. the previous fallback copy, saved to the stick (shim, usually)
        if let Some(fb) = at(FALLBACK_REL) {
            match copy_checked(&fb, &stick_dir.join(BACKUP_REL), esp.fallback.as_deref(), "saving the previous fallback loader to the stick") {
                Ok(_) => done.push(json!({ "step": "backup", "to": BACKUP_REL })),
                Err(e) => return finish(fail(&done, e)),
            }
        }
        // 2. Windows' loader back in its slot
        let slot_path = at(SLOT_REL).unwrap_or_else(|| esp_dir.join(SLOT_REL));
        if let Some(kept) = at(KEPT_REL) {
            let n = match copy_checked(&kept, &slot_path, esp.kept.as_deref(), "putting Windows' loader back into its slot") {
                Ok(n) => n,
                Err(e) => return finish(fail(&done, e)),
            };
            if let Err(e) = std::fs::remove_file(&kept) {
                return finish(fail(&done, format!("removing the kept name: {}", e)));
            }
            done.push(json!({ "step": "slot", "slot_sha": n, "kept_name_removed": true }));
        }
        // 3. the fallback loader from the snapshot
        if p["restore_fallback"] == json!(true) {
            let want = p["snapshot_fallback_sha"].as_str().map(str::to_string);
            let fb = at(FALLBACK_REL).unwrap_or_else(|| esp_dir.join(FALLBACK_REL));
            match copy_checked(snap_file.as_ref().unwrap(), &fb, want.as_deref(), "restoring the fallback loader from the snapshot") {
                Ok(n) => done.push(json!({ "step": "fallback", "sha": n })),
                Err(e) => return finish(fail(&done, e)),
            }
        }
        let slot_after = at(SLOT_REL).and_then(|q| sha(&q));
        let fallback_after = at(FALLBACK_REL).and_then(|q| sha(&q));
        // 4. the marker the guard reads at the next Windows start
        if let Err(e) = save(&esp_dir.join(MARKER_REL), &marker(&job_id, slot_after.as_deref(), now_utc, &program)) {
            return finish(fail(&done, format!("writing the marker for the guard: {}", e)));
        }
        done.push(json!({ "step": "marker", "path": MARKER_REL }));
        // 5. the firmware's order: Windows first, read back
        if p["order_changes"] == json!(true) {
            let new: Vec<u16> = p["order_after_num"].as_array().map(|a| a.iter().filter_map(|n| n.as_u64().map(|n| n as u16)).collect()).unwrap_or_default();
            let bytes: Vec<u8> = new.iter().flat_map(|n| n.to_le_bytes()).collect();
            if let Err(e) = efi::write_var(root, "BootOrder", &bytes) {
                return finish(fail(&done, format!("the startup order could not be written ({})", e)));
            }
            if efi::read_var(root, "BootOrder").map(|d| efi::u16_list(&d)) != Some(new) {
                return finish(fail(&done, "the startup order did not read back as written".into()));
            }
        }
        if efi::read_var(root, "BootNext").is_some() {
            let _ = efi::delete_var(root, "BootNext");
        }
        done.push(json!({ "step": "order", "after": p["order_after"] }));
        // 6. the record: on the stick and in the handoff folder
        let mut rec = record(&job_id, &p, esp.fallback.as_deref(), fallback_after.as_deref(), esp.slot.as_deref(), slot_after.as_deref(), now_utc, &program);
        if let Err(e) = save(&stick_dir.join(RECORD_REL), &rec) {
            return finish(fail(&done, format!("writing the record to the stick: {}", e)));
        }
        let _ = save(Path::new(&format!("{}/rollback.json", handoff)), &rec);
        // SAFETY: sync has no preconditions.
        unsafe { libc::sync() };
        rec["result"] = json!("done");
        rec["done"] = json!(done);
        finish(rec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::efi::tests::load_option;

    const ESP: [u8; 16] = [1; 16];
    const WIN: &str = "dc0a8965c662a2e68ac8ab6ff392ddaccb631e723039fd496bea0dfdc69753bf";
    const SHIM: &str = "571ea56b855dcf73bec6acb63c5ded44c2a191138bca0d8cfa5aa93f60f46fff";

    fn job() -> Value {
        json!({ "schema": "job/1", "job_id": "j1", "intent": { "path": "keep-windows" },
                "identity": { "system_disk": { "size_bytes": 1000, "serial_number": "ABC123" } } })
    }
    fn outcome() -> Value {
        json!({ "schema": "outcome/1", "path_taken": "keep-windows", "windows": { "kept": true }, "cutover": { "esp_snapshot": { "path": "upgrade_/esp-snapshot" } } })
    }
    fn stick_job() -> Value {
        json!({ "schema": "job/1", "job_id": "j1" })
    }
    fn snapshot() -> BTreeMap<String, String> {
        sums(&format!("{WIN}  ./EFI/Boot/bootx64.efi\n{WIN}  ./EFI/Microsoft/Boot/bootmgfw.efi\n"))
    }
    fn esp_with_slot() -> Esp {
        Esp { slot: Some(SHIM.into()), kept: Some(WIN.into()), fallback: Some(SHIM.into()), partition: Some(ESP) }
    }
    fn disk() -> Disk {
        Disk { size_bytes: 1000, serial: Some("abc123".into()) }
    }
    fn entries() -> Vec<(u16, LoadOption)> {
        vec![
            (4, efi::parse_load_option(&load_option("Fedora", ESP, "\\EFI\\fedora\\shimx64.efi")).unwrap()),
            (1, efi::parse_load_option(&load_option("Windows Boot Manager", ESP, "\\EFI\\Microsoft\\Boot\\bootmgfw.efi")).unwrap()),
        ]
    }

    #[test]
    fn sums_are_keyed_like_the_windows_side() {
        let m = snapshot();
        assert_eq!(m.get("efi/boot/bootx64.efi").map(String::as_str), Some(WIN));
        assert_eq!(m.len(), 2);
    }

    #[test]
    fn the_plan_puts_windows_first_and_restores_both_files() {
        let p = plan(&job(), &outcome(), &stick_job(), &snapshot(), &esp_with_slot(), &disk(), &entries(), Some(&[4, 1, 0x2001]));
        assert_eq!(p["refusals"], json!([]));
        assert_eq!(p["windows_entry"], "Boot0001");
        assert_eq!(p["order_after"], json!(["Boot0001", "Boot0004", "Boot2001"]));
        assert_eq!(p["order_changes"], true);
        assert_eq!(p["slot"], "from-kept");
        assert_eq!(p["restore_fallback"], true);
    }

    #[test]
    fn an_esp_already_windows_has_nothing_to_restore() {
        let esp = Esp { slot: Some(WIN.into()), kept: None, fallback: Some(WIN.into()), partition: Some(ESP) };
        let p = plan(&job(), &outcome(), &stick_job(), &snapshot(), &esp, &disk(), &entries(), Some(&[1, 4]));
        assert_eq!(p["refusals"], json!([]));
        assert_eq!(p["slot"], "already-windows");
        assert_eq!(p["restore_fallback"], false);
        assert_eq!(p["order_changes"], false);
    }

    #[test]
    fn every_refusal_comes_before_any_write() {
        let refuse = |j: Value, o: Value, sj: Value, e: Esp, d: Disk, ents: Vec<(u16, LoadOption)>, order: Option<&[u16]>| -> Vec<String> {
            plan(&j, &o, &sj, &snapshot(), &e, &d, &ents, order)["refusals"].as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect()
        };
        let mut j = job(); j["intent"]["path"] = json!("clean-slate");
        assert!(refuse(j, outcome(), stick_job(), esp_with_slot(), disk(), entries(), Some(&[4, 1]))[0].contains("not keep-windows"));
        let mut o = outcome(); o["windows"]["kept"] = json!(false);
        assert!(refuse(job(), o, stick_job(), esp_with_slot(), disk(), entries(), Some(&[4, 1])).iter().any(|r| r.contains("not kept")));
        let mut sj = stick_job(); sj["job_id"] = json!("other");
        assert!(refuse(job(), outcome(), sj, esp_with_slot(), disk(), entries(), Some(&[4, 1])).iter().any(|r| r.contains("another conversion")));
        assert!(refuse(job(), outcome(), stick_job(), esp_with_slot(), Disk { size_bytes: 999, serial: None }, entries(), Some(&[4, 1])).iter().any(|r| r.contains("identity")));
        assert!(refuse(job(), outcome(), stick_job(), esp_with_slot(), Disk { size_bytes: 1000, serial: Some("zzz".into()) }, entries(), Some(&[4, 1])).iter().any(|r| r.contains("serial")));
        let mut e = esp_with_slot(); e.slot = None;
        assert!(refuse(job(), outcome(), stick_job(), e, disk(), entries(), Some(&[4, 1])).iter().any(|r| r.contains("nothing to put first")));
        let mut e = esp_with_slot(); e.kept = Some(SHIM.into());
        assert!(refuse(job(), outcome(), stick_job(), e, disk(), entries(), Some(&[4, 1])).iter().any(|r| r.contains("same bytes")));
        assert!(refuse(job(), outcome(), stick_job(), esp_with_slot(), disk(), vec![entries()[0].clone()], Some(&[4])).iter().any(|r| r.contains("no entry for Windows")));
        let mut two = entries(); two.push((2, entries()[1].1.clone()));
        assert!(refuse(job(), outcome(), stick_job(), esp_with_slot(), disk(), two, Some(&[4, 1, 2])).iter().any(|r| r.contains("2 entries")));
        assert!(refuse(job(), outcome(), stick_job(), esp_with_slot(), disk(), entries(), None).iter().any(|r| r.contains("BootOrder")));
    }

    #[test]
    fn a_windows_entry_on_another_esp_does_not_count() {
        let mut ents = entries();
        ents[1].1.partition = Some([9; 16]);
        let p = plan(&job(), &outcome(), &stick_job(), &snapshot(), &esp_with_slot(), &disk(), &ents, Some(&[4, 1]));
        assert!(p["refusals"][0].as_str().unwrap().contains("no entry for Windows"));
    }

    #[test]
    fn the_record_reads_like_the_windows_sides() {
        let p = plan(&job(), &outcome(), &stick_job(), &snapshot(), &esp_with_slot(), &disk(), &entries(), Some(&[4, 1]));
        let r = record("j1", &p, Some(SHIM), Some(WIN), Some(SHIM), Some(WIN), "2026-10-12T00:00:00Z", "settle-in 0.3.0 (Linux)");
        assert_eq!(r["schema"], "rollback/1");
        assert_eq!(r["fallback_loader"]["restored"], true);
        assert_eq!(r["fallback_loader"]["sha_after"], WIN);
        assert_eq!(r["boot_order"]["windows_first"], true);
        assert_eq!(r["boot_order"]["after"][0], "Boot0001");
        assert_eq!(r["windows_slot"]["kept_name_removed"], true);
        assert!(r["windows_slot"]["guard_removed"].is_null(), "the guard answers at the next Windows start, not here");
        assert_eq!(r["linux_left_in_place"], true);
    }

    #[test]
    fn the_words_hold_no_key_and_name_the_stick() {
        let w = words().to_string();
        assert!(w.contains("USB stick"));
        assert!(!w.to_lowercase().contains("product key"));
        assert_eq!(words()["word"]["prompt"], "Type WINDOWS (in capitals) to continue. Anything else stops.");
    }
}
