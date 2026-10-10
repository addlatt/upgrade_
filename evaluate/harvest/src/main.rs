//! upgrade-harvest (Rust): the harvester's program, following
//! `evaluate/windows/Harvest-UpgradeState.ps1`.
//!
//!   upgrade-harvest folder-map --out <file> [--stick <drive>] [--materialize] [--timeout <seconds>]
//!       the folder map the job writer reads (what -FolderMapOut writes):
//!       the user folders with their sizes, their cloud placeholders, whose
//!       folders they are and, with --stick, whether they fit the stick.
//!       Exit 3 when the cloud step refused, as the script does.
//!   upgrade-harvest browsers [--skip-sizes]
//!       the browser profiles found, as JSON (Get-HarvestBrowsers)
//!   upgrade-harvest compare-maps <rust.json> <powershell.json>
//!       two folder maps of the same machine, field for field
//!
//! Read-only, except --materialize, which no launcher passes.

use serde_json::Value;
use std::process::ExitCode;

fn usage() -> ExitCode {
    eprintln!("upgrade-harvest {} (Rust; follows Harvest-UpgradeState.ps1 {})", env!("CARGO_PKG_VERSION"), upgrade_harvest::FOLLOWS_HARVESTER);
    eprintln!("usage: upgrade-harvest folder-map --out <file> [--stick <drive>] [--materialize] [--timeout <seconds>]");
    eprintln!("       upgrade-harvest browsers [--skip-sizes]");
    eprintln!("       upgrade-harvest compare-maps <rust.json> <powershell.json>");
    ExitCode::from(2)
}

#[cfg(windows)]
fn folder_map(args: &[String]) -> ExitCode {
    let value_of = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    let Some(out) = value_of("--out") else {
        eprintln!("give --out <file> - where the folder map goes");
        return ExitCode::from(2);
    };
    let timeout = value_of("--timeout").and_then(|t| t.parse().ok()).unwrap_or(600);
    let (_, now) = upgrade_scan::collect::now();
    let (map, refused) = upgrade_harvest::live::folder_map(value_of("--stick").as_deref(), args.iter().any(|a| a == "--materialize"), timeout, &now);
    if let Err(e) = std::fs::write(&out, serde_json::to_string_pretty(&map).unwrap_or_default() + "\n") {
        eprintln!("upgrade-harvest: cannot write {out}: {e}");
        return ExitCode::from(2);
    }
    if refused { ExitCode::from(3) } else { ExitCode::SUCCESS }
}

#[cfg(windows)]
fn browsers(args: &[String]) -> ExitCode {
    let found = upgrade_harvest::live::browsers(args.iter().any(|a| a == "--skip-sizes"));
    println!("{}", serde_json::to_string_pretty(&found).unwrap_or_default());
    ExitCode::SUCCESS
}

#[cfg(not(windows))]
fn folder_map(_args: &[String]) -> ExitCode {
    eprintln!("upgrade-harvest: the folder map is read on Windows");
    ExitCode::from(2)
}

#[cfg(not(windows))]
fn browsers(_args: &[String]) -> ExitCode {
    eprintln!("upgrade-harvest: the browsers are read on Windows");
    ExitCode::from(2)
}

fn compare_maps(rust_path: &str, ps_path: &str) -> ExitCode {
    let load = |p: &str| -> Result<Value, String> { serde_json::from_str(std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"))?.trim_start_matches('\u{feff}')).map_err(|e| format!("{p}: {e}")) };
    let (rust, ps) = match (load(rust_path), load(ps_path)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("upgrade-harvest: {e}");
            return ExitCode::from(2);
        }
    };
    println!();
    println!("  Rust:       {rust_path}   ({})", rust["HarvestedUtc"].as_str().unwrap_or(""));
    println!("  PowerShell: {ps_path}   ({})", ps["HarvestedUtc"].as_str().unwrap_or(""));
    println!();
    let (differences, drifted) = upgrade_harvest::compare::map_differences(&rust, &ps);
    for d in &drifted {
        println!("  drifted (allowed)  {d}");
    }
    if differences.is_empty() {
        println!("  SAME: the owner, {} folders, the cloud files, the stick and the fit, field for field.", rust["UserFolders"].as_array().map_or(0, Vec::len));
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
    if args.iter().any(|a| a == "--version") {
        println!("upgrade-harvest {} (Rust; follows Harvest-UpgradeState.ps1 {})", env!("CARGO_PKG_VERSION"), upgrade_harvest::FOLLOWS_HARVESTER);
        return ExitCode::SUCCESS;
    }
    match args.first().map(String::as_str) {
        Some("folder-map") => folder_map(&args[1..]),
        Some("browsers") => browsers(&args[1..]),
        Some("compare-maps") => {
            let (Some(a), Some(b)) = (args.get(1), args.get(2)) else { return usage() };
            compare_maps(a, b)
        }
        _ => usage(),
    }
}
