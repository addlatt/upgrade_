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
//!   settle-in summary [--root DIR] [--text]
//!       The screen, as JSON sections (the window) or as text (a console),
//!       from the public summary. No root needed; no secret in it.
//!   settle-in remove-old-boot-entry [--root DIR]
//!       The button (root, through pkexec): re-checks everything, then removes
//!       firmware entries that point at Windows on a partition that no longer
//!       exists. Prints what it did as JSON.
//!   settle-in go-back screen [--root DIR] [--text]
//!       "Go back to Windows": the first screen (the cost, which Windows it
//!       was, which one to download), as JSON sections or text. No root.
//!   settle-in go-back downloads [--home DIR]
//!       ISO files in the person's Downloads folder, newest first.
//!   settle-in go-back check FILE [--want 10|11]
//!       Checks a downloaded installer against Microsoft's published SHA-256
//!       table. Progress on stderr, the verdict as JSON. Refuses anything
//!       not in the table.
//!   settle-in go-back sticks [--root DIR] [--min-bytes N] [--facts]
//!       Every disk, and the rules it breaks (R16 on Linux); only a disk
//!       that breaks none is offered. --facts prints the raw facts instead
//!       (a recording, replayable in tests). Read-only, no root.
//!   settle-in go-back write --iso FILE [--want 10|11] --serial S --size N --typed MODEL
//!                            [--wimlib PATH] [--root DIR]
//!   settle-in go-back write --iso FILE [--want 10|11] --image NEWFILE --size N
//!       The one writer (root, through pkexec): re-checks the file and the
//!       stick, then writes the installer and reads it all back. --image
//!       writes a new file instead of a stick (tests and the rig). Progress
//!       on stderr; the result as JSON.
//!   settle-in --version

mod bootentry;
mod civil;
mod clock;
mod efi;
mod goback;
mod walkaway;
mod gpt;
mod hw;
mod sticks;
mod stickwrite;
mod ssh;
mod summary;
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

/// The public summary: readable by the person's window, holds no secret.
fn save_public(root: &str, v: &Value) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let dir = format!("{}/{}", root.trim_end_matches('/'), summary::PUBLIC_DIR);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {}", dir, e))?;
    let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755));
    let path = format!("{}/summary.json", dir);
    let tmp = format!("{}.part", path);
    let _ = std::fs::remove_file(&tmp);
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o644).open(&tmp).map_err(|e| format!("{}: {}", tmp, e))?;
    f.write_all((serde_json::to_string_pretty(v).map_err(|e| e.to_string())? + "\n").as_bytes()).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("{}: {}", path, e))
}

fn public_summary(root: &str) -> Result<Value, String> {
    read_json(&format!("{}/{}/summary.json", root.trim_end_matches('/'), summary::PUBLIC_DIR))
}

fn remove_old_boot_entry(root: &str) -> i32 {
    // SAFETY: geteuid has no preconditions.
    if root == "/" && unsafe { libc::geteuid() } != 0 {
        eprintln!("settle-in: removing a startup entry needs administrator rights (run it through pkexec)");
        return 1;
    }
    let handoff = format!("{}/{}", root.trim_end_matches('/'), HANDOFF);
    let job = match read_json(&format!("{}/job.json", handoff)) {
        Ok(j) if j.get("schema") == Some(&json!("job/1")) => j,
        Ok(_) | Err(_) => {
            println!("{}", json!({ "result": "refused", "why": "the job is not readable, so whether Windows was kept is not known" }));
            return 1;
        }
    };
    let out = bootentry::remove(root, &job);
    let report_path = format!("{}/settle-in/report.json", handoff);
    if let Ok(mut rep) = read_json(&report_path) {
        rep["old_boot_entry_removal"] = json!({ "at_utc": now_iso(), "outcome": out.clone() });
        let _ = save(&report_path, &rep);
    }
    if let Ok(mut s) = public_summary(root) {
        if out["result"] == "removed" || out["result"] == "nothing-to-remove" {
            s["old_boot_entry"] = json!({ "offered": false, "removed": out.get("removed").cloned().unwrap_or(json!([])) });
        } else {
            s["old_boot_entry"]["last_attempt"] = out.clone();
        }
        let _ = save_public(root, &s);
    }
    println!("{}", out);
    if out["result"] == "removed" || out["result"] == "nothing-to-remove" { 0 } else { 1 }
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
            "virtual_machine": clock::is_virtual(root),
            "zone_rules": match &zone { Ok(_) => json!(iana), Err(e) => json!(format!("missing: {}", e)) },
        },
    });

    // --- the clock, first: before anything time-sensitive, before the network
    let clock_before = earlier.as_ref().and_then(|e| e.pointer("/clock/result")).and_then(Value::as_str).map(str::to_string);
    report["clock"] = if clock_before.as_deref() == Some("attempting") {
        // a crash or power cut in the middle: never correct twice
        json!({ "result": "left-alone", "why": "an earlier attempt was interrupted; the clock is not corrected twice" })
    } else {
        match clock::evidence(&job, &outcome, clock::is_virtual(root)) {
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
    // remote access, only if Windows had it on (decided 2026-10-04)
    report["ssh"] = ssh::run(root, &job);
    println!("settle-in: ssh: {} {}", report["ssh"]["result"], report["ssh"].get("why").map(|w| w.to_string()).unwrap_or_default());

    // --- our own one-time entry: removed automatically (the owner, 2026-09-27)
    report["own_boot_entry"] = bootentry::remove_ours(root, &outcome);
    println!("settle-in: own boot entry: {}", report["own_boot_entry"]["result"]);

    // --- the old boot entry: only looked at here; removing is the person's button
    report["old_boot_entry"] = bootentry::describe(&bootentry::plan(&bootentry::facts(root, &job)));
    println!("settle-in: old boot entry offered: {} {}", report["old_boot_entry"]["offered"], report["old_boot_entry"]["why_not"]);
    if let Err(e) = save_public(root, &summary::build(root, &report, &job)) {
        eprintln!("settle-in: the summary for the window could not be written: {}", e);
    }

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

fn go_back(args: &[String], root: &str) -> i32 {
    let opt = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    match args.first().map(String::as_str) {
        Some("screen") => {
            let before = goback::windows_before(public_summary(root).ok().as_ref());
            let pc = goback::this_computer(root);
            let off = goback::offer(&before, &pc);
            let mut s = goback::screen(&before, &pc, &off);
            if args.iter().any(|a| a == "--text") {
                print!("{}", summary::text(&s));
            } else {
                s["facts"] = json!({ "before": before, "this_computer": pc, "offer": off });
                println!("{}", s);
            }
            0
        }
        Some("downloads") => {
            let home = opt("--home").or_else(|| std::env::var("HOME").ok()).unwrap_or_default();
            println!("{}", Value::Array(goback::find_downloads(&home)));
            0
        }
        Some("check") => {
            let Some(file) = args.get(1).filter(|f| !f.starts_with("--")) else {
                eprintln!("usage: settle-in go-back check FILE [--want 10|11]");
                return 64;
            };
            let want = opt("--want");
            let j = goback::check_report(file, want.as_deref(), &goback::media());
            println!("{}", j);
            if j["result"] == "verified" { 0 } else { 1 }
        }
        Some("write") => {
            let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_string_lossy().to_string())).unwrap_or_default();
            let req = stickwrite::Request {
                tree: None,
                iso: opt("--iso").unwrap_or_default(),
                want: opt("--want"),
                serial: opt("--serial").unwrap_or_default(),
                size_bytes: opt("--size").and_then(|v| v.parse().ok()).unwrap_or(0),
                typed: opt("--typed").unwrap_or_default(),
                image: opt("--image"),
                wimlib: opt("--wimlib").unwrap_or_else(|| format!("{}/wimlib-imagex", exe_dir)),
                root: root.to_string(),
            };
            let started = now_iso();
            let r = stickwrite::write(&req);
            // every real write leaves a record (rule #5): what was asked, every disk
            // as seen afterwards, and the result. Root-only; a record that cannot be
            // saved is said, never fatal (the stick is already what the result says)
            // SAFETY: geteuid has no preconditions.
            if unsafe { libc::geteuid() } == 0 {
                let dir = format!("{}/var/lib/upgrade_-go-back", root.trim_end_matches('/'));
                let rec = json!({
                    "schema": "go-back-write/1", "settle_in_version": VERSION, "started_utc": started, "finished_utc": now_iso(),
                    "request": { "iso": req.iso, "want": req.want, "serial": req.serial, "size_bytes": req.size_bytes, "typed": req.typed, "image": req.image },
                    "disks_after": sticks::collect(root), "result": r,
                });
                let path = format!("{}/stick-{}.json", dir, started.replace(':', ""));
                let saved = std::fs::create_dir_all(&dir)
                    .and_then(|_| std::fs::set_permissions(&dir, std::os::unix::fs::PermissionsExt::from_mode(0o700)))
                    .map_err(|e| format!("{}: {}", dir, e))
                    .and_then(|_| save(&path, &rec));
                match saved {
                    Ok(()) => eprintln!("{}", json!({ "record": path })),
                    Err(e) => eprintln!("{}", json!({ "record_not_saved": e })),
                }
            }
            println!("{}", r);
            if r["result"] == "written" { 0 } else { 1 }
        }
        Some("walkaway") => walkaway_cmd(&args[1..], root),
        Some("sticks") => {
            let facts = sticks::collect(root);
            if args.iter().any(|a| a == "--facts") {
                println!("{}", serde_json::to_string_pretty(&facts).unwrap_or_default());
            } else {
                let min = opt("--min-bytes").and_then(|m| m.parse().ok()).unwrap_or(8_000_000_000u64);
                println!("{}", sticks::judge(&facts, min));
            }
            0
        }
        _ => {
            eprintln!("usage: settle-in go-back screen [--root DIR] [--text] | downloads [--home DIR] | check FILE [--want 10|11] | sticks [--root DIR] [--min-bytes N] [--facts]");
            64
        }
    }
}

/// "xx-yy" from LANG ("en_US.UTF-8" -> "en-us"), for Microsoft's catalog.
fn catalog_language(opt: Option<String>) -> String {
    let l = opt.or_else(|| std::env::var("LANG").ok()).unwrap_or_default();
    let l = l.split('.').next().unwrap_or("").replace('_', "-").to_ascii_lowercase();
    if l.len() == 5 { l } else { "en-us".into() }
}

/// The walk-away way back (RISKS R33): `plan` (read-only) and `prepare` (root).
fn walkaway_cmd(args: &[String], root: &str) -> i32 {
    let opt = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let before = goback::windows_before(public_summary(root).ok().as_ref());
    let pc = goback::this_computer(root);
    let off = goback::offer(&before, &pc);
    let edition = walkaway::catalog_edition(&before);
    let language = catalog_language(opt("--language"));
    let facts = sticks::collect(root);
    let drives = walkaway::drives(root, &facts);
    match args.first().map(String::as_str) {
        Some("plan") => {
            let mut plan = json!({
                "before": before, "this_computer": pc, "offer": off, "edition": edition, "language": language,
                "drives": match &drives { Ok(d) => json!(d), Err(e) => json!({ "refused": e }) },
                "sentence": walkaway::SENTENCE,
            });
            plan["words"] = walkaway::words(&plan);
            println!("{}", plan);
            0
        }
        Some("undo") => {
            // SAFETY: geteuid has no preconditions.
            if unsafe { libc::geteuid() } != 0 {
                println!("{}", json!({ "result": "stopped", "why": "undoing the one-time start needs administrator rights (run it through pkexec)" }));
                return 1;
            }
            match walkaway::undo() {
                Ok(v) => { println!("{}", v); 0 }
                Err(e) => { println!("{}", json!({ "result": "stopped", "why": e })); 1 }
            }
        }
        Some("prepare") => {
            let started = now_iso();
            let mut steps: Vec<Value> = Vec::new();
            let finish = |steps: &mut Vec<Value>, result: Value| -> i32 {
                let ok = result["result"] == "ready";
                // SAFETY: geteuid has no preconditions.
                if unsafe { libc::geteuid() } == 0 {
                    let dir = format!("{}/var/lib/upgrade_-go-back", root.trim_end_matches('/'));
                    let rec = json!({ "schema": "go-back-walkaway/1", "settle_in_version": VERSION, "started_utc": started, "finished_utc": now_iso(), "steps": steps, "result": result });
                    let _ = std::fs::create_dir_all(&dir);
                    let _ = std::fs::set_permissions(&dir, std::os::unix::fs::PermissionsExt::from_mode(0o700));
                    let _ = save(&format!("{}/walkaway-{}.json", dir, started.replace(':', "")), &rec);
                }
                println!("{}", result);
                if ok { 0 } else { 1 }
            };
            let stop = |steps: &mut Vec<Value>, step: &str, why: String| -> i32 {
                eprintln!("{}", json!({ "step": step, "ok": false, "why": why }));
                steps.push(json!({ "step": step, "ok": false, "why": why }));
                finish(steps, json!({ "result": "stopped", "stopped_at": step, "why": why, "computer_changed": false }))
            };
            macro_rules! ok {
                ($step:expr, $detail:expr) => {{
                    eprintln!("{}", json!({ "step": $step, "ok": true }));
                    steps.push(json!({ "step": $step, "ok": true, "detail": $detail }));
                }};
            }
            // each step says when it starts, so the window can name it and show
            // that it is working even where there is no percentage (2026-10-03)
            let begin = |step: &str| eprintln!("{}", json!({ "step": step, "start": true }));
            // SAFETY: geteuid has no preconditions.
            if unsafe { libc::geteuid() } != 0 {
                return stop(&mut steps, "rights", "preparing the way back needs administrator rights (run it through pkexec)".into());
            }
            // 1. the person's decision, exactly
            if opt("--sentence").as_deref() != Some(walkaway::SENTENCE) {
                return stop(&mut steps, "consent", "the sentence was not typed exactly".into());
            }
            let account = opt("--account").unwrap_or_default();
            if !walkaway::account_ok(&account) {
                return stop(&mut steps, "consent", format!("\"{}\" cannot be a Windows account name", account));
            }
            let Some(windows) = off["windows"].as_str().map(str::to_string) else {
                return stop(&mut steps, "which-windows", off["why"].as_str().unwrap_or("no Windows fits this computer").to_string());
            };
            ok!("consent", json!({ "windows": windows, "edition": edition, "language": language, "account": account }));
            // 2. the drives to erase, named now; the gate finds them again
            begin("drives");
            let drives = match drives {
                Ok(d) => d,
                Err(e) => return stop(&mut steps, "drives", e),
            };
            ok!("drives", json!(drives));
            // 3. room to work
            let cache = opt("--cache").unwrap_or_else(|| format!("{}/var/cache/upgrade_-go-back", root.trim_end_matches('/')));
            if let Err(e) = std::fs::create_dir_all(&cache) {
                return stop(&mut steps, "room", format!("{}: {}", cache, e));
            }
            let _ = std::fs::set_permissions(&cache, std::os::unix::fs::PermissionsExt::from_mode(0o700));
            begin("room");
            let free = walkaway::free_bytes(&cache);
            if free < 14_000_000_000 {
                return stop(&mut steps, "room", format!("this computer needs 14 GB free to prepare Windows; it has {:.1} GB", free as f64 / 1e9));
            }
            // 4. Microsoft's catalog, then its file
            let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_string_lossy().to_string())).unwrap_or_default();
            let tool = |n: &str| opt(&format!("--{}", n)).unwrap_or_else(|| format!("{}/{}", exe_dir, n));
            begin("catalog");
            let xml = match walkaway::fetch_catalog(&windows, &cache, &tool("cabextract")) {
                Ok(x) => x,
                Err(e) => return stop(&mut steps, "catalog", e),
            };
            let entry = match walkaway::pick(&xml, &language, edition).or_else(|_| walkaway::pick(&xml, "en-us", edition)) {
                Ok(e) => e,
                Err(e) => return stop(&mut steps, "catalog", e),
            };
            ok!("catalog", entry.clone());
            begin("download");
            let esd = match walkaway::download(&entry, &cache) {
                Ok(p) => p,
                Err(e) => return stop(&mut steps, "download", e),
            };
            ok!("download", json!({ "file": esd, "sha1": entry["sha1"], "size": entry["size"] }));
            // 5. the stick's files
            let tree = format!("{}/stick-{}", cache, started.replace(':', ""));
            begin("build");
            let built = match walkaway::build_tree(&esd, edition, &account, &tree, &tool("upgrade-gate.exe"), &tool("wimlib-imagex")) {
                Ok(b) => b,
                Err(e) => {
                    let _ = std::fs::remove_dir_all(&tree);
                    return stop(&mut steps, "build", e);
                }
            };
            // Wi-Fi comes along (decided 2026-10-02): profiles onto the stick, names into the job
            begin("wifi");
            let wifi = match walkaway::carry_wifi(root, &tree) {
                Ok(w) => w,
                Err(e) => {
                    let _ = std::fs::remove_dir_all(&tree);
                    return stop(&mut steps, "wifi", e);
                }
            };
            ok!("wifi", json!(wifi));
            let edition_name = goback::edition_name(&before, Some(&windows)).unwrap_or_else(|| format!("Windows {}", windows));
            let job_id = format!("go-back-{}", started.replace([':', '-'], ""));
            let job = walkaway::job(&job_id, &started, walkaway::SENTENCE, &windows, edition, &edition_name, &entry["language"].as_str().unwrap_or("en-us").to_string(), &account, &drives, &entry, &wifi);
            let job_path = format!("{}/upgrade_/go-back.json", tree);
            if let Err(e) = std::fs::write(&job_path, serde_json::to_string_pretty(&job).unwrap_or_default()) {
                let _ = std::fs::remove_dir_all(&tree);
                return stop(&mut steps, "build", format!("{}: {}", job_path, e));
            }
            ok!("build", json!({ "tree": tree, "built": built, "job_id": job_id }));
            // 6. the stick (R16's rules, in stickwrite)
            let req = stickwrite::Request {
                tree: Some(tree.clone()),
                iso: String::new(),
                want: Some(windows.clone()),
                serial: opt("--serial").unwrap_or_default(),
                size_bytes: opt("--size").and_then(|v| v.parse().ok()).unwrap_or(0),
                typed: opt("--typed").unwrap_or_default(),
                image: opt("--image"),
                wimlib: tool("wimlib-imagex"),
                root: root.to_string(),
            };
            begin("stick");
            let w = stickwrite::write(&req);
            let _ = std::fs::remove_dir_all(&tree);
            if w["result"] != "written" {
                steps.push(json!({ "step": "stick", "ok": false, "detail": w }));
                return finish(&mut steps, json!({ "result": "stopped", "stopped_at": "stick", "why": w["why"], "stick_changed": w["stick_changed"], "computer_changed": false }));
            }
            let stick_name = w["steps"].as_array().into_iter().flatten().find(|s| s["step"] == "find-stick").and_then(|s| s["detail"]["name"].as_str()).unwrap_or("").to_string();
            ok!("stick", json!({ "name": stick_name, "steps": w["steps"] }));
            // 7. start from the stick once, at the next restart
            if req.image.is_some() || args.iter().any(|a| a == "--no-boot-entry") {
                return finish(&mut steps, json!({ "result": "ready", "job_id": job_id, "boot_once": null, "note": "no boot entry: a test image, or asked not to" }));
            }
            begin("boot-once");
            let usb_disks = sticks::collect(root).as_array().map(|a| a.iter().filter(|d| d["usb"] == json!(true)).count()).unwrap_or(0);
            match walkaway::boot_once(&stick_name, usb_disks) {
                Ok(b) => {
                    ok!("boot-once", b.clone());
                    finish(&mut steps, json!({ "result": "ready", "job_id": job_id, "boot_once": b }))
                }
                Err(e) => stop(&mut steps, "boot-once", format!("the computer could not be told to start from the stick once ({}); the stick is ready, nothing on this computer changed", e)),
            }
        }
        _ => {
            eprintln!("usage: settle-in go-back walkaway plan [--language xx-yy] | prepare --sentence S --account NAME --serial S --size N --typed MODEL [--language xx-yy] [--cache DIR] [--image NEWFILE] [--no-boot-entry]");
            64
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opt = |name: &str, default: &str| {
        args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned().unwrap_or_else(|| default.to_string())
    };
    match args.first().map(String::as_str) {
        Some("--version") => println!("settle-in {}", VERSION),
        Some("first-start") => std::process::exit(first_start(&opt("--root", "/"), &opt("--rtc", "/dev/rtc0"))),
        Some("summary") => match public_summary(&opt("--root", "/")) {
            Ok(s) => {
                let sec = summary::sections(&s);
                if args.iter().any(|a| a == "--text") { print!("{}", summary::text(&sec)) } else { println!("{}", sec) }
            }
            Err(e) => {
                eprintln!("settle-in: no summary yet ({})", e);
                std::process::exit(1);
            }
        },
        Some("go-back") => std::process::exit(go_back(&args[1..], &opt("--root", "/"))),
        Some("remove-old-boot-entry") => std::process::exit(remove_old_boot_entry(&opt("--root", "/"))),
        _ => {
            eprintln!("usage: settle-in first-start [--root DIR] [--rtc DEVICE] | summary [--root DIR] [--text] | remove-old-boot-entry [--root DIR] | --version");
            std::process::exit(64);
        }
    }
}
