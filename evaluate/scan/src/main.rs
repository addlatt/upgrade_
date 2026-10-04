//! upgrade-scan (Rust): judges a recording of a machine and prints the
//! report. It cannot read a live machine yet; that is step 3 of
//! docs/RUST-PORT.md. Until then `evaluate/windows/upgrade-scan.ps1` is
//! the scanner, and this program is for comparing the two.
//!
//!   upgrade-scan --replay machine.json [--now 2026-10-04T10:00:00] [--json]
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
