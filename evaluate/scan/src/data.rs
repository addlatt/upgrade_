//! The knowledge tables: devices, distributions, releases.
//!
//! `data/*.ps1` is still where they are edited (CLAUDE.md, "the contribution
//! surface"). `data/tools/export-tables.ps1` writes them out as
//! `data/tables.json`, which is compiled into this crate. `port-check.sh`
//! fails when the two disagree.

use crate::check::Status;
use crate::ps::one_or_many;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;

pub const TABLES_JSON: &str = include_str!("../../../data/tables.json");

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

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(rename_all = "PascalCase", default)]
pub struct Distro {
    pub name: String,
    pub kernel: String,
    pub nvidia_easy: bool,
    pub newcomer: i64,
    pub note: String,
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(rename_all = "PascalCase", default)]
pub struct BootFile {
    pub role: String,
    pub file: String,
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
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Tables {
    pub wifi: HashMap<String, Device>,
    pub wifi_vendor_fallback: HashMap<String, Device>,
    pub gpu: HashMap<String, Device>,
    pub gpu_vendor_rules: HashMap<String, Device>,
    pub audio_quirks: Vec<Quirk>,
    pub vmd_device_ids: Vec<String>,
    pub vendor_quirks: Vec<Quirk>,
    pub app_risk: Vec<Quirk>,
    pub distro_table_verified: String,
    pub distros: Vec<Distro>,
    pub releases: Vec<Release>,
}

impl Tables {
    /// Reads a tables file and refuses one a check could not use: a status
    /// outside the five, or a pattern that is not a regular expression.
    pub fn load(json: &str) -> Result<Tables, String> {
        let t: Tables = serde_json::from_str(json).map_err(|e| format!("tables.json: {e}"))?;
        let devices = t.wifi.iter().chain(&t.wifi_vendor_fallback).chain(&t.gpu).chain(&t.gpu_vendor_rules);
        for (id, d) in devices {
            if Status::parse(&d.status).is_none() {
                return Err(format!("tables.json: {id} has the status '{}'", d.status));
            }
        }
        for q in t.audio_quirks.iter().chain(&t.vendor_quirks) {
            if Status::parse(&q.status).is_none() {
                return Err(format!("tables.json: {} has the status '{}'", q.pattern, q.status));
            }
        }
        for q in t.audio_quirks.iter().chain(&t.vendor_quirks).chain(&t.app_risk) {
            regress::Regex::with_flags(&q.pattern, "i").map_err(|e| format!("tables.json: the pattern {} does not read ({e})", q.pattern))?;
        }
        Ok(t)
    }
}

/// The tables compiled into this build.
pub fn tables() -> &'static Tables {
    static T: OnceLock<Tables> = OnceLock::new();
    T.get_or_init(|| Tables::load(TABLES_JSON).expect("data/tables.json loads"))
}

pub(crate) fn status_of(text: &str) -> Status {
    // Tables::load refused anything else
    Status::parse(text).unwrap_or(Status::Unknown)
}
