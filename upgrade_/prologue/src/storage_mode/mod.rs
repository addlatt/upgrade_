//! The storage-mode harness (gate V5, risk R1), ported from
//! `evaluate/windows/Test-StorageMode.ps1` 0.1.0: the scanner's "Storage
//! controller mode" check fired in both SATA modes of one machine, one
//! click. Leg 1 scans in the mode the machine is in; a copy of the Windows
//! boot entry with `safeboot minimal` is booted exactly once through the
//! boot manager's one-time `bootsequence`; the machine restarts into the
//! firmware setup, where the person changes SATA Mode; Safe Mode binds the
//! driver for the re-enumerated controller; a SYSTEM startup task scans
//! again (leg 2) and asks for the original mode back; leg 3 scans a last
//! time and everything armed is undone. The record lands on the stick in
//! `upgrade_\storage-mode\` and `rig/v5-verdict.py` writes the rows from
//! it, never by hand.
//!
//! This module is the judging half: the pure functions (what the
//! controllers declare, which mode to ask for, what a leg means), the
//! texts, the state and the record, held to the script word for word by
//! `tests/storage_mode.rs`. `flow` runs the phases over a seam (`Live`)
//! so the same test drives them with canned answers; `live` (Windows
//! only) is the seam on a real machine.

pub mod flow;
#[cfg(windows)]
pub mod live;

use crate::val::{at, int, items, s, truthy};
use serde_json::{json, Value};
use upgrade_scan::ps::{eq_ci, re, starts_with_ci};

/// The PowerShell harness this port follows.
pub const FOLLOWS_HARNESS: &str = "0.1.0";
pub const TASK_NAME: &str = "upgrade_ storage-mode resume";
/// The `*` makes RunOnce fire in Safe Mode too.
pub const SAFE_RUNONCE_NAME: &str = "*upgrade_storage-mode-safe";
pub const NOTICE_RUNONCE_NAME: &str = "upgrade_storage-mode-notice";
pub const BCD_DESCRIPTION: &str = "upgrade_ storage-mode test (Safe Mode, one boot)";
pub const STICK_WAIT_SECONDS: i64 = 120;
pub const STICK_SUBDIR: &str = "upgrade_\\storage-mode";
pub const TITLE: &str = "upgrade_ - storage-mode test";
pub const PROGRAM: &str = "upgrade-prologue.exe";

/// What this program calls itself in the record.
pub fn version_line() -> String {
    format!("upgrade-prologue {} (Rust; follows Test-StorageMode.ps1 {FOLLOWS_HARNESS})", env!("CARGO_PKG_VERSION"))
}

/// Get-SmPciId: `vvvv:dddd`, lower case; nothing when the id has no VEN/DEV.
pub fn pci_id(device_id: &str) -> String {
    match re(r"VEN_([0-9A-Fa-f]{4})&DEV_([0-9A-Fa-f]{4})", "i").find(device_id) {
        Some(m) => match (m.group(1), m.group(2)) {
            (Some(v), Some(d)) => format!("{}:{}", &device_id[v], &device_id[d]).to_lowercase(),
            _ => String::new(),
        },
        None => String::new(),
    }
}

/// Get-SmClassCodes: the class+subclass (4 hex digits) of every
/// `PCI\CC_ccss[pp]` entry, upper case, deduplicated, in first-seen order.
pub fn class_codes(compatible_ids: &Value) -> Vec<String> {
    let entries: Vec<String> = match compatible_ids {
        Value::Array(l) => l.iter().map(s).collect(),
        other => vec![s(other)],
    };
    let mut codes: Vec<String> = Vec::new();
    for c in entries {
        if let Some(m) = re(r"CC_([0-9A-Fa-f]{4})", "i").find(&c) {
            if let Some(g) = m.group(1) {
                let x = c[g].to_uppercase();
                if !codes.iter().any(|k| eq_ci(k, &x)) {
                    codes.push(x);
                }
            }
        }
    }
    codes
}

/// Get-SmStorageControllers: Intel PCI devices that are mass-storage class
/// (`CC_01xx`, or PNPClass HDC/SCSIAdapter) or carry an Intel RST service,
/// as `{Name, PciId, Classes, Service, CompatibleIds}`, recorded verbatim.
pub fn storage_controllers(pnp: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    for d in items(pnp) {
        let did = s(at(d, "DeviceID"));
        if !did.starts_with("PCI\\") {
            continue;
        }
        let pci = pci_id(&did);
        if !pci.starts_with("8086:") {
            continue;
        }
        let codes = class_codes(at(d, "CompatibleID"));
        let svc = s(at(d, "Service"));
        let class = s(at(d, "PNPClass"));
        let storage = codes.iter().any(|c| c.starts_with("01")) || eq_ci(&class, "HDC") || eq_ci(&class, "SCSIAdapter");
        if storage || starts_with_ci(&svc, "iastor") {
            let compat: Vec<String> = match at(d, "CompatibleID") {
                Value::Array(l) => l.iter().filter(|v| truthy(v)).map(s).collect(),
                Value::Null => vec![],
                other => if truthy(other) { vec![s(other)] } else { vec![] },
            };
            out.push(json!({
                "Name": s(at(d, "Name")), "PciId": pci,
                "Classes": codes.iter().filter(|c| c.starts_with("01")).cloned().collect::<Vec<_>>(),
                "Service": svc, "CompatibleIds": compat,
            }));
        }
    }
    out
}

/// Get-SmModeWord: raid (class 0104 or the VMD driver), ahci, nvme (only
/// NVMe controllers), none (no Intel storage controller), else other.
pub fn mode_word(controllers: &[Value]) -> &'static str {
    if controllers.is_empty() {
        return "none";
    }
    let has = |c: &Value, code: &str| items(at(c, "Classes")).iter().any(|x| eq_ci(&s(x), code));
    if controllers.iter().any(|c| has(c, "0104") || starts_with_ci(&s(at(c, "Service")), "iastorvd")) {
        return "raid";
    }
    if controllers.iter().any(|c| has(c, "0106")) {
        return "ahci";
    }
    if controllers.iter().any(|c| has(c, "0108")) {
        return "nvme";
    }
    "other"
}

/// Get-SmNextMode: the other mode, or nothing when there is none to ask for.
pub fn next_mode(current: &str) -> Option<&'static str> {
    if eq_ci(current, "raid") {
        Some("ahci")
    } else if eq_ci(current, "ahci") || eq_ci(current, "nvme") {
        Some("raid")
    } else {
        None
    }
}

/// Get-SmModeLabel.
pub fn mode_label(mode: &str) -> String {
    if eq_ci(mode, "raid") {
        "Intel RST Premium / Optane / RAID".to_string()
    } else if eq_ci(mode, "ahci") {
        "AHCI".to_string()
    } else {
        mode.to_string()
    }
}

/// Get-SmReportCheck: the scanner report's storage line and verdict.
pub fn report_check(report: &Value) -> Value {
    let chk = items(at(report, "Checks")).into_iter().find(|c| eq_ci(&s(at(c, "Title")), "Storage controller mode"));
    json!({
        "Status": chk.map_or("(no check emitted)".to_string(), |c| s(at(c, "Status"))),
        "Detail": chk.map_or(String::new(), |c| s(at(c, "Detail"))),
        "Verdict": s(at(report, "Verdict.Level")),
        "ScannerVersion": s(at(report, "ScannerVersion")),
        "ScannedUtc": s(at(report, "ScannedUtc")),
    })
}

/// ConvertFrom-SmBcdCopy: the `{guid}` in what `bcdedit /copy` printed,
/// the one part that survives localisation.
pub fn bcd_copy_guid(text: &str) -> Option<String> {
    re(r"(\{[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\})", "").find(text).and_then(|m| m.group(1)).map(|g| text[g].to_string())
}

/// Test-SmSafeMode: the SafeBoot\Option key exists only during a Safe Mode boot.
pub fn is_safe_mode(option_value: Option<i64>) -> bool {
    option_value.is_some()
}

/// Get-SmLegOutcome: leg 2 against leg 1 (did the setting take?), leg 3
/// against leg 1 (is it back?).
pub fn leg_outcome(leg: i64, first_mode: &str, this_mode: &str) -> &'static str {
    let same = eq_ci(this_mode, first_mode);
    match leg {
        2 => if same { "mode-unchanged" } else { "changed" },
        3 => if same { "restored" } else { "not-restored" },
        _ => "unexpected-leg",
    }
}

/// New-SmSafeRunOnceCommand: fires at the Safe Mode sign-in as the person
/// and restarts only inside Safe Mode. The script launched its own copy
/// with `-SafeModeSignIn`; this launches the program's copy.
pub fn safe_runonce_command(exe: &str, state: &str) -> String {
    format!("\"{exe}\" storage-mode safe-mode-sign-in --state-dir \"{state}\"")
}

/// The sign-in notice's RunOnce command (the script's `-Notify`).
pub fn notice_command(exe: &str, state: &str) -> String {
    format!("\"{exe}\" storage-mode notify --state-dir \"{state}\"")
}

/// The program's copy in the state directory.
pub fn program_in(state: &str) -> String {
    format!("{}\\{PROGRAM}", state.trim_end_matches('\\'))
}

/// ConvertFrom-SmSafeMarker: one line per Safe Mode sign-in,
/// `utc|user|option|uptime_s`; shorter lines are dropped.
pub fn safe_marker_lines(lines: &[String]) -> Vec<Value> {
    lines
        .iter()
        .filter_map(|l| {
            let p: Vec<&str> = l.split('|').collect();
            if p.len() >= 4 {
                Some(json!({"Utc": p[0], "RunAs": p[1], "OptionValue": p[2], "UptimeSeconds": p[3], "Source": "runonce-signin"}))
            } else {
                None
            }
        })
        .collect()
}

/// The marker line the Safe Mode sign-in leaves.
pub fn safe_marker_line(utc: &str, run_as: &str, option_value: i64, uptime_seconds: i64) -> String {
    format!("{utc}|{run_as}|{option_value}|{uptime_seconds}")
}

/// Get-SmFlowResult: the flow's own verdict from its record.
pub fn flow_result(stage: &str, legs: i64, last_outcome: &str) -> String {
    if let Some(head) = stage.get(..8) {
        if eq_ci(head, "stopped:") {
            return stage[8..].to_string();
        }
    }
    if legs >= 3 {
        return last_outcome.to_string();
    }
    if legs == 2 && eq_ci(last_outcome, "mode-unchanged") {
        return "mode-unchanged".to_string();
    }
    "in-progress".to_string()
}

/// ConvertTo-SmCsvLine: every field quoted, quotes doubled, line ends flattened.
pub fn csv_line(fields: &[Value]) -> String {
    crate::judge::probe_csv_line(fields)
}

/// Get-DriveRoot: `e` is `E:\`.
pub fn drive_root(letter: &str) -> String {
    format!("{}:\\", letter.trim_end_matches([':', '\\']).to_uppercase())
}

/// Where one leg's files go on the stick.
pub fn leg_dir(root: &str, n: i64) -> String {
    format!("{}\\leg{n}", stick_dir(root))
}

/// `upgrade_\storage-mode` on the stick.
pub fn stick_dir(root: &str) -> String {
    format!("{}{STICK_SUBDIR}", root)
}

/// The state at the start.
pub fn new_state(started_utc: &str, facts: &Value, root: &str, bench: bool) -> Value {
    json!({
        "HarnessVersion": FOLLOWS_HARNESS, "StartedUtc": started_utc, "Stage": "start", "Facts": facts,
        "StickVolumeId": at(facts, "StickVolumeId").clone(), "StickRootAtStart": root,
        "Bench": bench, "Legs": [], "Resumes": [], "SafeBoots": [], "FwReboots": [], "SafeEntry": null, "Ask": null, "LastOutcome": "", "Cleanup": null, "Notice": null,
    })
}

/// One leg's record from the scanner's report and capture.
pub fn leg_record(n: i64, utc: &str, asked: &str, report: &Value, report_file: &str, capture: &Value, ctx: &Value) -> Value {
    let ctrls = storage_controllers(at(capture, "Pnp"));
    let mode = mode_word(&ctrls);
    json!({
        "N": n, "Utc": utc, "Asked": asked, "Mode": mode, "Controllers": ctrls, "Check": report_check(report),
        "ReportFile": report_file, "CaptureFile": "machine-capture.json",
        "RunAs": s(at(ctx, "RunAs")), "SessionId": int(at(ctx, "SessionId")), "Unattended": truthy(at(ctx, "Unattended")),
    })
}

/// The leg's log line.
pub fn leg_line(leg: &Value) -> String {
    let ctrls: Vec<String> = items(at(leg, "Controllers")).iter().map(|c| format!("{}[{}] {}", s(at(c, "PciId")), items(at(c, "Classes")).iter().map(|v| s(v)).collect::<Vec<_>>().join("/"), s(at(c, "Service")))).collect();
    format!("  leg {}: mode={}  controllers={}  check={} '{}'  verdict={}", int(at(leg, "N")), s(at(leg, "Mode")), ctrls.join("; "), s(at(leg, "Check.Status")), s(at(leg, "Check.Detail")), s(at(leg, "Check.Verdict")))
}

/// The record on the stick and in the state directory (`storage-mode/1`).
pub fn record(st: &Value) -> Value {
    let legs = items(at(st, "Legs")).len() as i64;
    let mut r = json!({
        "schema": "storage-mode/1", "harness_version": FOLLOWS_HARNESS, "harness_program": version_line(),
        "flow_result": flow_result(&s(at(st, "Stage")), legs, &s(at(st, "LastOutcome"))),
    });
    for (k, from) in [("facts", "Facts"), ("legs", "Legs"), ("resumes", "Resumes"), ("safe_boots", "SafeBoots"), ("safe_entry", "SafeEntry"), ("fw_reboots", "FwReboots"), ("cleanup", "Cleanup"), ("notice", "Notice"), ("stage", "Stage"), ("started_utc", "StartedUtc"), ("updated_utc", "UpdatedUtc")] {
        r[k] = st.get(from).cloned().unwrap_or(Value::Null);
    }
    r
}

/// `Key=Value` pairs of the cleanup, as the log prints them.
pub fn cleanup_words(cleanup: &Value) -> String {
    cleanup.as_object().into_iter().flatten().map(|(k, v)| format!("{k}={}", s(v))).collect::<Vec<_>>().join(" ")
}

/// The facts line of the start.
pub fn facts_line(f: &Value, root: &str) -> String {
    format!("  {} {}   BIOS {}   {} {}   Secure Boot {}   BitLocker {}   stick on {} as {root}", s(at(f, "Vendor")), s(at(f, "Model")), s(at(f, "BiosVersion")), s(at(f, "OsCaption")), s(at(f, "OsBuild")), s(at(f, "SecureBoot")), s(at(f, "BitLocker")), s(at(f, "StickBus")))
}

pub const BITLOCKER_REFUSAL: &str = "BitLocker protection is ON for C:. A Safe Mode boot through a copied boot entry can trip BitLocker recovery; suspend BitLocker (manage-bde -protectors -disable C:) or run this on a machine without it. Nothing was changed.";
pub const NO_INTEL_LOG: &str = "  no Intel storage controller on this machine - the SATA-mode test does not apply here (AMD, or a VM). Nothing was changed; the leg-1 scan is on the stick.";
pub const NO_INTEL_POPUP: &str = "This computer has no Intel storage controller, so the SATA-mode test does not apply. Nothing was changed. The scan is on the stick.";
pub const NO_STICK_TEXT: &str = "The USB stick is not plugged in. Plug it in and restart the computer - the test continues by itself.";

/// The explanation window before the first restart.
pub fn explanation_text(mode: &str, ask: &str, back: &str) -> String {
    format!(
        "This computer's storage controller is in {} mode now. The test needs one scan in each mode.\n\n\
         When you click OK the computer restarts. TAP F2 REPEATEDLY (or your computer's setup key) from the moment the screen goes dark until the setup screen appears - do this every time, whether or not the firmware opens setup by itself: Acer's InsydeH2O V1.21 accepts the request and ignores it (2026-09-15).\n\n\
         \x20 1st setup screen:  Main tab -> SATA Mode -> set it to  {}  -> F10 -> Yes\n\
         \x20     (Acer hides SATA Mode on some models: press Ctrl+S on the Main tab to show it. If it is still not there, press Esc and exit WITHOUT saving - the test then stops by itself.)\n\
         \x20 Windows then boots once into SAFE MODE (black screen, 'Safe Mode' in the corners) and shows a sign-in screen. NO PASSWORD IS NEEDED: HOLD THE POWER BUTTON about ten seconds until the computer turns off, then press it once to start it. (Windows 11 Home shows no power icon on that screen. Signing in with your password instead also works - it restarts by itself; the PIN does not work in Safe Mode.)\n\
         \x20 Windows scans by itself before anyone signs in, then restarts into the setup screen again.\n\
         \x20 2nd setup screen:  set SATA Mode back to  {}  -> F10 -> Yes\n\
         \x20 Safe Mode once more (hold the power button, start again); it scans a last time and cleans up. Sign in normally: a window shows the result.\n\n\
         Your files are not touched - changing SATA mode changes how the disk is addressed, not what is on it. If Windows ever shows a blue screen after a change, go back into setup and set the mode back; the test records how far it got.\n\n\
         Leave the USB stick in the whole time.",
        mode_label(mode),
        mode_label(ask),
        mode_label(back)
    )
}

/// The finish notice around a result's own words.
pub fn finish_text(result: &str, text: &str) -> String {
    format!("Storage-mode test finished: {result}.\n\n{text}\n\nEverything the test armed has been removed (boot entry copy, one-time boot sequence, RunOnce, startup task). The record is on the USB stick in upgrade_\\storage-mode\\.")
}

/// A stop's notice: what went wrong, and that nothing armed is left.
pub fn error_text(msg: &str, state: &str, stick_found: bool) -> String {
    format!("The storage-mode test stopped with an error and cleaned up after itself:\n{msg}\n\nNothing armed is left behind. The record is in {state}{}", if stick_found { " and on the stick (upgrade_\\storage-mode\\)." } else { "." })
}

/// The mode to ask for back: the first leg's, when it was one of the two.
pub fn mode_back(first_mode: &str) -> String {
    if eq_ci(first_mode, "raid") || eq_ci(first_mode, "ahci") { first_mode.to_string() } else { "ahci".to_string() }
}
