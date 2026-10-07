//! The prologue's records: its state (what every phase reads and writes
//! across restarts), the `prologue` block of `outcome.json`, and a stopped
//! outcome. The field names are the script's, so a state file the
//! PowerShell wrote reads here, and the other way round.

use crate::val::{at, int, items, one_of, s, truthy};
use crate::judge::resume_evidence;
use serde_json::{json, Value};

/// New-PrologueState. `started_utc` is the clock in .NET round-trip form.
pub fn new_state(job_id: &str, stick_id: &str, root: &str, version: &str, started_utc: &str) -> Value {
    json!({
        "PrologueVersion": version, "Stage": "started", "StartedUtc": started_utc, "UpdatedUtc": null,
        "JobId": job_id, "StickUniqueId": stick_id, "StickRootAtStart": root, "Restarts": 0, "Mismatches": [],
        "Ack": {"Present": false, "DiskHealth": false, "VolumeHealth": false},
        "VolumeCheck": {"Trigger": null, "Needed": false, "Ran": false, "Scan": null, "DiskHealthAtCheck": null, "BadBlocks": 0, "Gate": null, "Evidence": null, "Method": "none", "ArmedUtc": null, "ArmText": null, "Chkntfs": null, "Wininit1001": null, "Found000": null, "DirtyAfter": "unknown", "Restarts": 0},
        "Shrink": {"LastUnmovable": null, "RemeasuredGB": null, "RemeasuredBy": null, "DiskpartGB": null, "ApiError": null, "DiskpartError": null, "PartSize": null, "SizeMin": null, "FreeBytes": null, "Plan": null, "ForkTaken": null, "RequestedBytes": null, "FreedBytes": 0, "SizeBefore": null, "PagefileDisabled": false, "HibernationDisabled": false, "Mitigated": false, "Before": null, "Restored": null, "RestorePoints": null, "UsnJournal": null},
        "Update": {"Checks": [], "Restarts": 0, "ResumeTo": null},
        "Staged": null,
        "BitLocker": {"StatusBefore": null, "Source": null, "Suspended": false, "RebootCount": null},
        "Handoff": {"Armed": false, "Marker": null, "EntryGuid": null, "ArmedUtc": null, "BcdBackup": null, "Before": null, "GrubEnvReset": false},
        "Resumes": [],
        "Return": null,
    })
}

/// New-PrologueBlock: the `prologue` block of outcome.json, from the state.
pub fn block(st: &Value) -> Value {
    let vc = at(st, "VolumeCheck");
    let sh = at(st, "Shrink");
    let bl = at(st, "BitLocker");
    let ho = at(st, "Handoff");
    let health_text = s(at(vc, "DiskHealthAtCheck"));
    let health = if one_of(&health_text, &["Healthy", "Warning", "Unhealthy"]) {
        json!(health_text)
    } else if truthy(at(vc, "DiskHealthAtCheck")) {
        json!("Unknown")
    } else {
        Value::Null
    };
    let trigger = if truthy(at(vc, "Trigger")) { json!(s(at(vc, "Trigger"))) } else { Value::Null };
    let mut b = json!({
        "revalidated": items(at(st, "Mismatches")).is_empty(),
        "mismatches": items(at(st, "Mismatches")).into_iter().cloned().collect::<Vec<_>>(),
        "volume_check": {
            "needed": truthy(at(vc, "Needed")), "ran": truthy(at(vc, "Ran")), "disk_health_at_check": health, "method": s(at(vc, "Method")),
            "scan": at(vc, "Scan").clone(), "restarts": int(at(vc, "Restarts")), "wininit_1001": at(vc, "Wininit1001").clone(),
            "found000_present": at(vc, "Found000").clone(), "dirty_after": s(at(vc, "DirtyAfter")),
            "trigger": trigger,
        },
        "shrink": {
            "remeasured_gb": at(sh, "RemeasuredGB").clone(), "remeasured_by": at(sh, "RemeasuredBy").clone(), "remeasured_diskpart_gb": at(sh, "DiskpartGB").clone(),
            "fork_taken": at(sh, "ForkTaken").clone(), "requested_bytes": at(sh, "RequestedBytes").clone(), "freed_bytes": int(at(sh, "FreedBytes")),
            "pagefile_disabled": truthy(at(sh, "PagefileDisabled")), "hibernation_disabled": truthy(at(sh, "HibernationDisabled")),
            "restore_points_deleted": if truthy(at(sh, "RestorePoints")) { int(at(sh, "RestorePoints.Deleted")) } else { 0 },
            "usn_journal_deleted": if truthy(at(sh, "UsnJournal")) { int(at(sh, "UsnJournal.Deletions")) } else { 0 },
        },
    });
    let resumes = items(at(st, "Resumes"));
    if !resumes.is_empty() {
        b["resumes"] = Value::Array(resumes.into_iter().map(resume_evidence).collect());
    }
    let checks = items(at(st, "Update.Checks"));
    if truthy(at(st, "Update")) && !checks.is_empty() {
        // RISKS R25: every pending-restart check and the restarts it took
        b["windows_update"] = json!({"checks": checks.len(), "pending_seen": checks.iter().any(|c| truthy(at(c, "Pending"))), "restarts": int(at(st, "Update.Restarts"))});
    }
    if truthy(at(st, "Staged")) {
        let sg = at(st, "Staged");
        b["staged"] = json!({"files": int(at(sg, "Files")), "bytes": int(at(sg, "Bytes")), "failed": int(at(sg, "Failed")), "write_mbps": at(sg, "WriteMbps").clone(), "estimated_seconds": at(sg, "EstimatedSeconds").clone(), "manifest": s(at(sg, "Manifest"))});
    }
    let before = s(at(bl, "StatusBefore"));
    b["bitlocker"] = json!({"status_before": if one_of(&before, &["on", "off"]) { before } else { "off".to_string() }, "suspended": truthy(at(bl, "Suspended")), "reboot_count": at(bl, "RebootCount").clone()});
    b["handoff"] = json!({"armed": truthy(at(ho, "Armed")), "marker": at(ho, "Marker").clone(), "entry_guid": at(ho, "EntryGuid").clone(), "armed_utc": at(ho, "ArmedUtc").clone(), "bcd_backup": at(ho, "BcdBackup").clone()});
    b
}

/// New-PrologueStoppedOutcome: a refusal is an outcome too. `version` is
/// what `converter_version` names; `created_utc` is `yyyy-MM-ddTHH:mm:ssZ`.
pub fn stopped_outcome(job: &Value, st: &Value, stopped_at: &str, reason: &str, windows_partition: &Value, version: &str, created_utc: &str) -> Value {
    let fork = s(at(st, "Shrink.ForkTaken"));
    let path = if one_of(&fork, &["keep-windows", "clean-slate"]) { json!(fork) } else { Value::Null };
    let mut o = json!({
        "schema": "outcome/1", "job_id": s(at(job, "job_id")),
        "created_utc": created_utc,
        "converter_version": version,
        "status": "stopped", "stopped_at": stopped_at, "reason": reason, "path_taken": path,
        "commit_line": {"crossed": false, "crossed_utc": null, "act": null},
        "prologue": block(st),
    });
    if truthy(at(job, "risk_acknowledgement")) {
        o["risk_acknowledgement"] = json!({"statement": s(at(job, "risk_acknowledgement.statement")), "accepted_utc": s(at(job, "risk_acknowledgement.accepted_utc")), "overrides": items(at(job, "risk_acknowledgement.overrides")).into_iter().cloned().collect::<Vec<_>>()});
    }
    o["windows"] = json!({"kept": true, "partition": windows_partition.clone(), "reachable_via": "firmware-entry"});
    o["credentials"] = json!({"scrubbed": true, "scrub_after": if s(&o["path_taken"]) == "keep-windows" { "settle-in-pull" } else { "cutover" }});
    o["logs"] = json!(["upgrade_/report/prologue.log"]);
    if truthy(at(job, "erase_consent")) {
        o["erase_consent"] = at(job, "erase_consent").clone();
    }
    o
}
