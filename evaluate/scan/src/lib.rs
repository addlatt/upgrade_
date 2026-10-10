//! The scanner, ported from `evaluate/windows/upgrade-scan.ps1` (scanner
//! 0.5.0). The judging functions take facts and say what they mean; the
//! collectors (`collect`, Windows only) read the machine; `product` strings
//! them together and writes the report as the launchers expect it.
//!
//! The port is held to the PowerShell word for word. `tests/cases.json` is one
//! list of inputs, `tests/golden.json` is what the PowerShell says for each
//! (written by `tests/golden.ps1`), and `tests/parity.rs` requires the same
//! here: status, wording, order.

pub mod check;
pub mod collect;
pub mod data;
pub mod facts;
pub mod hardware;
pub mod parse;
pub mod product;
pub mod ps;
pub mod report;
pub mod run;
pub mod sbat;
pub mod software;
pub mod storage;
pub mod system;
pub mod verdict;

pub use check::{Check, Scan, Status};

/// The scanner version this port follows.
pub const FOLLOWS_SCANNER: &str = "0.5.0";
