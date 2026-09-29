//! "Go back to Windows" (architecture.md, "The way back to Windows"): the
//! same window program, opened from the app menu with `--go-back`. Like the
//! summary window it decides nothing and has no words of its own: the core
//! (`settle-in go-back screen`) gives the words, `check` judges the file,
//! `sticks` lists the sticks with every refusal, and the one write runs as
//! `pkexec settle-in go-back write`, which checks everything again as root.
//!
//! Decided 2026-09-29 (the owner): the way back is 100% managed (RISKS R33).
//! The pages are now: the cost, what happens (both drives by name), the stick,
//! the typed sentence, then `pkexec settle-in go-back walkaway prepare`, which
//! downloads Windows from Microsoft's catalog, writes the stick and sets a
//! one-time start from it. Then "Restart now". The countdown that is the
//! commit line runs on the stick, after the restart (settle-in/gate).

use eframe::egui;
use serde_json::{json, Value};
use std::io::BufRead;
use std::process::{Command, Stdio};
use std::sync::mpsc;

fn core() -> String {
    crate::core()
}

fn json_of(args: &[&str]) -> Option<Value> {
    let o = Command::new(core()).args(args).output().ok()?;
    serde_json::from_slice(&o.stdout).ok()
}

enum Page {
    Intro,
    What,
    Stick,
    Consent,
    Preparing,
    Ready,
    Undone,
    Failed(Value),
}

/// A long job in a thread: its stderr progress lines and its final JSON.
struct Job {
    rx: mpsc::Receiver<Value>,
    progress: f32,
    step: String,
}

fn spawn(ctx: &egui::Context, prog: &str, args: Vec<String>) -> Job {
    let (tx, rx) = mpsc::channel();
    let ctx = ctx.clone();
    let prog = prog.to_string();
    std::thread::spawn(move || {
        let child = Command::new(&prog).args(&args).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(e) => {
                let _ = tx.send(json!({ "done": { "result": "stopped", "why": e.to_string(), "stick_changed": false } }));
                ctx.request_repaint();
                return;
            }
        };
        if let Some(err) = child.stderr.take() {
            for l in std::io::BufReader::new(err).lines().map_while(Result::ok) {
                if let Ok(v) = serde_json::from_str::<Value>(&l) {
                    let _ = tx.send(json!({ "line": v }));
                    ctx.request_repaint();
                }
            }
        }
        let out = child.wait_with_output();
        let v = match out {
            Ok(o) => serde_json::from_slice::<Value>(&o.stdout).unwrap_or_else(|_| {
                json!({ "result": "stopped", "stick_changed": false,
                    "why": if o.status.code() == Some(126) { "the password was not given".to_string() } else { "the program gave no answer".to_string() } })
            }),
            Err(e) => json!({ "result": "stopped", "why": e.to_string(), "stick_changed": false }),
        };
        let _ = tx.send(json!({ "done": v }));
        ctx.request_repaint();
    });
    Job { rx, progress: 0.0, step: String::new() }
}

pub struct GoBack {
    screen: Value,
    plan: Value,
    page: Page,
    sentence: String,
    sticks: Vec<Value>,
    sticks_at: Option<std::time::Instant>,
    stick: Option<usize>,
    typed: String,
    job: Option<Job>,
}

impl GoBack {
    pub fn new(screen: Value) -> Self {
        let plan = json_of(&["go-back", "walkaway", "plan"]).unwrap_or(json!({}));
        GoBack { screen, plan, page: Page::Intro, sentence: String::new(), sticks: Vec::new(), sticks_at: None, stick: None, typed: String::new(), job: None }
    }

    fn w(&self, p: &str) -> String {
        self.plan.pointer(&format!("/words/{}", p)).and_then(Value::as_str).unwrap_or("").to_string()
    }

    fn lines(&self, ui: &mut egui::Ui, p: &str) {
        for l in self.plan.pointer(&format!("/words/{}", p)).and_then(Value::as_array).cloned().unwrap_or_default() {
            ui.label(l.as_str().unwrap_or(""));
        }
    }

    fn drives_ok(&self) -> bool {
        self.plan["drives"].as_array().map(|a| !a.is_empty()).unwrap_or(false)
    }

    fn refresh_sticks(&mut self) {
        // Windows 11's files take about 7 GB on the stick: 16 GB sticks (the words say so)
        let min = 14_000_000_000u64.to_string();
        self.sticks = json_of(&["go-back", "sticks", "--min-bytes", &min]).and_then(|v| v.as_array().cloned()).unwrap_or_default();
        self.sticks_at = Some(std::time::Instant::now());
        // a pick that is no longer offered is dropped (unplugged, or it changed)
        if let Some(i) = self.stick
            && self.sticks.get(i).map(|s| s["offered"] != json!(true)).unwrap_or(true) {
                self.stick = None;
                self.typed.clear();
            }
    }

    pub fn ui(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) -> bool {
        // a running job: take what it said
        let mut finished = None;
        if let Some(job) = &mut self.job {
            while let Ok(m) = job.rx.try_recv() {
                if let Some(l) = m.get("line") {
                    if let (Some(d), Some(t)) = (l["progress"].as_u64(), l["total"].as_u64())
                        && t > 0 {
                            job.progress = d as f32 / t as f32;
                        }
                    if let Some(s) = l["step"].as_str() {
                        job.step = s.to_string();
                    }
                }
                if let Some(d) = m.get("done") {
                    finished = Some(d.clone());
                }
            }
        }
        if let Some(d) = finished {
            self.job = None;
            self.page = match (&self.page, d["result"].as_str()) {
                (Page::Preparing, Some("ready")) => Page::Ready,
                (Page::Ready, Some("undone")) => Page::Undone,
                _ => Page::Failed(d),
            };
        }
        let busy = self.job.is_some();
        let mut close = false;
        ui.heading(self.screen["title"].as_str().unwrap_or("Go back to Windows"));
        ui.separator();
        egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
            match &self.page {
                Page::Intro => {
                    // the cost first; the guided download step is not part of the managed way
                    for sec in self.screen["sections"].as_array().cloned().unwrap_or_default() {
                        if sec.get("link").is_some() {
                            continue;
                        }
                        ui.add_space(10.0);
                        ui.label(egui::RichText::new(sec["heading"].as_str().unwrap_or("")).strong().size(17.0));
                        for l in sec["lines"].as_array().cloned().unwrap_or_default() {
                            ui.label(l.as_str().unwrap_or(""));
                        }
                    }
                }
                Page::What => {
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new(self.w("what/heading")).strong().size(17.0));
                    self.lines(ui, "what/lines");
                    for d in self.plan.pointer("/words/what/drives").and_then(Value::as_array).cloned().unwrap_or_default() {
                        ui.label(egui::RichText::new(format!("    {}", d.as_str().unwrap_or(""))).strong());
                    }
                    ui.label(self.w("what/after"));
                }
                Page::Stick => {
                    if self.sticks_at.map(|t| t.elapsed().as_secs() >= 2).unwrap_or(true) {
                        self.refresh_sticks();
                    }
                    ctx.request_repaint_after(std::time::Duration::from_secs(2));
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new(self.w("stick/heading")).strong().size(17.0));
                    self.lines(ui, "stick/lines");
                    ui.add_space(6.0);
                    let offered: Vec<usize> = (0..self.sticks.len()).filter(|i| self.sticks[*i]["offered"] == json!(true)).collect();
                    if offered.is_empty() {
                        ui.label(self.screen.pointer("/wizard/stick/none").and_then(Value::as_str).unwrap_or(""));
                    }
                    for i in offered {
                        let s = self.sticks[i]["shown_as"].as_str().unwrap_or("").to_string();
                        if ui.add(egui::RadioButton::new(self.stick == Some(i), s)).clicked() {
                            self.stick = Some(i);
                            self.typed.clear();
                        }
                    }
                    let refused: Vec<Value> = self.sticks.iter().filter(|s| s["offered"] != json!(true)).cloned().collect();
                    if !refused.is_empty() {
                        egui::CollapsingHeader::new(self.screen.pointer("/wizard/stick/refused_heading").and_then(Value::as_str).unwrap_or("Not offered:")).show(ui, |ui| {
                            for r in refused {
                                ui.label(format!("{}: {}", r["shown_as"].as_str().unwrap_or(""),
                                    r["refused_because"].as_array().cloned().unwrap_or_default().iter().filter_map(|x| x.as_str().map(str::to_string)).collect::<Vec<_>>().join("; ")));
                            }
                        });
                    }
                    if let Some(i) = self.stick {
                        let word = self.sticks[i]["confirm_word"].as_str().unwrap_or("").to_string();
                        ui.add_space(8.0);
                        ui.label(format!("{}  {}", self.screen.pointer("/wizard/stick/type_prompt").and_then(Value::as_str).unwrap_or(""), word));
                        ui.add(egui::TextEdit::singleline(&mut self.typed));
                    }
                }
                Page::Consent => {
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new(self.w("consent/heading")).strong().size(17.0));
                    self.lines(ui, "consent/lines");
                    ui.label(egui::RichText::new(self.w("consent/sentence")).strong());
                    ui.add(egui::TextEdit::multiline(&mut self.sentence).desired_rows(2));
                    ui.label(egui::RichText::new(self.w("consent/note")).weak());
                }
                Page::Preparing => {
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new(self.w("preparing/heading")).strong().size(17.0));
                    if let Some(j) = &self.job {
                        let name = self.plan.pointer(&format!("/words/preparing/steps/{}", j.step)).and_then(Value::as_str).unwrap_or(&j.step).to_string();
                        ui.label(name);
                        ui.add(egui::ProgressBar::new(j.progress).show_percentage());
                    }
                }
                Page::Ready => {
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new(self.w("ready/heading")).strong().size(17.0));
                    self.lines(ui, "ready/lines");
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.add_enabled(!busy, egui::Button::new(egui::RichText::new(self.w("ready/restart")).strong())).clicked() {
                            let _ = Command::new("systemctl").arg("reboot").spawn();
                        }
                        if ui.add_enabled(!busy, egui::Button::new(self.w("ready/undo"))).clicked() {
                            self.job = Some(spawn(ctx, "pkexec", vec![core(), "go-back".into(), "walkaway".into(), "undo".into()]));
                        }
                    });
                }
                Page::Undone => {
                    ui.add_space(10.0);
                    ui.label(self.w("ready/undone"));
                }
                Page::Failed(d) => {
                    ui.add_space(10.0);
                    ui.label(format!("{}{}", self.w("failed"), d["why"].as_str().unwrap_or("")));
                    if d["computer_changed"] != json!(true) {
                        ui.label(self.w("failed_unchanged"));
                    }
                }
            }
        });
        ui.add_space(12.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            let closable = !busy && !matches!(self.page, Page::Preparing);
            if ui.add_enabled(closable, egui::Button::new(self.screen["close"].as_str().unwrap_or("Close"))).clicked() {
                close = true;
            }
            let next = self.screen["next_plain"].as_str().unwrap_or("Next");
            match self.page {
                Page::Intro if self.drives_ok() => {
                    if ui.button(next).clicked() {
                        self.page = Page::What;
                    }
                }
                Page::What => {
                    if ui.button(next).clicked() {
                        self.page = Page::Stick;
                        self.refresh_sticks();
                    }
                }
                Page::Stick => {
                    let word = self.stick.and_then(|i| self.sticks.get(i)).and_then(|s| s["confirm_word"].as_str()).unwrap_or("").to_string();
                    if ui.add_enabled(!word.is_empty() && self.typed.trim() == word, egui::Button::new(next)).clicked() {
                        self.page = Page::Consent;
                    }
                }
                Page::Consent => {
                    let exact = self.sentence.trim() == self.w("consent/sentence");
                    if ui.add_enabled(exact, egui::Button::new(self.w("consent/button"))).clicked() {
                        let s = self.stick.and_then(|i| self.sticks.get(i)).cloned().unwrap_or(json!({}));
                        let account = std::env::var("USER").unwrap_or_default();
                        self.job = Some(spawn(ctx, "pkexec", vec![core(), "go-back".into(), "walkaway".into(), "prepare".into(),
                            "--sentence".into(), self.sentence.trim().to_string(), "--account".into(), account,
                            "--serial".into(), s["serial"].as_str().unwrap_or("").into(), "--size".into(), s["size_bytes"].to_string(),
                            "--typed".into(), self.typed.trim().to_string(), "--language".into(), self.plan["language"].as_str().unwrap_or("en-us").into()]));
                        self.page = Page::Preparing;
                    }
                }
                _ => {}
            }
        });
        close
    }
}
