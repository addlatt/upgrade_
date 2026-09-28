//! UPGRADE.exe: the window in front of the kit's scripts (decided 2026-09-27,
//! the owner: a Rust window that calls the scripts from the stick). It
//! decides nothing the scripts decide. It runs them with RUN-VERIFY.cmd's
//! arguments, shows their progress in plain words, stops where they stop,
//! and adds two things of its own: a stop on a RED scan before the job
//! writer runs (more cautious, never less), and a one-shot sign-in task
//! that opens it again after the restart to show what came back.
//!
//! This first slice is the verify flow only: nothing is installed and
//! nothing on the internal drive is changed. RUN-VERIFY.cmd stays on the
//! stick as the fallback.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod flow;
mod win;
mod words;

use eframe::egui;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const WINDOW_VERSION: &str = env!("CARGO_PKG_VERSION");

// ---------------------------------------------------------------- the record

/// One line in the stick's convert.log, next to the scripts' own lines, so
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

fn state_dir() -> PathBuf {
    let base = std::env::var_os("ProgramData").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("upgrade_").join("window")
}

fn handoff_state() -> PathBuf {
    let base = std::env::var_os("ProgramData").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("upgrade_").join("v0").join("handoff-state.json")
}

// ---------------------------------------------------------------- the runner

enum Stop {
    Kit(Vec<String>),
    Red(Option<flow::Verdict>),
    Refused(Vec<String>),
}

enum Msg {
    Step(usize),
    Line(String),
    Stop(Stop),
    Arming,
    Armed,
    Result(flow::ResultView),
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

/// Run one powershell.exe call, sending each line as it comes. None if it
/// could not start or the window was closed first.
fn run(args: &[String], root: &Path, tx: &Sender<Msg>, guard: &Shared) -> Option<(i32, Vec<String>)> {
    #[cfg(windows)]
    use std::os::windows::process::CommandExt;
    let mut cmd = Command::new("powershell.exe");
    cmd.args(args).current_dir(root).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
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
                let _ = tx.send(Msg::Line(format!("could not start powershell.exe: {}", e)));
                return Some((1, vec![format!("could not start powershell.exe: {}", e)]));
            }
        };
        let io = (child.stdout.take(), child.stderr.take());
        g.child = Some(child);
        io
    };
    let etx = tx.clone();
    let err_thread = std::thread::spawn(move || {
        let mut v = vec![];
        if let Some(e) = err {
            for l in BufReader::new(e).split(b'\n').map_while(Result::ok) {
                let s = String::from_utf8_lossy(&l).trim_end_matches('\r').to_string();
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
            let _ = tx.send(Msg::Line(s.clone()));
            lines.push(s);
        }
    }
    lines.extend(err_thread.join().unwrap_or_default());
    let child = guard.lock().unwrap().child.take();
    let code = child.and_then(|mut c| c.wait().ok()).and_then(|s| s.code()).unwrap_or(1);
    Some((code, lines))
}

/// RUN-VERIFY.cmd, as a thread. Every stop before step 4 leaves the
/// computer as it was; step 4 is Test-Handoff's own arm, which removes its
/// boot entry again if it cannot finish.
fn verify(stick: String, tx: Sender<Msg>, guard: Shared) {
    let root = PathBuf::from(format!("{}\\", stick));
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
        let Some((code, lines)) = run(&call.args, &root, &tx, &guard) else { return };
        if code != 0 && !call.may_fail {
            log_line(&root, &format!("stopped at step {} (exit {})", call.step, code));
            return stop(Stop::Refused(flow::refusal_lines(&lines)));
        }
        if i == 0 {
            // the window's own stop: RED goes no further (the job writer would refuse it too)
            let report = flow::newest_report(&root.join("upgrade_").join("reports"));
            let v = report.and_then(|p| std::fs::read(p).ok()).and_then(|b| flow::parse_json(&b)).and_then(|j| flow::verdict(&j));
            if !flow::verdict_allows(v.as_ref()) {
                log_line(&root, &format!("stopped after the scan: verdict {}", v.as_ref().map(|v| v.level.as_str()).unwrap_or("missing")));
                return stop(Stop::Red(v));
            }
        }
    }

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
    let res = run(&arm.args, &root, &tx, &guard);
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

enum Screen {
    Welcome,
    Running { step: usize },
    Stopped(Stop),
    Restarting,
    Waiting,
    Result(flow::ResultView),
}

struct App {
    screen: Screen,
    stick: String,
    after: bool,
    arming: bool,
    log: Vec<String>,
    rx: Option<Receiver<Msg>>,
    guard: Shared,
}

impl App {
    fn start(&mut self, ctx: &egui::Context) {
        let (tx, rx) = mpsc::channel();
        let (stick, guard) = (self.stick.clone(), self.guard.clone());
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
        std::thread::spawn(move || verify(stick, ptx, guard));
        self.rx = Some(rx);
        self.screen = Screen::Running { step: 0 };
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
                Msg::Stop(s) => {
                    self.arming = false;
                    self.screen = Screen::Stopped(s);
                }
                Msg::Result(v) => self.screen = Screen::Result(v),
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
            // Invoke-Logged starts the script as its own child: stop the whole tree
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                let _ = Command::new("taskkill.exe").args(["/PID", &c.id().to_string(), "/T", "/F"]).creation_flags(win::CREATE_NO_WINDOW).status();
            }
            let _ = c.kill();
        }
        drop(g);
        if matches!(self.screen, Screen::Running { .. }) {
            log_line(Path::new(&format!("{}\\", self.stick)), "closed by the person before the handoff was armed; nothing was changed");
        }
        if self.after {
            let _ = std::fs::remove_file(state_dir().join("state.json"));
            win::remove_after_exit(&state_dir());
        }
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

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.pump();
        self.on_close(&ctx);
        if self.rx.is_some() {
            ctx.request_repaint_after(Duration::from_millis(300));
        }
        let mut start = false;
        let mut close = false;
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.add_space(6.0);
                match &self.screen {
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
                            start = ui.add(egui::Button::new(egui::RichText::new(words::START).size(17.0))).clicked();
                            close = ui.button(words::CLOSE).clicked();
                        });
                    }
                    Screen::Running { step } => {
                        heading(ui, words::RUNNING_HEADING);
                        for (i, s) in words::STEPS.iter().enumerate() {
                            ui.horizontal(|ui| {
                                if i < *step {
                                    ui.label("✔");
                                    ui.label(*s);
                                } else if i == *step {
                                    ui.spinner();
                                    ui.label(egui::RichText::new(*s).strong());
                                } else {
                                    ui.label("•");
                                    ui.label(egui::RichText::new(*s).weak());
                                }
                            });
                        }
                        ui.add_space(10.0);
                        para(ui, if self.arming { words::RUNNING_ARMING } else { words::RUNNING_CANCEL });
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
                                heading(ui, words::STOPPED_HEADING);
                                for l in lines {
                                    para(ui, l);
                                }
                                ui.add_space(6.0);
                                para(ui, words::NOTHING_CHANGED);
                            }
                        }
                        para(ui, words::LOG_WHERE);
                        details(ui, &self.log);
                        ui.add_space(10.0);
                        close = ui.button(words::CLOSE).clicked();
                    }
                    Screen::Restarting => {
                        heading(ui, words::RESTARTING_HEADING);
                        for l in words::RESTARTING_LINES {
                            para(ui, l);
                        }
                        details(ui, &self.log);
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
                        close = ui.button(words::CLOSE).clicked();
                    }
                }
            });
        });
        if start {
            log_line(Path::new(&format!("{}\\", self.stick)), "the person pressed Start the test");
            self.start(&ctx);
        }
        if close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

// ---------------------------------------------------------------- start

/// Screens with made-up content, to look at the words without running
/// anything (UPGRADE.exe --preview <name>). Nothing is run or written.
fn preview(name: &str) -> Screen {
    let v = json!({"identity":{"result":"pass"},"hardware":{"display":"pass","wifi":"pass","wifi_detail":"wlp1s0 sees 28 networks","audio_firmware":"skipped"},"payload":{"result":"pass","read_mbps":"22.6"}});
    match name {
        "running" => Screen::Running { step: 2 },
        "kit" => Screen::Stopped(Stop::Kit(vec!["upgrade_\\LiveOS\\kde.squashfs".into()])),
        "red" => Screen::Stopped(Stop::Red(Some(flow::Verdict {
            level: "RED".into(),
            summary: "Do not convert this machine as it stands. Something here blocks the install outright - resolve it, or use a different machine.".into(),
            groups: vec![("Blocks the install".into(), vec!["Drive health: the SSD reports 725 bad blocks (SMART 187)".into()])],
        }))),
        "stopped" => Screen::Stopped(Stop::Refused(vec!["BitLocker state is unknown".into()])),
        "restarting" => Screen::Restarting,
        "waiting" => Screen::Waiting,
        "result" => Screen::Result(flow::result_view(true, Some(&v), None, Some("fired-once"))),
        "result-refused" => Screen::Result(flow::result_view(true, None, Some(&json!({"reason":"the copy of Linux on this USB stick is damaged"})), Some("fired-once"))),
        "no-linux" => Screen::Result(flow::result_view(true, None, None, Some("ignored"))),
        _ => Screen::Welcome,
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
    let root = PathBuf::from(format!("{}\\", app.stick));
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title(words::TITLE).with_inner_size([640.0, 620.0]).with_active(true),
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
    if let Some(i) = args.iter().position(|a| a == "--preview") {
        let name = args.get(i + 1).cloned().unwrap_or_default();
        return open(App { screen: preview(&name), stick: "E:".into(), after: false, arming: false, log: vec![], rx: None, guard });
    }

    if args.iter().any(|a| a == "--after-restart") {
        // one-shot: the task goes first, whatever happens next
        win::unregister_reopen();
        let Some(state) = std::fs::read(state_dir().join("state.json")).ok().and_then(|b| flow::parse_json(&b)) else { return };
        let (tx, rx) = mpsc::channel();
        let st = state.clone();
        std::thread::spawn(move || after_restart(st, tx));
        let stick = state["stick"].as_str().unwrap_or("").to_string();
        return open(App { screen: Screen::Waiting, stick, after: true, arming: false, log: vec![], rx: Some(rx), guard });
    }

    if !win::is_elevated() {
        // the one administrator prompt; if it is declined nothing has happened
        win::relaunch_elevated("");
        return;
    }
    let exe = std::env::current_exe().unwrap_or_default();
    let s = exe.to_string_lossy().to_string();
    let stick = if s.len() >= 2 && s.as_bytes()[1] == b':' { s[..2].to_uppercase() } else { String::new() };
    open(App { screen: Screen::Welcome, stick, after: false, arming: false, log: vec![], rx: None, guard });
}
