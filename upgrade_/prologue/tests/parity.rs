//! The Rust prologue's judging half against Invoke-Prologue.ps1 (RISKS R32;
//! VALIDATION V13). `cases.json` lists the calls: every case of the
//! PowerShell self-test under its own name, call by call with the same
//! inputs, plus more. `golden.json` is what the PowerShell returned for
//! each (written by `golden.ps1`). Every return must be equal in full.

use serde_json::{json, Value};
use upgrade_prologue::val::{int, items, s, truthy};
use upgrade_prologue::{judge, state, FOLLOWS_PROLOGUE};
use upgrade_scan::facts::DiskEvent;
use upgrade_scan::ps::Stamp;

const NOW_O: &str = "2026-10-07T12:00:00.0000000Z";
const NOW_Z: &str = "2026-10-07T12:00:00Z";

fn cases() -> Vec<Value> {
    serde_json::from_str(include_str!("cases.json")).unwrap()
}

fn lines(v: &Value) -> Vec<String> {
    items(v).into_iter().map(s).collect()
}

fn text(a: &Value, k: &str) -> String {
    a.get(k).map_or(String::new(), s)
}

fn flag(a: &Value, k: &str) -> bool {
    a.get(k).is_some_and(truthy)
}

fn opt_int(a: &Value, k: &str) -> Option<i64> {
    a.get(k).filter(|v| !v.is_null()).map(int)
}

/// The script's state with the case's edits laid over it, nested keys merged.
fn edited_state(edits: &Value) -> Value {
    let mut st = state::new_state("j", "s", "E:\\", FOLLOWS_PROLOGUE, NOW_O);
    for (k, v) in edits.as_object().into_iter().flatten() {
        match (st.get_mut(k), v) {
            (Some(Value::Object(have)), Value::Object(want)) => {
                for (kk, vv) in want {
                    have.insert(kk.clone(), vv.clone());
                }
            }
            _ => st[k.as_str()] = v.clone(),
        }
    }
    st
}

fn run(fn_name: &str, a: &Value) -> Value {
    let null = Value::Null;
    let arg = |k: &str| a.get(k).unwrap_or(&null);
    match fn_name {
        "Test-PrologueUpdatePending" => json!(judge::update_pending(arg("U"))),
        "Get-PrologueUpdateStep" => json!(judge::update_step(flag(a, "Pending"), int(arg("Restarts")), &text(a, "Where"), None)),
        "Get-PrologueRestorePointVerdict" => json!(judge::restore_point_verdict(opt_int(a, "Before"), opt_int(a, "After"))),
        "Format-PrologueRestorePoints" => json!(judge::format_restore_points(arg("R"))),
        "Test-PrologueUsnJournalFile" => json!(judge::usn_journal_file(&text(a, "LastUnmovable"))),
        "Get-PrologueUsnJournalStep" => json!(judge::usn_journal_step(flag(a, "Fits"), &text(a, "LastUnmovable"), flag(a, "Consented"), flag(a, "DoneThisBoot"))),
        "ConvertFrom-PrologueUsnQuery" => judge::usn_query(&lines(arg("Lines"))),
        "Get-PrologueRestorePlan" => json!(judge::restore_plan(arg("Shrink"), flag(a, "KeepHibernationOff"))),
        "Test-PrologueShadowStorageFile" => json!(judge::shadow_storage_file(&text(a, "LastUnmovable"))),
        "Get-PrologueRestorePointStep" => json!(judge::restore_point_step(flag(a, "Fits"), &text(a, "LastUnmovable"), flag(a, "Consented"), flag(a, "AlreadyDone"))),
        "Get-PrologueStopSentence" => json!(judge::stop_sentence(&lines(arg("Restored")))),
        "Compare-PrologueJob" => json!(judge::compare_job(arg("Job"), arg("F"))),
        "Get-PrologueVolumeTrigger" => json!(judge::volume_trigger(&text(a, "Dirty"), flag(a, "RepairQueued"))),
        "Test-PrologueRepairQueued" => judge::repair_queued(&text(a, "VolumeStatus"), &text(a, "NtfsFullChkdsk"), &text(a, "LastCheck")),
        "Test-PrologueNtfs98Fresh" => {
            // a {date} argument is a DateTime in the recorder: its date and time
            let when = |k: &str| match arg(k) {
                Value::Object(o) => s(&o["date"]),
                other => s(other),
            };
            json!(judge::ntfs98_fresh(&when("Ntfs98"), &when("LastCheck")))
        }
        "trigger-of-queued" => {
            let q = judge::repair_queued(&text(a, "VolumeStatus"), &text(a, "NtfsFullChkdsk"), &text(a, "LastCheck"));
            json!(judge::volume_trigger(&text(a, "Dirty"), truthy(&q["Queued"])))
        }
        "ConvertFrom-PrologueDefrag259" => json!(upgrade_scan::parse::defrag_259(&text(a, "Message"))),
        "Get-PrologueRepairMethod" => json!(judge::repair_method(&text(a, "Scan"), &text(a, "LogVerdict"), flag(a, "RepairNeeded"), flag(a, "NtfsFullChkdsk"))),
        "ConvertFrom-PrologueChkdskEvent" => {
            let r = upgrade_scan::parse::chkdsk_event(&text(a, "Message"));
            json!({"Verdict": r.verdict, "Records": r.records, "Queued": r.queued})
        }
        "ConvertFrom-PrologueDiskEvents" => {
            let events: Vec<DiskEvent> = items(arg("Events")).into_iter().map(|e| DiskEvent { id: int(&e["Id"]), time_created: Stamp::parse(e["TimeCreated"]["date"].as_str().unwrap()).unwrap(), message: Some(s(&e["Message"])) }).collect();
            let r = upgrade_scan::parse::disk_events(&events, int(arg("DiskNumber")));
            json!({"BadBlock": r.bad_block, "Paging": r.paging, "Reset": r.reset, "First": r.first.map(|t| t.iso()), "Last": r.last.map(|t| t.iso())})
        }
        "Test-PrologueDiskHealthGate" => judge::disk_health_gate(&text(a, "Health"), int(arg("BadBlocks")), flag(a, "Acknowledged")),
        "Format-PrologueScan" => json!(judge::format_scan(&text(a, "Cmdlet"), arg("Ev"))),
        "ConvertFrom-PrologueChkntfs" => json!(judge::chkntfs(&lines(arg("Lines")))),
        "ConvertFrom-PrologueFsutilDirty" => json!(upgrade_scan::parse::fsutil_dirty(&lines(arg("Lines")))),
        "ConvertFrom-PrologueDiskpartQueryMax" => json!(upgrade_scan::parse::diskpart_query_max(&lines(arg("Lines")))),
        "ConvertFrom-PrologueManageBde" => json!(judge::manage_bde(&lines(arg("Lines")))),
        "Get-PrologueShrinkPlan" => judge::shrink_plan(int(arg("PartSize")), int(arg("SizeMin")), int(arg("FreeBytes")), arg("LinuxMinGB").as_f64().unwrap_or(0.0), int(arg("FilesBytes"))),
        "Get-PrologueFork" => json!(judge::fork(&text(a, "JobPath"), flag(a, "Fits"), &text(a, "IfCannotKeep"), &text(a, "PathReason"))),
        "Get-PrologueEraseStartRefusal" => json!(judge::erase_start_refusal(arg("Job"), &text(a, "ConfirmWord"), &text(a, "EraseConsent"))),
        "Compare-PrologueEraseDisks" => json!(judge::compare_erase_disks(arg("Job"), arg("F"))),
        "Get-PrologueEraseReturn" => judge::erase_return(arg("Countdown"), arg("Verify")),
        "Get-PrologueStageRefusal" => json!(judge::stage_refusal(int(arg("Folders")), opt_int(a, "StagedFiles"))),
        "Get-PrologueTimeEstimate" => json!(judge::time_estimate(int(arg("Bytes")), arg("Mbps").as_f64())),
        "Format-PrologueDuration" => json!(judge::duration(opt_int(a, "Seconds"))),
        "Get-HandoffResult" => json!(judge::handoff_result(flag(a, "Fired"), flag(a, "SequenceCleared"), flag(a, "OrderUnchanged"))),
        "grubenv-clean" => {
            let b = judge::grub_env_block();
            json!({"Length": b.len(), "Fired": judge::grub_env_fired(&b)})
        }
        "Test-GrubEnvFired" => json!(judge::grub_env_fired(text(a, "Text").as_bytes())),
        "Find-StickRoot" => json!(judge::find_stick_root(arg("Volumes"), &text(a, "UniqueId"))),
        "block" => {
            let st = edited_state(arg("State"));
            // the round trip through the state file is the identity here
            state::block(&st)
        }
        "stopped" => state::stopped_outcome(arg("Job"), &edited_state(arg("State")), &text(a, "StoppedAt"), &text(a, "Reason"), arg("WindowsPartition"), &format!("prologue {FOLLOWS_PROLOGUE}"), NOW_Z),
        "state" => {
            let st = state::new_state("j", "s", "E:\\", FOLLOWS_PROLOGUE, NOW_O);
            let mut old = st.clone();
            old.as_object_mut().unwrap().shift_remove("Resumes");
            json!({"Resumes": items(&st["Resumes"]).len(), "OldContains": old.get("Resumes").is_some(), "Json": serde_json::to_string_pretty(&st).unwrap()})
        }
        "Get-ResumeContext" => judge::resume_context(&text(a, "UserName"), flag(a, "UserInteractive"), int(arg("SessionId")), flag(a, "ExplorerRunning")),
        "New-NoticeCommand" => json!(judge::notice_command(&text(a, "ScriptPath"), &text(a, "State"))),
        "Get-ProbeResult" => json!(judge::probe_result(flag(a, "Unattended"), flag(a, "StickFound"), flag(a, "TaskRemoved"))),
        "ConvertTo-ProbeCsvLine" => json!(judge::probe_csv_line(&items(arg("Fields")).into_iter().cloned().collect::<Vec<_>>())),
        "filesystem-only" => json!("filesystem-only"),
        "Get-DriveRoot" => match judge::drive_root(&text(a, "Letter")) {
            Ok(r) => json!(r),
            Err(e) => json!({"threw": e}),
        },
        other => panic!("cases.json names a function this test does not know: {other}"),
    }
}

/// The first place two values differ. Numbers compare as numbers; `Json`
/// (the state's own text, whose form each program chooses) by its parsed
/// value.
fn first_difference(path: &str, want: &Value, got: &Value) -> Option<String> {
    match (want, got) {
        (Value::Number(a), Value::Number(b)) if a.as_f64() == b.as_f64() => None,
        (Value::String(a), Value::String(b)) if path.ends_with("/Json") => {
            let (pa, pb): (Result<Value, _>, Result<Value, _>) = (serde_json::from_str(a), serde_json::from_str(b));
            match (pa, pb) {
                (Ok(x), Ok(y)) => first_difference(path, &x, &y),
                _ => Some(format!("{path}: not both JSON")),
            }
        }
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
    let (mut wrong, mut calls) = (Vec::new(), 0);
    for case in &cases {
        let name = case["name"].as_str().unwrap();
        let want_all = golden.get(name).unwrap_or_else(|| panic!("golden.json has no '{name}': run ./port-check.sh --record"));
        for (i, call) in case["calls"].as_array().unwrap().iter().enumerate() {
            calls += 1;
            let fn_name = call["fn"].as_str().unwrap();
            let got = run(fn_name, &call["args"]);
            let want = &want_all[i];
            if let Some(d) = first_difference("", want, &got) {
                wrong.push(format!("FAIL  {name} (call {i}, {fn_name})\n{d}"));
            } else if matches!(fn_name, "block" | "stopped") {
                // a record's fields come in the PowerShell's order too
                let (mut a, mut b) = (Vec::new(), Vec::new());
                key_order(want, "", &mut a);
                key_order(&got, "", &mut b);
                if let Some((x, y)) = a.iter().zip(&b).find(|(x, y)| x != y) {
                    wrong.push(format!("FAIL  {name} (call {i}): the record's fields are in another order\n  PowerShell: {x}\n  Rust:       {y}"));
                }
            }
        }
    }
    assert!(calls > 200, "{calls} calls");
    wrong.truncate(12);
    assert!(wrong.is_empty(), "differences from Invoke-Prologue.ps1 (first {}):\n\n{}", wrong.len(), wrong.join("\n\n"));
}

#[test]
fn the_self_test_is_all_here() {
    let n = cases().iter().filter(|c| c["origin"] == "selftest").count();
    assert!(n >= 144, "only {n} self-test cases in cases.json");
}

/// Every stopped outcome this code writes from real-shaped inputs passes
/// the contract. Two self-test cases feed inputs that are made up on
/// purpose (a job id "j" with an empty erase list; a keep-windows stop with
/// no partition) and are left out here: the PowerShell writes those too,
/// and the parity test above holds the Rust to the same text.
#[test]
fn stopped_outcomes_pass_the_contract() {
    const MADE_UP: [&str; 2] = ["erase: a stopped outcome carries the erase consent", "stopped outcome: a stop after the keep-windows fork keeps scrub_after settle-in-pull"];
    let (mut checked, mut wrong) = (0, Vec::new());
    for case in cases() {
        if MADE_UP.contains(&case["name"].as_str().unwrap()) {
            continue;
        }
        for call in case["calls"].as_array().unwrap().iter().filter(|c| c["fn"] == "stopped") {
            let o = run("stopped", &call["args"]);
            checked += 1;
            if let Err(e) = upgrade_schema::Outcome::from_value(o) {
                wrong.push(format!("{}: {e}", case["name"]));
            }
        }
    }
    assert!(checked >= 4, "{checked} outcomes checked");
    assert!(wrong.is_empty(), "{} stopped outcomes do not pass the contract:\n{}", wrong.len(), wrong.join("\n"));
}
