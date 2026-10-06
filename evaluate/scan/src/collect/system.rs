//! The collectors that need no privilege: the system facts, the device
//! list, three registry values and the installed programs. Each shapes its
//! fact exactly as the PowerShell collector does (`Get-UpgSystem`,
//! `Get-UpgPnp`, `Get-UpgSecureBootState`, `Get-UpgFastStartupState`, the
//! registry half of `Get-UpgSbatFacts`, `Get-UpgInstalledApps`).

use super::registry::{self, Hive};
use super::wmi::Wmi;
use super::Collected;
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
    use windows::Win32::Foundation::SYSTEMTIME;
    use windows::Win32::System::Time::SystemTimeToTzSpecificLocalTime;
    let t = text(v);
    if t.len() < 25 || !t[..14].bytes().all(|b| b.is_ascii_digit()) {
        return v.clone();
    }
    let n = |a: usize, b: usize| t[a..b].parse::<i64>().unwrap_or(0);
    let offset: i64 = t[21..25].parse().unwrap_or(0);
    // to UTC through a day count, so an offset can cross midnight
    let utc = crate::ps::Stamp { year: n(0, 4) as i32, month: n(4, 6) as u32, day: n(6, 8) as u32, hour: n(8, 10) as u32, minute: n(10, 12) as u32, second: n(12, 14) as u32 }.seconds() - offset * 60;
    let days = utc.div_euclid(86400);
    let rem = utc.rem_euclid(86400);
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u16;
    let month = (if mp < 10 { mp + 3 } else { mp - 9 }) as u16;
    let year = (yoe + era * 400 + if month <= 2 { 1 } else { 0 }) as u16;
    let st = SYSTEMTIME { wYear: year, wMonth: month, wDay: day, wHour: (rem / 3600) as u16, wMinute: (rem % 3600 / 60) as u16, wSecond: (rem % 60) as u16, ..Default::default() };
    let mut local = SYSTEMTIME::default();
    if unsafe { SystemTimeToTzSpecificLocalTime(None, &st, &mut local) }.is_err() {
        return v.clone();
    }
    json!(format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}", local.wYear, local.wMonth, local.wDay, local.wHour, local.wMinute, local.wSecond))
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

/// Get-UpgPnp: one enumeration, every device.
fn pnp(wmi: &Wmi) -> Result<Value, String> {
    Ok(Value::Array(wmi.query("Win32_PnPEntity", &["Name", "DeviceID", "PNPClass", "Service", "CompatibleID"])?))
}

const SECURE_BOOT: &str = r"SYSTEM\CurrentControlSet\Control\SecureBoot\State";
const SBAT: &str = r"SYSTEM\CurrentControlSet\Control\SecureBoot\SBAT";
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

pub fn collect() -> Collected {
    let mut c = Collected::default();
    match Wmi::connect() {
        Ok(wmi) => {
            match system(&wmi) {
                Ok(v) => {
                    c.facts.insert("Sys".into(), v);
                }
                Err(e) => c.errors.push(format!("Sys: {e}")),
            }
            match pnp(&wmi) {
                Ok(v) => {
                    c.facts.insert("Pnp".into(), v);
                }
                Err(e) => c.errors.push(format!("Pnp: {e}")),
            }
        }
        Err(e) => c.errors.push(format!("WMI: {e}")),
    }
    c.facts.insert("SecureBoot".into(), json!(registry::dword(&Hive::LocalMachine, SECURE_BOOT, "UEFISecureBootEnabled")));
    c.facts.insert("Hiberboot".into(), json!(registry::dword(&Hive::LocalMachine, POWER, "HiberbootEnabled")));
    // the registry half of Get-UpgSbatFacts; the firmware's own copy and the
    // stick's boot files are not read yet
    let mut levels = Vec::new();
    if let Some((_, bytes)) = registry::value(&Hive::LocalMachine, SBAT, "SbatLevel") {
        let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
        let level = String::from_utf8_lossy(&bytes[..end]).to_string();
        if !level.is_empty() {
            levels.push(json!({"Source": "Windows (registry)", "Text": level}));
        }
    }
    c.facts.insert("Sbat".into(), json!({"Levels": levels, "Files": []}));
    c.facts.insert("Apps".into(), json!(installed_apps()));
    c.not_read = vec!["IsAdmin", "Sbat (firmware SbatLevelRT, the stick's boot files)", "DbAuthorities", "Resume", "Disk", "VolumeHealth", "PhysicalDisk", "BitLocker", "Esp"];
    c
}
