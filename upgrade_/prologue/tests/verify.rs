//! Test-Handoff.ps1's judging half, replayed: every call in
//! verify-cases.json answered by the Rust (`verify.rs` with `judge.rs`) and
//! compared with what the script answered (verify-golden.json, written by
//! verify-golden.ps1 on Windows). The self-test's own case names must all
//! be here, so a case added to the script shows up as a missing name.

use serde_json::Value;
use upgrade_prologue::{judge, verify};

fn load(name: &str) -> Value {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join(name);
    serde_json::from_str(&std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))).unwrap()
}

fn s(v: &Value) -> String {
    v.as_str().unwrap_or("").to_string()
}

fn answer(func: &str, a: &[Value]) -> Value {
    match func {
        "handoff_result" => Value::String(verify::handoff_result(a[0].as_bool().unwrap(), a[1].as_bool().unwrap(), a[2].as_bool().unwrap(), &s(&a[3])).into()),
        "manage_bde" => Value::String(judge::manage_bde(&a[0].as_array().unwrap().iter().map(s).collect::<Vec<_>>())),
        "grubenv_clean" => {
            let b = judge::grub_env_block();
            Value::String(if b.len() == 1024 && &b[..25] == b"# GRUB Environment Block\n" && b[1023] == b'#' { "ok".into() } else { format!("bad: len={}", b.len()) })
        }
        "grubenv_fired_clean" => Value::Bool(judge::grub_env_fired(&judge::grub_env_block())),
        "grubenv_fired_text" => {
            let mut t = s(&a[0]).into_bytes();
            t.extend(std::iter::repeat(b'#').take(a[1].as_u64().unwrap() as usize));
            Value::Bool(judge::grub_env_fired(&t))
        }
        "payload_path" => Value::String(verify::payload_path(&s(&a[0]), &s(&a[1])).unwrap_or_else(|_| "refused".into())),
        "find_stick_root" => Value::String(judge::find_stick_root(&a[0], &s(&a[1])).unwrap_or_else(|| "null".into())),
        "csv_header_bom" => {
            let b = verify::csv_header_bytes();
            Value::String(if b.starts_with(&[0xEF, 0xBB, 0xBF]) { "bom".into() } else { "no-bom".into() })
        }
        "csv_header_text" => {
            let t = String::from_utf8(verify::csv_header_bytes()).unwrap();
            let first = t.lines().next().unwrap_or("");
            Value::String(if first == verify::CSV_HEADER { "ok".into() } else { format!("differs: {first}") })
        }
        "drive_root" => Value::String(verify::payload_drive_root(&s(&a[0])).unwrap_or_else(|_| "refused".into())),
        other => panic!("no Rust answer for {other}"),
    }
}

#[test]
fn every_call_matches_test_handoff() {
    let cases = load("verify-cases.json");
    let golden = load("verify-golden.json");
    assert_eq!(s(&golden["harness_version"]), verify::FOLLOWS_HARNESS, "the golden was recorded from another Test-Handoff.ps1 version");
    let calls = golden["calls"].as_array().expect("golden calls");
    let mut failures = vec![];
    let mut n = 0;
    for c in cases.as_array().unwrap() {
        let name = s(&c["name"]);
        let g = calls.iter().find(|g| s(&g["name"]) == name).unwrap_or_else(|| panic!("no golden answer for '{name}' (run tests/verify-golden.ps1)"));
        let got = answer(&s(&c["fn"]), c["args"].as_array().unwrap());
        n += 1;
        if got != g["result"] {
            failures.push(format!("  {name}\n    powershell: {}\n    rust:       {}", g["result"], got));
        }
    }
    assert!(failures.is_empty(), "{} of {n} calls differ from Test-Handoff.ps1:\n{}", failures.len(), failures.join("\n"));
    assert!(n >= 33, "the harness self-test has 33 cases; only {n} replayed");
}

#[test]
fn the_constants_are_the_scripts() {
    let golden = load("verify-golden.json");
    assert_eq!(s(&golden["csv_header"]), verify::CSV_HEADER);
    assert_eq!(s(&golden["payload_paths"]["shim"]), verify::payload_path("shim", "").unwrap());
    assert_eq!(s(&golden["payload_paths"]["shell"]), verify::payload_path("shell", "").unwrap());
    assert_eq!(s(&golden["return_task_name"]), verify::RETURN_TASK_NAME);
    assert_eq!(s(&golden["fired_marker"]), verify::FIRED_MARKER);
}

/// Every name in the script's own self-test is a case here (the ledger's
/// lines for Test-Handoff.ps1 are its self-test names).
#[test]
fn the_self_test_is_all_here() {
    let script = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../windows/Test-Handoff.ps1")).expect("Test-Handoff.ps1 beside the crate");
    let cases = load("verify-cases.json");
    let names: Vec<String> = cases.as_array().unwrap().iter().map(|c| s(&c["name"])).collect();
    let mut missing = vec![];
    let mut count = 0;
    for line in script.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("@{ Name = '") {
            if let Some(end) = rest.find('\'') {
                let n = rest[..end].replace("''", "'");
                count += 1;
                if !names.contains(&n) {
                    missing.push(n);
                }
            }
        }
    }
    assert!(count >= 33, "found only {count} self-test names in the script");
    assert!(missing.is_empty(), "self-test cases without a replay here:\n  {}", missing.join("\n  "));
}
