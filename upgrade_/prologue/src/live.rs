//! The prologue's live half, Windows only: every read of the machine and
//! every reversible write, following `Invoke-Prologue.ps1` 0.12.0 function
//! for function. Every tool runs through [`Recorder`] so its raw output is
//! kept. Nothing here decides: the judging half (`judge`, `state`) does.

use crate::judge;
use crate::tools::Recorder;
use crate::val::{at, int, items, s, truthy};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use upgrade_scan::collect::registry::{self, Hive};
use upgrade_scan::collect::storage::{bus_type, health_status, operational_status, storage_wmi};
use upgrade_scan::collect::wmi::{Wmi, BITLOCKER, CIMV2};
use upgrade_scan::collect::{self, events};
use upgrade_scan::parse::{chkdsk_event, defrag_259, diskpart_query_max, disk_events, fsutil_dirty};
use upgrade_scan::ps::{capture, matches, Stamp};

const GB: f64 = 1073741824.0;
const DAY: i64 = 86400;
pub const TASK_NAME: &str = "upgrade_ prologue resume";
pub const NOTICE_RUNONCE_NAME: &str = "upgrade_ prologue notice";
pub const STICK_WAIT_SECONDS: i64 = 120;
pub const GRUB_ENV_REL: &str = "EFI\\BOOT\\grubenv";
pub const PAYLOAD_EFI: &str = "\\EFI\\BOOT\\BOOTX64.EFI";
pub const STAGE_PROBE_BYTES: usize = 32 * 1048576;
const RUNONCE: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\RunOnce";

fn text(v: &Value) -> String {
    s(v)
}

/// Now, as .NET's round-trip text (`...Z`) and as `yyyy-MM-ddTHH:mm:ssZ`.
pub fn now_o() -> String {
    collect::now().1
}
pub fn now_z() -> String {
    format!("{}Z", &now_o()[..19])
}
pub fn now_local() -> Stamp {
    collect::now().0
}

/// A local time as UTC round-trip text (what `.ToUniversalTime().ToString('o')` gives).
pub fn local_to_utc_o(local: Stamp) -> String {
    use windows::Win32::Foundation::SYSTEMTIME;
    use windows::Win32::System::Time::TzSpecificLocalTimeToSystemTime;
    let st = SYSTEMTIME { wYear: local.year as u16, wMonth: local.month as u16, wDay: local.day as u16, wHour: local.hour as u16, wMinute: local.minute as u16, wSecond: local.second as u16, ..Default::default() };
    let mut utc = SYSTEMTIME::default();
    if unsafe { TzSpecificLocalTimeToSystemTime(None, &st, &mut utc) }.is_err() {
        return format!("{}.0000000Z", local.iso());
    }
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.0000000Z", utc.wYear, utc.wMonth, utc.wDay, utc.wHour, utc.wMinute, utc.wSecond)
}

/// A UTC round-trip text as a local Stamp (for an event filter).
pub fn utc_o_to_local(text: &str) -> Option<Stamp> {
    let st = Stamp::parse(text.get(..19)?)?;
    Some(collect::utc_to_local(st))
}

/// Test-Elevated.
pub fn is_elevated() -> bool {
    upgrade_scan::collect::win::is_admin()
}

/// Test-UefiBoot.
pub fn is_uefi(rec: &mut Recorder) -> bool {
    if std::env::var("firmware_type").map(|v| v == "UEFI").unwrap_or(false) {
        return true;
    }
    let r = rec.run("bcdedit", &["/enum", "{fwbootmgr}"]);
    r.ok() && matches("bootsequence|displayorder", &r.text())
}

/// Get-SecureBootState as the facts read it.
pub fn secure_boot() -> &'static str {
    match registry::dword(&Hive::LocalMachine, collect::SECURE_BOOT, "UEFISecureBootEnabled") {
        Some(1) => "on",
        Some(0) => "off",
        _ => "unknown",
    }
}

/// Get-BitLockerState: `{State, Source, Raw}`.
pub fn bitlocker_state(rec: &mut Recorder) -> Value {
    if let Ok(w) = Wmi::connect(BITLOCKER) {
        if let Ok(l) = w.query_where("Win32_EncryptableVolume", &["ProtectionStatus"], "DriveLetter='C:'") {
            if let Some(v) = l.first() {
                let st = match int(&v["ProtectionStatus"]) {
                    1 => "on",
                    0 => "off",
                    _ => "unknown",
                };
                if st != "unknown" {
                    return json!({"State": st, "Source": "cmdlet"});
                }
            }
        }
    }
    let r = rec.run("manage-bde", &["-status", "C:"]).clone();
    let st = judge::manage_bde(&r.lines());
    if st != "unknown" {
        return json!({"State": st, "Source": "manage-bde"});
    }
    json!({"State": "unknown", "Source": "none", "Raw": r.text()})
}

/// Get-PrologueDiskHealth.
pub fn disk_health(disk_number: i64, unique_id: &str) -> String {
    let Ok(w) = storage_wmi() else { return "Unknown".into() };
    let Ok(all) = w.query("MSFT_PhysicalDisk", &["DeviceId", "UniqueId", "HealthStatus"]) else { return "Unknown".into() };
    let by_number = all.iter().find(|p| text(&p["DeviceId"]) == disk_number.to_string());
    let pd = by_number.or_else(|| if unique_id.is_empty() { None } else { all.iter().find(|p| text(&p["UniqueId"]) == unique_id) });
    pd.map_or("Unknown".to_string(), |p| health_status(&p["HealthStatus"]))
}

/// Get-PrologueDiskEvents: the System log's `disk` events, 30 days.
pub fn disk_events_facts(disk_number: i64) -> Value {
    match events::query("System", &format!("*[System[Provider[@Name='disk'] and {}]]", events::within(30 * DAY))) {
        Ok(list) => {
            let evs: Vec<upgrade_scan::facts::DiskEvent> = list.into_iter().map(|e| upgrade_scan::facts::DiskEvent { id: e.id, time_created: e.time_local, message: Some(e.message) }).collect();
            let r = disk_events(&evs, disk_number);
            json!({"BadBlock": r.bad_block, "Paging": r.paging, "Reset": r.reset, "First": r.first.map(|t| t.iso()), "Last": r.last.map(|t| t.iso())})
        }
        Err(e) => json!({"BadBlock": 0, "Paging": 0, "Reset": 0, "First": null, "Last": null, "Error": e}),
    }
}

/// Get-PrologueVolumeEvidence: what Windows itself says about C: since a
/// moment (local time).
pub fn volume_evidence(since: Stamp) -> Value {
    let mut r = json!({"VolumeStatus": null, "VolumeHealth": null, "NtfsFullChkdsk": null, "LastCheck": null, "LogVerdict": "unknown", "LogRecords": 0, "LogQueued": 0, "LogWhen": null});
    let mut last: Option<Stamp> = events::query("Application", &format!("*[System[(EventID=1001) and {}]]", events::within(60 * DAY))).ok().and_then(|l| l.into_iter().find(|e| matches("Wininit", &e.provider))).map(|e| e.time_local);
    if let Ok(dir) = std::fs::read_dir(r"C:\System Volume Information\Chkdsk") {
        let newest = dir.flatten().filter(|e| { let n = e.file_name().to_string_lossy().to_lowercase(); n.starts_with("chkdsk") && n.ends_with(".log") }).filter_map(|e| e.metadata().ok()?.modified().ok()).max();
        if let Some(m) = newest {
            let secs = m.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
            let st = collect::utc_to_local(collect::utc_from_seconds(secs));
            if last.is_none_or(|l| st > l) {
                last = Some(st);
            }
        }
    }
    if let Some(l) = last {
        r["LastCheck"] = json!(local_to_utc_o(l));
    }
    if let Some(v) = storage_wmi().ok().and_then(|w| w.query_where("MSFT_Volume", &["OperationalStatus", "HealthStatus"], "DriveLetter='C'").ok()).and_then(|l| l.into_iter().next()) {
        r["VolumeStatus"] = json!(operational_status(&v["OperationalStatus"], true));
        r["VolumeHealth"] = json!(health_status(&v["HealthStatus"]));
    }
    if let Some(e) = events::query("System", &format!("*[System[(EventID=98) and {}]]", events::within(30 * DAY))).ok().and_then(|l| l.into_iter().find(|e| matches("Ntfs", &e.provider) && matches("Full Chkdsk", &e.message) && matches("Volume C:", &e.message))) {
        r["NtfsFullChkdsk"] = json!(local_to_utc_o(e.time_local));
    }
    let secs_since = (now_local().seconds() - since.seconds()).max(0);
    if let Some(e) = events::query("Application", &format!("*[System[Provider[@Name='Chkdsk'] and {}]]", events::within(secs_since + 5))).ok().and_then(|l| l.into_iter().next()) {
        let lg = chkdsk_event(&e.message);
        r["LogVerdict"] = json!(lg.verdict);
        r["LogRecords"] = json!(lg.records);
        r["LogQueued"] = json!(lg.queued);
        r["LogWhen"] = json!(local_to_utc_o(e.time_local));
    }
    r
}

/// Get-PrologueDirty.
pub fn dirty(rec: &mut Recorder) -> String {
    let r = rec.run("fsutil", &["dirty", "query", "C:"]);
    if r.error.is_some() { "unknown".into() } else { fsutil_dirty(&r.lines()).to_string() }
}

/// Invoke-PrologueScan (guardrail 1): the read-only online scan.
pub fn online_scan() -> String {
    upgrade_scan::collect::storage::online_scan().trim().to_string()
}

/// Get-PrologueChkntfs.
pub fn chkntfs(rec: &mut Recorder) -> &'static str {
    let r = rec.run("chkntfs", &["C:"]);
    if r.error.is_some() { "unknown" } else { judge::chkntfs(&r.lines()) }
}

/// Invoke-PrologueDiskpartQueryMax.
pub fn diskpart_query_max_lines(rec: &mut Recorder) -> Vec<String> {
    let script = std::env::temp_dir().join(format!("upgrade-prologue-diskpart-{}.txt", std::process::id()));
    if let Err(e) = std::fs::write(&script, "select volume C\r\nshrink querymax\r\n") {
        return vec![format!("diskpart: {e}")];
    }
    let path = script.to_string_lossy().to_string();
    let r = rec.run("diskpart.exe", &["/s", &path]).clone();
    let _ = std::fs::remove_file(&script);
    match &r.error {
        Some(e) => vec![format!("diskpart: {e}")],
        None => r.lines(),
    }
}

/// Measure-PrologueShrink: both read-only paths, errors kept verbatim.
pub fn measure_shrink(rec: &mut Recorder) -> Result<Value, String> {
    let w = storage_wmi()?;
    let part = w.query_where("MSFT_Partition", &["ObjectId", "Size"], "DriveLetter='C'")?.into_iter().next().ok_or("No MSFT_Partition objects found with property 'DriveLetter' equal to 'C'. Verify the value of the property and retry.")?;
    let vol = w.query_where("MSFT_Volume", &["SizeRemaining"], "DriveLetter='C'")?.into_iter().next().ok_or("No MSFT_Volume objects found with property 'DriveLetter' equal to 'C'. Verify the value of the property and retry.")?;
    let mut r = json!({"PartSize": int(&part["Size"]), "SizeMin": null, "ApiError": null, "DiskpartGB": null, "DiskpartError": null, "FreeBytes": int(&vol["SizeRemaining"])});
    match w.call("MSFT_Partition", &text(&part["__RELPATH"]), "GetSupportedSize", &["ReturnValue", "SizeMin", "SizeMax", "ExtendedStatus"]) {
        Ok(o) if int(&o["ReturnValue"]) == 0 => r["SizeMin"] = json!(int(&o["SizeMin"])),
        Ok(o) => r["ApiError"] = json!(upgrade_scan::collect::storage::storage_error(&o).split_whitespace().collect::<Vec<_>>().join(" ")),
        Err(e) => r["ApiError"] = json!(e.split_whitespace().collect::<Vec<_>>().join(" ")),
    }
    let dp = diskpart_query_max_lines(rec);
    match diskpart_query_max(&dp) {
        Some(g) => r["DiskpartGB"] = json!(g),
        None => {
            let kept: Vec<&String> = dp.iter().filter(|l| !l.trim().is_empty()).collect();
            r["DiskpartError"] = json!(kept[kept.len().saturating_sub(2)..].iter().map(|l| l.as_str()).collect::<Vec<_>>().join(" | "));
        }
    }
    Ok(r)
}

/// Get-PrologueLastUnmovable: Defrag 259 since a moment (local).
pub fn last_unmovable(since: Stamp) -> Option<String> {
    let secs = (now_local().seconds() - since.seconds()).max(0) + 5;
    events::query("Application", &format!("*[System[Provider[@Name='Microsoft-Windows-Defrag'] and (EventID=259) and {}]]", events::within(secs))).ok()?.into_iter().find(|e| e.time_local >= since).and_then(|e| defrag_259(&e.message))
}

/// Get-PrologueCheckOutcome: after the disk-check restart, what ran.
pub fn check_outcome(rec: &mut Recorder, since_utc: &str, wait_seconds: i64) -> Value {
    let mut r = json!({"Wininit1001": null, "Found000": false, "Dirty": "unknown", "WaitedSeconds": 0});
    let since = utc_o_to_local(since_utc).map(|t| t.seconds() - 120).unwrap_or_else(|| now_local().seconds() - 7200);
    let t0 = now_local().seconds();
    loop {
        let secs = (now_local().seconds() - since).max(0);
        if let Some(e) = events::query("Application", &format!("*[System[(EventID=1001) and {}]]", events::within(secs))).ok().and_then(|l| l.into_iter().find(|e| e.provider == "Microsoft-Windows-Wininit")) {
            let mut t = e.message.replace('\r', "");
            if t.len() > 6000 {
                t = format!("{}\n[truncated]", &t[..6000]);
            }
            r["Wininit1001"] = json!(t);
            break;
        }
        let waited = now_local().seconds() - t0;
        r["WaitedSeconds"] = json!(waited);
        if waited >= wait_seconds {
            break;
        }
        std::thread::sleep(std::time::Duration::from_secs(10));
    }
    if let Ok(d) = std::fs::read_dir("C:\\") {
        r["Found000"] = json!(d.flatten().any(|e| e.path().is_dir() && matches(r"^found\.\d{3}$", &e.file_name().to_string_lossy())));
    }
    r["Dirty"] = json!(dirty(rec));
    r
}

/// Get-FwbootmgrSnapshot.
pub fn fwbootmgr_snapshot(rec: &mut Recorder) -> Value {
    let t = rec.run("bcdedit", &["/enum", "{fwbootmgr}"]).text();
    let grab = |key: &str| capture(&format!(r"^\s*{key}\s+(.+(?:\r?\n\s{{20,}}.+)*)"), "m", &t).map(|m| m.split_whitespace().collect::<Vec<_>>().join(" ")).unwrap_or_default();
    json!({"DisplayOrder": grab("displayorder"), "BootSequence": grab("bootsequence")})
}

/// Invoke-PrologueRepairArm: schedule the rung, confirm Windows accepted it.
pub fn repair_arm(rec: &mut Recorder, method: &str) -> Result<Value, String> {
    let mut texts = Vec::new();
    let joined = |lines: Vec<String>| lines.iter().filter(|l| !l.trim().is_empty()).cloned().collect::<Vec<_>>().join(" | ");
    let ck: &str = if method == "spot-fix" {
        match storage_wmi().and_then(|w| {
            let v = w.query_where("MSFT_Volume", &["ObjectId"], "DriveLetter='C'")?.into_iter().next().ok_or("no C: volume")?;
            w.call_with("MSFT_Volume", &text(&v["__RELPATH"]), "Repair", &[("Scan", json!(false)), ("OfflineScanAndFix", json!(false)), ("SpotFix", json!(true))], &["ReturnValue", "Output", "ExtendedStatus"])
        }) {
            Ok(o) if int(&o["ReturnValue"]) == 0 => texts.push(format!("Repair-Volume -SpotFix: {}", text(&o["Output"]))),
            Ok(o) => texts.push(format!("Repair-Volume -SpotFix failed: {}", upgrade_scan::collect::storage::storage_error(&o))),
            Err(e) => texts.push(format!("Repair-Volume -SpotFix failed: {e}")),
        }
        let mut ck = chkntfs(rec);
        texts.push(format!("chkntfs after Repair-Volume: {ck}"));
        if ck != "scheduled" && ck != "dirty" {
            let out = rec.run_cmd("echo Y| chkdsk C: /spotfix").lines();
            texts.push(format!("chkdsk C: /spotfix: {}", joined(out.clone())));
            ck = judge::chkntfs(&out);
            if ck == "unknown" {
                ck = chkntfs(rec);
            }
            texts.push(format!("chkntfs after chkdsk: {ck}"));
        }
        ck
    } else if method == "chkdsk-f" {
        let out = rec.run_cmd("echo Y| chkdsk C: /f").lines();
        texts.push(format!("chkdsk C: /f: {}", joined(out.clone())));
        let mut ck = judge::chkntfs(&out);
        if ck == "unknown" {
            ck = chkntfs(rec);
        }
        texts.push(format!("chkntfs after chkdsk: {ck}"));
        ck
    } else {
        return Err(format!("no such repair method '{method}'"));
    };
    Ok(json!({"Text": texts.join("\n"), "Chkntfs": ck, "Scheduled": ck == "scheduled" || ck == "dirty"}))
}

fn c_device_id(w: &Wmi) -> Result<String, String> {
    let v = w.query_where("Win32_Volume", &["DeviceID"], "DriveLetter='C:'")?.into_iter().next().ok_or("no Win32_Volume for C:")?;
    Ok(text(&v["DeviceID"]))
}

/// Get-PrologueShadowCopyCount.
pub fn shadow_copy_count() -> Option<i64> {
    let w = Wmi::connect(CIMV2).ok()?;
    let dev = c_device_id(&w).ok()?;
    let l = w.query("Win32_ShadowCopy", &["ID", "VolumeName"]).ok()?;
    Some(l.iter().filter(|c| text(&c["VolumeName"]) == dev).count() as i64)
}

/// Invoke-PrologueDeleteRestorePoints: C: only, vssadmin then WMI one by one.
pub fn delete_restore_points(rec: &mut Recorder) -> Value {
    let before = shadow_copy_count();
    let r = rec.run("vssadmin", &["delete", "shadows", "/for=C:", "/all", "/quiet"]).clone();
    let (text_out, code) = match &r.error {
        Some(e) => (format!("vssadmin raised: {e}"), Value::Null),
        None => (r.text().trim().to_string(), json!(r.exit)),
    };
    let after_vss = shadow_copy_count();
    let mut wmi_lines = Vec::new();
    if ["deleted-none", "deleted-some", "unknown"].contains(&judge::restore_point_verdict(before, after_vss)) {
        match Wmi::connect(CIMV2).and_then(|w| {
            let dev = c_device_id(&w)?;
            let l = w.query("Win32_ShadowCopy", &["ID", "VolumeName"])?;
            Ok((w, l.into_iter().filter(|c| text(&c["VolumeName"]) == dev).collect::<Vec<_>>()))
        }) {
            Ok((w, list)) => {
                for sc in list {
                    match w.delete_instance(&text(&sc["__RELPATH"])) {
                        Ok(()) => wmi_lines.push(format!("{}: removed", text(&sc["ID"]))),
                        Err(e) => wmi_lines.push(format!("{}: {e}", text(&sc["ID"]))),
                    }
                }
            }
            Err(e) => wmi_lines.push(format!("listing shadow copies failed: {e}")),
        }
    }
    let after = shadow_copy_count();
    let deleted = match (before, after) {
        (Some(b), Some(a)) => (b - a).max(0),
        _ => 0,
    };
    json!({"Before": before, "AfterVssadmin": after_vss, "After": after, "Deleted": deleted, "Verdict": judge::restore_point_verdict(before, after), "VssadminExit": code, "Text": text_out, "Wmi": wmi_lines, "Utc": now_z()})
}

/// Invoke-PrologueDeleteUsnJournal.
pub fn delete_usn_journal(rec: &mut Recorder, st: &mut Value) -> Value {
    let before = judge::usn_query(&rec.run("fsutil", &["usn", "queryjournal", "C:"]).lines());
    let r = rec.run("fsutil", &["usn", "deletejournal", "/n", "C:"]).clone();
    let code = r.exit;
    let mut j = st["Shrink"]["UsnJournal"].clone();
    if !truthy(&j) {
        j = json!({"Before": before, "Deletions": 0, "LastRestarts": null, "Recreated": false, "ExitCode": null, "Text": null, "Utc": null});
    } else if !(truthy(&j["Before"]) && truthy(&j["Before"]["Active"])) && truthy(&before["Active"]) {
        j["Before"] = before;
    }
    if code == Some(0) {
        j["Deletions"] = json!(int(&j["Deletions"]) + 1);
        j["Recreated"] = json!(false);
    }
    j["LastRestarts"] = json!(int(&st["Restarts"]));
    j["ExitCode"] = json!(code);
    j["Text"] = json!(r.text().trim());
    j["Utc"] = json!(now_z());
    st["Shrink"]["UsnJournal"] = j.clone();
    j
}

/// Invoke-PrologueRecreateUsnJournal.
pub fn recreate_usn_journal(rec: &mut Recorder, st: &mut Value) -> String {
    let b = st["Shrink"]["UsnJournal"]["Before"].clone();
    let (m, a) = (int(&b["MaxBytes"]), int(&b["DeltaBytes"]));
    let r = rec.run("fsutil", &["usn", "createjournal", &format!("m={m}"), &format!("a={a}"), "C:"]).clone();
    if r.exit == Some(0) {
        st["Shrink"]["UsnJournal"]["Recreated"] = json!(true);
        return format!("change journal created again ({} MB; its record of earlier changes is gone)", upgrade_scan::ps::num(upgrade_scan::ps::round1(m as f64 / 1048576.0)));
    }
    format!("! the change journal could not be created again (fsutil exit {}; Windows creates it when a program next needs it)", r.exit.map_or("none".to_string(), |c| c.to_string()))
}

/// Get-PrologueUpdateFacts (RISKS R25), read-only.
pub fn update_facts() -> Value {
    const CBS: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Component Based Servicing";
    json!({"Utc": now_z(), "CbsRebootPending": registry::key_exists(&Hive::LocalMachine, &format!("{CBS}\\RebootPending")), "CbsRebootInProgress": registry::key_exists(&Hive::LocalMachine, &format!("{CBS}\\RebootInProgress")),
           "WuRebootRequired": registry::key_exists(&Hive::LocalMachine, r"SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\Auto Update\RebootRequired")})
}

/// Get-PrologueMemoryFilesBefore, read-only.
pub fn memory_files_before() -> Value {
    let mut b = json!({"HibernateEnabled": null, "AutoPagefile": null, "PagefileSettings": []});
    if let Some(h) = registry::dword(&Hive::LocalMachine, r"SYSTEM\CurrentControlSet\Control\Power", "HibernateEnabled") {
        b["HibernateEnabled"] = json!(h != 0);
    }
    if let Ok(w) = Wmi::connect(CIMV2) {
        if let Some(cs) = w.query("Win32_ComputerSystem", &["AutomaticManagedPagefile"]).ok().and_then(|l| l.into_iter().next()) {
            b["AutoPagefile"] = json!(truthy(&cs["AutomaticManagedPagefile"]));
        }
        if let Ok(l) = w.query("Win32_PageFileSetting", &["Name", "InitialSize", "MaximumSize"]) {
            b["PagefileSettings"] = Value::Array(l.iter().map(|p| json!({"Name": text(&p["Name"]), "InitialSize": int(&p["InitialSize"]), "MaximumSize": int(&p["MaximumSize"])})).collect());
        }
    }
    b
}

/// Invoke-PrologueHibernationOff.
pub fn hibernation_off(rec: &mut Recorder) -> bool {
    let _ = rec.run("powercfg", &["/h", "off"]);
    !Path::new("C:\\hiberfil.sys").exists()
}

/// Invoke-ProloguePagefileOff (takes effect at the next restart).
pub fn pagefile_off() -> bool {
    let Ok(w) = Wmi::connect(CIMV2) else { return false };
    let Some(cs) = w.query("Win32_ComputerSystem", &["AutomaticManagedPagefile"]).ok().and_then(|l| l.into_iter().next()) else { return false };
    if truthy(&cs["AutomaticManagedPagefile"]) && w.put_property(&text(&cs["__RELPATH"]), "AutomaticManagedPagefile", &json!(false)).is_err() {
        return false;
    }
    if let Ok(l) = w.query("Win32_PageFileSetting", &["Name"]) {
        for p in l {
            let _ = w.delete_instance(&text(&p["__RELPATH"]));
        }
    }
    true
}

/// Invoke-PrologueRestoreMemoryFiles: act on the plan; what was done, in words.
pub fn restore_memory_files(rec: &mut Recorder, st: &mut Value, keep_hibernation_off: bool) -> Vec<String> {
    let mut done = Vec::new();
    let plan = judge::restore_plan(&st["Shrink"], keep_hibernation_off);
    for a in plan {
        match a {
            "hibernation-on" => {
                let r = rec.run("powercfg", &["/h", "on"]).clone();
                if r.exit == Some(0) {
                    st["Shrink"]["HibernationDisabled"] = json!(false);
                    done.push("hibernation back on".to_string());
                } else {
                    done.push(format!("! hibernation could not be turned back on (powercfg exit {})", r.exit.map_or("none".to_string(), |c| c.to_string())));
                }
            }
            "usn-journal" => done.push(recreate_usn_journal(rec, st)),
            "hibernation-unknown" => done.push("! hibernation left off: no record of how it was set (powercfg /h on turns it back on)".to_string()),
            "pagefile-auto" => match Wmi::connect(CIMV2).and_then(|w| {
                let cs = w.query("Win32_ComputerSystem", &["AutomaticManagedPagefile"])?.into_iter().next().ok_or("no Win32_ComputerSystem")?;
                w.put_property(&text(&cs["__RELPATH"]), "AutomaticManagedPagefile", &json!(true))
            }) {
                Ok(()) => {
                    st["Shrink"]["PagefileDisabled"] = json!(false);
                    done.push("pagefile back to automatic (returns at the next restart)".to_string());
                }
                Err(e) => done.push(format!("! pagefile-auto failed: {e}")),
            },
            "pagefile-settings" => {
                let settings: Vec<Value> = items(&st["Shrink"]["Before"]["PagefileSettings"]).into_iter().cloned().collect();
                match Wmi::connect(CIMV2).and_then(|w| {
                    for p in &settings {
                        w.create_instance("Win32_PageFileSetting", &[("Name", json!(text(&p["Name"]))), ("InitialSize", json!(int(&p["InitialSize"]))), ("MaximumSize", json!(int(&p["MaximumSize"])))])?;
                    }
                    Ok(())
                }) {
                    Ok(()) => {
                        st["Shrink"]["PagefileDisabled"] = json!(false);
                        done.push(format!("pagefile settings put back ({}; returns at the next restart)", settings.len()));
                    }
                    Err(e) => done.push(format!("! pagefile-settings failed: {e}")),
                }
            }
            _ => {}
        }
    }
    st["Shrink"]["Restored"] = json!(done);
    done
}

fn c_partition(w: &Wmi) -> Result<Value, String> {
    w.query_where("MSFT_Partition", &["ObjectId", "PartitionNumber", "Guid", "Size"], "DriveLetter='C'")?.into_iter().next().ok_or_else(|| "No MSFT_Partition objects found with property 'DriveLetter' equal to 'C'. Verify the value of the property and retry.".to_string())
}

/// Invoke-PrologueShrink: Resize-Partition C: by the planned amount.
pub fn shrink(requested_bytes: i64) -> Result<Value, String> {
    let w = storage_wmi()?;
    let p = c_partition(&w)?;
    let size_before = int(&p["Size"]);
    let target = size_before - requested_bytes;
    let r = w.call_with("MSFT_Partition", &text(&p["__RELPATH"]), "Resize", &[("Size", json!(target))], &["ReturnValue", "ExtendedStatus"])?;
    if int(&r["ReturnValue"]) != 0 {
        return Err(upgrade_scan::collect::storage::storage_error(&r));
    }
    let after = int(&c_partition(&w)?["Size"]);
    Ok(json!({"SizeBefore": size_before, "SizeAfter": after, "Freed": size_before - after}))
}

/// Invoke-PrologueGrowBack.
pub fn grow_back(size_before: i64) -> bool {
    storage_wmi().and_then(|w| {
        let p = c_partition(&w)?;
        let r = w.call_with("MSFT_Partition", &text(&p["__RELPATH"]), "Resize", &[("Size", json!(size_before))], &["ReturnValue"])?;
        if int(&r["ReturnValue"]) == 0 { Ok(()) } else { Err("refused".into()) }
    })
    .is_ok()
}

/// The partition C: lives on, as outcome.json's `windows.partition`.
pub fn windows_partition() -> Option<Value> {
    let w = storage_wmi().ok()?;
    let p = c_partition(&w).ok()?;
    Some(json!({"number": int(&p["PartitionNumber"]), "guid": text(&p["Guid"]), "size_bytes": int(&p["Size"])}))
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut h = Sha256::new();
    std::io::copy(&mut f, &mut h).map_err(|e| e.to_string())?;
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn walk_files(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk_files(&p, out);
            } else {
                out.push(p);
            }
        }
    }
}

/// Invoke-PrologueStage (clean slate): the folders to the stick with
/// per-file checksums, at a write speed measured now. `log` takes the
/// lines the script would log.
pub fn stage(job: &Value, root: &str, log: &mut dyn FnMut(String)) -> Value {
    let dir = Path::new(root).join("upgrade_").join("staging");
    let _ = std::fs::create_dir_all(&dir);
    let folders: Vec<&Value> = items(at(job, "harvest.folders")).into_iter().filter(|f| truthy(at(f, "exists")) && truthy(at(f, "path"))).collect();
    let total: i64 = folders.iter().map(|f| int(at(f, "bytes"))).sum();
    // measure, then estimate - never the other way round
    let probe = dir.join(".probe");
    let mut buf = vec![0u8; STAGE_PROBE_BYTES];
    let _ = crate::random::fill(&mut buf);
    let t0 = std::time::Instant::now();
    let wrote = std::fs::File::create(&probe).and_then(|mut f| {
        use std::io::Write;
        f.write_all(&buf)?;
        f.sync_all()
    });
    let secs = t0.elapsed().as_secs_f64().max(0.001);
    let _ = std::fs::remove_file(&probe);
    let mbps = if wrote.is_ok() { upgrade_scan::ps::round1(STAGE_PROBE_BYTES as f64 / 1e6 / secs) } else { 0.0 };
    let est = judge::time_estimate(total, Some(mbps));
    log(format!("  staging {} folder(s), {} GB, stick writes at {} MB/s: {}", folders.len(), upgrade_scan::ps::fmt_n(total as f64 / GB, 2), upgrade_scan::ps::num(mbps), judge::duration(est)));
    let free = storage_wmi().ok().and_then(|w| w.query_where("MSFT_Volume", &["SizeRemaining"], &format!("DriveLetter='{}'", &root[..1])).ok()).and_then(|l| l.into_iter().next()).map_or(0, |v| int(&v["SizeRemaining"]));
    if total as f64 * 1.02 + 64.0 * 1048576.0 > free as f64 {
        return json!({"Files": 0, "Bytes": 0, "Failed": 0, "WriteMbps": mbps, "EstimatedSeconds": est, "Manifest": "upgrade_/staging/SHA256SUMS", "Error": format!("the files ({total} bytes) do not fit the stick's free space ({free} bytes)")});
    }
    let (mut lines, mut files, mut bytes, mut failed) = (Vec::new(), 0i64, 0i64, 0i64);
    for fo in &folders {
        let src = PathBuf::from(text(at(fo, "path")));
        let name = text(at(fo, "name"));
        let dst = dir.join(&name);
        let mut all = Vec::new();
        walk_files(&src, &mut all);
        for fi in all {
            let rel = fi.strip_prefix(&src).map(|r| r.to_string_lossy().to_string()).unwrap_or_default();
            let to = dst.join(&rel);
            let mut copy = || -> Result<(), String> {
                if let Some(parent) = to.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                std::fs::copy(&fi, &to).map_err(|e| e.to_string())?;
                let h = sha256_file(&to)?;
                lines.push(format!("{h}  ./staging/{name}/{}", rel.replace('\\', "/")));
                Ok(())
            };
            match copy() {
                Ok(()) => {
                    files += 1;
                    bytes += std::fs::metadata(&fi).map(|m| m.len() as i64).unwrap_or(0);
                }
                Err(e) => {
                    failed += 1;
                    log(format!("  ! could not stage {}: {e}", fi.display()));
                }
            }
        }
    }
    let manifest = if lines.is_empty() { String::new() } else { lines.join("\n") + "\n" };
    let _ = std::fs::write(dir.join("SHA256SUMS"), manifest);
    json!({"Files": files, "Bytes": bytes, "Failed": failed, "WriteMbps": mbps, "EstimatedSeconds": est, "Manifest": "upgrade_/staging/SHA256SUMS", "Error": null})
}

/// Reset-GrubEnv.
pub fn reset_grub_env(root: &str) -> bool {
    let env = Path::new(root).join(GRUB_ENV_REL);
    if env.exists() || Path::new(root).join("EFI\\BOOT\\grubx64.efi").exists() {
        return std::fs::write(&env, judge::grub_env_block()).is_ok();
    }
    false
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The SYSTEM startup task's XML: runs before and without a sign-in.
pub fn resume_task_xml(exe: &str, state: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo><Description>upgrade_: continue the conversion after a restart (removed by the prologue itself)</Description></RegistrationInfo>
  <Triggers><BootTrigger><Enabled>true</Enabled></BootTrigger></Triggers>
  <Principals><Principal id="Author"><UserId>S-1-5-18</UserId><RunLevel>HighestAvailable</RunLevel></Principal></Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <StartWhenAvailable>true</StartWhenAvailable>
    <ExecutionTimeLimit>PT4H</ExecutionTimeLimit>
    <Enabled>true</Enabled>
  </Settings>
  <Actions Context="Author"><Exec><Command>{e}</Command><Arguments>resume --state-dir "{s}"</Arguments></Exec></Actions>
</Task>
"#,
        e = xml_escape(exe),
        s = xml_escape(state)
    )
}

/// Protect-StateDir: only SYSTEM and Administrators may write; Users keep
/// read (for the notice). What the directory grants, read back, as evidence.
pub fn protect_state_dir(rec: &mut Recorder, state: &str) -> Result<Vec<String>, String> {
    let r = rec.run("icacls", &[state, "/inheritance:r", "/grant:r", "NT AUTHORITY\\SYSTEM:(OI)(CI)F", "BUILTIN\\Administrators:(OI)(CI)F", "BUILTIN\\Users:(OI)(CI)RX"]).clone();
    if !r.ok() {
        return Err(format!("icacls could not set the state directory's access ({})", r.text().trim()));
    }
    let back = rec.run("icacls", &[state]).clone();
    let grants: Vec<String> = back.lines().iter().filter(|l| l.contains(':') && !l.starts_with("Successfully")).map(|l| l.trim().trim_start_matches(state).trim().to_string()).filter(|l| !l.is_empty()).collect();
    let loose: Vec<&String> = grants.iter().filter(|g| matches("Users|Everyone|Authenticated", g) && matches(r"\((?:OI\)|CI\))*\(?(F|M|W|WD)\)", g)).collect();
    if !loose.is_empty() {
        return Err(format!("the state directory still grants write access to {}; refusing to register a SYSTEM task over it", loose.iter().map(|x| x.as_str()).collect::<Vec<_>>().join(", ")));
    }
    Ok(grants)
}

/// Register-ResumeTask: this program's copy in the state directory, run by
/// SYSTEM at startup. Returns the directory's access list as evidence.
pub fn register_resume_task(rec: &mut Recorder, state: &str) -> Result<Vec<String>, String> {
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let copy = Path::new(state).join("upgrade-prologue.exe");
    if !same_file(&me, &copy) {
        std::fs::copy(&me, &copy).map_err(|e| format!("copying the program to the state directory: {e}"))?;
    }
    let acl = protect_state_dir(rec, state)?;
    let xml_path = Path::new(state).join("resume-task.xml");
    let xml = resume_task_xml(&copy.to_string_lossy(), state);
    let mut bytes = vec![0xFF, 0xFE];
    for u in xml.encode_utf16() {
        bytes.extend_from_slice(&u.to_le_bytes());
    }
    std::fs::write(&xml_path, bytes).map_err(|e| e.to_string())?;
    let xp = xml_path.to_string_lossy().to_string();
    let r = rec.run("schtasks", &["/Create", "/TN", TASK_NAME, "/XML", &xp, "/F"]).clone();
    if !r.ok() {
        return Err(format!("schtasks could not register the resume task ({})", r.text().trim()));
    }
    let q = rec.run("schtasks", &["/Query", "/TN", TASK_NAME, "/XML"]).clone();
    if !q.ok() {
        return Err("the resume task is not present after registration".to_string());
    }
    if !matches("<UserId>(S-1-5-18|NT AUTHORITY\\\\SYSTEM|SYSTEM)</UserId>", &q.text()) {
        unregister_resume_task(rec);
        return Err("the resume task did not register as SYSTEM; removed again".to_string());
    }
    Ok(acl)
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x.to_string_lossy().to_lowercase() == y.to_string_lossy().to_lowercase(),
        _ => false,
    }
}

/// Unregister-ResumeTask: true when a task was there and went.
pub fn unregister_resume_task(rec: &mut Recorder) -> bool {
    if !rec.run("schtasks", &["/Query", "/TN", TASK_NAME]).ok() {
        return false;
    }
    rec.run("schtasks", &["/Delete", "/TN", TASK_NAME, "/F"]).ok()
}

/// Get-LiveResumeContext: who runs this, in which session, with a screen or not.
pub fn live_resume_context() -> Value {
    use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
    use windows::Win32::System::StationsAndDesktops::{GetProcessWindowStation, GetUserObjectInformationW, UOI_FLAGS, USEROBJECTFLAGS};
    use windows::Win32::System::Threading::GetCurrentProcessId;
    let mut session: u32 = 0;
    let _ = unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut session) };
    let mut flags = USEROBJECTFLAGS::default();
    let mut needed: u32 = 0;
    let interactive = unsafe {
        match GetProcessWindowStation() {
            Ok(h) => GetUserObjectInformationW(windows::Win32::Foundation::HANDLE(h.0), UOI_FLAGS, Some(&mut flags as *mut _ as *mut _), std::mem::size_of::<USEROBJECTFLAGS>() as u32, Some(&mut needed)).is_ok() && (flags.dwFlags & 1) != 0,
            Err(_) => false,
        }
    };
    let explorer = Wmi::connect(CIMV2).and_then(|w| w.query_where("Win32_Process", &["ProcessId"], "Name='explorer.exe'")).map(|l| !l.is_empty()).unwrap_or(false);
    judge::resume_context(&user_name(), interactive, session as i64, explorer)
}

/// `[Security.Principal.WindowsIdentity]::GetCurrent().Name`, as the script
/// reads it. The first rig run (2026-10-07) used `GetUserNameExW`, which names
/// SYSTEM as the machine account, so the record said `run_as: user` for a
/// resume that ran as SYSTEM in session 0.
fn user_name() -> String {
    upgrade_scan::collect::win::account_name()
}

/// Seconds since the machine started.
pub fn uptime_seconds() -> i64 {
    (unsafe { windows::Win32::System::SystemInformation::GetTickCount64() } / 1000) as i64
}

/// Every volume with a letter: `{DriveLetter, UniqueId}`.
pub fn volumes() -> Value {
    let list = storage_wmi().and_then(|w| w.query("MSFT_Volume", &["DriveLetter", "UniqueId"])).unwrap_or_default();
    Value::Array(list.into_iter().map(|v| {
        let letter = match &v["DriveLetter"] {
            Value::String(t) if !t.trim_matches('\0').is_empty() => json!(t.trim_matches('\0')),
            Value::Number(n) => n.as_u64().filter(|c| *c > 0).and_then(|c| char::from_u32(c as u32)).map_or(Value::Null, |c| json!(c.to_string())),
            _ => Value::Null,
        };
        json!({"DriveLetter": letter, "UniqueId": v["UniqueId"]})
    }).collect())
}

/// Find-Stick: by volume id, else the root recorded at the start if the job is still there.
pub fn find_stick(st: &Value) -> Option<String> {
    let id = text(&st["StickUniqueId"]);
    if !id.is_empty() {
        if let Some(r) = judge::find_stick_root(&volumes(), &id) {
            return Some(r);
        }
    }
    let root = text(&st["StickRootAtStart"]);
    if !root.is_empty() && Path::new(&root).join("upgrade_").join("job.json").exists() {
        return Some(root);
    }
    None
}

/// Wait-Stick: at startup the stick may not be enumerated yet.
pub fn wait_stick(st: &Value, seconds: i64) -> Option<String> {
    let t0 = now_local().seconds();
    loop {
        if let Some(r) = find_stick(st) {
            return Some(r);
        }
        if now_local().seconds() - t0 >= seconds {
            return None;
        }
        std::thread::sleep(std::time::Duration::from_secs(5));
    }
}

/// The RunOnce command: this program's copy in the state directory, `notify`.
pub fn notice_command_exe(state: &str) -> String {
    format!("\"{}\\upgrade-prologue.exe\" notify --state-dir \"{state}\"", state.trim_end_matches('\\'))
}

/// Set-Notice: queue text for the person's next sign-in.
pub fn set_notice(state: &str, title: &str, text_: &str, buttons: i64) -> bool {
    let n = json!({"title": title, "text": text_, "buttons": buttons, "queued_utc": now_o()});
    if std::fs::write(Path::new(state).join("notice.json"), serde_json::to_string_pretty(&n).unwrap_or_default()).is_err() {
        return false;
    }
    registry::set_string(&Hive::LocalMachine, RUNONCE, NOTICE_RUNONCE_NAME, &notice_command_exe(state)).is_ok()
}

pub fn clear_notice_runonce() {
    let _ = registry::delete_value(&Hive::LocalMachine, RUNONCE, NOTICE_RUNONCE_NAME);
}

#[link(name = "user32")]
unsafe extern "system" {
    fn MessageBoxTimeoutW(hwnd: isize, text: *const u16, caption: *const u16, utype: u32, wlang: u16, milliseconds: u32) -> i32;
}

/// Show-Popup: a message box with a timeout (6 = Yes, 7 = No, 1 = OK,
/// 32000 = timed out, as `WScript.Shell.Popup` gives -1).
pub fn show_popup(text_: &str, title: &str, seconds: i64, buttons: i64) -> i64 {
    let t: Vec<u16> = text_.encode_utf16().chain(std::iter::once(0)).collect();
    let c: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
    // MB_SETFOREGROUND | MB_TOPMOST
    let r = unsafe { MessageBoxTimeoutW(0, t.as_ptr(), c.as_ptr(), buttons as u32 | 0x00010000 | 0x00040000, 0, (seconds.max(1) * 1000) as u32) };
    if r == 32000 { -1 } else { r as i64 }
}

/// Show-Or-Queue: a popup when there is a screen, a queued notice when not.
pub fn show_or_queue(state: &str, title: &str, text_: &str, seconds: i64, buttons: i64) -> &'static str {
    if truthy(&live_resume_context()["Unattended"]) {
        set_notice(state, title, text_, buttons);
        return "queued";
    }
    show_popup(text_, title, seconds, buttons);
    "shown"
}

/// Restart-Machine: in 15 s, with the reason on the screen.
pub fn restart_machine(rec: &mut Recorder, why: &str) {
    let _ = rec.run("shutdown", &["/r", "/t", "15", "/c", &format!("upgrade_: {why}. Leave the USB stick in and do not interrupt.")]);
}

/// Remove-PrologueWifiSecrets: how many files went.
pub fn remove_wifi_secrets(root: &str) -> i64 {
    if root.is_empty() {
        return 0;
    }
    let dir = Path::new(root).join("upgrade_").join("artifacts").join("credentials").join("wifi");
    if !dir.exists() {
        return 0;
    }
    let mut files = Vec::new();
    walk_files(&dir, &mut files);
    let mut n = 0;
    for f in files {
        let _ = std::fs::write(&f, "SCRUBBED by the prologue");
        if std::fs::remove_file(&f).is_ok() {
            n += 1;
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    n
}

/// Get-PrologueFacts: what the live machine says, in the script's names.
pub fn facts(rec: &mut Recorder, root: &str) -> Result<Value, String> {
    let cimv2 = Wmi::connect(CIMV2).ok();
    let one = |class: &str, props: &[&str]| cimv2.as_ref().and_then(|w| w.query(class, props).ok()).and_then(|l| l.into_iter().next()).unwrap_or(Value::Null);
    let cs = one("Win32_ComputerSystem", &["Manufacturer", "Model"]);
    let os = one("Win32_OperatingSystem", &["Caption", "BuildNumber"]);
    let bios = one("Win32_BIOS", &["SerialNumber", "SMBIOSBIOSVersion"]);
    let sys = one("Win32_ComputerSystemProduct", &["UUID"]);
    let mut f = json!({"Vendor": text(&cs["Manufacturer"]), "Model": text(&cs["Model"]), "Uuid": text(&sys["UUID"]), "BiosSerial": text(&bios["SerialNumber"]),
                       "OsCaption": text(&os["Caption"]), "OsBuild": int(&os["BuildNumber"]), "BiosVersion": text(&bios["SMBIOSBIOSVersion"]),
                       "Firmware": std::env::var("firmware_type").unwrap_or_default(), "SecureBoot": secure_boot()});
    let w = storage_wmi()?;
    let part = w.query_where("MSFT_Partition", &["DiskNumber", "PartitionNumber", "Guid", "Size"], "DriveLetter='C'")?.into_iter().next().ok_or("No MSFT_Partition objects found with property 'DriveLetter' equal to 'C'. Verify the value of the property and retry.")?;
    let n = int(&part["DiskNumber"]);
    let disk = w.query_where("MSFT_Disk", &["Number", "SerialNumber", "UniqueId", "Size"], &format!("Number={n}"))?.into_iter().next().ok_or_else(|| format!("No MSFT_Disk objects found with property 'Number' equal to '{n}'. Verify the value of the property and retry."))?;
    f["Disk"] = json!({"Number": n, "Serial": text(&disk["SerialNumber"]).split_whitespace().collect::<String>(), "UniqueId": text(&disk["UniqueId"]), "Size": int(&disk["Size"])});
    f["Partition"] = json!({"number": int(&part["PartitionNumber"]), "guid": text(&part["Guid"]), "size_bytes": int(&part["Size"])});
    f["Health"] = json!(disk_health(n, &text(&disk["UniqueId"])));
    f["Dirty"] = json!(dirty(rec));
    let ev0 = volume_evidence(collect::utc_to_local(collect::utc_from_seconds(collect::now().0.seconds() - 30 * DAY)));
    let rq = judge::repair_queued(&text(&ev0["VolumeStatus"]), &text(&ev0["NtfsFullChkdsk"]), &text(&ev0["LastCheck"]));
    f["RepairQueued"] = json!(truthy(&rq["Queued"]));
    f["RepairQueuedWhy"] = rq["Why"].clone();
    f["RepairStale"] = rq["Stale"].clone();
    let blq = bitlocker_state(rec);
    f["BitLocker"] = blq["State"].clone();
    f["BitLockerSource"] = blq["Source"].clone();
    f["BitLockerRaw"] = blq["Raw"].clone();
    f["Stick"] = Value::Null;
    f["StickError"] = Value::Null;
    let letter = &root[..1];
    let stick = (|| -> Result<Value, String> {
        let sv = w.query_where("MSFT_Volume", &["UniqueId", "SizeRemaining"], &format!("DriveLetter='{letter}'"))?.into_iter().next().ok_or_else(|| format!("No MSFT_Volume objects found with property 'DriveLetter' equal to '{letter}'. Verify the value of the property and retry."))?;
        let sp = w.query_where("MSFT_Partition", &["DiskNumber"], &format!("DriveLetter='{letter}'"))?.into_iter().next().ok_or_else(|| format!("No MSFT_Partition objects found with property 'DriveLetter' equal to '{letter}'. Verify the value of the property and retry."))?;
        let dn = int(&sp["DiskNumber"]);
        let sd = w.query_where("MSFT_Disk", &["UniqueId", "Size", "BusType"], &format!("Number={dn}"))?.into_iter().next().ok_or_else(|| format!("No MSFT_Disk objects found with property 'Number' equal to '{dn}'. Verify the value of the property and retry."))?;
        Ok(json!({"UniqueId": text(&sd["UniqueId"]), "Size": int(&sd["Size"]), "Bus": bus_type(&sd["BusType"]), "VolumeId": text(&sv["UniqueId"]), "Free": int(&sv["SizeRemaining"])}))
    })();
    match stick {
        Ok(v) => f["Stick"] = v,
        Err(e) => f["StickError"] = json!(e),
    }
    f["AllDisks"] = Value::Array(w.query("MSFT_Disk", &["Number", "UniqueId", "Size", "BusType"]).unwrap_or_default().iter().map(|d| json!({"Number": int(&d["Number"]), "UniqueId": text(&d["UniqueId"]), "Size": int(&d["Size"]), "Bus": bus_type(&d["BusType"])})).collect());
    f["Hiberfil"] = json!(Path::new("C:\\hiberfil.sys").exists());
    f["Pagefile"] = json!(Path::new("C:\\pagefile.sys").exists());
    Ok(f)
}
