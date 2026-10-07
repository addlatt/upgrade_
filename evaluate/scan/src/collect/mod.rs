//! The live reads: what the scanner asks the machine. Windows only. Each
//! collector reads one thing and keeps what it read as a fact, so every run
//! leaves a capture behind (CLAUDE.md rule #5), and the judging half never
//! touches the machine itself.
//!
//! Built one collector at a time (docs/RUST-PORT.md, step 3c). What is not
//! read yet is said, never guessed: `Collected::not_read` names it.
//!
//! The proof is side by side: `upgrade-scan --record` on a machine, the
//! PowerShell recorder on the same machine in the same minute, and
//! `upgrade-scan --compare-facts` must find the two fact sets equal.

use serde_json::{json, Map, Value};

#[cfg(windows)]
mod events;
#[cfg(windows)]
mod firmware;
#[cfg(windows)]
mod registry;
#[cfg(windows)]
mod resume;
#[cfg(windows)]
mod storage;
#[cfg(windows)]
mod system;
#[cfg(windows)]
mod win;
#[cfg(windows)]
mod wmi;

#[cfg(windows)]
pub use win::utc_to_local;

/// What one run of the collectors produced: the facts, as the PowerShell
/// recorder shapes them, and the names of the facts this build cannot read
/// yet.
#[derive(Debug, Default)]
pub struct Collected {
    pub facts: Map<String, Value>,
    pub not_read: Vec<&'static str>,
    /// a read that failed, with its reason; the fact is then absent
    pub errors: Vec<String>,
}

/// Every fact the PowerShell recorder writes, in its order.
pub const FACT_NAMES: [&str; 14] = ["IsAdmin", "Sys", "Pnp", "SecureBoot", "Sbat", "DbAuthorities", "Resume", "Disk", "VolumeHealth", "PhysicalDisk", "Hiberboot", "BitLocker", "Esp", "Apps"];

impl Collected {
    pub fn to_capture(&self, version: &str, now: &str) -> Value {
        let mut doc = Map::new();
        doc.insert("Capture".into(), json!("upgrade_ machine capture 1"));
        doc.insert("Collector".into(), json!(format!("upgrade-scan {version} (Rust)")));
        doc.insert("Now".into(), json!(now));
        for name in FACT_NAMES {
            if let Some(v) = self.facts.get(name) {
                doc.insert(name.into(), v.clone());
            }
        }
        doc.insert("NotRead".into(), json!(self.not_read));
        doc.insert("ReadErrors".into(), json!(self.errors));
        Value::Object(doc)
    }
}

/// Read what this build can read. `kit_root`: the stick's root, when the
/// scanner runs from one. Off Windows there is nothing to read.
pub fn collect(kit_root: Option<&std::path::Path>) -> Collected {
    #[cfg(windows)]
    {
        system::collect(kit_root)
    }
    #[cfg(not(windows))]
    {
        let _ = kit_root;
        Collected { not_read: FACT_NAMES.to_vec(), errors: vec!["not running on Windows".into()], ..Default::default() }
    }
}

/// Only the system facts and the device list (what `-DumpMachine` reads).
pub fn collect_hardware() -> Collected {
    #[cfg(windows)]
    {
        system::collect_hardware()
    }
    #[cfg(not(windows))]
    {
        Collected { not_read: FACT_NAMES.to_vec(), errors: vec!["not running on Windows".into()], ..Default::default() }
    }
}

/// Seconds since 1970 as a UTC stamp.
pub fn utc_from_seconds(secs: i64) -> crate::ps::Stamp {
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // civil date from days (Howard Hinnant's algorithm)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let year = (yoe + era * 400 + if month <= 2 { 1 } else { 0 }) as i32;
    crate::ps::Stamp { year, month, day, hour: (rem / 3600) as u32, minute: (rem % 3600 / 60) as u32, second: (rem % 60) as u32 }
}

/// The clock now: this machine's local time (what `Get-Date` shows, and
/// what the report's header and file names carry), and the same moment in
/// UTC written as .NET's round-trip form (`2026-10-07T18:22:33.1234567Z`,
/// what `ScannedUtc` carries). Off Windows, local time is UTC.
pub fn now() -> (crate::ps::Stamp, String) {
    let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let utc = utc_from_seconds(d.as_secs() as i64);
    let utc_text = format!("{}.{:07}Z", utc.iso(), d.subsec_nanos() / 100);
    #[cfg(windows)]
    let local = win::utc_to_local(utc);
    #[cfg(not(windows))]
    let local = utc;
    (local, utc_text)
}

/// This account's Desktop folder, the report's default home.
pub fn desktop_folder() -> Option<std::path::PathBuf> {
    #[cfg(windows)]
    {
        win::desktop_folder()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// `--try-wmi`: a debugging aid while the collectors are being built.
/// `--try-wmi scan` runs the online scan of C: (read-only) and prints its
/// result word.
pub fn try_wmi(what: Option<&str>) -> Vec<String> {
    #[cfg(windows)]
    {
        let mut out = Vec::new();
        if what == Some("scan") {
            out.push(format!("online scan of C: -> {}", storage::online_scan()));
            return out;
        }
        match wmi::Wmi::connect(wmi::STORAGE) {
            Ok(w) => {
                out.extend(w.try_method("MSFT_Partition", "DriveLetter='C'", "GetSupportedSize"));
                match w.query("MSFT_PhysicalDiskToStorageReliabilityCounter", &["PhysicalDisk", "StorageReliabilityCounter"]) {
                    Ok(l) => {
                        for a in &l {
                            out.push(format!("assoc: {a}"));
                            if let Some(p) = a["StorageReliabilityCounter"].as_str() {
                                out.push(format!("counter: {:?}", w.get_object(p, &["DeviceId", "Temperature", "Wear"])));
                            }
                        }
                    }
                    Err(e) => out.push(format!("assoc query: {e}")),
                }
                match w.query("MSFT_StorageReliabilityCounter", &["DeviceId", "Temperature", "Wear"]) {
                    Ok(l) => out.extend(l.iter().map(|c| format!("counter row: {c}"))),
                    Err(e) => out.push(format!("counter query: {e}")),
                }
                match w.query_where("MSFT_PhysicalDisk", &["DeviceId", "ObjectId"], "DeviceId='0'") {
                    Ok(l) => {
                        for c in &l {
                            out.push(format!("physical disk: {c}"));
                            let rel = c["__RELPATH"].as_str().unwrap_or("");
                            for q in [format!("ASSOCIATORS OF {{{rel}}} WHERE AssocClass=MSFT_PhysicalDiskToStorageReliabilityCounter"), format!("ASSOCIATORS OF {{{rel}}}"), format!("REFERENCES OF {{{rel}}}")] {
                                out.push(format!("{q}\n      -> {:?}", w.wql(&q, &["DeviceId", "Temperature"]).map(|l| l.len())));
                            }
                        }
                    }
                    Err(e) => out.push(format!("physical disk query: {e}")),
                }
            }
            Err(e) => out.push(format!("storage: {e}")),
        }
        out
    }
    #[cfg(not(windows))]
    {
        let _ = what;
        vec!["not on Windows".into()]
    }
}
