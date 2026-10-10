//! Invoke-Rollback.ps1's judging half, replayed: every call in
//! rollback-cases.json answered by `rollback.rs` and compared with what the
//! script answered (rollback-golden.json, from rollback-golden.ps1 on
//! Windows). The script's own self-test names must all be here.

use serde_json::{json, Value};
use upgrade_prologue::{judge, rollback};

fn load(name: &str) -> Value {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join(name);
    serde_json::from_str(&std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))).unwrap()
}

fn s(v: &Value) -> String {
    v.as_str().unwrap_or("").to_string()
}

fn fixture_sums() -> Value {
    rollback::sums(&["aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  ./EFI/Boot/bootx64.efi".to_string(), "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb  ./EFI/Microsoft/Boot/bootmgfw.efi".to_string()])
}

fn fixture_out() -> Value {
    json!({"schema": "outcome/1", "path_taken": "keep-windows", "windows": {"kept": true}, "cutover": {"esp_snapshot": {"path": "upgrade_/esp-snapshot"}}})
}

fn sums_of(a: &Value) -> Value {
    if a.as_str() == Some("fixture") {
        return fixture_sums();
    }
    rollback::sums(&a.as_array().unwrap().iter().map(s).collect::<Vec<_>>())
}

fn out_of(a: &Value) -> Value {
    match a {
        Value::Null => Value::Null,
        Value::String(f) if f == "fixture" => fixture_out(),
        Value::Object(edits) => {
            let mut o = fixture_out();
            for (path, v) in edits {
                let parts: Vec<&str> = path.split('.').collect();
                let mut cur = &mut o;
                for p in &parts[..parts.len() - 1] {
                    cur = &mut cur[*p];
                }
                cur[parts[parts.len() - 1]] = v.clone();
            }
            o
        }
        other => panic!("an outcome argument must be null, \"fixture\" or edits, not {other}"),
    }
}

fn answer(func: &str, a: &[Value]) -> Value {
    match func {
        "sums" => {
            // the script's hashtable keys come back sorted
            let v = sums_of(&a[0]);
            let mut m: Vec<(String, Value)> = v.as_object().unwrap().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            m.sort_by(|x, y| x.0.cmp(&y.0));
            Value::Object(m.into_iter().collect())
        }
        "plan" => {
            let mism: Vec<String> = a[3].as_array().unwrap().iter().map(s).collect();
            let p = rollback::plan(&out_of(&a[0]), &sums_of(&a[1]), &s(&a[2]), &mism);
            json!({"Refusals": p["Refusals"], "Restore": p["Restore"], "WantSha": p["WantSha"]})
        }
        "identity" => {
            let job = json!({"identity": {"system_disk": {"unique_id": "eui.1", "size_bytes": 100}}});
            Value::Array(a[0].as_array().unwrap().iter().map(|d| json!(rollback::compare_identity(&job, d))).collect())
        }
        "displayorder" => json!(rollback::display_order_tokens(&s(&a[0]))),
        "windows_first" => Value::Array(a[0].as_array().unwrap().iter().map(|t| Value::Bool(rollback::windows_first(&t.as_array().unwrap().iter().map(s).collect::<Vec<_>>()))).collect()),
        "drive" => Value::Array(a[0].as_array().unwrap().iter().map(|d| Value::String(judge::drive_root(&s(d)).unwrap_or_else(|_| "refused".into()))).collect()),
        other => panic!("no Rust answer for {other}"),
    }
}

#[test]
fn every_call_matches_invoke_rollback() {
    let cases = load("rollback-cases.json");
    let golden = load("rollback-golden.json");
    assert_eq!(s(&golden["rollback_version"]), rollback::FOLLOWS_ROLLBACK, "the golden was recorded from another Invoke-Rollback.ps1 version");
    let calls = golden["calls"].as_array().expect("golden calls");
    let mut failures = vec![];
    let mut n = 0;
    for c in cases.as_array().unwrap() {
        let name = s(&c["name"]);
        let g = calls.iter().find(|g| s(&g["name"]) == name).unwrap_or_else(|| panic!("no golden answer for '{name}' (run tests/rollback-golden.ps1)"));
        let got = answer(&s(&c["fn"]), c["args"].as_array().unwrap());
        n += 1;
        if got != g["result"] {
            failures.push(format!("  {name}\n    powershell: {}\n    rust:       {}", g["result"], got));
        }
    }
    assert!(failures.is_empty(), "{} of {n} calls differ from Invoke-Rollback.ps1:\n{}", failures.len(), failures.join("\n"));
    assert!(n >= 14, "the script's self-test has 14 cases; only {n} replayed");
}

#[test]
fn the_self_test_is_all_here() {
    let script = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../windows/Invoke-Rollback.ps1")).expect("Invoke-Rollback.ps1 beside the crate");
    let cases = load("rollback-cases.json");
    let names: Vec<String> = cases.as_array().unwrap().iter().map(|c| s(&c["name"])).collect();
    let mut missing = vec![];
    let mut count = 0;
    for line in script.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("@{ Name = '") {
            // the name ends at a quote that is not doubled (a doubled quote is a quote in the name)
            let b = rest.as_bytes();
            let mut end = None;
            let mut i = 0;
            while i < b.len() {
                if b[i] == b'\'' {
                    if b.get(i + 1) == Some(&b'\'') {
                        i += 2;
                        continue;
                    }
                    end = Some(i);
                    break;
                }
                i += 1;
            }
            if let Some(end) = end {
                let n = rest[..end].replace("''", "'");
                count += 1;
                if !names.contains(&n) {
                    missing.push(n);
                }
            }
        }
    }
    assert!(count >= 14, "found only {count} self-test names in the script");
    assert!(missing.is_empty(), "self-test cases without a replay here:\n  {}", missing.join("\n  "));
}
