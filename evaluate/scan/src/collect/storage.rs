//! The storage reads: the disks and the room to shrink (Get-UpgDiskFacts),
//! the drive that holds C: (Get-UpgPhysicalDiskFacts), the volume's health
//! (Get-UpgVolumeHealth), BitLocker (Get-UpgBitLockerState) and the boot
//! partition (Get-UpgEspFacts). The Storage cmdlets read the same WMI
//! classes under `ROOT\Microsoft\Windows\Storage`; the names they print for
//! the numbers are given here.

use super::events::{self, within};
use super::win::{now_local, run_tool, wide};
use super::wmi::{wmi_error, Wmi, BITLOCKER, CIMV2, STORAGE, WMI_ROOT};
use crate::facts::DiskEvent;
use crate::parse::{chkdsk_event, defrag_259, disk_events, diskpart_query_max, fsutil_dirty, smart_attributes};
use crate::ps::{matches, round1, Stamp};
use serde_json::{json, Value};

const GB: f64 = 1073741824.0;
const DAY: i64 = 86400;

pub fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

pub fn int(v: &Value) -> Option<i64> {
    num(v).map(|f| f as i64)
}

/// A number as JSON: whole numbers stay whole (`9028`, not `9028.0`), as
/// PowerShell writes them.
pub fn count(v: &Value) -> Value {
    match num(v) {
        Some(f) if f.fract() == 0.0 && f.abs() < 9.0e15 => json!(f as i64),
        Some(f) => json!(f),
        None => Value::Null,
    }
}

pub fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// MSFT_Disk / MSFT_PhysicalDisk BusType, as the cmdlets print it.
pub fn bus_type(v: &Value) -> String {
    match int(v) {
        Some(0) => "Unknown",
        Some(1) => "SCSI",
        Some(2) => "ATAPI",
        Some(3) => "ATA",
        Some(4) => "1394",
        Some(5) => "SSA",
        Some(6) => "Fibre Channel",
        Some(7) => "USB",
        Some(8) => "RAID",
        Some(9) => "iSCSI",
        Some(10) => "SAS",
        Some(11) => "SATA",
        Some(12) => "SD",
        Some(13) => "MMC",
        Some(14) => "Virtual",
        Some(15) => "File Backed Virtual",
        Some(16) => "Storage Spaces",
        Some(17) => "NVMe",
        Some(18) => "SCM",
        Some(19) => "UFS",
        _ => "Unknown",
    }
    .to_string()
}

pub fn partition_style(v: &Value) -> String {
    match int(v) {
        Some(1) => "MBR",
        Some(2) => "GPT",
        _ => "Unknown",
    }
    .to_string()
}

pub fn health_status(v: &Value) -> String {
    match int(v) {
        Some(0) => "Healthy",
        Some(1) => "Warning",
        Some(2) => "Unhealthy",
        _ => "Unknown",
    }
    .to_string()
}

pub fn media_type(v: &Value) -> String {
    match int(v) {
        Some(3) => "HDD",
        Some(4) => "SSD",
        Some(5) => "SCM",
        _ => "Unspecified",
    }
    .to_string()
}

/// OperationalStatus (an array of codes) as the cmdlets print it, joined
/// with commas as the PowerShell collector joins it.
pub fn operational_status(v: &Value, volume: bool) -> String {
    let name = |code: i64| -> String {
        match (code, volume) {
            (0, _) => "Unknown".into(),
            (1, _) => "Other".into(),
            (2, _) => "OK".into(),
            (3, _) => "Degraded".into(),
            (4, _) => "Stressed".into(),
            (5, _) => "Predictive Failure".into(),
            (6, _) => "Error".into(),
            (7, _) => "Non-Recoverable Error".into(),
            (8, _) => "Starting".into(),
            (9, _) => "Stopping".into(),
            (10, _) => "Stopped".into(),
            (11, _) => "In Service".into(),
            (12, _) => "No Contact".into(),
            (13, _) => "Lost Communication".into(),
            (14, true) => "Scan Needed".into(),
            (15, true) => "Spot Fix Needed".into(),
            (16, true) => "Full Repair Needed".into(),
            (14, false) => "Aborted".into(),
            (15, false) => "Dormant".into(),
            (16, false) => "Supporting Entity in Error".into(),
            (17, _) => "Completed".into(),
            (0xD002, false) => "Failed Media".into(),
            (0xD003, false) => "Split".into(),
            (0xD004, false) => "Stale Metadata".into(),
            (0xD005, false) => "IO Error".into(),
            (0xD006, false) => "Unrecognized Metadata".into(),
            (0xD007, false) => "Removing From Pool".into(),
            (0xD008, false) => "In Maintenance Mode".into(),
            (0xD009, false) => "Updating Firmware".into(),
            (0xD00A, false) => "Device Hardware Error".into(),
            (0xD00B, false) => "Not Usable".into(),
            (0xD00C, false) => "Transient Error".into(),
            (0xD00D, false) => "Starting Maintenance Mode".into(),
            (0xD00E, false) => "Stopping Maintenance Mode".into(),
            (c, _) => c.to_string(),
        }
    };
    match v {
        Value::Array(l) => l.iter().filter_map(int).map(name).collect::<Vec<_>>().join(","),
        other => int(other).map(name).unwrap_or_default(),
    }
}

/// The words the Storage cmdlets use for a method's return value.
pub fn storage_error(result: &Value) -> String {
    if let Some(msg) = result.get("ExtendedStatus").and_then(|e| e.get("Message")).and_then(Value::as_str) {
        return msg.to_string();
    }
    match int(&result["ReturnValue"]) {
        Some(1) => "Not Supported".into(),
        Some(2) => "Unspecified Error".into(),
        Some(3) => "Timeout".into(),
        Some(4) => "Failed".into(),
        Some(5) => "Invalid Parameter".into(),
        Some(40001) => "Access denied".into(),
        Some(40002) => "There are not enough resources to complete the operation.".into(),
        Some(42002) => "The requested object was not found.".into(),
        Some(c) => format!("the Storage API answered {c}"),
        None => "the Storage API answered nothing".into(),
    }
}

pub fn storage_wmi() -> Result<Wmi, String> {
    Wmi::connect(STORAGE)
}

/// Get-UpgDiskFacts.
pub fn disk_facts(is_admin: bool) -> Value {
    let storage = storage_wmi();
    let measure_start = now_local();
    let disks: Vec<Value> = storage
        .as_ref()
        .ok()
        .and_then(|w| w.query("MSFT_Disk", &["Number", "FriendlyName", "Size", "PartitionStyle", "BusType"]).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|d| bus_type(&d["BusType"]) != "File Backed Virtual")
        .map(|d| json!({"Number": int(&d["Number"]), "FriendlyName": d["FriendlyName"], "Size": count(&d["Size"]), "PartitionStyle": partition_style(&d["PartitionStyle"]), "BusType": bus_type(&d["BusType"])}))
        .collect();
    let volume = storage.as_ref().ok().and_then(|w| w.query_where("MSFT_Volume", &["Size", "SizeRemaining"], "DriveLetter='C'").ok()).and_then(|l| l.into_iter().next());
    let sys_volume = volume.map(|v| json!({"Size": count(&v["Size"]), "SizeRemaining": count(&v["SizeRemaining"])}));

    // the gate for keeping Windows: the same query Disk Management uses.
    // When it fails, WHY is kept; no cause is guessed.
    let (mut shrink_gb, mut shrink_error, mut failed_at): (Option<f64>, Option<String>, Option<&str>) = (None, None, Some("Get-Partition"));
    match storage.as_ref().map_err(|e| e.clone()).and_then(|w| w.query_where("MSFT_Partition", &["ObjectId", "Size", "DiskNumber"], "DriveLetter='C'")) {
        Ok(parts) if !parts.is_empty() => {
            let part = &parts[0];
            failed_at = Some("Get-PartitionSupportedSize");
            let path = text(&part["__RELPATH"]);
            match storage.as_ref().unwrap().call("MSFT_Partition", &path, "GetSupportedSize", &["ReturnValue", "SizeMin", "SizeMax", "ExtendedStatus"]) {
                Ok(r) if int(&r["ReturnValue"]) == Some(0) => {
                    if let (Some(size), Some(min)) = (num(&part["Size"]), num(&r["SizeMin"])) {
                        shrink_gb = Some(round1((size - min) / GB));
                        failed_at = None;
                    } else {
                        shrink_error = Some("the Storage API returned no minimum size".into());
                    }
                }
                Ok(r) => shrink_error = Some(storage_error(&r)),
                Err(e) => shrink_error = Some(e),
            }
        }
        Ok(_) => shrink_error = Some("No MSFT_Partition objects found with property 'DriveLetter' equal to 'C'. Verify the value of the property and retry.".into()),
        Err(e) => shrink_error = Some(e),
    }
    // the second, independent read-only measurement: diskpart's `shrink querymax`
    let mut shrink_source = shrink_gb.map(|_| "storage-api");
    let mut diskpart_error = None;
    if shrink_gb.is_none() && is_admin {
        let lines = diskpart_query_max_lines();
        match diskpart_query_max(&lines) {
            Some(gb) => {
                shrink_gb = Some(gb);
                shrink_source = Some("diskpart");
            }
            None => {
                let kept: Vec<&String> = lines.iter().filter(|l| !l.trim().is_empty()).collect();
                let tail = &kept[kept.len().saturating_sub(2)..];
                diskpart_error = Some(tail.iter().map(|l| l.as_str()).collect::<Vec<_>>().join(" | "));
            }
        }
    }
    // a small number has a named cause: Windows logs the last unmovable file
    let mut last_unmovable = None;
    if is_admin && shrink_gb.is_some_and(|gb| gb < 25.0) {
        last_unmovable = last_unmovable_file(measure_start);
    }
    let part_count = storage.as_ref().ok().and_then(|w| w.query_where("MSFT_Partition", &["PartitionNumber"], "DiskNumber=0").ok()).map(|l| l.len()).unwrap_or(0);
    json!({
        "Disks": disks, "SysVolume": sys_volume, "LastUnmovable": last_unmovable,
        "ShrinkGB": shrink_gb, "ShrinkSource": shrink_source, "ShrinkError": shrink_error, "ShrinkFailedAt": failed_at,
        "DiskpartError": diskpart_error, "Disk0PartCount": part_count,
    })
}

/// diskpart's `shrink querymax` on C:, which only reports. Its lines, or
/// one line saying what went wrong.
fn diskpart_query_max_lines() -> Vec<String> {
    let script = std::env::temp_dir().join(format!("upgrade-scan-diskpart-{}.txt", std::process::id()));
    if let Err(e) = std::fs::write(&script, "select volume C\r\nshrink querymax\r\n") {
        return vec![format!("diskpart: {e}")];
    }
    let result = run_tool("diskpart.exe", &["/s", &script.to_string_lossy()], 60);
    let _ = std::fs::remove_file(&script);
    result.unwrap_or_else(|e| vec![format!("diskpart: {e}")])
}

/// Defrag event 259 since a moment; if none, `shrink querymax` makes
/// Windows write one.
pub fn last_unmovable_file(since: Stamp) -> Option<String> {
    let read = || {
        events::query("Application", &format!("*[System[Provider[@Name='Microsoft-Windows-Defrag'] and (EventID=259) and {}]]", within(600)))
            .ok()?
            .into_iter()
            .filter(|e| e.time_local >= since)
            .next()
            .and_then(|e| defrag_259(&e.message))
    };
    read().or_else(|| {
        let _ = diskpart_query_max_lines();
        std::thread::sleep(std::time::Duration::from_secs(3));
        read()
    })
}

/// The System log's `disk` events of the last 30 days, counted for one disk.
fn disk_event_facts(disk_number: i64) -> Value {
    match events::query("System", &format!("*[System[Provider[@Name='disk'] and {}]]", within(30 * DAY))) {
        Ok(list) => {
            let evs: Vec<DiskEvent> = list.into_iter().map(|e| DiskEvent { id: e.id, time_created: e.time_local, message: Some(e.message) }).collect();
            let r = disk_events(&evs, disk_number);
            json!({"BadBlock": r.bad_block, "Paging": r.paging, "Reset": r.reset, "First": r.first.map(|t| t.iso()), "Last": r.last.map(|t| t.iso()), "Days": 30})
        }
        Err(_) => Value::Null,
    }
}

/// Get-UpgSmartFacts: the ATA SMART attributes Windows exposes (SATA
/// drives; NVMe answers "Not supported").
fn smart_facts(disk_number: i64) -> Value {
    let unavailable = |e: String| json!({"Source": "unavailable", "Error": e});
    let cimv2 = match Wmi::connect(CIMV2) {
        Ok(w) => w,
        Err(e) => return unavailable(e),
    };
    let drive = match cimv2.query_where("Win32_DiskDrive", &["PNPDeviceID"], &format!("Index={disk_number}")) {
        Ok(l) => l.into_iter().next(),
        Err(e) => return unavailable(e),
    };
    let Some(drive) = drive else { return json!({"Source": "none", "Error": "no Win32_DiskDrive for the disk"}) };
    let pnp = text(&drive["PNPDeviceID"]).to_lowercase();
    let wmi = match Wmi::connect(WMI_ROOT) {
        Ok(w) => w,
        Err(e) => return unavailable(e),
    };
    let data = match wmi.query("MSStorageDriver_FailurePredictData", &["InstanceName", "VendorSpecific"]) {
        Ok(l) => l.into_iter().find(|d| text(&d["InstanceName"]).to_lowercase().starts_with(&pnp)),
        Err(e) => return unavailable(e),
    };
    let Some(data) = data else { return json!({"Source": "none", "Error": "no SMART data instance for the disk"}) };
    let bytes: Vec<u8> = data["VendorSpecific"].as_array().map(|l| l.iter().filter_map(|x| x.as_u64()).map(|x| x as u8).collect()).unwrap_or_default();
    let a = smart_attributes(&bytes);
    let status = wmi.query("MSStorageDriver_FailurePredictStatus", &["InstanceName", "PredictFailure"]).ok().and_then(|l| l.into_iter().find(|d| text(&d["InstanceName"]).to_lowercase().starts_with(&pnp)));
    let at = |id: u8| a.get(&id).copied();
    json!({"Source": "ata-smart", "Error": null, "PredictFailure": status.map(|s| s["PredictFailure"].as_bool()),
           "Reallocated": at(5), "Uncorrectable": at(187), "Pending": at(197), "OfflineUncorrectable": at(198), "Crc": at(199), "EndToEnd": at(184)})
}

/// Get-UpgPhysicalDiskFacts: the drive that holds C:, as Windows' storage
/// stack reports it, with the error log and SMART beside it.
pub fn physical_disk_facts(is_admin: bool) -> Value {
    let mut f = json!({"Found": false, "DiskNumber": null, "FriendlyName": null, "MediaType": null, "BusType": null,
                       "HealthStatus": null, "OperationalStatus": null, "Size": null, "Counters": null, "CountersError": null, "Error": null, "DiskEvents": null, "Smart": null});
    let storage = match storage_wmi() {
        Ok(w) => w,
        Err(e) => {
            f["Error"] = json!(e);
            return f;
        }
    };
    let part = match storage.query_where("MSFT_Partition", &["DiskNumber"], "DriveLetter='C'") {
        Ok(l) => l.into_iter().next(),
        Err(e) => {
            f["Error"] = json!(e);
            return f;
        }
    };
    let Some(part) = part else {
        f["Error"] = json!("No MSFT_Partition objects found with property 'DriveLetter' equal to 'C'. Verify the value of the property and retry.");
        return f;
    };
    let number = int(&part["DiskNumber"]).unwrap_or(-1);
    f["DiskNumber"] = json!(number);
    // ObjectId is the key: without it in the query, WMI gives the object no
    // path, and nothing can be asked of it afterwards
    let props = ["ObjectId", "DeviceId", "UniqueId", "FriendlyName", "MediaType", "BusType", "HealthStatus", "OperationalStatus", "Size"];
    let all = match storage.query("MSFT_PhysicalDisk", &props) {
        Ok(l) => l,
        Err(e) => {
            f["Error"] = json!(e);
            return f;
        }
    };
    let mut found: Vec<&Value> = all.iter().filter(|d| text(&d["DeviceId"]) == number.to_string()).collect();
    if found.len() != 1 {
        // DeviceId is the disk number as a string on every machine seen so
        // far; when it does not map one-to-one, the unique id decides
        let uid = storage.query_where("MSFT_Disk", &["UniqueId"], &format!("Number={number}")).ok().and_then(|l| l.into_iter().next()).map(|d| text(&d["UniqueId"])).unwrap_or_default();
        found = all.iter().filter(|d| !text(&d["UniqueId"]).is_empty() && text(&d["UniqueId"]) == uid).collect();
    }
    if found.len() != 1 {
        f["Error"] = json!(format!("Get-PhysicalDisk returned {} candidates for disk {number}", found.len()));
        return f;
    }
    let p = found[0];
    f["Found"] = json!(true);
    f["FriendlyName"] = p["FriendlyName"].clone();
    f["MediaType"] = json!(media_type(&p["MediaType"]));
    f["BusType"] = json!(bus_type(&p["BusType"]));
    f["HealthStatus"] = json!(health_status(&p["HealthStatus"]));
    f["OperationalStatus"] = json!(operational_status(&p["OperationalStatus"], false));
    f["Size"] = count(&p["Size"]);
    f["DiskEvents"] = disk_event_facts(number);
    f["Smart"] = smart_facts(number);
    if is_admin {
        // the counters hang off the physical disk through an association
        let assoc = format!("ASSOCIATORS OF {{{}}} WHERE AssocClass=MSFT_PhysicalDiskToStorageReliabilityCounter", text(&p["__RELPATH"]));
        match storage.wql(&assoc, &["Temperature", "Wear", "ReadErrorsUncorrected", "WriteErrorsUncorrected", "ReadErrorsTotal", "PowerOnHours"]) {
            Ok(l) if !l.is_empty() => {
                let c = &l[0];
                f["Counters"] = json!({"Temperature": count(&c["Temperature"]), "Wear": count(&c["Wear"]), "ReadErrorsUncorrected": count(&c["ReadErrorsUncorrected"]),
                                       "WriteErrorsUncorrected": count(&c["WriteErrorsUncorrected"]), "ReadErrorsTotal": count(&c["ReadErrorsTotal"]), "PowerOnHours": count(&c["PowerOnHours"])});
            }
            Ok(_) => f["CountersError"] = json!("no reliability counters for the disk"),
            Err(e) => f["CountersError"] = json!(e),
        }
    }
    f
}

/// The names `Repair-Volume` gives a scan's result, read from this
/// machine's own Storage module (`Volume.cdxml`), so the word is the one
/// PowerShell would print here. Windows 10 and 11 name them differently.
fn repair_status_names() -> Vec<(i64, String)> {
    let path = format!("{}\\System32\\WindowsPowerShell\\v1.0\\Modules\\Storage\\Volume.cdxml", std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into()));
    let mut names = Vec::new();
    if let Ok(text) = std::fs::read_to_string(&path) {
        if let Some(start) = text.find("EnumName=\"Volume.RepairStatus\"") {
            let rest = &text[start..];
            let end = rest.find("</Enum>").unwrap_or(rest.len());
            let re = regress::Regex::new(r#"<Value Name="([A-Za-z]+)" Value="(\d+)""#).expect("a fixed pattern");
            for m in re.find_iter(&rest[..end]) {
                if let (Some(n), Some(v)) = (m.group(1), m.group(2)) {
                    if let Ok(code) = rest[v].parse::<i64>() {
                        names.push((code, rest[n].to_string()));
                    }
                }
            }
        }
    }
    if names.is_empty() {
        // Windows 11 24H2's table, as read on the G16 (2026-10-06)
        for (c, n) in [(0, "NoErrorsFound"), (1, "ErrorsFixed"), (2, "MinorErrorsFixedOrCleanup"), (3, "Failed"), (4, "ScanNoErrorsFound"), (5, "ScanErrorsFoundAndFixedOnline"), (6, "ScanErrorsFixedOnlineAlsoNeedSpotFix"), (7, "ScanErrorsFoundNeedSpotFix"), (8, "ScanNeedsRetry"), (9, "ScanRunning")] {
            names.push((c, n.to_string()));
        }
    }
    names
}

/// `Repair-Volume -DriveLetter C -Scan`: the online scan, which only reads.
/// Its result as the cmdlet would print it, or `scan failed: <why>`.
pub fn online_scan() -> String {
    let storage = match storage_wmi() {
        Ok(w) => w,
        Err(e) => return format!("scan failed: {e}"),
    };
    let volume = match storage.query_where("MSFT_Volume", &["ObjectId"], "DriveLetter='C'") {
        Ok(l) if !l.is_empty() => l.into_iter().next().unwrap(),
        Ok(_) => return "scan failed: No MSFT_Volume objects found with property 'DriveLetter' equal to 'C'. Verify the value of the property and retry.".into(),
        Err(e) => return format!("scan failed: {e}"),
    };
    match storage.call_with("MSFT_Volume", &text(&volume["__RELPATH"]), "Repair", &[("Scan", json!(true)), ("OfflineScanAndFix", json!(false)), ("SpotFix", json!(false))], &["ReturnValue", "Output", "ExtendedStatus"]) {
        Ok(r) if int(&r["ReturnValue"]) == Some(0) => {
            let code = int(&r["Output"]).unwrap_or(-1);
            repair_status_names().into_iter().find(|(c, _)| *c == code).map(|(_, n)| n).unwrap_or_else(|| code.to_string())
        }
        Ok(r) => format!("scan failed: {}", storage_error(&r)),
        Err(e) => format!("scan failed: {e}"),
    }
}

/// Get-UpgVolumeHealth (elevated only).
pub fn volume_health(is_admin: bool, shrink_error: &str) -> Value {
    if !is_admin {
        return json!({"Dirty": "unknown", "Scan": null, "ScanRan": false, "Error": "not elevated"});
    }
    let (mut dirty, mut err): (String, Option<String>) = ("unknown".into(), None);
    match run_tool("fsutil", &["dirty", "query", "C:"], 60) {
        Ok(lines) => {
            dirty = fsutil_dirty(&lines).to_string();
            if dirty == "unknown" {
                err = Some(lines.join(" ").trim().to_string());
            }
        }
        Err(e) => err = Some(e),
    }
    let (mut vol_status, mut vol_health) = (Value::Null, Value::Null);
    if let Some(v) = storage_wmi().ok().and_then(|w| w.query_where("MSFT_Volume", &["OperationalStatus", "HealthStatus"], "DriveLetter='C'").ok()).and_then(|l| l.into_iter().next()) {
        vol_status = json!(operational_status(&v["OperationalStatus"], true));
        vol_health = json!(health_status(&v["HealthStatus"]));
    }
    // NTFS event 98: the volume needs a full chkdsk; Wininit 1001 or
    // autochk's log: the last boot-time check that completed
    let ntfs_full = events::query("System", &format!("*[System[(EventID=98) and {}]]", within(30 * DAY)))
        .ok()
        .and_then(|l| l.into_iter().find(|e| matches("Ntfs", &e.provider) && matches("Full Chkdsk", &e.message) && matches("Volume C:", &e.message)))
        .map(|e| e.time_local);
    let mut last_check = events::query("Application", &format!("*[System[(EventID=1001) and {}]]", within(60 * DAY))).ok().and_then(|l| l.into_iter().find(|e| matches("Wininit", &e.provider))).map(|e| e.time_local);
    if let Ok(dir) = std::fs::read_dir(r"C:\System Volume Information\Chkdsk") {
        let newest = dir.flatten().filter(|e| e.file_name().to_string_lossy().to_lowercase().starts_with("chkdsk") && e.file_name().to_string_lossy().to_lowercase().ends_with(".log")).filter_map(|e| e.metadata().ok()?.modified().ok()).max();
        if let Some(m) = newest {
            let secs = m.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
            let st = super::win::utc_to_local(super::win::utc_from_seconds(secs));
            if last_check.is_none_or(|l| st > l) {
                last_check = Some(st);
            }
        }
    }
    let scan_started = now_local();
    let reason = dirty == "dirty" || matches("volume with errors|corrupt", shrink_error) || matches("repair", &text(&vol_status)) || ntfs_full.is_some();
    let (scan, scan_ran) = if reason { (json!(online_scan()), true) } else { (Value::Null, false) };
    let since = if scan_ran { 5 } else { 7 * DAY };
    let logged = events::query("Application", &format!("*[System[Provider[@Name='Chkdsk'] and {}]]", within(since))).ok().and_then(|l| l.into_iter().next()).map(|e| {
        let r = chkdsk_event(&e.message);
        json!({"Verdict": r.verdict, "Records": r.records, "Queued": r.queued, "When": e.time_local.iso()})
    });
    let _ = scan_started;
    json!({"Dirty": dirty, "Scan": scan, "ScanRan": scan_ran, "Error": err,
           "VolumeStatus": vol_status, "VolumeHealth": vol_health, "NtfsFullChkdsk": ntfs_full.map(|t| t.iso()), "LastCheck": last_check.map(|t| t.iso()), "Logged": logged})
}

/// Get-UpgBitLockerState: the volumes whose protection is on.
pub fn bitlocker_state() -> Value {
    match Wmi::connect(BITLOCKER).and_then(|w| w.query("Win32_EncryptableVolume", &["DriveLetter", "ProtectionStatus"])) {
        Ok(vols) => {
            let mounts: Vec<Value> = vols.iter().filter(|v| int(&v["ProtectionStatus"]) == Some(1)).map(|v| v["DriveLetter"].clone()).collect();
            json!({"Succeeded": true, "EncryptedMounts": mounts})
        }
        Err(_) => json!({"Succeeded": false, "EncryptedMounts": []}),
    }
}

/// Get-UpgEspFacts (elevated only): mount the EFI system partition on a
/// free letter, measure it, read which volume the firmware's Windows entry
/// points at, unmount. Read-only: nothing on it is written.
pub fn esp_facts() -> Value {
    let failed = json!({"Succeeded": false, "FreeBytes": null, "TotalBytes": null, "HasWindowsBootFiles": null, "BootmgrPointsAtEsp": null, "BcdDevice": null});
    let letter = match "ZYXWVUT".chars().find(|c| !std::path::Path::new(&format!("{c}:\\")).exists()) {
        Some(c) => format!("{c}:"),
        None => return failed,
    };
    let _ = run_tool("cmd", &["/c", &format!("mountvol {letter} /S")], 60);
    if !std::path::Path::new(&format!("{letter}\\")).exists() {
        return failed;
    }
    let result = (|| {
        use windows::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, QueryDosDeviceW};
        let root = wide(&format!("{letter}\\"));
        let (mut free, mut total, mut total_free) = (0u64, 0u64, 0u64);
        unsafe { GetDiskFreeSpaceExW(windows::core::PCWSTR(root.as_ptr()), Some(&mut free), Some(&mut total), Some(&mut total_free)) }.ok()?;
        let has_boot_files = std::path::Path::new(&format!("{letter}\\EFI\\Microsoft\\Boot\\bootmgfw.efi")).exists();
        let bcd = run_tool("bcdedit", &["/enum", "{bootmgr}"], 60).unwrap_or_default().join("\n");
        let bcd_device = crate::ps::capture(r"^device\s+partition=(\S+)", "m", &bcd).map(str::to_string);
        let points = bcd_device.as_deref().map(|d| {
            if d.eq_ignore_ascii_case(&letter) {
                Some(true)
            } else if d.starts_with("\\Device\\") {
                let name = wide(letter.trim_end_matches('\\'));
                let mut buf = [0u16; 1024];
                let n = unsafe { QueryDosDeviceW(windows::core::PCWSTR(name.as_ptr()), Some(&mut buf)) };
                if n > 0 {
                    let end = buf.iter().position(|u| *u == 0).unwrap_or(buf.len());
                    Some(String::from_utf16_lossy(&buf[..end]).eq_ignore_ascii_case(d))
                } else {
                    None
                }
            } else {
                Some(false)
            }
        });
        Some(json!({"Succeeded": true, "FreeBytes": free, "TotalBytes": total, "HasWindowsBootFiles": has_boot_files, "BootmgrPointsAtEsp": points.flatten(), "BcdDevice": bcd_device}))
    })();
    let _ = run_tool("cmd", &["/c", &format!("mountvol {letter} /D")], 60);
    result.unwrap_or(failed)
}

/// The PowerShell error for a volume query, for the record.
#[allow(dead_code)]
fn volume_error(e: &windows::core::Error) -> String {
    wmi_error(e)
}
