//! `upgrade-kickstart`: the generator as a program, for the rig and for any
//! host that holds a `job.json` (the product's job writer calls the library
//! directly). Same bytes as the library, nothing else.
//!
//!   upgrade-kickstart --job job.json --out ks.cfg --label UPGV0 --manifest SHA256SUMS
//!
//! A refusal prints its words and exits 2; nothing is written then.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opt = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let (Some(job), Some(out), Some(label), Some(manifest)) = (opt("--job"), opt("--out"), opt("--label"), opt("--manifest")) else {
        eprintln!("upgrade-kickstart {} (Rust; follows New-Kickstart.ps1 {})", env!("CARGO_PKG_VERSION"), upgrade_kickstart::FOLLOWS_GENERATOR);
        eprintln!("usage: upgrade-kickstart --job <job.json> --out <ks.cfg> --label <stick label> --manifest <SHA256SUMS>");
        return ExitCode::from(2);
    };
    let job: serde_json::Value = match std::fs::read_to_string(&job).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).map_err(|e| e.to_string())) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("upgrade-kickstart: {job}: {e}");
            return ExitCode::from(2);
        }
    };
    let lines: Vec<String> = match std::fs::read_to_string(&manifest) {
        Ok(t) => t.lines().map(str::to_string).collect(),
        Err(e) => {
            eprintln!("upgrade-kickstart: {manifest}: {e}");
            return ExitCode::from(2);
        }
    };
    match upgrade_kickstart::kickstart(&job, &label, &lines) {
        Ok(text) => {
            if let Err(e) = std::fs::write(&out, text) {
                eprintln!("upgrade-kickstart: {out}: {e}");
                return ExitCode::from(2);
            }
            println!("{out}");
            ExitCode::SUCCESS
        }
        Err(why) => {
            eprintln!("upgrade-kickstart: REFUSED: {why}");
            ExitCode::from(2)
        }
    }
}
