//! The stick writer in Rust (RISKS R16, R32), following
//! `evaluate/windows/Write-UpgradeStick.ps1` 0.1.1 and
//! `evaluate/windows/usb-kit/Copy-Kit.ps1`. `judge` decides which disk
//! may be written and why every other one is refused; `flow` runs the
//! phases (list, plan, write) over a seam; `copy` refreshes a stick that
//! already carries a volume; `live` (Windows only) is the seam on a real
//! machine, and the one place that writes a device.

pub mod copy;
pub mod flow;
pub mod judge;
#[cfg(windows)]
pub mod live;

/// The PowerShell writer this port follows.
pub const FOLLOWS_WRITER: &str = "0.1.1";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The boot volume's label when `write` is given none: the one the product
/// finds the stick by (the installer's boot line, `settle-in rollback`, the
/// boot marker).
pub const BOOT_LABEL: &str = "UPGV0";
