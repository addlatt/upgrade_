//! upgrade-scan (Rust): judges a recording of a machine and prints the
//! report. It cannot read a live machine yet; that is step 3 of
//! docs/RUST-PORT.md. Until then `evaluate/windows/upgrade-scan.ps1` is
//! the scanner, and this program is for comparing the two.
//!
//!   upgrade-scan --replay machine.json [--now 2026-10-04T10:00:00] [--json]
//!   upgrade-scan --record [--out <folder>]          (Windows: read this machine, write a capture)
//!   upgrade-scan --compare-facts rust.json powershell.json
//!
//! A capture made by tools/Record-Machine.ps1 also holds what the PowerShell
//! scanner concluded from the same facts. Then the replay compares the two
//! and says whether they are the same, line for line (exit 1 if not).
//!
//! Read-only: it opens the one file it is given and writes to the screen.

use serde_json::{json, Value};
use std::process::ExitCode;
use upgrade_scan::facts::Machine;
use upgrade_scan::ps::Stamp;
use upgrade_scan::{report, run, FOLLOWS_SCANNER};

fn utc_now() -> Stamp {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
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
    Stamp { year, month, day, hour: (rem / 3600) as u32, minute: (rem % 3600 / 60) as u32, second: (rem % 60) as u32 }
}

/// The PowerShell scanner's own conclusions, from the same facts, against
/// this program's: every report line and every check, word for word.
fn compare(ps: &Value, machine: &Machine, outcome: &run::Outcome, now: Stamp) -> ExitCode {
    let ours = report::lines(machine, outcome, now, FOLLOWS_SCANNER);
    let theirs: Vec<String> = ps["Lines"].as_array().map(|l| l.iter().map(|x| x.as_str().unwrap_or("").to_string()).collect()).unwrap_or_default();
    let mut differences = Vec::new();
    for i in 0..ours.len().max(theirs.len()) {
        let (a, b) = (theirs.get(i), ours.get(i));
        if a != b {
            differences.push(format!("  report line {}:\n    PowerShell: {}\n    Rust:       {}", i + 1, a.map_or("(no line)", String::as_str), b.map_or("(no line)", String::as_str)));
        }
    }
    let checks = ps["Checks"].as_array().cloned().unwrap_or_default();
    if checks.len() != outcome.scan.checks.len() {
        differences.push(format!("  PowerShell made {} checks, Rust made {}", checks.len(), outcome.scan.checks.len()));
    }
    for (i, (p, r)) in checks.iter().zip(&outcome.scan.checks).enumerate() {
        let mine = [("Section", r.section.as_str()), ("Title", r.title.as_str()), ("Status", r.status.as_str()), ("Detail", r.detail.as_str()), ("Note", r.note.as_str()), ("MinKernel", r.min_kernel.as_str()), ("Remedy", r.remedy.as_str())];
        for (field, value) in mine {
            let theirs = p[field].as_str().unwrap_or("");
            if theirs != value {
                differences.push(format!("  check {} ({}), {field}:\n    PowerShell: {theirs}\n    Rust:       {value}", i + 1, r.title));
            }
        }
    }
    if ps["Verdict"].as_str() != Some(outcome.verdict.level.as_str()) {
        differences.push(format!("  verdict: PowerShell {}, Rust {}", ps["Verdict"], outcome.verdict.level.as_str()));
    }
    let kernel = outcome.required_kernel.map(|k| k.to_string());
    if ps["RequiredKernel"].as_str() != kernel.as_deref() {
        differences.push(format!("  required kernel: PowerShell {}, Rust {kernel:?}", ps["RequiredKernel"]));
    }
    println!();
    println!("  machine: {} {}   elevated: {}   verdict: {}", machine.sys.vendor.as_deref().unwrap_or(""), machine.sys.model.as_deref().unwrap_or(""), machine.is_admin, outcome.verdict.level.as_str());
    if differences.is_empty() {
        println!("  SAME: {} report lines and {} checks, word for word, as the PowerShell scanner concluded from these facts.", ours.len(), outcome.scan.checks.len());
        println!();
        return ExitCode::SUCCESS;
    }
    println!("  DIFFERENT in {} places (first {}):", differences.len(), differences.len().min(10));
    for d in differences.iter().take(10) {
        println!("{d}");
    }
    println!();
    ExitCode::from(1)
}

/// Read this machine with the Rust collectors and write the capture, in the
/// same shape as tools/Record-Machine.ps1 writes one.
fn record(out: Option<String>) -> ExitCode {
    let now = utc_now();
    let c = upgrade_scan::collect::collect();
    let doc = c.to_capture(env!("CARGO_PKG_VERSION"), &now.iso());
    let model = doc.pointer("/Sys/Model").and_then(Value::as_str).unwrap_or("unknown");
    let safe: String = model.chars().map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' }).collect::<String>().split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    let dir = out.unwrap_or_else(|| "captures".to_string());
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("upgrade-scan: cannot make {dir}: {e}");
        return ExitCode::from(2);
    }
    let path = format!("{dir}/upgrade-report-capture-rust-{safe}-{:04}{:02}{:02}-{:02}{:02}.json", now.year, now.month, now.day, now.hour, now.minute);
    if let Err(e) = std::fs::write(&path, serde_json::to_string_pretty(&doc).unwrap_or_default() + "\n") {
        eprintln!("upgrade-scan: cannot write {path}: {e}");
        return ExitCode::from(2);
    }
    println!();
    println!("  read: {}", c.facts.keys().cloned().collect::<Vec<_>>().join(", "));
    println!("  not read by this build yet: {}", c.not_read.join(", "));
    for e in &c.errors {
        println!("  read error: {e}");
    }
    println!("  written: {path}");
    println!("  It holds this machine's hardware facts and program list. Do not commit it.");
    println!();
    if c.errors.is_empty() { ExitCode::SUCCESS } else { ExitCode::from(1) }
}

/// Two captures of the same machine, the Rust collectors' and the
/// PowerShell recorder's: fact by fact, are they the same?
fn compare_facts(rust_path: &str, ps_path: &str) -> ExitCode {
    let load = |p: &str| -> Result<Value, String> { serde_json::from_str(std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"))?.trim_start_matches('\u{feff}')).map_err(|e| format!("{p}: {e}")) };
    let (rust, ps) = match (load(rust_path), load(ps_path)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("upgrade-scan: {e}");
            return ExitCode::from(2);
        }
    };
    let not_read: Vec<String> = rust["NotRead"].as_array().map(|l| l.iter().map(|x| x.as_str().unwrap_or("").to_string()).collect()).unwrap_or_default();
    let mut different = 0;
    println!();
    println!("  Rust:       {}   ({})", rust_path, rust["Now"].as_str().unwrap_or(""));
    println!("  PowerShell: {}   ({})", ps_path, ps["Now"].as_str().unwrap_or(""));
    println!();
    for name in upgrade_scan::collect::FACT_NAMES {
        if let Some(n) = not_read.iter().find(|n| n.starts_with(name)) {
            println!("    not read by Rust yet   {n}");
            continue;
        }
        let diffs = fact_differences(name, &rust[name], &ps[name]);
        if diffs.is_empty() {
            println!("    SAME                   {name}{}", fact_size(&rust[name]));
        } else {
            different += 1;
            println!("    DIFFERENT              {name} ({} differences; first {})", diffs.len(), diffs.len().min(5));
            for d in diffs.iter().take(5) {
                println!("      {d}");
            }
        }
    }
    println!();
    if different == 0 {
        println!("  every fact Rust read is what PowerShell read");
        println!();
        ExitCode::SUCCESS
    } else {
        println!("  {different} facts differ");
        println!();
        ExitCode::from(1)
    }
}

fn fact_size(v: &Value) -> String {
    match v {
        Value::Array(l) => format!(" ({} items)", l.len()),
        _ => String::new(),
    }
}

fn same_value(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same_value(p, q)),
        (Value::Object(x), Value::Object(y)) => x.len() == y.len() && x.iter().all(|(k, p)| y.get(k).is_some_and(|q| same_value(p, q))),
        _ => a == b,
    }
}

/// The differences inside one fact. The device list compares device by
/// device (by DeviceID), the program list as a set regardless of case and
/// order, everything else value by value.
fn fact_differences(name: &str, rust: &Value, ps: &Value) -> Vec<String> {
    let mut out = Vec::new();
    match name {
        "Pnp" => {
            let by_id = |v: &Value| -> std::collections::BTreeMap<String, Value> { v.as_array().into_iter().flatten().map(|d| (d["DeviceID"].as_str().unwrap_or("").to_string(), d.clone())).collect() };
            let (r, p) = (by_id(rust), by_id(ps));
            for id in r.keys().filter(|k| !p.contains_key(*k)) {
                out.push(format!("only Rust saw {id}"));
            }
            for id in p.keys().filter(|k| !r.contains_key(*k)) {
                out.push(format!("only PowerShell saw {id}"));
            }
            for (id, rd) in &r {
                if let Some(pd) = p.get(id) {
                    for field in ["Name", "PNPClass", "Service", "CompatibleID"] {
                        if !same_value(&rd[field], &pd[field]) {
                            out.push(format!("{id} {field}: Rust {} PowerShell {}", rd[field], pd[field]));
                        }
                    }
                }
            }
        }
        "Apps" => {
            let set = |v: &Value| -> std::collections::BTreeSet<String> { v.as_array().into_iter().flatten().map(|x| x.as_str().unwrap_or("").to_lowercase()).collect() };
            let (r, p) = (set(rust), set(ps));
            out.extend(r.difference(&p).map(|x| format!("only Rust listed {x}")));
            out.extend(p.difference(&r).map(|x| format!("only PowerShell listed {x}")));
        }
        _ => match (rust, ps) {
            (Value::Object(r), Value::Object(p)) => {
                let mut keys: Vec<&String> = r.keys().chain(p.keys()).collect();
                keys.sort();
                keys.dedup();
                for k in keys {
                    match (r.get(k), p.get(k)) {
                        (Some(x), Some(y)) if same_value(x, y) => {}
                        (x, y) => out.push(format!("{k}: Rust {} PowerShell {}", x.map_or("(absent)".to_string(), Value::to_string), y.map_or("(absent)".to_string(), Value::to_string))),
                    }
                }
            }
            (x, y) => {
                if !same_value(x, y) {
                    out.push(format!("Rust {x} PowerShell {y}"));
                }
            }
        },
    }
    out
}

fn usage() -> ExitCode {
    eprintln!("upgrade-scan {} (Rust; follows scanner {FOLLOWS_SCANNER}; replays recordings, reads no live machine yet)", env!("CARGO_PKG_VERSION"));
    eprintln!("usage: upgrade-scan --replay <machine.json> [--now YYYY-MM-DDTHH:MM:SS] [--json]");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let value_of = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    if args.iter().any(|a| a == "--version") {
        println!("upgrade-scan {} (Rust; follows scanner {FOLLOWS_SCANNER})", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    if args.iter().any(|a| a == "--record") {
        return record(value_of("--out"));
    }
    if let Some(i) = args.iter().position(|a| a == "--compare-facts") {
        let (Some(a), Some(b)) = (args.get(i + 1), args.get(i + 2)) else { return usage() };
        return compare_facts(a, b);
    }
    let Some(path) = value_of("--replay") else { return usage() };
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("upgrade-scan: cannot read {path}: {e}");
            return ExitCode::from(2);
        }
    };
    let doc: Value = match serde_json::from_str(text.trim_start_matches('\u{feff}')) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("upgrade-scan: {path} is not JSON: {e}");
            return ExitCode::from(2);
        }
    };
    let powershell = doc.get("PowerShell").cloned();
    let captured_now = doc.get("Now").and_then(Value::as_str).map(str::to_string);
    let now = match value_of("--now").or(captured_now) {
        Some(text) => match Stamp::parse(&text) {
            Some(t) => t,
            None => {
                eprintln!("upgrade-scan: --now {text} is not a date and time (2026-10-04T10:00:00)");
                return ExitCode::from(2);
            }
        },
        None => utc_now(),
    };
    // a -DumpMachine recording holds the device list only; it has no IsAdmin
    let hardware_only = doc.get("IsAdmin").is_none();
    let machine: Machine = match serde_json::from_value(doc) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("upgrade-scan: {path} is not a machine recording: {e}");
            return ExitCode::from(2);
        }
    };
    let outcome = if hardware_only {
        eprintln!("upgrade-scan: a hardware-only recording. Only the 7 checks that read the device list are replayed; the verdict covers those 7 and nothing else.");
        run::scan_hardware_only(&machine)
    } else {
        run::scan(&machine)
    };
    if let Some(ps) = powershell {
        return compare(&ps, &machine, &outcome, now);
    }
    if args.iter().any(|a| a == "--json") {
        let checks: Vec<Value> = outcome
            .scan
            .checks
            .iter()
            .map(|c| json!({"Section": c.section, "Title": c.title, "Status": c.status.as_str(), "Detail": c.detail, "Note": c.note, "MinKernel": c.min_kernel, "Remedy": c.remedy}))
            .collect();
        let doc = json!({
            "ScannerVersion": format!("{FOLLOWS_SCANNER} (Rust replay {})", env!("CARGO_PKG_VERSION")),
            "HardwareOnly": hardware_only,
            "RanAsAdmin": machine.is_admin,
            "RequiredKernel": outcome.required_kernel.map(|k| k.to_string()),
            "Verdict": {"Level": outcome.verdict.level.as_str(), "Summary": outcome.verdict.summary},
            "Recommended": outcome.recommendation.distros.iter().map(|d| d.name.clone()).collect::<Vec<_>>(),
            "Checks": checks,
            "Releases": outcome.scan.releases,
            "UnmatchedIds": report::unmatched_sorted(&outcome.scan.unmatched),
        });
        println!("{}", serde_json::to_string_pretty(&doc).unwrap_or_default());
    } else {
        for line in report::lines(&machine, &outcome, now, FOLLOWS_SCANNER) {
            println!("{line}");
        }
    }
    ExitCode::SUCCESS
}
