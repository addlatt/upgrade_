//! The harvester's pure half, ported from
//! `evaluate/windows/Harvest-UpgradeState.ps1` 0.3.0: the mappings and the
//! judgments that need no machine. The reads (folder sizes, `netsh`, the
//! cloud-file attributes of real files) are the collectors, still to come.
//!
//! `tests/parity.rs` holds every function here to the PowerShell's answers.

pub mod capacity;
pub mod cloud;
#[cfg(windows)]
pub mod folders;
pub mod names;
pub mod stick;
pub mod wlan;

/// The PowerShell harvester this port follows.
pub const FOLLOWS_HARVESTER: &str = "0.3.0";
