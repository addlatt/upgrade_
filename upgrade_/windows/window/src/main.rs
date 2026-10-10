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
//! (RUN-ERASE-AND-INSTALL.cmd), roll back (ROLLBACK.cmd), the walk-away
//! probe (RUN-PROBE.cmd) and cancel (CANCEL-CONVERSION.cmd). The data-loss
//! variants of convert and erase are reached only by starting the window
//! with --accepting-data-loss, its own launcher, as rule #1 asks. The .cmd
//! launchers stay on the stick as the fallback.
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

/// ROLLBACK.cmd, RUN-PROBE.cmd and CANCEL-CONVERSION.cmd are one call each;
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
    Cancel,
}

enum Screen {
    Choose,
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
    stick: String,
    after: bool,
    arming: bool,
    flow: Flow,
    /// started with --accepting-data-loss: the convert flow asks the sentence (RISKS R23)
    accepting_data_loss: bool,
    setup: Setup,
    word: String,
    word_tx: Option<Sender<String>>,
    log: Vec<String>,
    rx: Option<Receiver<Msg>>,
    guard: Shared,
}

impl App {
    fn new(screen: Screen, stick: String, after: bool, accepting_data_loss: bool, rx: Option<Receiver<Msg>>, guard: Shared) -> App {
        App { screen, stick, after, arming: false, flow: Flow::Verify, accepting_data_loss, setup: Setup::default(), word: String::new(), word_tx: None, log: vec![], rx, guard }
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

    fn start_convert(&mut self, ctx: &egui::Context) {
        let ptx = self.channel(ctx);
        let (stick, guard) = (self.stick.clone(), self.guard.clone());
        let (wtx, wrx) = mpsc::channel::<String>();
        self.word_tx = Some(wtx);
        let inputs = ConvertInputs {
            desktop: self.setup.desktop.unwrap_or(Desktop::Kde),
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
                Msg::Line(l) => self.log.push(l),
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
                        (Flow::Rollback, 0) => Screen::Done(words::ROLLBACK_DONE_HEADING.into(), words::ROLLBACK_DONE_LINE.into()),
                        (Flow::Probe, 0) => Screen::Done(words::PROBE_RESTARTING_HEADING.into(), words::PROBE_RESTARTING_LINE.into()),
                        (Flow::Cancel, 0) => Screen::Done(words::CANCEL_DONE_HEADING.into(), words::CANCEL_DONE_LINE.into()),
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
        if matches!(self.screen, Screen::Running { .. } | Screen::Decide { .. }) {
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

fn banner(ui: &mut egui::Ui) {
    ui.label(egui::RichText::new(words::ACK_BANNER).strong().color(egui::Color32::from_rgb(180, 30, 30)).size(18.0));
    ui.add_space(6.0);
}

enum Press {
    None,
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
                        if !self.accepting_data_loss {
                            choice(ui, words::CHOOSE_VERIFY, words::CHOOSE_VERIFY_LINE, Flow::Verify);
                            choice(ui, words::CHOOSE_CONVERT, words::CHOOSE_CONVERT_LINE, Flow::Convert);
                            choice(ui, words::CHOOSE_ERASE, words::CHOOSE_ERASE_LINE, Flow::Erase);
                            choice(ui, words::CHOOSE_ROLLBACK, words::CHOOSE_ROLLBACK_LINE, Flow::Rollback);
                            choice(ui, words::CHOOSE_PROBE, words::CHOOSE_PROBE_LINE, Flow::Probe);
                            choice(ui, words::CHOOSE_CANCEL, words::CHOOSE_CANCEL_LINE, Flow::Cancel);
                        } else {
                            choice(ui, words::CHOOSE_CONVERT_ACK, words::CHOOSE_CONVERT_ACK_LINE, Flow::Convert);
                            choice(ui, words::CHOOSE_ERASE_ACK, words::CHOOSE_CONVERT_ACK_LINE, Flow::Erase);
                        }
                        ui.add_space(12.0);
                        if ui.button(words::CLOSE).clicked() {
                            press = Press::Close;
                        }
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
                        ui.label(egui::RichText::new(words::DESKTOP_HEADING).strong().size(17.0));
                        ui.radio_value(&mut self.setup.desktop, Some(Desktop::Kde), words::DESKTOP_KDE);
                        para(ui, words::DESKTOP_KDE_LINE);
                        ui.radio_value(&mut self.setup.desktop, Some(Desktop::Gnome), words::DESKTOP_GNOME);
                        para(ui, words::DESKTOP_GNOME_LINE);
                        ui.radio_value(&mut self.setup.desktop, Some(Desktop::Console), words::DESKTOP_CONSOLE);
                        para(ui, words::DESKTOP_CONSOLE_LINE);
                        para(ui, words::DESKTOP_UNSURE);
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
                            Flow::Rollback => (words::ROLLBACK_RUNNING, &[]),
                            Flow::Probe => (words::PROBE_RUNNING, &[]),
                            Flow::Cancel => (words::CANCEL_RUNNING, &[]),
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
                                        Flow::Rollback => words::ROLLBACK_FAILED_HEADING,
                                        Flow::Cancel => words::CANCEL_FAILED_HEADING,
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
                        let (h, lines, button): (&str, &[&str], &str) = match self.flow {
                            Flow::Rollback => (words::ROLLBACK_HEADING, &words::ROLLBACK_LINES, words::CONVERT_CONTINUE),
                            Flow::Probe => (words::PROBE_HEADING, &words::PROBE_LINES, words::PROBE_GO),
                            _ => (words::CANCEL_HEADING, &words::CANCEL_LINES, words::CANCEL_GO),
                        };
                        heading(ui, h);
                        for l in lines {
                            para(ui, l);
                        }
                        ui.add_space(8.0);
                        if self.flow == Flow::Rollback {
                            ui.label(egui::RichText::new(words::ROLLBACK_TYPE).strong());
                            ui.add(egui::TextEdit::singleline(&mut self.word).desired_width(200.0));
                            ui.add_space(8.0);
                        }
                        ui.horizontal(|ui| {
                            if ui.add(egui::Button::new(egui::RichText::new(button).size(17.0))).clicked() {
                                press = if self.flow == Flow::Rollback { Press::Word } else { Press::Go };
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
                Flow::Convert | Flow::Erase => Screen::ConvertSetup,
                Flow::Rollback if !flow::rollback_snapshot_present(&stick_root(&self.stick)) => Screen::Stopped(Stop::Refused(vec![words::ROLLBACK_NO_SNAPSHOT.into()])),
                Flow::Rollback | Flow::Probe | Flow::Cancel => Screen::Ask,
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
            Press::Word if self.flow == Flow::Rollback => {
                let root = stick_root(&self.stick);
                if flow::rollback_ok(&self.word) {
                    log_line(&root, "the rollback word was typed");
                    let c = flow::rollback_call(&self.stick);
                    self.start_one(&ctx, c, false);
                } else {
                    log_line(&root, "the rollback word was not typed; stopped");
                    self.screen = Screen::Stopped(Stop::Refused(vec![words::DECIDE_NOT_CONFIRMED.into()]));
                }
            }
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
                Flow::Cancel => {
                    let c = flow::abort_call(&self.stick);
                    self.start_one(&ctx, c, false);
                }
                _ => {}
            },
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
        "convert" => (Screen::ConvertSetup, Flow::Convert, false),
        "convert-ack" => (Screen::ConvertSetup, Flow::Convert, true),
        "convert-running" => (Screen::Running { step: 2 }, Flow::Convert, false),
        "decide" => (Screen::Decide { linux_name: "ann".into() }, Flow::Convert, false),
        "convert-restarting" => (Screen::ConvertRestarting, Flow::Convert, false),
        "erase" => (Screen::ConvertSetup, Flow::Erase, false),
        "erase-decide" => (Screen::Decide { linux_name: "ann".into() }, Flow::Erase, false),
        "erase-restarting" => (Screen::ConvertRestarting, Flow::Erase, false),
        "rollback" => (Screen::Ask, Flow::Rollback, false),
        "rollback-done" => (Screen::Done(words::ROLLBACK_DONE_HEADING.into(), words::ROLLBACK_DONE_LINE.into()), Flow::Rollback, false),
        "probe" => (Screen::Ask, Flow::Probe, false),
        "cancel" => (Screen::Ask, Flow::Cancel, false),
        "convert-stopped" => (Screen::Stopped(Stop::Prologue(vec!["STOPPED at revalidate: job.json no longer matches this machine: volume_health.repair_queued: job says 'False', machine says 'True'".into(), "outcome.json (stopped) written to the stick".into()])), Flow::Convert, false),
        _ => (Screen::Welcome, Flow::Verify, false),
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
    log_line(&root, &format!("the window could not open ({}); pointed the person to RUN-VERIFY.cmd", e));
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

    if !win::is_elevated() {
        // the one administrator prompt; if it is declined nothing has happened
        win::relaunch_elevated(if accepting { "--accepting-data-loss" } else { "" });
        return;
    }
    let exe = std::env::current_exe().unwrap_or_default();
    let s = exe.to_string_lossy().to_string();
    let stick = if s.len() >= 2 && s.as_bytes()[1] == b':' { s[..2].to_uppercase() } else { String::new() };
    open(App::new(Screen::Choose, stick, false, accepting, None, guard));
}
