//! Installed programs that will not come along to Linux.

use crate::check::{Scan, Status};
use crate::data::tables;
use crate::ps::{eq_ci, matches};

/// `apps`: the display names of the installed programs.
pub fn apps(scan: &mut Scan, apps: &[String]) {
    // one empty name counts as none, as it does in PowerShell
    if apps.is_empty() || (apps.len() == 1 && apps[0].is_empty()) {
        scan.add("Software", "Installed software", Status::Unknown, "could not enumerate");
        return;
    }
    let mut reported: Vec<&str> = Vec::new();
    for rule in &tables().app_risk {
        let matched: Vec<&str> = apps.iter().map(String::as_str).filter(|a| matches(&rule.pattern, a)).collect();
        if matched.is_empty() || reported.iter().any(|p| eq_ci(p, &rule.pattern)) {
            continue;
        }
        reported.push(&rule.pattern);
        let (title, status) = if eq_ci(&rule.severity, "blocker") {
            ("No Linux equivalent", Status::Fail)
        } else if eq_ci(&rule.severity, "friction") {
            ("Works differently", Status::Warn)
        } else {
            ("Worth knowing", Status::Info)
        };
        let mut shown = matched.iter().take(3).copied().collect::<Vec<_>>().join(", ");
        if matched.len() > 3 {
            shown.push_str(&format!(" (+{} more)", matched.len() - 3));
        }
        scan.add("Software", title, status, shown).note(&*rule.note);
    }
    scan.add("Software", "Programs installed", Status::Info, format!("{} total", apps.len()));
}
