//! The decisions: which path a job takes, whether a queued repair makes a
//! shrink number untrustworthy, what the typed data-loss statement lifts,
//! which drives an erase names, and whether the stick's release can start.

use crate::val::{at, eq_ci, int, items, one_of, s, truthy};
use crate::{LINUX_MIN_GB, RISK_STATEMENT};
use serde_json::{json, Value};
use upgrade_scan::ps::matches;

/// Get-JobPath (architecture.md, "The conversion path is not a coin flip"):
/// keep Windows whenever the disk can, else clean slate, and only if the
/// person allowed that. Under `stop` the answer is keep-windows whenever
/// the disk and the boot partition allow it (the prologue measures again
/// and stops if it still does not fit), and no path at all when they do
/// not. Under `stop` a job is never a wipe (R18, 2026-09-22).
#[allow(clippy::too_many_arguments)]
pub fn path(disk_health: &str, esp_fits: bool, shrinkable_gb: Option<f64>, dirty: &str, disk_health_acknowledged: bool, repair_queued: bool, mitigable: bool, if_cannot_keep: &str) -> Option<(&'static str, &'static str)> {
    let keep = Some(("keep-windows", "default"));
    let may_wipe = eq_ci(if_cannot_keep, "clean-slate");
    if (eq_ci(disk_health, "Healthy") || disk_health_acknowledged) && esp_fits {
        match shrinkable_gb {
            Some(gb) if gb >= LINUX_MIN_GB || mitigable => return keep,
            // a flagged volume or a queued repair cannot be measured at all: not "no room"
            None if eq_ci(dirty, "dirty") || repair_queued => return keep,
            _ if !may_wipe => return keep,
            _ => {}
        }
    }
    if !may_wipe {
        return None;
    }
    Some(("clean-slate", "forced-no-room"))
}

/// A moment as Windows gave it: the words to print, and where it falls in
/// time when that is known.
#[derive(Debug, Clone, PartialEq)]
pub struct Moment {
    pub shown: String,
    pub order: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RepairQueued {
    pub queued: bool,
    pub why: String,
    /// an old request that a completed check has answered, for the record
    pub stale: String,
}

/// Test-JobRepairQueued: does Windows say C: has an offline repair queued,
/// whatever the dirty bit says? The volume's own status always counts. An
/// NTFS event 98 counts only when no completed check came after it: an
/// event is history, not state (R18, 2026-09-20).
pub fn repair_queued(volume_status: &str, ntfs_full_chkdsk: Option<&Moment>, last_check: Option<&Moment>) -> RepairQueued {
    let mut why = Vec::new();
    let mut stale = String::new();
    if matches("repair", volume_status) {
        why.push(format!("Get-Volume reports '{volume_status}'"));
    }
    if let Some(asked) = ntfs_full_chkdsk {
        let answered = last_check.filter(|done| matches!((done.order, asked.order), (Some(d), Some(a)) if d > a));
        match answered {
            Some(done) => stale = format!("NTFS asked for a full chkdsk on {}; a boot-time check completed after it, on {}", asked.shown, done.shown),
            None => why.push(format!("NTFS logged on {} that C: needs a full chkdsk", asked.shown)),
        }
    }
    RepairQueued { queued: !why.is_empty(), why: why.join("; "), stale }
}

/// The two checks the typed statement may lift, and what the job calls them.
fn liftable(check_title: &str) -> Option<&'static str> {
    [("Disk health", "disk-health"), ("Volume health", "volume-health")].iter().find(|(title, _)| eq_ci(title, check_title)).map(|(_, id)| *id)
}

/// Get-JobAcknowledgement (RISKS R23). A RED verdict is a job only when the
/// statement was typed verbatim and every failing hardware check is one of
/// the two it may lift. Returns the `risk_acknowledgement` block, nothing
/// when the verdict is not RED, or the refusal. Widening this is the thing
/// CLAUDE.md rule #1 forbids.
pub fn acknowledgement(verdict: &str, failed_checks: &[String], warn_checks: &[String], typed: &str, now_utc: &str) -> Result<Option<Value>, String> {
    if !eq_ci(verdict, "RED") {
        return Ok(None);
    }
    if typed.is_empty() {
        return Err("the scanner verdict is RED - no job, no override".into());
    }
    if typed != RISK_STATEMENT {
        return Err(format!("the scanner verdict is RED and the data-loss statement was not typed exactly (expected: {RISK_STATEMENT})"));
    }
    let not_liftable: Vec<&str> = failed_checks.iter().filter(|c| liftable(c).is_none()).map(String::as_str).collect();
    if !not_liftable.is_empty() {
        return Err(format!("the scanner verdict is RED for a reason no acknowledgement lifts: {}", not_liftable.join(", ")));
    }
    let mut overrides: Vec<&str> = failed_checks.iter().filter_map(|c| liftable(c)).collect();
    // the repair the prologue will run on the acknowledged disk is itself a data-loss risk
    if warn_checks.iter().any(|c| eq_ci(c, "Volume health")) {
        overrides.push("volume-health");
    }
    if overrides.is_empty() {
        return Err("the scanner verdict is RED but no failing check was found in the report; refusing".into());
    }
    overrides.sort();
    overrides.dedup();
    Ok(Some(json!({"statement": RISK_STATEMENT, "accepted_utc": now_utc, "overrides": overrides})))
}

/// Get-JobEraseDisks (RISKS R27): the drives an erase job names. Every
/// internal drive, the one holding C: first as system, at most one more as
/// home. The stick (by unique id) and removable buses are left alone. A bus
/// this version does not know, a third internal drive, a home drive that is
/// not Healthy, or a drive with no unique id is a refusal, never a guess.
pub fn erase_disks(disks: &Value, system_number: i64, stick_unique_id: &str) -> Result<Vec<Value>, Vec<String>> {
    const INTERNAL: [&str; 6] = ["SATA", "NVMe", "SAS", "SCSI", "ATA", "RAID"];
    const REMOVABLE: [&str; 3] = ["USB", "SD", "MMC"];
    let (mut sys, mut others, mut refusals): (Option<&Value>, Vec<&Value>, Vec<String>) = (None, Vec::new(), Vec::new());
    for d in items(disks) {
        let (bus, uid, number, name) = (s(at(d, "Bus")), s(at(d, "UniqueId")), s(at(d, "Number")), s(at(d, "Name")));
        if !stick_unique_id.is_empty() && eq_ci(&uid, stick_unique_id) {
            continue;
        }
        if one_of(&bus, &REMOVABLE) {
            continue;
        }
        if !one_of(&bus, &INTERNAL) {
            refusals.push(format!("drive {number} ({name}) is on bus '{bus}', which this version neither erases nor knows is safe to leave"));
            continue;
        }
        if uid.is_empty() {
            refusals.push(format!("drive {number} ({name}) has no unique id, so the installer could not be sure it is the same drive"));
            continue;
        }
        if int(at(d, "Number")) == system_number {
            sys = Some(d);
        } else {
            others.push(d);
        }
    }
    if sys.is_none() {
        refusals.push("the drive holding C: is not among the internal drives".into());
    }
    if others.len() > 1 {
        refusals.push(format!("this computer has {} internal drives; this version erases at most two", others.len() + 1));
    }
    for o in &others {
        let health = s(at(o, "Health"));
        if !eq_ci(&health, "Healthy") {
            refusals.push(format!("the second drive ({}) reports '{health}', and only a Healthy drive takes your home folder", s(at(o, "Name"))));
        }
    }
    let Some(sys) = sys.filter(|_| refusals.is_empty()) else { return Err(refusals) };
    let entry = |role: &str, d: &Value, health: String| json!({"role": role, "serial_number": s(at(d, "Serial")), "unique_id": s(at(d, "UniqueId")), "size_bytes": int(at(d, "Size")), "friendly_name": s(at(d, "Name")), "health_status": health});
    let sys_health = s(at(sys, "Health"));
    let mut list = vec![entry("system", sys, if one_of(&sys_health, &["Healthy", "Warning", "Unhealthy"]) { sys_health } else { "Unknown".into() })];
    list.extend(others.iter().map(|o| entry("home", o, "Healthy".into())));
    Ok(list)
}

/// Get-JobReleaseRefusal (RISKS R34): the job carries the release on this
/// stick only if the scan found this computer can start it. `kit` is the
/// stick's release.json, `releases` the scan report's list. Unknown is a
/// refusal, not a pass.
pub fn release_refusal(kit: &Value, releases: &Value) -> Option<String> {
    if !truthy(kit) || !truthy(at(kit, "id")) {
        return Some("this stick does not say which Linux release it carries (no release.json); rebuild it with the kit builder".into());
    }
    let (id, name) = (s(at(kit, "id")), s(at(kit, "name")));
    let Some(r) = items(releases).into_iter().find(|r| eq_ci(&s(at(r, "Id")), &id)) else {
        return Some(format!("the scan did not judge the release on this stick ({name}); run the scanner from this stick first"));
    };
    let starts = s(at(r, "Starts"));
    if eq_ci(&starts, "yes") {
        return None;
    }
    if eq_ci(&starts, "unknown") {
        return Some(format!("the scan could not tell whether this computer can start {name} (its key list was not read); run it as administrator"));
    }
    let why = items(at(r, "Why")).into_iter().map(s).collect::<Vec<_>>().join("; ");
    Some(format!("this computer cannot start {name}, the release on this stick: {why}"))
}
