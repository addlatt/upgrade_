//! The verify flow's handoff: `Test-Handoff.ps1` (the V0 harness, 0.3.1)
//! as judging functions. The verify flow (RUN-VERIFY.cmd, the window) arms
//! the one-shot boot handoff with it and classifies the return; nothing on
//! the internal drive changes. Pure: every function here is replayed
//! against the script's answers (`tests/verify.rs`, RISKS R32).
//!
//! The classifier, the manage-bde parse, the GRUB environment block and the
//! stick lookup are the prologue's own (`judge`); this module adds what the
//! harness has on top: the fail modes, the payload table, the evidence row.

use crate::judge;

/// The script version these functions follow.
pub const FOLLOWS_HARNESS: &str = "0.3.1";
/// The marker the Shell payload writes to the root of the stick (startup.nsh).
pub const FIRED_MARKER: &str = "fired.txt";
/// The one-shot logon task the arm registers and the check always removes.
pub const RETURN_TASK_NAME: &str = "upgrade_ V0 handoff return check";
/// v0-handoff.csv's columns, in the script's order.
pub const CSV_HEADER: &str = "timestamp,harness,vendor,model,firmware_version,secureboot,bitlocker,payload,failmode,result,keypress_free,windows_returned,notes";
/// Where each payload lives on the stick (make-kit.sh, handoff-payload/README.md).
pub const PAYLOAD_PATHS: [(&str, &str); 2] = [("shim", "\\EFI\\BOOT\\BOOTX64.EFI"), ("shell", "\\EFI\\SHELL\\SHELLX64.EFI")];
/// The three deliberately broken arms the harness knows.
pub const FAIL_MODES: [&str; 3] = ["NoFile", "SecureBootUnsigned", "NoSuspend"];

fn eq_ci(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// Get-PayloadPath: the EFI path the boot entry points at, for a payload and
/// a fail mode. An unknown payload is refused (the script throws).
pub fn payload_path(payload: &str, fail_mode: &str) -> Result<String, String> {
    if eq_ci(fail_mode, "NoFile") {
        return Ok("\\EFI\\BOOT\\DOES-NOT-EXIST.EFI".into());
    }
    PAYLOAD_PATHS.iter().find(|(n, _)| eq_ci(n, payload)).map(|(_, p)| p.to_string()).ok_or_else(|| format!("Unknown payload '{payload}'."))
}

/// Get-HandoffResult with the harness's fail modes: NoFile and
/// SecureBootUnsigned expect a clean fall-through to Windows, so for them
/// `ignored` is the pass and a firing is `persisted` (loud). NoSuspend is a
/// baseline-shaped row.
pub fn handoff_result(fired: bool, sequence_cleared: bool, order_unchanged: bool, fail_mode: &str) -> &'static str {
    if eq_ci(fail_mode, "NoFile") || eq_ci(fail_mode, "SecureBootUnsigned") {
        if !fired && order_unchanged {
            return "ignored";
        }
        if fired {
            return "persisted";
        }
        return "reordered";
    }
    judge::handoff_result(fired, sequence_cleared, order_unchanged)
}

/// Get-DriveRoot, with the harness's own wording.
pub fn payload_drive_root(letter: &str) -> Result<String, String> {
    judge::drive_root(letter).map_err(|_| format!("PayloadDrive must be a single drive letter, got '{letter}'."))
}

/// The header line as the script creates it: UTF-8 without a byte-order
/// mark, CRLF (PS 5.1's Out-File -Encoding UTF8 would put a BOM in front of
/// the first column name, which naive parsers then read as part of it).
pub fn csv_header_bytes() -> Vec<u8> {
    format!("{CSV_HEADER}\r\n").into_bytes()
}

/// One double-quoted CSV field, a quote doubled.
pub fn csv_escape(v: &str) -> String {
    format!("\"{}\"", v.replace('"', "\"\""))
}

/// One evidence row, fields joined as the script joins them.
pub fn csv_row(fields: &[&str]) -> String {
    fields.iter().map(|f| csv_escape(f)).collect::<Vec<_>>().join(",")
}

/// The harness-written part of the notes: what the machine was and how the
/// marker was read, independent of the operator.
pub fn harness_note(os_caption: &str, os_build: &str, bitlocker_source: &str, fired_via: &[String], auto: bool, payload: &str) -> String {
    let via = if fired_via.is_empty() { "none".to_string() } else { fired_via.join("+") };
    format!("[harness: os={os_caption} {os_build}; bitlocker-via={bitlocker_source}; fired-via={via}; mode={}; payload={payload}]", if auto { "auto" } else { "manual" })
}

/// The notes column: the harness's facts first, the operator's words after.
pub fn notes_with(harness: &str, notes: &str) -> String {
    if notes.trim().is_empty() { harness.to_string() } else { format!("{harness} {notes}") }
}

/// The popup's answer as the row records it: 6 = Yes (no key was needed),
/// 7 = No, anything else (the timeout) 'unknown'.
pub fn keypress_answer(code: i64) -> &'static str {
    match code {
        6 => "y",
        7 => "n",
        _ => "unknown",
    }
}

/// The classifier's words for the screen (the script's two red lines).
pub fn result_remark(result: &str) -> Option<&'static str> {
    match result {
        "persisted" => Some("-> firmware did NOT consume the one-shot. Shipping prologue needs a cleanup-on-return step."),
        "reordered" => Some("-> firmware permanently changed the boot order. Design input, not just a data point."),
        _ => None,
    }
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The one-shot return check as a logon task for the person who armed, run
/// elevated without a second prompt (Register-ScheduledTask -AtLogOn -User,
/// RunLevel Highest, one hour), as `schtasks /Create /XML` takes it.
pub fn return_task_xml(user: &str, exe: &str, state_dir: &str, results_csv: Option<&str>) -> String {
    let mut args = format!("verify-check --auto --state-dir \"{state_dir}\"");
    if let Some(c) = results_csv {
        args.push_str(&format!(" --results-csv \"{c}\""));
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo><Description>upgrade_: the verify flow's return check, once, at the next sign-in (removed when it runs)</Description></RegistrationInfo>
  <Triggers><LogonTrigger><Enabled>true</Enabled><UserId>{u}</UserId></LogonTrigger></Triggers>
  <Principals><Principal id="Author"><UserId>{u}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>HighestAvailable</RunLevel></Principal></Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT1H</ExecutionTimeLimit>
    <Enabled>true</Enabled>
  </Settings>
  <Actions Context="Author"><Exec><Command>{e}</Command><Arguments>{a}</Arguments></Exec></Actions>
</Task>
"#,
        u = xml(user),
        e = xml(exe),
        a = xml(&args)
    )
}

/// The live half, Windows only: Invoke-Arm and Invoke-Check as the script
/// does them, every tool call through the recorder. The arm is reversible
/// (the BCD is exported first, one entry is added, the one-shot is set);
/// the check restores everything whatever happened and writes one row.
#[cfg(windows)]
pub mod live {
    use super::*;
    use crate::live as plive;
    use crate::tools::Recorder;
    use crate::val::{s, truthy};
    use serde_json::{json, Value};
    use std::path::Path;
    use upgrade_scan::collect::wmi::{Wmi, CIMV2};
    use upgrade_scan::ps::capture;

    pub struct ArmArgs {
        pub payload_drive: String,
        pub payload: String,
        pub auto: bool,
        pub suspend_bitlocker: bool,
        pub fail_mode: String,
        pub state_dir: String,
        pub results_csv: Option<String>,
    }

    fn say(line: &str) {
        println!("{line}");
    }

    /// The default state directory: %ProgramData%\upgrade_\v0 (the window
    /// waits on handoff-state.json there after the restart).
    pub fn default_state_dir() -> String {
        format!("{}\\upgrade_\\v0", std::env::var("ProgramData").unwrap_or_else(|_| "C:\\ProgramData".into()))
    }

    /// Get-VolumeUniqueId for a drive letter, from the same list the
    /// return check searches.
    fn volume_unique_id(root: &str) -> Option<String> {
        let letter = root.chars().next()?.to_ascii_uppercase().to_string();
        plive::volumes().as_array()?.iter().find(|v| s(&v["DriveLetter"]).eq_ignore_ascii_case(&letter)).map(|v| s(&v["UniqueId"])).filter(|id| !id.is_empty())
    }

    fn machine() -> Value {
        let w = Wmi::connect(CIMV2).ok();
        let one = |class: &str, props: &[&str]| w.as_ref().and_then(|w| w.query(class, props).ok()).and_then(|l| l.into_iter().next()).unwrap_or(Value::Null);
        let cs = one("Win32_ComputerSystem", &["Manufacturer", "Model"]);
        let bios = one("Win32_BIOS", &["SMBIOSBIOSVersion"]);
        let os = one("Win32_OperatingSystem", &["Caption", "BuildNumber"]);
        json!({"Vendor": s(&cs["Manufacturer"]), "Model": s(&cs["Model"]), "Firmware": s(&bios["SMBIOSBIOSVersion"]), "OsCaption": s(&os["Caption"]), "OsBuild": s(&os["BuildNumber"])})
    }

    fn write_state(path: &Path, v: &Value) -> Result<(), String> {
        std::fs::write(path, serde_json::to_string_pretty(v).map_err(|e| e.to_string())? + "\n").map_err(|e| format!("{}: {e}", path.display()))
    }

    fn task_present(rec: &mut Recorder) -> bool {
        rec.run("schtasks", &["/Query", "/TN", RETURN_TASK_NAME]).ok()
    }

    /// Unregister-ReturnTask: true when a task was there and went.
    pub fn unregister_return_task(rec: &mut Recorder) -> bool {
        if !task_present(rec) {
            return false;
        }
        rec.run("schtasks", &["/Delete", "/TN", RETURN_TASK_NAME, "/F"]).ok()
    }

    /// Invoke-Arm. Returns Ok when armed (in -Auto the restart is already
    /// counting down); every Err left the machine as it was.
    pub fn arm(rec: &mut Recorder, a: &ArmArgs, version_line: &str) -> Result<(), String> {
        let state = Path::new(&a.state_dir);
        std::fs::create_dir_all(state).map_err(|e| format!("could not create {}: {e}", state.display()))?;
        let state_file = state.join("handoff-state.json");
        if state_file.exists() {
            return Err(format!("A test is already armed (state exists in {}). Run -Check first, or delete the state directory.", state.display()));
        }
        let root = payload_drive_root(&a.payload_drive)?;
        if !Path::new(&root).exists() {
            return Err(format!("Payload drive {root} not found."));
        }
        // The one refusal that matters most: the entry must point at a payload that exists. NoFile skips this on purpose.
        let efi_path = payload_path(&a.payload, &a.fail_mode)?;
        let payload_efi = Path::new(&root).join(efi_path.trim_start_matches('\\'));
        if !a.fail_mode.eq_ignore_ascii_case("NoFile") && !payload_efi.exists() {
            return Err(format!("No payload at {}. See handoff-payload\\README.md to build the stick.", payload_efi.display()));
        }
        let stick_id = volume_unique_id(&root);
        if a.auto && stick_id.is_none() {
            return Err(format!("Could not read the stick's volume id for {root}; -Auto needs it to find the stick again after the reboot."));
        }
        // a stale marker from a previous run would produce a false 'fired-once'
        let marker = Path::new(&root).join(FIRED_MARKER);
        if marker.exists() {
            std::fs::remove_file(&marker).map_err(|e| format!("could not remove the old {FIRED_MARKER}: {e}"))?;
        }
        let grub_env_reset = plive::reset_grub_env(&root);
        let m = machine();
        let sb = plive::secure_boot();
        let blq = plive::bitlocker_state(rec);
        let bl = s(&blq["State"]);
        say("");
        say(&format!("  upgrade_  V0 handoff test  -  ARM  ({version_line})"));
        say(&format!("  {} {}   firmware {}", s(&m["Vendor"]), s(&m["Model"]), s(&m["Firmware"])));
        say(&format!("  {} build {}", s(&m["OsCaption"]), s(&m["OsBuild"])));
        say(&format!("  Secure Boot: {sb}    BitLocker(C:): {bl} (via {})    payload: {} at {root}{}", s(&blq["Source"]), a.payload, efi_path.trim_start_matches('\\')));
        if a.auto {
            say("  AUTO: the return check will run itself at the next logon");
        }
        if grub_env_reset {
            say(&format!("  shim payload detected: {} reset to a clean block", plive::GRUB_ENV_REL));
        }
        if !a.fail_mode.is_empty() {
            say(&format!("  FAIL MODE: {}", a.fail_mode));
        }
        say("");
        // Refuse-by-default, before the BCD is touched (the script's words).
        if bl == "unknown" {
            if !s(&blq["Raw"]).is_empty() {
                say(&format!("  manage-bde said:\n{}", s(&blq["Raw"])));
            }
            return Err("BitLocker state on C: could not be determined; refusing to arm. Make it known (Settings > Privacy & security > Device encryption, or manage-bde -status C:) and re-run.".into());
        }
        let no_suspend = a.fail_mode.eq_ignore_ascii_case("NoSuspend");
        if bl == "on" && !a.suspend_bitlocker && !no_suspend {
            return Err("BitLocker is ON. Refusing to arm without suspension: re-run with -SuspendBitLocker (the shipping default), or -FailMode NoSuspend if the no-suspend path is the experiment. Have the recovery key saved somewhere that is not this computer first.".into());
        }
        // 1. the undo button, before anything else
        let backup = state.join("bcd-backup.bin");
        say("  exporting BCD backup...");
        if !rec.run("bcdedit", &["/export", &backup.to_string_lossy()]).ok() {
            return Err("bcdedit /export failed; refusing to arm without a backup.".into());
        }
        // 2. the boot order before
        let before = plive::fwbootmgr_snapshot(rec);
        // 3. BitLocker suspended for one restart, unless the no-suspend path is the experiment
        let mut did_suspend = false;
        if bl == "on" && !no_suspend {
            say("  suspending BitLocker for one reboot...");
            let r = rec.run("manage-bde", &["-protectors", "-disable", "C:", "-rebootcount", "1"]).clone();
            if !r.ok() {
                return Err(format!("manage-bde could not suspend BitLocker; refusing to arm. Output: {}", r.lines().join(" ")));
            }
            did_suspend = true;
        }
        if bl == "on" && no_suspend {
            say("  ! NoSuspend: BitLocker stays ON through the handoff. Recovery key at hand?");
        }
        // 4. the sequence under test, verbatim from docs/architecture.md
        say("  creating one-time boot entry...");
        let copy = rec.run("bcdedit", &["/copy", "{bootmgr}", "/d", "upgrade_ V0 handoff test"]).clone();
        if !copy.ok() {
            return Err(format!("bcdedit /copy failed: {}", copy.text()));
        }
        let guid = capture(r"(\{[0-9a-fA-F-]{36}\})", "", &copy.text()).map(String::from).ok_or_else(|| format!("Could not parse the new entry GUID from: {}", copy.text()))?;
        let _ = rec.run("bcdedit", &["/set", &guid, "device", &format!("partition={}", root.trim_end_matches('\\'))]);
        let _ = rec.run("bcdedit", &["/set", &guid, "path", &efi_path]);
        if !rec.run("bcdedit", &["/set", "{fwbootmgr}", "bootsequence", &guid]).ok() {
            let _ = rec.run("bcdedit", &["/delete", &guid]);
            return Err("Setting bootsequence failed; test entry removed.".into());
        }
        // 5. everything the check needs
        let record = json!({
            "HarnessVersion": FOLLOWS_HARNESS, "HarnessProgram": version_line, "ArmedUtc": plive::now_o(), "Guid": guid, "PayloadRoot": root, "PayloadEfi": efi_path,
            "Payload": a.payload, "StickUniqueId": stick_id, "Auto": a.auto, "ResultsCsvArg": a.results_csv, "FailMode": a.fail_mode, "DidSuspend": did_suspend,
            "Vendor": m["Vendor"], "Model": m["Model"], "Firmware": m["Firmware"], "OsCaption": m["OsCaption"], "OsBuild": m["OsBuild"],
            "SecureBoot": sb, "BitLocker": bl, "BitLockerSource": blq["Source"], "GrubEnvArmed": grub_env_reset, "Before": before, "BcdBackup": backup.to_string_lossy()
        });
        write_state(&state_file, &record)?;
        if !a.auto {
            say("");
            say("  ARMED.");
            say("  Reboot now, watch what happens, then run:  upgrade-prologue verify-check");
            say("");
            say("  If nothing is watching the screen, that is fine - the payload records");
            say("  itself. But note by hand whether any keypress was needed.");
            say("");
            return Ok(());
        }
        // 6. -Auto: the return check runs itself at the next sign-in, from a copy in the state
        //    directory (the stick's letter may change). If any of this fails the entry goes again.
        let registered = (|| -> Result<(), String> {
            let me = std::env::current_exe().map_err(|e| e.to_string())?;
            let exe = state.join("upgrade-prologue.exe");
            if std::fs::canonicalize(&me).ok() != std::fs::canonicalize(&exe).ok() {
                std::fs::copy(&me, &exe).map_err(|e| format!("copying the program to the state directory: {e}"))?;
            }
            let user = upgrade_scan::collect::win::account_name();
            if user.is_empty() {
                return Err("could not name the signed-in account for the logon task".into());
            }
            let xml = return_task_xml(&user, &exe.to_string_lossy(), &a.state_dir, a.results_csv.as_deref());
            let xml_path = state.join("return-task.xml");
            let mut bytes = vec![0xFF, 0xFE];
            for u in xml.encode_utf16() {
                bytes.extend_from_slice(&u.to_le_bytes());
            }
            std::fs::write(&xml_path, bytes).map_err(|e| e.to_string())?;
            let xp = xml_path.to_string_lossy().to_string();
            let r = rec.run("schtasks", &["/Create", "/TN", RETURN_TASK_NAME, "/XML", &xp, "/F"]).clone();
            if !r.ok() {
                return Err(format!("schtasks: {}", r.text().trim()));
            }
            if !task_present(rec) {
                return Err("task not present after registration".into());
            }
            Ok(())
        })();
        if let Err(e) = registered {
            let _ = rec.run("bcdedit", &["/deletevalue", "{fwbootmgr}", "bootsequence"]);
            let _ = rec.run("bcdedit", &["/delete", &guid]);
            let _ = std::fs::remove_file(&state_file);
            return Err(format!("Could not register the return check ({e}); the boot entry was removed again. Nothing is armed."));
        }
        say("");
        say("  ARMED. Rebooting in 20 seconds.");
        say("");
        let _ = rec.run("shutdown", &["/r", "/t", "20", "/c", "upgrade_ V0 handoff test: rebooting to test the boot handoff. Leave the USB stick in."]);
        plive::show_popup("Armed. This computer restarts in 20 seconds.\n\nLeave the USB stick plugged in. Watch the screen if you can.\n\nWhen Windows comes back, sign in as usual - the result appears by itself.", "upgrade_ V0 handoff test", 15, 0);
        Ok(())
    }

    fn read_line(prompt: &str) -> String {
        use std::io::{BufRead, Write};
        print!("{prompt}");
        let _ = std::io::stdout().flush();
        let mut l = String::new();
        let _ = std::io::stdin().lock().read_line(&mut l);
        l.trim().to_string()
    }

    /// Resolve-ResultsCsv: the one given, else the repo's evidence file when
    /// this program runs from the source tree, else the state directory's.
    fn resolve_results_csv(given: Option<&str>, state: &Path) -> String {
        if let Some(c) = given {
            return c.to_string();
        }
        if let Ok(me) = std::env::current_exe() {
            if let Some(repo) = me.parent().and_then(|d| d.parent()).and_then(|d| d.parent()).map(|d| d.join("docs").join("validation-results")) {
                if repo.is_dir() {
                    return repo.join("v0-handoff.csv").to_string_lossy().to_string();
                }
            }
        }
        state.join("v0-handoff.csv").to_string_lossy().to_string()
    }

    /// Invoke-Check. Returns the result word; the row is written, the entry
    /// and the task are gone, whatever the result.
    pub fn check(rec: &mut Recorder, auto: bool, restore_bcd: bool, state_dir: &str, results_csv: Option<&str>, version_line: &str) -> Result<String, String> {
        let state = Path::new(state_dir);
        let state_file = state.join("handoff-state.json");
        let r: Value = std::fs::read_to_string(&state_file).ok().and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok()).ok_or_else(|| format!("No armed test found in {}. Run -Arm first.", state.display()))?;
        let is_auto = auto || truthy(&r["Auto"]);
        // the stick may be back under another letter: by volume id
        let mut stick_root = s(&r["PayloadRoot"]);
        let mut present = true;
        let id = s(&r["StickUniqueId"]);
        if !id.is_empty() {
            match judge::find_stick_root(&plive::volumes(), &id) {
                Some(now) => stick_root = now,
                None => present = false,
            }
        }
        if !present || stick_root.is_empty() {
            say(&format!("  ! the stick is not present (armed as {}); markers unreadable, result will be 'error'", s(&r["PayloadRoot"])));
            stick_root = s(&r["PayloadRoot"]);
        }
        say("");
        say(&format!("  upgrade_  V0 handoff test  -  CHECK  ({version_line})"));
        say(&format!("  {} {}   firmware {}", s(&r["Vendor"]), s(&r["Model"]), s(&r["Firmware"])));
        let fail_mode = s(&r["FailMode"]);
        if !fail_mode.is_empty() {
            say(&format!("  FAIL MODE: {fail_mode}"));
        }
        say("");
        let marker = Path::new(&stick_root).join(FIRED_MARKER);
        let grub_env = Path::new(&stick_root).join(plive::GRUB_ENV_REL);
        let mut fired_via: Vec<String> = vec![];
        if marker.exists() {
            fired_via.push(FIRED_MARKER.into());
        }
        if grub_env.exists() && judge::grub_env_fired(&std::fs::read(&grub_env).unwrap_or_default()) {
            fired_via.push("grubenv".into());
        }
        let fired = !fired_via.is_empty();
        let after = plive::fwbootmgr_snapshot(rec);
        let sequence_cleared = s(&after["BootSequence"]).trim().is_empty();
        // our own entry is still listed until the cleanup below: filter it out, so
        // 'reordered' means the firmware moved the REAL entries
        let guid = s(&r["Guid"]);
        let after_text = s(&after["DisplayOrder"]);
        let after_tokens: Vec<&str> = after_text.split_whitespace().filter(|t| *t != guid).collect();
        let before_text = s(&r["Before"]["DisplayOrder"]);
        let before_tokens: Vec<&str> = before_text.split_whitespace().collect();
        let order_unchanged = after_tokens.join(" ") == before_tokens.join(" ");
        let result = handoff_result(fired, sequence_cleared, order_unchanged, &fail_mode);
        say(&format!("  marker present:      {fired}{}", if fired { format!(" ({})", fired_via.join(", ")) } else { String::new() }));
        say(&format!("  bootsequence clear:  {sequence_cleared}"));
        say(&format!("  boot order intact:   {order_unchanged}"));
        say("");
        say(&format!("  RESULT: {result}"));
        if let Some(remark) = result_remark(result) {
            say(&format!("  {remark}"));
        }
        say("");
        // restore, always, whatever happened
        if unregister_return_task(rec) {
            say("  removed the return-check logon task");
        }
        say("  removing test boot entry...");
        if !guid.is_empty() {
            let _ = rec.run("bcdedit", &["/delete", &guid]);
        }
        if !sequence_cleared {
            let _ = rec.run("bcdedit", &["/deletevalue", "{fwbootmgr}", "bootsequence"]);
        }
        let backup = s(&r["BcdBackup"]);
        if restore_bcd && !backup.is_empty() && Path::new(&backup).exists() {
            say("  re-importing BCD backup...");
            let _ = rec.run("bcdedit", &["/import", &backup]);
        }
        if marker.exists() {
            let _ = std::fs::remove_file(&marker);
        }
        plive::reset_grub_env(&stick_root);
        // the human-supplied fields: in -Auto, 'back in Windows' is true by construction and
        // the one human fact is asked in a popup that times out to 'unknown'
        say("");
        let (keypress, win_back, notes) = if is_auto {
            let ans = plive::show_popup(
                &format!("Result: {result}\n\nDuring the restart, did this computer come back to Windows WITHOUT anyone pressing a key?\n\nYes = no key was needed.   No = a key or a menu was needed.\n(This closes by itself in 5 minutes and records 'unknown'.)"),
                &format!("upgrade_ V0 handoff test - result: {result}"),
                300,
                4 + 32,
            );
            (keypress_answer(ans).to_string(), "y".to_string(), String::new())
        } else {
            let k = read_line("  Did the machine reach the payload/Windows with NO keypress? (y/n/na) ");
            let w = read_line("  Are you back in Windows normally right now? (y/n) ");
            let n = read_line("  Notes (recovery prompt? vendor logo hang? blank = none) ");
            (k, w, n)
        };
        let notes = notes_with(&harness_note(&s(&r["OsCaption"]), &s(&r["OsBuild"]), &s(&r["BitLockerSource"]), &fired_via, is_auto, &s(&r["Payload"])), &notes);
        // the evidence row: -Auto without an explicit file writes to the stick itself
        let mut csv_arg = results_csv.map(String::from);
        if csv_arg.is_none() && !s(&r["ResultsCsvArg"]).is_empty() {
            csv_arg = Some(s(&r["ResultsCsvArg"]));
        }
        if is_auto && csv_arg.is_none() {
            csv_arg = Some(if Path::new(&stick_root).exists() { Path::new(&stick_root).join("v0-handoff.csv") } else { state.join("v0-handoff.csv") }.to_string_lossy().to_string());
        }
        let csv = resolve_results_csv(csv_arg.as_deref(), state);
        if let Some(d) = Path::new(&csv).parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if !Path::new(&csv).exists() {
            std::fs::write(&csv, csv_header_bytes()).map_err(|e| format!("could not create {csv}: {e}"))?;
        }
        let payload_leaf = s(&r["PayloadEfi"]).rsplit('\\').next().unwrap_or("").to_string();
        let row = csv_row(&[&plive::now_o(), &s(&r["HarnessVersion"]), &s(&r["Vendor"]), &s(&r["Model"]), &s(&r["Firmware"]), &s(&r["SecureBoot"]), &s(&r["BitLocker"]), &payload_leaf, &fail_mode, result, &keypress, &win_back, &notes]);
        {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new().append(true).open(&csv).map_err(|e| format!("could not open {csv}: {e}"))?;
            f.write_all(format!("{row}\r\n").as_bytes()).map_err(|e| format!("could not write {csv}: {e}"))?;
        }
        let _ = std::fs::remove_file(&state_file);
        say("");
        say(&format!("  logged to {csv}"));
        say("");
        if is_auto {
            plive::show_popup(&format!("Result: {result}\n\nThe row was saved to:\n{csv}\n\nThe test boot entry has been removed; this computer is back to normal.\nYou can unplug the USB stick now and send it back."), "upgrade_ V0 handoff test - done", 120, 64);
        }
        Ok(result.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_header_has_no_byte_order_mark_and_ends_in_crlf() {
        let b = csv_header_bytes();
        assert_ne!(&b[..3], &[0xEF, 0xBB, 0xBF]);
        assert!(b.ends_with(b"\r\n"));
        assert_eq!(String::from_utf8(b).unwrap().trim_end(), CSV_HEADER);
    }

    #[test]
    fn a_row_quotes_every_field_and_doubles_quotes() {
        assert_eq!(csv_row(&["a", "", "say \"hi\""]), r#""a","","say ""hi""""#);
    }

    #[test]
    fn the_notes_lead_with_the_harness() {
        let h = harness_note("Microsoft Windows 10 Pro", "19045", "cmdlet", &["grubenv".into()], true, "shim");
        assert_eq!(h, "[harness: os=Microsoft Windows 10 Pro 19045; bitlocker-via=cmdlet; fired-via=grubenv; mode=auto; payload=shim]");
        assert_eq!(notes_with(&h, "  "), h);
        assert_eq!(notes_with(&h, "logo hang"), format!("{h} logo hang"));
        assert!(harness_note("w", "1", "none", &[], false, "shell").contains("fired-via=none; mode=manual"));
    }

    #[test]
    fn the_return_task_names_the_person_and_the_state() {
        let x = return_task_xml(r"PC\Ann & Bo", r"C:\ProgramData\upgrade_\v0\upgrade-prologue.exe", r"C:\ProgramData\upgrade_\v0", None);
        assert!(x.contains(r"<UserId>PC\Ann &amp; Bo</UserId>"));
        assert!(x.contains(r#"<Arguments>verify-check --auto --state-dir &quot;C:\ProgramData\upgrade_\v0&quot;</Arguments>"#));
        assert!(x.contains("<ExecutionTimeLimit>PT1H</ExecutionTimeLimit>"));
        assert!(return_task_xml("u", "e", "s", Some("E:\\v0-handoff.csv")).contains("--results-csv &quot;E:\\v0-handoff.csv&quot;"));
    }

    #[test]
    fn fail_modes_compare_without_case_as_powershell_does() {
        assert_eq!(payload_path("SHIM", "nofile").unwrap(), "\\EFI\\BOOT\\DOES-NOT-EXIST.EFI");
        assert_eq!(handoff_result(true, true, true, "securebootunsigned"), "persisted");
        assert_eq!(keypress_answer(-1), "unknown");
    }
}
