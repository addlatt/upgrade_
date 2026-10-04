//! The Rust harvester's pure half against Harvest-UpgradeState.ps1 (RISKS
//! R32; VALIDATION V13). `cases.json` lists the calls: the PowerShell
//! self-test's pure cases under their own names, plus more inputs.
//! `golden.json` is what the PowerShell returned (written by `golden.ps1`).

use serde_json::{json, Value};
use upgrade_harvest::capacity::{capacity, Drive};
use upgrade_harvest::stick::{stick_fit, Folder, RESERVE_BYTES};
use upgrade_harvest::{cloud, names, wlan};

fn cases() -> Vec<Value> {
    serde_json::from_str(include_str!("cases.json")).unwrap()
}

fn int(a: &Value, name: &str) -> i64 {
    a[name].as_i64().unwrap_or_else(|| panic!("argument {name}"))
}

fn run(fn_name: &str, a: &Value) -> Value {
    match fn_name {
        "ConvertTo-HarvestIanaTimeZone" => json!(names::iana_time_zone(a["WindowsTimeZoneId"].as_str().unwrap())),
        "ConvertTo-HarvestLinuxName" => json!(names::linux_name(a["WindowsName"].as_str().unwrap())),
        "Test-HarvestPlaceholderAttributes" => json!(cloud::is_placeholder(int(a, "Attributes"))),
        "Test-HarvestMaterialized" => json!(cloud::is_materialized(int(a, "Attributes"), int(a, "Length"), int(a, "BytesRead"), int(a, "AllocatedBytes"))),
        "Get-HarvestStickFit" => {
            let folders: Vec<Folder> = a["Folders"]
                .as_array()
                .unwrap()
                .iter()
                .map(|f| Folder { exists: f["Exists"].as_bool().unwrap(), bytes: int(f, "Bytes"), stick_bytes: int(f, "StickBytes"), files_over_4gib: int(f, "FilesOver4GiB") })
                .collect();
            let reserve = a.get("ReserveBytes").and_then(Value::as_i64).unwrap_or(RESERVE_BYTES);
            let r = stick_fit(&folders, int(a, "FreeBytes"), a["FileSystem"].as_str().unwrap(), int(a, "ClusterBytes"), reserve);
            json!({"FileSystem": r.file_system, "ClusterBytes": r.cluster_bytes, "FreeBytes": r.free_bytes, "FilesBytes": r.files_bytes, "NeededBytes": r.needed_bytes,
                   "FilesOver4GiB": r.files_over_4gib, "Fits": r.fits, "GapBytes": r.gap_bytes, "Reason": r.reason})
        }
        "ConvertFrom-HarvestWlanProfileXml" => {
            // as netsh writes it: UTF-8 with a byte order mark
            let mut bytes = vec![0xEF, 0xBB, 0xBF];
            bytes.extend_from_slice(a["Xml"].as_str().unwrap().as_bytes());
            match wlan::parse_profile(&bytes, a["IncludeSecrets"].as_bool().unwrap()) {
                Ok(p) => json!({"Ssid": p.ssid, "Authentication": p.authentication, "NmKeyMgmt": p.nm_key_mgmt, "Supported": p.supported, "AutoConnect": p.auto_connect, "HasSecret": p.has_secret, "Secret": p.secret}),
                Err(_) => json!({"threw": true}),
            }
        }
        "Get-HarvestCapacity" => {
            let bytes = |name: &str| a[name].as_array().unwrap().iter().map(|x| int(x, "Bytes")).collect::<Vec<_>>();
            let drives: Vec<Drive> = a["ExternalDrives"].as_array().unwrap().iter().map(|d| Drive { drive_letter: d["DriveLetter"].as_str().unwrap().to_string(), free_bytes: int(d, "FreeBytes") }).collect();
            let c = capacity(&bytes("UserFolders"), &bytes("Browsers"), &drives, int(a, "WindowsUsedBytes"));
            json!({"WindowsUsedBytes": c.windows_used_bytes, "UserDataBytes": c.user_data_bytes, "BrowserDataBytes": c.browser_data_bytes, "BackupNeededBytes": c.backup_needed_bytes,
                   "BackupNeededGB": c.backup_needed_gb, "ExternalPresent": c.external_present, "ExternalSufficient": c.external_sufficient,
                   "BestExternal": c.best_external.map(|d| json!({"DriveLetter": d.drive_letter, "FreeBytes": d.free_bytes}))})
        }
        other => panic!("cases.json names a function this test does not know: {other}"),
    }
}

/// Equal as JSON, numbers as numbers (122 equals 122.0).
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same(p, q)),
        (Value::Object(x), Value::Object(y)) => x.len() == y.len() && x.iter().all(|(k, p)| y.get(k).is_some_and(|q| same(p, q))),
        _ => a == b,
    }
}

#[test]
fn every_call_matches_powershell() {
    let golden: Value = serde_json::from_str(include_str!("golden.json")).unwrap();
    let cases = cases();
    assert_eq!(golden.as_object().unwrap().len(), cases.len(), "golden.json is stale: run ./port-check.sh --record");
    let mut wrong = Vec::new();
    for case in &cases {
        let name = case["name"].as_str().unwrap();
        for (i, call) in case["calls"].as_array().unwrap().iter().enumerate() {
            let got = run(call["fn"].as_str().unwrap(), &call["args"]);
            let want = &golden[name][i];
            if !same(want, &got) {
                wrong.push(format!("FAIL  {name} (call {i}, {})\n  PowerShell: {want}\n  Rust:       {got}", call["fn"].as_str().unwrap()));
            }
        }
    }
    assert!(wrong.is_empty(), "{} differ from the PowerShell harvester:\n\n{}", wrong.len(), wrong.join("\n\n"));
}

/// The PowerShell self-test's own assertions, held here directly.
#[test]
fn the_self_test_expectations_hold() {
    let mut wrong = Vec::new();
    let mut counted = 0;
    for case in cases().iter().filter(|c| c["origin"] == "selftest") {
        counted += 1;
        let name = case["name"].as_str().unwrap();
        for call in case["calls"].as_array().unwrap() {
            let got = run(call["fn"].as_str().unwrap(), &call["args"]);
            let mut asserted = false;
            if let Some(want) = call.get("want_value") {
                asserted = true;
                if !same(want, &got) {
                    wrong.push(format!("FAIL  {name}: returned {got}, expected {want}"));
                }
            }
            for (field, want) in call.get("want").and_then(Value::as_object).into_iter().flatten() {
                asserted = true;
                if !same(want, &got[field]) {
                    wrong.push(format!("FAIL  {name}: {field} was {}, expected {want}", got[field]));
                }
            }
            for (field, pattern) in call.get("want_match").and_then(Value::as_object).into_iter().flatten() {
                asserted = true;
                let re = regress::Regex::with_flags(pattern.as_str().unwrap(), "i").unwrap();
                if re.find(got[field].as_str().unwrap_or("")).is_none() {
                    wrong.push(format!("FAIL  {name}: {field} was {}, expected to match {pattern}", got[field]));
                }
            }
            assert!(asserted, "{name}: a self-test case must assert something");
        }
    }
    assert!(counted >= 35, "only {counted} self-test cases in cases.json");
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn a_profile_that_is_not_one_is_an_error_not_a_crash() {
    for bad in [&b"not xml"[..], b"<other/>", b"\xff\xfe<", b"\xc3\x28"] {
        assert!(wlan::parse_profile(bad, true).is_err());
    }
}
