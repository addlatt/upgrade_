//! upgrade-job (Rust): the job writer's program, following
//! `evaluate/windows/New-Job.ps1` and `evaluate/windows/Read-Password.ps1`.
//!
//!   upgrade-job password --out <file> --linux-name <name>
//!       ask for the new account's password at the console (hidden, twice),
//!       write its SHA-512 crypt hash to <file>; the password itself is
//!       never written. Exit 0 when set, 1 when not.
//!
//! The launchers run this directly, never through a logger: nothing typed
//! here is logged.

use std::process::ExitCode;
use upgrade_job::password;

fn usage() -> ExitCode {
    eprintln!("upgrade-job {} (Rust; follows New-Job.ps1 {}, Read-Password.ps1 {})", env!("CARGO_PKG_VERSION"), upgrade_job::FOLLOWS_JOB_WRITER, password::FOLLOWS_HASHER);
    eprintln!("usage: upgrade-job password --out <file> --linux-name <name>");
    ExitCode::from(2)
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
            ExitCode::from(password::ask_and_write(std::path::Path::new(&out), &name) as u8)
        }
        _ => usage(),
    }
}
