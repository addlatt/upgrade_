//! upgrade-prologue (Rust): the converter's Windows half, following
//! `upgrade_/windows/Invoke-Prologue.ps1`.
//!
//!   upgrade-prologue start --stick <X:> [--job <path>] [--confirm-word CONVERT]
//!                          [--acknowledge-data-loss "<sentence>"] [--erase-consent "<sentence>"]
//!                          [--state-dir <dir>]
//!   upgrade-prologue resume [--state-dir <dir>]       (the SYSTEM startup task, or by hand)
//!   upgrade-prologue notify [--state-dir <dir>]       (RunOnce at sign-in, as the person)
//!   upgrade-prologue abort  [--state-dir <dir>]
//!   upgrade-prologue probe --stick <X:> [--state-dir <dir>]
//!   upgrade-prologue rollback --stick <X:>           (Windows first again; Invoke-Rollback.ps1)
//!   upgrade-prologue facts --stick <X:> [--out <file>]   (read-only: what Get-PrologueFacts reads, for the side-by-side)
//!   upgrade-prologue compare-facts <rust.json> <powershell.json>
//!
//! Every tool it calls is kept with its raw output (`tools.jsonl` in the
//! state directory and in `upgrade_/report/` on the stick). Nothing here
//! crosses the commit line; every refusal happens before the step it guards
//! touches anything it cannot undo.

use std::process::ExitCode;

fn usage() -> ExitCode {
    eprintln!("{}", upgrade_prologue::flow_version());
    eprintln!("usage: upgrade-prologue start --stick <X:> [--job <path>] [--confirm-word CONVERT] [--acknowledge-data-loss <s>] [--erase-consent <s>] [--state-dir <dir>]");
    eprintln!("       upgrade-prologue resume|notify|abort [--state-dir <dir>]");
    eprintln!("       upgrade-prologue probe --stick <X:> [--state-dir <dir>]");
    eprintln!("       upgrade-prologue rollback --stick <X:>");
    eprintln!("       upgrade-prologue facts --stick <X:> [--out <file>]");
    eprintln!("       upgrade-prologue compare-facts <rust.json> <powershell.json>");
    ExitCode::from(2)
}

#[cfg(windows)]
fn run(args: &[String]) -> ExitCode {
    use upgrade_prologue::flow::{self, Ctx, StartArgs};
    use upgrade_prologue::live;
    let value_of = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    let state_dir = value_of("--state-dir").unwrap_or_else(|| format!("{}\\upgrade_\\prologue", std::env::var("ProgramData").unwrap_or_else(|_| "C:\\ProgramData".into())));
    let mode = args.first().map(String::as_str).unwrap_or("");
    if mode == "facts" {
        let Some(d) = value_of("--stick") else {
            eprintln!("give --stick <X:>");
            return ExitCode::from(2);
        };
        let mut rec = upgrade_prologue::tools::Recorder::new(None);
        let root = match upgrade_prologue::judge::drive_root(&d) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("  {e}");
                return ExitCode::from(1);
            }
        };
        let f = match live::facts(&mut rec, &root) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("  {e}");
                return ExitCode::from(1);
            }
        };
        let (_, now) = upgrade_scan::collect::now();
        let doc = serde_json::json!({"Recorder": upgrade_prologue::flow_version(), "Now": now, "Facts": f, "Tools": rec.runs.iter().map(|r| r.to_json()).collect::<Vec<_>>()});
        let out = value_of("--out").unwrap_or_else(|| std::env::temp_dir().join(format!("upgrade-report-prologuefacts-rust-{}.json", &now[..16].replace([':', '-'], ""))).to_string_lossy().into_owned());
        if let Err(e) = std::fs::write(&out, serde_json::to_string_pretty(&doc).unwrap_or_default() + "\n") {
            eprintln!("cannot write {out}: {e}");
            return ExitCode::from(2);
        }
        println!("  prologue facts written: {out}");
        return ExitCode::SUCCESS;
    }
    if mode == "notify" {
        flow::notify(&state_dir);
        return ExitCode::SUCCESS;
    }
    let mut ctx = Ctx::new(&state_dir, mode == "start");
    if !live::is_elevated() {
        eprintln!("the prologue needs Administrator: it reads and changes the disk and the boot configuration");
        return ExitCode::from(1);
    }
    if !live::is_uefi(&mut ctx.rec) {
        eprintln!("this machine is not UEFI-booted; the boot handoff does not apply");
        return ExitCode::from(1);
    }
    let r = match mode {
        "rollback" => match value_of("--stick") {
            Some(d) => match upgrade_prologue::rollback::live::rollback(&mut ctx.rec, &d, &upgrade_prologue::flow_version()) {
                Ok(code) => return ExitCode::from(code as u8),
                Err(e) => Err(e),
            },
            None => Err("give --stick <X:>".into()),
        },
        "abort" => {
            flow::abort(&mut ctx);
            Ok(())
        }
        "probe" => match value_of("--stick") {
            Some(d) => flow::probe_start(&mut ctx, &d),
            None => Err("give --stick <X:>".into()),
        },
        "start" => match value_of("--stick") {
            Some(d) => {
                let a = StartArgs { stick_drive: d.clone(), job_path: value_of("--job"), confirm_word: value_of("--confirm-word").unwrap_or_default(), acknowledge_data_loss: value_of("--acknowledge-data-loss").unwrap_or_default(), erase_consent: value_of("--erase-consent").unwrap_or_default() };
                let r = flow::start(&mut ctx, &a);
                if r.is_err() {
                    // a refusal before any state exists is a stop too: the Wi-Fi passwords leave the stick (2026-09-27)
                    if let Ok(root) = upgrade_prologue::judge::drive_root(&d) {
                        live::remove_wifi_secrets(&root);
                    }
                }
                r
            }
            None => Err("give --stick <X:>".into()),
        },
        "resume" => flow::resume(&mut ctx),
        _ => return usage(),
    };
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("  {e}");
            ExitCode::from(1)
        }
    }
}

#[cfg(not(windows))]
fn run(_args: &[String]) -> ExitCode {
    eprintln!("upgrade-prologue: the prologue runs on Windows");
    ExitCode::from(2)
}

fn compare_facts(rust_path: &str, ps_path: &str) -> ExitCode {
    let load = |p: &str| -> Result<serde_json::Value, String> { serde_json::from_str(std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"))?.trim_start_matches('\u{feff}')).map_err(|e| format!("{p}: {e}")) };
    let (rust, ps) = match (load(rust_path), load(ps_path)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("upgrade-prologue: {e}");
            return ExitCode::from(2);
        }
    };
    println!();
    println!("  Rust:       {rust_path}   ({})", rust["Now"].as_str().unwrap_or(""));
    println!("  PowerShell: {ps_path}   ({})", ps["Now"].as_str().unwrap_or(""));
    println!();
    let (differences, drifted) = upgrade_prologue::compare::differences(&rust["Facts"], &ps["Facts"]);
    for d in &drifted {
        println!("  drifted (allowed)  {d}");
    }
    if differences.is_empty() {
        println!("  SAME: every fact the prologue re-validates and decides from, field for field.");
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

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("compare-facts") {
        let (Some(a), Some(b)) = (args.get(1), args.get(2)) else { return usage() };
        return compare_facts(a, b);
    }
    if args.iter().any(|a| a == "--version") {
        println!("{}", upgrade_prologue::flow_version());
        return ExitCode::SUCCESS;
    }
    if args.is_empty() {
        return usage();
    }
    run(&args)
}
