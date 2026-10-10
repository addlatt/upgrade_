//! The Rust job writer's judging half against New-Job.ps1 (RISKS R32;
//! VALIDATION V13). `cases.json` lists the calls: all 126 cases of the
//! PowerShell self-test under their own names, with the same inputs, plus
//! more. `golden.json` is what the PowerShell returned for each (written by
//! `golden.ps1`). Every return must be equal in full, so everything the
//! self-test asserts about these returns holds here too.

use serde_json::{json, Value};
use upgrade_job::decide::{self, Moment};
use upgrade_job::document::{job_document, Choices, Stamp};
use upgrade_job::val::{int, items, s};
use upgrade_job::{maps, records, wifi, FOLLOWS_JOB_WRITER};
use upgrade_scan::parse;

const NOW: &str = "2026-10-04T12:00:00Z";
const JOB_ID: &str = "00000000-0000-4000-8000-000000000000";

fn cases() -> Vec<Value> {
    serde_json::from_str(include_str!("cases.json")).unwrap()
}

fn text(a: &Value, key: &str) -> String {
    s(a.get(key).unwrap_or(&Value::Null))
}

fn moment(v: Option<&Value>) -> Option<Moment> {
    match v? {
        Value::Null => None,
        Value::Object(o) => {
            let st = upgrade_scan::ps::Stamp::parse(o["date"].as_str().unwrap()).unwrap();
            Some(Moment { shown: st.to_string(), order: Some(st.seconds()) })
        }
        other => Some(Moment { shown: s(other), order: None }),
    }
}

fn facts(overrides: &Value) -> Value {
    let mut f: Value = serde_json::from_str(include_str!("base-facts.json")).unwrap();
    for (k, v) in overrides.as_object().into_iter().flatten() {
        f[k.as_str()] = v.clone();
    }
    f
}

fn run(fn_name: &str, a: &Value) -> Value {
    let null = Value::Null;
    let arg = |key: &str| a.get(key).unwrap_or(&null);
    match fn_name {
        "ConvertTo-JobIanaTimeZone" => json!(maps::iana_time_zone(&text(a, "WindowsId"))),
        "ConvertTo-JobKeymap" => json!(maps::keymap(&text(a, "InputMethodTip"))),
        "ConvertTo-JobLinuxName" => json!(maps::linux_name(&text(a, "WindowsName"))),
        "ConvertFrom-JobFsutilDirty" => json!(parse::fsutil_dirty(&items(arg("Lines")).into_iter().map(s).collect::<Vec<_>>())),
        "ConvertFrom-JobDefrag259" => json!(parse::defrag_259(&text(a, "Message"))),
        "Test-JobShrinkMitigable" => json!(parse::shrink_mitigable(&text(a, "LastUnmovable"))),
        "ConvertTo-JobClock" => {
            match records::clock(&text(a, "WindowsZone"), &text(a, "Iana"), arg("RealTimeIsUniversal"), arg("DynamicDstDisabled"), int(arg("OffsetMinutes")), int(arg("BaseOffsetMinutes")), arg("DstActive").as_bool().unwrap_or(false), &text(a, "NowUtc")) {
                Ok(clock) => json!({"Refusal": null, "Clock": clock}),
                Err(why) => json!({"Refusal": why, "Clock": null}),
            }
        }
        "ConvertTo-JobLicense" => records::license(arg("Os"), arg("Products"), arg("Firmware"), &text(a, "ReadError"), &text(a, "NowUtc")),
        "ConvertTo-JobSsh" => records::ssh(arg("StartType"), arg("KeyFiles"), &text(a, "ReadError")),
        "ConvertTo-JobSoftware" => records::software(arg("Desktop"), arg("Store"), a.get("Cap").map_or(2000, |c| int(c) as usize)),
        "ConvertFrom-JobWlanProfile" => json!(wifi::profile_row(&text(a, "Xml"))),
        "ConvertTo-JobWifi" => {
            let dir = a.get("Dir").map_or(wifi::WIFI_DIR.to_string(), s);
            match wifi::harvest_wifi(arg("Api"), int(arg("StoredCount")), &dir) {
                Ok((block, files)) => json!({"Refusal": null, "Wifi": block, "Files": files.iter().map(|f| json!({"Rel": f.rel, "Xml": f.xml})).collect::<Vec<_>>()}),
                Err(why) => json!({"Refusal": why}),
            }
        }
        "Get-JobPath" => {
            let p = decide::path(&text(a, "DiskHealth"), arg("EspFits").as_bool().unwrap(), arg("ShrinkableGB").as_f64(), &text(a, "Dirty"), arg("DiskHealthAcknowledged").as_bool().unwrap(), arg("RepairQueued").as_bool().unwrap(), arg("Mitigable").as_bool().unwrap(), &text(a, "IfCannotKeep"));
            json!({"Path": p.map(|x| x.0), "Reason": p.map(|x| x.1)})
        }
        "Test-JobRepairQueued" => {
            let r = decide::repair_queued(&text(a, "VolumeStatus"), moment(a.get("NtfsFullChkdsk")).as_ref(), moment(a.get("LastCheck")).as_ref());
            json!({"Queued": r.queued, "Why": r.why, "Stale": r.stale})
        }
        "Get-JobAcknowledgement" => {
            let names = |key: &str| items(arg(key)).into_iter().map(s).collect::<Vec<_>>();
            match decide::acknowledgement(&text(a, "Verdict"), &names("FailedChecks"), &names("WarnChecks"), &text(a, "Typed"), NOW) {
                Ok(block) => json!({"Refusal": null, "Block": block}),
                Err(why) => json!({"Refusal": why, "Block": null}),
            }
        }
        "Get-JobEraseDisks" => match decide::erase_disks(arg("Disks"), int(arg("SystemNumber")), &text(a, "StickUniqueId")) {
            Ok(disks) => json!({"Refusals": [], "Disks": disks}),
            Err(why) => json!({"Refusals": why, "Disks": []}),
        },
        "Get-JobReleaseRefusal" => json!(decide::release_refusal(arg("Kit"), arg("Releases"))),
        "New-JobDocument" => {
            let (desktop, start_at, hash, fork, rel, ack, erase) = (text(a, "Desktop"), a.get("StartAt").map_or("desktop".to_string(), s), text(a, "PasswordHash"), text(a, "IfCannotKeep"), text(a, "ReportRel"), text(a, "AcknowledgeDataLoss"), text(a, "EraseEverything"));
            let choice = Choices { desktop: &desktop, start_at: &start_at, password_hash: &hash, if_cannot_keep: &fork, report_rel: &rel, acknowledge_data_loss: &ack, erase_everything: &erase };
            match job_document(&facts(arg("F")), &choice, &Stamp { job_id: JOB_ID, now_utc: NOW, writer_version: FOLLOWS_JOB_WRITER }) {
                Ok(job) => json!({"Refusals": [], "Job": job}),
                Err(why) => json!({"Refusals": why, "Job": null}),
            }
        }
        other => panic!("cases.json names a function this test does not know: {other}"),
    }
}

/// The first place two values differ. Numbers compare as numbers. The two
/// software lists compare by name, because their order differs on purpose
/// (docs/RUST-PORT.md, "Differences kept on purpose").
fn first_difference(path: &str, want: &Value, got: &Value) -> Option<String> {
    match (want, got) {
        (Value::Number(a), Value::Number(b)) if a.as_f64() == b.as_f64() => None,
        (Value::Array(a), Value::Array(b)) => {
            if a.len() != b.len() {
                return Some(format!("{path}: PowerShell has {} items, Rust has {}\n  PowerShell: {want}\n  Rust:       {got}", a.len(), b.len()));
            }
            let by_name = |l: &Vec<Value>| {
                let mut l = l.clone();
                l.sort_by_key(|x| s(&x["name"]));
                l
            };
            let (a, b) = if path.ends_with("/desktop") || path.ends_with("/store") { (by_name(a), by_name(b)) } else { (a.clone(), b.clone()) };
            a.iter().zip(&b).enumerate().find_map(|(i, (x, y))| first_difference(&format!("{path}/{i}"), x, y))
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

/// The names of an object's fields, in order, all the way down.
fn key_order(v: &Value, path: &str, out: &mut Vec<String>) {
    match v {
        Value::Object(m) => {
            out.push(format!("{path}: {}", m.keys().cloned().collect::<Vec<_>>().join(",")));
            for (k, x) in m {
                key_order(x, &format!("{path}/{k}"), out);
            }
        }
        Value::Array(l) => l.iter().enumerate().for_each(|(i, x)| key_order(x, &format!("{path}/{i}"), out)),
        _ => {}
    }
}

#[test]
fn every_call_matches_powershell() {
    let golden: Value = serde_json::from_str(include_str!("golden.json")).unwrap();
    let cases = cases();
    assert_eq!(golden.as_object().unwrap().len(), cases.len(), "golden.json is stale: run ./port-check.sh --record");
    let (mut wrong, mut calls, mut jobs) = (Vec::new(), 0, 0);
    for case in &cases {
        let name = case["name"].as_str().unwrap();
        for (i, call) in case["calls"].as_array().unwrap().iter().enumerate() {
            calls += 1;
            let fn_name = call["fn"].as_str().unwrap();
            let got = run(fn_name, &call["args"]);
            let want = &golden[name][i];
            if let Some(d) = first_difference("", want, &got) {
                wrong.push(format!("FAIL  {name} (call {i}, {fn_name})\n{d}"));
            } else if fn_name == "New-JobDocument" && !got["Job"].is_null() {
                // a job's fields come in the PowerShell's order too
                jobs += 1;
                let (mut a, mut b) = (Vec::new(), Vec::new());
                key_order(&want["Job"], "", &mut a);
                key_order(&got["Job"], "", &mut b);
                // the software block is carried through as the facts gave it
                a.retain(|l| !l.contains("/software"));
                b.retain(|l| !l.contains("/software"));
                if let Some((x, y)) = a.iter().zip(&b).find(|(x, y)| x != y) {
                    wrong.push(format!("FAIL  {name} (call {i}): the job's fields are in another order\n  PowerShell: {x}\n  Rust:       {y}"));
                }
            }
        }
    }
    assert!(calls > 900 && jobs > 80, "{calls} calls, {jobs} jobs");
    wrong.truncate(12);
    assert!(wrong.is_empty(), "differences from New-Job.ps1 (first {}):\n\n{}", wrong.len(), wrong.join("\n\n"));
}

#[test]
fn the_self_test_is_all_here() {
    let n = cases().iter().filter(|c| c["origin"] == "selftest").count();
    assert!(n >= 126, "only {n} self-test cases in cases.json");
}

/// A job this code writes passes the contract, when its facts are real
/// enough to. The self-test's made-up machine is: every job from it is
/// checked against schemas/job.schema.json.
#[test]
fn the_jobs_pass_the_contract() {
    let (mut checked, mut wrong) = (0, Vec::new());
    for case in cases().iter().filter(|c| c["origin"] == "selftest") {
        for call in case["calls"].as_array().unwrap().iter().filter(|c| c["fn"] == "New-JobDocument") {
            let got = run("New-JobDocument", &call["args"]);
            if got["Job"].is_null() {
                continue;
            }
            checked += 1;
            if let Err(e) = upgrade_schema::Job::from_value(got["Job"].clone()) {
                wrong.push(format!("{}: {e}", case["name"]));
            }
        }
    }
    wrong.sort();
    wrong.dedup();
    assert!(checked > 75, "{checked} jobs checked");
    assert!(wrong.is_empty(), "{} of {checked} jobs do not pass the contract:\n{}", wrong.len(), wrong[..wrong.len().min(10)].join("\n"));
}
