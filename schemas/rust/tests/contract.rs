//! schemas/check.py, replayed against the Rust reader (RISKS R32, the
//! parity ledger's `schemas/check.py` lines). The refused documents come
//! from `refused-cases.json`, which check.py itself writes
//! (`python3 schemas/check.py --dump-cases schemas/rust/tests/refused-cases.json`),
//! so both checkers judge the same documents.

use serde_json::Value;
use std::path::PathBuf;
use upgrade_schema::{Job, Kind, Outcome, violations};

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples")
}

fn example(name: &str) -> Value {
    let text = std::fs::read_to_string(examples_dir().join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn kind_of(name: &str) -> Kind {
    match name.split('.').next() {
        Some("job") => Kind::Job,
        Some("outcome") => Kind::Outcome,
        other => panic!("{name}: unknown kind {other:?}"),
    }
}

/// Apply one edit: `[path, value]` sets, `[path]` deletes.
fn apply(doc: &mut Value, op: &Value) {
    let path = op[0].as_array().expect("an edit has a path");
    let (last, parents) = path.split_last().expect("an edit path is not empty");
    let mut cur = doc;
    for step in parents {
        cur = match step {
            Value::String(k) => cur.get_mut(k.as_str()),
            n => cur.get_mut(n.as_u64().unwrap() as usize),
        }
        .unwrap_or_else(|| panic!("edit path {path:?} does not exist"));
    }
    match (op.get(1), last) {
        (Some(v), Value::String(k)) => {
            cur.as_object_mut().unwrap().insert(k.clone(), v.clone());
        }
        (Some(v), n) => cur.as_array_mut().unwrap()[n.as_u64().unwrap() as usize] = v.clone(),
        (None, Value::String(k)) => {
            cur.as_object_mut().unwrap().remove(k.as_str());
        }
        (None, n) => {
            cur.as_array_mut().unwrap().remove(n.as_u64().unwrap() as usize);
        }
    }
}

#[test]
fn every_example_validates() {
    let mut names: Vec<String> = std::fs::read_dir(examples_dir())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".json"))
        .collect();
    names.sort();
    assert!(names.len() >= 9, "examples/ has documents");
    for name in names {
        let v = violations(kind_of(&name), &example(&name));
        let shown: Vec<String> = v.iter().map(|x| x.to_string()).collect();
        assert!(v.is_empty(), "example validates: {name}\n{}", shown.join("\n"));
    }
}

#[test]
fn example_pairs_share_job_id() {
    for pair in ["keep-windows", "clean-slate", "acknowledged-data-loss"] {
        let job = Job::from_value(example(&format!("job.{pair}.json"))).unwrap();
        let out = Outcome::from_value(example(&format!("outcome.{pair}.json"))).unwrap();
        assert!(!job.job_id().is_empty());
        assert_eq!(job.job_id(), out.job_id(), "example pair: {pair} job and outcome share job_id");
    }
}

#[test]
fn every_refused_case_is_refused() {
    let cases: Vec<Value> = serde_json::from_str(include_str!("refused-cases.json")).unwrap();
    // check.py had 91 refused cases when this was written (2026-10-04); a
    // shorter list means the file was cut, not that the contract loosened
    assert!(cases.len() >= 91, "refused-cases.json holds {} cases", cases.len());
    let mut accepted = Vec::new();
    for c in &cases {
        let base = c["base"].as_str().unwrap();
        let mut doc = example(base);
        for op in c["ops"].as_array().unwrap() {
            apply(&mut doc, op);
        }
        assert!(!c["ops"].as_array().unwrap().is_empty(), "{}: a refused case must differ from its example", c["name"]);
        if violations(kind_of(base), &doc).is_empty() {
            accepted.push(format!("refused: {} (document was ACCEPTED)", c["name"].as_str().unwrap()));
        }
    }
    assert!(accepted.is_empty(), "{}", accepted.join("\n"));
}

#[test]
fn a_document_is_only_handed_out_after_it_passed() {
    let mut red = example("job.keep-windows.json");
    red["scan"]["verdict"] = "RED".into();
    assert!(Job::from_value(red).is_err(), "rule #1: no RED job, ever");
    assert!(Job::read("{ not json").is_err());
    assert!(Outcome::from_value(example("job.keep-windows.json")).is_err(), "a job is not an outcome");
    let text = std::fs::read_to_string(examples_dir().join("job.keep-windows.json")).unwrap();
    let job = Job::read(&format!("\u{feff}{text}")).expect("a byte order mark from PowerShell 5.1 is read past");
    assert_eq!(job.at("/schema").and_then(Value::as_str), Some("job/1"));
}

#[test]
fn a_date_that_is_not_a_date_is_refused() {
    // stricter than check.py on a machine without Python's optional date
    // library, where `format` is skipped (docs/RUST-PORT.md, "Differences")
    let mut out = example("outcome.keep-windows.json");
    let mut found = 0;
    fn poison(v: &mut Value, found: &mut u32) {
        match v {
            Value::Object(m) => {
                for (k, x) in m.iter_mut() {
                    if *found == 0 && k.ends_with("_utc") && x.is_string() {
                        *x = "yesterday".into();
                        *found += 1;
                    } else {
                        poison(x, found);
                    }
                }
            }
            Value::Array(l) => l.iter_mut().for_each(|x| poison(x, found)),
            _ => {}
        }
    }
    poison(&mut out, &mut found);
    assert_eq!(found, 1);
    assert!(!violations(Kind::Outcome, &out).is_empty());
}

/// The differential run (VALIDATION V13, method 2): thousands of one-edit
/// documents, each judged by check.py, must get the same answer here.
/// `port-check.sh` writes the file and sets the variable.
#[test]
#[ignore = "needs UPGRADE_SCHEMA_MUTATIONS, written by check.py --dump-mutations (run ./port-check.sh)"]
fn differential_against_check_py() {
    let path = std::env::var("UPGRADE_SCHEMA_MUTATIONS").expect("UPGRADE_SCHEMA_MUTATIONS is set");
    let text = std::fs::read_to_string(&path).unwrap();
    let (mut n, mut stricter, mut wrong) = (0, 0, Vec::new());
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let m: Value = serde_json::from_str(line).unwrap();
        let base = m["base"].as_str().unwrap();
        let mut doc = example(base);
        apply(&mut doc, &m["op"]);
        let v = violations(kind_of(base), &doc);
        let python_accepted = m["accepted"].as_bool().unwrap();
        n += 1;
        if v.is_empty() == python_accepted {
            continue;
        }
        // the one allowed difference: a date-time Python did not check
        if python_accepted && v.iter().all(|x| x.message.ends_with("is not a date-time")) {
            stricter += 1;
            continue;
        }
        wrong.push(format!("{base} {}: python accepted={python_accepted}, rust says {:?}", m["op"], v.first().map(|x| x.to_string())));
    }
    assert!(n > 1000, "only {n} documents in {path}");
    assert!(wrong.is_empty(), "{} of {n} differ:\n{}", wrong.len(), wrong[..wrong.len().min(20)].join("\n"));
    println!("{n} one-edit documents judged the same; {stricter} more refused here for a date-time check.py skipped");
}
