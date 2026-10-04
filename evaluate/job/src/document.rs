//! New-JobDocument: facts and the person's choices in, a `job.json` or the
//! refusals out. Everything that can refuse refuses here, before anything
//! on the machine has changed.

use crate::val::{at, eq_ci, int, items, one_of, s, truthy};
use crate::{decide, harvest, maps, records, ERASE_STATEMENT, LINUX_MIN_GB, VERIFY_ONLY_HASH};
use serde_json::{json, Value};
use upgrade_scan::parse::shrink_mitigable;
use upgrade_scan::ps::{num, round1};

/// What the person chose on the launcher.
#[derive(Debug, Clone)]
pub struct Choices<'a> {
    /// `kde` or `gnome`
    pub desktop: &'a str,
    /// `desktop` or `console`: what the computer starts at
    pub start_at: &'a str,
    /// SHA-512 crypt; the password itself never reaches this code
    pub password_hash: &'a str,
    /// `stop` or `clean-slate`: what to do if Windows cannot be kept
    pub if_cannot_keep: &'a str,
    /// where the scan report sits, relative to the job folder
    pub report_rel: &'a str,
    /// what was typed for the data-loss statement (R23), or empty
    pub acknowledge_data_loss: &'a str,
    /// what was typed for the erase sentence (R27), or empty
    pub erase_everything: &'a str,
}

/// What the writer stamps on the job. Given from outside so the judging
/// stays a pure function of its inputs.
#[derive(Debug, Clone)]
pub struct Stamp<'a> {
    pub job_id: &'a str,
    /// `2026-10-04T12:00:00Z`
    pub now_utc: &'a str,
    /// the version of the program writing the job (`evaluate.version`)
    pub writer_version: &'a str,
}

/// The job, or every reason there is none.
pub fn job_document(f: &Value, choice: &Choices, stamp: &Stamp) -> Result<Value, Vec<String>> {
    let mut refusals: Vec<String> = Vec::new();
    let erase = !choice.erase_everything.is_empty();
    let mut erase_disks: Vec<Value> = Vec::new();
    if erase {
        if choice.erase_everything != ERASE_STATEMENT {
            refusals.push(format!("the erase sentence was not typed exactly (expected: {ERASE_STATEMENT})"));
        }
        if choice.password_hash.is_empty() || eq_ci(choice.password_hash, VERIFY_ONLY_HASH) {
            refusals.push("no password was chosen for the new account".into());
        } else if !choice.password_hash.starts_with("$6$") {
            refusals.push("the password hash is not SHA-512 crypt".into());
        }
        match decide::erase_disks(at(f, "AllDisks"), int(at(f, "Disk.Number")), &s(at(f, "Stick.UniqueId"))) {
            Ok(disks) => erase_disks = disks,
            Err(why) => refusals.extend(why),
        }
    }
    let verdict = s(at(f, "Verdict"));
    let names = |key: &str| items(at(f, key)).into_iter().map(s).collect::<Vec<_>>();
    let ack = match decide::acknowledgement(&verdict, &names("FailedChecks"), &names("WarnChecks"), choice.acknowledge_data_loss, stamp.now_utc) {
        Ok(block) => {
            if !one_of(&verdict, &["GREEN", "YELLOW", "RED"]) {
                refusals.push(format!("no scanner verdict found (got '{verdict}') - run the scanner with -Json first"));
            }
            block
        }
        Err(why) => {
            refusals.push(why);
            None
        }
    };
    let firmware = s(at(f, "Firmware"));
    if !eq_ci(&firmware, "UEFI") {
        refusals.push(format!("firmware is '{firmware}', not UEFI - the boot handoff does not apply"));
    }
    let bitlocker = s(at(f, "BitLocker"));
    if !one_of(&bitlocker, &["on", "off"]) {
        refusals.push("BitLocker state on C: could not be determined".into());
    }
    let windows_tz = s(at(f, "WindowsTz"));
    let iana = maps::iana_time_zone(&windows_tz);
    if iana.is_none() {
        refusals.push(format!("Windows time zone '{windows_tz}' has no IANA mapping in this version"));
    }
    let c = at(f, "Clock");
    let clock = records::clock(&windows_tz, iana.unwrap_or(""), at(c, "RealTimeIsUniversal"), at(c, "DynamicDstDisabled"), int(at(c, "OffsetMinutes")), int(at(c, "BaseOffsetMinutes")), truthy(at(c, "DstActive")), &s(at(c, "NowUtc")));
    if let Err(why) = &clock {
        refusals.push(why.clone());
    }
    let input_tip = s(at(f, "InputTip"));
    let keymap = maps::keymap(&input_tip);
    if keymap.is_none() {
        refusals.push(format!("keyboard layout '{input_tip}' has no mapping in this version"));
    }
    let stick = at(f, "Stick");
    if !truthy(stick) {
        refusals.push(format!("the stick's identity could not be read ({})", s(at(f, "StickError"))));
    } else if !eq_ci(&s(at(stick, "Bus")), "USB") {
        refusals.push(format!("the stick is on bus '{}', not USB", s(at(stick, "Bus"))));
    }
    if !truthy(at(f, "Disk.UniqueId")) {
        refusals.push("the system disk has no unique id".into());
    }
    if let Some(why) = decide::release_refusal(at(f, "KitRelease"), at(f, "Releases")) {
        refusals.push(why);
    }
    // the folder map decides what is kept; an erase job keeps nothing, so it
    // only needs the map to exist (it lists what will be deleted)
    let h = at(f, "Harvest");
    if erase {
        if !truthy(h) {
            refusals.push(format!("the list of your folders could not be read ({})", s(at(f, "HarvestError"))));
        }
    } else {
        refusals.extend(harvest::refusals(h, &s(at(f, "HarvestError"))));
    }
    let (Ok(clock), Some(iana), Some(keymap), true) = (clock, iana, keymap, refusals.is_empty()) else { return Err(refusals) };

    let esp_free = int(at(f, "EspFree"));
    let esp_fits = esp_free >= 32 * 1048576;
    let disk_ack = ack.as_ref().is_some_and(|b| items(&b["overrides"]).iter().any(|o| s(o) == "disk-health"));
    // A shrink number read while Windows has a full chkdsk queued is not a
    // measurement (the Aspire answered 0 GB with no error, 2026-09-17, R18):
    // it is recorded as unmeasured, with the reason, and the prologue
    // measures again after the check it will run.
    let repair_queued = truthy(at(f, "RepairQueued"));
    let mut shrink_gb = at(f, "ShrinkGB").as_f64();
    let mut shrink_error = at(f, "ShrinkError").clone();
    if repair_queued && shrink_gb.is_none_or(|gb| gb < LINUX_MIN_GB) {
        let answer = match shrink_gb {
            Some(gb) => format!("{} GB", num(gb)),
            None => format!("no number ({})", s(&shrink_error)),
        };
        shrink_error = json!(format!("Windows answered {answer} while a full disk check is queued ({}) - not a trustworthy measurement; the prologue measures again after the check", s(at(f, "RepairQueuedWhy"))));
        shrink_gb = None;
    }
    let health = s(at(f, "Health"));
    let dirty = s(at(f, "Dirty"));
    let mitigable = shrink_mitigable(&s(at(f, "LastUnmovable")));
    let path = if erase { Some(("clean-slate", "user-chose-fresh-start")) } else { decide::path(&health, esp_fits, shrink_gb, &dirty, disk_ack, repair_queued, mitigable, choice.if_cannot_keep) };
    let Some((path, path_reason)) = path else {
        let why = if !esp_fits { format!("the EFI system partition has {} MB free, too little for Linux's boot files beside Windows'", num(round1(esp_free as f64 / 1048576.0))) } else { format!("the system disk reports '{health}'") };
        return Err(vec![format!("Windows cannot be kept on this machine ({why}), and you chose to stop rather than wipe it - no job; nothing was changed")]);
    };
    if !erase {
        if let Some(why) = harvest::stick_fit_refusal(h, path) {
            return Err(vec![format!("{why} - no job; nothing was changed")]);
        }
    }
    let (folders, cloud_files, stick_fit) = harvest::to_job(h);
    let health_status = if one_of(&health, &["Healthy", "Warning", "Unhealthy"]) { health.clone() } else { "Unknown".to_string() };
    let text_or_null = |v: &Value| if truthy(v) { json!(s(v)) } else { Value::Null };
    let lic = at(f, "License");
    let license = if truthy(lic) { records::license(at(lic, "Os"), at(lic, "Products"), at(lic, "Firmware"), &s(at(lic, "Error")), &s(at(lic, "NowUtc"))) } else { records::license(&Value::Null, &Value::Null, &Value::Null, "not read", stamp.now_utc) };
    let ssh_facts = at(f, "Ssh");
    let ssh = if truthy(ssh_facts) { records::ssh(at(ssh_facts, "StartType"), at(ssh_facts, "KeyFiles"), &s(at(ssh_facts, "ReadError"))) } else { json!({"result": "not-harvested", "keys": [], "sources": [], "why": null}) };
    let software = if truthy(at(f, "Software")) { at(f, "Software").clone() } else { json!({"desktop": [], "store": [], "truncated": false}) };
    let bitlocker_on = eq_ci(&bitlocker, "on");

    let mut job = json!({
        "schema": "job/1",
        "job_id": stamp.job_id,
        "created_utc": stamp.now_utc,
        "evaluate": {"version": stamp.writer_version, "scanner_version": "see report", "harvest_version": s(at(h, "HarvestVersion")), "ran_as_admin": true},
        "identity": {
            "vendor": at(f, "Vendor"), "model": at(f, "Model"), "system_uuid": at(f, "Uuid"), "bios_serial": at(f, "BiosSerial"), "bios_version": at(f, "BiosVersion"),
            "firmware_mode": "UEFI", "secure_boot": at(f, "SecureBoot"), "os_caption": at(f, "OsCaption"), "os_build": int(at(f, "OsBuild")),
            "system_disk": {"number": int(at(f, "Disk.Number")), "serial_number": s(at(f, "Disk.Serial")), "unique_id": at(f, "Disk.UniqueId"),
                            "friendly_name": at(f, "Disk.Name"), "size_bytes": int(at(f, "Disk.Size")), "partition_style": at(f, "Disk.Style")},
        },
        "scan": {"verdict": at(f, "Verdict"), "required_kernel": text_or_null(at(f, "RequiredKernel")), "report": choice.report_rel},
        "intent": {
            "path": path, "path_reason": path_reason, "desktop": choice.desktop, "start_at": choice.start_at,
            "distro": {"name": "fedora", "release": s(at(f, "KitRelease.release"))},
            "account": {"windows_name": at(f, "UserName"), "full_name": text_or_null(at(f, "FullName")),
                        "linux_name": maps::linux_name(&s(at(f, "UserName"))), "password_hash": choice.password_hash},
            "locale": {"lang": format!("{}.UTF-8", s(at(f, "Locale")).replace('-', "_")), "timezone": iana, "keymap": keymap},
        },
        // The launcher's decision screen says restore points and NTFS's change
        // journal go if they are what stops the shrink, and that this cannot
        // be undone. CONVERT typed there is the consent (R18).
        "fork": {"if_cannot_keep": choice.if_cannot_keep, "volume_check_consented": true, "restore_points_consented": true, "usn_journal_consented": true},
        "storage": {
            "shrinkable_gb": shrink_gb, "shrink_source": if shrink_gb.is_some() { json!("storage-api") } else { Value::Null }, "shrink_error": shrink_error,
            "last_unmovable_file": text_or_null(at(f, "LastUnmovable")),
            "linux_min_gb": LINUX_MIN_GB as i64,
            "volume_health": {"dirty": at(f, "Dirty"), "repair_queued": repair_queued, "scan": null},
            "physical_disk": {"health_status": health_status, "operational_status": s(at(f, "Operational")), "media_type": s(at(f, "MediaType"))},
            "esp": {"size_bytes": int(at(f, "EspSize")), "free_bytes": esp_free, "fits_alongside_install": esp_fits},
        },
        "harvest": {
            "clock": clock,
            "windows_license": license,
            "folders": folders,
            "cloud_files": cloud_files,
            "stick_fit": stick_fit,
            "browsers": [],
            // the Wi-Fi export runs after every refusal has had its say: a
            // refused job never leaves passwords on the stick
            "wifi": {"result": "not-harvested", "secrets_dir": null, "profiles": []},
            "ssh": ssh,
            "bitlocker": {"status": at(f, "BitLocker"), "recovery_key_file": if bitlocker_on { json!("artifacts/credentials/bitlocker-C.txt") } else { Value::Null }},
            "firmware_artifacts": [],
            "software": software,
        },
        "stick": {"unique_id": at(stick, "UniqueId"), "serial_number": s(at(stick, "Serial")), "size_bytes": int(at(stick, "Size")),
                  "friendly_name": at(stick, "Name"), "label": if truthy(at(stick, "Label")) { at(stick, "Label").clone() } else { json!("UPGV0") }, "manifest": "SHA256SUMS"},
    });
    if let Some(block) = ack {
        job["risk_acknowledgement"] = block;
    }
    if erase {
        job["erase_consent"] = json!({"statement": ERASE_STATEMENT, "accepted_utc": stamp.now_utc, "disks": erase_disks});
    } else if path == "clean-slate" {
        job["staged"] = json!({"files": 0, "bytes": 0, "manifest": "staging/SHA256SUMS"});
    }
    Ok(job)
}
