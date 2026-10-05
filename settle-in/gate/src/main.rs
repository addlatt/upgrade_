//! upgrade-gate: the gate on a "Go back to Windows" stick (RISKS R33;
//! architecture.md, "The way back to Windows", stage 2).
//!
//! It runs first inside Windows Setup's own environment (WinPE), started by
//! a winpeshl.ini in the stick's boot.wim, before Setup:
//!
//!   1. finds the stick (the drive holding upgrade_\go-back.json) and reads
//!      the job the Linux program wrote;
//!   2. refuses, before anything changes, a job that is not ours, a job
//!      that already crossed its commit line, and any drive it cannot find
//!      by serial and exact size (or finds twice, or finds on the stick);
//!   3. shows a 2-minute countdown. Any key cancels: nothing erased, a
//!      restart into Linux (the stick's boot was one-time);
//!   4. when the countdown ends (the commit line), records the crossing on
//!      the stick FIRST, so this stick never starts twice, writes Setup's
//!      answer file naming the drives it found, and starts Windows Setup.
//!
//! Its only writes are its own records on the stick. Setup does the erase.
//!
//!   upgrade-gate                 the WinPE start (refuses to run anywhere else)
//!   upgrade-gate --list          the drives this Windows sees, as JSON (read-only)
//!   upgrade-gate --check JOB     what the gate would decide here (read-only)

mod logic;
mod win;

use logic::{Found, Seen};
use serde_json::{json, Value};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

fn seen_json(s: &[Seen]) -> Value {
    Value::Array(s.iter().map(|d| json!({ "number": d.number, "serial": d.serial, "size": d.size, "model": d.model, "ids": d.ids })).collect())
}
fn found_json(f: &[Found]) -> Value {
    Value::Array(f.iter().map(|d| json!({ "role": d.role, "number": d.number, "model": d.model, "size": d.size, "matched_by": d.how })).collect())
}

/// UTC now, "YYYY-MM-DDTHH:MM:SSZ" (WinPE's clock is the firmware's, as it reads it).
fn now_utc() -> String {
    let s = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    let (days, rem) = (s.div_euclid(86400), s.rem_euclid(86400));
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, m, d, rem / 3600, rem % 3600 / 60, rem % 60)
}

fn find_stick() -> Option<String> {
    ('C'..='Z').map(|c| format!("{}:", c)).find(|l| std::path::Path::new(&format!("{}\\upgrade_\\go-back.json", l)).exists())
}

fn load(p: &str) -> Option<Value> {
    std::fs::read_to_string(p).ok().and_then(|s| serde_json::from_str(s.trim_start_matches('\u{feff}')).ok())
}

/// Write the record and read it back. A write the stick silently dropped is
/// what 0.3.0 could not see (the Aspire, 2026-10-05: a cancelled countdown left
/// no record and the Wi-Fi file in place; nothing on screen said so).
fn save(p: &str, v: &Value) -> Result<(), String> {
    use std::io::Write;
    let text = serde_json::to_string_pretty(v).unwrap_or_default();
    let mut f = std::fs::File::create(p).map_err(|e| format!("create {}: {}", p, e))?;
    f.write_all(text.as_bytes()).map_err(|e| format!("write {}: {}", p, e))?;
    f.sync_all().map_err(|e| format!("sync {}: {}", p, e))?;
    drop(f);
    let back = std::fs::read_to_string(p).map_err(|e| format!("read back {}: {}", p, e))?;
    if back != text {
        return Err(format!("read back {}: the stick returned different bytes", p));
    }
    Ok(())
}

/// The Wi-Fi passwords leave the stick at every stop (decided 2026-10-02).
/// Checked afterwards: "removed" means the folder is gone.
fn wipe_wifi(stick: &str) -> Value {
    let d = format!("{}\\upgrade_\\wifi", stick);
    if !std::path::Path::new(&d).exists() {
        return json!("none on the stick");
    }
    let r = std::fs::remove_dir_all(&d);
    if std::path::Path::new(&d).exists() {
        return json!(format!("NOT removed ({})", r.err().map(|e| e.to_string()).unwrap_or_else(|| "still present after the removal".into())));
    }
    json!("removed")
}
fn wifi_gone(v: &Value) -> bool {
    matches!(v.as_str(), Some("removed") | Some("none on the stick"))
}

fn refuse_and_restart(record_path: Option<&str>, mut rec: Value, why: &str) -> i32 {
    rec["result"] = json!("refused");
    rec["why"] = json!(why);
    if let Some(p) = record_path {
        // <stick>\upgrade_\go-back-gate.json -> <stick>
        rec["wifi_on_stick"] = wipe_wifi(&p[..2]);
    }
    rec["ended_utc"] = json!(now_utc());
    let mut unsaved = None;
    if let Some(p) = record_path {
        unsaved = save(p, &rec).err();
    }
    win::clear();
    print!("{}", logic::refusal_screen(why));
    if let Some(e) = unsaved {
        print!("\n   (This reason could not be written to the USB stick: {}.)\n", e);
    }
    win::flush_keys();
    win::key_within(60_000);
    win::reboot();
    3
}

fn run() -> i32 {
    // Only ever inside WinPE, from the stick: never on an installed Windows.
    let sysdrive = std::env::var("SystemDrive").unwrap_or_default();
    if !sysdrive.eq_ignore_ascii_case("X:") || !std::path::Path::new("X:\\Windows\\System32\\wpeutil.exe").exists() {
        eprintln!("upgrade-gate only runs from a \"Go back to Windows\" stick, in Windows Setup. Nothing was done.");
        return 2;
    }
    let started = now_utc();
    let mut rec = json!({ "schema": "go-back-gate/1", "gate_version": VERSION, "started_utc": started });
    // On a real machine the USB stick appears some seconds after WinPE starts
    // (the Aspire, 2026-10-02: the gate looked once, at once, and refused with
    // no record); on the rig the disks are there at once. Wait up to 90 s.
    let mut stick = find_stick();
    let t0 = std::time::Instant::now();
    while stick.is_none() && t0.elapsed().as_secs() < 90 {
        win::clear();
        print!("\n\n   Looking for the USB stick... ({} s)\n\n   Nothing has been changed.\n", t0.elapsed().as_secs());
        std::thread::sleep(std::time::Duration::from_secs(2));
        stick = find_stick();
    }
    rec["stick_wait_s"] = json!(t0.elapsed().as_secs());
    let Some(stick) = stick else {
        // nowhere to write a record: say on screen what WinPE can see instead
        let seen: Vec<String> = ('C'..='Z').map(|c| format!("{}:", c)).filter(|l| std::path::Path::new(&format!("{}\\", l)).exists()).collect();
        let why = format!("the instructions from Linux were not found on this USB stick after {} s (drives seen: {}; this reason could not be written to the stick)",
            t0.elapsed().as_secs(), if seen.is_empty() { "none".to_string() } else { seen.join(" ") });
        return refuse_and_restart(None, rec, &why);
    };
    let rec_path = format!("{}\\upgrade_\\go-back-gate.json", stick);
    let previous = load(&rec_path);
    let Some(job) = load(&format!("{}\\upgrade_\\go-back.json", stick)) else {
        return refuse_and_restart(Some(&rec_path), rec, "the instructions on this USB stick could not be read");
    };
    rec["job_id"] = job["job_id"].clone();
    if let Some(p) = &previous {
        rec["previous"] = json!({ "job_id": p["job_id"], "result": p["result"], "ended_utc": p["ended_utc"] });
    }
    if logic::already_crossed(&job, previous.as_ref()) {
        // Started again after the line (the firmware chose the stick during
        // Setup's restarts, or it was left in). The record keeps "crossed":
        // only a note is added. The Wi-Fi files stay for the first sign-in.
        // Then Windows' own boot manager is asked for, once.
        let mut p = previous.clone().unwrap_or_else(|| json!({}));
        let mut again = p["started_again"].as_array().cloned().unwrap_or_default();
        let st = std::process::Command::new("X:\\Windows\\System32\\bcdedit.exe").args(["/set", "{fwbootmgr}", "bootsequence", "{bootmgr}"]).status();
        let handed = st.as_ref().map(|s| s.success()).unwrap_or(false);
        again.push(json!({ "utc": now_utc(), "gate_version": VERSION, "handed_to_windows": handed }));
        p["started_again"] = json!(again);
        let _ = save(&rec_path, &p);   // a note only; the crossing itself is already recorded
        win::clear();
        print!("{}", logic::after_crossing_screen(handed));
        if handed {
            std::thread::sleep(std::time::Duration::from_secs(8));
        } else {
            win::flush_keys();
            win::key_within(60_000);
        }
        win::reboot();
        return 4;
    }
    if let Err(why) = logic::check_job(&job, previous.as_ref()) {
        return refuse_and_restart(Some(&rec_path), rec, &why);
    }
    let seen = win::list_disks();
    let stick_disk = win::disk_of_volume(&stick);
    rec["seen"] = seen_json(&seen);
    rec["stick_disk"] = json!(stick_disk);
    if stick_disk.is_none() {
        return refuse_and_restart(Some(&rec_path), rec, "the USB stick's own drive could not be identified, so it cannot be kept safe");
    }
    let found = match logic::find_drives(&job, &seen, stick_disk) {
        Ok(f) => f,
        Err(why) => return refuse_and_restart(Some(&rec_path), rec, &why),
    };
    rec["found"] = found_json(&found);

    // The stick must take a write before the countdown starts (0.3.1): the crossing
    // is recorded there first, and a stick that cannot hold the record cannot be
    // kept from erasing twice. Checked by reading the record back.
    rec["result"] = json!("counting-down");
    if let Err(e) = save(&rec_path, &rec) {
        return refuse_and_restart(Some(&rec_path), rec, &format!("this USB stick did not keep what was written to it ({}), so it could not record a decision", e));
    }

    // The countdown: the last exit (rule #3). Keys pressed before it do not count.
    win::flush_keys();
    let t0 = std::time::Instant::now();
    let total = logic::COUNT_SECS;
    loop {
        let spent = t0.elapsed().as_secs();
        if spent >= total {
            break;
        }
        win::clear();
        print!("{}", logic::countdown_screen(total - spent, &found));
        let to_next = 1000 - (t0.elapsed().as_millis() % 1000) as u32;
        if win::key_within(to_next.max(50)) {
            rec["result"] = json!("cancelled");
            rec["wifi_on_stick"] = wipe_wifi(&stick);
            rec["countdown"] = json!({ "seconds": total, "cancelled_after_s": t0.elapsed().as_secs_f64() });
            rec["ended_utc"] = json!(now_utc());
            let saved = save(&rec_path, &rec);
            win::clear();
            print!("{}", logic::cancelled_screen(saved.as_ref().err().map(String::as_str), wifi_gone(&rec["wifi_on_stick"]), rec["wifi_on_stick"].as_str().unwrap_or("?")));
            if saved.is_err() || !wifi_gone(&rec["wifi_on_stick"]) {
                // something to photograph: wait for a key, up to 10 minutes
                win::flush_keys();
                win::key_within(600_000);
            } else {
                std::thread::sleep(std::time::Duration::from_secs(3));
            }
            win::reboot();
            return 1;
        }
    }

    // ---- the commit line ----
    rec["result"] = json!("crossed");
    rec["countdown"] = json!({ "seconds": total, "elapsed_s": t0.elapsed().as_secs_f64() });
    rec["crossed_utc"] = json!(now_utc());
    if let Err(e) = save(&rec_path, &rec) {
        // never cross on a stick that does not hold the crossing: it could erase twice
        rec["result"] = json!("refused");
        return refuse_and_restart(Some(&rec_path), rec, &format!("this USB stick did not keep the record of the decision ({}); nothing was erased", e));
    }
    // the clock (2026-10-04): only after the line, so a cancel leaves Linux's clock as it was
    let wall = win::local_wall();
    rec["clock"] = match logic::clock_fix(&job, &wall) {
        Some(w) => json!({ "hardware_clock_read": format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}", wall[0], wall[1], wall[2], wall[3], wall[4], wall[5]), "taken_as": "utc",
                           "wall_time_set": format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}", w[0], w[1], w[2], w[3], w[4], w[5]), "offset_seconds": job["clock"]["offset_seconds"], "set": win::set_wall(&w) }),
        None => json!({ "set": false, "why": "the job does not say the hardware clock holds UTC, or names no offset, or the clock was never set" }),
    };
    let _ = save(&rec_path, &rec);
    let xml = logic::unattend(&job, &found);
    let _ = std::fs::create_dir_all("X:\\upgrade_gate");
    let _ = std::fs::write("X:\\upgrade_gate\\unattend.xml", &xml);
    let _ = std::fs::write(format!("{}\\upgrade_\\go-back-unattend.xml", stick), &xml);
    win::clear();
    print!("\n\n   Erasing and installing Windows. This takes a while; you can walk away.\n");
    let st = std::process::Command::new("X:\\sources\\setup.exe").arg("/unattend:X:\\upgrade_gate\\unattend.xml").status();
    rec["setup_exit"] = json!(st.map(|s| s.code()).ok().flatten());
    rec["ended_utc"] = json!(now_utc());
    let _ = save(&rec_path, &rec);
    win::reboot();
    0
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version") => println!("upgrade-gate {}", VERSION),
        Some("--list") => println!("{}", serde_json::to_string_pretty(&seen_json(&win::list_disks())).unwrap_or_default()),
        Some("--check") => {
            let job = args.get(1).and_then(|p| load(p)).unwrap_or(Value::Null);
            let seen = win::list_disks();
            let out = match logic::check_job(&job, None).and_then(|_| logic::find_drives(&job, &seen, None)) {
                Ok(f) => json!({ "result": "would-count-down", "found": found_json(&f) }),
                Err(why) => json!({ "result": "would-refuse", "why": why }),
            };
            println!("{}", serde_json::to_string_pretty(&json!({ "decision": out, "seen": seen_json(&seen) })).unwrap_or_default());
        }
        None => std::process::exit(run()),
        Some(other) => {
            eprintln!("upgrade-gate: unknown argument {}", other);
            std::process::exit(2);
        }
    }
}
