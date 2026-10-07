//! The job writer's judging half, ported from `evaluate/windows/New-Job.ps1`
//! 0.18.0: everything that turns facts into a `job.json` or into refusals.
//! The reads (WMI, the registry, the Wi-Fi API) and the writing of files are
//! the live half, still to come.
//!
//! The facts are a JSON document with the PowerShell's own field names
//! (`$f` in `Get-JobFacts`), read with PowerShell's own leniency (`val`),
//! so an odd or missing fact is judged exactly as the script judges it.
//! `tests/parity.rs` holds every function to the PowerShell's answers.

pub mod decide;
pub mod document;
pub mod harvest;
pub mod maps;
pub mod password;
pub mod records;
pub mod val;
pub mod wifi;

/// The PowerShell job writer this port follows.
pub const FOLLOWS_JOB_WRITER: &str = "0.18.0";
/// The harvester versions whose folder map this writer reads. Any other is
/// refused, not guessed.
pub const KNOWN_HARVEST_VERSIONS: [&str; 1] = ["0.3.0"];
pub const LINUX_MIN_GB: f64 = 25.0;
/// RISKS R23 (decided 2026-09-13): typed verbatim on its own launcher, it
/// lifts exactly the drive-health and volume-health refusals. Nothing else,
/// and no shorter form.
pub const RISK_STATEMENT: &str = "I confirm that I understand the risks and could lose data";
/// RISKS R27 (decided 2026-09-26): the one-click erase and install. Separate
/// from the risk statement; neither stands in for the other.
pub const ERASE_STATEMENT: &str = "I confirm that everything on this computer will be deleted and nothing will be kept";
/// The placeholder a verify-only job carries. It is not a chosen password.
pub const VERIFY_ONLY_HASH: &str = "$6$upgradeV1$MkYfbaBe.FFp2fzSNrPiJ6RdPagcfI.crkepTcQpGsjGFMe8780OtkedouSyxvXdky5a6WiTWDy/.epwkWUk71";
