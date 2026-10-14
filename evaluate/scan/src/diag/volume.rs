//! `diag volume`: the read-only volume diagnostic that was the inline
//! PowerShell of `DIAG-VOLUME.cmd` (RISKS R18, for a machine whose C:
//! keeps NTFS's dirty flag through restarts). It reads the flag, what
//! autochk is set to do at boot, the last boot-time and online check
//! results Windows logged, NTFS and disk events, found.000, the volume's
//! own health, and writes one file to the stick:
//! `upgrade_\reports\volume-diag-<computer>.txt`. Changes nothing.
//!
//! The render is held to the launcher's lines word for word
//! (`tests/diag.rs`, `volume-golden.json`), including two habits of the
//! original worth knowing: the `Get-Volume` and `Get-PhysicalDisk` blocks
//! are PowerShell's own `Format-List` and `Format-Table` output, and an
//! NTFS or disk event whose message shrinks when its whitespace is
//! collapsed to fewer characters than the cut (300 or 200, or the
//! message's own length) is dropped, because the original cut the
//! collapsed text by the uncollapsed length and the `Substring` threw.

use super::{collapse, stamp_of};
use crate::ps::Stamp;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const DAYS: i64 = 30;
pub const NTFS_EVENTS: usize = 40;
pub const NTFS_CUT: usize = 300;
pub const DISK_EVENTS: usize = 30;
pub const DISK_CUT: usize = 200;
pub const DISK_PROVIDERS: [&str; 5] = ["disk", "storahci", "stornvme", "partmgr", "volmgr"];

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Found {
    #[serde(rename = "FullName")]
    pub full_name: String,
    /// local, `yyyy-MM-ddTHH:mm:ss`
    #[serde(rename = "LastWriteTime")]
    pub last_write_time: String,
    #[serde(rename = "Files")]
    pub files: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Volume {
    #[serde(rename = "DriveLetter")]
    pub drive_letter: String,
    #[serde(rename = "FileSystem")]
    pub file_system: String,
    #[serde(rename = "HealthStatus")]
    pub health_status: String,
    #[serde(rename = "OperationalStatus")]
    pub operational_status: String,
    #[serde(rename = "Size")]
    pub size: i64,
    #[serde(rename = "SizeRemaining")]
    pub size_remaining: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Event {
    /// local, `yyyy-MM-ddTHH:mm:ss`
    #[serde(rename = "TimeLocal")]
    pub time_local: String,
    #[serde(rename = "Provider", default)]
    pub provider: String,
    #[serde(rename = "Id", default)]
    pub id: i64,
    #[serde(rename = "Message")]
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PhysicalDisk {
    #[serde(rename = "DeviceId")]
    pub device_id: String,
    #[serde(rename = "FriendlyName")]
    pub friendly_name: String,
    #[serde(rename = "MediaType")]
    pub media_type: String,
    #[serde(rename = "HealthStatus")]
    pub health_status: String,
    #[serde(rename = "OperationalStatus")]
    pub operational_status: String,
    #[serde(rename = "Size")]
    pub size: i64,
}

/// What the diagnostic read.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Facts {
    /// UTC, .NET round-trip form
    #[serde(rename = "Now")]
    pub now: String,
    #[serde(rename = "Computer")]
    pub computer: String,
    #[serde(rename = "FsutilDirty")]
    pub fsutil_dirty: Vec<String>,
    #[serde(rename = "Chkntfs")]
    pub chkntfs: Vec<String>,
    /// the registry's multi-string, or nothing when the value is absent
    #[serde(rename = "BootExecute")]
    pub boot_execute: Option<Vec<String>>,
    #[serde(rename = "Found")]
    pub found: Vec<Found>,
    #[serde(rename = "Volume")]
    pub volume: Option<Volume>,
    /// `Repair-Volume -Scan`'s word, or nothing when it refused
    #[serde(rename = "RepairScan")]
    pub repair_scan: Option<String>,
    #[serde(rename = "NtfsInfo")]
    pub ntfs_info: Vec<String>,
    /// Application log, id 1001, any provider (the render keeps Wininit's)
    #[serde(rename = "Wininit")]
    pub wininit: Vec<Event>,
    #[serde(rename = "Chkdsk")]
    pub chkdsk: Vec<Event>,
    #[serde(rename = "Ntfs")]
    pub ntfs: Vec<Event>,
    /// System log, any provider (the render keeps the disk ones)
    #[serde(rename = "DiskEvents")]
    pub disk_events: Vec<Event>,
    #[serde(rename = "PhysicalDisks")]
    pub physical_disks: Vec<PhysicalDisk>,
    #[serde(rename = "HiberbootEnabled")]
    pub hiberboot_enabled: Option<i64>,
    #[serde(rename = "Hiberfil")]
    pub hiberfil: bool,
    /// local, `yyyy-MM-ddTHH:mm:ss`
    #[serde(rename = "LastBootUpTime")]
    pub last_boot_up_time: String,
}

/// `'[' + $dt + ']'`: PowerShell's `+` converts with the invariant culture,
/// `MM/dd/yyyy HH:mm:ss`, on any machine (unlike `"$dt"`, which is the
/// machine's own culture).
pub fn invariant(t: &Stamp) -> String {
    format!("{:02}/{:02}/{:04} {:02}:{:02}:{:02}", t.month, t.day, t.year, t.hour, t.minute, t.second)
}

fn when(text: &str) -> String {
    stamp_of(text).map(|t| invariant(&t)).unwrap_or_else(|| text.to_string())
}

/// `.Substring(0, [Math]::Min(cut, $_.Message.Length))` on the collapsed
/// message: the text, or nothing where the original threw.
fn cut(message: &str, limit: usize) -> Option<String> {
    let c = collapse(message);
    let n = limit.min(message.chars().count());
    if c.chars().count() < n {
        return None;
    }
    Some(c.chars().take(n).collect())
}

/// `Format-List` of the volume's six properties, as `Out-String` gives it.
pub fn format_list(v: &Volume) -> String {
    let rows = [("DriveLetter", v.drive_letter.clone()), ("FileSystem", v.file_system.clone()), ("HealthStatus", v.health_status.clone()), ("OperationalStatus", v.operational_status.clone()), ("Size", v.size.to_string()), ("SizeRemaining", v.size_remaining.to_string())];
    let width = rows.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
    let mut s = String::from("\r\n\r\n");
    for (k, val) in rows {
        s.push_str(&format!("{k:<width$} : {val}\r\n"));
    }
    s.push_str("\r\n\r\n\r\n");
    s
}

/// `Format-Table -AutoSize` of the physical disks, as `Out-String` gives
/// it: text columns left-aligned, the size right-aligned.
pub fn format_table(disks: &[PhysicalDisk]) -> String {
    if disks.is_empty() {
        return String::new();
    }
    let cols = ["DeviceId", "FriendlyName", "MediaType", "HealthStatus", "OperationalStatus", "Size"];
    let cell = |d: &PhysicalDisk, i: usize| match i {
        0 => d.device_id.clone(),
        1 => d.friendly_name.clone(),
        2 => d.media_type.clone(),
        3 => d.health_status.clone(),
        4 => d.operational_status.clone(),
        _ => d.size.to_string(),
    };
    let widths: Vec<usize> = (0..cols.len()).map(|i| disks.iter().map(|d| cell(d, i).chars().count()).chain(std::iter::once(cols[i].len())).max().unwrap_or(0)).collect();
    let line = |cells: Vec<String>| -> String {
        let mut parts = Vec::new();
        for (i, c) in cells.iter().enumerate() {
            parts.push(if i == 5 { format!("{c:>w$}", w = widths[i]) } else { format!("{c:<w$}", w = widths[i]) });
        }
        parts.join(" ").trim_end().to_string()
    };
    let mut s = String::from("\r\n");
    s.push_str(&line(cols.iter().map(|c| c.to_string()).collect()));
    s.push_str("\r\n");
    // the dashes run the header's length, aligned like the cells
    s.push_str(&line(cols.iter().map(|c| "-".repeat(c.len())).collect()));
    s.push_str("\r\n");
    for d in disks {
        s.push_str(&line((0..cols.len()).map(|i| cell(d, i)).collect()));
        s.push_str("\r\n");
    }
    s.push_str("\r\n\r\n");
    s
}

/// The file's elements (one may hold several lines), as the launcher's
/// `$o` array held them.
pub fn render(f: &Facts) -> Vec<String> {
    let mut o: Vec<String> = Vec::new();
    o.push(format!("== upgrade_ volume diagnostic {} {}", f.now, f.computer));
    o.push("-- fsutil dirty query C:".into());
    o.extend(f.fsutil_dirty.iter().cloned());
    o.push("-- chkntfs C:".into());
    o.extend(f.chkntfs.iter().cloned());
    o.push("-- BootExecute (what autochk runs at boot)".into());
    match &f.boot_execute {
        Some(l) => o.extend(l.iter().cloned()),
        None => o.push(String::new()),
    }
    o.push("-- found.*".into());
    for d in &f.found {
        o.push(format!("{}  {}  files={}", d.full_name, when(&d.last_write_time), d.files));
    }
    o.push("-- Get-Volume C:".into());
    o.push(f.volume.as_ref().map(format_list).unwrap_or_default());
    o.push("-- Repair-Volume -Scan (read-only)".into());
    o.push(f.repair_scan.clone().unwrap_or_default());
    o.push("-- fsutil fsinfo ntfsinfo C:".into());
    o.extend(f.ntfs_info.iter().cloned());
    o.push("-- Wininit 1001 (boot-time chkdsk output), last 30 days".into());
    for e in f.wininit.iter().filter(|e| e.provider.eq_ignore_ascii_case("Microsoft-Windows-Wininit")) {
        o.push(format!("[{}]", when(&e.time_local)));
        o.push(e.message.clone());
    }
    o.push("-- Chkdsk provider events (26212/26214/26226/26228 ...), last 30 days".into());
    for e in &f.chkdsk {
        o.push(format!("[{}] id={}", when(&e.time_local), e.id));
        o.push(e.message.clone());
    }
    o.push("-- Ntfs events (55 corruption, 98 mount, 130/131 ...), System log, last 30 days".into());
    for e in f.ntfs.iter().take(NTFS_EVENTS) {
        if let Some(m) = cut(&e.message, NTFS_CUT) {
            o.push(format!("[{}] id={} {m}", when(&e.time_local), e.id));
        }
    }
    o.push("-- disk events (System, provider disk/storahci/stornvme), last 30 days".into());
    for e in f.disk_events.iter().filter(|e| DISK_PROVIDERS.iter().any(|p| p.eq_ignore_ascii_case(&e.provider))).take(DISK_EVENTS) {
        if let Some(m) = cut(&e.message, DISK_CUT) {
            o.push(format!("[{}] {} id={} {m}", when(&e.time_local), e.provider, e.id));
        }
    }
    o.push("-- Get-PhysicalDisk".into());
    o.push(format_table(&f.physical_disks));
    o.push("-- Fast Startup (HiberbootEnabled) and hiberfil".into());
    o.push(format!("HiberbootEnabled={}  hiberfil={}", f.hiberboot_enabled.map(|v| v.to_string()).unwrap_or_default(), if f.hiberfil { "True" } else { "False" }));
    o.push("-- last boot / uptime".into());
    o.push(format!("LastBootUpTime={}", when(&f.last_boot_up_time)));
    o
}

/// The file's physical lines (an element with line ends inside is several).
pub fn lines(f: &Facts) -> Vec<String> {
    render(f).iter().flat_map(|e| e.split("\r\n").map(str::to_string).collect::<Vec<_>>()).collect()
}

/// `WriteAllLines` with UTF-8 and no byte order mark: every element then CRLF.
pub fn text(f: &Facts) -> String {
    render(f).iter().map(|e| format!("{e}\r\n")).collect()
}

/// `<stick>\upgrade_\reports\volume-diag-<COMPUTERNAME>.txt`.
pub fn default_out(root: &Path, computer: &str) -> PathBuf {
    root.join("upgrade_").join("reports").join(format!("volume-diag-{computer}.txt"))
}

/// A local stamp as the facts carry it.
pub fn local_text(t: &Stamp) -> String {
    t.iso()
}
