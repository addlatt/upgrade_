//! upgrade-job (Rust): the job writer's program, following
//! `evaluate/windows/New-Job.ps1` and `evaluate/windows/Read-Password.ps1`.
//!
//!   upgrade-job write --stick <X:> --out <stick>\upgrade_ --scan <reports dir>
//!                     --desktop kde|gnome --start-at desktop|console
//!                     [--password-hash-file <file>] [--if-cannot-keep stop|clean-slate]
//!                     [--acknowledge-data-loss "<sentence>"] [--erase-everything "<sentence>"]
//!                     [--verify-only] [--materialize]
//!                     [--kickstart <file> --stick-label <label> --manifest <SHA256SUMS>]
//!       read this machine, judge, write job.json (only as a document that
//!       passed the contract), the Wi-Fi password files and, when asked,
//!       ks.cfg. Refusals print "REFUSED - no job written:" and exit 2.
//!   upgrade-job password --out <file> --linux-name <name>
//!       the new account's password, asked twice at the console, hidden;
//!       only its SHA-512 crypt hash is written. Exit 0 when set, 1 when not.
//!   upgrade-job linux-name
//!       the Linux sign-in name for the Windows account running it
//!   upgrade-job harvest-settings --out <file> --out-dir <dir>
//!       only the clock, Wi-Fi, SSH and licence harvest (for the rig)
//!   upgrade-job facts [--scan <dir>] [--stick <X:>] [--out <file>]
//!       what the writer reads, as JSON, for the side-by-side comparison
//!   upgrade-job compare-facts <rust.json> <powershell.json>
//!
//! The password prompt is never run through a logger: nothing typed there
//! is logged.

#[cfg_attr(not(windows), allow(unused_imports))]
use serde_json::{json, Value};
#[cfg_attr(not(windows), allow(unused_imports))]
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use upgrade_job::{maps, password};

fn usage() -> ExitCode {
    eprintln!("upgrade-job {} (Rust; follows New-Job.ps1 {}, Read-Password.ps1 {})", env!("CARGO_PKG_VERSION"), upgrade_job::FOLLOWS_JOB_WRITER, password::FOLLOWS_HASHER);
    eprintln!("usage: upgrade-job write --stick <X:> --out <dir> --scan <dir> --desktop kde|gnome --start-at desktop|console [...]");
    eprintln!("       upgrade-job password --out <file> --linux-name <name>");
    eprintln!("       upgrade-job linux-name");
    eprintln!("       upgrade-job harvest-settings --out <file> --out-dir <dir>");
    eprintln!("       upgrade-job facts [--scan <dir>] [--stick <X:>] [--out <file>]");
    eprintln!("       upgrade-job compare-facts <rust.json> <powershell.json>");
    ExitCode::from(2)
}

#[cfg_attr(not(windows), allow(dead_code))]
fn refused(lines: &[String]) -> ExitCode {
    println!();
    println!("  REFUSED - no job written:");
    for l in lines {
        println!("    - {l}");
    }
    println!();
    ExitCode::from(2)
}

fn load_json(p: &str) -> Result<Value, String> {
    serde_json::from_str(std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"))?.trim_start_matches('\u{feff}')).map_err(|e| format!("{p}: {e}"))
}

fn compare_facts(rust_path: &str, ps_path: &str) -> ExitCode {
    let (rust, ps) = match (load_json(rust_path), load_json(ps_path)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("upgrade-job: {e}");
            return ExitCode::from(2);
        }
    };
    println!();
    println!("  Rust:       {rust_path}   ({})", rust["Now"].as_str().unwrap_or(""));
    println!("  PowerShell: {ps_path}   ({})", ps["Now"].as_str().unwrap_or(""));
    println!();
    let (mut differences, mut drifted) = upgrade_job::compare::facts_differences(&rust["Facts"], &ps["Facts"]);
    let (d2, r2) = upgrade_job::compare::facts_differences(&rust["Wifi"], &ps["Wifi"]);
    differences.extend(d2.into_iter().map(|d| format!("Wifi.{d}")));
    drifted.extend(r2);
    for d in &drifted {
        println!("  drifted (allowed)  {d}");
    }
    if differences.is_empty() {
        println!("  SAME: every fact the job is written from, and the Wi-Fi block, field for field.");
        println!();
        return ExitCode::SUCCESS;
    }
    println!("  DIFFERENT in {} places:", differences.len());
    for d in &differences {
        println!("  {d}");
    }
    println!();
    ExitCode::from(1)
}

#[cfg(windows)]
mod on_windows {
    use super::*;
    use upgrade_job::document::{job_document, Choices, Stamp};
    use upgrade_job::live;

    pub fn facts(args: &[String]) -> ExitCode {
        let value_of = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
        let scan = value_of("--scan").map(PathBuf::from);
        let stick = value_of("--stick");
        let f = match live::job_facts(scan.as_deref(), stick.as_deref(), false) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("upgrade-job: {e}");
                return ExitCode::from(1);
            }
        };
        let wifi = match upgrade_job::wifi::harvest_wifi(&live::wlan_profiles(), live::wlan_stored_count(), upgrade_job::wifi::WIFI_DIR) {
            Ok((block, files)) => json!({"Refusal": null, "Wifi": block, "FileCount": files.len()}),
            Err(why) => json!({"Refusal": why}),
        };
        let (_, now) = upgrade_scan::collect::now();
        let doc = json!({"Recorder": format!("upgrade-job {} (Rust)", env!("CARGO_PKG_VERSION")), "Now": now, "Facts": f, "Wifi": wifi});
        let out = value_of("--out").unwrap_or_else(|| std::env::temp_dir().join(format!("upgrade-report-jobfacts-rust-{}.json", &now[..16].replace([':', '-'], ""))).to_string_lossy().into_owned());
        if let Err(e) = std::fs::write(&out, serde_json::to_string_pretty(&doc).unwrap_or_default() + "\n") {
            eprintln!("upgrade-job: cannot write {out}: {e}");
            return ExitCode::from(2);
        }
        println!("  job facts written: {out}");
        println!("  It holds this machine's program list and folder paths. Do not commit it.");
        ExitCode::SUCCESS
    }

    pub fn harvest_settings(args: &[String]) -> ExitCode {
        let value_of = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
        let (Some(out), Some(out_dir)) = (value_of("--out"), value_of("--out-dir")) else {
            eprintln!("give --out <file> and --out-dir <stick>\\upgrade_");
            return ExitCode::from(2);
        };
        let c = live::clock_facts();
        let zone = c["WindowsZone"].as_str().unwrap_or("").to_string();
        let iana = maps::iana_time_zone(&zone).unwrap_or("");
        let mut why = Vec::new();
        if iana.is_empty() {
            why.push(format!("Windows time zone '{zone}' has no IANA mapping in this version"));
        }
        let clock = upgrade_job::records::clock(&zone, iana, &c["RealTimeIsUniversal"], &c["DynamicDstDisabled"], c["OffsetMinutes"].as_i64().unwrap_or(0), c["BaseOffsetMinutes"].as_i64().unwrap_or(0), c["DstActive"].as_bool().unwrap_or(false), c["NowUtc"].as_str().unwrap_or(""));
        let clock = match clock {
            Ok(v) => v,
            Err(e) => {
                why.push(e);
                Value::Null
            }
        };
        let mut wifi = Value::Null;
        if why.is_empty() {
            match live::export_wifi(Path::new(&out_dir)) {
                Ok(w) => wifi = w,
                Err(e) => why.push(e),
            }
        }
        if !why.is_empty() {
            for x in &why {
                println!("  REFUSED: {x}");
            }
            return ExitCode::from(2);
        }
        let cimv2 = upgrade_scan::collect::wmi::Wmi::connect(upgrade_scan::collect::wmi::CIMV2).ok();
        let sf = live::ssh_facts(cimv2.as_ref());
        let ssh = upgrade_job::records::ssh(&sf["StartType"], &sf["KeyFiles"], sf["ReadError"].as_str().unwrap_or(""));
        let l = live::license_facts(cimv2.as_ref());
        let license = upgrade_job::records::license(&l["Os"], &l["Products"], &l["Firmware"], l["Error"].as_str().unwrap_or(""), l["NowUtc"].as_str().unwrap_or(""));
        let doc = json!({"job_writer": env!("CARGO_PKG_VERSION"), "clock": clock, "wifi": wifi, "ssh": ssh, "windows_license": license});
        if let Err(e) = std::fs::write(&out, serde_json::to_string_pretty(&doc).unwrap_or_default() + "\n") {
            eprintln!("upgrade-job: cannot write {out}: {e}");
            return ExitCode::from(2);
        }
        println!("  clock + Wi-Fi harvest written: {out} (Wi-Fi: {}, {} network(s))", doc["wifi"]["result"].as_str().unwrap_or(""), doc["wifi"]["profiles"].as_array().map_or(0, Vec::len));
        ExitCode::SUCCESS
    }

    pub fn write(args: &[String]) -> ExitCode {
        let value_of = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
        let has = |flag: &str| args.iter().any(|a| a == flag);
        let (Some(stick), Some(out_dir)) = (value_of("--stick"), value_of("--out")) else {
            eprintln!("give --stick X: --out <stick>\\upgrade_ --scan <reports dir>");
            return ExitCode::from(2);
        };
        let desktop = value_of("--desktop").unwrap_or_else(|| "kde".to_string());
        if desktop != "kde" && desktop != "gnome" {
            eprintln!("--desktop must be kde or gnome");
            return ExitCode::from(2);
        }
        let if_cannot_keep = value_of("--if-cannot-keep").unwrap_or_else(|| "stop".to_string());
        if if_cannot_keep != "stop" && if_cannot_keep != "clean-slate" {
            eprintln!("--if-cannot-keep must be stop or clean-slate");
            return ExitCode::from(2);
        }
        let verify_only = has("--verify-only");
        if !upgrade_scan::collect::win::is_admin() {
            eprintln!("the job writer needs Administrator: the shrink measurement, the volume flag, BitLocker and the ESP are elevated-only reads");
            return ExitCode::from(2);
        }
        println!();
        println!("  upgrade_  job writer {} (Rust; follows New-Job.ps1 {})", env!("CARGO_PKG_VERSION"), upgrade_job::FOLLOWS_JOB_WRITER);
        println!("  reads this machine; writes job.json; changes nothing");
        let hash_file = value_of("--password-hash-file");
        if !verify_only && hash_file.is_none() {
            println!();
            println!("  REFUSED - no job written: no password was chosen for the new account (a job that installs needs --password-hash-file; only a verify-only job may use the placeholder)");
            return ExitCode::from(2);
        }
        let start_at = match value_of("--start-at") {
            Some(s) if s == "desktop" || s == "console" => s,
            Some(_) => {
                eprintln!("--start-at must be desktop or console");
                return ExitCode::from(2);
            }
            None => {
                println!();
                println!("  REFUSED - no job written: no choice was made of what the computer starts at (--start-at desktop or console)");
                return ExitCode::from(2);
            }
        };
        let mut password_hash = upgrade_job::VERIFY_ONLY_HASH.to_string();
        if let Some(p) = &hash_file {
            match std::fs::read_to_string(p) {
                Ok(t) => password_hash = t.trim().to_string(),
                Err(_) => {
                    eprintln!("no password hash at {p}");
                    return ExitCode::from(1);
                }
            }
        }
        let scan_dir = value_of("--scan").map(PathBuf::from);
        let facts = match live::job_facts(scan_dir.as_deref(), Some(&stick), has("--materialize")) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("upgrade-job: {e}");
                return ExitCode::from(1);
            }
        };
        let report_rel = facts["Report"].as_str().and_then(|p| Path::new(p).file_name()).map(|n| format!("reports/{}", n.to_string_lossy())).unwrap_or_else(|| "reports/none".to_string());
        let job_id = match live::new_guid() {
            Ok(g) => g,
            Err(e) => {
                eprintln!("upgrade-job: {e}");
                return ExitCode::from(1);
            }
        };
        let now = live::now_utc_z();
        let ack = value_of("--acknowledge-data-loss").unwrap_or_default();
        let erase = value_of("--erase-everything").unwrap_or_default();
        let choice = Choices { desktop: &desktop, start_at: &start_at, password_hash: &password_hash, if_cannot_keep: &if_cannot_keep, report_rel: &report_rel, acknowledge_data_loss: &ack, erase_everything: &erase };
        let mut job = match job_document(&facts, &choice, &Stamp { job_id: &job_id, now_utc: &now, writer_version: env!("CARGO_PKG_VERSION") }) {
            Ok(j) => j,
            Err(refusals) => return refused(&refusals),
        };
        let out = PathBuf::from(&out_dir);
        if let Err(e) = std::fs::create_dir_all(&out) {
            eprintln!("upgrade-job: cannot make {}: {e}", out.display());
            return ExitCode::from(1);
        }
        if !verify_only {
            // the launcher showed the owner's approved Wi-Fi sentence before anything was typed (2026-09-27)
            match live::export_wifi(&out) {
                Ok(w) => job["harvest"]["wifi"] = w,
                Err(e) => return refused(&[e]),
            }
        }
        // never softer: the document is written only as one that passed the whole contract
        let checked = match upgrade_schema::Job::from_value(job) {
            Ok(j) => j,
            Err(e) => return refused(&[format!("the job does not pass the contract (schemas/job.schema.json): {e}")]),
        };
        let j = checked.as_value();
        let job_path = out.join("job.json");
        if let Err(e) = std::fs::write(&job_path, serde_json::to_string_pretty(j).unwrap_or_default() + "\n") {
            eprintln!("upgrade-job: cannot write {}: {e}", job_path.display());
            return ExitCode::from(1);
        }
        if j["harvest"]["bitlocker"]["status"] == "on" {
            let cred = out.join("artifacts").join("credentials");
            let _ = std::fs::create_dir_all(&cred);
            let _ = std::fs::write(cred.join("bitlocker-C.txt"), format!("NOT HARVESTED - this job writer ({}) does not extract the recovery key yet. The live-boot leg does not need it.\n", env!("CARGO_PKG_VERSION")));
        }
        for line in live::summary_lines(j, &facts, &job_path, verify_only) {
            println!("{line}");
        }
        if let Some(ks) = value_of("--kickstart") {
            let label = value_of("--stick-label").unwrap_or_else(|| "UPGV0".to_string());
            let manifest: Vec<String> = value_of("--manifest").and_then(|m| std::fs::read_to_string(m).ok()).map(|t| t.trim_start_matches('\u{feff}').lines().map(|l| l.trim_end_matches('\r').to_string()).collect()).unwrap_or_default();
            match upgrade_kickstart::kickstart_for(&checked, &label, &manifest) {
                Ok(text) => {
                    if let Err(e) = std::fs::write(&ks, text) {
                        eprintln!("upgrade-job: cannot write {ks}: {e}");
                        return ExitCode::from(1);
                    }
                    println!("  kickstart written: {ks}");
                }
                Err(e) => return refused(&[format!("no kickstart: {e}")]),
            }
        }
        ExitCode::SUCCESS
    }
}

#[cfg(not(windows))]
mod on_windows {
    use super::*;
    fn off(what: &str) -> ExitCode {
        eprintln!("upgrade-job: {what} runs on Windows");
        ExitCode::from(2)
    }
    pub fn facts(_: &[String]) -> ExitCode {
        off("reading the facts")
    }
    pub fn harvest_settings(_: &[String]) -> ExitCode {
        off("the harvest")
    }
    pub fn write(_: &[String]) -> ExitCode {
        off("the job writer")
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let value_of = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    if args.iter().any(|a| a == "--version") {
        println!("upgrade-job {} (Rust; follows New-Job.ps1 {}, Read-Password.ps1 {})", env!("CARGO_PKG_VERSION"), upgrade_job::FOLLOWS_JOB_WRITER, password::FOLLOWS_HASHER);
        return ExitCode::SUCCESS;
    }
    match args.first().map(String::as_str) {
        Some("password") => {
            let Some(out) = value_of("--out") else {
                eprintln!("give --out <file> - where the hash goes");
                return ExitCode::from(2);
            };
            let Some(name) = value_of("--linux-name") else {
                eprintln!("give --linux-name <the sign-in name> - the person is told which account this password is for (2026-09-26)");
                return ExitCode::from(2);
            };
            ExitCode::from(password::ask_and_write(Path::new(&out), &name) as u8)
        }
        Some("linux-name") => {
            println!("{}", maps::linux_name(&std::env::var("USERNAME").unwrap_or_default()));
            ExitCode::SUCCESS
        }
        Some("write") => on_windows::write(&args[1..]),
        Some("facts") => on_windows::facts(&args[1..]),
        Some("harvest-settings") => on_windows::harvest_settings(&args[1..]),
        Some("compare-facts") => {
            let (Some(a), Some(b)) = (args.get(1), args.get(2)) else { return usage() };
            compare_facts(a, b)
        }
        _ => usage(),
    }
}
