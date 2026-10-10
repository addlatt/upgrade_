//! Rollback, following `upgrade_/windows/Invoke-Rollback.ps1` 0.1.0: Windows
//! Boot Manager first again, its fallback loader restored from the ESP
//! snapshot. It deletes nothing. The judging half (the plan and its
//! refusals) is held to the script by `tests/rollback.rs`; the live half
//! runs on Windows with every tool call kept.

#[cfg_attr(not(windows), allow(unused_imports))]
use crate::val::{at, int, s, truthy};
use serde_json::{json, Value};
use upgrade_scan::ps::capture;

pub const FOLLOWS_ROLLBACK: &str = "0.1.0";
pub const FALLBACK_REL: &str = "EFI/Boot/bootx64.efi";

/// ConvertFrom-RollbackSums: sha256sum lines keyed lower-case with forward
/// slashes, `./` stripped (FAT is case-insensitive).
pub fn sums(lines: &[String]) -> Value {
    let mut h = serde_json::Map::new();
    for l in lines {
        let re = upgrade_scan::ps::re(r"^([0-9a-fA-F]{64})\s[\s*](.+)$", "");
        if let Some(m) = re.find(l) {
            let (Some(sha), Some(path)) = (m.group(1), m.group(2)) else { continue };
            let key = l[path].trim().trim_start_matches("./").replace('\\', "/").to_lowercase();
            h.insert(key, json!(l[sha].to_lowercase()));
        }
    }
    Value::Object(h)
}

/// Get-RollbackPlan: every refusal, then the plan. Nothing here touches the machine.
pub fn plan(outcome: &Value, sums: &Value, current_sha: &str, identity_mismatches: &[String]) -> Value {
    let mut r: Vec<String> = Vec::new();
    if !truthy(outcome) {
        r.push("no outcome.json on the stick - nothing says a conversion happened here".into());
    } else {
        if s(&outcome["schema"]) != "outcome/1" {
            r.push(format!("outcome schema '{}' is not outcome/1", s(&outcome["schema"])));
        }
        if s(&outcome["path_taken"]) != "keep-windows" {
            r.push(format!("the conversion's path was '{}', not keep-windows - there is no kept Windows to roll back to", s(&outcome["path_taken"])));
        }
        if !truthy(&outcome["windows"]) || !truthy(at(outcome, "windows.kept")) {
            r.push("the outcome says Windows was not kept".into());
        }
        if !truthy(&outcome["cutover"]) || !truthy(at(outcome, "cutover.esp_snapshot")) {
            r.push("the outcome names no ESP snapshot".into());
        }
    }
    for m in identity_mismatches {
        r.push(format!("identity: {m}"));
    }
    let want = if truthy(sums) { s(&sums[FALLBACK_REL.to_lowercase()]) } else { String::new() };
    if want.is_empty() {
        r.push(format!("the snapshot has no checksum for {FALLBACK_REL} - nothing to restore from"));
    }
    if !r.is_empty() {
        return json!({"Refusals": r, "Restore": false, "WantSha": if want.is_empty() { Value::Null } else { json!(want) }});
    }
    let restore = !current_sha.eq_ignore_ascii_case(&want);
    json!({"Refusals": [], "Restore": restore, "WantSha": want, "CurrentSha": current_sha,
           "Reason": if restore { "the fallback slot holds something other than Windows' copy" } else { "the fallback slot already holds Windows' copy" }})
}

/// Compare-RollbackIdentity: a moved stick must never touch a stranger's ESP.
pub fn compare_identity(job: &Value, disk: &Value) -> Vec<String> {
    let mut m = Vec::new();
    if !s(at(job, "identity.system_disk.unique_id")).eq_ignore_ascii_case(&s(&disk["UniqueId"])) {
        m.push(format!("system disk unique id: job '{}', machine '{}'", s(at(job, "identity.system_disk.unique_id")), s(&disk["UniqueId"])));
    }
    if !s(at(job, "identity.system_disk.size_bytes")).eq_ignore_ascii_case(&s(&disk["Size"])) {
        m.push(format!("system disk size: job {}, machine {}", s(at(job, "identity.system_disk.size_bytes")), s(&disk["Size"])));
    }
    m
}

/// Get-DisplayOrderTokens: `bcdedit /enum {fwbootmgr}`'s displayorder, continuation lines included.
pub fn display_order_tokens(text: &str) -> Vec<String> {
    capture(r"^\s*displayorder\s+(.+(?:\r?\n\s{20,}.+)*)", "m", text).map(|m| m.split_whitespace().map(str::to_string).collect()).unwrap_or_default()
}

/// Test-WindowsFirst.
pub fn windows_first(tokens: &[String]) -> bool {
    tokens.first().is_some_and(|t| t.eq_ignore_ascii_case("{bootmgr}"))
}

#[cfg(windows)]
pub mod live {
    //! Invoke-Rollback's live half.
    use super::*;
    use crate::judge;
    use crate::tools::Recorder;
    use sha2::{Digest, Sha256};
    use std::path::{Path, PathBuf};
    use upgrade_scan::collect::storage::storage_wmi;

    fn sha(path: &Path) -> Option<String> {
        if !path.exists() {
            return None;
        }
        let mut f = std::fs::File::open(path).ok()?;
        let mut h = Sha256::new();
        std::io::copy(&mut f, &mut h).ok()?;
        Some(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
    }

    fn read_json(p: &Path) -> Option<Value> {
        serde_json::from_str(std::fs::read_to_string(p).ok()?.trim_start_matches('\u{feff}')).ok()
    }

    /// Mount-Esp on a free letter (S to Z); the root, e.g. `S:\`.
    fn mount_esp(rec: &mut Recorder) -> Result<String, String> {
        let used: Vec<String> = crate::live::volumes().as_array().into_iter().flatten().filter_map(|v| v["DriveLetter"].as_str().map(|l| l.to_uppercase())).collect();
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

    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() { walk(&p, out) } else { out.push(p) }
            }
        }
    }

    /// Invoke-Rollback. Prints as the script prints; exit code as the script's.
    pub fn rollback(rec: &mut Recorder, stick_drive: &str, version_line: &str, state_dir: &str) -> Result<i32, String> {
        let root = judge::drive_root(stick_drive)?;
        if !Path::new(&root).exists() {
            return Err(format!("stick {root} not found"));
        }
        println!();
        println!("  upgrade_  rollback {FOLLOWS_ROLLBACK}  ({version_line})");
        println!("  Windows Boot Manager first again; its fallback loader restored from the snapshot. Deletes nothing.");
        let up = Path::new(&root).join("upgrade_");
        let job = read_json(&up.join("job.json"));
        let outcome = read_json(&up.join("outcome.json")).unwrap_or(Value::Null);
        let snap_dir = up.join("esp-snapshot");
        let sums_v = std::fs::read_to_string(snap_dir.join("SHA256SUMS")).ok().map(|t| sums(&t.lines().map(|l| l.trim_end_matches('\r').to_string()).collect::<Vec<_>>())).unwrap_or(Value::Null);
        let w = storage_wmi()?;
        let part = w.query_where("MSFT_Partition", &["DiskNumber"], "DriveLetter='C'")?.into_iter().next().ok_or("No MSFT_Partition objects found with property 'DriveLetter' equal to 'C'. Verify the value of the property and retry.")?;
        let n = int(&part["DiskNumber"]);
        let disk = w.query_where("MSFT_Disk", &["UniqueId", "Size"], &format!("Number={n}"))?.into_iter().next().ok_or_else(|| format!("No MSFT_Disk objects found with property 'Number' equal to '{n}'. Verify the value of the property and retry."))?;
        let mm = match &job {
            Some(j) => compare_identity(j, &json!({"UniqueId": s(&disk["UniqueId"]), "Size": int(&disk["Size"])})),
            None => vec!["no job.json on the stick".to_string()],
        };
        let esp = mount_esp(rec)?;
        let result = (|| -> Result<i32, String> {
            let fallback = Path::new(&esp).join("EFI").join("Boot").join("bootx64.efi");
            let cur = sha(&fallback).unwrap_or_default();
            let p = plan(&outcome, &sums_v, &cur, &mm);
            let refusals: Vec<String> = crate::val::items(&p["Refusals"]).into_iter().map(s).collect();
            if !refusals.is_empty() {
                println!();
                println!("  REFUSED - nothing changed:");
                for x in &refusals {
                    println!("    - {x}");
                }
                println!();
                return Ok(2);
            }
            let before = rec.run("bcdedit", &["/enum", "{fwbootmgr}"]).text();
            let order_before = display_order_tokens(&before);
            println!("  fallback slot now: {cur}");
            println!("  Windows' copy:     {}", s(&p["WantSha"]));
            println!("  firmware order:    {}", order_before.join(" "));
            let (mut restored, mut backup) = (false, Value::Null);
            if truthy(&p["Restore"]) {
                let bdir = up.join("rollback");
                std::fs::create_dir_all(&bdir).map_err(|e| e.to_string())?;
                std::fs::copy(&fallback, bdir.join("bootx64.efi.before")).map_err(|e| format!("backing up the fallback loader: {e}"))?;
                backup = json!("upgrade_/rollback/bootx64.efi.before");
                let mut all = Vec::new();
                walk(&snap_dir, &mut all);
                let src = all.into_iter().find(|f| f.strip_prefix(&snap_dir).map(|r| r.to_string_lossy().replace('\\', "/").to_lowercase()).unwrap_or_default() == FALLBACK_REL.to_lowercase()).ok_or_else(|| format!("the snapshot directory has no {FALLBACK_REL} although its manifest lists one"))?;
                if sha(&src).as_deref() != Some(&s(&p["WantSha"])) {
                    return Err(format!("the snapshot's {FALLBACK_REL} does not match its own manifest; refusing to copy a file that fails its checksum"));
                }
                std::fs::copy(&src, &fallback).map_err(|e| format!("copying Windows' loader back: {e}"))?;
                let after = sha(&fallback).unwrap_or_default();
                if after != s(&p["WantSha"]) {
                    return Err(format!("after the copy the fallback loader's sha256 is {after}, not {}", s(&p["WantSha"])));
                }
                restored = true;
                println!("  restored EFI\\Boot\\bootx64.efi from the snapshot (previous copy saved to {})", s(&backup));
            } else {
                println!("  {}; nothing to restore", s(&p["Reason"]));
            }
            // shim in Windows' slot (architecture step 12): Windows' loader back under its own name first
            let mut slot_lines = Vec::new();
            let slot_undone = crate::windows_slot::live::undo(rec, state_dir, &mut |l| slot_lines.push(l))?;
            for l in &slot_lines {
                println!("{l}");
            }
            let _ = rec.run("bcdedit", &["/deletevalue", "{fwbootmgr}", "bootsequence"]);
            let setr = rec.run("bcdedit", &["/set", "{fwbootmgr}", "displayorder", "{bootmgr}", "/addfirst"]).clone();
            if !setr.ok() {
                return Err("bcdedit could not put Windows Boot Manager first".into());
            }
            let after_text = rec.run("bcdedit", &["/enum", "{fwbootmgr}"]).text();
            let order_after = display_order_tokens(&after_text);
            let win_first = windows_first(&order_after);
            println!("  firmware order now: {}  (Windows first: {win_first})", order_after.join(" "));
            let record = json!({
                "schema": "rollback/1", "rollback_version": FOLLOWS_ROLLBACK, "rollback_program": version_line, "job_id": job.as_ref().map_or(String::new(), |j| s(&j["job_id"])),
                "created_utc": crate::live::now_z(),
                "fallback_loader": {"restored": restored, "sha_before": cur, "sha_after": sha(&fallback), "snapshot_sha": p["WantSha"], "backup": backup},
                "boot_order": {"before": order_before, "after": order_after, "windows_first": win_first},
                "windows_slot": slot_undone,
                "linux_left_in_place": true,
            });
            std::fs::write(up.join("rollback.json"), serde_json::to_string_pretty(&record).unwrap_or_default() + "\n").map_err(|e| e.to_string())?;
            let _ = rec.write_all(&up.join("report").join("rollback-tools.jsonl"));
            println!("  record: {}upgrade_\\rollback.json", root);
            if !win_first {
                return Ok(3);
            }
            println!();
            println!("  Done. The next start boots Windows directly. Linux is still in the firmware boot menu; its space is untouched.");
            println!();
            Ok(0)
        })();
        dismount_esp(rec, &esp);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sums_and_plan_as_the_script() {
        let win = "a".repeat(64);
        let lines = vec![format!("{win}  ./EFI/Boot/bootx64.efi"), format!("{}  ./EFI/Microsoft/Boot/bootmgfw.efi", "b".repeat(64))];
        let m = sums(&lines);
        assert_eq!(s(&m["efi/boot/bootx64.efi"]), win);
        let out = json!({"schema": "outcome/1", "path_taken": "keep-windows", "windows": {"kept": true}, "cutover": {"esp_snapshot": {"path": "upgrade_/esp-snapshot"}}});
        let p = plan(&out, &m, &"c".repeat(64), &[]);
        assert!(truthy(&p["Restore"]) && crate::val::items(&p["Refusals"]).is_empty());
        let p = plan(&Value::Null, &m, &"c".repeat(64), &[]);
        assert!(s(&p["Refusals"][0]).contains("no outcome"));
        assert_eq!(display_order_tokens("identifier {fwbootmgr}\ndisplayorder            {a}\n                        {bootmgr}\ntimeout 0"), vec!["{a}", "{bootmgr}"]);
        assert!(windows_first(&["{bootmgr}".to_string()]) && !windows_first(&[]));
    }
}
