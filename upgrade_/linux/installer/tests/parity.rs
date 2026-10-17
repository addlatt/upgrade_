//! The installer's two steps against verify.sh 0.6.0 and outcome.sh 0.6.1
//! (RISKS R32; VALIDATION V13, V1, V2, V9, R21). `cases.json` is the
//! faked machines (by `make-cases.py`); `golden.json` is what the scripts
//! did for each, recorded once by `record.sh` through the sandbox
//! (`sandbox.py`), frozen. This test runs the program through the same
//! sandbox for every case and requires the same result: the exit code,
//! every line printed, every file left on the stick, in the installed
//! system and under /tmp, every tool called in order, every screen drawn
//! on the console, and the firmware's entries after.
//!
//! The sandbox needs a user namespace (`unshare -Urm`) and python3; the
//! test fails plainly without them rather than passing by doing nothing.

use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;

fn here() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn cases() -> Vec<Value> {
    serde_json::from_str(include_str!("cases.json")).unwrap()
}

fn golden() -> Value {
    serde_json::from_str(include_str!("golden.json")).unwrap()
}

fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same(v, w))),
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same(p, q)),
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        _ => a == b,
    }
}

/// The first place two values differ, as a path and both sides.
fn first_difference(a: &Value, b: &Value, path: &str) -> Option<String> {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            for k in x.keys().chain(y.keys()).collect::<std::collections::BTreeSet<_>>() {
                match (x.get(k), y.get(k)) {
                    (Some(p), Some(q)) => {
                        if let Some(d) = first_difference(p, q, &format!("{path}/{k}")) {
                            return Some(d);
                        }
                    }
                    (p, q) => return Some(format!("{path}/{k}: got {} want {}", p.map_or("(absent)".into(), short), q.map_or("(absent)".into(), short))),
                }
            }
            None
        }
        (Value::Array(x), Value::Array(y)) => {
            for i in 0..x.len().max(y.len()) {
                match (x.get(i), y.get(i)) {
                    (Some(p), Some(q)) => {
                        if let Some(d) = first_difference(p, q, &format!("{path}[{i}]")) {
                            return Some(d);
                        }
                    }
                    (p, q) => return Some(format!("{path}[{i}]: got {} want {}", p.map_or("(absent)".into(), short), q.map_or("(absent)".into(), short))),
                }
            }
            None
        }
        _ => {
            if same(a, b) { None } else { Some(format!("{path}: got {} want {}", short(a), short(b))) }
        }
    }
}

fn short(v: &Value) -> String {
    let s = v.to_string();
    if s.len() > 400 { format!("{}...", &s[..400]) } else { s }
}

#[test]
fn every_case_does_what_the_scripts_did() {
    let probe = Command::new("unshare").args(["-Urm", "true"]).output();
    assert!(probe.map(|o| o.status.success()).unwrap_or(false), "this test needs a user namespace (unshare -Urm) and python3; the sandbox cannot run here");
    let bin = env!("CARGO_BIN_EXE_upgrade-installer");
    let out_root = here().join("target").join("sandbox").join("parity");
    let g = golden();
    let mut failed = Vec::new();
    let mut n = 0;
    for (i, case) in cases().iter().enumerate() {
        n += 1;
        let name = case["name"].as_str().unwrap();
        let mode = case["mode"].as_str().unwrap();
        let out = out_root.join(format!("case-{:02}", i + 1));
        let mut cmd = Command::new("python3");
        cmd.arg(here().join("tests/sandbox.py")).arg(here().join("tests/cases.json")).arg(name).arg(bin).arg(mode).arg("JOB");
        if mode == "verify" {
            cmd.arg("LABEL");
        }
        cmd.arg("--out").arg(&out);
        let r = cmd.output().expect("python3 runs");
        assert!(r.status.success(), "the sandbox failed on {name}:\n{}{}", String::from_utf8_lossy(&r.stdout), String::from_utf8_lossy(&r.stderr));
        let mut got: Value = serde_json::from_str(&std::fs::read_to_string(out.join("result.json")).unwrap()).unwrap();
        got.as_object_mut().unwrap().remove("sandbox_stderr");
        let want = &g[name];
        assert!(!want.is_null(), "no golden for {name}");
        if let Some(d) = first_difference(&got, want, "") {
            failed.push(format!("{name}\n    {d}"));
        }
    }
    assert!(n >= 40, "the cases file lost cases");
    assert!(failed.is_empty(), "{} of {n} cases differ from the scripts:\n  {}", failed.len(), failed.join("\n  "));
}
