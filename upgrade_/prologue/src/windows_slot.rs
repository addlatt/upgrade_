//! Shim in Windows' slot (decided 2026-10-10, the owner; RISKS R21, the Acer
//! finding; architecture step 12). Some firmware (Acer's InsydeH2O) boots
//! only from its own priority list, where the one operating-system entry is
//! "Windows Boot Manager", the path `\EFI\Microsoft\Boot\bootmgfw.efi`; the
//! Linux entry Anaconda writes is never on that list, and the machine starts
//! Windows with no menu. On such firmware Windows' loader is moved one name
//! aside (same folder, so it still finds its BCD) and Fedora's signed shim is
//! put in its place; GRUB's Windows entry chainloads the kept name. The
//! judging half is here and replayed in tests; the live half (`live`) does the
//! files, the guard task and the undo.

use crate::val::{at, s, truthy};
use serde_json::{json, Value};

/// Windows' loader, under its kept name, in the same folder.
pub const KEPT_NAME: &str = "bootmgfw-kept.efi";
pub const WINDOWS_SLOT_REL: &str = "EFI\\Microsoft\\Boot\\bootmgfw.efi";
pub const KEPT_REL: &str = "EFI\\Microsoft\\Boot\\bootmgfw-kept.efi";
pub const SHIM_REL: &str = "EFI\\fedora\\shimx64.efi";
/// The bench-only marker on the stick that forces the arrangement on firmware
/// that would not need it (the rigs honour the Linux entry).
pub const BENCH_FORCE_MARKER: &str = "bench-force-windows-slot";
/// The guard task in the kept Windows (R22): re-applies the arrangement after
/// Windows' servicing puts its own file back.
pub const GUARD_TASK_NAME: &str = "upgrade_ windows-slot guard";

/// The detector, from the return's own evidence: the install completed on
/// the keep-Windows path, the installer left Fedora first, the handoff fired
/// (the stick did boot), and yet this code runs in Windows, which the firmware
/// chose by itself. `bench_force` is the rig's marker. None when the firmware
/// honoured the entry, or when the evidence is not all there.
pub fn needs_slot(outcome: &Value, fired: bool, bench_force: bool) -> Option<String> {
    if s(&outcome["status"]) != "completed" || s(&outcome["path_taken"]) != "keep-windows" {
        return None;
    }
    if bench_force {
        return Some("the bench marker forces the arrangement (the rig's firmware honours the Linux entry)".into());
    }
    if !fired {
        return None;
    }
    if !truthy(at(outcome, "cutover.boot_chain.linux_first_in_bootorder")) {
        return None;
    }
    Some("the install completed and left Fedora first in the firmware's order, yet the firmware started Windows with nobody pressing a key: it does not boot the Linux entry on its own".into())
}

/// What the files say before the arrangement is applied, judged: Ok(plan)
/// or the refusal. `slot`, `kept`, `shim` are the sha256 of the three files
/// (None = absent); `snapshot` is Windows' loader's sha from the stick's ESP
/// snapshot (None = no snapshot).
pub fn apply_plan(slot: Option<&str>, kept: Option<&str>, shim: Option<&str>, snapshot: Option<&str>) -> Result<Value, String> {
    let shim = shim.ok_or("no shim at EFI\\fedora\\shimx64.efi on the ESP: nothing to put in Windows' slot")?;
    let slot = slot.ok_or("no bootmgfw.efi in Windows' slot: refusing to touch a boot folder that is not as expected")?;
    if slot == shim {
        return Ok(json!({"AlreadyApplied": true, "MoveKept": false, "Reason": "shim is already in Windows' slot"}));
    }
    if let Some(k) = kept {
        if k != slot {
            return Err(format!("a kept copy already exists ({}) and differs from the loader in the slot ({}); refusing to overwrite it", &k[..12.min(k.len())], &slot[..12.min(slot.len())]));
        }
    }
    let matches_snapshot = snapshot.map(|x| x == slot);
    Ok(json!({"AlreadyApplied": false, "MoveKept": kept.is_none(), "SlotSha": slot, "ShimSha": shim, "MatchesSnapshot": matches_snapshot,
              "Reason": match matches_snapshot { Some(true) => "Windows' loader matches the snapshot; it moves to its kept name and shim takes the slot", Some(false) => "Windows' loader differs from the snapshot (serviced since?); it moves to its kept name as it is and shim takes the slot", None => "no snapshot to compare; Windows' loader moves to its kept name as it is and shim takes the slot" }}))
}

/// The guard's judgement at a Windows start: Windows' servicing put its own
/// file back when the slot no longer holds shim (either the shim recorded at
/// the arrangement or the shim on the ESP now).
pub fn guard_action(slot: Option<&str>, shim_now: Option<&str>, shim_recorded: &str) -> &'static str {
    match slot {
        None => "slot-missing",
        Some(x) if x == shim_recorded || Some(x) == shim_now => "nothing",
        Some(_) => "reapply",
    }
}

/// The record kept in the state directory and on the stick.
pub fn record(reason: &str, plan: &Value, slot_after: Option<&str>, kept_after: Option<&str>, utc: &str, program: &str) -> Value {
    json!({"schema": "windows-slot/1", "program": program, "applied_utc": utc, "reason": reason, "plan": plan,
           "slot_sha_after": slot_after, "kept_sha_after": kept_after, "guard_task": GUARD_TASK_NAME, "history": []})
}

#[cfg(windows)]
pub mod live {
    use super::*;
    use crate::live as plive;
    use crate::tools::Recorder;
    use sha2::{Digest, Sha256};
    use std::path::{Path, PathBuf};

    fn sha(path: &Path) -> Option<String> {
        if !path.exists() {
            return None;
        }
        let mut f = std::fs::File::open(path).ok()?;
        let mut h = Sha256::new();
        std::io::copy(&mut f, &mut h).ok()?;
        Some(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
    }

    /// Mount-Esp on a free letter (S to Z); the root, e.g. `S:\`.
    fn mount_esp(rec: &mut Recorder) -> Result<String, String> {
        let used: Vec<String> = plive::volumes().as_array().into_iter().flatten().filter_map(|v| v["DriveLetter"].as_str().map(|l| l.to_uppercase())).collect();
        let letter = "STUVWXYZ".chars().find(|c| !used.contains(&c.to_string()) && !Path::new(&format!("{c}:\\")).exists()).ok_or("no free drive letter to mount the ESP on")?;
        let _ = rec.run("mountvol", &[&format!("{letter}:"), "/S"]);
        if !Path::new(&format!("{letter}:\\EFI")).exists() {
            return Err(format!("mountvol {letter}: /S did not expose an EFI directory"));
        }
        Ok(format!("{letter}:\\"))
    }

    fn dismount_esp(rec: &mut Recorder, root: &str) {
        let _ = rec.run("mountvol", &[root.trim_end_matches('\\'), "/D"]);
    }

    fn snapshot_sha(stick_root: &str) -> Option<String> {
        let t = std::fs::read_to_string(Path::new(stick_root).join("upgrade_").join("esp-snapshot").join("SHA256SUMS")).ok()?;
        t.lines().find(|l| l.to_lowercase().replace('\\', "/").ends_with("efi/microsoft/boot/bootmgfw.efi")).map(|l| l[..64].to_lowercase())
    }

    fn record_path(state_dir: &str) -> PathBuf {
        Path::new(state_dir).join("windows-slot.json")
    }

    fn write_record(state_dir: &str, stick_root: Option<&str>, rec: &Value) {
        let text = serde_json::to_string_pretty(rec).unwrap_or_default() + "\n";
        let _ = std::fs::write(record_path(state_dir), &text);
        if let Some(root) = stick_root {
            let d = Path::new(root).join("upgrade_").join("report");
            let _ = std::fs::create_dir_all(&d);
            let _ = std::fs::write(d.join("windows-slot.json"), &text);
        }
    }

    fn copy_checked(from: &Path, to: &Path, what: &str) -> Result<String, String> {
        std::fs::copy(from, to).map_err(|e| format!("{what}: {e}"))?;
        let (a, b) = (sha(from), sha(to));
        if a.is_none() || a != b {
            return Err(format!("{what}: the copy's sha256 differs from the source"));
        }
        Ok(b.unwrap_or_default())
    }

    /// The guard task's XML: SYSTEM at startup, like the resume task, running `guard`.
    fn guard_task_xml(exe: &str, state: &str) -> String {
        plive::resume_task_xml(exe, state).replace("<Arguments>resume --state-dir", "<Arguments>guard --state-dir").replace("continue the conversion after a restart (removed by the prologue itself)", "keep shim in Windows' slot after Windows' servicing (RISKS R22; removed by the rollback)")
    }

    fn register_guard(rec: &mut Recorder, state: &str) -> Result<(), String> {
        let me = std::env::current_exe().map_err(|e| e.to_string())?;
        let copy = Path::new(state).join("upgrade-prologue.exe");
        plive::place_program(&me, &copy)?;
        let xml_path = Path::new(state).join("guard-task.xml");
        let xml = guard_task_xml(&copy.to_string_lossy(), state);
        let mut bytes = vec![0xFF, 0xFE];
        for u in xml.encode_utf16() {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        std::fs::write(&xml_path, bytes).map_err(|e| e.to_string())?;
        let xp = xml_path.to_string_lossy().to_string();
        let r = rec.run("schtasks", &["/Create", "/TN", GUARD_TASK_NAME, "/XML", &xp, "/F"]).clone();
        if !r.ok() {
            return Err(format!("schtasks could not register the guard task ({})", r.text().trim()));
        }
        Ok(())
    }

    pub fn unregister_guard(rec: &mut Recorder) -> bool {
        if !rec.run("schtasks", &["/Query", "/TN", GUARD_TASK_NAME]).ok() {
            return false;
        }
        rec.run("schtasks", &["/Delete", "/TN", GUARD_TASK_NAME, "/F"]).ok()
    }

    /// Apply the arrangement: Windows' loader to its kept name, shim into the
    /// slot, the guard registered, the record written. Every refusal comes
    /// before the first write. `stick_root` (when the stick is there) gives
    /// the snapshot to compare against and a place for the record.
    pub fn apply(rec: &mut Recorder, state_dir: &str, stick_root: Option<&str>, reason: &str, program: &str, log: &mut dyn FnMut(String)) -> Result<Value, String> {
        let esp = mount_esp(rec)?;
        let result = (|| -> Result<Value, String> {
            let slot = Path::new(&esp).join(WINDOWS_SLOT_REL);
            let kept = Path::new(&esp).join(KEPT_REL);
            let shim = Path::new(&esp).join(SHIM_REL);
            let (s_slot, s_kept, s_shim) = (sha(&slot), sha(&kept), sha(&shim));
            let snap = stick_root.and_then(snapshot_sha);
            let plan = apply_plan(s_slot.as_deref(), s_kept.as_deref(), s_shim.as_deref(), snap.as_deref())?;
            log(format!("      windows slot: {}", s(&plan["Reason"])));
            if !truthy(&plan["AlreadyApplied"]) {
                if truthy(&plan["MoveKept"]) {
                    let k = copy_checked(&slot, &kept, "keeping Windows' loader under its kept name")?;
                    log(format!("      {KEPT_REL} written ({})", &k[..12]));
                }
                let n = copy_checked(&shim, &slot, "putting shim into Windows' slot")?;
                log(format!("      {WINDOWS_SLOT_REL} is now shim ({})", &n[..12]));
            }
            register_guard(rec, state_dir)?;
            log(format!("      guard task registered: {GUARD_TASK_NAME}"));
            let r = record(reason, &plan, sha(&slot).as_deref(), sha(&kept).as_deref(), &plive::now_z(), program);
            write_record(state_dir, stick_root, &r);
            Ok(r)
        })();
        dismount_esp(rec, &esp);
        result
    }

    /// The guard at a Windows start: nothing, or shim back into the slot
    /// after Windows' servicing put its own file there (the newer Microsoft
    /// file becomes the kept copy).
    pub fn guard(rec: &mut Recorder, state_dir: &str, log: &mut dyn FnMut(String)) -> Result<&'static str, String> {
        let p = record_path(state_dir);
        let mut r: Value = serde_json::from_str(&std::fs::read_to_string(&p).map_err(|_| "no windows-slot.json: the arrangement was never applied here")?).map_err(|e| e.to_string())?;
        let shim_recorded = s(&r["plan"]["ShimSha"]);
        let esp = mount_esp(rec)?;
        let result = (|| -> Result<&'static str, String> {
            let slot = Path::new(&esp).join(WINDOWS_SLOT_REL);
            let kept = Path::new(&esp).join(KEPT_REL);
            let shim = Path::new(&esp).join(SHIM_REL);
            let action = guard_action(sha(&slot).as_deref(), sha(&shim).as_deref(), &shim_recorded);
            log(format!("      guard: slot {} -> {action}", sha(&slot).map(|x| x[..12].to_string()).unwrap_or("absent".into())));
            if action == "reapply" {
                let k = copy_checked(&slot, &kept, "keeping Windows' serviced loader under its kept name")?;
                let n = copy_checked(&shim, &slot, "putting shim back into Windows' slot")?;
                log(format!("      kept {} ; slot {}", &k[..12], &n[..12]));
                if let Some(h) = r["history"].as_array_mut() {
                    h.push(json!({"utc": plive::now_z(), "action": "reapplied", "kept_sha": k, "slot_sha": n}));
                }
                let _ = std::fs::write(&p, serde_json::to_string_pretty(&r).unwrap_or_default() + "\n");
            }
            Ok(action)
        })();
        dismount_esp(rec, &esp);
        result
    }

    /// The undo (the rollback): Windows' loader back from its kept name, the
    /// kept name removed, the guard gone, the record kept as history.
    /// Returns what was done, or None when the arrangement was never applied.
    pub fn undo(rec: &mut Recorder, state_dir: &str, log: &mut dyn FnMut(String)) -> Result<Option<Value>, String> {
        let esp = mount_esp(rec)?;
        let result = (|| -> Result<Option<Value>, String> {
            let slot = Path::new(&esp).join(WINDOWS_SLOT_REL);
            let kept = Path::new(&esp).join(KEPT_REL);
            if !kept.exists() {
                return Ok(None);
            }
            let before = sha(&slot);
            let n = copy_checked(&kept, &slot, "putting Windows' loader back into its slot")?;
            std::fs::remove_file(&kept).map_err(|e| format!("removing the kept name: {e}"))?;
            let removed = unregister_guard(rec);
            log(format!("      Windows' loader back in its slot ({}); kept name removed; guard task removed: {removed}", &n[..12]));
            let p = record_path(state_dir);
            let _ = std::fs::rename(&p, Path::new(state_dir).join("windows-slot-undone.json"));
            Ok(Some(json!({"slot_sha_before": before, "slot_sha_after": n, "guard_removed": removed})))
        })();
        dismount_esp(rec, &esp);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(status: &str, path: &str, linux_first: bool) -> Value {
        json!({"status": status, "path_taken": path, "cutover": {"boot_chain": {"linux_first_in_bootorder": linux_first}}})
    }

    #[test]
    fn the_detector_needs_every_piece_of_evidence() {
        assert!(needs_slot(&outcome("completed", "keep-windows", true), true, false).is_some());
        assert!(needs_slot(&outcome("completed", "keep-windows", true), false, false).is_none(), "the stick never booted: nothing to conclude");
        assert!(needs_slot(&outcome("completed", "keep-windows", false), true, false).is_none(), "Fedora was not left first: the firmware was never asked");
        assert!(needs_slot(&outcome("stopped", "keep-windows", true), true, false).is_none());
        assert!(needs_slot(&outcome("completed", "clean-slate", true), true, false).is_none());
        assert!(needs_slot(&outcome("completed", "keep-windows", false), false, true).is_some(), "the bench marker forces it");
        assert!(needs_slot(&outcome("stopped", "keep-windows", false), false, true).is_none(), "but never on a stopped install");
    }

    #[test]
    fn the_plan_refuses_before_it_writes() {
        assert!(apply_plan(Some("aa"), None, None, None).is_err(), "no shim");
        assert!(apply_plan(None, None, Some("ss"), None).is_err(), "no loader in the slot");
        assert!(apply_plan(Some("aa"), Some("bb"), Some("ss"), None).is_err(), "a different kept copy already there");
        let p = apply_plan(Some("aa"), Some("aa"), Some("ss"), Some("aa")).unwrap();
        assert!(!truthy(&p["MoveKept"]) && !truthy(&p["AlreadyApplied"]), "the kept copy is already right: only the slot changes");
        let p = apply_plan(Some("aa"), None, Some("ss"), Some("zz")).unwrap();
        assert_eq!(p["MatchesSnapshot"], json!(false));
        assert!(truthy(&apply_plan(Some("ss"), None, Some("ss"), None).unwrap()["AlreadyApplied"]));
    }

    #[test]
    fn the_guard_reapplies_only_when_windows_took_the_slot_back() {
        assert_eq!(guard_action(Some("ss"), Some("ss"), "ss"), "nothing");
        assert_eq!(guard_action(Some("s2"), Some("s2"), "ss"), "nothing", "a newer shim on the ESP counts as shim");
        assert_eq!(guard_action(Some("ms"), Some("ss"), "ss"), "reapply");
        assert_eq!(guard_action(None, Some("ss"), "ss"), "slot-missing");
    }
}
