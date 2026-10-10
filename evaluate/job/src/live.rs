//! The job writer's live half, Windows only, following `New-Job.ps1` 0.18.0:
//! `Get-JobFacts` (every read the job is written from, in the script's field
//! names, so `document::job_document` judges them as the script does), the
//! Wi-Fi export, and the files the writer leaves behind.
//!
//! Read-only on the machine. The one directory it writes is the job folder
//! on the stick. Nothing here decides: the judging half does, and a job is
//! written only as an `upgrade_schema::Job` that passed the whole contract.
//!
//! Proven side by side: `tools/Record-JobFacts.ps1` (the script's own
//! `Get-JobFacts`) and `upgrade-job facts` on the same machine in the same
//! minute, then `upgrade-job compare-facts`.

use crate::val::{at, int as vint, s, truthy};
use crate::wifi::{self, SecretFile};
use crate::{decide, records};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use upgrade_scan::collect::registry::{self, Hive};
use upgrade_scan::collect::storage::{bus_type, health_status, media_type, operational_status, partition_style, storage_error, storage_wmi};
use upgrade_scan::collect::win::run_tool;
use upgrade_scan::collect::wmi::{Wmi, BITLOCKER, CIMV2};
use upgrade_scan::collect::{self, events};
use upgrade_scan::parse::{defrag_259, fsutil_dirty};
use upgrade_scan::ps::{matches, round1, Stamp};
use windows::core::{GUID, HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::HANDLE;

const GB: f64 = 1073741824.0;
const DAY: i64 = 86400;
const ESP_GUID: &str = "{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}";
const LINUX_MIN_GB: f64 = crate::LINUX_MIN_GB;

fn text(v: &Value) -> String {
    match v {
        Value::String(t) => t.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(t) => t.trim().parse().ok(),
        _ => None,
    }
}

fn int(v: &Value) -> Option<i64> {
    num(v).map(|f| f as i64)
}

/// `"$($x.Field)"`: a string, empty for nothing.
fn str_of(v: &Value) -> String {
    text(v)
}

/// A disk's serial as the script keeps it: whitespace removed.
fn serial(v: &Value) -> String {
    text(v).split_whitespace().collect()
}

/// `$_.Exception.Message -replace '\s+', ' '`, trimmed.
fn one_line(e: &str) -> String {
    e.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `'yyyy-MM-ddTHH:mm:ssZ'` now.
pub fn now_utc_z() -> String {
    let (_, o) = collect::now();
    format!("{}Z", &o[..19])
}

/// `[guid]::NewGuid().ToString()`: a random GUID, lower case, dashed.
pub fn new_guid() -> Result<String, String> {
    let mut b = [0u8; 16];
    crate::password::os_random(&mut b)?;
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: Vec<String> = b.iter().map(|x| format!("{x:02x}")).collect();
    Ok(format!("{}-{}-{}-{}-{}", h[0..4].concat(), h[4..6].concat(), h[6..8].concat(), h[8..10].concat(), h[10..16].concat()))
}

/// The newest `upgrade-report-*.json` in the reports folder.
fn newest_report(dir: &Path) -> Option<PathBuf> {
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !(name.starts_with("upgrade-report-") && name.ends_with(".json")) {
            continue;
        }
        let t = e.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
        if best.as_ref().is_none_or(|(bt, _)| t > *bt) {
            best = Some((t, p));
        }
    }
    best.map(|(_, p)| p)
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(std::fs::read_to_string(path).ok()?.trim_start_matches('\u{feff}')).ok()
}

/// A CIM time read through the Rust event reader, as the script shows it
/// in a sentence (`"$NtfsFullChkdsk"`) and orders it.
fn moment(t: Stamp) -> decide::Moment {
    decide::Moment { shown: t.to_string(), order: Some(t.seconds()) }
}

/// Get-JobRepairQueued: the same two reads the scanner and the prologue make.
fn repair_queued(storage: Option<&Wmi>) -> decide::RepairQueued {
    let status = storage.and_then(|w| w.query_where("MSFT_Volume", &["OperationalStatus"], "DriveLetter='C'").ok()).and_then(|l| l.into_iter().next()).map(|v| operational_status(&v["OperationalStatus"], true)).unwrap_or_default();
    let n98 = events::query("System", &format!("*[System[(EventID=98) and {}]]", events::within(30 * DAY)))
        .ok()
        .and_then(|l| l.into_iter().find(|e| matches("Ntfs", &e.provider) && matches("Full Chkdsk", &e.message) && matches("Volume C:", &e.message)))
        .map(|e| e.time_local);
    let mut last = events::query("Application", &format!("*[System[(EventID=1001) and {}]]", events::within(60 * DAY))).ok().and_then(|l| l.into_iter().find(|e| matches("Wininit", &e.provider))).map(|e| e.time_local);
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
    decide::repair_queued(&status, n98.map(moment).as_ref(), last.map(moment).as_ref())
}

/// Get-JobClockFacts: the registry and the system's view of the zone.
pub fn clock_facts() -> Value {
    use windows::Win32::System::Time::GetDynamicTimeZoneInformation;
    const TZ: &str = r"SYSTEM\CurrentControlSet\Control\TimeZoneInformation";
    let zone = registry::string(&Hive::LocalMachine, TZ, "TimeZoneKeyName").unwrap_or_default();
    let rtu = registry::dword(&Hive::LocalMachine, TZ, "RealTimeIsUniversal").map_or(Value::Null, |v| json!(v));
    let dst_disabled = registry::dword(&Hive::LocalMachine, TZ, "DynamicDaylightTimeDisabled").map_or(Value::Null, |v| json!(v));
    let mut tzi = windows::Win32::System::Time::DYNAMIC_TIME_ZONE_INFORMATION::default();
    let kind = unsafe { GetDynamicTimeZoneInformation(&mut tzi) };
    let dst = kind == 2; // TIME_ZONE_ID_DAYLIGHT
    let base = -(tzi.Bias as i64);
    let offset = -(tzi.Bias as i64 + if dst { tzi.DaylightBias as i64 } else { tzi.StandardBias as i64 });
    json!({"WindowsZone": zone, "RealTimeIsUniversal": rtu, "DynamicDstDisabled": dst_disabled, "OffsetMinutes": offset, "BaseOffsetMinutes": base, "DstActive": dst, "NowUtc": now_utc_z()})
}

/// Get-JobLicenseFacts: the registry and Windows' licensing service. The
/// firmware key is tested for being there and dropped on the spot.
pub fn license_facts(cimv2: Option<&Wmi>) -> Value {
    const NT: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
    let reg = |n: &str| registry::string(&Hive::LocalMachine, NT, n).unwrap_or_default();
    let os = json!({"ProductName": reg("ProductName"), "EditionId": reg("EditionID"), "DisplayVersion": reg("DisplayVersion"), "Build": reg("CurrentBuild")});
    let mut r = json!({"Os": os, "Products": [], "Firmware": null, "Error": null, "NowUtc": now_utc_z()});
    let read = || -> Result<(Vec<Value>, Value), String> {
        let w = cimv2.ok_or_else(|| "WMI is not available".to_string())?;
        let products = w.query_where("SoftwareLicensingProduct", &["LicenseStatus", "ProductKeyChannel", "Description", "LicenseIsAddon"], "ApplicationID='55c92734-d682-4d71-983e-d6ec3f16059f' AND PartialProductKey IS NOT NULL")?;
        let list = products
            .iter()
            .map(|p| {
                let mut ch = text(&p["ProductKeyChannel"]);
                if ch.is_empty() {
                    if let Some(c) = upgrade_scan::ps::capture(r",\s*(\S+)\s+channel", "", &text(&p["Description"])) {
                        ch = c.to_string();
                    }
                }
                json!({"LicenseStatus": int(&p["LicenseStatus"]).unwrap_or(0), "Channel": ch, "Addon": truthy(&p["LicenseIsAddon"])})
            })
            .collect();
        let svc = w.query("SoftwareLicensingService", &["OA3xOriginalProductKey", "OA3xOriginalProductKeyDescription"])?.into_iter().next().unwrap_or(Value::Null);
        let present = !text(&svc["OA3xOriginalProductKey"]).trim().is_empty();
        Ok((list, json!({"Present": present, "Description": text(&svc["OA3xOriginalProductKeyDescription"])})))
    };
    match read() {
        Ok((products, firmware)) => {
            r["Products"] = Value::Array(products);
            r["Firmware"] = firmware;
        }
        Err(e) => r["Error"] = json!(e),
    }
    r
}

/// Get-JobSshFacts: the sshd service's start type (as `Get-Service` names
/// it) and the two authorized-keys files Windows' OpenSSH reads.
pub fn ssh_facts(cimv2: Option<&Wmi>) -> Value {
    let svc = cimv2.and_then(|w| w.query_where("Win32_Service", &["StartMode"], "Name='sshd'").ok()).and_then(|l| l.into_iter().next());
    let Some(svc) = svc else { return json!({"StartType": null, "KeyFiles": [], "ReadError": null}) };
    let start = match text(&svc["StartMode"]).as_str() {
        "Auto" => "Automatic".to_string(),
        other => other.to_string(),
    };
    let mut files = Vec::new();
    let profile = std::env::var("USERPROFILE").unwrap_or_default();
    let program_data = std::env::var("ProgramData").unwrap_or_default();
    for p in [format!("{profile}\\.ssh\\authorized_keys"), format!("{program_data}\\ssh\\administrators_authorized_keys")] {
        if !Path::new(&p).exists() {
            continue;
        }
        match std::fs::read_to_string(&p) {
            Ok(t) => files.push(json!({"Path": p, "Lines": t.lines().map(|l| l.trim_end_matches('\r')).collect::<Vec<_>>(), "Error": null})),
            Err(e) => files.push(json!({"Path": p, "Lines": [], "Error": e.to_string()})),
        }
    }
    json!({"StartType": start, "KeyFiles": files, "ReadError": null})
}

/// `(Get-WinUserLanguageList)[0].InputMethodTips[0]`: the first input
/// method of the first language in the person's list, from the registry.
pub fn input_tip() -> String {
    const UP: &str = r"Control Panel\International\User Profile";
    let Some((kind, bytes)) = registry::value(&Hive::CurrentUser, UP, "Languages") else { return String::new() };
    if kind != 7 {
        // REG_MULTI_SZ
        return String::new();
    }
    let units: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    let first: String = String::from_utf16_lossy(&units).split('\0').next().unwrap_or("").to_string();
    if first.is_empty() {
        return String::new();
    }
    let key = format!("{UP}\\{first}");
    // the input methods are values named "LANGID:KLID", their data the order
    let mut tips: Vec<(i64, String)> = registry::value_names(&Hive::CurrentUser, &key)
        .into_iter()
        .filter(|n| matches("^[0-9A-Fa-f]{4}:", n))
        .map(|n| (registry::dword(&Hive::CurrentUser, &key, &n).unwrap_or(i64::MAX), n))
        .collect();
    tips.sort();
    tips.into_iter().next().map(|(_, n)| n).unwrap_or_default()
}

/// The Store packages as `Get-AppxPackage -PackageTypeFilter Main` lists
/// them, with the manifest's own DisplayName and PublisherDisplayName (what
/// `Get-AppxPackageManifest` reads). `NonRemovable` is read as the package
/// being one of Windows' inbox applications (the registry list); the two
/// Store-signed packages Windows also marks non-removable on the G16 are
/// not in that list, so they are kept in the inventory here.
fn store_packages() -> Vec<Value> {
    use windows::ApplicationModel::PackageSignatureKind;
    use windows::Management::Deployment::{PackageManager, PackageTypes};
    const INBOX: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Appx\AppxAllUserStore\InboxApplications";
    let inbox: Vec<String> = registry::subkeys(&Hive::LocalMachine, INBOX).into_iter().map(|k| k.to_lowercase()).collect();
    let mut out = Vec::new();
    let Ok(pm) = PackageManager::new() else { return out };
    let Ok(list) = pm.FindPackagesByUserSecurityIdWithPackageTypes(&HSTRING::from(""), PackageTypes::Main) else { return out };
    for p in list {
        let Ok(id) = p.Id() else { continue };
        let name = id.Name().map(|h| h.to_string()).unwrap_or_default();
        let version = id.Version().map(|v| format!("{}.{}.{}.{}", v.Major, v.Minor, v.Build, v.Revision)).unwrap_or_default();
        let publisher = id.Publisher().map(|h| h.to_string()).unwrap_or_default();
        let full_name = id.FullName().map(|h| h.to_string()).unwrap_or_default();
        let kind = match p.SignatureKind().unwrap_or(PackageSignatureKind::None) {
            PackageSignatureKind::Developer => "Developer",
            PackageSignatureKind::Enterprise => "Enterprise",
            PackageSignatureKind::Store => "Store",
            PackageSignatureKind::System => "System",
            _ => "None",
        };
        let (mut display, mut publisher_display) = (String::new(), String::new());
        if let Ok(path) = p.InstalledPath() {
            if let Ok(xml) = std::fs::read_to_string(format!("{path}\\AppxManifest.xml")) {
                if let Ok(doc) = roxmltree::Document::parse(xml.trim_start_matches('\u{feff}')) {
                    let props = doc.root_element().children().find(|c| c.is_element() && c.tag_name().name() == "Properties");
                    let prop = |n: &str| props.and_then(|p| p.children().find(|c| c.is_element() && c.tag_name().name() == n)).and_then(|c| c.text()).unwrap_or("").to_string();
                    display = prop("DisplayName");
                    publisher_display = prop("PublisherDisplayName");
                }
            }
        }
        out.push(json!({"Name": name, "Version": version, "Publisher": publisher, "IsFramework": p.IsFramework().unwrap_or(false), "SignatureKind": kind,
                        "NonRemovable": inbox.contains(&full_name.to_lowercase()), "DisplayName": display, "PublisherDisplayName": publisher_display}));
    }
    out
}

/// Get-JobSoftware: the registry's Apps & features entries and the Store
/// packages, through `records::software`.
pub fn software() -> Value {
    let mut desktop = Vec::new();
    for (hive, key) in [
        (Hive::LocalMachine, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"),
        (Hive::LocalMachine, r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"),
        (Hive::CurrentUser, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"),
    ] {
        for sub in registry::subkeys(&hive, key) {
            let k = format!("{key}\\{sub}");
            let st = |n: &str| registry::string(&hive, &k, n).map_or(Value::Null, |v| json!(v));
            let sc = registry::dword(&hive, &k, "SystemComponent").map_or(Value::Null, |v| json!(v));
            desktop.push(json!({"DisplayName": st("DisplayName"), "DisplayVersion": st("DisplayVersion"), "Publisher": st("Publisher"), "SystemComponent": sc}));
        }
    }
    records::software(&Value::Array(desktop), &Value::Array(store_packages()), 2000)
}

/// Get-JobWlanProfiles: every saved profile with its password in clear
/// (`WLAN_PROFILE_GET_PLAINTEXT_KEY`; elevated), from the Native Wifi API,
/// one profile at a time by name.
pub fn wlan_profiles() -> Value {
    use windows::Win32::NetworkManagement::WiFi::{WlanCloseHandle, WlanEnumInterfaces, WlanFreeMemory, WlanGetProfile, WlanGetProfileList, WlanOpenHandle, WLAN_INTERFACE_INFO_LIST, WLAN_PROFILE_GET_PLAINTEXT_KEY, WLAN_PROFILE_INFO_LIST};
    const ERROR_SERVICE_NOT_ACTIVE: u32 = 1062;
    let (mut negotiated, mut handle) = (0u32, HANDLE::default());
    let rc = unsafe { WlanOpenHandle(2, None, &mut negotiated, &mut handle) };
    if rc == ERROR_SERVICE_NOT_ACTIVE {
        return json!({"Present": false, "Profiles": [], "Error": null});
    }
    if rc != 0 {
        return json!({"Present": true, "Profiles": [], "Error": format!("Windows' Wi-Fi interface answered error {rc}")});
    }
    let mut profiles = Vec::new();
    let mut error: Option<u32> = None;
    unsafe {
        let mut ifaces: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
        let rc = WlanEnumInterfaces(handle, None, &mut ifaces);
        if rc != 0 {
            error = Some(rc);
        } else {
            let n = (*ifaces).dwNumberOfItems as usize;
            let first = (*ifaces).InterfaceInfo.as_ptr();
            'all: for i in 0..n {
                let info = &*first.add(i);
                let guid: GUID = info.InterfaceGuid;
                let mut list: *mut WLAN_PROFILE_INFO_LIST = std::ptr::null_mut();
                let rc = WlanGetProfileList(handle, &guid, None, &mut list);
                if rc != 0 {
                    error = Some(rc);
                    break;
                }
                let np = (*list).dwNumberOfItems as usize;
                let pfirst = (*list).ProfileInfo.as_ptr();
                for j in 0..np {
                    let pi = &*pfirst.add(j);
                    let end = pi.strProfileName.iter().position(|u| *u == 0).unwrap_or(pi.strProfileName.len());
                    let name = String::from_utf16_lossy(&pi.strProfileName[..end]);
                    let mut flags: u32 = WLAN_PROFILE_GET_PLAINTEXT_KEY;
                    let mut access: u32 = 0;
                    let mut xml = PWSTR::null();
                    let rc = WlanGetProfile(handle, &guid, PCWSTR(pi.strProfileName.as_ptr()), None, &mut xml, Some(&mut flags), Some(&mut access));
                    if rc != 0 {
                        error = Some(rc);
                        WlanFreeMemory(list as *const _);
                        break 'all;
                    }
                    let text = xml.to_string().unwrap_or_default();
                    WlanFreeMemory(xml.0 as *const _);
                    profiles.push(json!({"Interface": format!("{guid:?}").to_lowercase(), "Name": name, "Xml": text}));
                }
                WlanFreeMemory(list as *const _);
            }
            WlanFreeMemory(ifaces as *const _);
        }
        WlanCloseHandle(handle, None);
    }
    match error {
        Some(rc) => json!({"Present": true, "Profiles": [], "Error": format!("Windows' Wi-Fi interface answered error {rc}")}),
        None => json!({"Present": true, "Profiles": profiles, "Error": null}),
    }
}

/// Get-JobWlanStoredCount: the second, independent count, the profile
/// files Windows keeps on disk (only real network profiles count).
pub fn wlan_stored_count() -> i64 {
    let root = PathBuf::from(format!("{}\\Microsoft\\Wlansvc\\Profiles\\Interfaces", std::env::var("ProgramData").unwrap_or_default()));
    let mut n = 0;
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("xml")) {
                if let Ok(t) = std::fs::read_to_string(&p) {
                    if wifi::profile_row(t.trim_start_matches('\u{feff}')).is_some() {
                        n += 1;
                    }
                }
            }
        }
    }
    n
}

/// Export-JobWifi: read, judge, write the password files under `out_dir`.
/// On any refusal or write failure nothing is left behind.
pub fn export_wifi(out_dir: &Path) -> Result<Value, String> {
    let dir = out_dir.join("artifacts").join("credentials").join("wifi");
    if dir.exists() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    let (block, files): (Value, Vec<SecretFile>) = wifi::harvest_wifi(&wlan_profiles(), wlan_stored_count(), wifi::WIFI_DIR)?;
    let write = || -> Result<(), String> {
        for f in &files {
            let p = out_dir.join(f.rel.replace('/', "\\"));
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            std::fs::write(&p, &f.xml).map_err(|e| e.to_string())?;
        }
        Ok(())
    };
    if let Err(e) = write() {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(format!("the Wi-Fi passwords could not be written to the stick ({e})"));
    }
    Ok(block)
}

/// Get-JobFacts. `scan_dir`: the reports folder; `stick_drive`: the
/// stick's letter (`E:`). An error is what the script would have thrown.
pub fn job_facts(scan_dir: Option<&Path>, stick_drive: Option<&str>, materialize: bool) -> Result<Value, String> {
    let cimv2 = Wmi::connect(CIMV2).ok();
    let storage = storage_wmi().ok();
    let mut f = serde_json::Map::new();
    let one = |w: Option<&Wmi>, class: &str, props: &[&str]| w.and_then(|w| w.query(class, props).ok()).and_then(|l| l.into_iter().next()).unwrap_or(Value::Null);
    let cs = one(cimv2.as_ref(), "Win32_ComputerSystem", &["Manufacturer", "Model"]);
    let os = one(cimv2.as_ref(), "Win32_OperatingSystem", &["Caption", "BuildNumber"]);
    let bios = one(cimv2.as_ref(), "Win32_BIOS", &["SerialNumber", "SMBIOSBIOSVersion"]);
    let sys = one(cimv2.as_ref(), "Win32_ComputerSystemProduct", &["UUID"]);
    f.insert("Vendor".into(), json!(str_of(&cs["Manufacturer"])));
    f.insert("Model".into(), json!(str_of(&cs["Model"])));
    f.insert("Uuid".into(), json!(str_of(&sys["UUID"])));
    f.insert("BiosSerial".into(), json!(str_of(&bios["SerialNumber"])));
    f.insert("BiosVersion".into(), json!(str_of(&bios["SMBIOSBIOSVersion"])));
    f.insert("OsCaption".into(), json!(str_of(&os["Caption"])));
    f.insert("OsBuild".into(), json!(int(&os["BuildNumber"]).unwrap_or(0)));
    f.insert("Firmware".into(), json!(std::env::var("firmware_type").unwrap_or_default()));
    f.insert("SecureBoot".into(), json!(match registry::dword(&Hive::LocalMachine, upgrade_scan::collect::SECURE_BOOT, "UEFISecureBootEnabled") { Some(1) => "on", Some(0) => "off", _ => "unknown" }));

    // Get-Partition C: and Get-Disk, both -ErrorAction Stop: the script dies without them
    let storage_ref = storage.as_ref().ok_or_else(|| "the Storage API could not be reached".to_string())?;
    let part = storage_ref.query_where("MSFT_Partition", &["ObjectId", "DiskNumber", "Size"], "DriveLetter='C'")?.into_iter().next().ok_or_else(|| "No MSFT_Partition objects found with property 'DriveLetter' equal to 'C'. Verify the value of the property and retry.".to_string())?;
    let disk_number = int(&part["DiskNumber"]).unwrap_or(-1);
    let disk = storage_ref.query_where("MSFT_Disk", &["Number", "SerialNumber", "UniqueId", "FriendlyName", "Size", "PartitionStyle", "BusType"], &format!("Number={disk_number}"))?.into_iter().next().ok_or_else(|| format!("No MSFT_Disk objects found with property 'Number' equal to '{disk_number}'. Verify the value of the property and retry."))?;
    f.insert("Disk".into(), json!({"Number": disk_number, "Serial": serial(&disk["SerialNumber"]), "UniqueId": str_of(&disk["UniqueId"]), "Name": str_of(&disk["FriendlyName"]), "Size": int(&disk["Size"]).unwrap_or(0), "Style": partition_style(&disk["PartitionStyle"])}));
    let physical = storage_ref.query("MSFT_PhysicalDisk", &["DeviceId", "HealthStatus", "OperationalStatus", "MediaType"]).unwrap_or_default();
    let pd_for = |n: i64| physical.iter().find(|p| text(&p["DeviceId"]) == n.to_string());
    let pd = pd_for(disk_number);
    f.insert("Health".into(), json!(pd.map_or("Unknown".to_string(), |p| health_status(&p["HealthStatus"]))));
    // every disk Windows sees, for the erase job's list (R27)
    let all: Vec<Value> = storage_ref
        .query("MSFT_Disk", &["Number", "SerialNumber", "UniqueId", "FriendlyName", "Size", "BusType"])
        .unwrap_or_default()
        .iter()
        .map(|d| {
            let n = int(&d["Number"]).unwrap_or(-1);
            json!({"Number": n, "Serial": serial(&d["SerialNumber"]), "UniqueId": str_of(&d["UniqueId"]), "Size": int(&d["Size"]).unwrap_or(0), "Name": str_of(&d["FriendlyName"]),
                   "Bus": bus_type(&d["BusType"]), "Health": pd_for(n).map_or("Unknown".to_string(), |p| health_status(&p["HealthStatus"]))})
        })
        .collect();
    f.insert("AllDisks".into(), Value::Array(all));
    f.insert("Operational".into(), json!(pd.map_or("unknown".to_string(), |p| operational_status(&p["OperationalStatus"], false))));
    f.insert("MediaType".into(), json!(pd.map_or(String::new(), |p| media_type(&p["MediaType"]))));

    // the shrink measurement, the same query Disk Management uses
    let measure_start = collect::now().0;
    let (mut shrink_gb, mut shrink_error) = (Value::Null, Value::Null);
    match storage_ref.call("MSFT_Partition", &text(&part["__RELPATH"]), "GetSupportedSize", &["ReturnValue", "SizeMin", "SizeMax", "ExtendedStatus"]) {
        Ok(r) if int(&r["ReturnValue"]) == Some(0) => match (num(&part["Size"]), num(&r["SizeMin"])) {
            (Some(size), Some(min)) => shrink_gb = json!(round1((size - min) / GB)),
            _ => shrink_error = json!("the Storage API returned no minimum size"),
        },
        Ok(r) => shrink_error = json!(one_line(&storage_error(&r))),
        Err(e) => shrink_error = json!(one_line(&e)),
    }
    f.insert("ShrinkGB".into(), shrink_gb.clone());
    f.insert("ShrinkError".into(), shrink_error);
    f.insert("Dirty".into(), json!(run_tool("fsutil", &["dirty", "query", "C:"], 60).map(|l| fsutil_dirty(&l).to_string()).unwrap_or_else(|_| "unknown".into())));
    let rq = repair_queued(storage.as_ref());
    f.insert("RepairQueued".into(), json!(rq.queued));
    f.insert("RepairQueuedWhy".into(), json!(rq.why));
    f.insert("RepairStale".into(), json!(rq.stale));
    let mut last_unmovable = Value::Null;
    if let Some(gb) = num(&shrink_gb) {
        if gb < LINUX_MIN_GB {
            last_unmovable = last_unmovable_since(measure_start).map_or(Value::Null, |v| json!(v));
        }
    }
    f.insert("LastUnmovable".into(), last_unmovable);

    // the EFI system partition on the system disk
    let (mut esp_size, mut esp_free, mut esp_error) = (0i64, 0i64, Value::Null);
    match storage_ref.query_where("MSFT_Partition", &["ObjectId", "Size", "GptType"], &format!("DiskNumber={disk_number}")) {
        Ok(parts) => match parts.iter().find(|p| text(&p["GptType"]).eq_ignore_ascii_case(ESP_GUID)) {
            Some(esp) => {
                esp_size = int(&esp["Size"]).unwrap_or(0);
                match storage_ref.wql(&format!("ASSOCIATORS OF {{{}}} WHERE AssocClass=MSFT_PartitionToVolume", text(&esp["__RELPATH"])), &["SizeRemaining"]) {
                    Ok(v) if !v.is_empty() => esp_free = int(&v[0]["SizeRemaining"]).unwrap_or(0),
                    Ok(_) => esp_error = json!("No MSFT_Volume objects found for the partition. Verify the value of the property and retry."),
                    Err(e) => esp_error = json!(e),
                }
            }
            None => esp_error = json!("no EFI System Partition on the system disk"),
        },
        Err(e) => esp_error = json!(e),
    }
    f.insert("EspSize".into(), json!(esp_size));
    f.insert("EspFree".into(), json!(esp_free));
    f.insert("EspError".into(), esp_error);

    // BitLocker: the WMI provider, then manage-bde's text
    let mut bitlocker = "unknown".to_string();
    if let Ok(w) = Wmi::connect(BITLOCKER) {
        if let Ok(l) = w.query_where("Win32_EncryptableVolume", &["ProtectionStatus"], "DriveLetter='C:'") {
            if let Some(v) = l.first() {
                bitlocker = match int(&v["ProtectionStatus"]) { Some(1) => "on", Some(0) => "off", _ => "unknown" }.to_string();
            }
        }
    }
    if bitlocker == "unknown" {
        if let Ok(lines) = run_tool("manage-bde", &["-status", "C:"], 60) {
            if let Some(m) = upgrade_scan::ps::capture(r"^\s*Protection Status:\s*Protection (On|Off)\s*$", "m", &lines.join("\n")) {
                bitlocker = m.to_lowercase();
            }
        }
    }
    f.insert("BitLocker".into(), json!(bitlocker));

    // the scanner's report
    f.insert("Verdict".into(), Value::Null);
    f.insert("RequiredKernel".into(), Value::Null);
    f.insert("Report".into(), Value::Null);
    f.insert("FailedChecks".into(), json!([]));
    f.insert("WarnChecks".into(), json!([]));
    f.insert("Releases".into(), json!([]));
    if let Some(dir) = scan_dir.filter(|d| d.exists()) {
        if let Some(p) = newest_report(dir) {
            if let Some(r) = read_json(&p) {
                f.insert("Verdict".into(), json!(str_of(&r["Verdict"]["Level"])));
                f.insert("RequiredKernel".into(), r["RequiredKernel"].clone());
                f.insert("Report".into(), json!(p.to_string_lossy()));
                let checks = r["Checks"].as_array().cloned().unwrap_or_default();
                f.insert("FailedChecks".into(), json!(checks.iter().filter(|c| c["Status"] == "fail" && c["Section"] != "Software").map(|c| str_of(&c["Title"])).collect::<Vec<_>>()));
                f.insert("WarnChecks".into(), json!(checks.iter().filter(|c| c["Status"] == "warn").map(|c| str_of(&c["Title"])).collect::<Vec<_>>()));
                f.insert("Releases".into(), r["Releases"].clone());
            }
        }
    }

    // the stick: its release.json and its disk's identity
    // StickError is only there when the read failed, as in the script
    f.insert("Stick".into(), Value::Null);
    f.insert("KitRelease".into(), Value::Null);
    if let Some(drive) = stick_drive {
        let root = format!("{}\\", drive.trim_end_matches('\\'));
        if let Some(v) = read_json(Path::new(&format!("{root}release.json"))) {
            f.insert("KitRelease".into(), v);
        }
        let letter = drive.trim_end_matches([':', '\\']).to_string();
        let read = || -> Result<Value, String> {
            let not_found = |class: &str| format!("No {class} objects found with property 'DriveLetter' equal to '{letter}'. Verify the value of the property and retry.");
            let sv = storage_ref.query_where("MSFT_Volume", &["FileSystemLabel"], &format!("DriveLetter='{letter}'"))?.into_iter().next().ok_or_else(|| not_found("MSFT_Volume"))?;
            let sp = storage_ref.query_where("MSFT_Partition", &["DiskNumber"], &format!("DriveLetter='{letter}'"))?.into_iter().next().ok_or_else(|| not_found("MSFT_Partition"))?;
            let n = int(&sp["DiskNumber"]).unwrap_or(-1);
            let sd = storage_ref.query_where("MSFT_Disk", &["UniqueId", "SerialNumber", "Size", "FriendlyName", "BusType"], &format!("Number={n}"))?.into_iter().next().ok_or_else(|| format!("No MSFT_Disk objects found with property 'Number' equal to '{n}'. Verify the value of the property and retry."))?;
            Ok(json!({"UniqueId": str_of(&sd["UniqueId"]), "Serial": serial(&sd["SerialNumber"]), "Size": int(&sd["Size"]).unwrap_or(0), "Name": str_of(&sd["FriendlyName"]), "Label": str_of(&sv["FileSystemLabel"]), "Bus": bus_type(&sd["BusType"])}))
        };
        match read() {
            Ok(v) => {
                f.insert("Stick".into(), v);
            }
            Err(e) => {
                f.insert("StickError".into(), json!(e));
            }
        }
    }

    let clock = clock_facts();
    f.insert("WindowsTz".into(), clock["WindowsZone"].clone());
    f.insert("Locale".into(), json!(system_locale()));
    f.insert("Clock".into(), clock);
    f.insert("License".into(), license_facts(cimv2.as_ref()));
    f.insert("Ssh".into(), ssh_facts(cimv2.as_ref()));
    f.insert("InputTip".into(), json!(input_tip()));
    f.insert("Software".into(), software());
    let (map, _) = upgrade_harvest::live::folder_map(stick_drive, materialize, 600, &collect::now().1);
    f.insert("Harvest".into(), map);
    f.insert("HarvestError".into(), Value::Null);
    let user = std::env::var("USERNAME").unwrap_or_default();
    f.insert("UserName".into(), json!(user));
    let full = cimv2.as_ref().and_then(|w| w.query_where("Win32_UserAccount", &["FullName"], &format!("Name='{user}' AND LocalAccount=True")).ok()).and_then(|l| l.into_iter().next()).map(|a| a["FullName"].clone()).unwrap_or(Value::Null);
    f.insert("FullName".into(), if text(&full).is_empty() { Value::Null } else { full });
    Ok(Value::Object(f))
}

/// `Get-WinSystemLocale`'s name (`en-US`).
fn system_locale() -> String {
    use windows::Win32::Globalization::GetSystemDefaultLocaleName;
    let mut buf = [0u16; 85];
    let n = unsafe { GetSystemDefaultLocaleName(&mut buf) };
    if n <= 1 { String::new() } else { String::from_utf16_lossy(&buf[..(n as usize - 1)]) }
}

/// Get-JobLastUnmovable: Defrag event 259 since the measurement; if none,
/// diskpart's `shrink querymax` (which only reports) makes Windows write one.
fn last_unmovable_since(since: Stamp) -> Option<String> {
    let read = || {
        events::query("Application", &format!("*[System[Provider[@Name='Microsoft-Windows-Defrag'] and (EventID=259) and {}]]", events::within(600)))
            .ok()?
            .into_iter()
            .find(|e| e.time_local >= since)
            .and_then(|e| defrag_259(&e.message))
    };
    read().or_else(|| {
        let script = std::env::temp_dir().join(format!("upgrade-job-diskpart-{}.txt", std::process::id()));
        if std::fs::write(&script, "select volume C\r\nshrink querymax\r\n").is_ok() {
            let _ = run_tool("diskpart.exe", &["/s", &script.to_string_lossy()], 60);
            let _ = std::fs::remove_file(&script);
        }
        std::thread::sleep(std::time::Duration::from_secs(3));
        read()
    })
}

/// The words `New-Job.ps1` prints after a job is written, from the job and
/// the facts (`{0:N2}` and the rest as PowerShell prints them).
pub fn summary_lines(j: &Value, facts: &Value, job_path: &Path, verify_only: bool) -> Vec<String> {
    use upgrade_scan::ps::fmt_n;
    let mb = 1048576.0;
    let mut l = Vec::new();
    let st = |p: &str| s(at(j, p));
    let n1 = |x: f64| fmt_n(round1(x), 1);
    l.push(String::new());
    l.push(format!("  job {}", st("job_id")));
    l.push(format!("  {} {}   disk {} ({} GB)   Secure Boot {}   BitLocker {}", st("identity.vendor"), st("identity.model"), st("identity.system_disk.friendly_name"), n1(vint(at(j, "identity.system_disk.size_bytes")) as f64 / 1e9), st("identity.secure_boot"), st("harvest.bitlocker.status")));
    l.push(format!("  verdict {}   disk health {}   shrinkable {} GB   ESP free {} MB   volume {}", st("scan.verdict"), st("storage.physical_disk.health_status"), st("storage.shrinkable_gb"), n1(vint(at(j, "storage.esp.free_bytes")) as f64 / mb), st("storage.volume_health.dirty")));
    let lu = st("storage.last_unmovable_file");
    if !lu.is_empty() {
        let tail = if upgrade_scan::parse::shrink_mitigable(&lu) { " - the prologue turns it off and measures again" } else { "" };
        l.push(format!("  Windows names the last unmovable file: {lu}{tail}"));
    }
    let stale = s(at(facts, "RepairStale"));
    if !stale.is_empty() {
        l.push(format!("  {stale} - not a queued repair"));
    }
    let shrink = at(j, "storage.shrinkable_gb");
    if st("intent.path") == "keep-windows" && (shrink.is_null() || num(shrink).is_some_and(|g| g < LINUX_MIN_GB)) {
        let choice = if st("fork.if_cannot_keep") == "stop" { "stops, as you chose" } else { "takes the clean slate you chose" };
        l.push(format!("  not enough room measured yet: the prologue measures again before it changes anything, and if Linux still does not fit it {choice}"));
    }
    l.push(format!("  path {} ({})   desktop {}, starts at the {}   locale {} {} {}", st("intent.path"), st("intent.path_reason"), st("intent.desktop"), st("intent.start_at"), st("intent.locale.lang"), st("intent.locale.keymap"), st("intent.locale.timezone")));
    l.push(format!("  stick {} {} GB '{}'", st("stick.friendly_name"), n1(vint(at(j, "stick.size_bytes")) as f64 / 1e9), st("stick.label")));
    if truthy(at(j, "risk_acknowledgement")) {
        let lifted: Vec<String> = at(j, "risk_acknowledgement.overrides").as_array().into_iter().flatten().map(s).collect();
        l.push(format!("  DATA LOSS ACCEPTED: the RED verdict was acknowledged; lifted: {}", lifted.join(", ")));
    }
    l.push(format!("  written: {}", job_path.display()));
    if !verify_only {
        l.push(String::new());
        l.push(format!("  Your Fedora sign-in:  user  {}   password  the one you just chose", st("intent.account.linux_name")));
    }
    let count = |p: &str| at(j, p).as_array().map_or(0, Vec::len);
    l.push(format!("  software inventory: {} desktop programs, {} Store apps (names only; stays on the stick)", count("harvest.software.desktop"), count("harvest.software.store")));
    if truthy(at(j, "erase_consent")) {
        l.push(String::new());
        l.push("  ERASE AND INSTALL: everything on these drives will be deleted, nothing is kept".to_string());
        for d in at(j, "erase_consent.disks").as_array().into_iter().flatten() {
            let role = s(at(d, "role"));
            let what = if role == "system" { "Fedora system" } else { "your home folder" };
            l.push(format!("    {:<7} {}  {} GB  serial {}  ({what})", role, s(at(d, "friendly_name")), fmt_n(vint(at(d, "size_bytes")) as f64 / 1e9, 1), s(at(d, "serial_number"))));
        }
        l.push("  In the installer a 2-minute countdown comes first: press any key there to cancel and go back to Windows.".to_string());
    }
    l.push(if truthy(at(j, "erase_consent")) { "  your folders now (all of these will be DELETED):" } else { "  your folders (settle-in copies these; a clean slate stages them to the stick):" }.to_string());
    for fo in at(j, "harvest.folders").as_array().into_iter().flatten() {
        if !truthy(at(fo, "exists")) {
            l.push(format!("    {:<10} not found", s(at(fo, "name"))));
            continue;
        }
        let od = if truthy(at(fo, "is_onedrive")) { "  (OneDrive)" } else { "" };
        l.push(format!("    {:<10} {:>8} GB  {:>7} files{od}  {}", s(at(fo, "name")), fmt_n(vint(at(fo, "bytes")) as f64 / GB, 2), vint(at(fo, "files")), s(at(fo, "path"))));
    }
    let sf = at(j, "harvest.stick_fit");
    let fit = if truthy(at(sf, "fits")) { "they fit".to_string() } else { format!("they do not fit: {}", s(at(facts, "Harvest.StickFit.Reason"))) };
    l.push(format!("  on the stick they would need {} GB; it has {} GB free ({}) - {fit}", fmt_n(vint(at(sf, "needed_bytes")) as f64 / GB, 2), fmt_n(vint(at(sf, "free_bytes")) as f64 / GB, 2), s(at(sf, "filesystem"))));
    match st("harvest.cloud_files.result").as_str() {
        "left-in-cloud" => l.push(format!("  OneDrive: {} online-only file(s) stay in OneDrive; they are not copied - after the conversion, sign in to OneDrive to reach them", st("harvest.cloud_files.placeholders_found"))),
        "materialized" => l.push(format!("  OneDrive: {} online-only file(s) downloaded and kept on this device", st("harvest.cloud_files.materialized"))),
        _ => {}
    }
    let others: Vec<String> = at(facts, "Harvest.Owner.OtherProfiles").as_array().into_iter().flatten().map(|o| s(at(o, "Path"))).collect();
    if !others.is_empty() {
        l.push(format!("  other accounts on this computer: {} - their files are not in this job (RISKS R5)", others.join(", ")));
    }
    match st("harvest.wifi.result").as_str() {
        "exported" => {
            let profiles: Vec<&Value> = at(j, "harvest.wifi.profiles").as_array().into_iter().flatten().collect();
            let ok = profiles.iter().filter(|p| truthy(at(p, "supported"))).count();
            l.push(format!("  Wi-Fi: {ok} saved network(s) Fedora will join by itself; their passwords are on the stick until the install ends"));
            for x in profiles.iter().filter(|p| !truthy(at(p, "supported"))) {
                l.push(format!("    not set up: {} - {}", s(at(x, "ssid")), s(at(x, "why_not"))));
            }
        }
        "no-wireless" => l.push("  Wi-Fi: this computer has no Wi-Fi".to_string()),
        "none-saved" => l.push("  Wi-Fi: no saved networks".to_string()),
        _ => {}
    }
    let rtc = if truthy(at(j, "harvest.clock.rtc_is_local")) { "local time (settle-in turns it to UTC on first startup)" } else { "UTC" };
    l.push(format!("  clock: {}, hardware clock in {rtc}", st("harvest.clock.iana")));
    let wl = at(j, "harvest.windows_license");
    if s(at(wl, "result")) == "read" {
        let ver = if truthy(at(wl, "windows_version")) { format!("Windows {}", s(at(wl, "windows_version"))) } else { "version unknown".to_string() };
        let act = if truthy(at(wl, "activated")) { "activated".to_string() } else { format!("NOT activated (status {})", s(at(wl, "license_status"))) };
        let fw = if truthy(at(wl, "firmware_key_present")) { format!("a key in the firmware ({})", s(at(wl, "firmware_key_description"))) } else { "no key in the firmware".to_string() };
        l.push(format!("  Windows: {ver} {}, {act}, channel {}, {fw} - kept for the way back to Windows; no key is copied", s(at(wl, "edition_id")), s(at(wl, "channel"))));
    } else {
        l.push(format!("  Windows licence: not read ({}) - the way back to Windows will know less", s(at(wl, "reason"))));
    }
    l.push("  not in this job: browsers, the BitLocker key".to_string());
    l.push(String::new());
    l
}
