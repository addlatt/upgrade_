//! The decisions, one function per PowerShell function, in the script's
//! order and words.

use crate::val::{at, eq_ci, int, items, s, truthy};
use crate::{CONFIRM_EXPECTED, ERASE_STATEMENT, FILES_MARGIN, GRUB_FIRED_VAR, UPDATE_MAX_RESTARTS, WINDOWS_KEEP_FREE_BYTES};
use serde_json::{json, Value};
use upgrade_scan::ps::{capture, matches, round1, Stamp};

const GB: f64 = 1073741824.0;

/// ConvertFrom-PrologueManageBde: `on`, `off` or `unknown`.
pub fn manage_bde(lines: &[String]) -> String {
    capture(r"^\s*Protection Status:\s*Protection (On|Off)\s*$", "m", &lines.join("\n")).map_or("unknown".to_string(), |m| m.to_lowercase())
}

/// ConvertFrom-PrologueChkntfs: `scheduled`, `dirty`, `clean` or `unknown`.
pub fn chkntfs(lines: &[String]) -> &'static str {
    let t = lines.join("\n");
    if matches("has been scheduled manually to run on next reboot", &t) || matches("will be checked the next time the system restarts", &t) {
        return "scheduled";
    }
    if matches(r"^\s*[A-Z]:\s+is\s+dirty", &format!("{t}")) || upgrade_scan::ps::re(r"^\s*[A-Z]:\s+is\s+dirty", "m").find(&t).is_some() {
        return "dirty";
    }
    if upgrade_scan::ps::re(r"^\s*[A-Z]:\s+is\s+not\s+dirty", "m").find(&t).is_some() {
        return "clean";
    }
    "unknown"
}

/// ConvertFrom-PrologueUsnQuery: the journal's two sizes, or not active.
pub fn usn_query(lines: &[String]) -> Value {
    let t = lines.join("\n");
    let m = capture(r"^\s*Maximum Size\s*:\s*0x([0-9a-f]+)", "m", &t);
    let a = capture(r"^\s*Allocation Delta\s*:\s*0x([0-9a-f]+)", "m", &t);
    match (m, a) {
        (Some(m), Some(a)) => json!({"Active": true, "MaxBytes": i64::from_str_radix(m, 16).unwrap_or(0), "DeltaBytes": i64::from_str_radix(a, 16).unwrap_or(0)}),
        _ => json!({"Active": false, "MaxBytes": null, "DeltaBytes": null}),
    }
}

/// A round-trip time (`2026-09-13T19:16:48.1962215Z`, or any text whose
/// first 19 characters are a date and time) as seconds; nothing otherwise.
/// A time with no zone is read as UTC: the prologue's evidence always
/// carries the `Z`.
fn instant(text: &str) -> Option<i64> {
    let t = text.trim();
    if t.len() >= 19 {
        return Stamp::parse(&t[..19]).map(|st| st.seconds());
    }
    // a date alone ([DateTime]'2026-09-16')
    Stamp::parse(&format!("{t}T00:00:00")).map(|st| st.seconds())
}

/// Test-PrologueNtfs98Fresh: does NTFS's request still stand? Empty = none.
pub fn ntfs98_fresh(ntfs98: &str, last_check: &str) -> bool {
    if ntfs98.is_empty() {
        return false;
    }
    if last_check.is_empty() {
        return true;
    }
    match (instant(ntfs98), instant(last_check)) {
        (Some(a), Some(b)) => a > b,
        // a time that does not parse: the script throws; here the request is taken as standing (never a skip)
        _ => true,
    }
}

/// Test-PrologueRepairQueued: `{Queued, Why, Stale}`.
pub fn repair_queued(volume_status: &str, ntfs98: &str, last_check: &str) -> Value {
    let mut why = Vec::new();
    let mut stale = String::new();
    if matches("repair", volume_status) {
        why.push(format!("Get-Volume reports '{volume_status}'"));
    }
    if !ntfs98.is_empty() {
        if ntfs98_fresh(ntfs98, last_check) {
            why.push(format!("NTFS logged at {ntfs98} that C: needs a full chkdsk"));
        } else {
            stale = format!("NTFS asked for a full chkdsk at {ntfs98}; a boot-time check completed after it, at {last_check}");
        }
    }
    json!({"Queued": !why.is_empty(), "Why": why.join("; "), "Stale": stale})
}

/// Get-PrologueVolumeTrigger: `dirty-flag`, `repair-queued`, `none` or `unreadable`.
pub fn volume_trigger(dirty: &str, repair_queued: bool) -> &'static str {
    if eq_ci(dirty, "dirty") {
        return "dirty-flag";
    }
    if eq_ci(dirty, "clean") {
        return if repair_queued { "repair-queued" } else { "none" };
    }
    "unreadable"
}

/// Get-PrologueRepairMethod (guardrail 3): `chkdsk-f`, `spot-fix` or `refuse`.
pub fn repair_method(scan: &str, log_verdict: &str, repair_needed: bool, ntfs_full_chkdsk: bool) -> &'static str {
    let t = scan.trim();
    if eq_ci(log_verdict, "found-problems") || repair_needed || ntfs_full_chkdsk || matches("^(ErrorsFound|ErrorsNotFixed)$", t) {
        return "chkdsk-f";
    }
    if matches("^(NoErrorsFound|ErrorsFixed)$", t) {
        return "spot-fix";
    }
    "refuse"
}

/// Test-PrologueDiskHealthGate (guardrail 2): `{Pass, Reason}`.
pub fn disk_health_gate(health: &str, bad_blocks: i64, acknowledged: bool) -> Value {
    let h = health.trim();
    let mut why = Vec::new();
    if !eq_ci(h, "Healthy") {
        why.push(format!("HealthStatus is '{h}', not Healthy"));
    }
    if bad_blocks > 0 {
        why.push(format!("Windows logged {bad_blocks} bad-block errors on this disk in the last 30 days"));
    }
    if why.is_empty() {
        return json!({"Pass": true, "Reason": "Healthy, no bad-block events"});
    }
    if acknowledged {
        return json!({"Pass": true, "Reason": format!("DATA LOSS ACCEPTED: {} - the person acknowledged the disk-health refusal", why.join("; "))});
    }
    json!({"Pass": false, "Reason": format!("{}; a repair on a failing drive can finish it off", why.join("; "))})
}

/// Compare-PrologueJob (step 1): every fact evaluate recorded that the live
/// machine can contradict. Empty means the job is this machine.
pub fn compare_job(job: &Value, f: &Value) -> Vec<String> {
    let mut m = Vec::new();
    fn cmp(m: &mut Vec<String>, name: &str, want: &Value, got: &Value) {
        if !eq_ci(&s(want), &s(got)) {
            m.push(format!("{name}: job says '{}', machine says '{}'", s(want), s(got)));
        }
    }
    cmp(&mut m, "system_disk.unique_id", at(job, "identity.system_disk.unique_id"), at(f, "Disk.UniqueId"));
    cmp(&mut m, "system_disk.serial_number", at(job, "identity.system_disk.serial_number"), at(f, "Disk.Serial"));
    cmp(&mut m, "system_disk.size_bytes", at(job, "identity.system_disk.size_bytes"), at(f, "Disk.Size"));
    cmp(&mut m, "bios_serial", at(job, "identity.bios_serial"), at(f, "BiosSerial"));
    cmp(&mut m, "system_uuid", at(job, "identity.system_uuid"), at(f, "Uuid"));
    cmp(&mut m, "firmware_mode", at(job, "identity.firmware_mode"), at(f, "Firmware"));
    cmp(&mut m, "secure_boot", at(job, "identity.secure_boot"), at(f, "SecureBoot"));
    cmp(&mut m, "os_build", at(job, "identity.os_build"), at(f, "OsBuild"));
    if truthy(at(f, "Stick")) {
        cmp(&mut m, "stick.unique_id", at(job, "stick.unique_id"), at(f, "Stick.UniqueId"));
        cmp(&mut m, "stick.size_bytes", at(job, "stick.size_bytes"), at(f, "Stick.Size"));
    } else {
        m.push(format!("stick: the stick's disk identity could not be read ({})", s(at(f, "StickError"))));
    }
    cmp(&mut m, "bitlocker.status", at(job, "harvest.bitlocker.status"), at(f, "BitLocker"));
    cmp(&mut m, "volume_health.dirty", at(job, "storage.volume_health.dirty"), at(f, "Dirty"));
    cmp(&mut m, "volume_health.repair_queued", &json!(truthy(at(job, "storage.volume_health.repair_queued"))), &json!(truthy(at(f, "RepairQueued"))));
    cmp(&mut m, "physical_disk.health_status", at(job, "storage.physical_disk.health_status"), at(f, "Health"));
    m
}

/// Compare-PrologueEraseDisks: every drive an erase job names must still be
/// here, by unique id and exact size, the first the drive holding C:.
pub fn compare_erase_disks(job: &Value, f: &Value) -> Vec<String> {
    let mut m = Vec::new();
    let disks = items(at(job, "erase_consent.disks"));
    if disks.is_empty() {
        return vec!["erase_consent names no drives".to_string()];
    }
    let first = disks[0];
    if !eq_ci(&s(at(first, "role")), "system") || !eq_ci(&s(at(first, "unique_id")), &s(at(f, "Disk.UniqueId"))) || int(at(first, "size_bytes")) != int(at(f, "Disk.Size")) {
        m.push(format!("erase_consent.disks[0] is not the drive holding C: ({}, {} bytes)", s(at(f, "Disk.UniqueId")), s(at(f, "Disk.Size"))));
    }
    for d in disks.iter().skip(1) {
        let hit = items(at(f, "AllDisks")).into_iter().find(|x| eq_ci(&s(at(x, "UniqueId")), &s(at(d, "unique_id"))));
        match hit {
            None => m.push(format!("the {} drive the job names ({}, {}) is not attached", s(at(d, "role")), s(at(d, "friendly_name")), s(at(d, "unique_id")))),
            Some(x) if int(at(x, "Size")) != int(at(d, "size_bytes")) => m.push(format!("the {} drive ({}) is {} bytes; the job says {}", s(at(d, "role")), s(at(d, "friendly_name")), s(at(x, "Size")), s(at(d, "size_bytes")))),
            _ => {}
        }
    }
    m
}

/// Get-PrologueEraseStartRefusal: which start is allowed.
pub fn erase_start_refusal(job: &Value, confirm_word: &str, erase_consent: &str) -> Option<String> {
    if truthy(at(job, "erase_consent")) {
        if s(at(job, "erase_consent.statement")) != ERASE_STATEMENT {
            return Some("the job carries an erase consent whose sentence is not the one this prologue knows; refusing".to_string());
        }
        if erase_consent != ERASE_STATEMENT {
            return Some("this job erases every drive, but the erase sentence was not typed for this run (-EraseConsent); nothing was started".to_string());
        }
        return None;
    }
    if !erase_consent.is_empty() {
        return Some("an erase sentence was given but the job is not an erase job; use the normal launcher".to_string());
    }
    if confirm_word != CONFIRM_EXPECTED {
        return Some(format!("the confirmation word was not typed (expected {CONFIRM_EXPECTED}); nothing was started"));
    }
    None
}

/// Get-PrologueEraseReturn: Windows came back on the erase path; say why.
pub fn erase_return(countdown: &Value, verify: &Value) -> Value {
    if truthy(countdown) && eq_ci(&s(at(countdown, "result")), "cancelled") {
        return json!({"StoppedAt": "countdown", "Reason": format!("a key was pressed during the countdown in the installer ({}); nothing was erased and Windows is as it was", s(at(countdown, "ended_utc")))});
    }
    if truthy(countdown) && eq_ci(&s(at(countdown, "result")), "elapsed") {
        return json!({"StoppedAt": "install", "Reason": format!("the countdown ended ({}) but Windows started again: the install did not complete - the drives may be partly erased; read upgrade_/report on the stick", s(at(countdown, "ended_utc")))});
    }
    if truthy(verify) && eq_ci(&s(at(verify, "identity.result")), "fail") {
        return json!({"StoppedAt": "identity", "Reason": "the installer did not find the drives the job names, by identity and exact size; it refused before the countdown and nothing was erased"});
    }
    if truthy(verify) && eq_ci(&s(at(verify, "payload.result")), "fail") {
        return json!({"StoppedAt": "verify-stick", "Reason": format!("the desktop image on the stick did not read back correctly ({}); the installer refused before the countdown and nothing was erased", s(at(verify, "payload.detail")))});
    }
    json!({"StoppedAt": "install", "Reason": "Windows came back before the countdown ended (the installer stopped, or the computer was restarted); nothing was erased - upgrade_/report/verify.log on the stick says where it stopped"})
}

/// Get-PrologueShrinkPlan: the keep-windows shrink's arithmetic.
pub fn shrink_plan(part_size: i64, size_min: i64, free_bytes: i64, linux_min_gb: f64, files_bytes: i64) -> Value {
    let shrinkable = (part_size - size_min).max(0);
    let target = (linux_min_gb * GB + files_bytes as f64 * FILES_MARGIN).ceil() as i64;
    let by_free = free_bytes - WINDOWS_KEEP_FREE_BYTES;
    let fits = target <= shrinkable && target <= by_free;
    let reason = if fits { "fits" } else if target > shrinkable { "immovable files cap the shrink below what Linux needs" } else { "Windows would be left with too little free space" };
    json!({"ShrinkableBytes": shrinkable, "TargetBytes": target, "MaxByFreeBytes": by_free, "Fits": fits, "RequestedBytes": if fits { json!(target) } else { Value::Null }, "Reason": reason})
}

/// Get-PrologueFork: the fork exactly as the job pre-chose it (RISKS R18).
pub fn fork(job_path: &str, fits: bool, if_cannot_keep: &str, path_reason: &str) -> &'static str {
    if eq_ci(job_path, "clean-slate") {
        if eq_ci(path_reason, "forced-no-room") && !eq_ci(if_cannot_keep, "clean-slate") {
            return "stop";
        }
        return "clean-slate";
    }
    if fits {
        return "keep-windows";
    }
    if eq_ci(if_cannot_keep, "clean-slate") {
        return "clean-slate";
    }
    "stop"
}

/// Get-PrologueTimeEstimate: seconds at a measured speed; nothing without one.
pub fn time_estimate(bytes: i64, mbps: Option<f64>) -> Option<i64> {
    let m = mbps?;
    if m <= 0.0 {
        return None;
    }
    Some((bytes as f64 / 1e6 / m).ceil() as i64)
}

/// Format-PrologueDuration.
pub fn duration(seconds: Option<i64>) -> String {
    let Some(s) = seconds else { return "unknown (no write speed measured)".to_string() };
    if s < 90 {
        return format!("about {s} seconds");
    }
    if s < 5400 {
        return format!("about {} minutes", (s as f64 / 60.0).ceil() as i64);
    }
    format!("about {} hours", upgrade_scan::ps::num(round1(s as f64 / 3600.0)))
}

/// Get-HandoffResult: the harness's classifier, unchanged.
pub fn handoff_result(fired: bool, sequence_cleared: bool, order_unchanged: bool) -> &'static str {
    if fired && sequence_cleared && order_unchanged {
        return "fired-once";
    }
    if fired && !sequence_cleared {
        return "persisted";
    }
    if !fired && order_unchanged {
        return "ignored";
    }
    if !order_unchanged {
        return "reordered";
    }
    "error"
}

/// New-GrubEnvBlock: a clean 1024-byte GRUB environment block.
pub fn grub_env_block() -> Vec<u8> {
    let mut b = vec![b'#'; 1024];
    let header = b"# GRUB Environment Block\n";
    b[..header.len()].copy_from_slice(header);
    b
}

/// Test-GrubEnvFired: does the block carry `upg_fired=1`?
pub fn grub_env_fired(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }
    let text: String = bytes.iter().map(|b| if *b < 128 { *b as char } else { '?' }).collect();
    upgrade_scan::ps::re(&format!("^{}=1\\s*$", regex_escape(GRUB_FIRED_VAR)), "m").find(&text).is_some()
}

fn regex_escape(t: &str) -> String {
    let mut out = String::new();
    for c in t.chars() {
        if "\\^$.|?*+()[]{}".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Find-StickRoot: the stick's root under whatever letter it has now.
pub fn find_stick_root(volumes: &Value, unique_id: &str) -> Option<String> {
    items(volumes).into_iter().find(|v| eq_ci(&s(at(v, "UniqueId")), unique_id) && truthy(at(v, "DriveLetter"))).map(|v| format!("{}:\\", s(at(v, "DriveLetter"))))
}

/// Get-ResumeContext: who runs this resume, and is there a screen.
pub fn resume_context(user_name: &str, user_interactive: bool, session_id: i64, explorer_running: bool) -> Value {
    json!({"RunAs": user_name, "Interactive": user_interactive, "SessionId": session_id, "ExplorerRunning": explorer_running, "Unattended": !user_interactive || session_id == 0})
}

/// ConvertTo-ResumeEvidence: SYSTEM or user, never the raw account name.
pub fn resume_evidence(r: &Value) -> Value {
    let run_as = if matches(r"(^|\\)SYSTEM$", &s(at(r, "RunAs"))) { "SYSTEM" } else { "user" };
    let wait = at(r, "StickWaitSeconds");
    json!({"utc": s(at(r, "Utc")), "run_as": run_as, "session_id": int(at(r, "SessionId")), "unattended": truthy(at(r, "Unattended")), "stick_wait_seconds": if wait.is_null() { Value::Null } else { json!(int(wait)) }})
}

/// Get-ProbeResult: the probe's verdict from its own facts.
pub fn probe_result(unattended: bool, stick_found: bool, task_removed: bool) -> &'static str {
    if !stick_found {
        return "stick-not-found";
    }
    if !unattended {
        return "resumed-attended";
    }
    if !task_removed {
        return "task-not-removed";
    }
    "resumed-unattended"
}

/// ConvertTo-ProbeCsvLine: every field quoted, quotes doubled, no newlines.
pub fn probe_csv_line(fields: &[Value]) -> String {
    fields
        .iter()
        .map(|f| {
            // runs of line ends become one space
            let (mut flat, mut in_break) = (String::new(), false);
            for c in s(f).chars() {
                if c == '\r' || c == '\n' {
                    if !in_break {
                        flat.push(' ');
                    }
                    in_break = true;
                } else {
                    flat.push(c);
                    in_break = false;
                }
            }
            format!("\"{}\"", flat.replace('"', "\"\""))
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// New-NoticeCommand: the RunOnce value.
pub fn notice_command(script_path: &str, state: &str) -> String {
    format!("powershell.exe -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File \"{script_path}\" -Notify -StateDir \"{state}\"")
}

/// Get-DriveRoot: `e` is `E:\`; a path is refused.
pub fn drive_root(letter: &str) -> Result<String, String> {
    let l = letter.trim_end_matches([':', '\\']).to_uppercase();
    if l.chars().count() != 1 {
        return Err(format!("StickDrive must be a single drive letter, got '{letter}'."));
    }
    Ok(format!("{l}:\\"))
}

/// Test-PrologueUpdatePending (RISKS R25): any one marker counts.
pub fn update_pending(u: &Value) -> bool {
    truthy(at(u, "CbsRebootPending")) || truthy(at(u, "CbsRebootInProgress")) || truthy(at(u, "WuRebootRequired"))
}

/// Get-PrologueUpdateStep: `clear`, `restart` or `stop`.
pub fn update_step(pending: bool, restarts: i64, where_: &str, max: Option<i64>) -> &'static str {
    if !pending {
        return "clear";
    }
    if eq_ci(where_, "before-arm") {
        return "stop";
    }
    if restarts < max.unwrap_or(UPDATE_MAX_RESTARTS) {
        return "restart";
    }
    "stop"
}

/// Get-PrologueRestorePointVerdict: what the counts say happened.
pub fn restore_point_verdict(before: Option<i64>, after: Option<i64>) -> &'static str {
    let (Some(b), Some(a)) = (before, after) else { return "unknown" };
    if b == 0 {
        return "none-there";
    }
    if a == 0 {
        return "deleted-all";
    }
    if a < b {
        return "deleted-some";
    }
    "deleted-none"
}

/// Format-PrologueRestorePoints: the log line for a restore-point attempt.
pub fn format_restore_points(r: &Value) -> String {
    let what = match s(at(r, "Verdict")).as_str() {
        "deleted-all" => format!("deleted all {}", s(at(r, "Before"))),
        "deleted-some" => format!("deleted {} of {} - {} remain", s(at(r, "Deleted")), s(at(r, "Before")), s(at(r, "After"))),
        "deleted-none" => format!("deleted NONE of {}", s(at(r, "Before"))),
        "none-there" => "there were none to delete".to_string(),
        _ => "the count could not be read, so it is not known whether any were deleted".to_string(),
    };
    let exit = at(r, "VssadminExit");
    let mut how = format!("vssadmin exit {}", if exit.is_null() { "unknown".to_string() } else { s(exit) });
    let text = s(at(r, "Text"));
    if !text.is_empty() {
        how.push_str(&format!(": {}", text.split('\n').next_back().unwrap_or("").trim()));
    }
    let w: Vec<String> = items(at(r, "Wmi")).into_iter().map(s).filter(|x| !x.is_empty()).collect();
    if !w.is_empty() {
        how.push_str(&format!("; then one by one: {}", w.join("; ")));
    }
    format!("restore points: {what} ({how})")
}

/// Test-PrologueShadowStorageFile: System Restore's storage, as Defrag 259 names it.
pub fn shadow_storage_file(last_unmovable: &str) -> bool {
    matches(r"^\\?System Volume Information\\[^\\]*\{3808876b-c176-4e48-b7ae-04046e6cc752\}", last_unmovable)
}

/// Get-PrologueRestorePointStep (R18): `none`, `already-done`, `no-consent` or `delete`.
pub fn restore_point_step(fits: bool, last_unmovable: &str, consented: bool, already_done: bool) -> &'static str {
    if fits || !shadow_storage_file(last_unmovable) {
        return "none";
    }
    if already_done {
        return "already-done";
    }
    if !consented {
        return "no-consent";
    }
    "delete"
}

/// Test-PrologueUsnJournalFile: NTFS's change journal, as Defrag 259 names it.
pub fn usn_journal_file(last_unmovable: &str) -> bool {
    matches(r"^\\?\$Extend\\\$UsnJrnl(:\$J)?(:\$DATA|::\$DATA)?$", last_unmovable)
}

/// Get-PrologueUsnJournalStep (R18).
pub fn usn_journal_step(fits: bool, last_unmovable: &str, consented: bool, done_this_boot: bool) -> &'static str {
    if fits || !usn_journal_file(last_unmovable) {
        return "none";
    }
    if done_this_boot {
        return "already-done";
    }
    if !consented {
        return "no-consent";
    }
    "delete"
}

/// Get-PrologueRestorePlan: what a stop (or the return after an install)
/// puts back. Only what this run turned off, and only to what it was.
pub fn restore_plan(shrink: &Value, keep_hibernation_off: bool) -> Vec<&'static str> {
    let mut plan = Vec::new();
    if !truthy(shrink) {
        return plan;
    }
    let uj = at(shrink, "UsnJournal");
    if truthy(uj) && int(at(uj, "Deletions")) > 0 && !truthy(at(uj, "Recreated")) && truthy(at(uj, "Before")) && truthy(at(uj, "Before.Active")) {
        plan.push("usn-journal");
    }
    let b = at(shrink, "Before");
    if truthy(at(shrink, "HibernationDisabled")) && !keep_hibernation_off {
        if truthy(b) && at(b, "HibernateEnabled") == &Value::Bool(true) {
            plan.push("hibernation-on");
        } else if !truthy(b) || at(b, "HibernateEnabled").is_null() {
            plan.push("hibernation-unknown");
        }
    }
    if truthy(at(shrink, "PagefileDisabled")) {
        let auto_false = truthy(b) && at(b, "AutoPagefile") == &Value::Bool(false);
        if auto_false && !items(at(b, "PagefileSettings")).is_empty() {
            plan.push("pagefile-settings");
        } else if auto_false {
            // it had none before; it has none now
        } else {
            plan.push("pagefile-auto");
        }
    }
    plan
}

/// Get-PrologueStageRefusal: nothing to stage, or nothing staged, is a refusal.
pub fn stage_refusal(folders: i64, staged_files: Option<i64>) -> Option<String> {
    if folders < 1 {
        return Some("the job lists none of your folders (none of the six was found on this computer), so there is nothing to copy to the stick; refusing to prepare a wipe with no copy of your files".to_string());
    }
    if staged_files.is_some_and(|n| n < 1) {
        return Some(format!("no files were copied to the stick from the {folders} folder(s) the job lists; refusing to prepare a wipe with no copy of your files"));
    }
    None
}

/// Get-PrologueStopSentence: "as it was" only when that is true.
pub fn stop_sentence(restored: &[String]) -> String {
    let r: Vec<&String> = restored.iter().filter(|x| !x.is_empty()).collect();
    let bad: Vec<String> = r.iter().filter(|x| x.starts_with('!')).map(|x| x.trim_start_matches(['!', ' ']).to_string()).collect();
    if !bad.is_empty() {
        return format!("Windows was NOT fully put back: {}.", bad.join("; "));
    }
    if r.iter().any(|x| x.to_lowercase().contains("next restart")) {
        return "Windows is as it was, once it has restarted: the pagefile returns at the next restart.".to_string();
    }
    "Windows is as it was.".to_string()
}

/// Format-PrologueScan: the cmdlet's answer and the evidence in one line.
pub fn format_scan(cmdlet: &str, ev: &Value) -> String {
    let mut parts = vec![format!("cmdlet: {cmdlet}")];
    if truthy(ev) {
        let mut log = format!("log: {}", s(at(ev, "LogVerdict")));
        if int(at(ev, "LogRecords")) > 0 {
            log.push_str(&format!(" ({} corruption records", int(at(ev, "LogRecords"))));
            if int(at(ev, "LogQueued")) > 0 {
                log.push_str(&format!(", {} queued for offline repair", int(at(ev, "LogQueued"))));
            }
            log.push(')');
        }
        parts.push(log);
        if truthy(at(ev, "VolumeStatus")) {
            parts.push(format!("volume: {}", s(at(ev, "VolumeStatus"))));
        }
        if truthy(at(ev, "NtfsFullChkdsk")) {
            parts.push(format!("ntfs98: {}", s(at(ev, "NtfsFullChkdsk"))));
        }
    }
    parts.join("; ")
}
