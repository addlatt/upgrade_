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
mod registry;
#[cfg(windows)]
mod system;
#[cfg(windows)]
mod wmi;

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

/// Read what this build can read. Off Windows there is nothing to read.
pub fn collect() -> Collected {
    #[cfg(windows)]
    {
        system::collect()
    }
    #[cfg(not(windows))]
    {
        Collected { not_read: FACT_NAMES.to_vec(), errors: vec!["not running on Windows".into()], ..Default::default() }
    }
}
