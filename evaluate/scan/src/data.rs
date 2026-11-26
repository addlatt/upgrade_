//! The knowledge tables: devices and releases.
//!
//! `data/devices/*.json` and `data/releases.json` are
//! the source (CLAUDE.md, "the contribution surface"; decided 2026-10-14,
//! the owner: JSON, one file per table) and are compiled into this crate.
//! Every file carries an `_about` entry for the reader and a `Source` and
//! `Verified` field per entry for its provenance; nothing here reads those.
//! `load` refuses a table a check could not use.

use crate::check::Status;
use crate::ps::{one_or_many, Version};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;

pub const WIFI_JSON: &str = include_str!("../../../data/devices/wifi.json");
pub const GPU_JSON: &str = include_str!("../../../data/devices/gpu.json");
pub const NVIDIA_JSON: &str = include_str!("../../../data/devices/nvidia.json");
pub const AUDIO_JSON: &str = include_str!("../../../data/devices/audio.json");
pub const STORAGE_JSON: &str = include_str!("../../../data/devices/storage.json");
pub const VENDORS_JSON: &str = include_str!("../../../data/devices/vendors.json");
pub const APPS_JSON: &str = include_str!("../../../data/devices/apps.json");
pub const RELEASES_JSON: &str = include_str!("../../../data/releases.json");

/// The seven files, as this build carries them.
pub struct Files<'a> {
    pub wifi: &'a str,
    pub gpu: &'a str,
    pub nvidia: &'a str,
    pub audio: &'a str,
    pub storage: &'a str,
    pub vendors: &'a str,
    pub apps: &'a str,
    pub releases: &'a str,
}

pub const FILES: Files<'static> = Files { wifi: WIFI_JSON, gpu: GPU_JSON, nvidia: NVIDIA_JSON, audio: AUDIO_JSON, storage: STORAGE_JSON, vendors: VENDORS_JSON, apps: APPS_JSON, releases: RELEASES_JSON };

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(rename_all = "PascalCase", default)]
pub struct Device {
    pub name: String,
    pub vendor: String,
    pub driver: String,
    pub min_kernel: String,
    pub status: String,
    pub note: String,
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(rename_all = "PascalCase", default)]
pub struct Quirk {
    #[serde(rename = "Match")]
    pub pattern: String,
    pub name: String,
    pub min_kernel: String,
    pub status: String,
    pub severity: String,
    pub note: String,
}

/// One kernel image of a release, as `measure-release.py` read it.
#[derive(Debug, Deserialize, Default, Clone)]
#[serde(rename_all = "PascalCase", default)]
pub struct Kernel {
    /// `stick`, or `installed ...` for a kernel the installed system gets
    pub role: String,
    pub file: String,
    /// what the image names itself (`uname -r`): `6.19.10-300.fc44.x86_64`
    pub release: String,
    pub sha256: String,
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(rename_all = "PascalCase", default)]
pub struct BootFile {
    pub role: String,
    pub file: String,
    pub sha256: String,
    pub sbat: Option<String>,
    #[serde(deserialize_with = "one_or_many")]
    pub sbat_level: Vec<String>,
    #[serde(deserialize_with = "one_or_many")]
    pub signed_by: Vec<String>,
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(rename_all = "PascalCase", default)]
pub struct Release {
    pub id: String,
    pub name: String,
    pub installer: Option<String>,
    pub boot: Vec<BootFile>,
    pub kernels: Vec<Kernel>,
}

impl Release {
    /// The oldest kernel the installed system can come up on, with its
    /// release text. Nothing when the table records none.
    pub fn installed_kernel(&self) -> Option<(Version, &str)> {
        self.kernels.iter().filter(|k| k.role.starts_with("installed")).filter_map(|k| Version::parse_release(&k.release).map(|v| (v, k.release.as_str()))).min_by_key(|(v, _)| *v)
    }
}

/// One NVIDIA driver branch on the measured release's stick, as
/// `measure-nvidia.py` read it from the package's own list.
#[derive(Debug, Deserialize, Default, Clone)]
#[serde(rename_all = "PascalCase", default)]
pub struct NvidiaBranch {
    /// `580`
    pub branch: String,
    pub package_sha256: String,
    /// the installed kernel the stick carries this branch's module built
    /// and signed for, or empty
    pub signed_module_for: String,
    /// the PCI device ids (vendor 10de) the package lists, `dddd` each
    pub ids: String,
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(rename_all = "PascalCase", default)]
pub struct NvidiaMeasured {
    /// the release id in `releases.json` whose ISO was read
    pub release: String,
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(rename_all = "PascalCase", default)]
pub struct Nvidia {
    pub measured: NvidiaMeasured,
    pub branches: Vec<NvidiaBranch>,
}

impl Nvidia {
    /// The branch the measured release's installer picks for a card: the
    /// newest one whose package lists it. Nothing when none does. A branch
    /// with no module built and signed for the installed kernel does not
    /// count: it would have to be built on the computer, and under Secure
    /// Boot that needs a key step nobody is there for.
    pub fn branch_for(&self, device: &str) -> Option<&NvidiaBranch> {
        self.branches.iter().filter(|b| !b.signed_module_for.is_empty() && b.ids.split(' ').any(|i| i == device)).max_by_key(|b| b.branch.parse::<u32>().unwrap_or(0))
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub struct Tables {
    pub wifi: HashMap<String, Device>,
    pub wifi_vendor_fallback: HashMap<String, Device>,
    pub gpu: HashMap<String, Device>,
    pub gpu_vendor_rules: HashMap<String, Device>,
    pub nvidia: Nvidia,
    pub audio_quirks: Vec<Quirk>,
    pub vmd_device_ids: Vec<String>,
    pub vendor_quirks: Vec<Quirk>,
    pub app_risk: Vec<Quirk>,
    pub releases: Vec<Release>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct WifiFile {
    devices: HashMap<String, Device>,
    vendor_fallback: HashMap<String, Device>,
}
#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct GpuFile {
    devices: HashMap<String, Device>,
    vendor_rules: HashMap<String, Device>,
}
#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct QuirksFile {
    quirks: Vec<Quirk>,
    risks: Vec<Quirk>,
}
#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct StorageFile {
    vmd_device_ids: Vec<String>,
}
#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct ReleasesFile {
    releases: Vec<Release>,
}

fn read<T: serde::de::DeserializeOwned>(name: &str, json: &str) -> Result<T, String> {
    serde_json::from_str(json).map_err(|e| format!("{name}: {e}"))
}

impl Tables {
    /// Reads the files and refuses a table a check could not use: a status
    /// outside the five, a pattern that is not a regular expression, a
    /// device id that is not `vvvv:dddd` in lower case, a sha256 that is not
    /// 64 hex digits, a release with no measured installed kernel.
    pub fn load(f: &Files) -> Result<Tables, String> {
        let wifi: WifiFile = read("data/devices/wifi.json", f.wifi)?;
        let gpu: GpuFile = read("data/devices/gpu.json", f.gpu)?;
        let audio: QuirksFile = read("data/devices/audio.json", f.audio)?;
        let storage: StorageFile = read("data/devices/storage.json", f.storage)?;
        let vendors: QuirksFile = read("data/devices/vendors.json", f.vendors)?;
        let apps: QuirksFile = read("data/devices/apps.json", f.apps)?;
        let releases: ReleasesFile = read("data/releases.json", f.releases)?;
        let nvidia: Nvidia = read("data/devices/nvidia.json", f.nvidia)?;
        let t = Tables {
            wifi: wifi.devices,
            wifi_vendor_fallback: wifi.vendor_fallback,
            gpu: gpu.devices,
            gpu_vendor_rules: gpu.vendor_rules,
            nvidia,
            audio_quirks: audio.quirks,
            vmd_device_ids: storage.vmd_device_ids,
            vendor_quirks: vendors.quirks,
            app_risk: apps.risks,
            releases: releases.releases,
        };
        let is_hex = |s: &str, n: usize| s.len() == n && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase());
        for (id, d) in t.wifi.iter().chain(&t.gpu) {
            if !(id.len() == 9 && id.as_bytes()[4] == b':' && is_hex(&id[..4], 4) && is_hex(&id[5..], 4)) {
                return Err(format!("devices: the id {id} is not vvvv:dddd in lower-case hex"));
            }
            if Status::parse(&d.status).is_none() {
                return Err(format!("devices: {id} has the status '{}'", d.status));
            }
        }
        for (id, d) in t.wifi_vendor_fallback.iter().chain(&t.gpu_vendor_rules) {
            if !is_hex(id, 4) {
                return Err(format!("devices: the vendor id {id} is not vvvv in lower-case hex"));
            }
            if Status::parse(&d.status).is_none() {
                return Err(format!("devices: {id} has the status '{}'", d.status));
            }
        }
        if !t.releases.iter().any(|r| r.id == t.nvidia.measured.release) {
            return Err(format!("nvidia: measured from '{}', which is not a release in the table: run data/tools/measure-nvidia.py", t.nvidia.measured.release));
        }
        for b in &t.nvidia.branches {
            if b.branch.parse::<u32>().is_err() || !is_hex(&b.package_sha256, 64) {
                return Err(format!("nvidia: the branch '{}' is not a number, or its package has no sha256", b.branch));
            }
            if b.ids.is_empty() || b.ids.split(' ').any(|i| !is_hex(i, 4)) {
                return Err(format!("nvidia: the branch {} lists an id that is not dddd in lower-case hex", b.branch));
            }
        }
        for id in &t.vmd_device_ids {
            if !(id.len() == 9 && is_hex(&id[..4], 4) && is_hex(&id[5..], 4)) {
                return Err(format!("storage: the VMD id {id} is not vvvv:dddd in lower-case hex"));
            }
        }
        for q in t.audio_quirks.iter().chain(&t.vendor_quirks) {
            if Status::parse(&q.status).is_none() {
                return Err(format!("devices: {} has the status '{}'", q.pattern, q.status));
            }
        }
        for q in &t.app_risk {
            if !["blocker", "friction", "info"].contains(&q.severity.as_str()) {
                return Err(format!("apps: {} has the severity '{}'", q.pattern, q.severity));
            }
        }
        for q in t.audio_quirks.iter().chain(&t.vendor_quirks).chain(&t.app_risk) {
            regress::Regex::with_flags(&q.pattern, "i").map_err(|e| format!("devices: the pattern {} does not read ({e})", q.pattern))?;
        }
        let mut ids = std::collections::HashSet::new();
        for r in &t.releases {
            if r.id.is_empty() || !ids.insert(r.id.clone()) {
                return Err(format!("releases: the id '{}' is empty or repeated", r.id));
            }
            for b in &r.boot {
                if !is_hex(&b.sha256, 64) {
                    return Err(format!("releases: {} {} has no sha256", r.id, b.file));
                }
            }
            for k in &r.kernels {
                if !is_hex(&k.sha256, 64) || Version::parse_release(&k.release).is_none() {
                    return Err(format!("releases: {} kernel {} has no sha256 or no release that reads ('{}')", r.id, k.file, k.release));
                }
            }
            if r.installed_kernel().is_none() {
                return Err(format!("releases: {} records no installed kernel (measure-release.py writes it)", r.id));
            }
        }
        Ok(t)
    }
}

/// The tables compiled into this build.
pub fn tables() -> &'static Tables {
    static T: OnceLock<Tables> = OnceLock::new();
    T.get_or_init(|| Tables::load(&FILES).expect("the data tables load"))
}

pub(crate) fn status_of(text: &str) -> Status {
    // Tables::load refused anything else
    Status::parse(text).unwrap_or(Status::Unknown)
}
