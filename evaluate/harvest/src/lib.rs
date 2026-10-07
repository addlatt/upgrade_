//! The harvester, ported from `evaluate/windows/Harvest-UpgradeState.ps1`
//! 0.3.0: the mappings and judgments that need no machine (`tests/parity.rs`
//! holds them to the PowerShell's answers), the filesystem reads
//! (`folders`, Windows only), and the live half that writes the folder map
//! the job writer reads (`live`, Windows only; proven side by side).

pub mod capacity;
pub mod cloud;
pub mod compare;
#[cfg(windows)]
pub mod folders;
#[cfg(windows)]
pub mod live;
pub mod names;
pub mod stick;
pub mod wlan;

/// The PowerShell harvester this port follows.
pub const FOLLOWS_HARVESTER: &str = "0.3.0";
