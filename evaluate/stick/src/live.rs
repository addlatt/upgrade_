//! The writer's live half, Windows only: the disks as the Storage cmdlets
//! see them, and the one destructive path (Clear, Initialize, two
//! partitions, two formats, the copy, the read-back), following
//! Invoke-UpgStickWrite. The Storage namespace's classes are the cmdlets'
//! own (`MSFT_Disk`, `MSFT_Partition`, `MSFT_Volume`), called the way
//! `Get-Disk | Clear-Disk | New-Partition | Format-Volume` call them.

use crate::copy;
use crate::flow::{Live, WriteResult};
use crate::judge::{s, sha256sums};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use upgrade_scan::collect::storage::{bus_type, media_type, partition_style, storage_wmi};
use upgrade_scan::collect::wmi::{Wmi, CIMV2};
use upgrade_scan::collect::win;

pub struct WinLive {
    results_csv: Option<String>,
}

impl WinLive {
    pub fn new(results_csv: Option<String>) -> WinLive {
        WinLive { results_csv }
    }
}

fn text(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(t) => t.clone(),
        other => other.to_string(),
    }
}

fn u(v: &Value) -> i64 {
    v.as_i64().or_else(|| v.as_u64().map(|x| x as i64)).or_else(|| v.as_str().and_then(|t| t.parse().ok())).unwrap_or(0)
}

fn letter(v: &Value) -> Option<String> {
    match v {
        Value::String(t) if !t.trim_matches('\0').is_empty() => Some(t.trim_matches('\0').to_string()),
        Value::Number(n) => n.as_u64().filter(|c| *c > 0).and_then(|c| char::from_u32(c as u32)).map(|c| c.to_string()),
        _ => None,
    }
}

const DISK_PROPS: [&str; 13] = ["ObjectId", "Number", "FriendlyName", "UniqueId", "SerialNumber", "BusType", "Size", "PartitionStyle", "IsSystem", "IsBoot", "IsOffline", "IsReadOnly", "LargestFreeExtent"];

/// Get-UpgAttachedDisks.
pub fn attached_disks() -> Result<Vec<Value>, String> {
    let w = storage_wmi()?;
    let mut physical = std::collections::BTreeMap::new();
    for p in w.query("MSFT_PhysicalDisk", &["DeviceId", "MediaType"]).unwrap_or_default() {
        physical.insert(text(&p["DeviceId"]), media_type(&p["MediaType"]));
    }
    let mut sys_letters: Vec<String> = vec!["C".into()];
    if let Ok(c) = Wmi::connect(CIMV2) {
        for p in c.query("Win32_PageFileUsage", &["Name"]).unwrap_or_default() {
            if let Some(l) = text(&p["Name"]).chars().next() {
                sys_letters.push(l.to_string());
            }
        }
    }
    if let Some(l) = std::env::var("SystemDrive").ok().and_then(|d| d.chars().next()) {
        sys_letters.push(l.to_string());
    }
    let mut out = Vec::new();
    for d in w.query("MSFT_Disk", &DISK_PROPS)? {
        let number = u(&d["Number"]);
        let mut vols = Vec::new();
        for p in w.query_where("MSFT_Partition", &["DriveLetter", "Size"], &format!("DiskNumber={number}")).unwrap_or_default() {
            let l = letter(&p["DriveLetter"]);
            let v = l.as_ref().and_then(|l| w.query_where("MSFT_Volume", &["FileSystemLabel", "FileSystem"], &format!("DriveLetter='{l}'")).ok()).and_then(|list| list.into_iter().next());
            vols.push(json!({
                "DriveLetter": l,
                "Label": v.as_ref().map(|v| text(&v["FileSystemLabel"])),
                "FileSystem": v.as_ref().map(|v| text(&v["FileSystem"])),
                "SizeBytes": u(&p["Size"]),
            }));
        }
        let holds = vols.iter().any(|v| v["DriveLetter"].as_str().is_some_and(|l| sys_letters.iter().any(|x| x.eq_ignore_ascii_case(l))));
        out.push(json!({
            "Number": number, "FriendlyName": text(&d["FriendlyName"]), "UniqueId": text(&d["UniqueId"]),
            "SerialNumber": text(&d["SerialNumber"]).split_whitespace().collect::<String>(),
            "BusType": bus_type(&d["BusType"]), "MediaType": physical.get(&number.to_string()).cloned().unwrap_or_else(|| "unknown".into()),
            "SizeBytes": u(&d["Size"]), "PartitionStyle": if u(&d["PartitionStyle"]) == 0 { "RAW".to_string() } else { partition_style(&d["PartitionStyle"]) },
            "IsSystem": d["IsSystem"].as_bool().unwrap_or(false), "IsBoot": d["IsBoot"].as_bool().unwrap_or(false),
            "IsOffline": d["IsOffline"].as_bool().unwrap_or(false), "IsReadOnly": d["IsReadOnly"].as_bool().unwrap_or(false),
            "Volumes": vols, "HoldsSystemLetter": holds,
        }));
    }
    Ok(out)
}

fn call(w: &Wmi, class: &str, path: &str, method: &str, inputs: &[(&str, Value)], outs: &[&str]) -> Result<Value, String> {
    let r = w.call_with(class, path, method, inputs, outs)?;
    let rv = u(&r["ReturnValue"]);
    if rv != 0 {
        return Err(format!("{method} failed: {}", upgrade_scan::collect::storage::storage_error(&r)));
    }
    Ok(r)
}

fn disk_by_id(w: &Wmi, unique_id: &str) -> Result<Vec<Value>, String> {
    Ok(w.query("MSFT_Disk", &DISK_PROPS)?.into_iter().filter(|d| text(&d["UniqueId"]).eq_ignore_ascii_case(unique_id)).collect())
}

fn one_disk(w: &Wmi, unique_id: &str) -> Result<Value, String> {
    disk_by_id(w, unique_id)?.into_iter().next().ok_or_else(|| format!("the disk {unique_id} is gone"))
}

/// The partition with the highest number on a disk, with its letter.
fn newest_partition(w: &Wmi, number: i64) -> Result<Value, String> {
    let mut parts = w.query_where("MSFT_Partition", &["ObjectId", "PartitionNumber", "DriveLetter", "Size"], &format!("DiskNumber={number}"))?;
    parts.sort_by_key(|p| u(&p["PartitionNumber"]));
    parts.pop().ok_or_else(|| "no partition after creating one".to_string())
}

fn format_partition(w: &Wmi, partition: &Value, file_system: &str, label: &str) -> Result<(), String> {
    let vols = w.wql(&format!("ASSOCIATORS OF {{{}}} WHERE AssocClass=MSFT_PartitionToVolume", text(&partition["__RELPATH"])), &["ObjectId"])?;
    let vol = vols.first().ok_or("the new partition has no volume")?;
    call(w, "MSFT_Volume", &text(&vol["__RELPATH"]), "Format", &[("FileSystem", json!(file_system)), ("FileSystemLabel", json!(label)), ("Full", json!(false)), ("Force", json!(true))], &["ReturnValue", "ExtendedStatus"])?;
    Ok(())
}

const MIB: i64 = 1048576;
const GIB: i64 = 1073741824;

/// Invoke-UpgStickWrite: re-resolve by unique id NOW from a fresh list,
/// clear, MBR, FAT32 boot partition, exFAT staging partition, copy,
/// read every file back against SHA256SUMS.
pub fn write_stick(disk: &Value, source: &str, boot_label: &str, boot_gb: i64, host: &mut dyn FnMut(&str)) -> Result<WriteResult, String> {
    let w = storage_wmi()?;
    let id = s(disk, "UniqueId");
    let fresh = disk_by_id(&w, &id)?;
    if fresh.len() != 1 {
        return Err(format!("re-resolve: {} disks carry the id {id} now; refusing", fresh.len()));
    }
    let live = &fresh[0];
    if bus_type(&live["BusType"]) != "USB" || live["IsSystem"].as_bool().unwrap_or(false) || live["IsBoot"].as_bool().unwrap_or(false) {
        return Err("re-resolve: the disk no longer satisfies the rules; refusing".into());
    }
    if u(&live["Size"]) != crate::judge::int(disk, "SizeBytes") {
        return Err("re-resolve: the disk size changed; refusing".into());
    }
    // From the first destructive call on, the disk is written whatever happens next: an
    // error after it is a written, unverified stick, never "written n" (the G16, 2026-10-27:
    // the stick dropped off the bus mid-copy and the row said written n).
    let mut result = WriteResult { written: true, ..Default::default() };
    match write_cleared(&w, live, &id, source, boot_label, boot_gb, host, &mut result) {
        Ok(()) => Ok(result),
        Err(e) => {
            host(&format!("  ! {e}"));
            result.verified = false;
            result.notes.push(format!("stopped after the disk was cleared: {e}"));
            Ok(result)
        }
    }
}

/// The destructive half of `write_stick`, from the clear to the read-back.
#[allow(clippy::too_many_arguments)]
fn write_cleared(w: &Wmi, live: &Value, id: &str, source: &str, boot_label: &str, boot_gb: i64, host: &mut dyn FnMut(&str), result: &mut WriteResult) -> Result<(), String> {
    let id = id.to_string();
    host(&format!("  clearing disk {} ({})...", u(&live["Number"]), text(&live["FriendlyName"])));
    call(&w, "MSFT_Disk", &text(&live["__RELPATH"]), "Clear", &[("RemoveData", json!(true)), ("RemoveOEM", json!(true)), ("ZeroOutEntireDisk", json!(false))], &["ReturnValue", "ExtendedStatus"])?;
    let live = one_disk(&w, &id)?;
    // a real stick comes back MBR-initialized with no partitions, a VHDX RAW (2026-09-13)
    match u(&live["PartitionStyle"]) {
        0 => {
            call(&w, "MSFT_Disk", &text(&live["__RELPATH"]), "Initialize", &[("PartitionStyle", json!(1))], &["ReturnValue", "ExtendedStatus"])?;
        }
        1 => {}
        _ => {
            call(&w, "MSFT_Disk", &text(&live["__RELPATH"]), "ConvertStyle", &[("PartitionStyle", json!(1))], &["ReturnValue", "ExtendedStatus"])?;
        }
    }
    let live = one_disk(&w, &id)?;
    let number = u(&live["Number"]);
    if !w.query_where("MSFT_Partition", &["PartitionNumber"], &format!("DiskNumber={number}")).unwrap_or_default().is_empty() {
        return Err("the disk still holds partitions after Clear-Disk; refusing to write".into());
    }
    let size = u(&live["Size"]);
    let mut boot_bytes = boot_gb * GIB;
    if boot_bytes >= size - 64 * MIB {
        boot_bytes = size - 64 * MIB;
        result.notes.push("boot partition capped to the disk".into());
    }
    host(&format!("  boot partition: FAT32 {boot_label}, {} GiB...", upgrade_scan::ps::num(upgrade_scan::ps::round1(boot_bytes as f64 / GIB as f64))));
    // a 64-bit size travels as text through WMI
    call(&w, "MSFT_Disk", &text(&live["__RELPATH"]), "CreatePartition", &[("Size", json!(boot_bytes.to_string())), ("IsActive", json!(true)), ("AssignDriveLetter", json!(true))], &["ReturnValue", "ExtendedStatus"])?;
    let boot = newest_partition(&w, number)?;
    format_partition(&w, &boot, "FAT32", boot_label)?;
    result.boot_letter = letter(&boot["DriveLetter"]).or_else(|| newest_partition(&w, number).ok().and_then(|p| letter(&p["DriveLetter"]))).unwrap_or_default();
    let live = one_disk(&w, &id)?;
    let remaining = u(&live["LargestFreeExtent"]);
    if remaining > 64 * MIB {
        host(&format!("  staging partition: exFAT UPGDATA, {} GiB...", upgrade_scan::ps::num(upgrade_scan::ps::round1(remaining as f64 / GIB as f64))));
        call(&w, "MSFT_Disk", &text(&live["__RELPATH"]), "CreatePartition", &[("UseMaximumSize", json!(true)), ("AssignDriveLetter", json!(true))], &["ReturnValue", "ExtendedStatus"])?;
        let data = newest_partition(&w, number)?;
        format_partition(&w, &data, "exFAT", "UPGDATA")?;
        result.data_letter = newest_partition(&w, number).ok().and_then(|p| letter(&p["DriveLetter"])).unwrap_or_default();
    } else {
        result.notes.push("no room for a staging partition".into());
    }
    // copy, then verify by reading every file back against SHA256SUMS
    let root = PathBuf::from(format!("{}:\\", result.boot_letter));
    host(&format!("  copying {source} -> {} ...", root.display()));
    let src = Path::new(source);
    let mut files = Vec::new();
    walk(src, &mut files);
    for f in &files {
        let rel = f.strip_prefix(src).map_err(|e| e.to_string())?;
        let dest = root.join(rel);
        if let Some(d) = dest.parent() {
            std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
        }
        std::fs::copy(f, &dest).map_err(|e| format!("{} -> {}: {e}", f.display(), dest.display()))?;
    }
    let lines: Vec<String> = std::fs::read_to_string(src.join("SHA256SUMS")).map_err(|e| e.to_string())?.lines().map(|l| l.trim_end_matches('\r').to_string()).collect();
    let sums = sha256sums(&lines);
    for (rel, want) in &sums {
        let p = copy::under(&root, rel);
        if p.exists() && copy::sha256_file(&p).as_deref() == Some(want.as_str()) {
            result.files += 1;
        } else {
            result.failed.push(rel.clone());
        }
    }
    result.verified = result.failed.is_empty() && result.files as usize == sums.len() && !sums.is_empty();
    let live = one_disk(&w, &id)?;
    result.stick = json!({
        "unique_id": text(&live["UniqueId"]), "serial_number": text(&live["SerialNumber"]).split_whitespace().collect::<String>(), "size_bytes": u(&live["Size"]),
        "friendly_name": text(&live["FriendlyName"]), "label": boot_label, "manifest": "SHA256SUMS",
    });
    Ok(())
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        entries.sort();
        for p in entries {
            if p.is_dir() {
                walk(&p, out);
            } else {
                out.push(p);
            }
        }
    }
}

/// Resolve-ResultsCsv: the repository's file when this runs inside one,
/// else `%ProgramData%\upgrade_\r16\r16-stick-writer.csv`.
pub fn default_results_csv() -> String {
    let rel = Path::new("docs").join("validation-results").join("r16-stick-writer.csv");
    let mut starts: Vec<PathBuf> = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        starts.push(cwd);
    }
    if let Some(d) = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)) {
        starts.push(d);
    }
    for start in starts {
        let mut d = Some(start.as_path());
        while let Some(dir) = d {
            if dir.join(&rel).parent().is_some_and(Path::exists) && dir.join("CLAUDE.md").exists() {
                return dir.join(&rel).to_string_lossy().to_string();
            }
            d = dir.parent();
        }
    }
    format!("{}\\upgrade_\\r16\\r16-stick-writer.csv", std::env::var("ProgramData").unwrap_or_else(|_| "C:\\ProgramData".into()))
}

impl Live for WinLive {
    fn elevated(&self) -> bool {
        win::is_admin()
    }
    fn now_o(&self) -> String {
        upgrade_scan::collect::now().1
    }
    fn attached_disks(&mut self) -> Vec<Value> {
        match attached_disks() {
            Ok(l) => l,
            Err(e) => {
                eprintln!("  the disks could not be listed: {e}");
                Vec::new()
            }
        }
    }
    fn machine(&self) -> Value {
        let c = Wmi::connect(CIMV2).ok();
        let one = |class: &str, props: &[&str]| c.as_ref().and_then(|w| w.query(class, props).ok()).and_then(|l| l.into_iter().next()).unwrap_or(Value::Null);
        let cs = one("Win32_ComputerSystem", &["Manufacturer", "Model"]);
        let os = one("Win32_OperatingSystem", &["Caption", "BuildNumber"]);
        json!({"Manufacturer": text(&cs["Manufacturer"]), "Model": text(&cs["Model"]), "Caption": text(&os["Caption"]), "BuildNumber": text(&os["BuildNumber"])})
    }
    fn read_host(&mut self, prompt: &str) -> String {
        use std::io::Write;
        print!("{prompt}: ");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        let _ = std::io::stdin().read_line(&mut line);
        line.trim_end_matches(['\r', '\n']).to_string()
    }
    fn source_has_sums(&self, source: &str) -> bool {
        Path::new(source).join("SHA256SUMS").exists()
    }
    fn write_stick(&mut self, disk: &Value, source: &str, boot_label: &str, boot_gb: i64) -> Result<WriteResult, String> {
        write_stick(disk, source, boot_label, boot_gb, &mut |l| println!("{l}"))
    }
    fn results_csv(&self) -> String {
        self.results_csv.clone().unwrap_or_else(default_results_csv)
    }
    fn append_evidence(&mut self, csv: &str, line: &str) {
        use std::io::Write;
        let p = Path::new(csv);
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let new = !p.exists();
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
            if new {
                let _ = write!(f, "{}\r\n", crate::judge::CSV_HEADER);
            }
            let _ = write!(f, "{line}\r\n");
        }
    }
    fn out_file(&mut self, path: &str, text: &str) {
        let _ = std::fs::write(path, text.replace('\n', "\r\n"));
    }
    fn host(&mut self, line: &str) {
        println!("{line}");
    }
}

/// Copy-Kit.ps1: refresh a removable USB volume from the kit and eject it.
/// The previous run's records go to `save_to` first (default: a dated folder
/// beside the kit), read back, before anything is removed.
pub fn copy_kit(drive: &str, kit: Option<&str>, save_to: Option<&str>) -> Result<(), String> {
    let l = drive.trim_end_matches([':', '\\']).to_uppercase();
    let w = storage_wmi()?;
    let vol = w.query_where("MSFT_Volume", &["FileSystemLabel", "DriveType", "SizeRemaining"], &format!("DriveLetter='{l}'"))?.into_iter().next().ok_or_else(|| format!("No MSFT_Volume objects found with property 'DriveLetter' equal to '{l}'. Verify the value of the property and retry."))?;
    let part = w.query_where("MSFT_Partition", &["DiskNumber"], &format!("DriveLetter='{l}'"))?.into_iter().next().ok_or("the volume has no partition")?;
    let disk = w.query_where("MSFT_Disk", &["FriendlyName", "BusType", "Size"], &format!("Number={}", u(&part["DiskNumber"])))?.into_iter().next().ok_or("the partition has no disk")?;
    let bus = bus_type(&disk["BusType"]);
    let drive_type = match u(&vol["DriveType"]) {
        2 => "Removable",
        3 => "Fixed",
        0 => "Unknown",
        other => return Err(format!("{l}: is {bus}/drive type {other}, not a removable USB volume; refusing")),
    };
    if bus != "USB" || drive_type != "Removable" {
        return Err(format!("{l}: is {bus}/{drive_type}, not a removable USB volume; refusing"));
    }
    let kit_dir = kit.map(PathBuf::from).unwrap_or_else(default_kit);
    println!("  stick {l}: '{}' {} {} GB, {} GB free", text(&vol["FileSystemLabel"]), text(&disk["FriendlyName"]), upgrade_scan::ps::num(upgrade_scan::ps::round1(u(&disk["Size"]) as f64 / 1e9)), upgrade_scan::ps::fmt_n(u(&vol["SizeRemaining"]) as f64 / 1e9, 2).replace(',', ""));
    let root = PathBuf::from(format!("{l}:\\"));
    let save_dir = save_to.map(PathBuf::from).unwrap_or_else(|| {
        // 2026-10-27T14:53:39.4607517Z -> 20261027T145339Z (no ':' in a Windows name)
        let stamp: String = upgrade_scan::collect::now().1.chars().take(19).filter(|c| c.is_ascii_alphanumeric()).collect::<String>() + "Z";
        kit_dir.parent().unwrap_or(&kit_dir).join(format!("stick-saved-{stamp}"))
    });
    copy::refresh(&kit_dir, &root, &save_dir, &mut |line| println!("{line}"))?;
    match eject(&l) {
        Ok(()) => println!("  ejected {l}: - safe to unplug"),
        Err(e) => println!("  could not eject automatically ({e}); eject from the tray icon"),
    }
    Ok(())
}

fn default_kit() -> PathBuf {
    for start in [std::env::current_dir().ok(), std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf))].into_iter().flatten() {
        let mut d = Some(start.as_path());
        while let Some(dir) = d {
            let k = dir.join("dist").join("kit").join("stick");
            if k.join("SHA256SUMS").exists() {
                return k;
            }
            d = dir.parent();
        }
    }
    PathBuf::from("\\\\wsl.localhost\\Ubuntu\\home\\addlatt\\upgrade_\\dist\\kit\\stick")
}

/// Lock, dismount, allow removal, eject: what the shell's Eject verb does.
fn eject(letter: &str) -> Result<(), String> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE};
    use windows::Win32::Storage::FileSystem::{CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
    use windows::Win32::System::Ioctl::{FSCTL_DISMOUNT_VOLUME, FSCTL_LOCK_VOLUME, IOCTL_STORAGE_EJECT_MEDIA, IOCTL_STORAGE_MEDIA_REMOVAL, PREVENT_MEDIA_REMOVAL};
    use windows::Win32::System::IO::DeviceIoControl;
    let path = win::wide(&format!("\\\\.\\{letter}:"));
    unsafe {
        let h = CreateFileW(PCWSTR(path.as_ptr()), (GENERIC_READ | GENERIC_WRITE).0, FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING, FILE_FLAGS_AND_ATTRIBUTES(0), None).map_err(|e| e.message())?;
        let mut n = 0u32;
        let step = |code: u32, input: Option<*const core::ffi::c_void>, size: u32, n: &mut u32| DeviceIoControl(h, code, input, size, None, 0, Some(n), None);
        let r = (|| -> Result<(), String> {
            step(FSCTL_LOCK_VOLUME, None, 0, &mut n).map_err(|e| format!("lock: {}", e.message()))?;
            step(FSCTL_DISMOUNT_VOLUME, None, 0, &mut n).map_err(|e| format!("dismount: {}", e.message()))?;
            let allow = PREVENT_MEDIA_REMOVAL { PreventMediaRemoval: false.into() };
            step(IOCTL_STORAGE_MEDIA_REMOVAL, Some(&allow as *const _ as *const _), std::mem::size_of::<PREVENT_MEDIA_REMOVAL>() as u32, &mut n).map_err(|e| format!("allow removal: {}", e.message()))?;
            step(IOCTL_STORAGE_EJECT_MEDIA, None, 0, &mut n).map_err(|e| format!("eject: {}", e.message()))?;
            Ok(())
        })();
        let _ = CloseHandle(h);
        r
    }
}
