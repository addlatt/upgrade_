//! Get-UpgSystem, Get-UpgPnp, Get-UpgSecureBootState, Get-UpgFastStartupState,
//! Get-UpgInstalledApps, and the run that strings every collector together in
//! the scanner's own order, with the same "elevated only" gates.

use super::registry::{self, Hive};
use super::wmi::{Wmi, CIMV2};
use super::{firmware, resume, storage, win, Collected};
use crate::ps::round1;
use serde_json::{json, Value};

const GB: f64 = 1073741824.0;

fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn number(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// A CIM date (`20250520000000.000000+000`, the offset in minutes from UTC)
/// as `Get-CimInstance` gives it: this machine's local time, written
/// `2025-05-19T20:00:00`. Anything else is kept as it came.
fn cim_date(v: &Value) -> Value {
    let t = text(v);
    if t.len() < 25 || !t[..14].bytes().all(|b| b.is_ascii_digit()) {
        return v.clone();
    }
    let n = |a: usize, b: usize| t[a..b].parse::<i64>().unwrap_or(0);
    let offset: i64 = t[21..25].parse().unwrap_or(0);
    let utc = crate::ps::Stamp { year: n(0, 4) as i32, month: n(4, 6) as u32, day: n(6, 8) as u32, hour: n(8, 10) as u32, minute: n(10, 12) as u32, second: n(12, 14) as u32 }.seconds() - offset * 60;
    json!(win::utc_to_local(win::utc_from_seconds(utc)).iso())
}

/// Get-UpgSystem.
fn system(wmi: &Wmi) -> Result<Value, String> {
    let cs = wmi.query("Win32_ComputerSystem", &["Manufacturer", "Model", "TotalPhysicalMemory"])?.into_iter().next().unwrap_or(Value::Null);
    let os = wmi.query("Win32_OperatingSystem", &["Caption", "BuildNumber"])?.into_iter().next().unwrap_or(Value::Null);
    let cpu = wmi.query("Win32_Processor", &["Name", "Architecture", "NumberOfCores"])?.into_iter().next().unwrap_or(Value::Null);
    let bios = wmi.query("Win32_BIOS", &["SMBIOSBIOSVersion", "ReleaseDate"])?.into_iter().next().unwrap_or(Value::Null);
    let batteries = wmi.query("Win32_Battery", &["Name"]).unwrap_or_default();
    let cpu_name = text(&cpu["Name"]).split_whitespace().collect::<Vec<_>>().join(" ");
    Ok(json!({
        "Vendor": cs["Manufacturer"], "Model": cs["Model"],
        "RamGB": number(&cs["TotalPhysicalMemory"]).map(|b| round1(b / GB)),
        "OsCaption": os["Caption"], "OsBuild": number(&os["BuildNumber"]).map(|b| b as i64).unwrap_or(0),
        "CpuName": cpu_name, "CpuArch": cpu["Architecture"], "CpuCores": cpu["NumberOfCores"],
        "BiosVersion": bios["SMBIOSBIOSVersion"], "BiosDate": cim_date(&bios["ReleaseDate"]),
        "IsLaptop": !batteries.is_empty(),
        "Firmware": std::env::var("firmware_type").ok(),
    }))
}

/// Get-UpgPnp: one enumeration, every device. The system properties WMI
/// adds (`__PATH`) are left out; the recorder's shape has five fields.
fn pnp(wmi: &Wmi) -> Result<Value, String> {
    Ok(Value::Array(
        wmi.query("Win32_PnPEntity", &["Name", "DeviceID", "PNPClass", "Service", "CompatibleID"])?
            .into_iter()
            .map(|d| json!({"Name": d["Name"], "DeviceID": d["DeviceID"], "PNPClass": d["PNPClass"], "Service": d["Service"], "CompatibleID": d["CompatibleID"]}))
            .collect(),
    ))
}

const SECURE_BOOT: &str = r"SYSTEM\CurrentControlSet\Control\SecureBoot\State";
const POWER: &str = r"SYSTEM\CurrentControlSet\Control\Session Manager\Power";

/// Get-UpgInstalledApps: the display names under the three uninstall keys,
/// each name once.
fn installed_apps() -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for (hive, key) in [
        (Hive::LocalMachine, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"),
        (Hive::LocalMachine, r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"),
        (Hive::CurrentUser, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"),
    ] {
        for sub in registry::subkeys(&hive, key) {
            if let Some(name) = registry::string(&hive, &format!("{key}\\{sub}"), "DisplayName").filter(|n| !n.is_empty()) {
                if !names.iter().any(|x| x.to_lowercase() == name.to_lowercase()) {
                    names.push(name);
                }
            }
        }
    }
    names.sort_by_key(|n| n.to_lowercase());
    names
}

/// Every collector, in the scanner's order, with its gates. `kit_root`:
/// where the kit's boot files sit (the stick's root), if the scanner runs
/// from one.
pub fn collect(kit_root: Option<&std::path::Path>) -> Collected {
    let mut c = Collected::default();
    let is_admin = win::is_admin();
    c.facts.insert("IsAdmin".into(), json!(is_admin));
    let cimv2 = Wmi::connect(CIMV2);
    match &cimv2 {
        Ok(wmi) => {
            match system(wmi) {
                Ok(v) => {
                    c.facts.insert("Sys".into(), v);
                }
                Err(e) => c.errors.push(format!("Sys: {e}")),
            }
            match pnp(wmi) {
                Ok(v) => {
                    c.facts.insert("Pnp".into(), v);
                }
                Err(e) => c.errors.push(format!("Pnp: {e}")),
            }
        }
        Err(e) => c.errors.push(format!("WMI: {e}")),
    }
    c.facts.insert("SecureBoot".into(), json!(registry::dword(&Hive::LocalMachine, SECURE_BOOT, "UEFISecureBootEnabled")));
    c.facts.insert("Sbat".into(), firmware::sbat_facts(kit_root, is_admin));
    if is_admin {
        c.facts.insert("DbAuthorities".into(), json!(firmware::trusted_authorities()));
    }
    c.facts.insert("Resume".into(), resume::resume_facts(cimv2.as_ref().ok()));
    let disk = storage::disk_facts(is_admin);
    let shrink_error = text(&disk["ShrinkError"]);
    c.facts.insert("Disk".into(), disk);
    c.facts.insert("VolumeHealth".into(), storage::volume_health(is_admin, &shrink_error));
    c.facts.insert("PhysicalDisk".into(), storage::physical_disk_facts(is_admin));
    c.facts.insert("Hiberboot".into(), json!(registry::dword(&Hive::LocalMachine, POWER, "HiberbootEnabled")));
    if is_admin {
        c.facts.insert("BitLocker".into(), storage::bitlocker_state());
        c.facts.insert("Esp".into(), storage::esp_facts());
    }
    c.facts.insert("Apps".into(), json!(installed_apps()));
    c
}
