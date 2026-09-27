//! settle-in: the first startup of the new Linux system (architecture.md,
//! "Module 3: settle-in" and "It runs on any Linux", decided 2026-09-27).
//!
//! One self-contained program, the same file on every distribution. It
//! reads only the handoff folder the installer adapter filled
//! (/var/lib/upgrade_/: job.json, outcome.json, the Wi-Fi password files)
//! and works through kernel interfaces and file formats every desktop
//! distribution shares.
//!
//!   settle-in first-start [--root DIR] [--rtc DEVICE]
//!       Before the network, once: the clock, then Wi-Fi. Writes its report
//!       to /var/lib/upgrade_/settle-in/report.json (root-only) and a done
//!       marker. --root runs it against a copy of a system (tests); --rtc
//!       names the hardware clock device (default /dev/rtc0).
//!   settle-in --version

mod civil;
mod clock;
mod hw;
mod wifi;
mod zone;

use serde_json::{json, Value};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const HANDOFF: &str = "var/lib/upgrade_";

fn read_json(path: &str) -> Result<Value, String> {
    let t = std::fs::read_to_string(path).map_err(|e| format!("{}: {}", path, e))?;
    serde_json::from_str(&t).map_err(|e| format!("{} is not JSON ({})", path, e))
}

/// Root-only, whole or not at all.
fn save(path: &str, v: &Value) -> Result<(), String> {
    let tmp = format!("{}.part", path);
    let _ = std::fs::remove_file(&tmp);
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp).map_err(|e| format!("{}: {}", tmp, e))?;
    let text = serde_json::to_string_pretty(v).map_err(|e| e.to_string())? + "\n";
    f.write_all(text.as_bytes()).and_then(|_| f.sync_all()).map_err(|e| format!("{}: {}", tmp, e))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {}", path, e))
}

fn now_iso() -> String {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    civil::iso_utc(t)
}

fn exists(p: &str) -> bool {
    std::path::Path::new(p).exists()
}

fn first_start(root: &str, rtc: &str) -> i32 {
    let r = root.trim_end_matches('/');
    let handoff = format!("{}/{}", r, HANDOFF);
    let state = format!("{}/settle-in", handoff);
    let done = format!("{}/first-start.done", state);
    let report_path = format!("{}/report.json", state);
    if !exists(&format!("{}/job.json", handoff)) {
        println!("settle-in: no handoff folder ({}); this system was not installed by upgrade_, nothing to do", handoff);
        return 0;
    }
    if exists(&done) {
        println!("settle-in: first startup already done ({})", done);
        return 0;
    }
    // a reader that meets a version it does not understand refuses, not guesses
    let job = match read_json(&format!("{}/job.json", handoff)) {
        Ok(j) if j.get("schema") == Some(&json!("job/1")) => j,
        Ok(j) => { eprintln!("settle-in: job schema {} is not job/1; refusing", j.get("schema").unwrap_or(&Value::Null)); return 2; }
        Err(e) => { eprintln!("settle-in: {}", e); return 2; }
    };
    let outcome = match read_json(&format!("{}/outcome.json", handoff)) {
        Ok(o) if o.get("schema") == Some(&json!("outcome/1")) => o,
        Ok(o) => { eprintln!("settle-in: outcome schema {} is not outcome/1; refusing", o.get("schema").unwrap_or(&Value::Null)); return 2; }
        Err(e) => { eprintln!("settle-in: {}", e); return 2; }
    };
    if let Err(e) = std::fs::create_dir_all(&state) {
        eprintln!("settle-in: {}: {}", state, e);
        return 2;
    }
    let earlier = read_json(&report_path).ok();
    let iana = job.pointer("/harvest/clock/iana").and_then(Value::as_str).unwrap_or("").to_string();
    let zone = zone::Zone::load(root, &iana);
    let mut report = json!({
        "schema": "settle-in-report/1",
        "settle_in_version": VERSION,
        "job_id": job.get("job_id"),
        "started_utc": now_iso(),
        "floor": {
            "uefi": exists(&format!("{}/sys/firmware/efi", r)),
            "systemd": exists(&format!("{}/run/systemd/system", r)),
            "networkmanager": wifi::has_networkmanager(root),
            "zone_rules": match &zone { Ok(_) => json!(iana), Err(e) => json!(format!("missing: {}", e)) },
        },
    });

    // --- the clock, first: before anything time-sensitive, before the network
    let clock_before = earlier.as_ref().and_then(|e| e.pointer("/clock/result")).and_then(Value::as_str).map(str::to_string);
    report["clock"] = if clock_before.as_deref() == Some("attempting") {
        // a crash or power cut in the middle: never correct twice
        json!({ "result": "left-alone", "why": "an earlier attempt was interrupted; the clock is not corrected twice" })
    } else {
        match clock::evidence(&job, &outcome) {
            Err(why) => json!({ "result": "left-alone", "why": why }),
            Ok(ev) => {
                report["clock"] = json!({ "result": "attempting" });
                if let Err(e) = save(&report_path, &report) {
                    eprintln!("settle-in: cannot record the attempt, so the clock is not touched: {}", e);
                    json!({ "result": "left-alone", "why": format!("the attempt could not be recorded first ({})", e) })
                } else {
                    let created = job.get("created_utc").and_then(Value::as_str).and_then(civil::parse_iso_utc).unwrap_or(0);
                    let mut m = hw::Real { rtc_path: rtc.to_string() };
                    clock::run(&mut m, root, &ev, zone.as_ref().ok(), created, &outcome)
                }
            }
        }
    };
    println!("settle-in: clock: {} {}", report["clock"]["result"], report["clock"].get("why").map(|w| w.to_string()).unwrap_or_default());

    // --- Wi-Fi
    report["wifi"] = wifi::run(root, &handoff, &job);
    println!("settle-in: wifi: {} ({} created)", report["wifi"]["result"], report["wifi"].get("created").unwrap_or(&json!(0)));

    report["finished_utc"] = json!(now_iso());
    if let Err(e) = save(&report_path, &report) {
        eprintln!("settle-in: {}", e);
        return 2;
    }
    if let Err(e) = std::fs::write(&done, format!("{}\n", now_iso())) {
        eprintln!("settle-in: {}: {}", done, e);
        return 2;
    }
    println!("settle-in: report written: {}", report_path);
    0
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opt = |name: &str, default: &str| {
        args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned().unwrap_or_else(|| default.to_string())
    };
    match args.first().map(String::as_str) {
        Some("--version") => println!("settle-in {}", VERSION),
        Some("first-start") => std::process::exit(first_start(&opt("--root", "/"), &opt("--rtc", "/dev/rtc0"))),
        _ => {
            eprintln!("usage: settle-in first-start [--root DIR] [--rtc DEVICE] | --version");
            std::process::exit(64);
        }
    }
}
