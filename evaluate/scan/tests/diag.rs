//! `upgrade-scan diag` against the two scripts it replaced, line for line
//! (RISKS R32; VALIDATION V13).
//!
//! - `diag-cases.json`: made-up facts, in the shape the collectors fill
//!   (written by `make-diag-cases.py`).
//! - `diag-golden.json`: what `Diag-SecureBoot.ps1` and `Diag-Smart.ps1`
//!   wrote for each, recorded by `diag-golden.ps1` through mocked cmdlets
//!   before the scripts were deleted (2026-10-14). Frozen
//!   (`FROZEN-GOLDENS.sha256`).

use serde_json::Value;
use upgrade_scan::diag::{en_us, secureboot, smart};
use upgrade_scan::parse::smart_table;
use upgrade_scan::ps::Stamp;

fn cases() -> Vec<Value> {
    serde_json::from_str(include_str!("diag-cases.json")).unwrap()
}

fn golden() -> Value {
    serde_json::from_str(include_str!("diag-golden.json")).unwrap()
}

fn render(case: &Value) -> Vec<String> {
    match case["diag"].as_str().unwrap() {
        "secureboot" => secureboot::render(&serde_json::from_value(case["facts"].clone()).expect("secureboot facts")),
        "smart" => smart::render(&serde_json::from_value(case["facts"].clone()).expect("smart facts")),
        other => panic!("no diag {other}"),
    }
}

#[test]
fn every_case_says_what_the_script_said() {
    let golden = golden();
    let mut failures = Vec::new();
    let mut count = 0;
    for case in cases() {
        let name = case["name"].as_str().unwrap();
        let ours = render(&case);
        let theirs: Vec<String> = golden[name].as_array().unwrap_or_else(|| panic!("no golden for {name}")).iter().map(|v| v.as_str().unwrap().to_string()).collect();
        count += 1;
        if ours == theirs {
            continue;
        }
        let at = ours.iter().zip(&theirs).position(|(a, b)| a != b).unwrap_or(ours.len().min(theirs.len()));
        failures.push(format!(
            "{name}\n  Rust {} entries, script {}; first difference at entry {}:\n    script: {:?}\n    Rust:   {:?}",
            ours.len(),
            theirs.len(),
            at + 1,
            theirs.get(at),
            ours.get(at)
        ));
    }
    assert!(failures.is_empty(), "{} of {count} cases differ from the scripts:\n{}", failures.len(), failures.join("\n"));
    assert_eq!(count, 6, "the six cases");
}

#[test]
fn the_facts_round_trip_through_json() {
    for case in cases() {
        let again = match case["diag"].as_str().unwrap() {
            "secureboot" => serde_json::to_value(serde_json::from_value::<secureboot::Facts>(case["facts"].clone()).unwrap()).unwrap(),
            _ => serde_json::to_value(serde_json::from_value::<smart::Facts>(case["facts"].clone()).unwrap()).unwrap(),
        };
        let mut c = case.clone();
        c["facts"] = again;
        assert_eq!(render(&c), render(&case), "{}", case["name"]);
    }
}

#[test]
fn the_aspire_shaped_case_names_its_numbers() {
    let case = cases().into_iter().find(|c| c["name"].as_str().unwrap().contains("Aspire")).unwrap();
    let lines = render(&case);
    assert!(lines.iter().any(|l| l.contains("187  Reported_Uncorrectable") && l.contains(" 725  (low16=725)")), "{lines:#?}");
    assert!(lines.iter().any(|l| l.contains("5    Reallocated_Sector_Ct") && l.contains("0x0033")));
    assert!(lines.iter().any(|l| l.contains("168  vendor_168")));
    assert!(lines.iter().any(|l| l == "  id 7: 4 events, first 8/27/2026 6:45:00 PM, last 9/13/2026 10:10:04 AM"));
}

#[test]
fn dates_print_as_an_english_windows_prints_them() {
    let t = |s: &str| en_us(&Stamp::parse(s).unwrap());
    assert_eq!(t("2026-10-03T16:01:02"), "10/3/2026 4:01:02 PM");
    assert_eq!(t("2026-10-03T00:05:00"), "10/3/2026 12:05:00 AM");
    assert_eq!(t("2026-10-03T12:00:00"), "10/3/2026 12:00:00 PM");
    assert_eq!(t("2026-01-09T09:07:03"), "1/9/2026 9:07:03 AM");
}

#[test]
fn the_smart_table_reads_every_entry() {
    let mut b = vec![0u8; 512];
    b[0] = 0x10;
    b[2..14].copy_from_slice(&[187, 0x32, 0x00, 100, 99, 0xd5, 0x02, 0, 0, 0, 0, 0]);
    b[14..26].copy_from_slice(&[187, 0x32, 0x00, 100, 99, 0xd5, 0x02, 0, 0, 0, 0, 0]);
    b[26..38].copy_from_slice(&[194, 0x22, 0x00, 65, 50, 35, 0, 50, 0, 20, 0, 0]);
    let rows = smart_table(&b);
    assert_eq!(rows.len(), 3, "duplicates are kept");
    assert_eq!((rows[0].id, rows[0].flags, rows[0].current, rows[0].worst, rows[0].raw, rows[0].raw16), (187, 0x32, 100, 99, 725, 725));
    assert_eq!((rows[2].raw, rows[2].raw16), (35 | (50 << 16) | (20 << 32), 35));
    assert!(smart_table(&[0x10, 0]).is_empty());
}

#[test]
fn the_files_are_named_as_the_scripts_named_them() {
    let root = std::path::Path::new("stick");
    let tail = |p: std::path::PathBuf| p.iter().map(|c| c.to_string_lossy().into_owned()).collect::<Vec<_>>();
    assert_eq!(tail(secureboot::default_out(root, "2026-10-14T16:01:02.1234567Z")), ["stick", "upgrade_", "report", "secureboot-diag-20261014T160102Z.txt"]);
    assert_eq!(tail(smart::default_out(root, "ASPIRE")), ["stick", "upgrade_", "reports", "smart-diag-ASPIRE.txt"]);
}
