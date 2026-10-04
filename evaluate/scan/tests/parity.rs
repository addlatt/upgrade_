//! The Rust judging half against the PowerShell scanner, word for word
//! (RISKS R32; VALIDATION V13).
//!
//! - `cases.json`: one list of inputs. `origin: selftest` cases are the
//!   PowerShell `-SelfTest` cases under their own names; `corpus` cases are
//!   the machine recordings; `port` cases reach wording the self-test never
//!   reaches.
//! - `golden.json`: what the PowerShell scanner answers for each, written by
//!   `golden.ps1` (run by `./port-check.sh`).
//!
//! Two tests: the answers here equal the PowerShell's in full, and the
//! self-test's own expectations hold here without looking at the golden file.

use serde::de::DeserializeOwned;
use serde_json::{json, Map, Value};
use std::path::PathBuf;
use upgrade_scan::data::{tables, Release};
use upgrade_scan::facts::*;
use upgrade_scan::ps::Version;
use upgrade_scan::{hardware, parse, sbat, software, storage, system, verdict, Scan};

fn cases() -> Vec<Value> {
    serde_json::from_str(include_str!("cases.json")).unwrap()
}

fn arg<T: DeserializeOwned>(args: &Value, name: &str) -> T {
    let v = args.get(name).cloned().unwrap_or(Value::Null);
    serde_json::from_value(v).unwrap_or_else(|e| panic!("argument {name}: {e}"))
}

fn opt_num(v: Option<f64>) -> Value {
    v.map_or(Value::Null, |x| json!(x))
}

/// Run one call of a case. Returns what the function returned (null for the
/// ones that only add checks) and, for a verdict call, the three answers.
fn run(scan: &mut Scan, fn_name: &str, a: &Value, extra: &mut Map<String, Value>) -> Value {
    match fn_name {
        "verdict" => {
            for c in a["Checks"].as_array().unwrap() {
                let text = |k: &str| c.get(k).and_then(Value::as_str).unwrap_or("").to_string();
                let status = upgrade_scan::Status::parse(&text("Status")).expect("a known status");
                scan.add(&text("Section"), &text("Title"), status, text("Detail")).min_kernel(text("MinKernel"));
            }
            let kernel = verdict::required_kernel(scan);
            let v = verdict::verdict(scan);
            let rec = verdict::recommendation(scan, kernel);
            let groups: Vec<Value> = v.groups.iter().map(|g| json!({"Priority": g.priority, "Label": g.label, "Items": g.items})).collect();
            extra.insert("verdict".into(), json!({"Level": v.level.as_str(), "Summary": v.summary, "Groups": groups}));
            extra.insert("kernel".into(), kernel.map_or(Value::Null, |k| json!(k.to_string())));
            let names = |l: &[&upgrade_scan::data::Distro]| l.iter().map(|d| d.name.clone()).collect::<Vec<_>>();
            extra.insert("recommendation".into(), json!({"Distros": names(&rec.distros), "HasNvidia": rec.has_nvidia, "LowRam": rec.low_ram, "Excluded": names(&rec.excluded)}));
            Value::Null
        }
        "corpus" => {
            let cap = capture(a["File"].as_str().unwrap());
            replay(scan, &cap);
            Value::Null
        }
        "distro-table" => Value::Array(
            tables().distros.iter().map(|d| json!({"Name": d.name, "Kernel": d.kernel, "Parsed": Version::parse(&d.kernel).map(|v| v.to_string())})).collect(),
        ),
        "Test-UpgArchitecture" => { system::architecture(scan, &arg(a, "Sys")); Value::Null }
        "Test-UpgMemory" => { system::memory(scan, &arg(a, "Sys")); Value::Null }
        "Test-UpgFirmware" => { system::firmware(scan, &arg(a, "Sys"), arg(a, "SecureBoot")); Value::Null }
        "Test-UpgResume" => { system::resume(scan, arg::<Option<ResumeFacts>>(a, "Facts").as_ref()); Value::Null }
        "Test-UpgWin11Context" => { system::current_os(scan, &arg(a, "Sys")); Value::Null }
        "Test-UpgSbat" => {
            sbat::judge_sbat(scan, arg(a, "SecureBoot"), &arg::<Vec<SbatSource>>(a, "Levels"), &arg::<Vec<KitBootFile>>(a, "Files"));
            Value::Null
        }
        "Test-UpgReleases" => {
            let level = sbat::parse(a["LevelText"].as_str().unwrap());
            let table: Vec<Release> = if a["Table"] == "data" { tables().releases.clone() } else { arg(a, "Table") };
            let db: Option<Vec<String>> = arg(a, "DbAuthorities");
            sbat::judge_releases(scan, arg(a, "SecureBoot"), &level, db.as_deref(), &table);
            Value::Null
        }
        "Test-UpgStorageMode" => { storage::storage_mode(scan, &arg::<Vec<Pnp>>(a, "Pnp")); Value::Null }
        "Test-UpgDisk" => {
            storage::disk(scan, &arg(a, "Facts"), arg(a, "IsAdmin"), arg::<Option<bool>>(a, "RepairQueued").unwrap_or(false));
            Value::Null
        }
        "Test-UpgPhysicalDisk" => { storage::physical_disk(scan, arg::<Option<PhysicalDiskFacts>>(a, "Facts").as_ref()); Value::Null }
        "Test-UpgVolumeHealth" => { storage::volume_health(scan, arg(a, "IsAdmin"), arg::<Option<VolumeHealth>>(a, "Health").as_ref()); Value::Null }
        "Test-UpgFastStartup" => { storage::fast_startup(scan, arg(a, "HiberbootEnabled")); Value::Null }
        "Test-UpgBitLocker" => { storage::bitlocker(scan, arg(a, "IsAdmin"), arg::<Option<BitLockerState>>(a, "State").as_ref()); Value::Null }
        "Test-UpgEsp" => { storage::esp(scan, arg(a, "IsAdmin"), arg::<Option<EspFacts>>(a, "Facts").as_ref()); Value::Null }
        "Test-UpgWifi" => { hardware::wifi(scan, &arg::<Vec<Pnp>>(a, "Pnp")); Value::Null }
        "Test-UpgGpu" => { hardware::gpu(scan, &arg::<Vec<Pnp>>(a, "Pnp")); Value::Null }
        "Test-UpgAudio" => { hardware::audio(scan, &arg::<Vec<Pnp>>(a, "Pnp")); Value::Null }
        "Test-UpgVendor" => { hardware::vendor(scan, &arg(a, "Sys")); Value::Null }
        "Test-UpgApps" => { software::apps(scan, &arg::<Vec<String>>(a, "Apps")); Value::Null }
        "ConvertFrom-UpgDiskpartQueryMax" => opt_num(parse::diskpart_query_max(&arg::<Vec<String>>(a, "Lines"))),
        "ConvertFrom-UpgFsutilDirty" => json!(parse::fsutil_dirty(&arg::<Vec<String>>(a, "Lines"))),
        "ConvertFrom-UpgChkdskEvent" => {
            let r = parse::chkdsk_event(a["Message"].as_str().unwrap());
            json!({"Verdict": r.verdict, "Records": r.records, "Queued": r.queued})
        }
        "ConvertFrom-UpgDefrag259" => json!(parse::defrag_259(a["Message"].as_str().unwrap())),
        "Test-UpgShrinkMitigable" => json!(parse::shrink_mitigable(a["LastUnmovable"].as_str().unwrap())),
        "Test-UpgRepairQueued" => json!(parse::repair_queued(arg::<Option<VolumeHealth>>(a, "Health").as_ref())),
        "ConvertFrom-UpgDiskEvents" => {
            let r = parse::disk_events(&arg::<Vec<DiskEvent>>(a, "Events"), arg(a, "DiskNumber"));
            json!({"BadBlock": r.bad_block, "Paging": r.paging, "Reset": r.reset, "First": r.first.map(|t| t.iso()), "Last": r.last.map(|t| t.iso()), "Days": 30})
        }
        "ConvertFrom-UpgSmartAttributes" => {
            let h = parse::smart_attributes(&arg::<Vec<u8>>(a, "Bytes"));
            Value::Object(h.into_iter().map(|(k, v)| (k.to_string(), json!(v))).collect())
        }
        other => panic!("cases.json names a function this test does not know: {other}"),
    }
}

fn capture(file: &str) -> Capture {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../windows/corpus").join(file);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(text.trim_start_matches('\u{feff}')).unwrap_or_else(|e| panic!("{file}: {e}"))
}

/// The checks a recording can replay: the ones that read only the
/// enumeration (the same seven the PowerShell self-test replays).
fn replay(scan: &mut Scan, cap: &Capture) {
    system::architecture(scan, &cap.sys);
    system::memory(scan, &cap.sys);
    storage::storage_mode(scan, &cap.pnp);
    hardware::wifi(scan, &cap.pnp);
    hardware::gpu(scan, &cap.pnp);
    hardware::audio(scan, &cap.pnp);
    hardware::vendor(scan, &cap.sys);
}

fn run_case(case: &Value) -> (Scan, Value) {
    let mut scan = Scan::new();
    let mut extra = Map::new();
    let mut returns = Vec::new();
    for call in case["calls"].as_array().unwrap() {
        returns.push(run(&mut scan, call["fn"].as_str().unwrap(), &call["args"], &mut extra));
    }
    let checks: Vec<Value> = scan
        .checks
        .iter()
        .map(|c| json!({"Section": c.section, "Title": c.title, "Status": c.status.as_str(), "Detail": c.detail, "Note": c.note, "MinKernel": c.min_kernel, "Remedy": c.remedy}))
        .collect();
    let mut out = extra;
    out.insert("checks".into(), Value::Array(checks));
    out.insert("unmatched".into(), json!(scan.unmatched));
    out.insert("releases".into(), serde_json::to_value(&scan.releases).unwrap());
    out.insert("returns".into(), Value::Array(returns));
    (scan, Value::Object(out))
}

/// The first place two JSON values differ, or nothing. Numbers compare as
/// numbers (17 equals 17.0); everything else exactly.
fn first_difference(path: &str, want: &Value, got: &Value) -> Option<String> {
    match (want, got) {
        (Value::Number(a), Value::Number(b)) if a.as_f64() == b.as_f64() => None,
        (Value::Array(a), Value::Array(b)) => {
            if a.len() != b.len() {
                return Some(format!("{path}: PowerShell has {} items, Rust has {}\n  PowerShell: {want}\n  Rust:       {got}", a.len(), b.len()));
            }
            a.iter().zip(b).enumerate().find_map(|(i, (x, y))| first_difference(&format!("{path}/{i}"), x, y))
        }
        (Value::Object(a), Value::Object(b)) => {
            let mut keys: Vec<&String> = a.keys().chain(b.keys()).collect();
            keys.sort();
            keys.dedup();
            keys.into_iter().find_map(|k| match (a.get(k), b.get(k)) {
                (Some(x), Some(y)) => first_difference(&format!("{path}/{k}"), x, y),
                (x, y) => Some(format!("{path}/{k}: PowerShell {x:?}, Rust {y:?}")),
            })
        }
        _ if want == got => None,
        _ => Some(format!("{path}:\n  PowerShell: {want}\n  Rust:       {got}")),
    }
}

#[test]
fn every_case_matches_powershell() {
    let golden: Value = serde_json::from_str(include_str!("golden.json")).unwrap();
    let cases = cases();
    assert_eq!(golden.as_object().unwrap().len(), cases.len(), "golden.json is stale: run ./port-check.sh --record");
    let mut wrong = Vec::new();
    for case in &cases {
        let name = case["name"].as_str().unwrap();
        let want = golden.get(name).unwrap_or_else(|| panic!("golden.json has no '{name}': run ./port-check.sh --record"));
        let (_, got) = run_case(case);
        if let Some(d) = first_difference("", want, &got) {
            wrong.push(format!("FAIL  {name}\n{d}"));
        }
    }
    assert!(wrong.is_empty(), "{} of {} cases differ from the PowerShell scanner:\n\n{}", wrong.len(), cases.len(), wrong.join("\n\n"));
}

fn statuses(scan: &Scan, title: &str) -> Vec<String> {
    let mut got: Vec<String> = scan.checks.iter().filter(|c| c.title == title).map(|c| c.status.as_str().to_string()).collect();
    got.sort();
    got
}

fn text_of(scan: &Scan, title: &str) -> String {
    scan.checks.iter().filter(|c| c.title == title).map(|c| format!("{} {} {}", c.detail, c.note, c.remedy)).collect::<Vec<_>>().join(" ")
}

/// The PowerShell self-test's own assertions, held here directly: the
/// expected statuses, the phrases that must and must not appear, the values
/// the parsers return, the verdict and the kernel.
#[test]
fn the_self_test_expectations_hold() {
    let mut wrong = Vec::new();
    let mut counted = 0;
    for case in cases().iter().filter(|c| c["origin"] != "port") {
        let name = case["name"].as_str().unwrap();
        let (scan, out) = run_case(case);
        counted += 1;
        let mut errors = Vec::new();
        let mut expect: Map<String, Value> = case.get("expect").and_then(Value::as_object).cloned().unwrap_or_default();
        if case["origin"] == "corpus" {
            let cap = capture(case["calls"][0]["args"]["File"].as_str().unwrap());
            if cap.expected.is_empty() {
                errors.push("capture has an empty Expected block - curate it before it lands in corpus/".to_string());
            }
            expect = cap.expected;
        }
        for (title, want) in &expect {
            let mut want: Vec<String> = match want {
                Value::Array(l) => l.iter().map(|x| x.as_str().unwrap().to_string()).collect(),
                x => vec![x.as_str().unwrap().to_string()],
            };
            want.sort();
            let got = statuses(&scan, title);
            if want != got {
                errors.push(format!("'{title}' was [{}], expected [{}]", got.join(","), want.join(",")));
            }
        }
        for (title, phrase) in case.get("match").and_then(Value::as_object).into_iter().flatten() {
            if !text_of(&scan, title).contains(phrase.as_str().unwrap()) {
                errors.push(format!("'{title}' text lacks '{}'", phrase.as_str().unwrap()));
            }
        }
        for (title, phrase) in case.get("notmatch").and_then(Value::as_object).into_iter().flatten() {
            if text_of(&scan, title).contains(phrase.as_str().unwrap()) {
                errors.push(format!("'{title}' text must not contain '{}'", phrase.as_str().unwrap()));
            }
        }
        for (i, call) in case["calls"].as_array().unwrap().iter().enumerate() {
            if let Some(want) = call.get("returns") {
                if let Some(d) = first_difference(&format!("call {i} ({})", call["fn"].as_str().unwrap()), want, &out["returns"][i]) {
                    errors.push(d);
                }
            }
        }
        if let Some(level) = case.get("expect_verdict").and_then(Value::as_str) {
            if out["verdict"]["Level"] != level {
                errors.push(format!("verdict was {}, expected {level}", out["verdict"]["Level"]));
            }
            if let Some(k) = case.get("expect_kernel").filter(|k| !k.is_null()) {
                if &out["kernel"] != k {
                    errors.push(format!("kernel was {}, expected {k}", out["kernel"]));
                }
            }
            // a RED verdict must list its reasons
            if level == "RED" && out["verdict"]["Groups"].as_array().unwrap().is_empty() {
                errors.push("RED verdict listed no reasons".to_string());
            }
        }
        if name == "distro table: every kernel parses" {
            for d in out["returns"][0].as_array().unwrap() {
                if d["Parsed"].is_null() {
                    errors.push(format!("distro table: {} has unparseable kernel {}", d["Name"], d["Kernel"]));
                }
            }
        }
        if !errors.is_empty() {
            wrong.push(format!("FAIL  {name}\n      {}", errors.join("\n      ")));
        }
    }
    // 99 self-test cases, the distro table check, 3 recordings (scanner 0.5.0)
    assert!(counted >= 103, "only {counted} self-test and corpus cases in cases.json");
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn every_recording_in_the_corpus_is_a_case() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../windows/corpus");
    let listed: Vec<String> = cases().iter().filter(|c| c["origin"] == "corpus").map(|c| c["calls"][0]["args"]["File"].as_str().unwrap().to_string()).collect();
    for entry in std::fs::read_dir(dir).unwrap() {
        let name = entry.unwrap().file_name().to_string_lossy().into_owned();
        if name.ends_with(".json") {
            assert!(listed.contains(&name), "corpus/{name} is not replayed: add it to tests/cases.json and run ./port-check.sh --record");
        }
    }
}
