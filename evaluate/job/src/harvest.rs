//! The folder map in a job. The map is what settle-in pulls and what a
//! clean slate stages to the stick. A map that is wrong is silent data
//! loss, found after Windows is gone, so each problem here is a refusal,
//! never a note (CLAUDE.md rule #1).

use crate::val::{at, eq_ci, int, items, list, one_of, s, truthy};
use crate::KNOWN_HARVEST_VERSIONS;
use serde_json::{json, Value};
use upgrade_scan::ps::fmt_n;

const GB: f64 = 1073741824.0;

/// Get-JobHarvestRefusals. `h` is the harvester's folder map (nothing when
/// it could not be made, and then `harvest_error` says why).
pub fn refusals(h: &Value, harvest_error: &str) -> Vec<String> {
    if !truthy(h) {
        return vec![format!("the list of your folders could not be read ({harvest_error})")];
    }
    let version = s(at(h, "HarvestVersion"));
    if !one_of(&version, &KNOWN_HARVEST_VERSIONS) {
        return vec![format!("the folder map comes from harvester '{version}', which this job writer does not read (it reads {}) - the kit is mixed; nothing was changed", KNOWN_HARVEST_VERSIONS.join(", "))];
    }
    let mut r = Vec::new();
    let o = at(h, "Owner");
    let name = s(at(o, "ProcessName"));
    let owners: Vec<String> = items(at(o, "DesktopOwnerSids")).into_iter().filter(|x| truthy(x)).map(s).collect();
    if owners.is_empty() {
        r.push(format!("could not tell who is signed in on this screen, so could not tell whose folders these are (this window runs as {name}) - start the launcher from the desktop of the person whose computer this is"));
    } else if !owners.iter().any(|sid| eq_ci(sid, &s(at(o, "ProcessSid")))) {
        r.push(format!("this window runs as {name}, but another account is signed in on this screen - the folders read would be the wrong person's. Start the launcher from an account that is itself an administrator"));
    }
    let present: Vec<&Value> = items(at(h, "UserFolders")).into_iter().filter(|f| truthy(at(f, "Exists"))).collect();
    for f in &present {
        let folder = s(at(f, "Name"));
        if truthy(at(f, "Truncated")) {
            r.push(format!("{folder} holds more files than this version counts ({}), so its size would be too low (RISKS R6)", s(at(f, "Files"))));
        }
        if int(at(f, "Unreadable")) > 0 {
            let first = list(at(f, "UnreadableFirst")).first().map(|x| s(x)).unwrap_or_default();
            r.push(format!("Windows would not let the harvest read {} folder(s) inside {folder} (first: {first}) - its size would be too low and those files would not be copied (RISKS R6)", s(at(f, "Unreadable"))));
        }
    }
    let c = at(h, "CloudFiles");
    let result = s(at(c, "Result"));
    if eq_ci(&result, "refused") || int(at(c, "Failed")) > 0 {
        r.push(format!("{} of {} OneDrive online-only file(s) could not be downloaded; copied from Linux they would arrive EMPTY (RISKS R8). Check that OneDrive is running and signed in, then run it again", s(at(c, "Failed")), s(at(c, "PlaceholdersFound"))));
    } else if eq_ci(&result, "materialized") {
        // online-only files left alone are not a refusal (decided 2026-09-26):
        // the job records them and settle-in reconnects OneDrive. Files that
        // went online-only again after a download are.
        let again: i64 = present.iter().map(|f| int(at(f, "CloudOnlyNow"))).sum();
        if again > 0 {
            r.push(format!("after downloading, {again} OneDrive file(s) were online-only again - OneDrive freed them while the list was being made (RISKS R8)"));
        }
    } else if !one_of(&result, &["none-found", "not-attempted"]) {
        r.push(format!("the OneDrive check ended as '{result}', which this version does not understand"));
    }
    if !truthy(at(h, "StickFit")) {
        r.push(format!("the stick's free space could not be read ({})", s(at(h, "Stick.Error"))));
    }
    r
}

/// Get-JobStickFitRefusal. Clean slate stages the folders to the stick and
/// then wipes Windows: a job whose folders do not fit, or that would leave
/// other people's files behind (RISKS R5), is no job. The gap is named so
/// the next try can succeed.
pub fn stick_fit_refusal(h: &Value, path: &str) -> Option<String> {
    if !eq_ci(path, "clean-slate") {
        return None;
    }
    let fit = at(h, "StickFit");
    if !truthy(at(fit, "Fits")) {
        let more = if int(at(fit, "GapBytes")) > 0 {
            let at_least = (int(at(fit, "NeededBytes")) as f64 / GB * 10.0).ceil() / 10.0;
            format!("; a stick with at least {} GB free would hold them", fmt_n(at_least, 1))
        } else {
            String::new()
        };
        return Some(format!("Windows cannot be kept, and your folders do not fit on this stick: {}{more}", s(at(fit, "Reason"))));
    }
    // `list`, not `items`: a map that does not say who else is on the
    // computer counts as one unnamed account, as it does in PowerShell
    let others = list(at(h, "Owner.OtherProfiles"));
    if !others.is_empty() {
        let paths = others.iter().map(|o| s(at(o, "Path"))).collect::<Vec<_>>().join(", ");
        return Some(format!("this computer has {} other account(s) ({paths}); a clean slate would delete their files, and this version copies only yours (RISKS R5)", others.len()));
    }
    None
}

/// ConvertTo-JobHarvest: the folder map as the job's `harvest.folders`,
/// `harvest.cloud_files` and `harvest.stick_fit`. Called only after
/// `refusals` found nothing.
pub fn to_job(h: &Value) -> (Value, Value, Value) {
    let folders: Vec<Value> = list(at(h, "UserFolders"))
        .into_iter()
        .map(|f| {
            let path = if truthy(at(f, "Path")) { json!(s(at(f, "Path"))) } else { Value::Null };
            json!({"name": s(at(f, "Name")), "path": path, "exists": truthy(at(f, "Exists")), "is_onedrive": truthy(at(f, "IsOneDrive")),
                   "files": int(at(f, "Files")), "bytes": int(at(f, "Bytes")), "cloud_only_files": int(at(f, "CloudOnlyFiles")), "truncated": false})
        })
        .collect();
    let c = at(h, "CloudFiles");
    let result = s(at(c, "Result"));
    let cloud = json!({"placeholders_found": int(at(c, "PlaceholdersFound")), "materialized": int(at(c, "Materialized")), "failed": 0,
                       "result": if eq_ci(&result, "not-attempted") { "left-in-cloud".to_string() } else { result }});
    let fit = at(h, "StickFit");
    let stick_fit = json!({"filesystem": s(at(fit, "FileSystem")), "cluster_bytes": int(at(fit, "ClusterBytes")), "free_bytes": int(at(fit, "FreeBytes")), "files_bytes": int(at(fit, "FilesBytes")),
                           "needed_bytes": int(at(fit, "NeededBytes")), "files_over_4gib": int(at(fit, "FilesOver4GiB")), "fits": truthy(at(fit, "Fits")), "gap_bytes": int(at(fit, "GapBytes"))});
    (json!(folders), cloud, stick_fit)
}
