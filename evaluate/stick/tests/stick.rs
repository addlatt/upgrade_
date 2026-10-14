//! The stick writer against Write-UpgradeStick.ps1 0.1.1 (RISKS R16, R32;
//! VALIDATION V13). `stick-cases.json` holds the inputs (by
//! `make-stick-cases.py`); `stick-golden.json` is what the script
//! answered and did for each (recorded by `stick-golden.ps1`, frozen):
//! for a function case its return, for a flow case every line printed,
//! the prompt, the write asked for, the evidence row, the stick.json and
//! the exit code. The Rust must match, act for act.
//!
//! One difference is allowed on purpose (RUST-PORT.md): the evidence
//! row's `writer` field names this program, where the script wrote its
//! own version.

use serde_json::{json, Value};
use std::collections::VecDeque;
use upgrade_stick::flow::{self, Args, Live, WriteResult};
use upgrade_stick::judge::{self, s};

fn cases() -> Vec<Value> {
    serde_json::from_str(include_str!("stick-cases.json")).unwrap()
}

fn golden() -> Value {
    serde_json::from_str(include_str!("stick-golden.json")).unwrap()
}

struct Fake {
    case: Value,
    events: Vec<Value>,
    csv: Vec<String>,
}

impl Live for Fake {
    fn elevated(&self) -> bool {
        self.case["elevated"].as_bool().unwrap_or(false)
    }
    fn now_o(&self) -> String {
        s(&self.case, "now")
    }
    fn attached_disks(&mut self) -> Vec<Value> {
        judge::items(&self.case, "disks")
    }
    fn machine(&self) -> Value {
        self.case["machine"].clone()
    }
    fn read_host(&mut self, prompt: &str) -> String {
        self.events.push(json!({"read-host": prompt}));
        s(&self.case, "answer")
    }
    fn source_has_sums(&self, _source: &str) -> bool {
        self.case["source_has_sums"].as_bool().unwrap_or(true)
    }
    fn write_stick(&mut self, disk: &Value, source: &str, boot_label: &str, boot_gb: i64) -> Result<WriteResult, String> {
        self.events.push(json!({"write": s(disk, "UniqueId"), "source": source, "label": boot_label, "boot_gb": boot_gb}));
        let w = &self.case["write"];
        if let Some(e) = w.get("error") {
            return Err(s(w, "error").clone() + &String::new() + e.as_str().map(|_| "").unwrap_or(""));
        }
        Ok(WriteResult {
            written: w["Written"].as_bool().unwrap_or(false),
            verified: w["Verified"].as_bool().unwrap_or(false),
            files: judge::int(w, "Files"),
            failed: judge::items(w, "Failed").iter().map(|x| x.as_str().unwrap_or("").to_string()).collect(),
            boot_letter: s(w, "BootLetter"),
            data_letter: s(w, "DataLetter"),
            notes: judge::items(w, "Notes").iter().map(|x| x.as_str().unwrap_or("").to_string()).collect(),
            stick: w["Stick"].clone(),
        })
    }
    fn results_csv(&self) -> String {
        "<csv>".into()
    }
    fn append_evidence(&mut self, _csv: &str, line: &str) {
        if self.csv.is_empty() {
            self.csv.push(judge::CSV_HEADER.to_string());
        }
        self.csv.push(line.to_string());
    }
    fn out_file(&mut self, path: &str, text: &str) {
        self.events.push(json!({"out-file": path, "doc": serde_json::from_str::<Value>(text).unwrap_or(Value::Null)}));
    }
    fn host(&mut self, line: &str) {
        self.events.push(json!({"host": line}));
    }
}

/// The Rust's value against the golden's: objects without regard to key
/// order, numbers as numbers.
fn same(rust: &Value, gold: &Value) -> bool {
    match (rust, gold) {
        (Value::Object(a), Value::Object(b)) => a.len() == b.len() && a.iter().all(|(k, v)| b.get(k).is_some_and(|w| same(v, w))),
        (Value::Array(a), Value::Array(b)) => a.len() == b.len() && a.iter().zip(b).all(|(x, y)| same(x, y)),
        (Value::Number(a), Value::Number(b)) => a.as_f64() == b.as_f64(),
        _ => rust == gold,
    }
}

fn run_fn(case: &Value) -> Value {
    let a = &case["args"];
    let f = |k: &str| a[k].as_f64().unwrap_or(0.0);
    let i = |k: &str| judge::int(a, k);
    match s(case, "fn").as_str() {
        "Resolve-UpgStickTarget" => judge::resolve(&judge::items(a, "Disks"), &s(a, "Target"), i("ExpectedBytes"), f("ExpectedGB")),
        "Test-UpgStickSize" => json!(judge::size_matches(i("SizeBytes"), i("ExpectedBytes"), f("ExpectedGB"))),
        "Get-UpgDiskRefusals" => json!(judge::refusals(&a["Disk"], &s(a, "Target"), i("ExpectedBytes"), f("ExpectedGB"))),
        "Get-UpgConfirmWord" => json!(judge::confirm_word(&a["Disk"])),
        "ConvertFrom-UpgSha256Sums" => {
            let lines: Vec<String> = judge::items(a, "Lines").iter().map(|l| l.as_str().unwrap_or("").to_string()).collect();
            let mut m = serde_json::Map::new();
            for (k, v) in judge::sha256sums(&lines) {
                m.insert(k, json!(v));
            }
            Value::Object(m)
        }
        "Format-DiskLine" => json!(judge::disk_line(&a["Disk"])),
        other => panic!("no such function here: {other}"),
    }
}

/// A golden object whose keys compare without regard to case (a PowerShell
/// hashtable's), against the Rust's.
fn same_map_ci(rust: &Value, gold: &Value) -> bool {
    let (Some(a), Some(b)) = (rust.as_object(), gold.as_object()) else { return same(rust, gold) };
    a.len() == b.len() && a.iter().all(|(k, v)| b.iter().any(|(gk, gv)| gk.eq_ignore_ascii_case(k) && same(v, gv)))
}

#[test]
fn every_function_answers_what_the_script_answered() {
    let g = golden();
    let mut failed = Vec::new();
    let mut n = 0;
    for case in cases().iter().filter(|c| s(c, "kind") == "fn") {
        n += 1;
        let name = s(case, "name");
        let want = &g[&name]["result"];
        let got = run_fn(case);
        let ok = if s(case, "fn") == "ConvertFrom-UpgSha256Sums" { same_map_ci(&got, want) } else { same(&got, want) };
        if !ok {
            failed.push(format!("{name}\n    got  {got}\n    want {want}"));
        }
    }
    assert!(n >= 40, "the cases file lost its function cases");
    assert!(failed.is_empty(), "{} of {n} function cases differ from the script:\n  {}", failed.len(), failed.join("\n  "));
}

/// The evidence row with the `writer` field (the one allowed difference) blanked.
fn row_without_writer(line: &str) -> String {
    let mut parts: VecDeque<&str> = line.splitn(3, ',').collect();
    if parts.len() == 3 {
        let ts = parts.pop_front().unwrap();
        parts.pop_front();
        let rest = parts.pop_front().unwrap();
        return format!("{ts},\"<writer>\",{rest}");
    }
    line.to_string()
}

#[test]
fn every_flow_does_what_the_script_did() {
    let g = golden();
    let mut failed = Vec::new();
    let mut n = 0;
    for case in cases().iter().filter(|c| s(c, "kind") == "flow") {
        n += 1;
        let name = s(case, "name");
        let want = &g[&name];
        let mut fake = Fake { case: case.clone(), events: Vec::new(), csv: Vec::new() };
        let a = Args {
            phase: s(case, "phase"),
            target: s(case, "target"),
            expected_size_bytes: judge::int(case, "expected_bytes"),
            expected_size_gb: case["expected_gb"].as_f64().unwrap_or(0.0),
            source: s(case, "source"),
            label: s(case, "label"),
            boot_size_gb: judge::int(case, "boot_size_gb"),
            confirmed_label: Some(s(case, "confirmed_label")).filter(|c| !c.is_empty()),
        };
        let (exit, error) = match flow::run(&mut fake, &a) {
            Ok(code) => (code, Value::Null),
            Err(e) => (1, json!(e)),
        };
        let mut why = Vec::new();
        if exit != judge::int(want, "exit") as i32 {
            why.push(format!("exit {exit}, the script's {}", judge::int(want, "exit")));
        }
        if !same(&error, &want["error"]) {
            why.push(format!("error {error}, the script's {}", want["error"]));
        }
        let gold_events = judge::items(want, "events");
        for i in 0..fake.events.len().max(gold_events.len()) {
            match (fake.events.get(i), gold_events.get(i)) {
                (Some(a), Some(b)) if same(a, b) => {}
                (a, b) => {
                    why.push(format!("event {i}:\n      got  {}\n      want {}", a.map_or("(none)".into(), Value::to_string), b.map_or("(none)".into(), Value::to_string)));
                    break;
                }
            }
        }
        let gold_csv: Vec<String> = judge::items(want, "csv").iter().map(|l| row_without_writer(l.as_str().unwrap_or(""))).collect();
        let got_csv: Vec<String> = fake.csv.iter().map(|l| row_without_writer(l)).collect();
        if got_csv != gold_csv {
            why.push(format!("csv:\n      got  {got_csv:?}\n      want {gold_csv:?}"));
        }
        if !why.is_empty() {
            failed.push(format!("{name}\n    {}", why.join("\n    ")));
        }
    }
    assert!(n >= 15, "the cases file lost its flow cases");
    assert!(failed.is_empty(), "{} of {n} flow cases differ from the script:\n  {}", failed.len(), failed.join("\n  "));
}
