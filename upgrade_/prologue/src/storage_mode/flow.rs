//! The harness's phases, following Test-StorageMode.ps1 0.1.0: start (leg
//! 1, the arm, the restart into the firmware setup), resume (from the
//! SYSTEM startup task after each restart: the Safe Mode boot, or the
//! next leg), notify (RunOnce at sign-in), safe-mode-sign-in (the
//! `*`-prefixed RunOnce at the Safe Mode sign-in), abort, and the stop
//! that undoes whatever was armed. Every read and write goes through
//! [`Live`], so `tests/storage_mode.rs` drives these phases with canned
//! answers and holds every act to what the script did.

use super::*;
use crate::val::{at, int, items, s, truthy};
use serde_json::{json, Value};
use upgrade_scan::ps::{re, Stamp};

/// What one leg's scan left: the JSON report (its file name and content)
/// and the machine capture, or nothing where the scanner wrote none.
pub struct LegFiles {
    pub report: Option<(String, Value)>,
    pub capture: Option<Value>,
}

/// Everything the harness reads from or does to the machine.
pub trait Live {
    fn elevated(&self) -> bool;
    /// `(Get-Date).ToUniversalTime().ToString('o')`.
    fn now_o(&self) -> String;
    /// `HKLM\SYSTEM\CurrentControlSet\Control\SafeBoot\Option` `OptionValue`, when the key exists.
    fn safe_mode(&self) -> Option<i64>;
    /// Get-LiveContext: `{Utc, RunAs, Interactive, SessionId, ExplorerRunning, Unattended, UptimeSeconds}`.
    fn context(&self) -> Value;
    /// Get-Facts: the machine and the stick, or why they could not be read.
    fn facts(&mut self, root: &str) -> Result<Value, String>;
    fn find_stick(&mut self, st: &Value) -> Option<String>;
    fn wait_stick(&mut self, st: &Value) -> Option<String>;
    /// Make `dir` (emptied), scan into it (the report, its JSON and `scan.log`) and write `machine-capture.json`.
    fn scan_leg(&mut self, dir: &str) -> LegFiles;
    /// Invoke-Native: a tool's exit code and merged output.
    fn native(&mut self, program: &str, args: &[String]) -> (i64, String);
    /// Show-Popup: the button pressed (1 OK, 2 Cancel, 6 Yes, 7 No), -1 on timeout.
    fn popup(&mut self, title: &str, text: &str, seconds: i64, buttons: i64) -> i64;
    fn reg_set(&mut self, name: &str, value: &str) -> Result<(), String>;
    fn reg_remove(&mut self, name: &str);
    fn reg_present(&self, name: &str) -> bool;
    fn protect_state_dir(&mut self, state: &str) -> Result<(), String>;
    /// This program into the state directory (the script copied itself and the scanner).
    fn copy_program(&mut self, state: &str) -> Result<(), String>;
    fn register_task(&mut self, state: &str) -> Result<(), String>;
    /// True when a task was there and went.
    fn unregister_task(&mut self) -> bool;
    /// Write-Log's screen half.
    fn log(&mut self, line: &str);
    /// Write-Host.
    fn host(&mut self, line: &str);
    /// The Safe Mode sign-in marker, beside the state directory.
    fn marker_path(&self) -> String;
    fn exists(&self, path: &str) -> bool;
    fn mkdir(&mut self, path: &str);
    fn remove(&mut self, path: &str);
    fn move_file(&mut self, from: &str, to: &str);
    fn append_line(&mut self, path: &str, line: &str);
    /// Save-Json.
    fn save_json(&mut self, path: &str, doc: &Value);
    fn read_json(&self, path: &str) -> Option<Value>;
    fn read_lines(&self, path: &str) -> Vec<String>;
}

pub struct Args {
    pub phase: String,
    pub stick_drive: String,
    pub bench: bool,
    pub no_prompt: bool,
}

/// One run of a phase: the seam, the state directory, the two logs.
pub struct Flow<'a> {
    pub live: &'a mut dyn Live,
    pub state_dir: String,
    log_file: Option<String>,
    stick_log: Option<String>,
}

fn join(dir: &str, name: &str) -> String {
    format!("{}\\{name}", dir.trim_end_matches('\\'))
}

impl<'a> Flow<'a> {
    pub fn new(live: &'a mut dyn Live, state_dir: &str) -> Flow<'a> {
        Flow { live, state_dir: state_dir.to_string(), log_file: None, stick_log: None }
    }

    /// Write-Log: the screen and both logs.
    fn log(&mut self, line: &str) {
        self.live.log(line);
        let stamp = format!("{}Z", &self.live.now_o()[..19]);
        for p in [self.log_file.clone(), self.stick_log.clone()].into_iter().flatten() {
            self.live.append_line(&p, &format!("{stamp} {line}"));
        }
    }

    fn state_path(&self, name: &str) -> String {
        join(&self.state_dir, name)
    }

    /// Save-State.
    fn save_state(&mut self, st: &mut Value) {
        st["UpdatedUtc"] = json!(self.live.now_o());
        let p = self.state_path("state.json");
        self.live.save_json(&p, st);
    }

    /// Write-Record: the stick (when known) and the state directory.
    fn write_record(&mut self, st: &Value, root: &str) {
        let rec = record(st);
        if !root.is_empty() {
            let p = join(&stick_dir(root), "storage-mode.json");
            self.live.save_json(&p, &rec);
        }
        let p = self.state_path("storage-mode.json");
        self.live.save_json(&p, &rec);
    }

    fn move_state(&mut self, to: &str) {
        let (a, b) = (self.state_path("state.json"), self.state_path(to));
        self.live.move_file(&a, &b);
    }

    /// Set-Notice: queue text for the person's next sign-in.
    fn set_notice(&mut self, title: &str, text: &str, buttons: i64) -> &'static str {
        let n = json!({"title": title, "text": text, "buttons": buttons, "queued_utc": self.live.now_o()});
        let p = self.state_path("notice.json");
        self.live.save_json(&p, &n);
        let cmd = notice_command(&program_in(&self.state_dir), &self.state_dir);
        match self.live.reg_set(NOTICE_RUNONCE_NAME, &cmd) {
            Ok(()) => "queued",
            Err(e) => {
                self.log(&format!("  ! could not queue the sign-in notice: {e}"));
                "not-queued"
            }
        }
    }

    /// Show-Or-Queue.
    fn show_or_queue(&mut self, title: &str, text: &str, buttons: i64) -> &'static str {
        if truthy(&self.live.context()["Unattended"]) {
            return self.set_notice(title, text, buttons);
        }
        self.live.popup(title, text, 600, buttons);
        "shown"
    }

    fn bcd(&mut self, args: &[&str]) -> (i64, String) {
        let a: Vec<String> = args.iter().map(|x| x.to_string()).collect();
        self.live.native("bcdedit", &a)
    }

    fn shutdown(&mut self, args: &[&str]) -> (i64, String) {
        let a: Vec<String> = args.iter().map(|x| x.to_string()).collect();
        self.live.native("shutdown", &a)
    }

    /// Set-SafeBootOnce: the boot manager boots the entry exactly once.
    fn set_safeboot_once(&mut self, guid: &str) -> Result<(), String> {
        let (exit, text) = self.bcd(&["/bootsequence", guid]);
        if exit != 0 {
            return Err(format!("bcdedit /bootsequence failed: {text}"));
        }
        let (_, chk) = self.bcd(&["/enum", "{bootmgr}"]);
        if !chk.to_lowercase().contains(&guid.to_lowercase()) {
            return Err(format!("bootsequence not visible on {{bootmgr}} after setting it: {chk}"));
        }
        Ok(())
    }

    /// Arm-SafeEntry: copy the running Windows entry, make the copy Safe
    /// Mode, take it out of the menu, boot it once.
    fn arm_safe_entry(&mut self) -> Result<Value, String> {
        let (exit, text) = self.bcd(&["/copy", "{current}", "/d", BCD_DESCRIPTION]);
        if exit != 0 {
            return Err(format!("bcdedit /copy failed: {text}"));
        }
        let guid = bcd_copy_guid(&text).ok_or_else(|| format!("bcdedit /copy gave no entry id: {text}"))?;
        let (exit, text) = self.bcd(&["/set", &guid, "safeboot", "minimal"]);
        if exit != 0 {
            self.bcd(&["/delete", &guid]);
            return Err(format!("bcdedit /set safeboot failed: {text}"));
        }
        let (_, en) = self.bcd(&["/enum", &guid]);
        if re(r"safeboot\s+Minimal", "i").find(&en).is_none() {
            self.bcd(&["/delete", &guid]);
            return Err(format!("the copied entry does not show safeboot Minimal: {en}"));
        }
        let (exit, text) = self.bcd(&["/displayorder", &guid, "/remove"]);
        if exit != 0 {
            self.bcd(&["/delete", &guid]);
            return Err(format!("bcdedit /displayorder /remove failed: {text}"));
        }
        self.set_safeboot_once(&guid)?;
        Ok(json!({"Guid": guid, "Enum": en}))
    }

    /// Invoke-Cleanup: every exit path undoes the copy, the one-time
    /// sequence, the RunOnce, the marker and the task, and says what it did.
    fn cleanup(&mut self, st: &Value) -> Value {
        let mut c = serde_json::Map::new();
        let guid = s(at(st, "SafeEntry.Guid"));
        if truthy(at(st, "SafeEntry")) && !guid.is_empty() {
            let (_, bs) = self.bcd(&["/enum", "{bootmgr}"]);
            if bs.to_lowercase().contains(&guid.to_lowercase()) {
                c.insert("BootSequenceRemoved".into(), json!(self.bcd(&["/deletevalue", "{bootmgr}", "bootsequence"]).0 == 0));
            } else {
                c.insert("BootSequenceRemoved".into(), json!("not-set"));
            }
            c.insert("SafeEntryDeleted".into(), json!(self.bcd(&["/delete", &guid]).0 == 0));
        }
        self.live.reg_remove(SAFE_RUNONCE_NAME);
        c.insert("SafeRunOnceRemoved".into(), json!(!self.live.reg_present(SAFE_RUNONCE_NAME)));
        let marker = self.live.marker_path();
        self.live.remove(&marker);
        c.insert("TaskRemoved".into(), json!(self.live.unregister_task()));
        Value::Object(c)
    }

    /// Restart-IntoFirmware: `shutdown /fw` boots straight into the setup;
    /// firmware without it refuses, and a plain restart with the setup key
    /// is the fallback. On the bench, a plain restart only (Hyper-V's
    /// firmware honours the indication but has no setup to show).
    fn restart_into_firmware(&mut self, why: &str, skip_firmware: bool) -> Result<&'static str, String> {
        if skip_firmware {
            let (exit, text) = self.shutdown(&["/r", "/t", "20", "/c", &format!("upgrade_ storage-mode test (bench): {why}")]);
            if exit != 0 {
                return Err(format!("shutdown /r failed ({exit}: {text})"));
            }
            return Ok("plain-bench");
        }
        let (exit, text) = self.shutdown(&["/r", "/fw", "/t", "20", "/c", &format!("upgrade_ storage-mode test: {why}")]);
        if exit == 0 {
            return Ok("fw");
        }
        self.log(&format!("  shutdown /fw refused ({exit}: {text}); plain restart instead - press the setup key at power-on"));
        let (exit, text) = self.shutdown(&["/r", "/t", "20", "/c", &format!("upgrade_ storage-mode test: {why}. Press the setup key (F2 on Acer) as soon as the screen goes dark.")]);
        if exit != 0 {
            return Err(format!("shutdown /r failed too ({exit}: {text})"));
        }
        Ok("plain")
    }

    /// Invoke-Leg: scan and capture into `legN` on the stick, read both back.
    fn invoke_leg(&mut self, n: i64, root: &str, asked: &str) -> Result<Value, String> {
        let dir = leg_dir(root, n);
        self.log(&format!("  leg {n}: scanning (upgrade-scan {} (Rust))...", upgrade_scan::FOLLOWS_SCANNER));
        let files = self.live.scan_leg(&dir);
        let (report_file, report) = files.report.ok_or_else(|| format!("leg {n}: the scanner left no JSON report in {dir} (see scan.log)"))?;
        let capture = files.capture.ok_or_else(|| format!("leg {n}: no machine capture was written"))?;
        let ctx = self.live.context();
        let leg = leg_record(n, &self.live.now_o(), asked, &report, &report_file, &capture, &ctx);
        self.log(&leg_line(&leg));
        Ok(leg)
    }

    /// Finish: clean up, tell, record, retire the state.
    fn finish(&mut self, st: &mut Value, root: &str, result: &str, text: &str) {
        st["Cleanup"] = self.cleanup(st);
        st["Stage"] = json!(format!("done:{result}"));
        st["Notice"] = json!(self.show_or_queue(TITLE, &finish_text(result, text), 64));
        self.save_state(st);
        self.write_record(st, root);
        self.log(&format!("  finished: {result}  cleanup={}", cleanup_words(&st["Cleanup"])));
        self.move_state("state-done.json");
    }

    /// Arm-Next: (re)arm the Safe Mode boot and restart into the firmware setup.
    fn arm_next(&mut self, st: &mut Value, root: &str, ask: &str, why: &str) -> Result<(), String> {
        if truthy(at(st, "SafeEntry")) && !s(at(st, "SafeEntry.Guid")).is_empty() {
            let guid = s(at(st, "SafeEntry.Guid"));
            self.set_safeboot_once(&guid)?;
        } else {
            st["SafeEntry"] = self.arm_safe_entry()?;
        }
        let cmd = safe_runonce_command(&program_in(&self.state_dir), &self.state_dir);
        self.live.reg_set(SAFE_RUNONCE_NAME, &cmd)?;
        st["Ask"] = json!(ask);
        st["Stage"] = json!(format!("armed-{}", items(at(st, "Legs")).len()));
        self.save_state(st);
        self.write_record(st, root);
        let how = self.restart_into_firmware(why, truthy(at(st, "Bench")))?;
        let mut fw = items(at(st, "FwReboots")).into_iter().cloned().collect::<Vec<_>>();
        fw.push(json!({"Utc": self.live.now_o(), "Ask": ask, "Method": how}));
        st["FwReboots"] = Value::Array(fw);
        self.save_state(st);
        self.write_record(st, root);
        let guid = s(at(st, "SafeEntry.Guid"));
        self.log(&format!("  armed: Safe Mode once via {guid}; asking for {}; restarting ({how}) in 20 s", mode_label(ask)));
        Ok(())
    }

    /// Invoke-StartPhase.
    pub fn start(&mut self, a: &Args) -> Result<(), String> {
        if !self.live.elevated() {
            return Err("run this elevated (the launcher asks for it)".into());
        }
        let state = self.state_dir.clone();
        self.live.mkdir(&state);
        if self.live.exists(&self.state_path("state.json")) {
            return Err(format!("a storage-mode test is already in progress (state in {state}). Restart to let it resume, or run -Abort."));
        }
        let root = drive_root(&a.stick_drive);
        if !self.live.exists(&root) {
            return Err(format!("stick {root} not found"));
        }
        let sdir = stick_dir(&root);
        self.live.mkdir(&sdir);
        let lf = self.state_path("storage-mode.log");
        self.live.remove(&lf);
        self.log_file = Some(lf);
        let sl = join(&sdir, "storage-mode.log");
        self.live.remove(&sl);
        self.stick_log = Some(sl);
        self.live.remove(&join(&sdir, "storage-mode.json"));
        self.log("");
        self.log(&format!("  upgrade_  storage-mode harness {FOLLOWS_HARNESS}  -  the Storage controller mode check, both SATA modes (V5 / R1)"));
        let f = self.live.facts(&root)?;
        self.log(&facts_line(&f, &root));
        if eq_ci(&s(at(&f, "BitLocker")), "on") {
            return Err(BITLOCKER_REFUSAL.into());
        }
        let mut st = new_state(&self.live.now_o(), &f, &root, a.bench);
        let leg1 = self.invoke_leg(1, &root, "initial")?;
        st["Legs"] = json!([leg1.clone()]);
        let mode = s(at(&leg1, "Mode"));
        if eq_ci(&mode, "none") && !a.bench {
            st["Stage"] = json!("stopped:no-intel-controller");
            self.save_state(&mut st);
            self.write_record(&st, &root);
            self.move_state("state-stopped.json");
            self.log(NO_INTEL_LOG);
            if !a.no_prompt {
                self.live.popup(TITLE, NO_INTEL_POPUP, 600, 48);
            }
            return Ok(());
        }
        // bench: a VM has no mode to flip; exercise the mechanics anyway
        let ask = next_mode(&mode).unwrap_or("raid");
        let back = mode_back(&mode);
        let text = explanation_text(&mode, ask, &back);
        if !a.no_prompt {
            let r = self.live.popup("upgrade_ - storage-mode test: what happens next", &text, 300, 1 + 64);
            if r == 2 {
                st["Stage"] = json!("stopped:cancelled");
                self.save_state(&mut st);
                self.write_record(&st, &root);
                self.move_state("state-stopped.json");
                self.log("  cancelled at the explanation - nothing was changed");
                return Ok(());
            }
        }
        self.live.copy_program(&state)?;
        self.live.protect_state_dir(&state)?;
        self.live.register_task(&state)?;
        self.save_state(&mut st);
        self.arm_next(&mut st, &root, ask, &format!("set SATA Mode to {} on the setup screen, then save", mode_label(ask)))?;
        self.log(&format!("  On the setup screen: SATA Mode -> {} -> F10 -> Yes. At the Safe Mode screen: hold the power button, start again (no password). Leave the stick in.", mode_label(ask)));
        Ok(())
    }

    /// Invoke-ResumePhase.
    pub fn resume(&mut self) -> Result<(), String> {
        let state = self.state_dir.clone();
        let Some(mut st) = self.live.read_json(&self.state_path("state.json")) else {
            self.live.host(&format!("  nothing to resume (no state in {state})"));
            self.live.unregister_task();
            return Ok(());
        };
        self.log_file = Some(self.state_path("storage-mode.log"));
        let mut ctx = self.live.context();
        let safe = self.live.safe_mode();
        if is_safe_mode(safe) {
            // Safe Mode: the driver for the re-enumerated controller is bound by now; record, restart
            let option = safe.unwrap_or(0);
            let mut boots = items(at(&st, "SafeBoots")).into_iter().cloned().collect::<Vec<_>>();
            boots.push(json!({"Utc": ctx["Utc"], "RunAs": ctx["RunAs"], "SessionId": ctx["SessionId"], "OptionValue": option, "UptimeSeconds": ctx["UptimeSeconds"], "Stage": s(at(&st, "Stage"))}));
            st["SafeBoots"] = Value::Array(boots);
            self.save_state(&mut st);
            self.log(&format!("  Safe Mode boot (option {option}) as {}, session {}, {} s after boot - restarting in 10 s", s(&ctx["RunAs"]), s(&ctx["SessionId"]), s(&ctx["UptimeSeconds"])));
            let (exit, text) = self.shutdown(&["/r", "/t", "10", "/c", "upgrade_ storage-mode test: Safe Mode boot done, restarting"]);
            self.log(&format!("  shutdown from Safe Mode: exit {exit} {text}"));
            return Ok(());
        }
        let marker = self.live.marker_path();
        if self.live.exists(&marker) {
            let mut boots = items(at(&st, "SafeBoots")).into_iter().cloned().collect::<Vec<_>>();
            boots.extend(safe_marker_lines(&self.live.read_lines(&marker)));
            st["SafeBoots"] = Value::Array(boots);
            self.live.remove(&marker);
        }
        let root = self.live.wait_stick(&st);
        let started = Stamp::parse(s(&ctx["Utc"]).get(..19).unwrap_or("")).map_or(0, |t| t.seconds());
        let now = Stamp::parse(&self.live.now_o()[..19]).map_or(0, |t| t.seconds());
        ctx["StickWaitSeconds"] = json!(now - started);
        ctx["Stage"] = json!(s(at(&st, "Stage")));
        let mut resumes = items(at(&st, "Resumes")).into_iter().cloned().collect::<Vec<_>>();
        resumes.push(ctx.clone());
        st["Resumes"] = Value::Array(resumes);
        self.save_state(&mut st);
        if let Some(r) = &root {
            self.stick_log = Some(join(&stick_dir(r), "storage-mode.log"));
        }
        self.log("");
        self.log(&format!("  upgrade_  storage-mode harness {FOLLOWS_HARNESS}  -  RESUME (stage {})", s(at(&st, "Stage"))));
        self.log(&format!("  running as {}, session {}, uptime {} s, stick after {} s -> {}", s(&ctx["RunAs"]), s(&ctx["SessionId"]), s(&ctx["UptimeSeconds"]), s(&ctx["StickWaitSeconds"]), if truthy(&ctx["Unattended"]) { "unattended" } else { "attended" }));
        let Some(root) = root else {
            self.log(&format!("  ! the USB stick is not present after {STICK_WAIT_SECONDS} s; leaving the task in place"));
            self.show_or_queue(TITLE, NO_STICK_TEXT, 48);
            return Ok(());
        };
        let stage = s(at(&st, "Stage"));
        if !stage.to_lowercase().starts_with("armed-") {
            self.log(&format!("  state is at stage '{stage}', nothing to continue; cleaning up"));
            self.finish(&mut st, &root, "unexpected-stage", &format!("The test was at stage '{stage}' when the resume ran."));
            return Ok(());
        }
        let n = items(at(&st, "Legs")).len() as i64 + 1;
        let ask = s(at(&st, "Ask"));
        let leg = self.invoke_leg(n, &root, &ask)?;
        let mut legs = items(at(&st, "Legs")).into_iter().cloned().collect::<Vec<_>>();
        legs.push(leg.clone());
        st["Legs"] = Value::Array(legs.clone());
        let first = legs[0].clone();
        let outcome = leg_outcome(n, &s(at(&first, "Mode")), &s(at(&leg, "Mode")));
        st["LastOutcome"] = json!(outcome);
        self.save_state(&mut st);
        self.write_record(&st, &root);
        self.log(&format!("  leg {n}: asked {}, saw {} -> {outcome}", mode_label(&ask), s(at(&leg, "Mode"))));
        let leg_text = |label: i64, l: &Value| format!("leg {label} ({}): [{}] {}", s(at(l, "Mode")), s(at(l, "Check.Status")), s(at(l, "Check.Detail")));
        match outcome {
            "changed" => {
                let back = mode_back(&s(at(&first, "Mode")));
                self.arm_next(&mut st, &root, &back, &format!("set SATA Mode back to {} on the setup screen, then save", mode_label(&back)))?;
            }
            "mode-unchanged" => {
                let t = format!("The SATA mode did not change between the two scans (both {}). Either the setup screen has no SATA Mode option on this computer, or it was not saved. The scanner's line was: [{}] {}", s(at(&leg, "Mode")), s(at(&leg, "Check.Status")), s(at(&leg, "Check.Detail")));
                self.finish(&mut st, &root, "mode-unchanged", &t);
            }
            "restored" => {
                let t = format!("Both modes were scanned and the original mode ({}) is back.\n  {}\n  {}\n  {}", s(at(&first, "Mode")), leg_text(1, &first), leg_text(2, &legs[1]), leg_text(3, &leg));
                self.finish(&mut st, &root, "restored", &t);
            }
            "not-restored" => {
                let t = format!("Both modes were scanned, but the controller is in {} mode now, not the original {}. Windows boots in this mode; set it back on the setup screen when convenient.\n  {}", s(at(&leg, "Mode")), s(at(&first, "Mode")), leg_text(2, &legs[1]));
                self.finish(&mut st, &root, "not-restored", &t);
            }
            other => self.finish(&mut st, &root, other, "Unexpected leg."),
        }
        Ok(())
    }

    /// Invoke-NotifyPhase: at sign-in, as the person.
    pub fn notify(&mut self) {
        let p = self.state_path("notice.json");
        let Some(n) = self.live.read_json(&p) else { return };
        self.live.popup(&s(&n["title"]), &s(&n["text"]), 600, int(&n["buttons"]));
        self.live.remove(&p);
    }

    /// Invoke-SafeModeSignInPhase: leave the Safe Mode boot's evidence and
    /// restart; outside Safe Mode it does nothing, so a leftover RunOnce
    /// can never restart someone's normal boot.
    pub fn safe_mode_sign_in(&mut self) {
        let Some(option) = self.live.safe_mode() else { return };
        let ctx = self.live.context();
        let line = safe_marker_line(&self.live.now_o(), &s(&ctx["RunAs"]), option, int(&ctx["UptimeSeconds"]));
        let marker = self.live.marker_path();
        self.live.append_line(&marker, &line);
        self.shutdown(&["/r", "/t", "5", "/c", "upgrade_ storage-mode test: Safe Mode boot done, restarting"]);
    }

    /// Invoke-AbortPhase: undo everything this harness armed, by hand.
    pub fn abort(&mut self) -> Result<(), String> {
        if !self.live.elevated() {
            return Err("run this elevated".into());
        }
        let Some(mut st) = self.live.read_json(&self.state_path("state.json")) else {
            self.live.unregister_task();
            self.live.reg_remove(SAFE_RUNONCE_NAME);
            self.live.host("  nothing in progress; task and RunOnce removed if present");
            return Ok(());
        };
        st["Cleanup"] = self.cleanup(&st);
        self.live.reg_remove(NOTICE_RUNONCE_NAME);
        st["Stage"] = json!("stopped:aborted");
        self.save_state(&mut st);
        self.move_state("state-aborted.json");
        self.live.host(&format!("  aborted; cleanup: {}", cleanup_words(&st["Cleanup"])));
        Ok(())
    }

    /// The main block: a phase, and the stop that undoes what was armed
    /// when start or resume fails. Returns the exit code.
    pub fn run(&mut self, a: &Args) -> i32 {
        let r = match a.phase.as_str() {
            "start" => self.start(a),
            "resume" => self.resume(),
            "notify" => {
                self.notify();
                Ok(())
            }
            "safe-mode-sign-in" => {
                self.safe_mode_sign_in();
                Ok(())
            }
            "abort" => self.abort(),
            other => Err(format!("no such phase: {other}")),
        };
        let Err(msg) = r else { return 0 };
        self.live.host("");
        self.live.host(&format!("  storage-mode harness stopped: {msg}"));
        self.log(&format!("  ! stopped: {msg}"));
        if a.phase == "start" || a.phase == "resume" {
            // a failure must not leave the machine armed: undo whatever was armed, tell, leave
            let state = self.state_dir.clone();
            match self.live.read_json(&self.state_path("state.json")) {
                Some(mut st) => {
                    st["Cleanup"] = self.cleanup(&st);
                    st["Stage"] = json!("stopped:error");
                    st["Error"] = json!(msg);
                    self.save_state(&mut st);
                    let r = self.live.find_stick(&st);
                    self.write_record(&st, r.as_deref().unwrap_or(""));
                    let txt = error_text(&msg, &state, r.is_some());
                    if a.phase == "resume" {
                        self.set_notice(TITLE, &txt, 48);
                    } else {
                        self.live.host(&format!("  cleanup: {}", cleanup_words(&st["Cleanup"])));
                    }
                    self.move_state("state-stopped.json");
                }
                None => {
                    self.live.unregister_task();
                }
            }
        }
        1
    }
}
