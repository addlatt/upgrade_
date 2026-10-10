//! The harvester's live half, Windows only, following
//! `Harvest-UpgradeState.ps1` 0.3.0: the known folders (`Get-HarvestUserFolders`),
//! the stick's volume facts (`Get-HarvestStick`), whose desktop this is
//! (`Get-HarvestOwner`), the browsers (`Get-HarvestBrowsers`), and the folder
//! map the job writer reads (`-FolderMapOut`), in the script's field names.
//!
//! Read-only. The one step that is not a read, materializing cloud
//! placeholders, runs only when asked, as in the script, and no launcher
//! asks (decided 2026-09-26: online-only files stay in the cloud).
//!
//! Proven side by side: the script's `-FolderMapOut` and
//! `upgrade-harvest folder-map` on the same machine in the same minute,
//! then `upgrade-harvest compare-maps`.

use crate::folders::{folder_stats, materialize};
use crate::stick::{n2, stick_fit, Folder, RESERVE_BYTES};
use serde_json::{json, Value};
use upgrade_scan::collect::wmi::{Wmi, CIMV2, STORAGE};
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Security::Authentication::Identity::{GetUserNameExW, NameSamCompatible};
use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows::Win32::Security::{GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};
use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows::Win32::System::Threading::{GetCurrentProcess, GetCurrentProcessId, OpenProcessToken};
use windows::Win32::UI::Shell::{FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Music, FOLDERID_Pictures, FOLDERID_Videos, SHGetKnownFolderPath, KNOWN_FOLDER_FLAG};

const MAX_FILES: i64 = 250000;
const GB: f64 = 1073741824.0;
const DOWNLOADS_GUID: &str = "{374DE290-123F-4565-9164-39C4925E467B}";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn int(v: &Value) -> i64 {
    match v {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)).unwrap_or(0),
        Value::String(s) => s.trim().parse().unwrap_or(0),
        Value::Bool(b) => *b as i64,
        _ => 0,
    }
}

/// `[Environment]::GetFolderPath(...)`: the folder's path, or nothing when
/// the folder does not exist (the .NET call returns an empty string then,
/// and the shell call fails the same way).
fn known_folder(id: &windows::core::GUID) -> Option<String> {
    let p = unsafe { SHGetKnownFolderPath(id, KNOWN_FOLDER_FLAG(0), None) }.ok()?;
    let path = unsafe { p.to_string() }.ok();
    unsafe { CoTaskMemFree(Some(p.0 as *const std::ffi::c_void)) };
    path
}

/// The Downloads folder as the script reads it: the known-folder registry
/// value under its GUID, environment variables expanded. Nothing when the
/// value is not there (then the script lists no Downloads at all).
fn downloads_folder() -> Option<String> {
    let sub = wide(r"Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders");
    let name = wide(DOWNLOADS_GUID);
    let mut size: u32 = 0;
    let r = unsafe { RegGetValueW(HKEY_CURRENT_USER, PCWSTR(sub.as_ptr()), PCWSTR(name.as_ptr()), RRF_RT_REG_SZ, None, None, Some(&mut size)) };
    if r.is_err() || size == 0 {
        return None;
    }
    let mut buf: Vec<u16> = vec![0; (size as usize).div_ceil(2) + 1];
    let r = unsafe { RegGetValueW(HKEY_CURRENT_USER, PCWSTR(sub.as_ptr()), PCWSTR(name.as_ptr()), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as *mut _), Some(&mut size)) };
    if r.is_err() {
        return None;
    }
    let end = buf.iter().position(|u| *u == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..end]))
}

/// The six targets of Get-HarvestUserFolders, in its order; a path that
/// is empty means the folder is not there.
pub fn known_folders() -> Vec<(&'static str, String)> {
    let mut t = vec![
        ("Desktop", known_folder(&FOLDERID_Desktop).unwrap_or_default()),
        ("Documents", known_folder(&FOLDERID_Documents).unwrap_or_default()),
        ("Pictures", known_folder(&FOLDERID_Pictures).unwrap_or_default()),
        ("Music", known_folder(&FOLDERID_Music).unwrap_or_default()),
        ("Videos", known_folder(&FOLDERID_Videos).unwrap_or_default()),
    ];
    if let Some(d) = downloads_folder() {
        t.push(("Downloads", d));
    }
    t
}

/// Get-HarvestUserFolders, sized (`skip_sizes` false) or not, with the
/// on-stick size when `cluster_bytes` is above zero.
pub fn user_folders(skip_sizes: bool, cluster_bytes: i64) -> Vec<Value> {
    let mut out = Vec::new();
    for (name, path) in known_folders() {
        if path.is_empty() || !std::path::Path::new(&path).exists() {
            out.push(json!({"Name": name, "Path": path, "Exists": false, "IsOneDrive": false, "Files": 0, "Bytes": 0,
                            "CloudOnlyFiles": 0, "Truncated": false, "Unreadable": 0, "UnreadableFirst": [], "MaxFileBytes": 0, "FilesOver4GiB": 0, "StickBytes": 0}));
            continue;
        }
        if !skip_sizes {
            println!("  sizing {name}...");
        }
        let s = if skip_sizes { Default::default() } else { folder_stats(std::path::Path::new(&path), MAX_FILES, cluster_bytes, name) };
        out.push(json!({"Name": name, "Path": path, "Exists": true, "IsOneDrive": path.to_lowercase().contains("onedrive"),
                        "Files": s.files, "Bytes": s.bytes, "CloudOnlyFiles": s.cloud_only_files, "Truncated": s.truncated, "Unreadable": s.unreadable,
                        "UnreadableFirst": s.unreadable_first, "MaxFileBytes": s.max_file_bytes, "FilesOver4GiB": s.files_over_4gib, "StickBytes": s.stick_bytes}));
    }
    out
}

/// Get-HarvestStick: the stick volume's filesystem, cluster and free
/// space, read as `Get-Volume -DriveLetter` reads them (`MSFT_Volume`), so
/// an error says what the cmdlet would have said.
pub fn stick(drive: &str) -> Value {
    let letter = drive.trim_end_matches([':', '\\']).to_string();
    let label = format!("{letter}:");
    let failed = |why: String| json!({"Drive": label, "FileSystem": null, "ClusterBytes": 0, "FreeBytes": 0, "SizeBytes": 0, "Error": why.split_whitespace().collect::<Vec<_>>().join(" ")});
    if letter.len() != 1 || !letter.chars().all(|c| c.is_ascii_alphabetic()) {
        return failed(format!("Cannot process argument transformation on parameter 'DriveLetter'. Cannot convert value \"{letter}\" to type \"System.Char\". Error: \"String must be exactly one character long.\""));
    }
    let w = match Wmi::connect(STORAGE) {
        Ok(w) => w,
        Err(e) => return failed(e),
    };
    match w.query_where("MSFT_Volume", &["FileSystem", "AllocationUnitSize", "SizeRemaining", "Size"], &format!("DriveLetter='{letter}'")) {
        Ok(l) if !l.is_empty() => {
            let v = &l[0];
            json!({"Drive": label, "FileSystem": text(&v["FileSystem"]), "ClusterBytes": int(&v["AllocationUnitSize"]), "FreeBytes": int(&v["SizeRemaining"]), "SizeBytes": int(&v["Size"]), "Error": null})
        }
        Ok(_) => failed(format!("No MSFT_Volume objects found with property 'DriveLetter' equal to '{letter}'. Verify the value of the property and retry.")),
        Err(e) => failed(e),
    }
}

/// This process's account SID as text.
fn process_sid() -> Result<String, String> {
    let mut token = HANDLE::default();
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }.map_err(|e| format!("the process token: {e}"))?;
    let mut size: u32 = 0;
    let _ = unsafe { GetTokenInformation(token, TokenUser, None, 0, &mut size) };
    let mut buf: Vec<u8> = vec![0; size as usize];
    let r = unsafe { GetTokenInformation(token, TokenUser, Some(buf.as_mut_ptr() as *mut _), size, &mut size) };
    let _ = unsafe { CloseHandle(token) };
    r.map_err(|e| format!("the token's user: {e}"))?;
    let user = unsafe { &*(buf.as_ptr() as *const TOKEN_USER) };
    let mut s = PWSTR::null();
    unsafe { ConvertSidToStringSidW(user.User.Sid, &mut s) }.map_err(|e| format!("the SID as text: {e}"))?;
    let out = unsafe { s.to_string() }.map_err(|e| e.to_string());
    unsafe { windows::Win32::Foundation::LocalFree(Some(windows::Win32::Foundation::HLOCAL(s.0 as *mut _))) };
    out
}

/// `[Security.Principal.WindowsIdentity]::GetCurrent().Name`: `MACHINE\name`.
fn process_name() -> String {
    let mut size: u32 = 0;
    let _ = unsafe { GetUserNameExW(NameSamCompatible, None, &mut size) };
    let mut buf: Vec<u16> = vec![0; size as usize + 1];
    if unsafe { GetUserNameExW(NameSamCompatible, Some(PWSTR(buf.as_mut_ptr())), &mut size) } {
        String::from_utf16_lossy(&buf[..size as usize])
    } else {
        String::new()
    }
}

/// A CIM date (`20261007123012.123456-240`) as `.ToUniversalTime().ToString('o')`.
fn cim_to_utc_o(v: &Value) -> Value {
    let t = text(v);
    if t.len() < 25 || !t[..14].bytes().all(|b| b.is_ascii_digit()) {
        return Value::Null;
    }
    let n = |a: usize, b: usize| t[a..b].parse::<i64>().unwrap_or(0);
    let offset: i64 = t[21..25].parse().unwrap_or(0);
    let secs = upgrade_scan::ps::Stamp { year: n(0, 4) as i32, month: n(4, 6) as u32, day: n(6, 8) as u32, hour: n(8, 10) as u32, minute: n(10, 12) as u32, second: n(12, 14) as u32 }.seconds() - offset * 60;
    let fraction = &t[15..21];
    json!(format!("{}.{fraction}0Z", upgrade_scan::collect::utc_from_seconds(secs).iso()))
}

/// Get-HarvestOwner: whose folders these are. The known-folder paths
/// belong to the account this process runs as; the job writer compares it
/// with the owner of the desktop (explorer.exe) in this session, by SID,
/// and sees the machine's other profiles (R5).
pub fn owner() -> Value {
    let sid = process_sid().unwrap_or_default();
    let name = process_name();
    let mut session: u32 = 0;
    let _ = unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut session) };
    let mut owners: Vec<String> = Vec::new();
    let mut others: Vec<Value> = Vec::new();
    if let Ok(w) = Wmi::connect(CIMV2) {
        if let Ok(l) = w.query_where("Win32_Process", &["Handle", "SessionId"], "Name='explorer.exe'") {
            for p in l.iter().filter(|p| int(&p["SessionId"]) == session as i64) {
                if let Ok(r) = w.call("Win32_Process", &text(&p["__RELPATH"]), "GetOwnerSid", &["Sid", "ReturnValue"]) {
                    let s = text(&r["Sid"]);
                    if !s.is_empty() {
                        owners.push(s);
                    }
                }
            }
        }
        if let Ok(l) = w.query("Win32_UserProfile", &["SID", "LocalPath", "Special", "LastUseTime"]) {
            // S-1-5-21: local and domain accounts; S-1-12-1: Microsoft Entra (work or school) accounts
            for p in l {
                let psid = text(&p["SID"]);
                let special = matches!(p["Special"], Value::Bool(true));
                let kind = psid.starts_with("S-1-5-21-") || psid.starts_with("S-1-12-1-");
                if !special && psid != sid && kind {
                    others.push(json!({"Sid": psid, "Path": text(&p["LocalPath"]), "LastUseUtc": cim_to_utc_o(&p["LastUseTime"])}));
                }
            }
        }
    }
    owners.sort_by_key(|s| s.to_lowercase());
    owners.dedup_by_key(|s| s.to_lowercase());
    json!({"ProcessSid": sid, "ProcessName": name, "SessionId": session, "DesktopOwnerSids": owners, "OtherProfiles": others})
}

/// Get-HarvestBrowsers: the three profile folders, sized unless `skip_sizes`.
pub fn browsers(skip_sizes: bool) -> Vec<Value> {
    let appdata = std::env::var("APPDATA").unwrap_or_default();
    let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
    let candidates = [
        ("Firefox", format!("{appdata}\\Mozilla\\Firefox"), "~/.mozilla/firefox", true, "Bookmarks, history, extensions and saved passwords all transfer. The Firefox password store is cross-platform."),
        ("Chrome", format!("{local}\\Google\\Chrome\\User Data"), "~/.config/google-chrome", false, "Bookmarks, history and extensions transfer. Saved passwords do NOT - they are encrypted with Windows DPAPI, which has no Linux equivalent. Sign into Chrome sync or export passwords to CSV BEFORE converting."),
        ("Edge", format!("{local}\\Microsoft\\Edge\\User Data"), "~/.config/microsoft-edge", false, "Bookmarks, history and extensions transfer. Saved passwords do NOT - Windows DPAPI encryption. Export them or enable sync BEFORE converting."),
    ];
    let mut found = Vec::new();
    for (name, path, target, passwords_port, note) in candidates {
        if !std::path::Path::new(&path).exists() {
            continue;
        }
        if !skip_sizes {
            println!("  measuring {name} profile...");
        }
        let s = if skip_sizes { Default::default() } else { folder_stats(std::path::Path::new(&path), MAX_FILES, 0, "") };
        found.push(json!({"Name": name, "Path": path, "LinuxTarget": target, "PasswordsPort": passwords_port, "Note": note, "Bytes": s.bytes, "Files": s.files}));
    }
    found
}

/// The folder map as `-FolderMapOut` writes it, and whether the cloud
/// step refused (then the script exits 3). `now_utc_o` is the clock in
/// .NET's round-trip form.
pub fn folder_map(stick_drive: Option<&str>, materialize_placeholders: bool, timeout_secs: u64, now_utc_o: &str) -> (Value, bool) {
    let stick_facts = stick_drive.map(stick);
    let cluster = stick_facts.as_ref().map_or(0, |s| int(&s["ClusterBytes"]));
    let mut folders = user_folders(false, cluster);
    let found: i64 = folders.iter().map(|f| int(&f["CloudOnlyFiles"])).sum();
    let mut cloud = json!({"PlaceholdersFound": found, "Materialized": 0, "Failed": 0, "Bytes": 0, "Result": if found == 0 { "none-found" } else { "not-attempted" }, "FailedFiles": []});
    let before: Vec<(String, i64)> = folders.iter().map(|f| (text(&f["Name"]), int(&f["CloudOnlyFiles"]))).collect();
    if materialize_placeholders && found > 0 {
        println!("  cloud placeholders...");
        let paths: Vec<std::path::PathBuf> = folders.iter().filter(|f| f["Exists"] == true).map(|f| std::path::PathBuf::from(text(&f["Path"]))).collect();
        let refs: Vec<&std::path::Path> = paths.iter().map(|p| p.as_path()).collect();
        let m = materialize(&refs, timeout_secs, MAX_FILES);
        let failed: Vec<Value> = m.files.iter().filter(|f| !f.materialized).take(20).map(|f| json!({"Path": f.path, "Error": f.error})).collect();
        cloud = json!({"PlaceholdersFound": m.placeholders_found, "Materialized": m.materialized, "Failed": m.failed, "Bytes": m.bytes, "Result": m.result, "FailedFiles": failed});
        folders = user_folders(false, cluster);
    }
    // CloudOnlyFiles is what was online-only at harvest, before any
    // materialization (the job's cloud_only_files); CloudOnlyNow is what a
    // fresh read finds after it - anything above 0 there is a refusal.
    for f in folders.iter_mut() {
        let now = int(&f["CloudOnlyFiles"]);
        let name = text(&f["Name"]);
        let was = before.iter().find(|(n, _)| *n == name).map_or(0, |(_, c)| *c);
        f["CloudOnlyNow"] = json!(now);
        f["CloudOnlyFiles"] = json!(was);
    }
    let fit = match &stick_facts {
        Some(s) if s["Error"].is_null() => {
            let list: Vec<Folder> = folders.iter().map(|f| Folder { exists: f["Exists"] == true, bytes: int(&f["Bytes"]), stick_bytes: int(&f["StickBytes"]), files_over_4gib: int(&f["FilesOver4GiB"]) }).collect();
            let r = stick_fit(&list, int(&s["FreeBytes"]), &text(&s["FileSystem"]), int(&s["ClusterBytes"]), RESERVE_BYTES);
            json!({"FileSystem": r.file_system, "ClusterBytes": r.cluster_bytes, "FreeBytes": r.free_bytes, "FilesBytes": r.files_bytes, "NeededBytes": r.needed_bytes,
                   "FilesOver4GiB": r.files_over_4gib, "Fits": r.fits, "GapBytes": r.gap_bytes, "Reason": r.reason})
        }
        _ => Value::Null,
    };
    let present = folders.iter().filter(|f| f["Exists"] == true).count();
    let total: i64 = folders.iter().filter(|f| f["Exists"] == true).map(|f| int(&f["Bytes"])).sum();
    let stick_line = if fit.is_null() {
        String::new()
    } else if fit["Fits"] == true {
        "; stick: fits".to_string()
    } else {
        format!("; stick: does not fit - {}", text(&fit["Reason"]))
    };
    println!("  folder map: {present} folder(s), {} GB, {found} online-only file(s) found{stick_line}", n2(total as f64 / GB));
    let refused = cloud["Result"] == "refused";
    let map = json!({
        "HarvestVersion": crate::FOLLOWS_HARVESTER,
        "Harvester": format!("upgrade-harvest {} (Rust; follows Harvest-UpgradeState.ps1 {})", env!("CARGO_PKG_VERSION"), crate::FOLLOWS_HARVESTER),
        "HarvestedUtc": now_utc_o,
        "Owner": owner(),
        "UserFolders": folders,
        "CloudFiles": cloud,
        "Stick": stick_facts,
        "StickFit": fit,
    });
    (map, refused)
}
