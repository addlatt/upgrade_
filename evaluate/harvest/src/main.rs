//! upgrade-harvest (Rust): the harvester's program, following
//! `evaluate/windows/Harvest-UpgradeState.ps1`.
//!
//!   upgrade-harvest folder-map --out <file>
//!       the folder map the job writer reads (what -FolderMapOut writes):
//!       the user folders with their sizes, their cloud placeholders and
//!       whose folders they are.
//!
//! Read-only. Nothing is downloaded and the stick is not read (the
//! --materialize and --stick options left on 2026-11-26).

use std::process::ExitCode;

fn usage() -> ExitCode {
    eprintln!("upgrade-harvest {} (Rust; follows Harvest-UpgradeState.ps1 {})", env!("CARGO_PKG_VERSION"), upgrade_harvest::FOLLOWS_HARVESTER);
    eprintln!("usage: upgrade-harvest folder-map --out <file>");
    ExitCode::from(2)
}

#[cfg(windows)]
fn folder_map(args: &[String]) -> ExitCode {
    let value_of = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    let Some(out) = value_of("--out") else {
        eprintln!("give --out <file> - where the folder map goes");
        return ExitCode::from(2);
    };
    let (_, now) = upgrade_scan::collect::now();
    let (map, refused) = upgrade_harvest::live::folder_map(&now);
    if let Err(e) = std::fs::write(&out, serde_json::to_string_pretty(&map).unwrap_or_default() + "\n") {
        eprintln!("upgrade-harvest: cannot write {out}: {e}");
        return ExitCode::from(2);
    }
    if refused { ExitCode::from(3) } else { ExitCode::SUCCESS }
}

#[cfg(not(windows))]
fn folder_map(_args: &[String]) -> ExitCode {
    eprintln!("upgrade-harvest: the folder map is read on Windows");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    // no idle sleep while this program works (roadmap item 16, R33); ends with the process
    let _ = upgrade_scan::awake::hold();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version") {
        println!("upgrade-harvest {} (Rust; follows Harvest-UpgradeState.ps1 {})", env!("CARGO_PKG_VERSION"), upgrade_harvest::FOLLOWS_HARVESTER);
        return ExitCode::SUCCESS;
    }
    match args.first().map(String::as_str) {
        Some("folder-map") => folder_map(&args[1..]),
        _ => usage(),
    }
}
