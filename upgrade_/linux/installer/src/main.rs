//! upgrade-installer: the installer's two steps, in Rust, on the stick as
//! upgrade_/upgrade-installer (static; RISKS R32, roadmap item 19).
//!
//!   upgrade-installer verify  <job.json> [<stick label>]   the %pre refusal gate (was verify.sh 0.6.0)
//!   upgrade-installer outcome <job.json>                   the %post: the installed system's last settings (the chroot %post's
//!                                                          shell until 0.1.3), then the checklist and adapter (was outcome.sh 0.6.1)
//!
//! Both are started by the kickstart upgrade-kickstart writes, run inside
//! Anaconda's stage 2, and leave everything they learn on the stick under
//! upgrade_/report/. Two environment variables shorten the waits for the
//! parity test only: UPG_TEST_COUNTDOWN and UPG_TEST_REFUSAL_SECS.

mod outcome;
mod post;
mod tty;
mod util;
mod verify;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let version = format!("upgrade-installer {} (Rust; follows verify.sh {} and outcome.sh {})", env!("CARGO_PKG_VERSION"), verify::FOLLOWS, outcome::FOLLOWS);
    let code = match args.first().map(String::as_str) {
        Some("--version") => {
            println!("{version}");
            0
        }
        Some("verify") if args.len() >= 2 => verify::main(&args[1], args.get(2).map(String::as_str).unwrap_or("UPGV0")),
        Some("outcome") if args.len() >= 2 => outcome::main(&args[1]),
        _ => {
            eprintln!("{version}");
            eprintln!("usage: upgrade-installer verify <job.json> [<stick label>]");
            eprintln!("       upgrade-installer outcome <job.json>");
            2
        }
    };
    std::process::exit(code);
}
