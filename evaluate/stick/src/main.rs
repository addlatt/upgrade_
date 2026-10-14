//! upgrade-stick (Rust): the stick writer, following Write-UpgradeStick.ps1
//! 0.1.1 and Copy-Kit.ps1 (RISKS R16, R32).
//!
//!   upgrade-stick list
//!   upgrade-stick plan  --target <unique id> (--expected-bytes N | --expected-gb X) [--results-csv <file>]
//!   upgrade-stick write --target <unique id> (--expected-bytes N | --expected-gb X) --source <dist/kit/stick>
//!                       [--label UPGRADE] [--boot-size-gb 4] [--confirmed-label <word>] [--results-csv <file>]
//!   upgrade-stick copy  --drive <X:> [--kit <dist/kit/stick>]
//!
//! Never a disk number: --target is the disk's UniqueId from `list`. plan
//! is read-only and records a row; write refuses by default, re-resolves
//! the disk by its id right before the first destructive call, and reads
//! every file back against SHA256SUMS. Every plan and write appends a row
//! to docs/validation-results/r16-stick-writer.csv (or --results-csv).

use std::process::ExitCode;

fn usage() -> ExitCode {
    eprintln!("upgrade-stick {} (Rust; follows Write-UpgradeStick.ps1 {})", upgrade_stick::VERSION, upgrade_stick::FOLLOWS_WRITER);
    eprintln!("usage: upgrade-stick list");
    eprintln!("       upgrade-stick plan  --target <unique id> (--expected-bytes N | --expected-gb X) [--results-csv <file>]");
    eprintln!("       upgrade-stick write --target <unique id> (--expected-bytes N | --expected-gb X) --source <dir> [--label UPGRADE] [--boot-size-gb 4] [--confirmed-label <word>] [--results-csv <file>]");
    eprintln!("       upgrade-stick copy  --drive <X:> [--kit <dir>]");
    ExitCode::from(2)
}

#[cfg(windows)]
fn run(args: &[String]) -> ExitCode {
    use upgrade_stick::flow::{self, Args};
    use upgrade_stick::live::WinLive;
    let value_of = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    let phase = args.first().map(String::as_str).unwrap_or("");
    if phase == "copy" {
        let Some(drive) = value_of("--drive") else {
            eprintln!("give --drive <X:>");
            return ExitCode::from(2);
        };
        return match upgrade_stick::live::copy_kit(&drive, value_of("--kit").as_deref()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("  {e}");
                ExitCode::from(1)
            }
        };
    }
    if !["list", "plan", "write"].contains(&phase) {
        return usage();
    }
    let a = Args {
        phase: phase.to_string(),
        target: value_of("--target").unwrap_or_default(),
        expected_size_bytes: value_of("--expected-bytes").and_then(|v| v.parse().ok()).unwrap_or(0),
        expected_size_gb: value_of("--expected-gb").and_then(|v| v.parse().ok()).unwrap_or(0.0),
        source: value_of("--source").unwrap_or_default(),
        label: value_of("--label").unwrap_or_else(|| "UPGRADE".into()),
        boot_size_gb: value_of("--boot-size-gb").and_then(|v| v.parse().ok()).unwrap_or(4),
        confirmed_label: value_of("--confirmed-label"),
    };
    if phase != "list" && a.target.is_empty() {
        eprintln!("give --target <unique id> (from `upgrade-stick list`)");
        return ExitCode::from(2);
    }
    if phase == "write" && a.source.is_empty() {
        eprintln!("give --source <dir> (dist/kit/stick from make-kit.sh)");
        return ExitCode::from(2);
    }
    let mut live = WinLive::new(value_of("--results-csv"));
    match flow::run(&mut live, &a) {
        Ok(code) => ExitCode::from(code as u8),
        Err(e) => {
            eprintln!("  {e}");
            ExitCode::from(1)
        }
    }
}

#[cfg(not(windows))]
fn run(_args: &[String]) -> ExitCode {
    eprintln!("upgrade-stick: the stick writer runs on Windows");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version") {
        println!("upgrade-stick {} (Rust; follows Write-UpgradeStick.ps1 {})", upgrade_stick::VERSION, upgrade_stick::FOLLOWS_WRITER);
        return ExitCode::SUCCESS;
    }
    if args.is_empty() {
        return usage();
    }
    run(&args)
}
