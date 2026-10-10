//! The scanner as the product runs it: read this machine, judge it, and
//! write the report where and as `upgrade-scan.ps1 -Json -OutDir` writes it,
//! so the job writer finds the same file. This is what `upgrade-scan scan`
//! does, and what the window will call directly.
//!
//! What is written and read here:
//! - `upgrade-report-<model>-<stamp>.txt`: the report, as `Out-File
//!   -Encoding UTF8` writes it on Windows PowerShell 5.1 (a byte order mark,
//!   CRLF line ends).
//! - `upgrade-report-<model>-<stamp>.json`: the fields the PowerShell writes,
//!   in its order (`ScannerVersion`, `ScannedUtc`, `System`, `RanAsAdmin`,
//!   `RequiredKernel`, `Verdict`, `Recommended`, `Checks`, `Releases`,
//!   `UnmatchedIds`), plus `Scanner` (which program wrote it) and, only when
//!   a read failed, `ReadErrors`. Plain UTF-8, no byte order mark: every
//!   reader of this file (`New-Job.ps1`, the window, the Rust) accepts both.
//! - `machine-capture.json`: the hardware-only capture `-DumpMachine` writes,
//!   in its shape, for the corpus.

use crate::collect::{self, Collected};
use crate::facts::Machine;
use crate::ps::Stamp;
use crate::{report, run, FOLLOWS_SCANNER};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

/// One scan of this machine: what was read, what was concluded, the report.
pub struct ScanRun {
    pub machine: Machine,
    pub capture: Value,
    pub outcome: run::Outcome,
    pub lines: Vec<String>,
    /// local time, as the report's header and the file names carry it
    pub now: Stamp,
    /// the same moment in UTC, .NET round-trip form
    pub scanned_utc: String,
    pub read_errors: Vec<String>,
}

/// Read this machine with every collector and judge it. `kit_root` is where
/// the kit's boot files sit (the stick's root, where the scanner runs from).
pub fn scan_this_machine(kit_root: Option<&Path>) -> Result<ScanRun, String> {
    let (now, scanned_utc) = collect::now();
    let c = collect::collect(kit_root);
    if c.facts.get("Sys").is_none() {
        return Err(format!("could not read this machine: {}", c.errors.join("; ")));
    }
    let capture = c.to_capture(env!("CARGO_PKG_VERSION"), &now.iso());
    let machine: Machine = serde_json::from_value(capture.clone()).map_err(|e| format!("the facts read do not form a machine: {e}"))?;
    let outcome = run::scan(&machine);
    let lines = report::lines(&machine, &outcome, now, FOLLOWS_SCANNER);
    Ok(ScanRun { machine, capture, outcome, lines, now, scanned_utc, read_errors: c.errors })
}

/// `($sys.Model -replace '[^A-Za-z0-9]+', '-').Trim('-')`
pub fn safe_model(model: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for ch in model.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            dash = false;
        } else if !dash {
            out.push('-');
            dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

/// `Get-Date -Format 'yyyyMMdd-HHmm'`
pub fn stamp(now: Stamp) -> String {
    format!("{:04}{:02}{:02}-{:02}{:02}", now.year, now.month, now.day, now.hour, now.minute)
}

/// The JSON report, field for field as the PowerShell writes it.
pub fn report_json(r: &ScanRun) -> Value {
    let o = &r.outcome;
    let checks: Vec<Value> = o
        .scan
        .checks
        .iter()
        .map(|c| json!({"Section": c.section, "Title": c.title, "Status": c.status.as_str(), "Detail": c.detail, "Note": c.note, "MinKernel": c.min_kernel, "Remedy": c.remedy}))
        .collect();
    let groups: Vec<Value> = o.verdict.groups.iter().map(|g| json!({"Priority": g.priority, "Label": g.label, "Items": g.items})).collect();
    let mut doc = Map::new();
    doc.insert("ScannerVersion".into(), json!(FOLLOWS_SCANNER));
    doc.insert("Scanner".into(), json!(format!("upgrade-scan {} (Rust; follows upgrade-scan.ps1 {FOLLOWS_SCANNER})", env!("CARGO_PKG_VERSION"))));
    doc.insert("ScannedUtc".into(), json!(r.scanned_utc));
    doc.insert("System".into(), r.capture.get("Sys").cloned().unwrap_or(Value::Null));
    doc.insert("RanAsAdmin".into(), json!(r.machine.is_admin));
    doc.insert("RequiredKernel".into(), o.required_kernel.map_or(Value::Null, |k| json!(k.to_string())));
    doc.insert("Verdict".into(), json!({"Level": o.verdict.level.as_str(), "Summary": o.verdict.summary, "Groups": groups}));
    doc.insert("Recommended".into(), json!(o.recommendation.distros.iter().map(|d| d.name.clone()).collect::<Vec<_>>()));
    doc.insert("Checks".into(), Value::Array(checks));
    doc.insert("Releases".into(), serde_json::to_value(&o.scan.releases).unwrap_or(Value::Array(vec![])));
    doc.insert("UnmatchedIds".into(), json!(report::unmatched_sorted(&o.scan.unmatched)));
    if !r.read_errors.is_empty() {
        doc.insert("ReadErrors".into(), json!(r.read_errors));
    }
    Value::Object(doc)
}

/// Write the report files into `out_dir` (made if missing). Returns the
/// text file's path and, with `json`, the JSON file's.
pub fn write_report(r: &ScanRun, out_dir: &Path, json: bool) -> Result<(PathBuf, Option<PathBuf>), String> {
    std::fs::create_dir_all(out_dir).map_err(|e| format!("cannot make {}: {e}", out_dir.display()))?;
    let model = r.machine.sys.model.as_deref().unwrap_or("");
    let base = format!("upgrade-report-{}-{}", safe_model(model), stamp(r.now));
    let txt = out_dir.join(format!("{base}.txt"));
    // Out-File -Encoding UTF8 on Windows PowerShell 5.1: a byte order mark, CRLF after every line
    let mut text = String::from("\u{feff}");
    for l in &r.lines {
        text.push_str(l);
        text.push_str("\r\n");
    }
    std::fs::write(&txt, text).map_err(|e| format!("cannot write {}: {e}", txt.display()))?;
    let mut json_path = None;
    if json {
        let p = out_dir.join(format!("{base}.json"));
        let doc = serde_json::to_string_pretty(&report_json(r)).map_err(|e| e.to_string())? + "\n";
        std::fs::write(&p, doc).map_err(|e| format!("cannot write {}: {e}", p.display()))?;
        json_path = Some(p);
    }
    Ok((txt, json_path))
}

/// Export-UpgMachineCapture: the hardware-only capture for the corpus.
/// Only PCI, ACPI, HDAUDIO and INTELAUDIO device paths (USB and ROOT paths
/// can embed serial numbers), and only the system fields the checks read.
pub fn machine_capture(c: &Collected, captured: &str) -> Value {
    let sys = c.facts.get("Sys").cloned().unwrap_or(Value::Null);
    let keep = |id: &str| {
        let u = id.to_uppercase();
        ["PCI\\", "ACPI\\", "HDAUDIO\\", "INTELAUDIO\\"].iter().any(|p| u.starts_with(p))
    };
    let pnp: Vec<Value> = c
        .facts
        .get("Pnp")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|d| keep(d["DeviceID"].as_str().unwrap_or("")))
        .map(|d| {
            let compat = match &d["CompatibleID"] {
                Value::Array(l) => Value::Array(l.clone()),
                Value::Null => Value::Array(vec![]),
                other => Value::Array(vec![other.clone()]),
            };
            json!({"Name": d["Name"], "DeviceID": d["DeviceID"], "PNPClass": d["PNPClass"], "Service": d["Service"], "CompatibleID": compat})
        })
        .collect();
    let label = format!("{} {}", sys["Vendor"].as_str().unwrap_or(""), sys["Model"].as_str().unwrap_or("")).trim().to_string();
    json!({
        "SchemaVersion": 1,
        "Captured": captured,
        "Label": label,
        "Sys": {"Vendor": sys["Vendor"], "Model": sys["Model"], "RamGB": sys["RamGB"], "CpuName": sys["CpuName"], "CpuArch": sys["CpuArch"], "CpuCores": sys["CpuCores"], "IsLaptop": sys["IsLaptop"], "Firmware": sys["Firmware"]},
        "Pnp": pnp,
        "Expected": {},
    })
}

/// A .NET JSON date (`\/Date(1700000000000)\/`, milliseconds since 1970,
/// UTC) as this machine's local time, written the way the Rust writes dates.
fn dotnet_date_local(text: &str) -> Option<String> {
    let ms: i64 = text.strip_prefix("/Date(")?.strip_suffix(")/")?.parse().ok()?;
    let utc = collect::utc_from_seconds(ms.div_euclid(1000));
    #[cfg(windows)]
    let local = crate::collect::utc_to_local(utc);
    #[cfg(not(windows))]
    let local = utc;
    Some(local.iso())
}

/// Two JSON reports of the same machine, the Rust's and the PowerShell's:
/// every field that decides anything, compared in full. `System.BiosDate` is
/// compared as a moment (the PowerShell writes it as a .NET date); `Scanner`
/// and `ReadErrors` are the Rust's own; `ScannedUtc` is a clock.
pub fn report_differences(rust: &Value, ps: &Value) -> Vec<String> {
    let mut out = Vec::new();
    let mut diff = |path: &str, a: &Value, b: &Value| {
        if !same(a, b) {
            out.push(format!("{path}:\n    PowerShell: {b}\n    Rust:       {a}"));
        }
    };
    for k in ["ScannerVersion", "RanAsAdmin", "RequiredKernel", "Verdict", "Recommended", "Releases", "UnmatchedIds"] {
        diff(k, &rust[k], &ps[k]);
    }
    let (rc, pc) = (rust["Checks"].as_array().cloned().unwrap_or_default(), ps["Checks"].as_array().cloned().unwrap_or_default());
    if rc.len() != pc.len() {
        out.push(format!("Checks: PowerShell made {} checks, Rust made {}", pc.len(), rc.len()));
    }
    for (i, (r, p)) in rc.iter().zip(&pc).enumerate() {
        for field in ["Section", "Title", "Status", "Detail", "Note", "MinKernel", "Remedy"] {
            if !same(&r[field], &p[field]) {
                out.push(format!("Checks[{i}] ({}) {field}:\n    PowerShell: {}\n    Rust:       {}", p["Title"].as_str().unwrap_or(""), p[field], r[field]));
            }
        }
    }
    let (rs, ps_) = (&rust["System"], &ps["System"]);
    let mut keys: Vec<String> = rs.as_object().into_iter().flatten().map(|(k, _)| k.clone()).chain(ps_.as_object().into_iter().flatten().map(|(k, _)| k.clone())).collect();
    keys.sort();
    keys.dedup();
    for k in keys {
        let (a, b) = (&rs[&k], &ps_[&k]);
        if k == "BiosDate" {
            let b_local = b.as_str().and_then(dotnet_date_local).map_or(b.clone(), Value::String);
            if !same(a, &b_local) {
                out.push(format!("System.BiosDate:\n    PowerShell: {b} (as local time: {b_local})\n    Rust:       {a}"));
            }
            continue;
        }
        if !same(a, b) {
            out.push(format!("System.{k}:\n    PowerShell: {b}\n    Rust:       {a}"));
        }
    }
    out
}

fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same(p, q)),
        // ConvertTo-Json writes a one-item array as the item
        (Value::Array(x), y) | (y, Value::Array(x)) if x.len() == 1 && !y.is_array() => same(&x[0], y),
        (Value::Object(x), Value::Object(y)) => x.len() == y.len() && x.iter().all(|(k, p)| y.get(k).is_some_and(|q| same(p, q))),
        _ => a == b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_model_is_the_powershell_replace_and_trim() {
        assert_eq!(safe_model("ROG Zephyrus G16 GA605WV_GA605WV"), "ROG-Zephyrus-G16-GA605WV-GA605WV");
        assert_eq!(safe_model("  Aspire A515-51G  "), "Aspire-A515-51G");
        assert_eq!(safe_model("---"), "");
        assert_eq!(safe_model("Surface Pro 11 (ARM)"), "Surface-Pro-11-ARM");
    }

    #[test]
    fn the_stamp_is_the_powershell_format() {
        assert_eq!(stamp(Stamp { year: 2026, month: 10, day: 7, hour: 9, minute: 5, second: 59 }), "20261007-0905");
    }

    #[test]
    fn a_dotnet_date_reads_as_a_moment() {
        // 2025-05-20T00:00:00Z; off Windows local time is UTC
        let local = dotnet_date_local("/Date(1747699200000)/").unwrap();
        assert!(local.starts_with("2025-05-"), "{local}");
        assert_eq!(dotnet_date_local("2025-05-20T00:00:00"), None);
    }

    #[test]
    fn one_item_arrays_compare_as_the_item() {
        assert!(same(&json!(["a"]), &json!("a")));
        assert!(same(&json!({"Items": ["a"]}), &json!({"Items": "a"})));
        assert!(!same(&json!(["a", "b"]), &json!("a")));
        assert!(same(&json!(17), &json!(17.0)));
    }
}
