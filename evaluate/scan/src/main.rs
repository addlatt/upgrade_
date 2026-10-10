//! upgrade-scan (Rust): the scanner, following `evaluate/windows/upgrade-scan.ps1`.
//!
//! The product command (what the launchers run; Windows):
//!
//!   upgrade-scan scan [--json] [--out <folder>] [--kit <stick root>] [--no-file]
//!       read this machine, print the report, write upgrade-report-<model>-<stamp>.txt
//!       (and .json) where upgrade-scan.ps1 -Json -OutDir writes them
//!   upgrade-scan dump-machine <file>
//!       the hardware-only capture upgrade-scan.ps1 -DumpMachine writes
//!
//! The comparison commands (how the port is proven side by side):
//!
//!   upgrade-scan --replay machine.json [--now 2026-10-04T10:00:00] [--json] [--against powershell.json]
//!   upgrade-scan --record [--out <folder>] [--kit <stick root>]   (Windows: read this machine, write a capture)
//!   upgrade-scan --compare-facts rust.json powershell.json
//!   upgrade-scan compare-reports rust-report.json powershell-report.json
//!
//! A capture made by tools/Record-Machine.ps1 also holds what the PowerShell
//! scanner concluded from the same facts. Then the replay compares the two
//! and says whether they are the same, line for line (exit 1 if not).
//!
//! Read-only on the machine: it reads, and writes only its report files.

use serde_json::{json, Value};
use std::process::ExitCode;
use upgrade_scan::facts::Machine;
use upgrade_scan::ps::Stamp;
use upgrade_scan::{collect, product, report, run, FOLLOWS_SCANNER};

fn utc_now() -> Stamp {
    collect::utc_from_seconds(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0))
}

/// `scan`: the scanner as the launchers run it.
fn scan(args: &[String]) -> ExitCode {
    let value_of = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    let json = args.iter().any(|a| a == "--json");
    let no_file = args.iter().any(|a| a == "--no-file");
    // the kit's boot files sit beside the scanner, as $PSScriptRoot
    let kit = value_of("--kit").map(std::path::PathBuf::from).or_else(|| std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())));
    println!();
    println!("  scanning...");
    let r = match product::scan_this_machine(kit.as_deref()) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("upgrade-scan: {e}");
            return ExitCode::from(2);
        }
    };
    println!();
    for line in &r.lines {
        println!("{line}");
    }
    for e in &r.read_errors {
        eprintln!("  read error: {e}");
    }
    if !no_file {
        let out = match value_of("--out").map(std::path::PathBuf::from).or_else(collect::desktop_folder) {
            Some(d) => d,
            None => {
                eprintln!("upgrade-scan: give --out <folder> (no Desktop folder to default to here)");
                return ExitCode::from(2);
            }
        };
        match product::write_report(&r, &out, json) {
            Ok((txt, js)) => {
                println!();
                println!("  Report saved: {}", txt.display());
                if let Some(p) = js {
                    println!("  JSON saved:   {}", p.display());
                }
            }
            Err(e) => {
                eprintln!("upgrade-scan: {e}");
                return ExitCode::from(2);
            }
        }
    }
    println!();
    ExitCode::SUCCESS
}

/// `dump-machine <file>`: the hardware-only capture for the corpus.
fn dump_machine(path: Option<&String>) -> ExitCode {
    let Some(path) = path else {
        eprintln!("usage: upgrade-scan dump-machine <file>");
        return ExitCode::from(2);
    };
    println!();
    println!("  capturing hardware enumeration...");
    let c = collect::collect_hardware();
    if c.facts.get("Sys").is_none() || c.facts.get("Pnp").is_none() {
        eprintln!("upgrade-scan: could not read this machine: {}", c.errors.join("; "));
        return ExitCode::from(2);
    }
    let (now, _) = collect::now();
    let doc = product::machine_capture(&c, &format!("{:04}-{:02}-{:02}", now.year, now.month, now.day));
    if let Err(e) = std::fs::write(path, serde_json::to_string_pretty(&doc).unwrap_or_default() + "\n") {
        eprintln!("upgrade-scan: cannot write {path}: {e}");
        return ExitCode::from(2);
    }
    println!("  machine capture written to {path}");
    println!("  review it, fill Expected, and add it to evaluate/windows/corpus/ to make it a permanent test.");
    ExitCode::SUCCESS
}

/// `compare-reports`: the Rust's JSON report against the PowerShell's, from
/// the same machine in the same minute.
fn compare_reports(rust_path: &str, ps_path: &str) -> ExitCode {
    let load = |p: &str| -> Result<Value, String> { serde_json::from_str(std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"))?.trim_start_matches('\u{feff}')).map_err(|e| format!("{p}: {e}")) };
    let (rust, ps) = match (load(rust_path), load(ps_path)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("upgrade-scan: {e}");
            return ExitCode::from(2);
        }
    };
    println!();
    println!("  Rust:       {rust_path}   ({})", rust["ScannedUtc"].as_str().unwrap_or(""));
    println!("  PowerShell: {ps_path}   ({})", ps["ScannedUtc"].as_str().unwrap_or(""));
    println!();
    let differences = product::report_differences(&rust, &ps);
    let checks = rust["Checks"].as_array().map_or(0, Vec::len);
    if differences.is_empty() {
        println!("  SAME: verdict {}, {checks} checks, the releases, the recommendation and the system facts, field for field.", rust["Verdict"]["Level"].as_str().unwrap_or(""));
        println!();
        return ExitCode::SUCCESS;
    }
    println!("  DIFFERENT in {} places:", differences.len());
    for d in &differences {
        println!("  {d}");
    }
    println!();
    ExitCode::from(1)
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
fn record(out: Option<String>, kit: Option<String>) -> ExitCode {
    let now = utc_now();
    let c = upgrade_scan::collect::collect(kit.as_deref().map(std::path::Path::new));
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
        // a whole fact not read yet is skipped; a field of one ("Fact.Field ...") is left out of the comparison
        if let Some(n) = not_read.iter().find(|n| n.split([' ', '.']).next() == Some(name) && !n.starts_with(&format!("{name}."))) {
            println!("    not read by Rust yet   {n}");
            continue;
        }
        let skipped: Vec<&str> = not_read.iter().filter_map(|n| n.strip_prefix(&format!("{name}."))).map(|rest| rest.split(' ').next().unwrap_or("")).collect();
        let (mut r, mut p) = (rust[name].clone(), ps[name].clone());
        for field in &skipped {
            for side in [&mut r, &mut p] {
                if let Some(m) = side.as_object_mut() {
                    m.remove(*field);
                }
            }
            println!("    not read by Rust yet   {name}.{field}");
        }
        let diffs = fact_differences(name, &r, &p);
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

/// Two numbers within 2 percent and one unit of each other.
fn within_drift(a: &Value, b: &Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(p), Some(q)) => (p - q).abs() <= p.abs().max(q.abs()) * 0.02 + 1.0,
        _ => a == b,
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
    // values that move by the minute: free space, a temperature, hours of
    // use. Two recorders a minute apart differ here without being wrong.
    let drifts = |field: &str| matches!((name, field), ("Disk", "SysVolume") | ("Disk", "ShrinkGB") | ("Esp", "FreeBytes") | ("PhysicalDisk", "Counters"));
    let within = |a: &Value, b: &Value| -> bool {
        match (a, b) {
            (Value::Number(x), Value::Number(y)) => match (x.as_f64(), y.as_f64()) {
                (Some(p), Some(q)) => (p - q).abs() <= p.abs().max(q.abs()) * 0.02 + 1.0,
                _ => false,
            },
            (Value::Object(x), Value::Object(y)) => x.len() == y.len() && x.iter().all(|(k, p)| y.get(k).is_some_and(|q| same_value(p, q) || within_drift(p, q))),
            _ => same_value(a, b),
        }
    };
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
                        (Some(x), Some(y)) if drifts(k) && within(x, y) => {}
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
    eprintln!("upgrade-scan {} (Rust; follows scanner {FOLLOWS_SCANNER})", env!("CARGO_PKG_VERSION"));
    eprintln!("usage: upgrade-scan scan [--json] [--out <folder>] [--kit <stick root>] [--no-file]");
    eprintln!("       upgrade-scan dump-machine <file>");
    eprintln!("       upgrade-scan --replay <machine.json> [--now YYYY-MM-DDTHH:MM:SS] [--json] [--against <powershell.json>]");
    eprintln!("       upgrade-scan --record [--out <folder>] [--kit <stick root>]");
    eprintln!("       upgrade-scan --compare-facts <rust.json> <powershell.json>");
    eprintln!("       upgrade-scan compare-reports <rust-report.json> <powershell-report.json>");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let value_of = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    if args.iter().any(|a| a == "--version") {
        println!("upgrade-scan {} (Rust; follows scanner {FOLLOWS_SCANNER})", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    match args.first().map(String::as_str) {
        Some("scan") => return scan(&args[1..]),
        Some("dump-machine") => return dump_machine(args.get(1)),
        Some("compare-reports") => {
            let (Some(a), Some(b)) = (args.get(1), args.get(2)) else { return usage() };
            return compare_reports(a, b);
        }
        _ => {}
    }
    if let Some(i) = args.iter().position(|a| a == "--try-wmi") {
        for line in upgrade_scan::collect::try_wmi(args.get(i + 1).map(String::as_str)) {
            println!("  {line}");
        }
        return ExitCode::SUCCESS;
    }
    if args.iter().any(|a| a == "--record") {
        return record(value_of("--out"), value_of("--kit"));
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
    let mut captured_now = doc.get("Now").and_then(Value::as_str).map(str::to_string);
    // --against: the PowerShell scanner's conclusions from its own capture
    // of the same machine, to compare with what Rust concludes from what
    // Rust itself read
    let mut powershell = doc.get("PowerShell").cloned();
    if let Some(other) = value_of("--against") {
        match std::fs::read_to_string(&other).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str::<Value>(t.trim_start_matches('\u{feff}')).map_err(|e| e.to_string())) {
            Ok(v) => {
                powershell = v.get("PowerShell").cloned();
                // the report's clock line comes from the PowerShell run being compared with
                if let Some(n) = v.get("Now").and_then(Value::as_str) {
                    captured_now = Some(n.to_string());
                }
            }
            Err(e) => {
                eprintln!("upgrade-scan: {other}: {e}");
                return ExitCode::from(2);
            }
        }
        if powershell.is_none() {
            eprintln!("upgrade-scan: {other} holds no PowerShell block");
            return ExitCode::from(2);
        }
    }
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
