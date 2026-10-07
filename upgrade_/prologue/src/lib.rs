//! The prologue's judging half, ported from
//! `upgrade_/windows/Invoke-Prologue.ps1` 0.12.0: everything that decides,
//! taken apart from everything that acts. The R18 guardrails (the volume
//! trigger, the repair rung, the disk-health gate), the re-validation of
//! the job against the live machine, the shrink plan and the fork, the
//! ladder's rungs and their consents (restore points, the change journal,
//! the memory files and what a stop puts back), R25's waiting update, the
//! erase path's start and return, the handoff's classifier, the resume's
//! context, and the records: the state, the `prologue` block of
//! `outcome.json`, and a stopped outcome.
//!
//! It reads no machine and writes no file. `tests/parity.rs` holds every
//! function to the PowerShell's answers, word for word.

pub mod compare;
#[cfg(windows)]
pub mod flow;
pub mod judge;
#[cfg(windows)]
pub mod live;
pub mod random;
pub mod rollback;
pub mod state;
pub mod tools;
pub mod val;

/// What this program calls itself.
pub fn flow_version() -> String {
    format!("upgrade-prologue {} (Rust; follows Invoke-Prologue.ps1 {FOLLOWS_PROLOGUE})", env!("CARGO_PKG_VERSION"))
}

/// The PowerShell prologue this port follows.
pub const FOLLOWS_PROLOGUE: &str = "0.12.0";
pub const CONFIRM_EXPECTED: &str = "CONVERT";
/// RISKS R23: typed verbatim; lifts exactly the disk-health gate and the volume-health stop.
pub const RISK_STATEMENT: &str = "I confirm that I understand the risks and could lose data";
/// RISKS R27: the one-click erase and install; stands in for CONVERT on that path.
pub const ERASE_STATEMENT: &str = "I confirm that everything on this computer will be deleted and nothing will be kept";
pub const GRUB_FIRED_VAR: &str = "upg_fired";
/// What the kept Windows must still have free after the shrink.
pub const WINDOWS_KEEP_FREE_BYTES: i64 = 8 * 1073741824;
/// Headroom over the harvested bytes Linux must hold until reclaim.
pub const FILES_MARGIN: f64 = 1.2;
/// RISKS R25: at most this many restarts of our own for a waiting update.
pub const UPDATE_MAX_RESTARTS: i64 = 3;
pub const PROBE_CSV_HEADER: [&str; 18] = ["timestamp", "prologue_version", "vendor", "model", "bios", "os", "secure_boot", "stick_bus", "run_as", "session_id", "interactive", "explorer_running", "uptime_s", "stick_wait_s", "notice", "task_removed", "result", "notes"];
