//! The SMART / drive diagnostic (`Diag-Smart.ps1`, RISKS R18, the Aspire,
//! 2026-09-13): every physical disk with its reliability counters, the
//! `Win32_DiskDrive` index map, the ATA SMART tables Windows exposes
//! (`root\wmi`, SATA drives; NVMe answers "Not supported"), the `disk`
//! events and the storage-stack events of the last 60 days, and the
//! lettered volumes.

use super::{collapse, en_us, hexbytes, stamp_of};
use crate::parse::smart_table;
use crate::ps::{n, num, round1};
use serde::{Deserialize, Serialize};

pub const DAYS: i64 = 60;
/// The storage-stack providers the script picked out of the System log.
pub const STORAGE_PROVIDERS: [&str; 6] = ["storahci", "stornvme", "Microsoft-Windows-Ntfs", "Ntfs", "volmgr", "partmgr"];
/// `Get-StorageReliabilityCounter`, run without elevation, says this.
pub const COUNTERS_NOT_ELEVATED: &str = "Access to a CIM resource was not available to the client.";

/// The attribute names the script printed; any other id is `vendor_<id>`.
pub fn attribute_name(id: u8) -> String {
    let name = match id {
        1 => "Raw_Read_Error_Rate",
        5 => "Reallocated_Sector_Ct",
        9 => "Power_On_Hours",
        12 => "Power_Cycle_Count",
        170 => "Available_Reserved_Space",
        171 => "Program_Fail_Count",
        172 => "Erase_Fail_Count",
        173 => "Wear_Leveling_Count",
        174 => "Unexpected_Power_Loss",
        177 => "Wear_Leveling_Count",
        179 => "Used_Rsvd_Blk_Cnt_Tot",
        181 => "Program_Fail_Cnt_Total",
        182 => "Erase_Fail_Count_Total",
        183 => "Runtime_Bad_Block",
        184 => "End-to-End_Error",
        187 => "Reported_Uncorrectable",
        188 => "Command_Timeout",
        190 => "Airflow_Temperature",
        194 => "Temperature",
        195 => "Hardware_ECC_Recovered",
        196 => "Reallocated_Event_Count",
        197 => "Current_Pending_Sector",
        198 => "Offline_Uncorrectable",
        199 => "UDMA_CRC_Error_Count",
        231 => "SSD_Life_Left",
        232 => "Available_Reservd_Space",
        233 => "Media_Wearout_Indicator",
        234 => "Thermal_Throttle",
        241 => "Total_LBAs_Written",
        242 => "Total_LBAs_Read",
        249 => "NAND_Writes",
        other => return format!("vendor_{other}"),
    };
    name.to_string()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Reliability {
    pub wear: Option<i64>,
    pub temperature: Option<i64>,
    pub temperature_max: Option<i64>,
    pub power_on_hours: Option<i64>,
    pub read_errors_uncorrected: Option<i64>,
    pub write_errors_uncorrected: Option<i64>,
    pub read_errors_total: Option<i64>,
    pub write_errors_total: Option<i64>,
    pub read_latency_max: Option<i64>,
    pub write_latency_max: Option<i64>,
    pub start_stop_cycle_count: Option<i64>,
    pub load_unload_cycle_count: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct PhysicalDisk {
    pub device_id: String,
    pub friendly_name: String,
    pub bus_type: String,
    pub media_type: String,
    pub health_status: String,
    pub operational_status: Vec<String>,
    pub size: Option<u64>,
    pub firmware_version: String,
    pub serial_number: String,
    /// nothing = the cmdlet returned no counters (every value prints empty)
    pub reliability: Option<Reliability>,
    /// the cmdlet's error, which replaces the counters line
    pub reliability_error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct DiskDrive {
    pub index: Option<i64>,
    pub model: String,
    pub interface_type: String,
    pub status: String,
    #[serde(rename = "PNPDeviceID")]
    pub pnp_device_id: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct StatusItem {
    pub instance_name: String,
    pub predict_failure: Option<bool>,
    pub reason: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct DataItem {
    pub instance_name: String,
    #[serde(with = "hexbytes")]
    pub vendor_specific: Option<Vec<u8>>,
}

/// One `Get-CimInstance` call: its error, or its instances.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Read<T> {
    pub error: Option<String>,
    pub items: Vec<T>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Event {
    /// local time with its ticks, `2026-10-03T09:01:02.1234567`; the text
    /// orders the events, the first 19 characters print
    pub time_local: String,
    pub provider: String,
    pub id: i64,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Volume {
    pub drive_letter: String,
    pub file_system_label: String,
    pub file_system: String,
    pub health_status: String,
    pub operational_status: String,
    pub size: Option<u64>,
    pub size_remaining: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Facts {
    pub now: String,
    pub computer: String,
    pub physical_disks: Vec<PhysicalDisk>,
    pub disk_drives: Vec<DiskDrive>,
    pub failure_predict_status: Read<StatusItem>,
    pub failure_predict_data: Read<DataItem>,
    pub failure_predict_thresholds: Read<DataItem>,
    /// the System log's `disk` events of the last 60 days, newest first
    pub disk_events: Vec<Event>,
    /// the storage providers' events of the last 60 days, newest first
    pub storage_events: Vec<Event>,
    /// the volumes with a drive letter
    pub volumes: Vec<Volume>,
}

/// `Short`: whitespace collapsed, cut at `max` characters.
pub fn short(message: &str, max: usize) -> String {
    let t = collapse(message);
    if t.chars().count() > max { t.chars().take(max).collect() } else { t }
}

fn gb(size: Option<u64>) -> String {
    num(round1(size.unwrap_or(0) as f64 / 1e9))
}

fn time(e: &Event) -> String {
    stamp_of(&e.time_local).map(|t| en_us(&t)).unwrap_or_default()
}

/// Events sorted oldest first (`Sort-Object TimeCreated`), the last `keep`
/// of them (`Select-Object -Last`).
fn last_sorted(events: &[Event], keep: usize) -> Vec<&Event> {
    let mut sorted: Vec<&Event> = events.iter().collect();
    sorted.sort_by(|a, b| a.time_local.cmp(&b.time_local));
    let skip = sorted.len().saturating_sub(keep);
    sorted.into_iter().skip(skip).collect()
}

/// `Group-Object`: the keys in the order each is first seen, with its members.
fn groups<'a, K: PartialEq>(events: &'a [Event], key: impl Fn(&Event) -> K) -> Vec<(K, Vec<&'a Event>)> {
    let mut out: Vec<(K, Vec<&Event>)> = Vec::new();
    for e in events {
        let k = key(e);
        match out.iter_mut().find(|(g, _)| *g == k) {
            Some((_, members)) => members.push(e),
            None => out.push((k, vec![e])),
        }
    }
    out
}

fn first_last(members: &[&Event]) -> (String, String) {
    let first = members.iter().min_by(|a, b| a.time_local.cmp(&b.time_local)).map(|e| time(e)).unwrap_or_default();
    let last = members.iter().max_by(|a, b| a.time_local.cmp(&b.time_local)).map(|e| time(e)).unwrap_or_default();
    (first, last)
}

/// The lines of the text file, in the script's order and words.
pub fn render(f: &Facts) -> Vec<String> {
    let mut o: Vec<String> = Vec::new();
    o.push(format!("== upgrade_ SMART diagnostic {} {}", f.now, f.computer));
    o.push("-- physical disks".into());
    for d in &f.physical_disks {
        o.push(format!(
            "  disk {}: {}  bus={} media={} health={} op={} size={} GB firmware={} serial={}",
            d.device_id, d.friendly_name, d.bus_type, d.media_type, d.health_status, d.operational_status.join(","), gb(d.size), d.firmware_version, d.serial_number
        ));
        match &d.reliability_error {
            Some(e) => o.push(format!("      reliability counters: {e}")),
            None => {
                let c = d.reliability.clone().unwrap_or_default();
                o.push(format!(
                    "      reliability: wear={} temp={}C maxtemp={}C poweron={}h readErr(uncorr)={} writeErr(uncorr)={} readErrTotal={} writeErrTotal={} readLatencyMax={}ms writeLatencyMax={}ms startStop={} loadUnload={}",
                    n(c.wear), n(c.temperature), n(c.temperature_max), n(c.power_on_hours), n(c.read_errors_uncorrected), n(c.write_errors_uncorrected), n(c.read_errors_total), n(c.write_errors_total), n(c.read_latency_max), n(c.write_latency_max), n(c.start_stop_cycle_count), n(c.load_unload_cycle_count)
                ));
            }
        }
    }
    o.push(r"-- Win32_DiskDrive (index N is the N in \Device\HarddiskN of the disk events below)".into());
    for d in &f.disk_drives {
        o.push(format!("  index {}: {}  iface={} status={} pnp={}", n(d.index), d.model, d.interface_type, d.status, d.pnp_device_id));
    }
    o.push(r"-- SMART failure prediction (root\wmi)".into());
    match &f.failure_predict_status.error {
        Some(e) => o.push(format!("  FailurePredictStatus: {e}")),
        None => {
            for s in &f.failure_predict_status.items {
                let predict = s.predict_failure.map(|b| if b { "True" } else { "False" }).unwrap_or("");
                o.push(format!("  {}: PredictFailure={} Reason={}", s.instance_name, predict, n(s.reason)));
            }
        }
    }
    match &f.failure_predict_data.error {
        Some(e) => o.push(format!("  FailurePredictData: {e}")),
        None => {
            for s in &f.failure_predict_data.items {
                o.push(format!("  attributes for {}:", s.instance_name));
                o.push(format!("    {:<4} {:<26} {:>5} {:>5} {:>8} {}", "id", "name", "cur", "worst", "flags", "raw"));
                for a in smart_table(s.vendor_specific.as_deref().unwrap_or(&[])) {
                    o.push(format!("    {:<4} {:<26} {:>5} {:>5} {:>8} {}  (low16={})", a.id, attribute_name(a.id), a.current, a.worst, format!("0x{:04x}", a.flags), a.raw, a.raw16));
                }
            }
        }
    }
    match &f.failure_predict_thresholds.error {
        Some(e) => o.push(format!("  FailurePredictThresholds: {e}")),
        None => {
            for s in &f.failure_predict_thresholds.items {
                let th: Vec<String> = smart_table(s.vendor_specific.as_deref().unwrap_or(&[])).iter().map(|a| format!("{}:{}", a.id, a.flags & 0xff)).collect();
                o.push(format!("  thresholds for {}: {}", s.instance_name, th.join(" ")));
            }
        }
    }
    o.push("-- 'disk' events (7 bad block, 51 paging, 153 reset, 157 removal), System log, last 60 days".into());
    if f.disk_events.is_empty() {
        o.push("  none".into());
    } else {
        for (id, members) in groups(&f.disk_events, |e| e.id) {
            let (first, last) = first_last(&members);
            o.push(format!("  id {id}: {} events, first {first}, last {last}", members.len()));
        }
        for e in last_sorted(&f.disk_events, 40) {
            o.push(format!("  [{}] id={} {}", time(e), e.id, short(&e.message, 160)));
        }
    }
    o.push("-- storahci / stornvme / Ntfs events, System log, last 60 days".into());
    if f.storage_events.is_empty() {
        o.push("  none".into());
    } else {
        for ((provider, id), members) in groups(&f.storage_events, |e| (e.provider.clone(), e.id)) {
            let (_, last) = first_last(&members);
            o.push(format!("  {provider}, {id}: {} events, last {last}", members.len()));
        }
        for e in last_sorted(&f.storage_events, 20) {
            o.push(format!("  [{}] {} id={} {}", time(e), e.provider, e.id, short(&e.message, 200)));
        }
    }
    o.push("-- volumes".into());
    for v in &f.volumes {
        o.push(format!("  {}: {} {} health={} op={} size={} GB free={} GB", v.drive_letter, v.file_system_label, v.file_system, v.health_status, v.operational_status, gb(v.size), gb(v.size_remaining)));
    }
    o
}

/// Where the script wrote its file: `<root>\upgrade_\reports\smart-diag-<computer>.txt`.
pub fn default_out(root: &std::path::Path, computer: &str) -> std::path::PathBuf {
    root.join("upgrade_").join("reports").join(format!("smart-diag-{computer}.txt"))
}
