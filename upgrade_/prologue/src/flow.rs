//! The stages, following `Invoke-Prologue.ps1` 0.12.0: start, the disk
//! check and its return, the re-measure and the fork, the shrink or the
//! staging, the arm, the return to Windows, the walk-away probe, the notice,
//! the abort, and the stop that undoes what it can. Every decision is the
//! judging half's; every tool call is kept by the recorder; the record on
//! the stick (`upgrade_/prologue.json`, `prologue-return.json`,
//! `outcome.json`, `report/tools.jsonl`) is what the rig and the owner read.

use crate::judge;
use crate::live;
use crate::state;
use crate::tools::Recorder;
use crate::val::{at, int, items, s, truthy};
use crate::{FILES_MARGIN, FOLLOWS_PROLOGUE, RISK_STATEMENT, UPDATE_MAX_RESTARTS, WINDOWS_KEEP_FREE_BYTES};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const GB: f64 = 1073741824.0;
const UPDATE_WAIT_SECONDS: i64 = 600;

/// What this program calls itself in the records.
pub fn version_line() -> String {
    format!("upgrade-prologue {} (Rust; follows Invoke-Prologue.ps1 {FOLLOWS_PROLOGUE})", env!("CARGO_PKG_VERSION"))
}

pub struct Ctx {
    pub rec: Recorder,
    pub state_dir: String,
    pub log_file: Option<PathBuf>,
    pub stick_log: Option<PathBuf>,
    /// true when this process is the `start` one (popups are for a person at the keyboard)
    pub started_here: bool,
}

impl Ctx {
    pub fn new(state_dir: &str, started_here: bool) -> Ctx {
        Ctx { rec: Recorder::new(Some(Path::new(state_dir).join("tools.jsonl"))), state_dir: state_dir.to_string(), log_file: None, stick_log: None, started_here }
    }

    /// Write-Log: the screen and both logs.
    pub fn log(&self, line: &str) {
        println!("{line}");
        let stamp = live::now_z();
        for p in [&self.log_file, &self.stick_log].into_iter().flatten() {
            use std::io::Write;
            if let Some(d) = p.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
                let _ = writeln!(f, "{stamp} {line}");
            }
        }
    }

    fn set_stick_log(&mut self, root: &str, name: &str) {
        let p = Path::new(root).join("upgrade_").join("report").join(name);
        let _ = std::fs::create_dir_all(p.parent().unwrap());
        self.stick_log = Some(p);
        self.rec.stick_file = Some(Path::new(root).join("upgrade_").join("report").join("tools.jsonl"));
    }
}

fn write_json(path: &Path, v: &Value) -> Result<(), String> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, serde_json::to_string_pretty(v).map_err(|e| e.to_string())? + "\n").map_err(|e| format!("{}: {e}", path.display()))
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(std::fs::read_to_string(path).ok()?.trim_start_matches('\u{feff}')).ok()
}

/// Save-State.
pub fn save_state(ctx: &Ctx, st: &mut Value) {
    st["UpdatedUtc"] = json!(live::now_o());
    let _ = write_json(&Path::new(&ctx.state_dir).join("state.json"), st);
}

/// Read-State.
pub fn read_state(state_dir: &str) -> Option<Value> {
    read_json(&Path::new(state_dir).join("state.json"))
}

/// Write-Record: `upgrade_/prologue.json` on the stick, at every stage transition.
pub fn write_record(ctx: &Ctx, st: &Value, root: &str) {
    if root.is_empty() || !Path::new(root).exists() {
        return;
    }
    let rec = json!({"schema": "prologue/1", "prologue_version": FOLLOWS_PROLOGUE, "prologue_program": version_line(), "job_id": st["JobId"], "stage": st["Stage"], "updated_utc": live::now_z(), "prologue": state::block(st), "state": st});
    let _ = write_json(&Path::new(root).join("upgrade_").join("prologue.json"), &rec);
    let _ = ctx.rec.write_all(&Path::new(root).join("upgrade_").join("report").join("tools.jsonl"));
}

/// Read-Job: the job on the stick, its shape checked as the script checks it.
pub fn read_job(path: &Path) -> Result<Value, String> {
    if !path.exists() {
        return Err(format!("no job at {} - run the scanner and the job writer first", path.display()));
    }
    let j = read_json(path).ok_or_else(|| format!("{} is not JSON", path.display()))?;
    if s(&j["schema"]) != "job/1" {
        return Err(format!("job schema '{}' is not job/1; refusing", s(&j["schema"])));
    }
    for k in ["job_id", "identity", "intent", "fork", "storage", "harvest", "stick"] {
        if j.get(k).is_none() {
            return Err(format!("job.json lacks '{k}'; refusing"));
        }
    }
    Ok(j)
}

fn is_erase(job: &Value) -> bool {
    truthy(&job["erase_consent"])
}

fn gb1(bytes: f64) -> String {
    upgrade_scan::ps::num(upgrade_scan::ps::round1(bytes / GB))
}

/// Stop-Prologue: undo what this run did, record everything, write the
/// stopped outcome to the stick, scrub the stick's credentials, exit 2.
pub fn stop(ctx: &mut Ctx, st: &mut Value, root: &str, job: Option<&Value>, stopped_at: &str, reason: &str) -> ! {
    ctx.log("");
    ctx.log(&format!("  STOPPED at {stopped_at}: {reason}"));
    st["Stage"] = json!(format!("stopped:{stopped_at}"));
    if truthy(&st["Handoff"]["Armed"]) && truthy(&st["Handoff"]["EntryGuid"]) {
        let guid = s(&st["Handoff"]["EntryGuid"]);
        let _ = ctx.rec.run("bcdedit", &["/deletevalue", "{fwbootmgr}", "bootsequence"]);
        let _ = ctx.rec.run("bcdedit", &["/delete", &guid]);
        st["Handoff"]["Armed"] = json!(false);
        st["Handoff"]["Marker"] = Value::Null;
        ctx.log("  removed the one-shot boot entry");
    }
    if truthy(&st["BitLocker"]["Suspended"]) {
        let _ = ctx.rec.run("manage-bde", &["-protectors", "-enable", "C:"]);
        ctx.log("  BitLocker protection re-enabled");
    }
    if int(&st["Shrink"]["FreedBytes"]) > 0 && truthy(&st["Shrink"]["SizeBefore"]) {
        if live::grow_back(int(&st["Shrink"]["SizeBefore"])) {
            ctx.log("  C: grown back to its original size");
            st["Shrink"]["FreedBytes"] = json!(0);
        } else {
            ctx.log(&format!("  ! could not grow C: back; {} bytes remain unallocated (Disk Management can extend C: into them)", int(&st["Shrink"]["FreedBytes"])));
        }
    }
    let restored = live::restore_memory_files(&mut ctx.rec, st, false);
    for r in &restored {
        ctx.log(&format!("  {r}"));
    }
    if !root.is_empty() {
        let _ = std::fs::remove_file(Path::new(root).join("upgrade_").join("boot-install"));
    }
    live::unregister_resume_task(&mut ctx.rec);
    if let (false, Some(job)) = (root.is_empty(), job) {
        let wp = live::windows_partition().unwrap_or(Value::Null);
        let o = state::stopped_outcome(job, st, stopped_at, reason, &wp, &format!("prologue {FOLLOWS_PROLOGUE} ({})", version_line()), &live::now_z());
        // never softer: the outcome goes to the stick only as one the contract accepts; a refusal of it is said, not hidden
        match upgrade_schema::Outcome::from_value(o.clone()) {
            Ok(_) => {}
            Err(e) => ctx.log(&format!("  ! the stopped outcome does not pass the contract ({e}); written anyway so the record is not lost")),
        }
        let _ = write_json(&Path::new(root).join("upgrade_").join("outcome.json"), &o);
        let cred = Path::new(root).join("upgrade_").join("artifacts").join("credentials");
        let wifi_gone = live::remove_wifi_secrets(root);
        if let Ok(rd) = std::fs::read_dir(&cred) {
            for e in rd.flatten().filter(|e| e.path().is_file()) {
                let _ = std::fs::write(e.path(), "SCRUBBED by the prologue at a stop");
                let _ = std::fs::remove_file(e.path());
            }
        }
        ctx.log(&format!("  outcome.json (stopped) written to the stick; credentials scrubbed ({wifi_gone} Wi-Fi password file(s) removed)"));
    }
    write_record(ctx, st, root);
    let sd = Path::new(&ctx.state_dir);
    let _ = std::fs::copy(sd.join("state.json"), sd.join("state-stopped.json"));
    let _ = std::fs::remove_file(sd.join("state.json"));
    if !ctx.started_here {
        let sentence = judge::stop_sentence(&restored);
        live::show_or_queue(&ctx.state_dir, "upgrade_ - stopped", &format!("The conversion stopped at: {stopped_at}\n\n{reason}\n\n{sentence} Nothing was installed. The record is on the USB stick (upgrade_\\outcome.json)."), 600, 48);
    }
    std::process::exit(2)
}

/// Invoke-UpdateGate (RISKS R25): read, record, act. `clear` or `restart`; a stop does not return.
fn update_gate(ctx: &mut Ctx, st: &mut Value, root: &str, job: &Value, where_: &str, resume_to: Option<&str>) -> &'static str {
    if !truthy(&st["Update"]) {
        st["Update"] = json!({"Checks": [], "Restarts": 0, "ResumeTo": null});
    }
    let mut u = live::update_facts();
    u["Where"] = json!(where_);
    let pending = judge::update_pending(&u);
    u["Pending"] = json!(pending);
    st["Update"]["Checks"].as_array_mut().map(|a| a.push(u.clone()));
    let step = judge::update_step(pending, int(&st["Update"]["Restarts"]), where_, Some(UPDATE_MAX_RESTARTS));
    ctx.log(&format!("      Windows Update ({where_}): {}", if pending { format!("an update is waiting for a restart (CBS RebootPending {}, RebootInProgress {}, WU RebootRequired {})", u["CbsRebootPending"], u["CbsRebootInProgress"], u["WuRebootRequired"]) } else { "nothing is waiting for a restart".to_string() }));
    if step == "clear" {
        save_state(ctx, st);
        return "clear";
    }
    if step == "restart" {
        st["Update"]["Restarts"] = json!(int(&st["Update"]["Restarts"]) + 1);
        st["Update"]["ResumeTo"] = json!(resume_to);
        st["Restarts"] = json!(int(&st["Restarts"]) + 1);
        st["Stage"] = json!("update-restart");
        save_state(ctx, st);
        write_record(ctx, st, root);
        if let Err(e) = live::register_resume_task(&mut ctx.rec, &ctx.state_dir) {
            stop(ctx, st, root, Some(job), "windows-update", &format!("could not register the resume task ({e})"));
        }
        ctx.log(&format!("  restarting in 15 s: {}", "letting Windows finish installing an update before the conversion goes on"));
    live::restart_machine(&mut ctx.rec, "letting Windows finish installing an update before the conversion goes on");
        return "restart";
    }
    save_state(ctx, st);
    let why = if where_ == "before-arm" {
        "Windows began waiting to restart for an update after the shrink; the boot to the USB stick is not set up while it waits (RISKS R25)".to_string()
    } else {
        format!("Windows still has an update waiting for a restart after {} restart(s) to let it finish; the conversion does not interrupt it. Let Windows finish updating, then run the conversion again", int(&st["Update"]["Restarts"]))
    };
    stop(ctx, st, root, Some(job), "windows-update", &why)
}

/// Invoke-UpdateReturn: back from an update restart.
fn update_return(ctx: &mut Ctx, st: &mut Value, root: &str, job: &Value) -> &'static str {
    ctx.log("  back from the update restart; giving Windows time to finish updating");
    let deadline = live::now_local().seconds() + UPDATE_WAIT_SECONDS;
    while live::now_local().seconds() < deadline && judge::update_pending(&live::update_facts()) {
        std::thread::sleep(std::time::Duration::from_secs(20));
    }
    let resume_to = s(&st["Update"]["ResumeTo"]);
    update_gate(ctx, st, root, job, "after-update-restart", Some(&resume_to))
}

/// Invoke-VolumeStage (step 1b): `continue` or `restart`; stops on its own otherwise.
fn volume_stage(ctx: &mut Ctx, st: &mut Value, root: &str, job: &Value, f: &Value) -> &'static str {
    if s(at(job, "intent.path")) != "keep-windows" {
        ctx.log("  1b. disk check: not needed (the job does not keep Windows)");
        return "continue";
    }
    let trigger = judge::volume_trigger(&s(&f["Dirty"]), truthy(&f["RepairQueued"]));
    if trigger == "none" {
        ctx.log("  1b. disk check: not needed (C: carries no dirty flag and Windows has no repair queued)");
        return "continue";
    }
    if trigger == "unreadable" {
        stop(ctx, st, root, Some(job), "volume-check", "the volume flag on C: could not be read (fsutil answered in a form this prologue does not understand)");
    }
    if !truthy(at(job, "fork.volume_check_consented")) {
        stop(ctx, st, root, Some(job), "volume-check", "C: needs a disk check and the job carries no consent to run one");
    }
    st["VolumeCheck"]["Trigger"] = json!(trigger);
    if trigger == "dirty-flag" {
        ctx.log("  1b. C: carries the dirty flag - running the read-only online scan...");
    } else {
        ctx.log(&format!("  1b. C: carries no dirty flag, but Windows says a repair is queued ({}) - its shrink answer cannot be trusted until that check has run (R18, 2026-09-17); running the read-only online scan...", s(&f["RepairQueuedWhy"])));
    }
    let scan_started = upgrade_scan::collect::utc_to_local(upgrade_scan::collect::utc_from_seconds(upgrade_scan::collect::now().0.seconds() - 5));
    let scan = live::online_scan();
    st["VolumeCheck"]["Needed"] = json!(true);
    let ev = live::volume_evidence(scan_started);
    st["VolumeCheck"]["Evidence"] = ev.clone();
    st["VolumeCheck"]["Scan"] = json!(judge::format_scan(&scan, &ev));
    ctx.log(&format!("      online scan: {}", s(&st["VolumeCheck"]["Scan"])));
    let (disk_number, unique_id) = (int(&f["Disk"]["Number"]), s(&f["Disk"]["UniqueId"]));
    let health = live::disk_health(disk_number, &unique_id);
    let de = live::disk_events_facts(disk_number);
    st["VolumeCheck"]["DiskHealthAtCheck"] = json!(health);
    st["VolumeCheck"]["BadBlocks"] = json!(int(&de["BadBlock"]));
    ctx.log(&format!("      physical disk health: {health}; disk error log (30 days): {} bad-block, {} paging, {} reset events", int(&de["BadBlock"]), int(&de["Paging"]), int(&de["Reset"])));
    let gate = judge::disk_health_gate(&health, int(&de["BadBlock"]), truthy(&st["Ack"]["DiskHealth"]));
    st["VolumeCheck"]["Gate"] = gate["Reason"].clone();
    if !truthy(&gate["Pass"]) {
        stop(ctx, st, root, Some(job), "volume-check", &format!("C: is flagged for a disk check but the drive is not one to repair: {}. Copy your files off this computer and replace the drive. Nothing was changed.", s(&gate["Reason"])));
    }
    if s(&gate["Reason"]).starts_with("DATA LOSS ACCEPTED") {
        ctx.log(&format!("      {}", s(&gate["Reason"])));
    } else {
        ctx.log(&format!("      disk gate: {}", s(&gate["Reason"])));
    }
    let method = judge::repair_method(&scan, &s(&ev["LogVerdict"]), upgrade_scan::ps::matches("repair", &s(&ev["VolumeStatus"])), judge::ntfs98_fresh(&s(&ev["NtfsFullChkdsk"]), &s(&ev["LastCheck"])));
    if method == "refuse" {
        stop(ctx, st, root, Some(job), "volume-check", &format!("the online scan did not give a usable answer ('{scan}') and nothing in Windows' own log says what is wrong; refusing to repair on a guess"));
    }
    ctx.log(&format!("      method: {method}{}", if method == "chkdsk-f" { " (Windows logged real corruption; the full check is the only rung that clears it - files on unreadable sectors come out truncated or missing)" } else { "" }));
    let arm = match live::repair_arm(&mut ctx.rec, method) {
        Ok(a) => a,
        Err(e) => stop(ctx, st, root, Some(job), "volume-check", &e),
    };
    st["VolumeCheck"]["Method"] = json!(method);
    st["VolumeCheck"]["ArmText"] = arm["Text"].clone();
    st["VolumeCheck"]["Chkntfs"] = arm["Chkntfs"].clone();
    ctx.log(&format!("      {}", s(&arm["Text"]).replace('\n', "\n      ")));
    if !truthy(&arm["Scheduled"]) {
        stop(ctx, st, root, Some(job), "volume-check", &format!("Windows did not accept the {method} for the next restart (chkntfs says '{}')", s(&arm["Chkntfs"])));
    }
    st["VolumeCheck"]["ArmedUtc"] = json!(live::now_o());
    st["VolumeCheck"]["Restarts"] = json!(int(&st["VolumeCheck"]["Restarts"]) + 1);
    st["Restarts"] = json!(int(&st["Restarts"]) + 1);
    st["Stage"] = json!("check-armed");
    save_state(ctx, st);
    write_record(ctx, st, root);
    match live::register_resume_task(&mut ctx.rec, &ctx.state_dir) {
        Ok(acl) => {
            st["StateDirAcl"] = json!(acl);
            save_state(ctx, st);
        }
        Err(e) => stop(ctx, st, root, Some(job), "volume-check", &format!("could not register the resume task ({e}); the scheduled check will still run at the next restart, but this conversion is not continuing")),
    }
    ctx.log("      the disk check runs at the next restart; it may be slow - DO NOT interrupt it.");
    ctx.log(&format!("  restarting in 15 s: {}", "running the disk check on C:"));
    live::restart_machine(&mut ctx.rec, "running the disk check on C:");
    "restart"
}

/// Invoke-CheckReturn: back from the disk-check restart.
fn check_return(ctx: &mut Ctx, st: &mut Value, root: &str, job: &Value) -> &'static str {
    let armed_utc = s(&st["VolumeCheck"]["ArmedUtc"]);
    let o = live::check_outcome(&mut ctx.rec, &armed_utc, 120);
    st["VolumeCheck"]["Wininit1001"] = o["Wininit1001"].clone();
    st["VolumeCheck"]["Found000"] = json!(truthy(&o["Found000"]));
    st["VolumeCheck"]["DirtyAfter"] = o["Dirty"].clone();
    let trigger = s(&st["VolumeCheck"]["Trigger"]);
    let wininit = truthy(&o["Wininit1001"]);
    let dirty = s(&o["Dirty"]);
    st["VolumeCheck"]["Ran"] = json!(wininit || (trigger != "repair-queued" && dirty == "clean"));
    ctx.log(&format!("  1b. after the restart: Wininit 1001 {}; found.000 {}; C: is now {dirty}", if wininit { "recorded" } else { "NOT found" }, o["Found000"]));
    if wininit {
        ctx.log(&format!("      {}", s(&o["Wininit1001"]).split('\n').take(12).collect::<Vec<_>>().join("\n      ")));
    }
    let method = s(&st["VolumeCheck"]["Method"]);
    if trigger == "repair-queued" {
        let armed_local = live::utc_o_to_local(&armed_utc).unwrap_or_else(live::now_local);
        let after = live::volume_evidence(armed_local);
        let rqa = judge::repair_queued(&s(&after["VolumeStatus"]), "", "");
        ctx.log(&format!("      Windows after the check: volume '{}' ({}); check log {}", s(&after["VolumeStatus"]), s(&after["VolumeHealth"]), s(&after["LogVerdict"])));
        if !wininit {
            stop(ctx, st, root, Some(job), "volume-check", &format!("the {method} scheduled for Windows' queued repair did not run at the restart (no Wininit 1001); refusing to measure a volume Windows still wants to repair"));
        }
        if truthy(&rqa["Queued"]) {
            stop(ctx, st, root, Some(job), "volume-check", &format!("after {method} Windows still reports a repair queued ({}); this prologue will not escalate further", s(&rqa["Why"])));
        }
        if dirty != "clean" {
            stop(ctx, st, root, Some(job), "volume-check", &format!("after {method} the volume flag on C: reads '{dirty}'"));
        }
        return "continue";
    }
    if dirty == "clean" {
        return "continue";
    }
    if dirty != "dirty" {
        stop(ctx, st, root, Some(job), "volume-check", "after the disk check the volume flag could not be read");
    }
    if method == "chkdsk-f" || int(&st["VolumeCheck"]["Restarts"]) >= 2 {
        stop(ctx, st, root, Some(job), "volume-check", &format!("C: still carries the dirty flag after {method} ({} restart(s)); Windows needs a disk check this prologue will not escalate further", int(&st["VolumeCheck"]["Restarts"])));
    }
    let scan_started = upgrade_scan::collect::utc_to_local(upgrade_scan::collect::utc_from_seconds(upgrade_scan::collect::now().0.seconds() - 5));
    let scan = live::online_scan();
    let ev = live::volume_evidence(scan_started);
    st["VolumeCheck"]["Evidence"] = ev.clone();
    let rescan = judge::format_scan(&scan, &ev);
    st["VolumeCheck"]["Scan"] = json!(format!("{} | rescan: {rescan}", s(&st["VolumeCheck"]["Scan"])));
    ctx.log(&format!("      flag still set; rescan: {rescan}"));
    let m = judge::repair_method(&scan, &s(&ev["LogVerdict"]), upgrade_scan::ps::matches("repair", &s(&ev["VolumeStatus"])), judge::ntfs98_fresh(&s(&ev["NtfsFullChkdsk"]), &s(&ev["LastCheck"])));
    if m != "chkdsk-f" {
        if truthy(&st["Ack"]["VolumeHealth"]) {
            ctx.log("      DATA LOSS ACCEPTED: the flag survived the spot-fix and nothing names the cause; the person acknowledged the volume-health refusal, so the full check runs");
        } else {
            stop(ctx, st, root, Some(job), "volume-check", &format!("C: still carries the dirty flag after the spot-fix and neither the online scan nor Windows' own log names an error ({rescan}); refusing to run the full check on a guess"));
        }
    }
    let (n, uid) = (int(at(job, "identity.system_disk.number")), s(at(job, "identity.system_disk.unique_id")));
    let health = live::disk_health(n, &uid);
    let de = live::disk_events_facts(n);
    st["VolumeCheck"]["DiskHealthAtCheck"] = json!(health);
    st["VolumeCheck"]["BadBlocks"] = json!(int(&de["BadBlock"]));
    let gate = judge::disk_health_gate(&health, int(&de["BadBlock"]), truthy(&st["Ack"]["DiskHealth"]));
    st["VolumeCheck"]["Gate"] = gate["Reason"].clone();
    if !truthy(&gate["Pass"]) {
        stop(ctx, st, root, Some(job), "volume-check", &format!("before the full check the drive is not one to repair: {}", s(&gate["Reason"])));
    }
    let arm = match live::repair_arm(&mut ctx.rec, "chkdsk-f") {
        Ok(a) => a,
        Err(e) => stop(ctx, st, root, Some(job), "volume-check", &e),
    };
    st["VolumeCheck"]["Method"] = json!("chkdsk-f");
    st["VolumeCheck"]["ArmText"] = json!(format!("{}\n{}", s(&st["VolumeCheck"]["ArmText"]), s(&arm["Text"])));
    st["VolumeCheck"]["Chkntfs"] = arm["Chkntfs"].clone();
    if !truthy(&arm["Scheduled"]) {
        stop(ctx, st, root, Some(job), "volume-check", &format!("Windows did not accept chkdsk /f for the next restart (chkntfs says '{}')", s(&arm["Chkntfs"])));
    }
    st["VolumeCheck"]["ArmedUtc"] = json!(live::now_o());
    st["VolumeCheck"]["Restarts"] = json!(int(&st["VolumeCheck"]["Restarts"]) + 1);
    st["Restarts"] = json!(int(&st["Restarts"]) + 1);
    st["Stage"] = json!("check-armed");
    save_state(ctx, st);
    write_record(ctx, st, root);
    ctx.log("      the full disk check runs at the next restart; it may take a long time - DO NOT interrupt it.");
    ctx.log(&format!("  restarting in 15 s: {}", "running the full disk check on C:"));
    live::restart_machine(&mut ctx.rec, "running the full disk check on C:");
    "restart"
}

/// Invoke-Continue: re-measure, take the fork, shrink or stage, then arm.
fn continue_stage(ctx: &mut Ctx, st: &mut Value, root: &str, job: &Value) {
    ctx.log("  1b. re-measuring shrinkable space by both read-only paths...");
    let measure_start = upgrade_scan::collect::utc_to_local(upgrade_scan::collect::utc_from_seconds(upgrade_scan::collect::now().0.seconds() - 5));
    let m = match live::measure_shrink(&mut ctx.rec) {
        Ok(m) => m,
        Err(e) => stop(ctx, st, root, Some(job), "shrink", &format!("the shrink could not be measured: {e}")),
    };
    for k in ["PartSize", "SizeMin", "FreeBytes", "ApiError", "DiskpartGB", "DiskpartError"] {
        st["Shrink"][k] = m[k].clone();
    }
    if !m["SizeMin"].is_null() {
        st["Shrink"]["RemeasuredGB"] = json!(upgrade_scan::ps::round1((int(&m["PartSize"]) - int(&m["SizeMin"])) as f64 / GB));
        st["Shrink"]["RemeasuredBy"] = json!("storage-api");
    } else if !m["DiskpartGB"].is_null() {
        st["Shrink"]["RemeasuredGB"] = m["DiskpartGB"].clone();
        st["Shrink"]["RemeasuredBy"] = json!("diskpart");
    } else {
        st["Shrink"]["RemeasuredGB"] = Value::Null;
        st["Shrink"]["RemeasuredBy"] = Value::Null;
    }
    ctx.log(&format!("      Storage API: {};  diskpart: {};  C: free {} GB",
        if !m["SizeMin"].is_null() { format!("{} GB shrinkable", s(&st["Shrink"]["RemeasuredGB"])) } else { format!("refused - {}", s(&m["ApiError"])) },
        if !m["DiskpartGB"].is_null() { format!("{} GB", s(&m["DiskpartGB"])) } else { format!("no figure - {}", s(&m["DiskpartError"])) },
        gb1(int(&m["FreeBytes"]) as f64)));
    let linux_min = at(job, "storage.linux_min_gb").as_f64().unwrap_or(25.0);
    let files_bytes: i64 = items(at(job, "harvest.folders")).into_iter().filter(|fo| truthy(at(fo, "exists"))).map(|fo| int(at(fo, "bytes"))).sum();
    let mut fits = false;
    let mut plan = Value::Null;
    if s(at(job, "intent.path")) == "keep-windows" && !st["Shrink"]["RemeasuredGB"].is_null() {
        let size_min = if !m["SizeMin"].is_null() { int(&m["SizeMin"]) } else { (int(&m["PartSize"]) as f64 - m["DiskpartGB"].as_f64().unwrap_or(0.0) * GB) as i64 };
        plan = judge::shrink_plan(int(&m["PartSize"]), size_min, int(&m["FreeBytes"]), linux_min, files_bytes);
        st["Shrink"]["Plan"] = plan.clone();
        fits = truthy(&plan["Fits"]);
        ctx.log(&format!("      plan: Linux needs {} GB (linux_min {} GB + files {} GB x {FILES_MARGIN}); shrinkable {} GB; Windows keeps {} GB free -> {}", gb1(int(&plan["TargetBytes"]) as f64), upgrade_scan::ps::num(linux_min), upgrade_scan::ps::fmt_n(files_bytes as f64 / GB, 2), gb1(int(&plan["ShrinkableBytes"]) as f64), (WINDOWS_KEEP_FREE_BYTES as f64 / GB).round(), s(&plan["Reason"])));
        if !fits {
            let lu = live::last_unmovable(measure_start).unwrap_or_default();
            if !lu.is_empty() {
                st["Shrink"]["LastUnmovable"] = json!(lu);
                ctx.log(&format!("      Windows names the last unmovable file: {lu}"));
            }
            let rp = judge::restore_point_step(fits, &lu, truthy(at(job, "fork.restore_points_consented")), truthy(&st["Shrink"]["RestorePoints"]));
            match rp {
                "delete" => {
                    ctx.log("      that is System Restore's storage; the job consents - deleting Windows' restore points on C: (this cannot be undone)");
                    let r = live::delete_restore_points(&mut ctx.rec);
                    ctx.log(&format!("      {}", judge::format_restore_points(&r)));
                    st["Shrink"]["RestorePoints"] = r;
                    save_state(ctx, st);
                    write_record(ctx, st, root);
                    return continue_stage(ctx, st, root, job);
                }
                "no-consent" => ctx.log("      that is System Restore's storage; the job carries no consent to delete restore points - left alone"),
                "already-done" => ctx.log(&format!("      deleting restore points was already tried in this run ({}) and their storage is still named - nothing more to try there", judge::format_restore_points(&st["Shrink"]["RestorePoints"]))),
                _ => {}
            }
            let uj_state = &st["Shrink"]["UsnJournal"];
            let done_this_boot = truthy(uj_state) && !uj_state["LastRestarts"].is_null() && int(&uj_state["LastRestarts"]) == int(&st["Restarts"]);
            let uj = judge::usn_journal_step(fits, &lu, truthy(at(job, "fork.usn_journal_consented")), done_this_boot);
            match uj {
                "delete" => {
                    ctx.log("      that is NTFS's change journal; the job consents - deleting it (Windows' record of recent file changes, not a file; it is created again afterwards)");
                    let r = live::delete_usn_journal(&mut ctx.rec, st);
                    ctx.log(&format!("      change journal: {}; was {}; fsutil: {}", if int(&r["ExitCode"]) == 0 && !r["ExitCode"].is_null() { "deleted".to_string() } else { format!("fsutil exit {}", s(&r["ExitCode"])) }, if truthy(&r["Before"]["Active"]) { format!("{} MB max", upgrade_scan::ps::num(upgrade_scan::ps::round1(int(&r["Before"]["MaxBytes"]) as f64 / 1048576.0))) } else { "not active".to_string() }, s(&r["Text"]).split('\n').next_back().unwrap_or("").trim()));
                    save_state(ctx, st);
                    write_record(ctx, st, root);
                    return continue_stage(ctx, st, root, job);
                }
                "no-consent" => ctx.log("      that is NTFS's change journal; the job carries no consent to delete it - left alone"),
                "already-done" => ctx.log("      the change journal was already deleted since the last restart and is still named - nothing more to try there"),
                _ => {}
            }
        }
        if !fits && !truthy(&st["Shrink"]["Mitigated"]) && int(&plan["TargetBytes"]) > int(&plan["ShrinkableBytes"]) {
            ctx.log("      does not fit cold; disabling the pagefile and restarting once to re-measure");
            if !truthy(&st["Shrink"]["Before"]) {
                st["Shrink"]["Before"] = live::memory_files_before();
                save_state(ctx, st);
            }
            st["Shrink"]["HibernationDisabled"] = json!(live::hibernation_off(&mut ctx.rec));
            st["Shrink"]["PagefileDisabled"] = json!(live::pagefile_off());
            st["Shrink"]["Mitigated"] = json!(true);
            st["Restarts"] = json!(int(&st["Restarts"]) + 1);
            st["Stage"] = json!("mitigated");
            save_state(ctx, st);
            write_record(ctx, st, root);
            if let Err(e) = live::register_resume_task(&mut ctx.rec, &ctx.state_dir) {
                stop(ctx, st, root, Some(job), "shrink", &format!("could not register the resume task ({e})"));
            }
            ctx.log(&format!("  restarting in 15 s: {}", "freeing space on C: for the measurement"));
    live::restart_machine(&mut ctx.rec, "freeing space on C: for the measurement");
            return;
        }
    }
    let fork = judge::fork(&s(at(job, "intent.path")), fits, &s(at(job, "fork.if_cannot_keep")), &s(at(job, "intent.path_reason")));
    st["Shrink"]["ForkTaken"] = json!(fork);
    ctx.log(&format!("      fork: {fork} (job path {}, if_cannot_keep {})", s(at(job, "intent.path")), s(at(job, "fork.if_cannot_keep"))));
    if fork == "stop" {
        save_state(ctx, st);
        let measured = if !st["Shrink"]["RemeasuredGB"].is_null() { format!("{} GB", s(&st["Shrink"]["RemeasuredGB"])) } else { "no figure".to_string() };
        let needs = if truthy(&plan) { int(&plan["TargetBytes"]) as f64 } else { linux_min * GB };
        let why = if truthy(&plan) { format!(" ({})", s(&plan["Reason"])) } else { String::new() };
        stop(ctx, st, root, Some(job), "shrink", &format!("re-measured {measured} shrinkable; Linux needs {} GB; you chose to stop rather than give up Windows{why}", gb1(needs)));
    }
    if update_gate(ctx, st, root, job, "before-shrink", Some("continue")) == "restart" {
        return;
    }
    if fork == "keep-windows" {
        ctx.log("  2.  keep Windows: hibernation off, then the shrink");
        if !truthy(&st["Shrink"]["Before"]) {
            st["Shrink"]["Before"] = live::memory_files_before();
            save_state(ctx, st);
        }
        st["Shrink"]["HibernationDisabled"] = json!(live::hibernation_off(&mut ctx.rec));
        let requested = int(&plan["RequestedBytes"]);
        st["Shrink"]["RequestedBytes"] = json!(requested);
        match live::shrink(requested) {
            Ok(r) => {
                st["Shrink"]["SizeBefore"] = r["SizeBefore"].clone();
                st["Shrink"]["FreedBytes"] = r["Freed"].clone();
                ctx.log(&format!("      Resize-Partition: C: {} GB -> {} GB, freed {} GB", gb1(int(&r["SizeBefore"]) as f64), gb1(int(&r["SizeAfter"]) as f64), gb1(int(&r["Freed"]) as f64)));
            }
            Err(e) => {
                save_state(ctx, st);
                stop(ctx, st, root, Some(job), "shrink", &format!("Resize-Partition refused: {e}"));
            }
        }
        if int(&st["Shrink"]["FreedBytes"]) < requested {
            save_state(ctx, st);
            stop(ctx, st, root, Some(job), "shrink", &format!("the shrink freed {} bytes, less than the {requested} planned", int(&st["Shrink"]["FreedBytes"])));
        }
        if judge::restore_plan(&st["Shrink"], true).contains(&"usn-journal") {
            let line = live::recreate_usn_journal(&mut ctx.rec, st);
            ctx.log(&format!("      {line}"));
        }
        save_state(ctx, st);
        write_record(ctx, st, root);
    } else {
        ctx.log("  2.  clean slate: staging your files to the stick");
        let n_folders = items(at(job, "harvest.folders")).into_iter().filter(|fo| truthy(at(fo, "exists")) && truthy(at(fo, "path"))).count() as i64;
        if let Some(why) = judge::stage_refusal(n_folders, None) {
            save_state(ctx, st);
            stop(ctx, st, root, Some(job), "stage-files", &why);
        }
        let mut lines = Vec::new();
        let staged = live::stage(job, root, &mut |l| lines.push(l));
        for l in &lines {
            ctx.log(l);
        }
        st["Staged"] = staged.clone();
        save_state(ctx, st);
        write_record(ctx, st, root);
        if truthy(&staged["Error"]) {
            stop(ctx, st, root, Some(job), "stage-files", &s(&staged["Error"]));
        }
        if int(&staged["Failed"]) > 0 {
            stop(ctx, st, root, Some(job), "stage-files", &format!("{} file(s) could not be staged to the stick", int(&staged["Failed"])));
        }
        if let Some(why) = judge::stage_refusal(n_folders, Some(int(&staged["Files"]))) {
            stop(ctx, st, root, Some(job), "stage-files", &why);
        }
        ctx.log(&format!("      staged {} files, {} GB, checksums in {}", int(&staged["Files"]), upgrade_scan::ps::fmt_n(int(&staged["Bytes"]) as f64 / GB, 2), s(&staged["Manifest"])));
        stop(ctx, st, root, Some(job), "confirm", &format!("clean slate needs the live session's two-minute human check before the wipe, and that gate is not built in this version; the prologue will not arm an unattended wipe. {} file(s), {} GB, are staged on the stick with checksums; Windows is untouched.", int(&staged["Files"]), upgrade_scan::ps::fmt_n(int(&staged["Bytes"]) as f64 / GB, 2)));
    }
    arm(ctx, st, root, job);
}

/// Invoke-EraseContinue (RISKS R27): nothing changes here; the installer erases.
fn erase_continue(ctx: &mut Ctx, st: &mut Value, root: &str, job: &Value) {
    st["Shrink"]["ForkTaken"] = json!("clean-slate");
    let names: Vec<String> = items(at(job, "erase_consent.disks")).into_iter().map(|d| format!("{} ({})", s(at(d, "friendly_name")), s(at(d, "role")))).collect();
    ctx.log(&format!("  2.  erase and install: nothing is changed here. In the installer a 2-minute countdown comes first; when it ends, {} are erased.", names.join(" and ")));
    save_state(ctx, st);
    write_record(ctx, st, root);
    arm(ctx, st, root, job);
}

/// Invoke-Arm (step 4): suspend BitLocker, arm the handoff, restart into the installer.
fn arm(ctx: &mut Ctx, st: &mut Value, root: &str, job: &Value) {
    let erase = is_erase(job);
    update_gate(ctx, st, root, job, "before-arm", None);
    ctx.log("  4.  arming the one-shot boot handoff");
    let blq = live::bitlocker_state(&mut ctx.rec);
    st["BitLocker"]["StatusBefore"] = blq["State"].clone();
    st["BitLocker"]["Source"] = blq["Source"].clone();
    if s(&blq["State"]) == "unknown" {
        save_state(ctx, st);
        let raw = if truthy(&blq["Raw"]) { format!(" (manage-bde said: {})", s(&blq["Raw"])) } else { String::new() };
        stop(ctx, st, root, Some(job), "arm-handoff", &format!("BitLocker state on C: could not be determined{raw}; refusing to arm a boot that might stop at a recovery-key prompt"));
    }
    let report = Path::new(root).join("upgrade_").join("report");
    let _ = std::fs::create_dir_all(&report);
    let backup = Path::new(&ctx.state_dir).join("bcd-backup.bin");
    let _ = std::fs::remove_file(&backup);
    let bp = backup.to_string_lossy().to_string();
    let ex = ctx.rec.run("bcdedit", &["/export", &bp]).clone();
    if !ex.ok() || !backup.exists() {
        save_state(ctx, st);
        stop(ctx, st, root, Some(job), "arm-handoff", "bcdedit /export failed; refusing to arm without a backup");
    }
    let _ = std::fs::copy(&backup, report.join("bcd-backup.bin"));
    st["Handoff"]["BcdBackup"] = json!("upgrade_/report/bcd-backup.bin");
    st["Handoff"]["Before"] = live::fwbootmgr_snapshot(&mut ctx.rec);
    if s(&blq["State"]) == "on" {
        let r = ctx.rec.run("manage-bde", &["-protectors", "-disable", "C:", "-rebootcount", "1"]).clone();
        if !r.ok() {
            save_state(ctx, st);
            stop(ctx, st, root, Some(job), "arm-handoff", &format!("manage-bde could not suspend BitLocker: {}", r.lines().join(" ")));
        }
        st["BitLocker"]["Suspended"] = json!(true);
        st["BitLocker"]["RebootCount"] = json!(1);
        ctx.log("      BitLocker suspended for one restart");
    }
    let up = Path::new(root).join("upgrade_");
    let _ = std::fs::remove_file(up.join("boot-verify"));
    // a stick that converted a computer before says so and never installs (R35); arming a new job is the one thing that clears it
    let _ = std::fs::remove_file(up.join("converted"));
    let _ = std::fs::write(up.join("boot-install"), format!("prologue {FOLLOWS_PROLOGUE} ({}) job {}\n", version_line(), s(&st["JobId"])));
    st["Handoff"]["Marker"] = json!("boot-install");
    st["Handoff"]["GrubEnvReset"] = json!(live::reset_grub_env(root));
    let copy = ctx.rec.run("bcdedit", &["/copy", "{bootmgr}", "/d", "upgrade_"]).clone();
    let guid = upgrade_scan::ps::capture(r"(\{[0-9a-fA-F-]{36}\})", "", &copy.text()).map(str::to_string);
    let Some(guid) = guid.filter(|_| copy.ok()) else {
        save_state(ctx, st);
        stop(ctx, st, root, Some(job), "arm-handoff", &format!("bcdedit /copy failed: {}", copy.text().trim()));
    };
    let _ = ctx.rec.run("bcdedit", &["/set", &guid, "device", &format!("partition={}", root.trim_end_matches('\\'))]);
    let _ = ctx.rec.run("bcdedit", &["/set", &guid, "path", live::PAYLOAD_EFI]);
    let seq = ctx.rec.run("bcdedit", &["/set", "{fwbootmgr}", "bootsequence", &guid]).clone();
    if !seq.ok() {
        let _ = ctx.rec.run("bcdedit", &["/delete", &guid]);
        save_state(ctx, st);
        stop(ctx, st, root, Some(job), "arm-handoff", "setting bootsequence failed; the entry was removed again");
    }
    st["Handoff"]["Armed"] = json!(true);
    st["Handoff"]["EntryGuid"] = json!(guid);
    st["Handoff"]["ArmedUtc"] = json!(live::now_z());
    st["Stage"] = json!("armed");
    st["Restarts"] = json!(int(&st["Restarts"]) + 1);
    save_state(ctx, st);
    write_record(ctx, st, root);
    if let Err(e) = live::register_resume_task(&mut ctx.rec, &ctx.state_dir) {
        stop(ctx, st, root, Some(job), "arm-handoff", &format!("could not register the return check ({e}); the boot entry was removed again"));
    }
    ctx.log(&format!("      armed: entry {guid} -> {root}{}, marker boot-install", live::PAYLOAD_EFI.trim_start_matches('\\')));
    ctx.log("");
    ctx.log("  This computer restarts into the installer in 15 seconds. Leave the stick in.");
    if erase {
        ctx.log("  In the installer a 2-minute countdown comes first. Press any key during it to cancel and come back to this Windows, untouched. When it ends, everything is erased and Fedora is installed.");
    } else {
        ctx.log("  Windows is still here and still bootable; it stays that way until you reclaim it in Linux.");
    }
    ctx.log(&format!("  restarting in 15 s: {}", "starting the installer from the USB stick"));
    live::restart_machine(&mut ctx.rec, "starting the installer from the USB stick");
    if !ctx.started_here && !truthy(&live::live_resume_context()["Unattended"]) {
        live::show_popup("Restarting into the installer in 15 seconds. Leave the USB stick in.", "upgrade_", 12, 0);
    }
}

/// Invoke-Return: Windows is back after the handoff. Classify, clean up, record, leave.
fn return_stage(ctx: &mut Ctx, st: &mut Value, root: &str) {
    ctx.log("  return: Windows is back after the handoff");
    if !root.is_empty() {
        ctx.log(&format!("  {} Wi-Fi password file(s) removed from the stick", live::remove_wifi_secrets(root)));
    }
    let (mut fired, mut via) = (false, Vec::new());
    if !root.is_empty() {
        let ge = Path::new(root).join(live::GRUB_ENV_REL);
        if ge.exists() && judge::grub_env_fired(&std::fs::read(&ge).unwrap_or_default()) {
            fired = true;
            via.push("grubenv");
        }
        if Path::new(root).join("upgrade_").join("outcome.json").exists() {
            via.push("outcome.json");
        }
    }
    let after = live::fwbootmgr_snapshot(&mut ctx.rec);
    let cleared = s(&after["BootSequence"]).trim().is_empty();
    let guid = s(&st["Handoff"]["EntryGuid"]);
    let after_text = s(&after["DisplayOrder"]);
    let after_tokens: Vec<&str> = after_text.split_whitespace().filter(|t| *t != guid).collect();
    let before_text = s(&st["Handoff"]["Before"]["DisplayOrder"]);
    let before_tokens: Vec<&str> = before_text.split_whitespace().collect();
    let unchanged = after_tokens.join(" ") == before_tokens.join(" ");
    let result = judge::handoff_result(fired, cleared, unchanged);
    ctx.log(&format!("      fired={fired} ({}) sequence_cleared={cleared} order_unchanged={unchanged} -> {result}", via.join("+")));
    live::unregister_resume_task(&mut ctx.rec);
    if !guid.is_empty() {
        let _ = ctx.rec.run("bcdedit", &["/delete", &guid]);
    }
    if !cleared {
        let _ = ctx.rec.run("bcdedit", &["/deletevalue", "{fwbootmgr}", "bootsequence"]);
    }
    if !root.is_empty() {
        let _ = std::fs::remove_file(Path::new(root).join("upgrade_").join("boot-install"));
        live::reset_grub_env(root);
    }
    // the kept Windows stays a normal Windows: the pagefile comes back; hibernation stays off (the volume must be mountable from Linux)
    for r in live::restore_memory_files(&mut ctx.rec, st, true) {
        ctx.log(&format!("      {r}"));
    }
    let bl_now = s(&live::bitlocker_state(&mut ctx.rec)["State"]);
    st["Return"] = json!({"ReturnedUtc": live::now_z(), "Fired": fired, "FiredVia": via.join("+"), "SequenceCleared": cleared, "OrderUnchanged": unchanged, "Result": result, "DisplayOrderAfter": after["DisplayOrder"], "BitLockerNow": bl_now});
    st["Stage"] = json!("returned");
    save_state(ctx, st);
    if !root.is_empty() {
        let rec = json!({"schema": "prologue-return/1", "prologue_version": FOLLOWS_PROLOGUE, "prologue_program": version_line(), "job_id": st["JobId"], "handoff": st["Return"], "secure_boot": live::secure_boot()});
        let _ = write_json(&Path::new(root).join("upgrade_").join("prologue-return.json"), &rec);
        write_record(ctx, st, root);
    }
    let sd = Path::new(&ctx.state_dir);
    let _ = std::fs::copy(sd.join("state.json"), sd.join("state-returned.json"));
    let _ = std::fs::remove_file(sd.join("state.json"));
    ctx.log("      boot entry removed, task removed; record on the stick");
    // the erase path (R27): Windows is back, so nothing was erased - record why
    let erase_job = if root.is_empty() { None } else { read_job(&Path::new(root).join("upgrade_").join("job.json")).ok().filter(|j| is_erase(j) && s(&j["job_id"]) == s(&st["JobId"])) };
    if let Some(j) = erase_job {
        let rd = Path::new(root).join("upgrade_").join("report");
        let cd = read_json(&rd.join("countdown.json")).unwrap_or(Value::Null);
        let vf = read_json(&rd.join("verify.json")).unwrap_or(Value::Null);
        let er = judge::erase_return(&cd, &vf);
        let o = state::stopped_outcome(&j, st, &s(&er["StoppedAt"]), &s(&er["Reason"]), &Value::Null, &format!("prologue {FOLLOWS_PROLOGUE} ({})", version_line()), &live::now_z());
        let _ = write_json(&Path::new(root).join("upgrade_").join("outcome.json"), &o);
        ctx.log(&format!("  STOPPED at {}: {}", s(&er["StoppedAt"]), s(&er["Reason"])));
        live::show_or_queue(&ctx.state_dir, "upgrade_ - nothing was erased", &format!("Windows is back and nothing was erased.\n\n{}.\n\nThe record is on the USB stick (upgrade_\\outcome.json).", s(&er["Reason"])), 300, 64);
        return;
    }
    live::show_or_queue(&ctx.state_dir, "upgrade_ - back in Windows", &format!("The one-time boot entry has been removed (handoff: {result}).\n\nIf the conversion completed, Linux is the first boot choice and Windows is in its menu. The record is on the USB stick."), 120, 64);
}

/// What `start` takes from the launcher.
pub struct StartArgs {
    pub stick_drive: String,
    pub job_path: Option<String>,
    pub confirm_word: String,
    pub acknowledge_data_loss: String,
    pub erase_consent: String,
}

/// Invoke-StartPhase. An `Err` is what the script would have thrown (and
/// the caller scrubs the stick's Wi-Fi files, as the script does).
pub fn start(ctx: &mut Ctx, a: &StartArgs) -> Result<(), String> {
    let state = ctx.state_dir.clone();
    std::fs::create_dir_all(&state).map_err(|e| e.to_string())?;
    if Path::new(&state).join("state.json").exists() {
        return Err(format!("a conversion is already in progress (state in {state}). Restart to let it resume, or run abort."));
    }
    let root = judge::drive_root(&a.stick_drive)?;
    if !Path::new(&root).exists() {
        return Err(format!("stick {root} not found"));
    }
    let job_file = a.job_path.clone().map(PathBuf::from).unwrap_or_else(|| Path::new(&root).join("upgrade_").join("job.json"));
    let job = read_job(&job_file)?;
    if let Some(r) = judge::erase_start_refusal(&job, &a.confirm_word, &a.erase_consent) {
        return Err(r);
    }
    let erase = is_erase(&job);
    let job_ack = truthy(&job["risk_acknowledgement"]);
    if job_ack {
        if s(at(&job, "risk_acknowledgement.statement")) != RISK_STATEMENT {
            return Err("the job carries a risk acknowledgement whose statement is not the one this prologue knows; refusing".into());
        }
        if a.acknowledge_data_loss != RISK_STATEMENT {
            return Err("the job carries a data-loss acknowledgement but the statement was not typed for this run (-AcknowledgeDataLoss); refusing".into());
        }
    } else if !a.acknowledge_data_loss.is_empty() {
        return Err("a data-loss statement was given but the job carries no acknowledgement; use the normal launcher".into());
    }
    if !Path::new(&root).join(live::PAYLOAD_EFI.trim_start_matches('\\')).exists() {
        return Err(format!("no payload at {root}{} - this is not the kit stick", live::PAYLOAD_EFI.trim_start_matches('\\')));
    }
    if !Path::new(&root).join("upgrade_").join("ks.cfg").exists() {
        return Err("no kickstart on the stick (upgrade_\\ks.cfg) - run the generator first".into());
    }
    let report = Path::new(&root).join("upgrade_").join("report");
    let _ = std::fs::remove_dir_all(&report);
    let _ = std::fs::create_dir_all(&report);
    let log = Path::new(&state).join("prologue.log");
    let _ = std::fs::remove_file(&log);
    let _ = std::fs::remove_file(Path::new(&state).join("tools.jsonl"));
    ctx.log_file = Some(log);
    ctx.set_stick_log(&root, "prologue.log");
    ctx.log("");
    ctx.log(&format!("  upgrade_  prologue {FOLLOWS_PROLOGUE}  -  START  ({})", version_line()));
    ctx.log(&format!("  job {}   path {}   desktop {}   stick {root}", s(&job["job_id"]), s(at(&job, "intent.path")), s(at(&job, "intent.desktop"))));
    let f = live::facts(&mut ctx.rec, &root)?;
    let mut st = state::new_state(&s(&job["job_id"]), &s(&f["Stick"]["VolumeId"]), &root, FOLLOWS_PROLOGUE, &live::now_o());
    if job_ack {
        let ov: Vec<String> = items(at(&job, "risk_acknowledgement.overrides")).into_iter().map(s).collect();
        st["Ack"] = json!({"Present": true, "DiskHealth": ov.iter().any(|x| x == "disk-health"), "VolumeHealth": ov.iter().any(|x| x == "volume-health"), "AcceptedUtc": s(at(&job, "risk_acknowledgement.accepted_utc"))});
        ctx.log(&format!("  DATA LOSS ACCEPTED (typed {}): this run lifts {}. Files on this machine may be lost.", s(at(&job, "risk_acknowledgement.accepted_utc")), ov.join(", ")));
    }
    st["Facts"] = json!({"vendor": f["Vendor"], "model": f["Model"], "os": format!("{} {}", s(&f["OsCaption"]), s(&f["OsBuild"])), "secure_boot": f["SecureBoot"], "disk": f["Disk"], "health": f["Health"], "dirty_at_start": f["Dirty"], "bitlocker": f["BitLocker"], "bitlocker_via": f["BitLockerSource"], "hiberfil": f["Hiberfil"], "pagefile": f["Pagefile"]});
    ctx.log(&format!("  {} {}   {} {}   Secure Boot {}   BitLocker {} (via {})   disk health {}   C: {}{}{}", s(&f["Vendor"]), s(&f["Model"]), s(&f["OsCaption"]), s(&f["OsBuild"]), s(&f["SecureBoot"]), s(&f["BitLocker"]), s(&f["BitLockerSource"]), s(&f["Health"]), s(&f["Dirty"]),
        if truthy(&f["RepairQueued"]) { format!(" (repair queued: {})", s(&f["RepairQueuedWhy"])) } else { String::new() },
        if truthy(&f["RepairStale"]) { format!(" ({} - not a queued repair)", s(&f["RepairStale"])) } else { String::new() }));
    save_state(ctx, &mut st);
    ctx.log("  1.  re-validating job.json against this machine...");
    let mut mm = judge::compare_job(&job, &f);
    if erase {
        mm.extend(judge::compare_erase_disks(&job, &f));
    }
    st["Mismatches"] = json!(mm);
    if !mm.is_empty() {
        for x in &mm {
            ctx.log(&format!("      ! {x}"));
        }
        save_state(ctx, &mut st);
        stop(ctx, &mut st, &root, Some(&job), "revalidate", &format!("job.json no longer matches this machine: {}", mm.join("; ")));
    }
    ctx.log("      matches: disk identity, firmware, Secure Boot, stick, BitLocker, volume flag, disk health");
    save_state(ctx, &mut st);
    write_record(ctx, &st, &root);
    if erase {
        ctx.log(&format!("  ERASE AND INSTALL (typed {}): every drive named in the job is erased in the installer, after its countdown.", s(at(&job, "erase_consent.accepted_utc"))));
        if update_gate(ctx, &mut st, &root, &job, "before-changes", Some("erase")) == "restart" {
            return Ok(());
        }
        erase_continue(ctx, &mut st, &root, &job);
        return Ok(());
    }
    if update_gate(ctx, &mut st, &root, &job, "before-changes", Some("volume")) == "restart" {
        return Ok(());
    }
    if volume_stage(ctx, &mut st, &root, &job, &f) == "restart" {
        return Ok(());
    }
    continue_stage(ctx, &mut st, &root, &job);
    Ok(())
}

/// Invoke-ResumePhase: the SYSTEM task at startup, or a person by hand.
pub fn resume(ctx: &mut Ctx) -> Result<(), String> {
    let state = ctx.state_dir.clone();
    let Some(mut st) = read_state(&state) else {
        println!("  nothing to resume (no state in {state})");
        live::unregister_resume_task(&mut ctx.rec);
        return Ok(());
    };
    ctx.log_file = Some(Path::new(&state).join("prologue.log"));
    let mut cx = live::live_resume_context();
    let started = live::now_local().seconds();
    cx["Utc"] = json!(live::now_o());
    cx["Stage"] = st["Stage"].clone();
    cx["UptimeSeconds"] = json!(live::uptime_seconds());
    let root = live::wait_stick(&st, live::STICK_WAIT_SECONDS).unwrap_or_default();
    cx["StickWaitSeconds"] = json!(live::now_local().seconds() - started);
    if st["Resumes"].as_array().is_none() {
        st["Resumes"] = json!([]);
    }
    st["Resumes"].as_array_mut().map(|a| a.push(cx.clone()));
    save_state(ctx, &mut st);
    if !root.is_empty() {
        ctx.set_stick_log(&root, "prologue.log");
    }
    ctx.log("");
    ctx.log(&format!("  upgrade_  prologue {FOLLOWS_PROLOGUE}  -  RESUME (stage {}, restart {})  ({})", s(&st["Stage"]), int(&st["Restarts"]), version_line()));
    ctx.log(&format!("  running as {}, session {}, interactive {}, explorer {}, uptime {} s, stick after {} s -> {}", s(&cx["RunAs"]), int(&cx["SessionId"]), cx["Interactive"], cx["ExplorerRunning"], int(&cx["UptimeSeconds"]), int(&cx["StickWaitSeconds"]), if truthy(&cx["Unattended"]) { "unattended" } else { "attended" }));
    let stage = s(&st["Stage"]);
    if root.is_empty() && stage == "probe-armed" {
        probe_return(ctx, &mut st, "");
        return Ok(());
    }
    if root.is_empty() {
        ctx.log(&format!("  ! the USB stick is not present after {} s; leaving the task in place", live::STICK_WAIT_SECONDS));
        live::show_or_queue(&state, "upgrade_", "The USB stick is not plugged in. Plug it in and restart the computer - the conversion continues by itself.", 300, 48);
        return Ok(());
    }
    if stage == "armed" {
        return_stage(ctx, &mut st, &root);
        return Ok(());
    }
    if stage == "probe-armed" {
        probe_return(ctx, &mut st, &root);
        return Ok(());
    }
    let job = read_job(&Path::new(&root).join("upgrade_").join("job.json"))?;
    if s(&job["job_id"]) != s(&st["JobId"]) {
        let why = format!("the job on the stick ({}) is not the one this conversion started with ({})", s(&job["job_id"]), s(&st["JobId"]));
        stop(ctx, &mut st, &root, Some(&job), "revalidate", &why);
    }
    match stage.as_str() {
        "check-armed" => {
            if check_return(ctx, &mut st, &root, &job) == "restart" {
                return Ok(());
            }
        }
        "mitigated" => ctx.log("  back from the pagefile restart"),
        "update-restart" => {
            if update_return(ctx, &mut st, &root, &job) == "restart" {
                return Ok(());
            }
            if s(&st["Update"]["ResumeTo"]) == "volume" {
                let f = live::facts(&mut ctx.rec, &root)?;
                if volume_stage(ctx, &mut st, &root, &job, &f) == "restart" {
                    return Ok(());
                }
            }
        }
        other => return Err(format!("state is at stage '{other}', which resume does not continue from")),
    }
    if s(&st["Update"]["ResumeTo"]) == "erase" {
        erase_continue(ctx, &mut st, &root, &job);
        return Ok(());
    }
    continue_stage(ctx, &mut st, &root, &job);
    Ok(())
}

/// Invoke-ProbeStart: the walk-away probe (read-only, one restart).
pub fn probe_start(ctx: &mut Ctx, stick_drive: &str) -> Result<(), String> {
    let state = ctx.state_dir.clone();
    std::fs::create_dir_all(&state).map_err(|e| e.to_string())?;
    if Path::new(&state).join("state.json").exists() {
        return Err(format!("a conversion or probe is already in progress (state in {state}). Restart to let it resume, or run abort."));
    }
    let root = judge::drive_root(stick_drive)?;
    if !Path::new(&root).exists() {
        return Err(format!("stick {root} not found"));
    }
    let _ = std::fs::create_dir_all(Path::new(&root).join("upgrade_").join("report"));
    let log = Path::new(&state).join("prologue.log");
    let _ = std::fs::remove_file(&log);
    ctx.log_file = Some(log);
    ctx.set_stick_log(&root, "probe.log");
    ctx.log("");
    ctx.log(&format!("  upgrade_  prologue {FOLLOWS_PROLOGUE}  -  WALK-AWAY PROBE (read-only, one restart)  ({})", version_line()));
    let f = live::facts(&mut ctx.rec, &root)?;
    if !truthy(&f["Stick"]) {
        return Err(format!("the stick's identity could not be read ({}); the probe needs it to find the stick again after the restart", s(&f["StickError"])));
    }
    let mut st = state::new_state("probe", &s(&f["Stick"]["VolumeId"]), &root, FOLLOWS_PROLOGUE, &live::now_o());
    st["Stage"] = json!("probe-armed");
    st["Facts"] = json!({"vendor": f["Vendor"], "model": f["Model"], "bios": f["BiosVersion"], "os": format!("{} {}", s(&f["OsCaption"]), s(&f["OsBuild"])), "secure_boot": f["SecureBoot"], "stick_bus": f["Stick"]["Bus"]});
    ctx.log(&format!("  {} {}   BIOS {}   {} {}   Secure Boot {}   stick on {} as {root}", s(&f["Vendor"]), s(&f["Model"]), s(&f["BiosVersion"]), s(&f["OsCaption"]), s(&f["OsBuild"]), s(&f["SecureBoot"]), s(&f["Stick"]["Bus"])));
    save_state(ctx, &mut st);
    let acl = live::register_resume_task(&mut ctx.rec, &state)?;
    st["StateDirAcl"] = json!(acl);
    st["Restarts"] = json!(1);
    save_state(ctx, &mut st);
    ctx.log("  the SYSTEM startup task is registered; the state directory is locked; restarting.");
    ctx.log("  Do NOT sign in when Windows comes back - leave it at the sign-in screen for two minutes. The record lands on the stick by itself.");
    ctx.log(&format!("  restarting in 15 s: {}", "the walk-away probe"));
    live::restart_machine(&mut ctx.rec, "the walk-away probe");
    Ok(())
}

/// Invoke-ProbeReturn: back from the probe's restart. Record, clean up, leave.
fn probe_return(ctx: &mut Ctx, st: &mut Value, root: &str) {
    ctx.log("  probe: back after the restart");
    let cx = items(&st["Resumes"]).last().cloned().cloned().unwrap_or(Value::Null);
    let removed = live::unregister_resume_task(&mut ctx.rec);
    let result = judge::probe_result(truthy(&cx["Unattended"]), !root.is_empty(), removed);
    let facts = if truthy(&st["Facts"]) { st["Facts"].clone() } else { json!({}) };
    let notice = live::show_or_queue(&ctx.state_dir, "upgrade_ - walk-away probe", &format!("The walk-away probe finished: {result}.\n\nThe resume ran as {} in session {}, {} s after boot; the stick appeared after {} s.\n\nNothing on this computer was changed. The row is on the USB stick (upgrade_\\walkaway-probe.csv).", s(&cx["RunAs"]), int(&cx["SessionId"]), int(&cx["UptimeSeconds"]), int(&cx["StickWaitSeconds"])), 600, 64);
    st["Stage"] = json!(format!("probe-done:{result}"));
    st["Return"] = json!({"ReturnedUtc": live::now_z(), "Result": result, "Notice": notice, "TaskRemoved": removed});
    save_state(ctx, st);
    let row: Vec<Value> = vec![json!(live::now_z()), json!(FOLLOWS_PROLOGUE), facts["vendor"].clone(), facts["model"].clone(), facts["bios"].clone(), facts["os"].clone(), facts["secure_boot"].clone(), facts["stick_bus"].clone(),
        cx["RunAs"].clone(), cx["SessionId"].clone(), cx["Interactive"].clone(), cx["ExplorerRunning"].clone(), cx["UptimeSeconds"].clone(), cx["StickWaitSeconds"].clone(), json!(notice), json!(removed), json!(result),
        json!(format!("state stage={}; resume stage={}; resumed at {}; {}", s(&st["Stage"]), s(&cx["Stage"]), s(&cx["Utc"]), version_line()))];
    if !root.is_empty() {
        let csv = Path::new(root).join("upgrade_").join("walkaway-probe.csv");
        if !csv.exists() {
            let header: Vec<Value> = crate::PROBE_CSV_HEADER.iter().map(|h| json!(h)).collect();
            let _ = std::fs::write(&csv, judge::probe_csv_line(&header) + "\n");
        }
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(&csv) {
            let _ = writeln!(f, "{}", judge::probe_csv_line(&row));
        }
        let rec = json!({"schema": "walkaway-probe/1", "prologue_version": FOLLOWS_PROLOGUE, "prologue_program": version_line(), "result": result, "facts": facts, "resume": cx, "notice": notice, "task_removed": removed, "state": st});
        let _ = write_json(&Path::new(root).join("upgrade_").join("probe.json"), &rec);
        ctx.log(&format!("  probe: {result} - row appended to upgrade_\\walkaway-probe.csv, record in upgrade_\\probe.json"));
    }
    let sd = Path::new(&ctx.state_dir);
    let _ = std::fs::copy(sd.join("state.json"), sd.join("state-probe.json"));
    let _ = std::fs::remove_file(sd.join("state.json"));
}

/// Invoke-NotifyPhase: RunOnce at sign-in, as the person.
pub fn notify(state_dir: &str) {
    let p = Path::new(state_dir).join("notice.json");
    let Some(n) = read_json(&p) else { return };
    live::show_popup(&s(&n["text"]), &s(&n["title"]), 600, int(&n["buttons"]));
    let _ = std::fs::remove_file(&p);
}

/// Invoke-AbortPhase: stop an in-progress conversion between phases.
pub fn abort(ctx: &mut Ctx) {
    let state = ctx.state_dir.clone();
    let st = read_state(&state);
    live::unregister_resume_task(&mut ctx.rec);
    live::clear_notice_runonce();
    let Some(mut st) = st else {
        println!("  nothing in progress");
        return;
    };
    if truthy(&st["Handoff"]["Armed"]) && truthy(&st["Handoff"]["EntryGuid"]) {
        let guid = s(&st["Handoff"]["EntryGuid"]);
        let _ = ctx.rec.run("bcdedit", &["/deletevalue", "{fwbootmgr}", "bootsequence"]);
        let _ = ctx.rec.run("bcdedit", &["/delete", &guid]);
        println!("  removed the one-shot boot entry");
    }
    if truthy(&st["BitLocker"]["Suspended"]) {
        let _ = ctx.rec.run("manage-bde", &["-protectors", "-enable", "C:"]);
        println!("  BitLocker protection re-enabled");
    }
    for r in live::restore_memory_files(&mut ctx.rec, &mut st, false) {
        println!("  {r}");
    }
    if let Some(root) = live::find_stick(&st) {
        let _ = std::fs::remove_file(Path::new(&root).join("upgrade_").join("boot-install"));
        println!("  {} Wi-Fi password file(s) removed from the stick", live::remove_wifi_secrets(&root));
    }
    let sd = Path::new(&state);
    let _ = std::fs::rename(sd.join("state.json"), sd.join("state-aborted.json"));
    println!("  aborted at stage '{}'; state kept as state-aborted.json. A shrink already made is not undone here (Disk Management can extend C:).", s(&st["Stage"]));
}
