//! UPGRADE.exe: the window in front of the kit's programs (decided 2026-09-27,
//! the owner: a Rust window that calls the kit from the stick). It decides
//! nothing the programs decide. It runs the kit's Rust programs
//! (`upgrade-scan`, `upgrade-job`, `upgrade-prologue`; the cut-over, RISKS
//! R32) with the launchers' steps, shows their progress in plain words,
//! stops where they stop, and adds a few things of its own: a stop on a RED
//! scan before the job writer runs (more cautious, never less), the typed
//! word and sentences compared byte for byte, and for the verify flow a
//! one-shot sign-in task that opens it again after the restart to show what
//! came back. Every line a program prints goes to upgrade_\convert.log on
//! the stick, as Invoke-Logged.ps1 did for the scripts.
//!
//! The launchers as flows (2026-10-08): verify (RUN-VERIFY.cmd: nothing is
//! installed and nothing on the internal drive is changed), convert
//! (RUN-CONVERT.cmd: keep Windows, install Linux beside it), erase
//! (RUN-ERASE-AND-INSTALL.cmd; since 2026-10-17 Convert's clean-slate answer,
//! not a button of its own), roll back (a pointer to Linux since 2026-10-17), the walk-away
//! probe (RUN-PROBE.cmd). The cancel button (CANCEL-CONVERSION.cmd) left on
//! 2026-11-02 (the owner: the last click is the last turn back). The data-loss
//! variants of convert and erase are reached only by starting the window
//! with --accepting-data-loss, its own launcher, as rule #1 asks. Verify and
//! the probe show only with --developer (roadmap item 20, 2026-10-17): they
//! make the rows, and a person converting never needs them.
//!
//! The picker (roadmap item 24, decided 2026-11-07 and 2026-11-08, the
//! owner): Convert starts with a read-only scan, then the person chooses
//! the Linux, from the releases the kit's table offers that the scan says
//! this computer can start (RISKS R34). `upgrade-fetch` downloads it onto
//! this stick and refuses any byte that differs; only then comes the set-up
//! screen. All of it is before the last click: nothing on the computer has
//! changed. `--developer --from <http folder>` fetches from a local copy
//! (the rig's server); the table's sha256s still decide.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod flow;
mod win;
mod words;

use eframe::egui;
use flow::Desktop;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const WINDOW_VERSION: &str = env!("CARGO_PKG_VERSION");

// ---------------------------------------------------------------- the record

/// One line in the stick's convert.log, next to the programs' own lines, so
/// every run leaves its trace (rule #5).
fn log_line(root: &Path, text: &str) {
    use std::io::Write;
    let p = root.join("upgrade_").join("convert.log");
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
        let _ = writeln!(f, "{}  window {}: {}\r", flow::now_utc(), WINDOW_VERSION, text);
    }
}

/// A program's own line, kept as it was printed (Invoke-Logged.ps1 did this
/// for the scripts).
fn log_raw(root: &Path, text: &str) {
    use std::io::Write;
    let p = root.join("upgrade_").join("convert.log");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
        let _ = writeln!(f, "{}\r", text);
    }
}

fn state_dir() -> PathBuf {
    let base = std::env::var_os("ProgramData").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("upgrade_").join("window")
}

fn handoff_state() -> PathBuf {
    let base = std::env::var_os("ProgramData").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("upgrade_").join("v0").join("handoff-state.json")
}

fn stick_root(stick: &str) -> PathBuf {
    PathBuf::from(format!("{}\\", stick))
}

// ---------------------------------------------------------------- the runner

enum Stop {
    Kit(Vec<String>),
    /// the download refused: its words; nothing on the computer changed, and the person may try again
    Fetch(Vec<String>),
    /// the scan says this computer can start none of the releases: each with its reason
    NoRelease(Vec<flow::Choice>),
    Red(Option<flow::Verdict>),
    Refused(Vec<String>),
    /// the prologue stopped: its own words, and Windows is as it was
    Prologue(Vec<String>),
}

enum Msg {
    Step(usize),
    Line(String),
    Stop(Stop),
    Arming,
    Armed,
    Result(flow::ResultView),
    /// the convert flow's job is written: the sign-in name, and the decision is the person's
    Decide(String),
    /// the prologue armed the handoff: the computer restarts in 15 s
    ConvertArmed,
    /// a one-call flow (roll back, probe, cancel) finished: its exit code and lines
    Done(i32, Vec<String>),
    /// the picker's scan is done: the buttons
    Choices(Vec<flow::Choice>),
    /// the download is done and checked
    Fetched,
}

/// Shared between the window and the runner, so closing the window and
/// starting the handoff cannot cross: once arming has begun the window
/// cannot close, and once the window has closed nothing more starts.
#[derive(Default)]
struct Guard {
    cancelled: bool,
    arming: bool,
    child: Option<Child>,
}

type Shared = Arc<Mutex<Guard>>;

/// Run one kit program, sending each line as it comes and keeping it in the
/// stick's log. None if it could not start or the window was closed first.
fn run(c: &flow::Call, root: &Path, tx: &Sender<Msg>, guard: &Shared) -> Option<(i32, Vec<String>)> {
    #[cfg(windows)]
    use std::os::windows::process::CommandExt;
    log_raw(root, &format!("==== {}  {}", flow::now_utc(), flow::command_line(c)));
    let mut cmd = Command::new(&c.program);
    cmd.args(&c.args).current_dir(root).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    cmd.creation_flags(win::CREATE_NO_WINDOW);
    let (out, err) = {
        let mut g = guard.lock().unwrap();
        if g.cancelled {
            return None;
        }
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                let line = format!("could not start {}: {}", c.program, e);
                log_raw(root, &line);
                let _ = tx.send(Msg::Line(line.clone()));
                return Some((1, vec![line]));
            }
        };
        let io = (child.stdout.take(), child.stderr.take());
        g.child = Some(child);
        io
    };
    let etx = tx.clone();
    let eroot = root.to_path_buf();
    let err_thread = std::thread::spawn(move || {
        let mut v = vec![];
        if let Some(e) = err {
            for l in BufReader::new(e).split(b'\n').map_while(Result::ok) {
                let s = String::from_utf8_lossy(&l).trim_end_matches('\r').to_string();
                log_raw(&eroot, &s);
                let _ = etx.send(Msg::Line(s.clone()));
                v.push(s);
            }
        }
        v
    });
    let mut lines = vec![];
    if let Some(o) = out {
        for l in BufReader::new(o).split(b'\n').map_while(Result::ok) {
            let s = String::from_utf8_lossy(&l).trim_end_matches('\r').to_string();
            log_raw(root, &s);
            let _ = tx.send(Msg::Line(s.clone()));
            lines.push(s);
        }
    }
    lines.extend(err_thread.join().unwrap_or_default());
    let child = guard.lock().unwrap().child.take();
    let code = child.and_then(|mut c| c.wait().ok()).and_then(|s| s.code()).unwrap_or(1);
    log_raw(root, &format!("==== exit {}", code));
    Some((code, lines))
}

/// The scan's own stop: RED goes no further (the job writer would refuse it
/// too), except on the data-loss path, where the job writer judges the RED
/// with the sentence (flow::scan_stops).
fn scan_stop(root: &Path, acknowledged: bool) -> Option<Stop> {
    let report = flow::newest_report(&root.join("upgrade_").join("reports"));
    let v = report.and_then(|p| std::fs::read(p).ok()).and_then(|b| flow::parse_json(&b)).and_then(|j| flow::verdict(&j));
    if !flow::scan_stops(v.as_ref(), acknowledged) {
        if !flow::verdict_allows(v.as_ref()) {
            log_line(root, "verdict RED handed to the job writer with the acknowledgement (RISKS R23)");
        }
        return None;
    }
    log_line(root, &format!("stopped after the scan: verdict {}", v.as_ref().map(|v| v.level.as_str()).unwrap_or("missing")));
    Some(Stop::Red(v))
}

/// RUN-VERIFY.cmd, as a thread. Every stop before step 4 leaves the
/// computer as it was; step 4 is the verify arm's own (`upgrade-prologue
/// verify-arm`, following Test-Handoff.ps1), which removes its boot entry
/// again if it cannot finish.
fn verify(stick: String, tx: Sender<Msg>, guard: Shared) {
    let root = stick_root(&stick);
    let stop = |s: Stop| {
        let _ = tx.send(Msg::Stop(s));
    };
    let _ = tx.send(Msg::Step(0));
    let missing = flow::missing_kit_files(&root);
    if !missing.is_empty() {
        return stop(Stop::Kit(missing.iter().map(|s| s.to_string()).collect()));
    }
    let computer = std::env::var("COMPUTERNAME").unwrap_or_default();
    log_line(&root, &format!("======== the verify flow on {} (stick {})", computer, stick));
    let _ = std::fs::create_dir_all(root.join("upgrade_").join("reports"));

    for (i, call) in flow::verify_calls(&stick).iter().enumerate() {
        let _ = tx.send(Msg::Step(call.step));
        let Some((code, lines)) = run(call, &root, &tx, &guard) else { return };
        if code != 0 && !call.may_fail {
            log_line(&root, &format!("stopped at step {} (exit {})", call.step, code));
            return stop(Stop::Refused(flow::refusal_lines(&lines)));
        }
        if i == 0 {
            if let Some(s) = scan_stop(&root, false) {
                return stop(s);
            }
        }
    }

    // the kickstart came out of the job writer's call (step 3 of the .cmd is inside step 2 here)
    let _ = tx.send(Msg::Step(3));
    // RUN-VERIFY.cmd's markers: this boot is a verify, and the old report goes
    let u = root.join("upgrade_");
    if let Err(e) = std::fs::write(u.join("boot-verify"), "v1\r\n") {
        return stop(Stop::Refused(vec![format!("could not write upgrade_\\boot-verify on the stick ({})", e)]));
    }
    let _ = std::fs::remove_file(u.join("boot-install"));
    let _ = std::fs::remove_dir_all(u.join("report"));

    // the reopen, before anything is armed: a restart the window cannot follow is not started
    let dir = state_dir();
    let exe = dir.join("UPGRADE.exe");
    let armed_utc = flow::now_utc();
    let prepared = (|| -> Result<(), String> {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let me = std::env::current_exe().map_err(|e| e.to_string())?;
        std::fs::copy(&me, &exe).map_err(|e| format!("could not copy the window to {} ({})", exe.display(), e))?;
        let st = json!({ "schema": "window-state/1", "flow": "verify", "window_version": WINDOW_VERSION, "stick": stick,
                         "stick_volume": win::volume_path(&format!("{}\\", stick)), "armed_utc": armed_utc });
        std::fs::write(dir.join("state.json"), serde_json::to_vec_pretty(&st).unwrap()).map_err(|e| e.to_string())?;
        let user = format!("{}\\{}", std::env::var("USERDOMAIN").unwrap_or_default(), std::env::var("USERNAME").unwrap_or_default());
        win::register_reopen(&flow::reopen_task_xml(&user, &exe.to_string_lossy()), &dir)
    })();
    if let Err(e) = prepared {
        win::unregister_reopen();
        let _ = std::fs::remove_file(dir.join("state.json"));
        log_line(&root, &format!("stopped: the window could not arrange to open again after the restart ({})", e));
        return stop(Stop::Refused(vec![format!("the window could not arrange to open again after the restart ({})", e)]));
    }
    log_line(&root, "the sign-in task that reopens the window is registered");

    {
        let mut g = guard.lock().unwrap();
        if g.cancelled {
            drop(g);
            win::unregister_reopen();
            let _ = std::fs::remove_file(dir.join("state.json"));
            return;
        }
        g.arming = true;
    }
    let _ = tx.send(Msg::Arming);
    let _ = tx.send(Msg::Step(4));
    let arm = flow::arm_call(&stick);
    let res = run(&arm, &root, &tx, &guard);
    guard.lock().unwrap().arming = false;
    match res {
        Some((0, _)) => {
            log_line(&root, "armed; the computer restarts in 20 s");
            let _ = tx.send(Msg::Armed);
        }
        other => {
            win::unregister_reopen();
            let _ = std::fs::remove_file(dir.join("state.json"));
            log_line(&root, "the handoff did not arm; the reopen task is removed again");
            stop(Stop::Refused(other.map(|(_, l)| flow::refusal_lines(&l)).unwrap_or_default()));
        }
    }
}

/// What the person chose before the convert flow started. The password is
/// wiped as soon as its hash is written.
struct ConvertInputs {
    desktop: Desktop,
    /// the release the picker fetched onto the stick (item 24)
    release: String,
    password: String,
    /// the acknowledgement sentence as typed on the data-loss path, else empty
    ack: String,
    /// the erase sentence as typed (RUN-ERASE-AND-INSTALL.cmd), else empty: the convert flow
    erase: String,
}

/// RUN-CONVERT.cmd and RUN-ERASE-AND-INSTALL.cmd (and their data-loss
/// variants), as a thread. Everything up to the person's decision leaves
/// the computer as it was; the prologue's own refusals come before anything
/// it changes, and it says STOPPED when it stops. `word_rx` brings the
/// typed word (convert) or the button press (erase) from the window once
/// the job is written and the sign-in name is on the screen.
fn convert(stick: String, mut inputs: ConvertInputs, tx: Sender<Msg>, guard: Shared, word_rx: Receiver<String>) {
    let root = stick_root(&stick);
    let stop = |s: Stop| {
        let _ = tx.send(Msg::Stop(s));
    };
    let _ = tx.send(Msg::Step(0));
    let missing = flow::missing_kit_files_convert(&root);
    if !missing.is_empty() {
        flow::wipe(&mut inputs.password);
        return stop(Stop::Kit(missing.iter().map(|s| s.to_string()).collect()));
    }
    // the Linux the picker fetched must still be on the stick, whole (item 24)
    let (image, _) = inputs.desktop.job_args();
    let status = flow::call(1, false, &stick, "upgrade-fetch.exe", &["status", "--release", &inputs.release, "--desktop", image]);
    match run(&status, &root, &tx, &guard) {
        Some((0, _)) => {}
        Some((_, lines)) => {
            flow::wipe(&mut inputs.password);
            return stop(Stop::Refused(lines.into_iter().map(|l| l.trim_start_matches("fetch: ").to_string()).collect()));
        }
        None => return,
    }
    let computer = std::env::var("COMPUTERNAME").unwrap_or_default();
    let which = format!("the {} flow{}", if inputs.erase.is_empty() { "convert" } else { "erase" }, if inputs.ack.is_empty() { "" } else { ", ACCEPTING DATA LOSS" });
    log_line(&root, &format!("======== {} on {} (stick {})", which, computer, stick));
    let _ = std::fs::create_dir_all(root.join("upgrade_").join("reports"));
    let (d, s) = inputs.desktop.job_args();
    log_line(&root, &format!("chose: {}, starts at the {}", d, s));

    // the hash first, so the password leaves memory before anything runs
    let hash_file = flow::hash_file_path();
    let hashed = flow::password_hash(&inputs.password);
    flow::wipe(&mut inputs.password);
    let hash = match hashed {
        Ok(h) => h,
        Err(e) => return stop(Stop::Refused(vec![format!("the password could not be hashed ({})", e)])),
    };
    let hash_text = format!("{}\n", hash);
    let hash_file_text = hash_file.to_string_lossy().to_string();
    let calls = if inputs.erase.is_empty() { flow::convert_calls(&stick, inputs.desktop, &hash_file_text, &inputs.ack) } else { flow::erase_calls(&stick, inputs.desktop, &hash_file_text, &inputs.erase, &inputs.ack) };
    let wifi_dir = root.join("upgrade_").join("artifacts").join("credentials").join("wifi");
    let mut linux_name = String::new();

    for (i, call) in calls.iter().enumerate() {
        let _ = tx.send(Msg::Step(call.step));
        if i == 2 {
            // the job writer takes the hash from a file the launcher keeps in %TEMP%; it goes right after
            if let Err(e) = std::fs::write(&hash_file, &hash_text) {
                return stop(Stop::Refused(vec![format!("could not write the password hash for the job writer ({})", e)]));
            }
        }
        let res = run(call, &root, &tx, &guard);
        if i == 2 {
            let _ = std::fs::remove_file(&hash_file);
        }
        let Some((code, lines)) = res else { return };
        if code != 0 && !call.may_fail {
            log_line(&root, &format!("stopped at step {} (exit {})", call.step, code));
            // the Wi-Fi passwords leave the stick at every stop (2026-09-27)
            let _ = std::fs::remove_dir_all(&wifi_dir);
            return stop(Stop::Refused(flow::refusal_lines(&lines)));
        }
        if i == 0 {
            if let Some(s) = scan_stop(&root, !inputs.ack.is_empty()) {
                return stop(s);
            }
            // the Linux account name, as the launcher asks it before the password (RUN-CONVERT.cmd line 102)
            let Some((ncode, nlines)) = run(&flow::linux_name_call(&stick), &root, &tx, &guard) else { return };
            match flow::linux_name_from(&nlines) {
                Some(n) if ncode == 0 => linux_name = n,
                _ => {
                    log_line(&root, "the Linux account name could not be worked out; stopped");
                    return stop(Stop::Refused(vec![words::LINUX_NAME_UNKNOWN.into()]));
                }
            }
        }
    }
    let ack = inputs.ack.clone();
    // RUN-CONVERT.cmd lines 146-147: this boot is an install
    let u = root.join("upgrade_");
    let _ = std::fs::remove_file(u.join("boot-verify"));
    let _ = std::fs::remove_file(u.join("boot-install"));

    // the decision is the person's: the window shows the sign-in and asks for the word
    let _ = tx.send(Msg::Step(3));
    let _ = tx.send(Msg::Decide(linux_name));
    let Ok(word) = word_rx.recv() else {
        // the window closed, or the word was not the word: nothing was changed
        let _ = std::fs::remove_dir_all(&wifi_dir);
        return;
    };
    {
        let mut g = guard.lock().unwrap();
        if g.cancelled {
            return;
        }
        g.arming = true;
    }
    let _ = tx.send(Msg::Arming);
    let _ = tx.send(Msg::Step(4));
    let prologue = if inputs.erase.is_empty() { flow::prologue_start_call(&stick, &word, &ack) } else { flow::erase_prologue_call(&stick, &inputs.erase, &ack) };
    let res = run(&prologue, &root, &tx, &guard);
    guard.lock().unwrap().arming = false;
    match res {
        Some((0, _)) => {
            log_line(&root, "the prologue armed the handoff; the computer restarts in 15 s");
            let _ = tx.send(Msg::ConvertArmed);
        }
        Some((code, lines)) => {
            log_line(&root, &format!("the prologue stopped (exit {})", code));
            stop(Stop::Prologue(flow::prologue_stop_lines(&lines)));
        }
        None => {}
    }
}

/// The picker's read-only scan (item 24): the kit check, the scan, the
/// scan's own stop (a RED goes no further, as in Convert), then the buttons.
fn pick(stick: String, acknowledged: bool, tx: Sender<Msg>, guard: Shared) {
    let root = stick_root(&stick);
    let stop = |s: Stop| {
        let _ = tx.send(Msg::Stop(s));
    };
    let missing = flow::missing_kit_files_convert(&root);
    if !missing.is_empty() {
        return stop(Stop::Kit(missing.iter().map(|s| s.to_string()).collect()));
    }
    let computer = std::env::var("COMPUTERNAME").unwrap_or_default();
    log_line(&root, &format!("======== the picker on {} (stick {})", computer, stick));
    let _ = std::fs::create_dir_all(root.join("upgrade_").join("reports"));
    let Some((code, lines)) = run(&flow::pick_scan_call(&stick), &root, &tx, &guard) else { return };
    if code != 0 {
        log_line(&root, &format!("the picker's scan stopped (exit {})", code));
        return stop(Stop::Refused(flow::refusal_lines(&lines)));
    }
    if let Some(s) = scan_stop(&root, acknowledged) {
        return stop(s);
    }
    let report = flow::newest_report(&root.join("upgrade_").join("reports")).and_then(|p| std::fs::read(p).ok()).and_then(|b| flow::parse_json(&b));
    let choices = flow::choices(upgrade_scan::data::RELEASES_JSON, report.as_ref());
    for c in &choices {
        log_line(&root, &format!("offered: {} ({}/{}): {:?}", c.title(), c.id, c.desktop, c.starts));
    }
    if !choices.iter().any(|c| c.starts == flow::Starts::Yes) {
        return stop(Stop::NoRelease(choices));
    }
    let _ = tx.send(Msg::Choices(choices));
}

/// The download (item 24): `upgrade-fetch get`, whose progress lines the
/// window shows. A refusal keeps nothing and the person may try again.
fn fetch(stick: String, choice: flow::Choice, from: Option<String>, tx: Sender<Msg>, guard: Shared) {
    let root = stick_root(&stick);
    log_line(&root, &format!("the person chose {} ({}/{})", choice.title(), choice.id, choice.desktop));
    let Some((code, lines)) = run(&flow::fetch_call(&stick, &choice, from.as_deref()), &root, &tx, &guard) else { return };
    if code == 0 {
        log_line(&root, "the download is on the stick and checked");
        let _ = tx.send(Msg::Fetched);
    } else {
        log_line(&root, &format!("the download stopped (exit {})", code));
        let _ = tx.send(Msg::Stop(Stop::Fetch(flow::refusal_lines(&lines))));
    }
}

/// RUN-PROBE.cmd is one call;
/// the probe restarts the computer itself, so the window cannot close while
/// it runs.
fn one_call(stick: String, c: flow::Call, restarts: bool, tx: Sender<Msg>, guard: Shared) {
    let root = stick_root(&stick);
    let computer = std::env::var("COMPUTERNAME").unwrap_or_default();
    log_line(&root, &format!("======== {} on {} (stick {})", c.args.first().map(String::as_str).unwrap_or(""), computer, stick));
    if restarts {
        {
            let mut g = guard.lock().unwrap();
            if g.cancelled {
                return;
            }
            g.arming = true;
        }
        let _ = tx.send(Msg::Arming);
    }
    let _ = tx.send(Msg::Step(1));
    let res = run(&c, &root, &tx, &guard);
    guard.lock().unwrap().arming = false;
    if let Some((code, lines)) = res {
        log_line(&root, &format!("{} finished (exit {})", c.args.first().map(String::as_str).unwrap_or(""), code));
        let _ = tx.send(Msg::Done(code, lines));
    }
}

/// After the restart: wait for the return check to finish, then read what
/// came back on the stick.
fn after_restart(state: Value, tx: Sender<Msg>) {
    let deadline = std::time::Instant::now() + Duration::from_secs(15 * 60);
    while handoff_state().exists() {
        if std::time::Instant::now() > deadline {
            let _ = tx.send(Msg::Stop(Stop::Refused(vec![words::BACK_TIMED_OUT.to_string()])));
            return;
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    // the stick by its volume name first (its letter can change), then by its old letter
    let mut roots = vec![];
    if let Some(v) = state["stick_volume"].as_str() {
        roots.push(PathBuf::from(v));
    }
    if let Some(s) = state["stick"].as_str() {
        roots.push(PathBuf::from(format!("{}\\", s)));
    }
    let root = roots.into_iter().find(|r| r.join("upgrade_").exists());
    let read = |rel: &[&str]| -> Option<Vec<u8>> {
        let r = root.as_ref()?;
        std::fs::read(rel.iter().fold(r.clone(), |p, s| p.join(s))).ok()
    };
    let verify = read(&["upgrade_", "report", "verify.json"]).and_then(|b| flow::parse_json(&b));
    let refusal = read(&["upgrade_", "report", "refusal.json"]).and_then(|b| flow::parse_json(&b));
    let since = state["armed_utc"].as_str().unwrap_or("");
    let handoff = read(&["v0-handoff.csv"]).and_then(|b| flow::handoff_result(&String::from_utf8_lossy(&b), since));
    let view = flow::result_view(root.is_some(), verify.as_ref(), refusal.as_ref(), handoff.as_deref());
    if let Some(r) = &root {
        log_line(r, &format!("after the restart: {} (handoff {})", view.heading, handoff.as_deref().unwrap_or("no row")));
    }
    let _ = tx.send(Msg::Result(view));
}

// ---------------------------------------------------------------- the window

#[derive(Clone, Copy, PartialEq)]
enum Flow {
    Verify,
    Convert,
    Erase,
    Rollback,
    Probe,
}

/// The first screen's buttons. Verify and Probe make the rows (V0, V1, R24)
/// and a person converting never needs them: they show only with
/// --developer (roadmap item 20, R31). Erase is no longer a button: it is
/// Convert's clean-slate answer, on the set-up screen. The data-loss launcher
/// shows only its own convert (rule #1, R23).
fn first_screen(accepting_data_loss: bool, developer: bool) -> Vec<(&'static str, &'static str, Flow)> {
    if accepting_data_loss {
        return vec![(words::CHOOSE_CONVERT_ACK, words::CHOOSE_CONVERT_ACK_LINE, Flow::Convert)];
    }
    let mut v = vec![];
    if developer {
        v.push((words::CHOOSE_VERIFY, words::CHOOSE_VERIFY_LINE, Flow::Verify));
    }
    v.push((words::CHOOSE_CONVERT, words::CHOOSE_CONVERT_LINE, Flow::Convert));
    v.push((words::CHOOSE_ROLLBACK, words::CHOOSE_ROLLBACK_LINE, Flow::Rollback));
    if developer {
        v.push((words::CHOOSE_PROBE, words::CHOOSE_PROBE_LINE, Flow::Probe));
    }
    v
}

enum Screen {
    Choose,
    /// the picker's scan is running
    Picking,
    /// the picker: one release with one desktop
    Pick,
    /// the download: bytes so far and in all
    Fetching { done: u64, total: u64 },
    Welcome,
    /// the convert and erase set-up: the desktop, the sentences, the password
    ConvertSetup,
    Running { step: usize },
    Decide { linux_name: String },
    Stopped(Stop),
    Restarting,
    ConvertRestarting,
    /// roll back, probe, cancel: the words and the one button (or the word)
    Ask,
    /// a one-call flow finished well: its heading and line
    Done(String, String),
    Waiting,
    Result(flow::ResultView),
}

/// What the convert set-up screen holds while the person fills it in.
#[derive(Default)]
struct Setup {
    desktop: Option<Desktop>,
    password: String,
    again: String,
    ack: String,
    erase: String,
    error: String,
}

struct App {
    screen: Screen,
    /// the picker's buttons, the one chosen, and the one on the stick (item 24)
    choices: Vec<flow::Choice>,
    picked: Option<usize>,
    fetched: Option<flow::Choice>,
    /// --developer --from <http folder>: the rig's local copy of the releases
    from: Option<String>,
    stick: String,
    after: bool,
    arming: bool,
    flow: Flow,
    /// started with --accepting-data-loss: the convert flow asks the sentence (RISKS R23)
    accepting_data_loss: bool,
    /// started with --developer: Verify and Probe on the first screen (roadmap item 20)
    developer: bool,
    setup: Setup,
    word: String,
    word_tx: Option<Sender<String>>,
    log: Vec<String>,
    rx: Option<Receiver<Msg>>,
    guard: Shared,
}

impl App {
    fn new(screen: Screen, stick: String, after: bool, accepting_data_loss: bool, rx: Option<Receiver<Msg>>, guard: Shared) -> App {
        App { screen, choices: vec![], picked: None, fetched: None, from: None, stick, after, arming: false, flow: Flow::Verify, accepting_data_loss, developer: false, setup: Setup::default(), word: String::new(), word_tx: None, log: vec![], rx, guard }
    }

    fn channel(&mut self, ctx: &egui::Context) -> Sender<Msg> {
        let (tx, rx) = mpsc::channel();
        let c = ctx.clone();
        let (ptx, prx) = mpsc::channel::<Msg>();
        // forward and repaint, so the window moves as soon as a line arrives
        std::thread::spawn(move || {
            for m in prx {
                if tx.send(m).is_err() {
                    break;
                }
                c.request_repaint();
            }
        });
        self.rx = Some(rx);
        ptx
    }

    fn start_verify(&mut self, ctx: &egui::Context) {
        let ptx = self.channel(ctx);
        let (stick, guard) = (self.stick.clone(), self.guard.clone());
        std::thread::spawn(move || verify(stick, ptx, guard));
        self.flow = Flow::Verify;
        self.screen = Screen::Running { step: 0 };
    }

    fn start_pick(&mut self, ctx: &egui::Context) {
        let ptx = self.channel(ctx);
        let (stick, guard, ack) = (self.stick.clone(), self.guard.clone(), self.accepting_data_loss);
        std::thread::spawn(move || pick(stick, ack, ptx, guard));
        self.picked = None;
        self.fetched = None;
        self.screen = Screen::Picking;
    }

    fn start_fetch(&mut self, ctx: &egui::Context) {
        let Some(c) = self.picked.and_then(|i| self.choices.get(i)).cloned() else { return };
        let ptx = self.channel(ctx);
        let (stick, guard, from) = (self.stick.clone(), self.guard.clone(), self.from.clone());
        let chosen = c.clone();
        std::thread::spawn(move || fetch(stick, chosen, from, ptx, guard));
        self.fetched = None;
        self.screen = Screen::Fetching { done: 0, total: 0 };
    }

    fn start_convert(&mut self, ctx: &egui::Context) {
        let ptx = self.channel(ctx);
        let (stick, guard) = (self.stick.clone(), self.guard.clone());
        let (wtx, wrx) = mpsc::channel::<String>();
        self.word_tx = Some(wtx);
        let inputs = ConvertInputs {
            desktop: self.setup.desktop.unwrap_or(Desktop::Kde),
            release: self.fetched.as_ref().map(|c| c.id.clone()).unwrap_or_default(),
            password: std::mem::take(&mut self.setup.password),
            ack: if self.accepting_data_loss { self.setup.ack.clone() } else { String::new() },
            erase: if self.flow == Flow::Erase { self.setup.erase.clone() } else { String::new() },
        };
        flow::wipe(&mut self.setup.again);
        std::thread::spawn(move || convert(stick, inputs, ptx, guard, wrx));
        self.screen = Screen::Running { step: 0 };
    }

    fn start_one(&mut self, ctx: &egui::Context, c: flow::Call, restarts: bool) {
        let ptx = self.channel(ctx);
        let (stick, guard) = (self.stick.clone(), self.guard.clone());
        std::thread::spawn(move || one_call(stick, c, restarts, ptx, guard));
        self.screen = Screen::Running { step: 1 };
    }

    fn pump(&mut self) {
        let Some(rx) = &self.rx else { return };
        while let Ok(m) = rx.try_recv() {
            match m {
                Msg::Step(s) => self.screen = Screen::Running { step: s },
                Msg::Line(l) => match upgrade_fetch::progress_of(&l) {
                    Some((done, total)) => {
                        if let Screen::Fetching { .. } = self.screen {
                            self.screen = Screen::Fetching { done, total };
                        }
                    }
                    None => self.log.push(l),
                },
                Msg::Choices(c) => {
                    // nothing chosen for the person, as the desktop never was
                    self.picked = None;
                    self.choices = c;
                    self.screen = Screen::Pick;
                }
                Msg::Fetched => {
                    let c = self.picked.and_then(|i| self.choices.get(i)).cloned();
                    // the person picks the desktop, unless there is only one (code review 2026-11-08)
                    let offered = c.as_ref().map(|c| flow::desktops_for(&c.desktop)).unwrap_or_default();
                    self.setup.desktop = if offered.len() == 1 { offered.first().copied() } else { None };
                    self.fetched = c;
                    self.screen = Screen::ConvertSetup;
                }
                Msg::Arming => self.arming = true,
                Msg::Armed => {
                    self.arming = false;
                    self.screen = Screen::Restarting;
                }
                Msg::ConvertArmed => {
                    self.arming = false;
                    self.screen = Screen::ConvertRestarting;
                }
                Msg::Decide(name) => self.screen = Screen::Decide { linux_name: name },
                Msg::Stop(s) => {
                    self.arming = false;
                    self.screen = Screen::Stopped(s);
                }
                Msg::Result(v) => self.screen = Screen::Result(v),
                Msg::Done(code, lines) => {
                    self.arming = false;
                    self.screen = match (self.flow, code) {
                        (Flow::Probe, 0) => Screen::Done(words::PROBE_RESTARTING_HEADING.into(), words::PROBE_RESTARTING_LINE.into()),
                        (_, _) => Screen::Stopped(Stop::Refused(flow::refusal_lines(&lines))),
                    };
                }
            }
        }
    }

    fn on_close(&mut self, ctx: &egui::Context) {
        if !ctx.input(|i| i.viewport().close_requested()) {
            return;
        }
        let mut g = self.guard.lock().unwrap();
        if g.arming {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            return;
        }
        g.cancelled = true;
        if let Some(c) = g.child.as_mut() {
            // a program may have started one of its own: stop the whole tree
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                let _ = Command::new("taskkill.exe").args(["/PID", &c.id().to_string(), "/T", "/F"]).creation_flags(win::CREATE_NO_WINDOW).status();
            }
            let _ = c.kill();
        }
        drop(g);
        self.word_tx = None;
        if matches!(self.screen, Screen::Running { .. } | Screen::Decide { .. } | Screen::Picking | Screen::Fetching { .. }) {
            log_line(&stick_root(&self.stick), "closed by the person before the handoff was armed; nothing was changed");
        }
        if self.after {
            let _ = std::fs::remove_file(state_dir().join("state.json"));
            win::remove_after_exit(&state_dir());
        }
    }

    /// The set-up screen's own checks, in the launcher's order: the desktop,
    /// the sentence on the data-loss path, the password pair.
    fn setup_refusal(&self) -> Option<String> {
        if self.setup.desktop.is_none() {
            return Some(words::DESKTOP_UNSURE.into());
        }
        if let Some(c) = &self.fetched {
            if !self.setup.desktop.is_some_and(|d| flow::desktops_for(&c.desktop).contains(&d)) {
                return Some(words::DESKTOP_UNSURE.into());
            }
        }
        if self.accepting_data_loss && !flow::ack_ok(&self.setup.ack) {
            return Some(format!("{} {}", words::ACK_TYPE, upgrade_job::RISK_STATEMENT));
        }
        if self.flow == Flow::Erase && !flow::erase_ok(&self.setup.erase) {
            return Some(words::ERASE_TYPE.into());
        }
        flow::password_refusal(&self.setup.password, &self.setup.again).map(|why| format!("{}: {}.", words::PASSWORD_NOT_SET, why))
    }
}

fn heading(ui: &mut egui::Ui, t: &str) {
    ui.label(egui::RichText::new(t).strong().size(22.0));
    ui.add_space(8.0);
}

fn para(ui: &mut egui::Ui, t: &str) {
    ui.add(egui::Label::new(t).wrap());
    ui.add_space(4.0);
}

fn details(ui: &mut egui::Ui, log: &[String]) {
    if log.is_empty() {
        return;
    }
    ui.add_space(8.0);
    egui::CollapsingHeader::new(words::DETAILS).show(ui, |ui| {
        egui::ScrollArea::vertical().max_height(220.0).stick_to_bottom(true).show(ui, |ui| {
            for l in log {
                ui.monospace(l);
            }
        });
    });
}

fn steps(ui: &mut egui::Ui, names: &[&str], step: usize) {
    for (i, s) in names.iter().enumerate() {
        ui.horizontal(|ui| {
            if i < step {
                ui.label("✔");
                ui.label(*s);
            } else if i == step {
                ui.spinner();
                ui.label(egui::RichText::new(*s).strong());
            } else {
                ui.label("•");
                ui.label(egui::RichText::new(*s).weak());
            }
        });
    }
}

/// The desktop's own line, as the set-up screen says it.
fn desktop_line(desktop: &str) -> &'static str {
    match desktop {
        "kde" => words::DESKTOP_KDE_LINE,
        "gnome" => words::DESKTOP_GNOME_LINE,
        _ => "",
    }
}

fn banner(ui: &mut egui::Ui) {
    ui.label(egui::RichText::new(words::ACK_BANNER).strong().color(egui::Color32::from_rgb(180, 30, 30)).size(18.0));
    ui.add_space(6.0);
}

enum Press {
    None,
    /// the picker's one button
    Fetch,
    /// back to the picker after a download stopped
    PickAgain,
    StartVerify,
    StartConvert,
    Word,
    /// the one button of the erase decision, the probe and the cancel screens
    Go,
    Close,
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.pump();
        self.on_close(&ctx);
        if self.rx.is_some() {
            ctx.request_repaint_after(Duration::from_millis(300));
        }
        let mut press = Press::None;
        let mut go: Option<Flow> = None;
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.add_space(6.0);
                if self.accepting_data_loss && matches!(self.screen, Screen::ConvertSetup | Screen::Running { .. } | Screen::Decide { .. } | Screen::ConvertRestarting) {
                    banner(ui);
                }
                match &self.screen {
                    Screen::Choose => {
                        heading(ui, words::CHOOSE_HEADING);
                        let mut choice = |ui: &mut egui::Ui, label: &str, line: &str, f: Flow| {
                            if ui.add(egui::Button::new(egui::RichText::new(label).size(17.0))).clicked() {
                                go = Some(f);
                            }
                            para(ui, line);
                            ui.add_space(8.0);
                        };
                        for (label, line, f) in first_screen(self.accepting_data_loss, self.developer) {
                            choice(ui, label, line, f);
                        }
                        ui.add_space(12.0);
                        if ui.button(words::CLOSE).clicked() {
                            press = Press::Close;
                        }
                    }
                    Screen::Picking => {
                        heading(ui, words::PICKING_HEADING);
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(words::PICKING_LINE);
                        });
                        details(ui, &self.log);
                    }
                    Screen::Pick => {
                        heading(ui, words::PICK_HEADING);
                        para(ui, words::PICK_LEAD);
                        ui.add_space(6.0);
                        for (i, c) in self.choices.iter().enumerate() {
                            let ok = c.starts == flow::Starts::Yes;
                            ui.add_enabled_ui(ok, |ui| {
                                ui.radio_value(&mut self.picked, Some(i), egui::RichText::new(c.title()).size(17.0));
                            });
                            match &c.starts {
                                flow::Starts::Yes => para(ui, desktop_line(&c.desktop)),
                                flow::Starts::No(why) => para(ui, &format!("{} {}", words::PICK_CANNOT, why)),
                                flow::Starts::Unknown => para(ui, words::PICK_UNKNOWN),
                            }
                            ui.add_space(6.0);
                        }
                        para(ui, words::DESKTOP_UNSURE);
                        ui.add_space(6.0);
                        para(ui, words::PICK_SIZE);
                        ui.add_space(12.0);
                        ui.horizontal(|ui| {
                            let can = self.picked.and_then(|i| self.choices.get(i)).is_some_and(|c| c.starts == flow::Starts::Yes);
                            if ui.add_enabled(can, egui::Button::new(egui::RichText::new(words::PICK_GO).size(17.0))).clicked() {
                                press = Press::Fetch;
                            }
                            if ui.button(words::CLOSE).clicked() {
                                press = Press::Close;
                            }
                        });
                    }
                    Screen::Fetching { done, total } => {
                        let title = self.picked.and_then(|i| self.choices.get(i)).map(|c| c.title()).unwrap_or_default();
                        heading(ui, &format!("{} {}", words::FETCH_HEADING, title));
                        if *total == 0 {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(words::FETCH_STARTING);
                            });
                        } else {
                            let gb = |b: u64| b as f64 / 1e9;
                            ui.add(egui::ProgressBar::new(*done as f32 / *total as f32).show_percentage());
                            para(ui, &format!("{:.1} of {:.1} GB.", gb(*done), gb(*total)));
                        }
                        ui.add_space(6.0);
                        para(ui, words::FETCH_NOTHING_CHANGED);
                        para(ui, words::FETCH_CLOSE);
                        details(ui, &self.log);
                    }
                    Screen::Welcome => {
                        heading(ui, words::WELCOME_HEADING);
                        para(ui, words::WELCOME_LEAD);
                        ui.add_space(6.0);
                        ui.label(egui::RichText::new(words::WELCOME_STEPS_HEADING).strong());
                        for (i, s) in words::WELCOME_STEPS.iter().enumerate() {
                            para(ui, &format!("{}.  {}", i + 1, s));
                        }
                        ui.add_space(6.0);
                        ui.label(egui::RichText::new(words::WELCOME_STICK).strong());
                        para(ui, words::WELCOME_BITLOCKER);
                        ui.add_space(12.0);
                        ui.horizontal(|ui| {
                            if ui.add(egui::Button::new(egui::RichText::new(words::START).size(17.0))).clicked() {
                                press = Press::StartVerify;
                            }
                            if ui.button(words::CLOSE).clicked() {
                                press = Press::Close;
                            }
                        });
                    }
                    Screen::ConvertSetup => {
                        if self.accepting_data_loss {
                            heading(ui, words::ACK_HEADING);
                            para(ui, words::ACK_LEAD);
                            ui.label(egui::RichText::new(words::ACK_BEFORE_HEADING).strong());
                            for l in words::ACK_BEFORE {
                                para(ui, &format!("•  {}", l));
                            }
                            ui.add_space(8.0);
                        }
                        // Convert's one switch: keep Windows, or the clean slate (the erase
                        // flow, unchanged). A switch clears the typed erase sentence, so it is
                        // always typed under the words it answers.
                        ui.label(egui::RichText::new(words::PATH_HEADING).strong().size(17.0));
                        let before = self.flow;
                        ui.radio_value(&mut self.flow, Flow::Convert, words::PATH_KEEP);
                        para(ui, words::PATH_KEEP_LINE);
                        ui.radio_value(&mut self.flow, Flow::Erase, words::PATH_CLEAN);
                        para(ui, words::PATH_CLEAN_LINE);
                        if self.flow != before {
                            self.setup.erase.clear();
                        }
                        ui.add_space(10.0);
                        if self.flow == Flow::Erase {
                            heading(ui, words::ERASE_HEADING);
                            ui.label(egui::RichText::new(words::ERASE_READ_FIRST).strong().size(17.0));
                            for l in words::ERASE_LINES {
                                para(ui, l);
                            }
                        } else {
                            heading(ui, words::CONVERT_HEADING);
                            para(ui, words::CONVERT_LEAD);
                            ui.label(egui::RichText::new(words::WELCOME_STEPS_HEADING).strong());
                            for (i, s) in words::CONVERT_ORDER.iter().enumerate() {
                                para(ui, &format!("{}.  {}", i + 1, s));
                            }
                            para(ui, words::CONVERT_WIFI);
                        }
                        ui.add_space(10.0);
                        if let Some(c) = &self.fetched {
                            ui.label(egui::RichText::new(format!("{} {}", c.title(), words::FETCHED_LINE)).strong());
                            ui.add_space(6.0);
                        }
                        ui.label(egui::RichText::new(words::DESKTOP_HEADING).strong().size(17.0));
                        let offered = self.fetched.as_ref().map(|c| flow::desktops_for(&c.desktop)).unwrap_or_else(|| vec![Desktop::Kde, Desktop::Gnome, Desktop::Console]);
                        for d in offered {
                            let (label, line) = match d {
                                Desktop::Kde => (words::DESKTOP_KDE, words::DESKTOP_KDE_LINE),
                                Desktop::Gnome => (words::DESKTOP_GNOME, words::DESKTOP_GNOME_LINE),
                                Desktop::Console => (words::DESKTOP_CONSOLE, words::DESKTOP_CONSOLE_LINE),
                            };
                            ui.radio_value(&mut self.setup.desktop, Some(d), label);
                            para(ui, line);
                        }
                        if self.accepting_data_loss {
                            ui.add_space(10.0);
                            ui.label(egui::RichText::new(words::ACK_TYPE).strong().size(17.0));
                            ui.monospace(upgrade_job::RISK_STATEMENT);
                            ui.add(egui::TextEdit::singleline(&mut self.setup.ack).desired_width(f32::INFINITY));
                        }
                        if self.flow == Flow::Erase {
                            ui.add_space(10.0);
                            ui.label(egui::RichText::new(words::DECIDE_HEADING).strong().size(17.0));
                            para(ui, words::ERASE_TYPE);
                            ui.monospace(upgrade_job::ERASE_STATEMENT);
                            ui.add(egui::TextEdit::singleline(&mut self.setup.erase).desired_width(f32::INFINITY));
                        }
                        ui.add_space(10.0);
                        ui.label(egui::RichText::new(words::PASSWORD_HEADING).strong().size(17.0));
                        para(ui, words::PASSWORD_LEAD);
                        egui::Grid::new("pw").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                            ui.label(words::PASSWORD_LABEL);
                            ui.add(egui::TextEdit::singleline(&mut self.setup.password).password(true).desired_width(260.0));
                            ui.end_row();
                            ui.label(words::PASSWORD_AGAIN);
                            ui.add(egui::TextEdit::singleline(&mut self.setup.again).password(true).desired_width(260.0));
                            ui.end_row();
                        });
                        if !self.setup.error.is_empty() {
                            ui.add_space(4.0);
                            ui.label(egui::RichText::new(&self.setup.error).color(egui::Color32::from_rgb(180, 30, 30)));
                        }
                        ui.add_space(12.0);
                        ui.horizontal(|ui| {
                            if ui.add(egui::Button::new(egui::RichText::new(words::CONVERT_CONTINUE).size(17.0))).clicked() {
                                press = Press::StartConvert;
                            }
                            if ui.button(words::CLOSE).clicked() {
                                press = Press::Close;
                            }
                        });
                    }
                    Screen::Running { step } => {
                        let (h, names): (&str, &[&str]) = match self.flow {
                            Flow::Verify => (words::RUNNING_HEADING, &words::STEPS),
                            Flow::Convert => (words::CONVERT_RUNNING_HEADING, &words::CONVERT_STEPS),
                            Flow::Erase => (words::ERASE_RUNNING_HEADING, &words::ERASE_STEPS),
                            Flow::Rollback => (words::ROLLBACK_HEADING, &[]),
                            Flow::Probe => (words::PROBE_RUNNING, &[]),
                        };
                        heading(ui, h);
                        if names.is_empty() {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(h);
                            });
                        } else {
                            steps(ui, names, *step);
                        }
                        ui.add_space(10.0);
                        para(
                            ui,
                            match (self.flow, self.arming) {
                                (Flow::Verify, true) | (Flow::Probe, true) => words::RUNNING_ARMING,
                                (_, true) => words::CONVERT_RUNNING_PROLOGUE,
                                (_, false) => words::RUNNING_CANCEL,
                            },
                        );
                        details(ui, &self.log);
                    }
                    Screen::Decide { linux_name } => {
                        heading(ui, words::SIGN_IN_HEADING);
                        egui::Grid::new("signin").num_columns(2).spacing([18.0, 6.0]).show(ui, |ui| {
                            ui.label(egui::RichText::new(words::SIGN_IN_USER).strong());
                            ui.monospace(linux_name);
                            ui.end_row();
                        });
                        para(ui, words::SIGN_IN_PASSWORD);
                        ui.add_space(10.0);
                        if self.flow == Flow::Erase {
                            heading(ui, words::ERASE_DECIDE_HEADING);
                            for l in words::ERASE_DECIDE_LINES {
                                para(ui, l);
                            }
                            ui.add_space(12.0);
                            ui.horizontal(|ui| {
                                if ui.add(egui::Button::new(egui::RichText::new(words::ERASE_GO).size(17.0))).clicked() {
                                    press = Press::Go;
                                }
                                if ui.button(words::CLOSE).clicked() {
                                    press = Press::Close;
                                }
                            });
                        } else {
                            heading(ui, words::DECIDE_HEADING);
                            for l in words::DECIDE_LINES {
                                para(ui, l);
                            }
                            ui.add_space(6.0);
                            ui.label(egui::RichText::new(words::DECIDE_TYPE).strong());
                            ui.add(egui::TextEdit::singleline(&mut self.word).desired_width(200.0));
                            ui.add_space(12.0);
                            ui.horizontal(|ui| {
                                if ui.add(egui::Button::new(egui::RichText::new(words::CONVERT_CONTINUE).size(17.0))).clicked() {
                                    press = Press::Word;
                                }
                                if ui.button(words::CLOSE).clicked() {
                                    press = Press::Close;
                                }
                            });
                        }
                        details(ui, &self.log);
                    }
                    Screen::Stopped(s) => {
                        match s {
                            Stop::Kit(files) => {
                                heading(ui, words::KIT_HEADING);
                                para(ui, words::KIT_LEAD);
                                for f in files {
                                    ui.monospace(f);
                                }
                                ui.add_space(6.0);
                                para(ui, words::KIT_FIX);
                            }
                            Stop::Fetch(lines) => {
                                heading(ui, words::FETCH_STOPPED_HEADING);
                                for l in lines {
                                    para(ui, l);
                                }
                                ui.add_space(6.0);
                                para(ui, words::FETCH_STOPPED_FOOT);
                                if ui.add(egui::Button::new(egui::RichText::new(words::FETCH_AGAIN).size(17.0))).clicked() {
                                    press = Press::PickAgain;
                                }
                            }
                            Stop::NoRelease(choices) => {
                                heading(ui, words::PICK_NONE_HEADING);
                                for c in choices {
                                    let why = match &c.starts {
                                        flow::Starts::No(w) => w.clone(),
                                        _ => words::PICK_UNKNOWN.to_string(),
                                    };
                                    para(ui, &format!("•  {}: {}", c.title(), why));
                                }
                                ui.add_space(6.0);
                                para(ui, words::NOTHING_CHANGED);
                            }
                            Stop::Red(v) => {
                                heading(ui, words::RED_HEADING);
                                if let Some(v) = v {
                                    para(ui, &v.summary);
                                    for (label, items) in &v.groups {
                                        ui.add_space(4.0);
                                        ui.label(egui::RichText::new(label).strong());
                                        for it in items {
                                            para(ui, &format!("•  {}", it));
                                        }
                                    }
                                }
                                ui.add_space(6.0);
                                para(ui, words::NOTHING_CHANGED);
                            }
                            Stop::Refused(lines) => {
                                heading(
                                    ui,
                                    match self.flow {
                                        Flow::Convert | Flow::Erase => words::CONVERT_STOPPED_HEADING,
                                        _ => words::STOPPED_HEADING,
                                    },
                                );
                                for l in lines {
                                    para(ui, l);
                                }
                                ui.add_space(6.0);
                                para(ui, words::NOTHING_CHANGED);
                            }
                            Stop::Prologue(lines) => {
                                heading(ui, words::CONVERT_STOPPED_HEADING);
                                for l in lines {
                                    para(ui, l);
                                }
                                ui.add_space(6.0);
                                para(ui, words::CONVERT_STOPPED_FOOT);
                            }
                        }
                        para(ui, words::LOG_WHERE);
                        details(ui, &self.log);
                        ui.add_space(10.0);
                        if ui.button(words::CLOSE).clicked() {
                            press = Press::Close;
                        }
                    }
                    Screen::Restarting => {
                        heading(ui, words::RESTARTING_HEADING);
                        for l in words::RESTARTING_LINES {
                            para(ui, l);
                        }
                        details(ui, &self.log);
                    }
                    Screen::ConvertRestarting => {
                        let (h, lines): (&str, &[&str]) = if self.flow == Flow::Erase { (words::ERASE_RESTARTING_HEADING, &words::ERASE_RESTARTING_LINES) } else { (words::CONVERT_RESTARTING_HEADING, &words::CONVERT_RESTARTING_LINES) };
                        heading(ui, h);
                        for l in lines {
                            para(ui, l);
                        }
                        details(ui, &self.log);
                    }
                    Screen::Ask => {
                        let (h, lines, button): (&str, &[&str], &str) = (words::PROBE_HEADING, &words::PROBE_LINES, words::PROBE_GO);
                        heading(ui, h);
                        for l in lines {
                            para(ui, l);
                        }
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.add(egui::Button::new(egui::RichText::new(button).size(17.0))).clicked() {
                                press = Press::Go;
                            }
                            if ui.button(words::CLOSE).clicked() {
                                press = Press::Close;
                            }
                        });
                    }
                    Screen::Done(h, line) => {
                        heading(ui, h);
                        para(ui, line);
                        details(ui, &self.log);
                        ui.add_space(10.0);
                        if ui.button(words::CLOSE).clicked() {
                            press = Press::Close;
                        }
                    }
                    Screen::Waiting => {
                        heading(ui, words::BACK_HEADING);
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(words::BACK_WAITING);
                        });
                        ui.add_space(8.0);
                        para(ui, words::BACK_POPUP);
                    }
                    Screen::Result(v) => {
                        heading(ui, &v.heading);
                        egui::Grid::new("rows").num_columns(2).spacing([18.0, 6.0]).show(ui, |ui| {
                            for (k, val) in &v.rows {
                                ui.label(egui::RichText::new(k).strong());
                                ui.add(egui::Label::new(val).wrap());
                                ui.end_row();
                            }
                        });
                        ui.add_space(8.0);
                        for l in &v.lines {
                            para(ui, l);
                        }
                        ui.add_space(10.0);
                        if ui.button(words::CLOSE).clicked() {
                            press = Press::Close;
                        }
                    }
                }
            });
        });
        if let Some(f) = go {
            self.flow = f;
            self.screen = match f {
                Flow::Verify => Screen::Welcome,
                // the picker first (item 24): the scan, the choice, the download
                Flow::Convert | Flow::Erase => {
                    self.start_pick(&ctx);
                    Screen::Picking
                }
                Flow::Rollback => Screen::Done(words::ROLLBACK_HEADING.into(), words::ROLLBACK_FROM_LINUX.into()),
                Flow::Probe => Screen::Ask,
            };
        }
        match press {
            Press::StartVerify => {
                log_line(&stick_root(&self.stick), "the person pressed Start the test");
                self.start_verify(&ctx);
            }
            Press::StartConvert => match self.setup_refusal() {
                Some(why) => self.setup.error = why,
                None => {
                    self.setup.error.clear();
                    log_line(&stick_root(&self.stick), "the person pressed Continue on the convert set-up");
                    self.start_convert(&ctx);
                }
            },
            Press::Word => {
                let root = stick_root(&self.stick);
                if flow::confirm_ok(&self.word) {
                    log_line(&root, "the confirmation word was typed");
                    if let Some(tx) = self.word_tx.take() {
                        let _ = tx.send(std::mem::take(&mut self.word));
                    }
                    self.screen = Screen::Running { step: 4 };
                } else {
                    log_line(&root, "the confirmation word was not typed; stopped");
                    self.word_tx = None;
                    self.screen = Screen::Stopped(Stop::Refused(vec![words::DECIDE_NOT_CONFIRMED.into()]));
                }
            }
            Press::Go => match self.flow {
                Flow::Erase => {
                    log_line(&stick_root(&self.stick), "the person pressed Restart into the installer");
                    if let Some(tx) = self.word_tx.take() {
                        let _ = tx.send(String::new());
                    }
                    self.screen = Screen::Running { step: 4 };
                }
                Flow::Probe => {
                    let c = flow::probe_call(&self.stick);
                    self.start_one(&ctx, c, true);
                }
                _ => {}
            },
            Press::Fetch => {
                if self.picked.and_then(|i| self.choices.get(i)).is_some_and(|c| c.starts == flow::Starts::Yes) {
                    log_line(&stick_root(&self.stick), "the person pressed Download and continue");
                    self.start_fetch(&ctx);
                }
            }
            Press::PickAgain => {
                log_line(&stick_root(&self.stick), "the person pressed Try again after the download stopped");
                self.screen = Screen::Pick;
            }
            Press::Close => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Press::None => {}
        }
    }
}

// ---------------------------------------------------------------- start

/// Screens with made-up content, to look at the words without running
/// anything (UPGRADE.exe --preview <name>). Nothing is run or written.
fn preview(name: &str) -> (Screen, Flow, bool) {
    let v = json!({"identity":{"result":"pass"},"hardware":{"display":"pass","wifi":"pass","wifi_detail":"wlp1s0 sees 28 networks","audio_firmware":"skipped"},"payload":{"result":"pass","read_mbps":"22.6"}});
    match name {
        "choose" => (Screen::Choose, Flow::Verify, false),
        "choose-ack" => (Screen::Choose, Flow::Verify, true),
        "running" => (Screen::Running { step: 2 }, Flow::Verify, false),
        "kit" => (Screen::Stopped(Stop::Kit(vec!["upgrade_\\LiveOS\\kde.squashfs".into()])), Flow::Verify, false),
        "red" => (
            Screen::Stopped(Stop::Red(Some(flow::Verdict {
                level: "RED".into(),
                summary: "Do not convert this machine as it stands. Something here blocks the install outright - resolve it, or use a different machine.".into(),
                groups: vec![("Blocks the install".into(), vec!["Drive health: the SSD reports 725 bad blocks (SMART 187)".into()])],
            }))),
            Flow::Verify,
            false,
        ),
        "stopped" => (Screen::Stopped(Stop::Refused(vec!["BitLocker state is unknown".into()])), Flow::Verify, false),
        "restarting" => (Screen::Restarting, Flow::Verify, false),
        "waiting" => (Screen::Waiting, Flow::Verify, false),
        "result" => (Screen::Result(flow::result_view(true, Some(&v), None, Some("fired-once"))), Flow::Verify, false),
        "result-refused" => (Screen::Result(flow::result_view(true, None, Some(&json!({"reason":"the copy of Linux on this USB stick is damaged"})), Some("fired-once"))), Flow::Verify, false),
        "no-linux" => (Screen::Result(flow::result_view(true, None, None, Some("ignored"))), Flow::Verify, false),
        "picking" => (Screen::Picking, Flow::Convert, false),
        "convert" => (Screen::ConvertSetup, Flow::Convert, false),
        "convert-ack" => (Screen::ConvertSetup, Flow::Convert, true),
        "convert-running" => (Screen::Running { step: 2 }, Flow::Convert, false),
        "decide" => (Screen::Decide { linux_name: "ann".into() }, Flow::Convert, false),
        "convert-restarting" => (Screen::ConvertRestarting, Flow::Convert, false),
        "erase" => (Screen::ConvertSetup, Flow::Erase, false),
        "erase-decide" => (Screen::Decide { linux_name: "ann".into() }, Flow::Erase, false),
        "erase-restarting" => (Screen::ConvertRestarting, Flow::Erase, false),
        "rollback" => (Screen::Done(words::ROLLBACK_HEADING.into(), words::ROLLBACK_FROM_LINUX.into()), Flow::Rollback, false),
        "probe" => (Screen::Ask, Flow::Probe, false),
        "convert-stopped" => (Screen::Stopped(Stop::Prologue(vec!["STOPPED at revalidate: job.json no longer matches this machine: volume_health.repair_queued: job says 'False', machine says 'True'".into(), "outcome.json (stopped) written to the stick".into()])), Flow::Convert, false),
        _ => (Screen::Welcome, Flow::Verify, false),
    }
}

/// The picker's previews (item 24): made-up buttons, nothing fetched.
fn preview_picker(app: &mut App, name: &str) {
    let c = |desktop: &str, label: &str, starts: flow::Starts| flow::Choice { id: "fedora-44".into(), name: "Fedora 44".into(), desktop: desktop.into(), label: label.into(), starts };
    let yes = vec![c("kde", "KDE Plasma", flow::Starts::Yes), c("gnome", "GNOME", flow::Starts::Yes)];
    let no = vec![c("kde", "KDE Plasma", flow::Starts::No("grub,3 < grub,5".into())), c("gnome", "GNOME", flow::Starts::Unknown)];
    app.flow = Flow::Convert;
    match name {
        "pick" => {
            app.choices = yes;
            app.picked = Some(0);
            app.screen = Screen::Pick;
        }
        "pick-refused" => {
            app.choices = vec![c("kde", "KDE Plasma", flow::Starts::Yes), c("gnome", "GNOME", flow::Starts::No("grub,3 < grub,5".into()))];
            app.picked = Some(0);
            app.screen = Screen::Pick;
        }
        "pick-none" => app.screen = Screen::Stopped(Stop::NoRelease(no)),
        "fetching" => {
            app.choices = yes;
            app.picked = Some(0);
            app.screen = Screen::Fetching { done: 1_300_000_000, total: 4_597_067_776 };
        }
        "fetch-stopped" => app.screen = Screen::Stopped(Stop::Fetch(vec!["the download of Fedora-KDE-Desktop-Live-44-1.7.x86_64.iso did not match its measured sha256; nothing was kept".into()])),
        "convert-fetched" => {
            app.fetched = Some(yes[0].clone());
            app.setup.desktop = Some(Desktop::Kde);
            app.screen = Screen::ConvertSetup;
        }
        _ => {}
    }
}

/// Glow (OpenGL) first; a machine whose display driver offers no usable
/// OpenGL (the rig's basic display adapter has only 1.1: V12, 2026-09-27)
/// gets wgpu, which draws through Direct3D and falls back to Windows' own
/// software renderer. winit allows one event loop per process, so the second
/// try is this program started again with --wgpu. Every failure goes into the
/// stick's log before anything else happens (rule #5).
fn open(app: App) {
    let wgpu = std::env::args().any(|a| a == "--wgpu");
    let root = stick_root(&app.stick);
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title(words::TITLE).with_inner_size([680.0, 720.0]).with_active(true),
        renderer: if wgpu { eframe::Renderer::Wgpu } else { eframe::Renderer::Glow },
        ..Default::default()
    };
    let r = eframe::run_native(
        words::TITLE,
        opts,
        Box::new(|cc| {
            cc.egui_ctx.set_zoom_factor(1.15);
            Ok(Box::new(app))
        }),
    );
    let Err(e) = r else { return };
    if !wgpu {
        log_line(&root, &format!("the window could not open with OpenGL ({}); trying wgpu", e));
        if let Ok(me) = std::env::current_exe() {
            let mut args: Vec<String> = std::env::args().skip(1).collect();
            args.push("--wgpu".into());
            if Command::new(me).args(&args).spawn().is_ok() {
                return;
            }
        }
    }
    log_line(&root, &format!("the window could not open ({}); pointed the person to the programs on the stick", e));
    win::message_box(words::TITLE, &format!("{}\n\n({})", words::NO_WINDOW, e));
    std::process::exit(1);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version") {
        println!("UPGRADE.exe {}", WINDOW_VERSION);
        return;
    }
    let guard: Shared = Arc::new(Mutex::new(Guard::default()));
    let accepting = args.iter().any(|a| a == "--accepting-data-loss");
    if let Some(i) = args.iter().position(|a| a == "--preview") {
        let name = args.get(i + 1).cloned().unwrap_or_default();
        let (screen, flow, ack) = preview(&name);
        let mut app = App::new(screen, "E:".into(), false, ack, None, guard);
        app.flow = flow;
        preview_picker(&mut app, &name);
        return open(app);
    }

    if args.iter().any(|a| a == "--after-restart") {
        // one-shot: the task goes first, whatever happens next
        win::unregister_reopen();
        let Some(state) = std::fs::read(state_dir().join("state.json")).ok().and_then(|b| flow::parse_json(&b)) else { return };
        let (tx, rx) = mpsc::channel();
        let st = state.clone();
        std::thread::spawn(move || after_restart(st, tx));
        let stick = state["stick"].as_str().unwrap_or("").to_string();
        return open(App::new(Screen::Waiting, stick, true, false, Some(rx), guard));
    }

    let developer = args.iter().any(|a| a == "--developer");
    // a local copy of the releases (the rig's server), for the developer only; the table's sha256s still decide
    let from = if developer { args.iter().position(|a| a == "--from").and_then(|i| args.get(i + 1)).filter(|u| u.starts_with("http://") || u.starts_with("https://")).cloned() } else { None };
    if !win::is_elevated() {
        // the one administrator prompt; if it is declined nothing has happened
        let mut pass: Vec<String> = [(accepting, "--accepting-data-loss"), (developer, "--developer")].iter().filter(|(on, _)| *on).map(|(_, a)| a.to_string()).collect();
        if let Some(f) = &from {
            pass.push(format!("--from {f}"));
        }
        win::relaunch_elevated(&pass.join(" "));
        return;
    }
    let exe = std::env::current_exe().unwrap_or_default();
    let s = exe.to_string_lossy().to_string();
    let stick = if s.len() >= 2 && s.as_bytes()[1] == b':' { s[..2].to_uppercase() } else { String::new() };
    let mut app = App::new(Screen::Choose, stick, false, accepting, None, guard);
    app.developer = developer;
    app.from = from;
    open(app);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flows(a: bool, d: bool) -> Vec<&'static str> {
        first_screen(a, d).iter().map(|(_, _, f)| match f { Flow::Verify => "verify", Flow::Convert => "convert", Flow::Erase => "erase", Flow::Rollback => "rollback", Flow::Probe => "probe" }).collect()
    }

    #[test]
    fn verify_and_probe_only_with_the_developer_flag() {
        assert_eq!(flows(false, false), ["convert", "rollback"], "no cancel button since 2026-11-02");
        assert_eq!(flows(false, true), ["verify", "convert", "rollback", "probe"]);
    }

    #[test]
    fn the_data_loss_launcher_shows_only_its_own_convert() {
        assert_eq!(flows(true, false), ["convert"]);
        assert_eq!(flows(true, true), ["convert"], "the developer flag adds nothing to the data-loss screen");
    }
}
