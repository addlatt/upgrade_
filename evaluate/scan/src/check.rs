//! One line of the report (a check) and the scan that collects them.

use crate::ps::eq_ci;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ok,
    Warn,
    Fail,
    Unknown,
    Info,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Warn => "warn",
            Status::Fail => "fail",
            Status::Unknown => "unknown",
            Status::Info => "info",
        }
    }

    /// A status named in a data table. Anything else is a broken table.
    pub fn parse(text: &str) -> Option<Status> {
        [Status::Ok, Status::Warn, Status::Fail, Status::Unknown, Status::Info].into_iter().find(|x| eq_ci(x.as_str(), text))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Check {
    pub section: String,
    pub title: String,
    pub status: Status,
    pub detail: String,
    pub note: String,
    pub min_kernel: String,
    pub remedy: String,
}

/// What Test-UpgReleases records for the JSON report, per release.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct ReleaseVerdict {
    pub id: String,
    pub name: String,
    /// `yes`, `no` or `unknown`
    pub starts: String,
    pub why: Vec<String>,
}

/// Everything one scan has said so far. The judging functions add to it,
/// in order; the verdict reads it.
#[derive(Debug, Default)]
pub struct Scan {
    pub checks: Vec<Check>,
    /// Devices the tables do not know one by one ("wifi 8086:abcd (name)").
    pub unmatched: Vec<String>,
    pub releases: Vec<ReleaseVerdict>,
}

impl Scan {
    pub fn new() -> Scan {
        Scan::default()
    }

    pub fn add(&mut self, section: &str, title: &str, status: Status, detail: impl Into<String>) -> &mut Check {
        self.checks.push(Check {
            section: section.to_string(),
            title: title.to_string(),
            status,
            detail: detail.into(),
            note: String::new(),
            min_kernel: String::new(),
            remedy: String::new(),
        });
        self.checks.last_mut().expect("just pushed")
    }
}

impl Check {
    pub fn note(&mut self, text: impl Into<String>) -> &mut Check {
        self.note = text.into();
        self
    }
    pub fn remedy(&mut self, text: impl Into<String>) -> &mut Check {
        self.remedy = text.into();
        self
    }
    pub fn min_kernel(&mut self, text: impl Into<String>) -> &mut Check {
        self.min_kernel = text.into();
        self
    }
}
