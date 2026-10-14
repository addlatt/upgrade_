//! The writer's phases, following Write-UpgradeStick.ps1's main block:
//! list (every attached disk with the identity the other phases need),
//! plan (decide, print, record, write nothing) and write (the one
//! destructive path, after every rule and the typed confirmation). Every
//! read and write goes through [`Live`], so `tests/stick.rs` drives the
//! phases with canned answers and holds every act to what the script did.

use crate::judge::*;
use crate::{FOLLOWS_WRITER, VERSION};
use serde_json::{json, Value};

/// What one write did (Invoke-UpgStickWrite's result).
#[derive(Debug, Clone, Default)]
pub struct WriteResult {
    pub written: bool,
    pub verified: bool,
    pub files: i64,
    pub failed: Vec<String>,
    pub boot_letter: String,
    pub data_letter: String,
    pub notes: Vec<String>,
    /// the stick's identity for job.json (`stick.json`)
    pub stick: Value,
}

pub trait Live {
    fn elevated(&self) -> bool;
    /// `(Get-Date).ToUniversalTime().ToString('o')`.
    fn now_o(&self) -> String;
    /// Get-UpgAttachedDisks: every attached disk with what the rules need.
    fn attached_disks(&mut self) -> Vec<Value>;
    /// `{Manufacturer, Model, Caption, BuildNumber}` of this machine.
    fn machine(&self) -> Value;
    /// Read-Host.
    fn read_host(&mut self, prompt: &str) -> String;
    /// `Test-Path (Join-Path $Source 'SHA256SUMS')`.
    fn source_has_sums(&self, source: &str) -> bool;
    /// Invoke-UpgStickWrite: the destructive half, on the disk re-resolved by id.
    fn write_stick(&mut self, disk: &Value, source: &str, boot_label: &str, boot_gb: i64) -> Result<WriteResult, String>;
    /// Resolve-ResultsCsv.
    fn results_csv(&self) -> String;
    /// Append one evidence line (the header first when the file is new).
    fn append_evidence(&mut self, csv: &str, line: &str);
    /// `stick.json` on the boot partition.
    fn out_file(&mut self, path: &str, text: &str);
    fn host(&mut self, line: &str);
}

pub struct Args {
    pub phase: String,
    pub target: String,
    pub expected_size_bytes: i64,
    pub expected_size_gb: f64,
    pub source: String,
    pub label: String,
    pub boot_size_gb: i64,
    pub confirmed_label: Option<String>,
}

fn version_line() -> String {
    format!("{VERSION} (Rust; follows Write-UpgradeStick.ps1 {FOLLOWS_WRITER})")
}

/// The main block. Returns the exit code, or the message of an uncaught throw.
pub fn run(live: &mut dyn Live, a: &Args) -> Result<i32, String> {
    let disks = live.attached_disks();
    let m = live.machine();
    let machine = format!("{} {}", s(&m, "Manufacturer"), s(&m, "Model"));
    let os_build = s(&m, "BuildNumber");
    if a.phase == "list" {
        live.host("");
        live.host(&format!("  upgrade_  stick writer {FOLLOWS_WRITER}  -  attached disks"));
        live.host("");
        for d in &disks {
            live.host(&disk_line(d));
            live.host(&format!("          id: {}   serial: {}", s(d, "UniqueId"), s(d, "SerialNumber")));
        }
        live.host("");
        live.host("  Point at a disk with -Target <id> and -ExpectedSizeGB <as printed on it>; -Plan first.");
        live.host("");
        return Ok(0);
    }
    let r = resolve(&disks, &a.target, a.expected_size_bytes, a.expected_size_gb);
    let usb = disks.iter().filter(|d| upgrade_scan::ps::eq_ci(&s(d, "BusType"), "USB")).count();
    let expected = expected_bytes(a.expected_size_bytes, a.expected_size_gb);
    let writing = a.phase == "write";
    let mode = if writing { "write" } else { "plan" };
    live.host("");
    live.host(&format!("  upgrade_  stick writer {FOLLOWS_WRITER}  -  {}", mode.to_uppercase()));
    live.host(&format!("  {machine}   {} {os_build}   {} disks attached, {usb} on USB", s(&m, "Caption"), disks.len()));
    live.host("");
    let rows = items(&r, "Rows");
    for row in &rows {
        let why: Vec<String> = row["Refusals"].as_array().into_iter().flatten().map(|x| x.as_str().unwrap_or("").to_string()).collect();
        let tag = if why.is_empty() { "WRITABLE" } else { "refused " };
        live.host(&format!("  {tag} {}", disk_line(&row["Disk"]).trim_start()));
        if !why.is_empty() {
            live.host(&format!("             {}", why.join("; ")));
        }
    }
    live.host("");
    let decision = s(&r, "Decision");
    let reason = s(&r, "Reason");
    live.host(&format!("  DECISION: {decision}{}", if reason.is_empty() { String::new() } else { format!(" - {reason}") }));
    let mut ev = json!({
        "Machine": machine, "OsBuild": os_build, "Mode": mode, "DisksAttached": disks.len(), "UsbDisks": usb,
        "Target": a.target, "ExpectedBytes": expected, "Decision": decision, "Refusals": reason, "Written": "n", "Verified": "n",
        "Notes": format!("[writer: elevated={}] {}", if live.elevated() { "True" } else { "False" }, per_disk_notes(&rows)),
    });
    if !writing || decision != "selected" {
        let csv = evidence(live, &ev);
        live.host(&format!("  nothing written. logged to {csv}"));
        live.host("");
        return Ok(if writing { 2 } else { 0 });
    }
    // --- write
    if !live.elevated() {
        return Err("Writing a stick needs an elevated PowerShell (Administrator).".into());
    }
    if !live.source_has_sums(&a.source) {
        return Err(format!("-Source must contain SHA256SUMS (make-kit.sh writes it): {}", a.source));
    }
    let chosen = r["Chosen"].clone();
    let word = confirm_word(&chosen);
    let has_label = word != s(&chosen, "FriendlyName");
    live.host("");
    live.host(&format!("  About to ERASE disk {}: {}, {} GB, {}.", int(&chosen, "Number"), s(&chosen, "FriendlyName"), gb1(int(&chosen, "SizeBytes")), if has_label { format!("label '{word}'") } else { "no label".to_string() }));
    live.host("  Everything on it will be gone. This is the only device this run will touch.");
    let answer = match &a.confirmed_label {
        Some(c) if !c.is_empty() => c.clone(),
        _ => live.read_host(&format!("  Type the {} shown above to confirm", if has_label { "label" } else { "model name" })),
    };
    if answer != word {
        ev["Decision"] = json!("refused");
        ev["Refusals"] = json!(format!("confirmation did not match ('{answer}' vs '{word}')"));
        let csv = evidence(live, &ev);
        live.host(&format!("  confirmation did not match. nothing written. logged to {csv}"));
        live.host("");
        return Ok(2);
    }
    let w = match live.write_stick(&chosen, &a.source, &a.label, a.boot_size_gb) {
        Ok(w) => {
            ev["Written"] = json!(if w.written { "y" } else { "n" });
            ev["Verified"] = json!(if w.verified { "y" } else { "n" });
            let extra = if w.notes.is_empty() { String::new() } else { format!(" {}", w.notes.join("; ")) };
            let notes = format!("{} | wrote boot={}: data={}: files_verified={} failed={}{extra}", s(&ev, "Notes"), w.boot_letter, w.data_letter, w.files, w.failed.join(","));
            ev["Notes"] = json!(notes);
            Some(w)
        }
        Err(e) => {
            ev["Notes"] = json!(format!("{} | write failed: {e}", s(&ev, "Notes")));
            live.host(&format!("  ! {e}"));
            None
        }
    };
    let csv = evidence(live, &ev);
    live.host("");
    let verified = w.as_ref().is_some_and(|w| w.verified);
    if let Some(w) = w.as_ref().filter(|w| w.verified) {
        live.host(&format!("  written and verified: {} files on {}:  staging partition {}:", w.files, w.boot_letter, w.data_letter));
        let path = format!("{}:\\stick.json", w.boot_letter);
        live.out_file(&path, &(serde_json::to_string_pretty(&w.stick).unwrap_or_default() + "\n"));
        live.host(&format!("  identity for job.json written to {path}"));
    } else {
        live.host("  NOT verified - do not use this stick.");
    }
    live.host(&format!("  logged to {csv}"));
    live.host("");
    Ok(if verified { 0 } else { 1 })
}

/// Write-EvidenceRow: the row, appended; the file's path.
fn evidence(live: &mut dyn Live, ev: &Value) -> String {
    let csv = live.results_csv();
    let f = |k: &str| s(ev, k);
    let line = evidence_line(&[live.now_o(), version_line(), f("Machine"), f("OsBuild"), f("Mode"), f("DisksAttached"), f("UsbDisks"), f("Target"), f("ExpectedBytes"), f("Decision"), f("Refusals"), f("Written"), f("Verified"), f("Notes")]);
    live.append_evidence(&csv, &line);
    csv
}
