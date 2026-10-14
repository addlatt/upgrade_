//! The storage-mode harness against Test-StorageMode.ps1 0.1.0 (RISKS R32,
//! VALIDATION V13, gate V5). `storage-mode-cases.json` holds the inputs
//! (written by `make-storage-mode-cases.py`); `storage-mode-golden.json`
//! is what the script did for each (recorded by `storage-mode-golden.ps1`,
//! frozen): for a function case its return, for a flow case every act in
//! order (log lines, popups, tool calls, saves, registry and task calls),
//! what it left in the state directory and on the stick, and its exit
//! code. The Rust must do the same, act for act and file for file.
//!
//! Two differences are allowed on purpose, both written in RUST-PORT.md:
//! the record carries `harness_program` (which program wrote it) beside
//! `harness_version`; and where the script re-saved a leg's `Classes` and
//! `CompatibleIds` as `{"Length": n}` objects (PowerShell 5.1 serialises a
//! string it read back from JSON that way, a bug the script's own comment
//! records from the Aspire on 2026-09-15), the Rust keeps the strings, so
//! a golden `{"Length": n}` matches a Rust string of that length.

use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use upgrade_prologue::storage_mode as sm;
use upgrade_prologue::storage_mode::flow::{Args, Flow, LegFiles, Live};
use upgrade_prologue::val::{int, items, s, truthy};

const STATE_ROOT: &str = "C:\\ProgramData\\upgrade_";
const STATE_DIR: &str = "C:\\ProgramData\\upgrade_\\storage-mode";
const STICK: &str = "E:\\";

fn cases() -> Vec<Value> {
    serde_json::from_str(include_str!("storage-mode-cases.json")).unwrap()
}

fn golden() -> Value {
    serde_json::from_str(include_str!("storage-mode-golden.json")).unwrap()
}

/// `scanning (<whatever>)...` reads the same whichever scanner ran.
fn norm_line(line: &str) -> String {
    match (line.find("scanning ("), line.find(")...")) {
        (Some(a), Some(b)) if b > a => format!("{}scanning (<scanner>)...{}", &line[..a], &line[b + 4..]),
        _ => line.to_string(),
    }
}

struct Fake {
    case: Value,
    tmp: PathBuf,
    events: Vec<Value>,
    popups: VecDeque<i64>,
    legs: Vec<Value>,
    leg_index: usize,
    registry: BTreeMap<String, String>,
    refuse: Option<String>,
    task_present: bool,
}

impl Fake {
    fn new(case: &Value, tmp: &Path) -> Fake {
        let mut registry = BTreeMap::new();
        let mut refuse = None;
        for (k, v) in case["runonce"].as_object().into_iter().flatten() {
            if k == "__refuse__" {
                refuse = Some(s(v));
            } else {
                registry.insert(k.clone(), s(v));
            }
        }
        Fake {
            case: case.clone(),
            tmp: tmp.to_path_buf(),
            events: Vec::new(),
            popups: items(&case["popups"]).iter().map(|p| int(p)).collect(),
            legs: items(&case["legs"]).into_iter().cloned().collect(),
            leg_index: 0,
            registry,
            refuse,
            task_present: truthy(&case["task_present"]),
        }
    }

    /// A Windows path the flow uses, as a file under the temporary folder.
    fn map(&self, path: &str) -> PathBuf {
        let rel = if let Some(r) = path.strip_prefix(STATE_ROOT) {
            format!("ProgramData/upgrade_{}", r.replace('\\', "/"))
        } else if let Some(r) = path.strip_prefix(STICK) {
            format!("stick/{}", r.replace('\\', "/"))
        } else {
            panic!("the flow used a path outside the state directory and the stick: {path}")
        };
        self.tmp.join(rel)
    }

    fn ev(&mut self, v: Value) {
        self.events.push(v);
    }

    fn stick(&self) -> Option<String> {
        if truthy(&self.case["stick"]) { Some(STICK.to_string()) } else { None }
    }
}

impl Live for Fake {
    fn elevated(&self) -> bool {
        truthy(&self.case["elevated"])
    }
    fn now_o(&self) -> String {
        s(&self.case["now"])
    }
    fn safe_mode(&self) -> Option<i64> {
        let v = &self.case["safe_mode"];
        if v.is_null() { None } else { Some(int(v)) }
    }
    fn context(&self) -> Value {
        self.case["context"].clone()
    }
    fn facts(&mut self, root: &str) -> Result<Value, String> {
        self.ev(json!({"facts": root}));
        let f = &self.case["facts"];
        if let Some(e) = f.get("error") {
            return Err(s(e));
        }
        Ok(f.clone())
    }
    fn find_stick(&mut self, _st: &Value) -> Option<String> {
        let r = self.stick();
        self.ev(json!({"find-stick": r.is_some()}));
        r
    }
    fn wait_stick(&mut self, _st: &Value) -> Option<String> {
        let r = self.stick();
        self.ev(json!({"wait-stick": r.is_some()}));
        r
    }
    fn scan_leg(&mut self, dir: &str) -> LegFiles {
        let d = self.map(dir);
        std::fs::create_dir_all(&d).unwrap();
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            if e.path().is_file() {
                std::fs::remove_file(e.path()).unwrap();
            }
        }
        let leg = self.legs.get(self.leg_index).cloned().unwrap_or(Value::Null);
        self.leg_index += 1;
        self.ev(json!({"scan": dir}));
        let mut report = None;
        if !truthy(&leg["no_report"]) {
            let name = s(&leg["report_file"]);
            std::fs::write(d.join(&name), serde_json::to_string_pretty(&leg["report"]).unwrap()).unwrap();
            report = Some((name, leg["report"].clone()));
        }
        std::fs::write(d.join("scan.log"), "scan output line 1\r\nscan output line 2\r\n").unwrap();
        self.ev(json!({"capture": format!("{dir}\\machine-capture.json")}));
        let mut capture = None;
        if !truthy(&leg["no_capture"]) {
            std::fs::write(d.join("machine-capture.json"), serde_json::to_string_pretty(&leg["capture"]).unwrap()).unwrap();
            capture = Some(leg["capture"].clone());
        }
        LegFiles { report, capture }
    }
    fn native(&mut self, program: &str, args: &[String]) -> (i64, String) {
        let (mut exit, mut text) = (0, String::new());
        for n in items(&self.case["native"]) {
            if s(&n["program"]) != program || args.first().map(String::as_str) != Some(s(&n["first"]).as_str()) {
                continue;
            }
            if let Some(second) = n.get("second") {
                if args.get(1).map(String::as_str) != Some(s(second).as_str()) {
                    continue;
                }
            }
            exit = int(&n["exit"]);
            text = s(&n["text"]);
            break;
        }
        self.ev(json!({"native": program, "args": args, "exit": exit}));
        (exit, text)
    }
    fn popup(&mut self, title: &str, text: &str, seconds: i64, buttons: i64) -> i64 {
        let answer = self.popups.pop_front().unwrap_or(-1);
        self.ev(json!({"popup": title, "text": text, "seconds": seconds, "buttons": buttons, "answer": answer}));
        answer
    }
    fn reg_set(&mut self, name: &str, value: &str) -> Result<(), String> {
        if self.refuse.as_deref() == Some(name) {
            return Err("Requested registry access is not allowed.".into());
        }
        self.registry.insert(name.to_string(), value.to_string());
        self.ev(json!({"reg-set": name}));
        Ok(())
    }
    fn reg_remove(&mut self, name: &str) {
        let present = self.registry.contains_key(name);
        self.ev(json!({"reg-remove": name, "present": present}));
        if present && self.registry[name] != "sticky" {
            self.registry.remove(name);
        }
    }
    fn reg_present(&self, name: &str) -> bool {
        self.registry.contains_key(name)
    }
    fn protect_state_dir(&mut self, state: &str) -> Result<(), String> {
        self.ev(json!({"protect": state}));
        match &self.case["protect_error"] {
            Value::Null => Ok(()),
            e => Err(s(e)),
        }
    }
    fn copy_program(&mut self, _state: &str) -> Result<(), String> {
        self.ev(json!({"copy": "program"}));
        Ok(())
    }
    fn register_task(&mut self, _state: &str) -> Result<(), String> {
        self.ev(json!({"task": "register"}));
        match &self.case["register_task_error"] {
            Value::Null => Ok(()),
            e => Err(s(e)),
        }
    }
    fn unregister_task(&mut self) -> bool {
        let r = self.task_present;
        self.ev(json!({"task": "unregister", "present": r}));
        self.task_present = false;
        r
    }
    fn log(&mut self, line: &str) {
        self.ev(json!({"log": norm_line(line)}));
    }
    fn host(&mut self, line: &str) {
        self.ev(json!({"host": line}));
    }
    fn marker_path(&self) -> String {
        format!("{STATE_ROOT}\\storage-mode-safeboot.txt")
    }
    fn exists(&self, path: &str) -> bool {
        self.map(path).exists()
    }
    fn mkdir(&mut self, path: &str) {
        std::fs::create_dir_all(self.map(path)).unwrap();
    }
    fn remove(&mut self, path: &str) {
        let _ = std::fs::remove_file(self.map(path));
    }
    fn move_file(&mut self, from: &str, to: &str) {
        std::fs::rename(self.map(from), self.map(to)).unwrap();
    }
    fn append_line(&mut self, path: &str, line: &str) {
        use std::io::Write;
        let p = self.map(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(p).unwrap();
        write!(f, "{line}\r\n").unwrap();
    }
    fn save_json(&mut self, path: &str, doc: &Value) {
        let p = self.map(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, serde_json::to_string_pretty(doc).unwrap()).unwrap();
        self.ev(json!({"save": path, "doc": doc}));
    }
    fn read_json(&self, path: &str) -> Option<Value> {
        serde_json::from_str(&std::fs::read_to_string(self.map(path)).ok()?).ok()
    }
    fn read_lines(&self, path: &str) -> Vec<String> {
        std::fs::read_to_string(self.map(path)).unwrap_or_default().lines().map(|l| l.trim_end_matches('\r').to_string()).collect()
    }
}

/// Every file under the temporary folder, named as the flow named it.
fn snapshot(tmp: &Path) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    let mut stack = vec![tmp.join("ProgramData/upgrade_"), tmp.join("stick")];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            let rel = p.strip_prefix(tmp).unwrap().to_string_lossy().replace('/', "\\");
            let name = if let Some(r) = rel.strip_prefix("ProgramData\\upgrade_") { format!("{STATE_ROOT}{r}") } else { format!("{STICK}{}", rel.strip_prefix("stick\\").unwrap()) };
            let text = std::fs::read_to_string(&p).unwrap();
            let v = if name.ends_with(".json") {
                serde_json::from_str(&text).unwrap()
            } else {
                let mut lines: Vec<String> = text.split('\n').map(|l| norm_line(l.trim_end_matches('\r'))).collect();
                if lines.last().is_some_and(|l| l.is_empty()) {
                    lines.pop();
                }
                json!(lines)
            };
            out.insert(name, v);
        }
    }
    out
}

/// The Rust's value against the golden's, with the two allowed differences.
fn same(rust: &Value, gold: &Value) -> bool {
    match (rust, gold) {
        (Value::String(t), Value::Object(o)) if o.len() == 1 && o.contains_key("Length") => t.chars().count() as i64 == int(&o["Length"]),
        (Value::Object(a), Value::Object(b)) => {
            let a_keys: Vec<&String> = a.keys().filter(|k| *k != "harness_program").collect();
            a_keys.len() == b.len() && a_keys.iter().all(|k| b.get(*k).is_some_and(|v| same(&a[*k], v)))
        }
        (Value::Array(a), Value::Array(b)) => a.len() == b.len() && a.iter().zip(b).all(|(x, y)| same(x, y)),
        (Value::Number(a), Value::Number(b)) => a.as_f64() == b.as_f64(),
        _ => rust == gold,
    }
}

fn run_fn(case: &Value) -> Value {
    let a = &case["args"];
    match s(&case["fn"]).as_str() {
        "Get-SmPciId" => json!(sm::pci_id(&s(&a["DeviceId"]))),
        "Get-SmClassCodes" => json!(sm::class_codes(&a["CompatibleIds"])),
        "Get-SmStorageControllers" => Value::Array(sm::storage_controllers(&a["Pnp"])),
        "mode-of" => json!(sm::mode_word(&sm::storage_controllers(&a["Pnp"]))),
        "Get-SmNextMode" => sm::next_mode(&s(&a["Current"])).map_or(Value::Null, |m| json!(m)),
        "Get-SmModeLabel" => json!(sm::mode_label(&s(&a["Mode"]))),
        "Get-SmReportCheck" => sm::report_check(&a["Report"]),
        "ConvertFrom-SmBcdCopy" => sm::bcd_copy_guid(&s(&a["Text"])).map_or(Value::Null, |g| json!(g)),
        "Test-SmSafeMode" => json!(sm::is_safe_mode(if a["OptionValue"].is_null() { None } else { Some(int(&a["OptionValue"])) })),
        "Get-SmLegOutcome" => json!(sm::leg_outcome(int(&a["Leg"]), &s(&a["FirstMode"]), &s(&a["ThisMode"]))),
        "ConvertFrom-SmSafeMarker" => Value::Array(sm::safe_marker_lines(&items(&a["Lines"]).iter().map(|l| s(l)).collect::<Vec<_>>())),
        "Get-SmFlowResult" => json!(sm::flow_result(&s(&a["Stage"]), int(&a["Legs"]), &s(&a["LastOutcome"]))),
        "ConvertTo-SmCsvLine" => json!(sm::csv_line(&items(&a["Fields"]).into_iter().cloned().collect::<Vec<_>>())),
        other => panic!("no such function here: {other}"),
    }
}

fn run_flow(case: &Value, tmp: &Path) -> (i32, Vec<Value>, BTreeMap<String, Value>) {
    let _ = std::fs::remove_dir_all(tmp);
    std::fs::create_dir_all(tmp.join("ProgramData/upgrade_/storage-mode")).unwrap();
    std::fs::create_dir_all(tmp.join("stick")).unwrap();
    let phase = s(&case["phase"]);
    if truthy(&case["stick_folder"]) {
        std::fs::create_dir_all(tmp.join("stick/upgrade_/storage-mode")).unwrap();
    }
    if !case["state"].is_null() {
        std::fs::write(tmp.join("ProgramData/upgrade_/storage-mode/state.json"), serde_json::to_string_pretty(&case["state"]).unwrap()).unwrap();
    }
    for (name, doc) in case["state_files"].as_object().into_iter().flatten() {
        std::fs::write(tmp.join("ProgramData/upgrade_/storage-mode").join(name), serde_json::to_string_pretty(doc).unwrap()).unwrap();
    }
    if let Value::Array(lines) = &case["marker"] {
        let text: String = lines.iter().map(|l| format!("{}\r\n", s(l))).collect();
        std::fs::write(tmp.join("ProgramData/upgrade_/storage-mode-safeboot.txt"), text).unwrap();
    }
    let mut fake = Fake::new(case, tmp);
    let args = Args { phase: phase.clone(), stick_drive: s(&case["args"]["stick"]), bench: truthy(&case["args"]["bench"]), no_prompt: truthy(&case["args"]["no_prompt"]) };
    let exit = Flow::new(&mut fake, STATE_DIR).run(&args);
    let files = snapshot(tmp);
    (exit, fake.events, files)
}

#[test]
fn every_function_answers_what_the_script_answered() {
    let g = golden();
    let mut failed = Vec::new();
    let mut n = 0;
    for case in cases().iter().filter(|c| s(&c["kind"]) == "fn") {
        n += 1;
        let name = s(&case["name"]);
        let want = &g[&name]["result"];
        let got = run_fn(case);
        if !same(&got, want) {
            failed.push(format!("{name}\n    got  {got}\n    want {want}"));
        }
    }
    assert!(n >= 60, "the cases file lost its function cases");
    assert!(failed.is_empty(), "{} of {n} function cases differ from the script:\n  {}", failed.len(), failed.join("\n  "));
}

#[test]
fn every_flow_does_what_the_script_did() {
    let g = golden();
    let tmp = std::env::temp_dir().join(format!("upgrade-storage-mode-test-{}", std::process::id()));
    let mut failed = Vec::new();
    let mut n = 0;
    for case in cases().iter().filter(|c| s(&c["kind"]) == "flow") {
        n += 1;
        let name = s(&case["name"]);
        let want = &g[&name];
        let (exit, events, files) = run_flow(case, &tmp);
        let mut why = Vec::new();
        if exit != int(&want["exit"]) as i32 {
            why.push(format!("exit {exit}, the script's {}", int(&want["exit"])));
        }
        let gold_events = items(&want["events"]);
        for i in 0..events.len().max(gold_events.len()) {
            match (events.get(i), gold_events.get(i)) {
                (Some(a), Some(b)) if same(a, b) => {}
                (a, b) => {
                    why.push(format!("event {i}:\n      got  {}\n      want {}", a.map_or("(none)".to_string(), |v| v.to_string()), b.map_or("(none)".to_string(), |v| v.to_string())));
                    break;
                }
            }
        }
        let gold_files = want["files"].as_object().cloned().unwrap_or_default();
        for k in files.keys().chain(gold_files.keys()).collect::<std::collections::BTreeSet<_>>() {
            match (files.get(k), gold_files.get(k)) {
                (Some(a), Some(b)) if same(a, b) => {}
                (a, b) => why.push(format!("file {k}:\n      got  {}\n      want {}", a.map_or("(none)".to_string(), |v| v.to_string()), b.map_or("(none)".to_string(), |v| v.to_string()))),
            }
        }
        if !why.is_empty() {
            failed.push(format!("{name}\n    {}", why.join("\n    ")));
        }
    }
    let _ = std::fs::remove_dir_all(&tmp);
    assert!(n >= 50, "the cases file lost its flow cases");
    assert!(failed.is_empty(), "{} of {n} flow cases differ from the script:\n  {}", failed.len(), failed.join("\n  "));
}

#[test]
fn the_runonce_commands_launch_this_program() {
    let safe = sm::safe_runonce_command(&sm::program_in(STATE_DIR), STATE_DIR);
    assert_eq!(safe, "\"C:\\ProgramData\\upgrade_\\storage-mode\\upgrade-prologue.exe\" storage-mode safe-mode-sign-in --state-dir \"C:\\ProgramData\\upgrade_\\storage-mode\"");
    assert!(sm::SAFE_RUNONCE_NAME.starts_with('*'), "the value name carries the Safe Mode asterisk");
    assert_eq!(sm::notice_command(&sm::program_in(STATE_DIR), STATE_DIR), "\"C:\\ProgramData\\upgrade_\\storage-mode\\upgrade-prologue.exe\" storage-mode notify --state-dir \"C:\\ProgramData\\upgrade_\\storage-mode\"");
    // the marker line reads back as the script's ConvertFrom-SmSafeMarker read it
    let line = sm::safe_marker_line("2026-09-14T06:07:40Z", "UPGRIGHV\\rig", 1, 341);
    let back = sm::safe_marker_lines(&[line]);
    assert_eq!(back[0], json!({"Utc": "2026-09-14T06:07:40Z", "RunAs": "UPGRIGHV\\rig", "OptionValue": "1", "UptimeSeconds": "341", "Source": "runonce-signin"}));
}
