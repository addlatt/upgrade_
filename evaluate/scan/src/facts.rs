//! What the collectors hand the judging functions. The field names are the
//! PowerShell scanner's own, so a capture written by it (`-DumpMachine`, a
//! corpus file) loads here unchanged. A field that was not read is absent,
//! and absent is never taken for "fine".

use crate::ps::{one_or_many, Stamp};
use serde::Deserialize;

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(rename_all = "PascalCase", default)]
pub struct Sys {
    pub vendor: Option<String>,
    pub model: Option<String>,
    #[serde(rename = "RamGB")]
    pub ram_gb: Option<f64>,
    pub os_caption: Option<String>,
    pub os_build: Option<i64>,
    pub cpu_name: Option<String>,
    /// Win32_Processor.Architecture: 9 = x64, 12 = ARM64, 0 = x86
    pub cpu_arch: Option<i64>,
    pub cpu_cores: Option<i64>,
    pub firmware: Option<String>,
    pub bios_version: Option<String>,
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(default)]
pub struct Pnp {
    #[serde(rename = "Name")]
    pub name: Option<String>,
    #[serde(rename = "DeviceID")]
    pub device_id: Option<String>,
    #[serde(rename = "PNPClass")]
    pub pnp_class: Option<String>,
    #[serde(rename = "Service")]
    pub service: Option<String>,
    #[serde(rename = "CompatibleID", deserialize_with = "one_or_many")]
    pub compatible_id: Vec<String>,
}

/// A machine recording (`evaluate/windows/corpus/*.json`).
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "PascalCase")]
pub struct Capture {
    pub label: String,
    pub sys: Sys,
    pub pnp: Vec<Pnp>,
    pub expected: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(rename_all = "PascalCase", default)]
pub struct SbatSource {
    pub source: Option<String>,
    pub text: Option<String>,
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(rename_all = "PascalCase", default)]
pub struct KitBootFile {
    pub name: Option<String>,
    pub sbat: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(rename_all = "PascalCase", default)]
pub struct ResumeFacts {
    pub schedule_service: Option<String>,
    pub task_creation_policy: Option<i64>,
    pub domain_joined: Option<bool>,
    pub azure_ad_joined: Option<bool>,
    pub mdm: Option<String>,
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(rename_all = "PascalCase", default)]
pub struct DiskInfo {
    pub number: Option<i64>,
    pub friendly_name: Option<String>,
    pub size: Option<f64>,
    pub partition_style: Option<String>,
    pub bus_type: Option<String>,
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(rename_all = "PascalCase", default)]
pub struct VolumeSize {
    pub size: f64,
    pub size_remaining: f64,
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(rename_all = "PascalCase", default)]
pub struct DiskFacts {
    pub disks: Vec<DiskInfo>,
    pub sys_volume: Option<VolumeSize>,
    pub last_unmovable: Option<String>,
    #[serde(rename = "ShrinkGB")]
    pub shrink_gb: Option<f64>,
    pub shrink_source: Option<String>,
    pub shrink_error: Option<String>,
    pub shrink_failed_at: Option<String>,
    pub diskpart_error: Option<String>,
    pub disk0_part_count: Option<i64>,
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(rename_all = "PascalCase", default)]
pub struct Counters {
    pub read_errors_uncorrected: Option<i64>,
    pub write_errors_uncorrected: Option<i64>,
    pub wear: Option<i64>,
    pub power_on_hours: Option<i64>,
}

#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
#[serde(rename_all = "PascalCase", default)]
pub struct DiskEvents {
    pub bad_block: i64,
    pub paging: i64,
    pub reset: i64,
    pub first: Option<Stamp>,
    pub last: Option<Stamp>,
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(rename_all = "PascalCase", default)]
pub struct Smart {
    pub source: Option<String>,
    pub reallocated: Option<i64>,
    pub uncorrectable: Option<i64>,
    pub pending: Option<i64>,
    pub crc: Option<i64>,
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(rename_all = "PascalCase", default)]
pub struct PhysicalDiskFacts {
    pub found: bool,
    pub friendly_name: Option<String>,
    pub health_status: Option<String>,
    pub operational_status: Option<String>,
    pub error: Option<String>,
    pub counters: Option<Counters>,
    pub disk_events: Option<DiskEvents>,
    pub smart: Option<Smart>,
}

/// What a Chkdsk event in the Application log concluded.
#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
#[serde(rename_all = "PascalCase", default)]
pub struct ChkdskLog {
    /// `found-problems`, `no-problems` or `unknown`
    pub verdict: String,
    pub records: i64,
    pub queued: i64,
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(rename_all = "PascalCase", default)]
pub struct VolumeHealth {
    /// `clean`, `dirty` or `unknown`
    pub dirty: Option<String>,
    pub scan: Option<String>,
    pub scan_ran: bool,
    pub error: Option<String>,
    pub volume_status: Option<String>,
    pub volume_health: Option<String>,
    pub ntfs_full_chkdsk: Option<Stamp>,
    pub last_check: Option<Stamp>,
    pub logged: Option<ChkdskLog>,
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(rename_all = "PascalCase", default)]
pub struct BitLockerState {
    pub succeeded: bool,
    pub encrypted_mounts: Vec<String>,
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(rename_all = "PascalCase", default)]
pub struct EspFacts {
    pub succeeded: bool,
    pub free_bytes: Option<f64>,
    pub total_bytes: Option<f64>,
    pub has_windows_boot_files: Option<bool>,
    pub bootmgr_points_at_esp: Option<bool>,
    pub bcd_device: Option<String>,
}

/// One event from the System log's `disk` provider.
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "PascalCase")]
pub struct DiskEvent {
    pub id: i64,
    pub time_created: Stamp,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(rename_all = "PascalCase", default)]
pub struct SbatFacts {
    pub levels: Vec<SbatSource>,
    pub files: Vec<KitBootFile>,
}

/// Everything one scan reads from a machine, in one piece. The collectors
/// will fill it from a live Windows (step 3); a recording fills it from a
/// file. Either way the judging is the same call: `run::scan`.
#[derive(Debug, Deserialize, Default, Clone)]
#[serde(rename_all = "PascalCase", default)]
pub struct Machine {
    pub is_admin: bool,
    pub sys: Sys,
    pub pnp: Vec<Pnp>,
    /// 1 on, 0 off, absent = could not read
    pub secure_boot: Option<i64>,
    pub sbat: SbatFacts,
    /// absent = could not read (not elevated); an empty list is a read list
    pub db_authorities: Option<Vec<String>>,
    pub resume: Option<ResumeFacts>,
    pub disk: DiskFacts,
    pub volume_health: Option<VolumeHealth>,
    pub physical_disk: Option<PhysicalDiskFacts>,
    pub hiberboot: Option<i64>,
    pub bit_locker: Option<BitLockerState>,
    pub esp: Option<EspFacts>,
    pub apps: Vec<String>,
}
