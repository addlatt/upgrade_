//! upgrade-fetch (Rust): the Linux the person picked, onto the stick this
//! program runs from (roadmap item 24).
//!
//!   upgrade-fetch status --release <id> --desktop <id>
//!   upgrade-fetch get    --release <id> --desktop <id> [--from <http(s) folder>]
//!   upgrade-fetch rehearse --release <id> --desktop <id> [--from <http(s) folder>]
//!
//! `rehearse` is the developer's test of the real network path (rule #5):
//! the whole `get` into one fixed folder, %TEMP%\upgrade_-fetch-rehearsal,
//! made to look like a kit stick. It takes no path and writes nowhere else.
//!
//! No argument names a drive: the stick is the drive this program sits at
//! the root of. `status` reads only. `get` refuses by default and says
//! REFUSED with the reason; exit 0 means the release is on the stick and
//! every file was checked. `--from` is a developer's copy of the same files
//! (the rig's local server); the table's sha256s still decide.

use std::process::ExitCode;
#[cfg(windows)]
use upgrade_fetch::get;
use upgrade_fetch::{plan, stick};

fn usage() -> ExitCode {
    eprintln!("upgrade-fetch {}", upgrade_fetch::VERSION);
    eprintln!("usage: upgrade-fetch status --release <id> --desktop <id>");
    eprintln!("       upgrade-fetch get    --release <id> --desktop <id> [--from <http(s) folder>]");
    eprintln!("       upgrade-fetch rehearse --release <id> --desktop <id> [--from <http(s) folder>]");
    ExitCode::from(2)
}

/// The drive this program sits at the root of, as `E:`, or empty.
fn own_drive() -> (String, std::path::PathBuf) {
    let exe = std::env::current_exe().unwrap_or_default();
    let dir = exe.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let s = dir.to_string_lossy().to_string();
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s).to_string();
    // a drive's root: `E:\` (and nothing after it)
    if s.len() == 3 && s.as_bytes()[1] == b':' && s.ends_with('\\') && s.as_bytes()[0].is_ascii_alphabetic() {
        (s[..2].to_uppercase(), std::path::PathBuf::from(&s))
    } else {
        (String::new(), dir)
    }
}

#[cfg(windows)]
mod machine {
    use upgrade_fetch::stick::{DriveKind, StickFacts};
    use windows_sys::Win32::Storage::FileSystem::*;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub struct Win {
        pub drive: String,
    }

    impl upgrade_fetch::get::Machine for Win {
        fn stick_facts(&self) -> StickFacts {
            let root = wide(&format!("{}\\", self.drive));
            let kind = if self.drive.is_empty() {
                DriveKind::Unknown
            } else {
                match unsafe { GetDriveTypeW(root.as_ptr()) } {
                    2 => DriveKind::Removable,
                    3 => DriveKind::Fixed,
                    4 => DriveKind::Remote,
                    5 => DriveKind::CdRom,
                    6 => DriveKind::RamDisk,
                    _ => DriveKind::Unknown,
                }
            };
            let mut label = vec![0u16; 64];
            let mut fs = vec![0u16; 64];
            let ok = !self.drive.is_empty() && unsafe { GetVolumeInformationW(root.as_ptr(), label.as_mut_ptr(), 64, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), fs.as_mut_ptr(), 64) } != 0;
            let text = |v: &[u16]| String::from_utf16_lossy(&v[..v.iter().position(|c| *c == 0).unwrap_or(v.len())]);
            StickFacts {
                drive: self.drive.clone(),
                kind,
                label: if ok { text(&label) } else { String::new() },
                file_system: if ok { text(&fs) } else { String::new() },
                system_drive: std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into()),
            }
        }
        fn free_bytes(&self, root: &std::path::Path) -> Result<u64, String> {
            let r = wide(&root.to_string_lossy());
            let mut free = 0u64;
            if unsafe { GetDiskFreeSpaceExW(r.as_ptr(), &mut free, std::ptr::null_mut(), std::ptr::null_mut()) } == 0 {
                return Err(format!("could not read the free space on {}", root.display()));
            }
            Ok(free)
        }
        fn pause(&mut self, secs: u64) {
            std::thread::sleep(std::time::Duration::from_secs(secs));
        }
    }
}

fn main() -> ExitCode {
    // no idle sleep while this program works (roadmap item 16, R33); ends with the process
    let _ = upgrade_scan::awake::hold();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version") {
        println!("upgrade-fetch {}", upgrade_fetch::VERSION);
        return ExitCode::SUCCESS;
    }
    let value_of = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    let phase = args.first().map(String::as_str).unwrap_or("");
    let (Some(id), Some(desktop)) = (value_of("--release"), value_of("--desktop")) else { return usage() };
    if !["status", "get", "rehearse"].contains(&phase) {
        return usage();
    }
    let plan = match plan::plan(upgrade_scan::data::RELEASES_JSON, &id, &desktop) {
        Ok(p) => p,
        Err(e) => {
            println!("REFUSED: {e}");
            return ExitCode::from(1);
        }
    };
    let (drive, root) = own_drive();
    if phase == "status" {
        return match stick::holds(&root, &plan) {
            Ok(()) => {
                println!("fetch: this stick carries {} {}", plan.name, plan.desktop_label);
                ExitCode::SUCCESS
            }
            Err(e) => {
                println!("fetch: {e}");
                ExitCode::from(3)
            }
        };
    }
    if phase == "rehearse" {
        return rehearse(&plan, value_of("--from").as_deref());
    }
    run_get(&drive, &root, &plan, value_of("--from").as_deref())
}

#[cfg(windows)]
fn rehearse(plan: &plan::Plan, from: Option<&str>) -> ExitCode {
    use upgrade_fetch::stick::{DriveKind, StickFacts};
    struct Pretend(machine::Win);
    impl get::Machine for Pretend {
        fn stick_facts(&self) -> StickFacts {
            StickFacts { drive: "R:".into(), kind: DriveKind::Removable, label: stick::LABEL.into(), file_system: "NTFS (rehearsal)".into(), system_drive: "C:".into() }
        }
        fn free_bytes(&self, root: &std::path::Path) -> Result<u64, String> {
            self.0.free_bytes(root)
        }
        fn pause(&mut self, secs: u64) {
            self.0.pause(secs)
        }
    }
    let root = std::env::temp_dir().join("upgrade_-fetch-rehearsal");
    if !root.join("KIT-MANIFEST.txt").exists() {
        let _ = std::fs::remove_dir_all(&root);
        if let Err(e) = std::fs::create_dir_all(root.join("upgrade_")).and_then(|_| std::fs::write(root.join("KIT-MANIFEST.txt"), "a rehearsal folder, not a stick\n")).and_then(|_| std::fs::write(root.join("UPGRADE.exe"), "")).and_then(|_| std::fs::write(root.join("SHA256SUMS"), "")) {
            println!("REFUSED: could not make {}: {e}", root.display());
            return ExitCode::from(1);
        }
    }
    println!("fetch: a rehearsal into {}, not a stick", root.display());
    let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
    let mut m = Pretend(machine::Win { drive });
    run_with(&root, plan, from, &mut m)
}

#[cfg(not(windows))]
fn rehearse(_: &plan::Plan, _: Option<&str>) -> ExitCode {
    println!("REFUSED: upgrade-fetch downloads on Windows");
    ExitCode::from(2)
}

#[cfg(windows)]
fn run_get(drive: &str, root: &std::path::Path, plan: &plan::Plan, from: Option<&str>) -> ExitCode {
    let mut m = machine::Win { drive: drive.to_string() };
    run_with(root, plan, from, &mut m)
}

#[cfg(windows)]
fn run_with(root: &std::path::Path, plan: &plan::Plan, from: Option<&str>, m: &mut dyn get::Machine) -> ExitCode {
    use std::io::Write;
    let mut net = match upgrade_fetch::net::WinNet::new(&format!("upgrade_-fetch/{}", upgrade_fetch::VERSION)) {
        Ok(n) => n,
        Err(e) => {
            println!("REFUSED: {e}");
            return ExitCode::from(1);
        }
    };
    let mut say = |l: &str| {
        println!("{l}");
        let _ = std::io::stdout().flush();
    };
    match get::get(root, plan, from, &mut net, m, &mut say) {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            println!("REFUSED: {e}");
            println!("Nothing on this computer was changed.");
            ExitCode::from(1)
        }
    }
}

#[cfg(not(windows))]
fn run_get(_: &str, _: &std::path::Path, _: &plan::Plan, _: Option<&str>) -> ExitCode {
    println!("REFUSED: upgrade-fetch downloads on Windows");
    ExitCode::from(2)
}
