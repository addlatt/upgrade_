//! What the window runs and how it reads the answers. Pure: nothing here
//! touches the machine, so all of it is tested on any OS (rule #5, the logic
//! level). The programs decide; this file only mirrors RUN-VERIFY.cmd's
//! steps as calls to the kit's Rust programs (the cut-over, RISKS R32:
//! `upgrade-scan`, `upgrade-job`, `upgrade-prologue`), and reads what they
//! and verify.sh wrote.

use crate::words;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// The files the verify flow refuses to start without (RUN-VERIFY.cmd's
/// list, with the Rust programs in place of the scripts).
pub const KIT_FILES_VERIFY: [&str; 8] = [
    "upgrade-scan.exe",
    "upgrade-job.exe",
    "upgrade-prologue.exe",
    "EFI\\BOOT\\BOOTX64.EFI",
    "images\\install.img",
    "upgrade_\\verify.sh",
    "upgrade_\\LiveOS\\kde.squashfs",
    "SHA256SUMS",
];

pub fn missing_kit_files(root: &Path) -> Vec<&'static str> {
    KIT_FILES_VERIFY.iter().copied().filter(|f| !root.join(win_rel(f)).exists()).collect()
}

/// A kit-relative Windows path as a PathBuf for this OS (tests run on Linux).
fn win_rel(rel: &str) -> PathBuf {
    rel.split('\\').collect()
}

/// One call of a kit program: which step it belongs to, the program on the
/// stick and its arguments.
#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    pub step: usize,
    pub program: String,
    pub args: Vec<String>,
    /// the scanner's machine capture: its exit code does not stop the run
    /// (RUN-VERIFY.cmd sends it to nul and does not check it)
    pub may_fail: bool,
}

/// `stick` is the drive, e.g. "E:"; every path is built from it as the .cmd
/// builds them from %~dp0 and %~d0.
pub fn call(step: usize, may_fail: bool, stick: &str, program: &str, rest: &[&str]) -> Call {
    let root = format!("{}\\", stick);
    Call { step, may_fail, program: format!("{}{}", root, program), args: rest.iter().map(|s| s.replace("{root}", &root).replace("{stick}", stick)).collect() }
}

/// What a call looks like on one line (for the log and the Details pane).
pub fn command_line(c: &Call) -> String {
    let q = |s: &str| if s.contains(' ') { format!("\"{}\"", s) } else { s.to_string() };
    std::iter::once(q(&c.program)).chain(c.args.iter().map(|a| q(a))).collect::<Vec<_>>().join(" ")
}

/// RUN-VERIFY.cmd, steps 1-3, as calls to the Rust programs. Step 0 (the kit
/// check) is not a call. The job writer writes the kickstart too (step 3),
/// as `upgrade-job write --kickstart` does what New-Kickstart.ps1 did.
pub fn verify_calls(stick: &str) -> Vec<Call> {
    vec![
        call(1, false, stick, "upgrade-scan.exe", &["scan", "--json", "--out", "{root}upgrade_\\reports", "--kit", "{root}"]),
        call(1, true, stick, "upgrade-scan.exe", &["dump-machine", "{root}machine-capture.json"]),
        call(
            2,
            false,
            stick,
            "upgrade-job.exe",
            &["write", "--stick", "{stick}", "--out", "{root}upgrade_", "--scan", "{root}upgrade_\\reports", "--desktop", "kde", "--start-at", "desktop", "--verify-only", "--kickstart", "{root}upgrade_\\ks.cfg", "--stick-label", "UPGV0", "--manifest", "{root}SHA256SUMS"],
        ),
    ]
}

/// Step 4: arm the handoff (`upgrade-prologue verify-arm`, following
/// Test-Handoff.ps1 -Arm -Auto). It restarts the computer 20 s after it
/// succeeds; its return check runs itself at the next sign-in.
pub fn arm_call(stick: &str) -> Call {
    call(4, false, stick, "upgrade-prologue.exe", &["verify-arm", "--stick", "{stick}", "--auto", "--payload", "shim", "--suspend-bitlocker"])
}

// ---------------------------------------------------------------- the convert flow (RUN-CONVERT.cmd, RUN-CONVERT-ACCEPTING-DATA-LOSS.cmd)

/// The files RUN-CONVERT.cmd refuses to start without, with the Rust
/// programs in place of the scripts.
pub const KIT_FILES_CONVERT: [&str; 10] = [
    "upgrade-scan.exe",
    "upgrade-job.exe",
    "upgrade-prologue.exe",
    "EFI\\BOOT\\BOOTX64.EFI",
    "images\\install.img",
    "upgrade_\\verify.sh",
    "upgrade_\\outcome.sh",
    "upgrade_\\LiveOS\\kde.squashfs",
    "upgrade_\\LiveOS\\gnome.squashfs",
    "SHA256SUMS",
];

pub fn missing_kit_files_convert(root: &Path) -> Vec<&'static str> {
    KIT_FILES_CONVERT.iter().copied().filter(|f| !root.join(win_rel(f)).exists()).collect()
}

/// The launcher's three choices, as the job writer takes them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Desktop {
    Kde,
    Gnome,
    Console,
}

impl Desktop {
    /// (--desktop, --start-at): the text console still installs KDE.
    pub fn job_args(self) -> (&'static str, &'static str) {
        match self {
            Desktop::Kde => ("kde", "desktop"),
            Desktop::Gnome => ("gnome", "desktop"),
            Desktop::Console => ("kde", "console"),
        }
    }
}

/// The acknowledgement sentence (RISKS R23), byte for byte; `ok` only when
/// the typed text is exactly the job writer's own.
pub fn ack_ok(typed: &str) -> bool {
    typed == upgrade_job::RISK_STATEMENT
}

/// The confirmation word, byte for byte (`CONVERT`, nothing else).
pub const CONFIRM_WORD: &str = "CONVERT";
pub fn confirm_ok(typed: &str) -> bool {
    typed == CONFIRM_WORD
}

/// Read-Password.ps1's rules, through the job crate: None when the pair is
/// acceptable, else why not (its words).
pub fn password_refusal(first: &str, second: &str) -> Option<&'static str> {
    upgrade_job::password::pair_refusal(first, second)
}

/// The hash the job writer takes (`--password-hash-file`), from the password
/// and a fresh salt from the OS. The password bytes are the caller's to wipe.
pub fn password_hash(password: &str) -> Result<String, String> {
    let salt = upgrade_job::password::new_salt()?;
    Ok(upgrade_job::password::sha512_crypt(password, &salt, None))
}

pub fn wipe(s: &mut String) {
    upgrade_job::password::wipe(s)
}

/// `upgrade-job linux-name`: the Linux account name from the Windows one
/// (New-Job.ps1 -PrintLinuxName). Its one line of output is the name.
pub fn linux_name_call(stick: &str) -> Call {
    call(2, false, stick, "upgrade-job.exe", &["linux-name"])
}

/// The name from the program's output: its last non-empty line, or None.
pub fn linux_name_from(output: &[String]) -> Option<String> {
    output.iter().map(|l| l.trim()).filter(|l| !l.is_empty()).last().map(String::from)
}

/// RUN-CONVERT.cmd's steps 1 and 3-4 as calls: the scan (and the capture),
/// then the job with its kickstart. `hash_file` is where the window wrote
/// the password hash (deleted after the call); `ack` is the acknowledgement
/// sentence when the person typed it on the data-loss path, else empty.
pub fn convert_calls(stick: &str, desktop: Desktop, hash_file: &str, ack: &str) -> Vec<Call> {
    let (d, s) = desktop.job_args();
    let mut job = vec!["write", "--stick", "{stick}", "--out", "{root}upgrade_", "--scan", "{root}upgrade_\\reports", "--desktop", d, "--start-at", s, "--if-cannot-keep", "stop", "--password-hash-file", hash_file, "--kickstart", "{root}upgrade_\\ks.cfg", "--stick-label", "UPGV0", "--manifest", "{root}SHA256SUMS"];
    if !ack.is_empty() {
        job.push("--acknowledge-data-loss");
        job.push(ack);
    }
    vec![
        call(1, false, stick, "upgrade-scan.exe", &["scan", "--json", "--out", "{root}upgrade_\\reports", "--kit", "{root}"]),
        call(1, true, stick, "upgrade-scan.exe", &["dump-machine", "{root}machine-capture.json"]),
        call(2, false, stick, "upgrade-job.exe", &job),
    ]
}

/// Step 6: the prologue (`upgrade-prologue start`, following Invoke-Prologue.ps1
/// -Start), with the word as typed. It restarts the computer itself when it
/// has armed; a stop leaves Windows as it was and exits non-zero.
pub fn prologue_start_call(stick: &str, word: &str, ack: &str) -> Call {
    let mut a = vec!["start", "--stick", "{stick}", "--confirm-word", word];
    if !ack.is_empty() {
        a.push("--acknowledge-data-loss");
        a.push(ack);
    }
    call(4, false, stick, "upgrade-prologue.exe", &a)
}

/// The prologue's own stop, for the screen: its "STOPPED at ..." line and
/// what follows it (the reason), else its last lines.
pub fn prologue_stop_lines(output: &[String]) -> Vec<String> {
    if let Some(i) = output.iter().position(|l| l.trim_start().starts_with("STOPPED")) {
        return output[i..].iter().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
    }
    refusal_lines(output)
}

// ---------------------------------------------------------------- erase, roll back, probe, cancel

/// The erase sentence (RISKS R27), byte for byte: the job writer's own.
pub fn erase_ok(typed: &str) -> bool {
    typed == upgrade_job::ERASE_STATEMENT
}

/// RUN-ERASE-AND-INSTALL.cmd's steps 1 and 5-6 as calls: the scan, then the
/// job that names every drive it will erase (-EraseEverything with the
/// sentence as typed; -AcknowledgeDataLoss on the data-loss variant).
pub fn erase_calls(stick: &str, desktop: Desktop, hash_file: &str, erase: &str, ack: &str) -> Vec<Call> {
    let (d, s) = desktop.job_args();
    let mut job = vec!["write", "--stick", "{stick}", "--out", "{root}upgrade_", "--scan", "{root}upgrade_\\reports", "--desktop", d, "--start-at", s, "--if-cannot-keep", "stop", "--erase-everything", erase, "--password-hash-file", hash_file, "--kickstart", "{root}upgrade_\\ks.cfg", "--stick-label", "UPGV0", "--manifest", "{root}SHA256SUMS"];
    if !ack.is_empty() {
        job.push("--acknowledge-data-loss");
        job.push(ack);
    }
    vec![
        call(1, false, stick, "upgrade-scan.exe", &["scan", "--json", "--out", "{root}upgrade_\\reports", "--kit", "{root}"]),
        call(1, true, stick, "upgrade-scan.exe", &["dump-machine", "{root}machine-capture.json"]),
        call(2, false, stick, "upgrade-job.exe", &job),
    ]
}

/// Step 6 of the erase launcher: the prologue with the sentence as typed
/// (-EraseConsent), the acknowledgement too on the data-loss variant.
pub fn erase_prologue_call(stick: &str, erase: &str, ack: &str) -> Call {
    let mut a = vec!["start", "--stick", "{stick}", "--erase-consent", erase];
    if !ack.is_empty() {
        a.push("--acknowledge-data-loss");
        a.push(ack);
    }
    call(4, false, stick, "upgrade-prologue.exe", &a)
}

/// ROLLBACK.cmd: the word, the snapshot the stick must hold, the call.
pub const ROLLBACK_WORD: &str = "ROLLBACK";
pub fn rollback_ok(typed: &str) -> bool {
    typed == ROLLBACK_WORD
}
pub fn rollback_snapshot_present(root: &Path) -> bool {
    root.join("upgrade_").join("esp-snapshot").join("SHA256SUMS").exists()
}
pub fn rollback_call(stick: &str) -> Call {
    call(1, false, stick, "upgrade-prologue.exe", &["rollback", "--stick", "{stick}"])
}

/// RUN-PROBE.cmd: the walk-away probe (it restarts the computer itself).
pub fn probe_call(stick: &str) -> Call {
    call(1, false, stick, "upgrade-prologue.exe", &["probe", "--stick", "{stick}"])
}

/// CANCEL-CONVERSION.cmd: the abort, with the stick so the prologue writes
/// its before-cancel capture there first.
pub fn abort_call(stick: &str) -> Call {
    call(1, false, stick, "upgrade-prologue.exe", &["abort", "--stick", "{stick}"])
}

/// The window's own temporary file for the hash, in %TEMP% as the launcher
/// puts it (never on the stick, never logged).
pub fn hash_file_path() -> PathBuf {
    let mut n = [0u8; 8];
    let _ = upgrade_job::password::os_random(&mut n);
    std::env::temp_dir().join(format!("upgrade-pw-{}.txt", n.iter().map(|b| format!("{:02x}", b)).collect::<String>()))
}

/// Windows PowerShell's Out-File -Encoding UTF8 writes a byte-order mark.
pub fn parse_json(bytes: &[u8]) -> Option<Value> {
    let b = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    serde_json::from_slice(b).ok()
}

#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    pub level: String,
    pub summary: String,
    pub groups: Vec<(String, Vec<String>)>,
}

/// The scanner's JSON report (upgrade-report-*.json): its Verdict block.
/// No verdict is not GREEN: the caller treats None as a stop.
pub fn verdict(report: &Value) -> Option<Verdict> {
    let v = report.get("Verdict")?;
    let level = v.get("Level")?.as_str()?.to_string();
    let summary = v.get("Summary").and_then(Value::as_str).unwrap_or("").to_string();
    let groups = match v.get("Groups") {
        Some(Value::Array(a)) => a.clone(),
        Some(o @ Value::Object(_)) => vec![o.clone()], // ConvertTo-Json flattens a one-item array
        _ => vec![],
    };
    let groups = groups
        .iter()
        .map(|g| {
            let label = g.get("Label").and_then(Value::as_str).unwrap_or("").to_string();
            let items = match g.get("Items") {
                Some(Value::Array(a)) => a.iter().filter_map(|x| x.as_str().map(String::from)).collect(),
                Some(Value::String(s)) => vec![s.clone()],
                _ => vec![],
            };
            (label, items)
        })
        .collect();
    Some(Verdict { level, summary, groups })
}

/// Whether the run may go on after the scan. Only GREEN and YELLOW go on, as
/// in the job writer; RED, a missing verdict or any other word stops here.
pub fn verdict_allows(v: Option<&Verdict>) -> bool {
    matches!(v.map(|v| v.level.as_str()), Some("GREEN") | Some("YELLOW"))
}

/// Whether the window's own stop after the scan applies. On the data-loss
/// path (the sentence typed, RISKS R23) a RED verdict goes on to the job
/// writer, which lifts exactly the two health refusals the sentence covers
/// and refuses every other RED; the window stopping first made the sentence
/// unusable through the window (the Aspire, 2026-10-10). A missing verdict
/// stops on every path.
pub fn scan_stops(v: Option<&Verdict>, acknowledged: bool) -> bool {
    if verdict_allows(v) {
        return false;
    }
    !(acknowledged && v.map(|v| v.level.as_str()) == Some("RED"))
}

/// The newest scanner JSON in the reports folder (names carry the time).
pub fn newest_report(dir: &Path) -> Option<PathBuf> {
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !(name.starts_with("upgrade-report-") && name.ends_with(".json")) {
            continue;
        }
        let t = e.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
        if best.as_ref().map(|(bt, _)| t > *bt).unwrap_or(true) {
            best = Some((t, p));
        }
    }
    best.map(|(_, p)| p)
}

/// The scripts' own reasons, for the stop screen, in their words. The job
/// writer prints "REFUSED: <why>", "REFUSED - no job written: <why>", or
/// "REFUSED - no job written:" followed by "- <why>" lines. With no REFUSED
/// line (a script threw), the last few lines it printed stand in. The full
/// output stays in the log and under Details.
pub fn refusal_lines(output: &[String]) -> Vec<String> {
    let mut out = vec![];
    let mut in_list = false;
    for l in output.iter().map(|l| l.trim()) {
        if let Some(i) = l.find("REFUSED") {
            let rest = l[i + "REFUSED".len()..].trim_start_matches(':').trim();
            let rest = rest.strip_prefix("- no job written:").unwrap_or(rest).trim();
            if !rest.is_empty() {
                out.push(rest.to_string());
            }
            in_list = true;
        } else if in_list && l.starts_with("- ") {
            out.push(l[2..].trim().to_string());
        } else {
            in_list = false;
        }
    }
    if out.is_empty() {
        out = output.iter().map(|l| l.trim().to_string()).filter(|l| !l.is_empty() && !l.starts_with("====")).collect();
        let n = out.len();
        out.drain(..n.saturating_sub(3));
    }
    out
}

/// One CSV line with double-quoted fields ("" is a quote), as Test-Handoff writes them.
pub fn csv_fields(line: &str) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    let mut q = false;
    let mut it = line.chars().peekable();
    while let Some(c) = it.next() {
        match (c, q) {
            ('"', true) if it.peek() == Some(&'"') => {
                cur.push('"');
                it.next();
            }
            ('"', _) => q = !q,
            (',', false) => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out
}

/// The `result` of the last row in the stick's v0-handoff.csv, written after
/// `since` (an RFC 3339 UTC time; the timestamps compare as text).
pub fn handoff_result(csv: &str, since: &str) -> Option<String> {
    let mut lines = csv.lines().map(|l| l.trim_start_matches('\u{feff}'));
    let header = csv_fields(lines.next()?);
    let ts = header.iter().position(|h| h == "timestamp")?;
    let res = header.iter().position(|h| h == "result")?;
    let last = lines.filter(|l| !l.trim().is_empty()).last()?;
    let f = csv_fields(last);
    let t = f.get(ts)?;
    if t.as_str() < since {
        return None;
    }
    f.get(res).cloned()
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResultView {
    pub heading: String,
    pub rows: Vec<(String, String)>,
    pub lines: Vec<String>,
}

fn word(r: Option<&str>) -> String {
    match r {
        Some("pass") => words::OK.into(),
        Some("fail") => words::PROBLEM.into(),
        _ => words::NOT_CHECKED.into(),
    }
}

fn with_detail(r: Option<&str>, d: Option<&str>) -> String {
    let w = word(r);
    match d.map(str::trim).filter(|d| !d.is_empty()) {
        Some(d) => format!("{} ({})", w, d),
        None => w,
    }
}

fn handoff_word(r: &str) -> String {
    match r {
        "fired-once" => words::HANDOFF_FIRED.into(),
        "ignored" => words::HANDOFF_IGNORED.into(),
        "persisted" => words::HANDOFF_PERSISTED.into(),
        "reordered" => words::HANDOFF_REORDERED.into(),
        other => other.to_string(),
    }
}

/// What came back on the stick, in plain words. `verify` is report/verify.json,
/// `refusal` report/refusal.json, `handoff` the return check's result. The
/// launcher deletes report/ before it arms, so anything there is this run's.
pub fn result_view(stick_found: bool, verify: Option<&Value>, refusal: Option<&Value>, handoff: Option<&str>) -> ResultView {
    if !stick_found {
        return ResultView { heading: words::RESULT_NO_STICK.into(), rows: vec![], lines: vec![words::RESULT_NO_STICK_LINE.into()] };
    }
    let mut rows = vec![];
    let mut lines = vec![];
    let heading = if let Some(r) = refusal {
        lines.push(r.get("reason").and_then(Value::as_str).unwrap_or("").to_string());
        words::RESULT_REFUSED
    } else if let Some(v) = verify {
        let s = |a: &str, b: &str| v.get(a).and_then(|x| x.get(b)).and_then(Value::as_str);
        rows.push((words::ROW_RIGHT_COMPUTER.into(), word(s("identity", "result"))));
        rows.push((words::ROW_SCREEN.into(), with_detail(s("hardware", "display"), None)));
        rows.push((words::ROW_WIFI.into(), with_detail(s("hardware", "wifi"), s("hardware", "wifi_detail"))));
        if s("hardware", "audio_firmware") != Some("skipped") {
            rows.push((words::ROW_SOUND.into(), with_detail(s("hardware", "audio_firmware"), None)));
        }
        let mbps = s("payload", "read_mbps").filter(|m| !m.is_empty()).map(|m| format!("read back at {} MB/s", m));
        rows.push((words::ROW_IMAGE.into(), with_detail(s("payload", "result"), mbps.as_deref())));
        words::RESULT_RAN
    } else {
        lines.push(words::RESULT_DID_NOT_RUN_LINE.into());
        words::RESULT_DID_NOT_RUN
    };
    if let Some(h) = handoff {
        rows.push((words::ROW_RESTART.into(), handoff_word(h)));
    }
    lines.push(words::RESULT_FOOT.into());
    ResultView { heading: heading.into(), rows, lines }
}

/// Seconds since 1970 as "YYYY-MM-DDTHH:MM:SSZ" (UTC), the form the rows use.
pub fn utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // civil-from-days (Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, m, d, rem / 3600, rem % 3600 / 60, rem % 60)
}

pub fn now_utc() -> String {
    utc(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0))
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The one-shot sign-in task that reopens the window after the restart:
/// the same shape as Test-Handoff's return-check task (a logon trigger for
/// the person who started it, elevated without a second prompt), which fired
/// on the Aspire. The window deletes it as soon as it starts.
pub fn reopen_task_xml(user: &str, exe: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo><Description>upgrade_: show the test result after the restart (removed when it runs)</Description></RegistrationInfo>
  <Triggers><LogonTrigger><Enabled>true</Enabled><UserId>{u}</UserId></LogonTrigger></Triggers>
  <Principals><Principal id="Author"><UserId>{u}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>HighestAvailable</RunLevel></Principal></Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT2H</ExecutionTimeLimit>
    <Enabled>true</Enabled>
  </Settings>
  <Actions Context="Author"><Exec><Command>{e}</Command><Arguments>--after-restart</Arguments></Exec></Actions>
</Task>
"#,
        u = xml(user),
        e = xml(exe)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn scan_call_matches_run_verify_cmd_step_1() {
        // RUN-VERIFY.cmd line 51 (upgrade-scan.ps1 -Json -OutDir), with %~dp0 = E:\, as the Rust scanner takes it
        let c = &verify_calls("E:")[0];
        assert_eq!(command_line(c), r"E:\upgrade-scan.exe scan --json --out E:\upgrade_\reports --kit E:\");
    }

    #[test]
    fn job_call_matches_run_verify_cmd_steps_2_and_3() {
        // RUN-VERIFY.cmd lines 57 and 68: the job (-VerifyOnly) and the kickstart from it, in one call
        let c = &verify_calls("E:")[2];
        assert_eq!(
            command_line(c),
            r"E:\upgrade-job.exe write --stick E: --out E:\upgrade_ --scan E:\upgrade_\reports --desktop kde --start-at desktop --verify-only --kickstart E:\upgrade_\ks.cfg --stick-label UPGV0 --manifest E:\SHA256SUMS"
        );
        assert!(!c.may_fail);
    }

    #[test]
    fn arm_call_matches_run_verify_cmd_step_4() {
        // line 82: Test-Handoff.ps1 -Arm -Auto -Payload shim -PayloadDrive E: -SuspendBitLocker
        assert_eq!(command_line(&arm_call("E:")), r"E:\upgrade-prologue.exe verify-arm --stick E: --auto --payload shim --suspend-bitlocker");
    }

    #[test]
    fn only_the_machine_capture_may_fail() {
        let v = verify_calls("E:");
        assert_eq!(v.iter().filter(|c| c.may_fail).count(), 1);
        assert!(v[1].args.iter().any(|a| a == "dump-machine"));
        assert!(!arm_call("E:").may_fail);
    }

    #[test]
    fn convert_calls_match_run_convert_cmd() {
        // RUN-CONVERT.cmd lines 61-62 (the scan), 102 (the name), 124 (the job, -IfCannotKeep stop -PasswordHashFile), 137 (the kickstart)
        let v = convert_calls("E:", Desktop::Gnome, r"C:\Users\a\AppData\Local\Temp\upgrade-pw-1.txt", "");
        assert_eq!(command_line(&v[0]), r"E:\upgrade-scan.exe scan --json --out E:\upgrade_\reports --kit E:\");
        assert!(v[1].may_fail);
        assert_eq!(
            command_line(&v[2]),
            r"E:\upgrade-job.exe write --stick E: --out E:\upgrade_ --scan E:\upgrade_\reports --desktop gnome --start-at desktop --if-cannot-keep stop --password-hash-file C:\Users\a\AppData\Local\Temp\upgrade-pw-1.txt --kickstart E:\upgrade_\ks.cfg --stick-label UPGV0 --manifest E:\SHA256SUMS"
        );
        assert_eq!(command_line(&linux_name_call("E:")), r"E:\upgrade-job.exe linux-name");
        // line 187: the prologue with the word as typed
        assert_eq!(command_line(&prologue_start_call("E:", "CONVERT", "")), r"E:\upgrade-prologue.exe start --stick E: --confirm-word CONVERT");
    }

    #[test]
    fn the_data_loss_path_carries_the_sentence_to_the_job_and_the_prologue() {
        // RUN-CONVERT-ACCEPTING-DATA-LOSS.cmd: -AcknowledgeDataLoss "%ACK%" on both
        let s = upgrade_job::RISK_STATEMENT;
        let v = convert_calls("E:", Desktop::Kde, "h", s);
        assert_eq!(v[2].args[v[2].args.len() - 2..], ["--acknowledge-data-loss".to_string(), s.to_string()]);
        assert_eq!(command_line(&prologue_start_call("E:", "CONVERT", s)), format!(r#"E:\upgrade-prologue.exe start --stick E: --confirm-word CONVERT --acknowledge-data-loss "{}""#, s));
        assert!(ack_ok(s));
        assert!(!ack_ok(&s.to_lowercase()));
        assert!(!ack_ok(&format!("{} ", s)));
    }

    #[test]
    fn the_word_is_compared_byte_for_byte() {
        assert!(confirm_ok("CONVERT"));
        assert!(!confirm_ok("convert"));
        assert!(!confirm_ok("CONVERT "));
        assert!(!confirm_ok(""));
    }

    #[test]
    fn the_console_choice_still_installs_kde() {
        assert_eq!(Desktop::Console.job_args(), ("kde", "console"));
        assert_eq!(Desktop::Kde.job_args(), ("kde", "desktop"));
        assert_eq!(Desktop::Gnome.job_args(), ("gnome", "desktop"));
    }

    #[test]
    fn the_password_rules_are_the_scripts() {
        assert_eq!(password_refusal("", ""), Some("the password is empty"));
        assert_eq!(password_refusal("a", "b"), Some("the two entries are not the same"));
        assert_eq!(password_refusal("a\tb", "a\tb"), Some("the password contains a control character"));
        assert_eq!(password_refusal("correct horse", "correct horse"), None);
        let h = password_hash("x").unwrap();
        assert!(h.starts_with("$6$"), "{h}");
    }

    #[test]
    fn the_linux_name_is_the_programs_last_line() {
        assert_eq!(linux_name_from(&["".into(), "ann".into(), "".into()]), Some("ann".into()));
        assert_eq!(linux_name_from(&["".into()]), None);
    }

    #[test]
    fn a_prologue_stop_shows_its_own_words() {
        let out: Vec<String> = ["  1.  re-validating job.json...", "      ! disk identity differs", "", "  STOPPED at revalidate: job.json no longer matches this machine", "  outcome.json (stopped) written to the stick"].iter().map(|s| s.to_string()).collect();
        assert_eq!(prologue_stop_lines(&out), vec!["STOPPED at revalidate: job.json no longer matches this machine", "outcome.json (stopped) written to the stick"]);
    }

    #[test]
    fn the_convert_kit_list_adds_what_the_install_needs() {
        for f in ["upgrade_\\outcome.sh", "upgrade_\\LiveOS\\gnome.squashfs", "upgrade-prologue.exe"] {
            assert!(KIT_FILES_CONVERT.contains(&f), "{f}");
        }
        assert!(hash_file_path().to_string_lossy().contains("upgrade-pw-"));
    }

    #[test]
    fn erase_calls_match_run_erase_and_install_cmd() {
        // RUN-ERASE-AND-INSTALL.cmd line 155: -IfCannotKeep stop -EraseEverything "%ERASE%" -PasswordHashFile; line 189: -EraseConsent "%ERASE%"
        let e = upgrade_job::ERASE_STATEMENT;
        let v = erase_calls("E:", Desktop::Kde, "h", e, "");
        assert_eq!(
            command_line(&v[2]),
            format!(r#"E:\upgrade-job.exe write --stick E: --out E:\upgrade_ --scan E:\upgrade_\reports --desktop kde --start-at desktop --if-cannot-keep stop --erase-everything "{e}" --password-hash-file h --kickstart E:\upgrade_\ks.cfg --stick-label UPGV0 --manifest E:\SHA256SUMS"#)
        );
        assert_eq!(command_line(&erase_prologue_call("E:", e, "")), format!(r#"E:\upgrade-prologue.exe start --stick E: --erase-consent "{e}""#));
        // the data-loss variant (line 181, 215): -AcknowledgeDataLoss on both
        let s = upgrade_job::RISK_STATEMENT;
        let v = erase_calls("E:", Desktop::Kde, "h", e, s);
        assert_eq!(v[2].args[v[2].args.len() - 2..], ["--acknowledge-data-loss".to_string(), s.to_string()]);
        assert!(erase_prologue_call("E:", e, s).args.contains(&"--acknowledge-data-loss".to_string()));
        assert!(erase_ok(e));
        assert!(!erase_ok(&e.replace("nothing", "Nothing")));
    }

    #[test]
    fn rollback_probe_and_cancel_calls() {
        assert_eq!(command_line(&rollback_call("E:")), r"E:\upgrade-prologue.exe rollback --stick E:");
        assert_eq!(command_line(&probe_call("E:")), r"E:\upgrade-prologue.exe probe --stick E:");
        assert_eq!(command_line(&abort_call("E:")), r"E:\upgrade-prologue.exe abort --stick E:");
        assert!(rollback_ok("ROLLBACK"));
        assert!(!rollback_ok("rollback"));
        let d = std::env::temp_dir().join(format!("upg-window-rb-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        assert!(!rollback_snapshot_present(&d));
        std::fs::create_dir_all(d.join("upgrade_").join("esp-snapshot")).unwrap();
        std::fs::write(d.join("upgrade_").join("esp-snapshot").join("SHA256SUMS"), "x").unwrap();
        assert!(rollback_snapshot_present(&d));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_path_with_a_space_is_quoted_on_the_log_line() {
        let c = Call { step: 1, program: r"E:\upgrade-scan.exe".into(), args: vec!["--out".into(), r"E:\my reports".into()], may_fail: false };
        assert_eq!(command_line(&c), r#"E:\upgrade-scan.exe --out "E:\my reports""#);
    }

    #[test]
    fn missing_kit_files_are_named() {
        let d = std::env::temp_dir().join(format!("upg-window-kit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("upgrade_").join("LiveOS")).unwrap();
        for f in KIT_FILES_VERIFY {
            let p = d.join(win_rel(f));
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, "x").unwrap();
        }
        assert!(missing_kit_files(&d).is_empty());
        std::fs::remove_file(d.join("upgrade_").join("LiveOS").join("kde.squashfs")).unwrap();
        assert_eq!(missing_kit_files(&d), vec!["upgrade_\\LiveOS\\kde.squashfs"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn json_with_a_byte_order_mark_parses() {
        let mut b = vec![0xEF, 0xBB, 0xBF];
        b.extend_from_slice(br#"{"Verdict":{"Level":"GREEN"}}"#);
        assert_eq!(verdict(&parse_json(&b).unwrap()).unwrap().level, "GREEN");
    }

    #[test]
    fn red_stops_and_green_yellow_go_on() {
        let red = json!({"Verdict":{"Level":"RED","Summary":"Do not convert","Groups":[{"Label":"Blocks","Items":["Disk: bad blocks"]}]}});
        let v = verdict(&red).unwrap();
        assert!(!verdict_allows(Some(&v)));
        assert_eq!(v.groups, vec![("Blocks".to_string(), vec!["Disk: bad blocks".to_string()])]);
        assert!(verdict_allows(verdict(&json!({"Verdict":{"Level":"GREEN"}})).as_ref()));
        assert!(verdict_allows(verdict(&json!({"Verdict":{"Level":"YELLOW"}})).as_ref()));
    }

    #[test]
    fn the_sentence_hands_a_red_verdict_to_the_job_writer_and_nothing_else() {
        let red = verdict(&json!({"Verdict":{"Level":"RED"}}));
        let yellow = verdict(&json!({"Verdict":{"Level":"YELLOW"}}));
        assert!(scan_stops(red.as_ref(), false), "RED without the sentence stops");
        assert!(!scan_stops(red.as_ref(), true), "RED with the sentence goes to the job writer");
        assert!(!scan_stops(yellow.as_ref(), false));
        assert!(scan_stops(None, true), "no verdict stops even with the sentence");
        assert!(scan_stops(verdict(&json!({"Verdict":{"Level":"red"}})).as_ref(), true), "an unknown word stops even with the sentence");
    }

    #[test]
    fn no_verdict_or_an_unknown_word_stops() {
        assert!(!verdict_allows(verdict(&json!({})).as_ref()));
        assert!(!verdict_allows(verdict(&json!({"Verdict":{"Level":"green"}})).as_ref()));
        assert!(!verdict_allows(None));
    }

    #[test]
    fn a_one_item_group_flattened_by_convertto_json_is_read() {
        let r = json!({"Verdict":{"Level":"YELLOW","Groups":{"Label":"Before","Items":"Wi-Fi: needs 6.7"}}});
        assert_eq!(verdict(&r).unwrap().groups, vec![("Before".to_string(), vec!["Wi-Fi: needs 6.7".to_string()])]);
    }

    #[test]
    fn refusal_lines_come_from_the_scripts_words() {
        let out = vec![
            "  scanning...".to_string(),
            "  REFUSED: the scan says RED".to_string(),
            "  REFUSED - no job written: no password was chosen".to_string(),
        ];
        assert_eq!(refusal_lines(&out), vec!["the scan says RED", "no password was chosen"]);
        // New-Job.ps1's list form
        let list = vec![
            "  REFUSED - no job written:".to_string(),
            "    - BitLocker state is unknown".to_string(),
            "    - legacy BIOS".to_string(),
            "".to_string(),
            "  - not a reason".to_string(),
        ];
        assert_eq!(refusal_lines(&list), vec!["BitLocker state is unknown", "legacy BIOS"]);
    }

    #[test]
    fn a_script_that_threw_shows_its_last_lines() {
        let out: Vec<String> = ["a", "b", "", "c", "Could not register the return check; Nothing is armed.", "==== exit 1"].iter().map(|s| s.to_string()).collect();
        assert_eq!(refusal_lines(&out), vec!["b", "c", "Could not register the return check; Nothing is armed."]);
    }

    #[test]
    fn csv_fields_handle_quotes_and_commas() {
        assert_eq!(csv_fields(r#""a","b, c","say ""hi""""#), vec!["a", "b, c", r#"say "hi""#]);
    }

    #[test]
    fn handoff_result_is_the_last_row_after_the_run_started() {
        let csv = "\u{feff}timestamp,harness,vendor,model,firmware_version,secureboot,bitlocker,payload,failmode,result,keypress_free,windows_returned,notes\n\
                   \"2026-09-12T20:00:00.0000000Z\",\"0.3.1\",\"Acer\",\"A515\",\"1.1\",\"True\",\"off\",\"shimx64.efi\",\"\",\"fired-once\",\"y\",\"y\",\"[harness: x]\"\n\
                   \"2026-09-27T21:00:00.0000000Z\",\"0.3.1\",\"Acer\",\"A515\",\"1.1\",\"True\",\"off\",\"shimx64.efi\",\"\",\"ignored\",\"unknown\",\"y\",\"[harness: y]\"\n";
        assert_eq!(handoff_result(csv, "2026-09-27T20:00:00Z").as_deref(), Some("ignored"));
        // a row from an earlier run is not this run's result
        assert_eq!(handoff_result(csv, "2026-09-28T00:00:00Z"), None);
    }

    #[test]
    fn result_after_a_verify_that_ran() {
        let v = json!({"identity":{"result":"pass"},"hardware":{"display":"pass","wifi":"pass","wifi_detail":"wlp1s0 sees 28 networks","audio_firmware":"skipped"},"payload":{"result":"pass","read_mbps":"22.6"}});
        let r = result_view(true, Some(&v), None, Some("fired-once"));
        assert_eq!(r.heading, words::RESULT_RAN);
        assert!(r.rows.contains(&(words::ROW_WIFI.to_string(), "works (wlp1s0 sees 28 networks)".to_string())));
        assert!(r.rows.contains(&(words::ROW_IMAGE.to_string(), "works (read back at 22.6 MB/s)".to_string())));
        assert!(!r.rows.iter().any(|(k, _)| k == words::ROW_SOUND), "skipped sound firmware is not a row");
        assert_eq!(r.rows.last().unwrap().1, words::HANDOFF_FIRED);
    }

    #[test]
    fn a_failed_check_says_problem_not_works() {
        let v = json!({"identity":{"result":"fail"},"hardware":{"display":"fail","wifi":"skipped"},"payload":{"result":"fail"}});
        let r = result_view(true, Some(&v), None, None);
        assert!(r.rows.contains(&(words::ROW_RIGHT_COMPUTER.to_string(), words::PROBLEM.to_string())));
        assert!(r.rows.contains(&(words::ROW_WIFI.to_string(), words::NOT_CHECKED.to_string())));
    }

    #[test]
    fn a_refusal_wins_over_everything_else() {
        let r = result_view(true, Some(&json!({})), Some(&json!({"reason":"the copy of Linux on this USB stick is damaged"})), Some("fired-once"));
        assert_eq!(r.heading, words::RESULT_REFUSED);
        assert_eq!(r.lines[0], "the copy of Linux on this USB stick is damaged");
    }

    #[test]
    fn nothing_on_the_stick_means_linux_did_not_start() {
        let r = result_view(true, None, None, Some("ignored"));
        assert_eq!(r.heading, words::RESULT_DID_NOT_RUN);
        assert_eq!(r.rows, vec![(words::ROW_RESTART.to_string(), words::HANDOFF_IGNORED.to_string())]);
        assert_eq!(result_view(false, None, None, None).heading, words::RESULT_NO_STICK);
    }

    #[test]
    fn utc_times() {
        assert_eq!(utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(utc(1_790_552_142), "2026-09-27T23:35:42Z");
    }

    #[test]
    fn the_reopen_task_escapes_and_names_the_person() {
        let x = reopen_task_xml(r"PC\Ann & Bo", r"C:\ProgramData\upgrade_\window\UPGRADE.exe");
        assert!(x.contains(r"<UserId>PC\Ann &amp; Bo</UserId>"));
        assert!(x.contains("<Arguments>--after-restart</Arguments>"));
        assert!(x.contains("<RunLevel>HighestAvailable</RunLevel>"));
        assert_eq!(x.matches("<LogonTrigger>").count(), 1);
    }
}
