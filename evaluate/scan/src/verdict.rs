//! From the checks to one answer: RED, YELLOW or GREEN, the kernel the
//! machine needs, and which distributions fit. There is no way to turn a
//! RED into anything else here (CLAUDE.md rule #1).

use crate::check::{Check, Scan, Status};
use crate::ps::{eq_ci, Version};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Red,
    Yellow,
    Green,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Red => "RED",
            Level::Yellow => "YELLOW",
            Level::Green => "GREEN",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub priority: u8,
    pub label: &'static str,
    pub items: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    pub level: Level,
    pub summary: &'static str,
    pub groups: Vec<Group>,
}

/// The newest kernel any check asks for.
pub fn required_kernel(scan: &Scan) -> Option<Version> {
    scan.checks.iter().filter_map(|c| Version::parse(&c.min_kernel)).max()
}

/// "Fast Startup is on" and "this disk may be encrypted" are not the same
/// kind of thing, so every problem is ranked into one of four buckets.
fn priority(c: &Check) -> u8 {
    if eq_ci(&c.section, "Software") {
        4
    } else if c.status == Status::Fail || c.status == Status::Unknown {
        1 // unknown risk is still risk
    } else if eq_ci(&c.section, "Storage") {
        2
    } else {
        3
    }
}

fn label(priority: u8) -> &'static str {
    match priority {
        1 => "MUST RESOLVE - these can cost you data or stop the install",
        2 => "DO BEFORE YOU START",
        3 => "EXPECT TO DEAL WITH",
        _ => "SOFTWARE YOU WILL LOSE OR HAVE TO REPLACE",
    }
}

/// A software blocker is a different "no" from a hardware one: the machine
/// runs Linux fine, the person loses a tool. Only hardware makes RED.
pub fn verdict(scan: &Scan) -> Verdict {
    let software = |c: &&Check| eq_ci(&c.section, "Software");
    let fails: Vec<&Check> = scan.checks.iter().filter(|c| c.status == Status::Fail).collect();
    let hw_fail = fails.iter().any(|c| !software(c));
    let sw_fail = fails.iter().any(software);
    let issues: Vec<&Check> = scan.checks.iter().filter(|c| matches!(c.status, Status::Fail | Status::Warn | Status::Unknown)).collect();

    let mut groups = Vec::new();
    for p in 1..=4u8 {
        let items: Vec<String> = issues.iter().filter(|c| priority(c) == p).map(|c| format!("{}: {}", c.title, c.detail)).collect();
        if !items.is_empty() {
            groups.push(Group { priority: p, label: label(p), items });
        }
    }
    if hw_fail {
        return Verdict { level: Level::Red, summary: "Do not convert this machine as it stands. Something here blocks the install outright - resolve it, or use a different machine.", groups };
    }
    if sw_fail {
        return Verdict { level: Level::Yellow, summary: "The hardware is fine. The real question is whether you can work without the software listed below - that is a decision only you can make.", groups };
    }
    if !groups.is_empty() {
        return Verdict { level: Level::Yellow, summary: "Convertible, with specific steps to take first.", groups };
    }
    Verdict { level: Level::Green, summary: "No obstacles found. This machine should convert cleanly.", groups: Vec::new() }
}
