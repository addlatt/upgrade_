//! "Go back to Windows" (architecture.md, "The way back to Windows"): the
//! same window program, opened from the app menu with `--go-back`. Like the
//! summary window it decides nothing and has no words of its own: the core
//! (`settle-in go-back screen`) gives the words, `check` judges the file,
//! `sticks` lists the sticks with every refusal, and the one write runs as
//! `pkexec settle-in go-back write`, which checks everything again as root.

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
    File,
    Stick,
    Done,
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
    page: Page,
    files: Vec<Value>,
    file: Option<String>,
    checked: Option<Value>,
    sticks: Vec<Value>,
    sticks_at: Option<std::time::Instant>,
    stick: Option<usize>,
    typed: String,
    job: Option<Job>,
    note: Option<String>,
}

impl GoBack {
    pub fn new(screen: Value) -> Self {
        GoBack { screen, page: Page::Intro, files: Vec::new(), file: None, checked: None, sticks: Vec::new(), sticks_at: None, stick: None, typed: String::new(), job: None, note: None }
    }

    fn w(&self, p: &str) -> String {
        self.screen.pointer(&format!("/wizard/{}", p)).and_then(Value::as_str).unwrap_or("").to_string()
    }

    fn lines(&self, ui: &mut egui::Ui, p: &str) {
        for l in self.screen.pointer(&format!("/wizard/{}", p)).and_then(Value::as_array).cloned().unwrap_or_default() {
            ui.label(l.as_str().unwrap_or(""));
        }
    }

    fn want(&self) -> String {
        self.screen.pointer("/facts/offer/windows").and_then(Value::as_str).unwrap_or("").to_string()
    }

    fn look_for_files(&mut self) {
        self.files = json_of(&["go-back", "downloads"]).and_then(|v| v.as_array().cloned()).unwrap_or_default();
        if self.file.is_none() {
            self.file = self.files.first().and_then(|f| f["path"].as_str()).map(str::to_string);
        }
    }

    fn refresh_sticks(&mut self) {
        let size = self.checked.as_ref().and_then(|c| c["size"].as_u64()).unwrap_or(0);
        let min = (size + 256 * 1024 * 1024).to_string();
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
            match self.page {
                Page::File => {
                    if d["result"] == "verified" {
                        self.checked = Some(d);
                        self.note = None;
                        self.page = Page::Stick;
                        self.refresh_sticks();
                    } else {
                        self.note = Some(d["why"].as_str().unwrap_or("the file could not be checked").to_string());
                    }
                }
                Page::Stick => self.page = if d["result"] == "written" { Page::Done } else { Page::Failed(d) },
                _ => {}
            }
        }
        let busy = self.job.is_some();
        let mut close = false;
        ui.heading(self.screen["title"].as_str().unwrap_or("Go back to Windows"));
        ui.separator();
        egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
            match &self.page {
                Page::Intro => {
                    for s in self.screen["sections"].as_array().cloned().unwrap_or_default() {
                        ui.add_space(10.0);
                        ui.label(egui::RichText::new(s["heading"].as_str().unwrap_or("")).strong().size(17.0));
                        for l in s["lines"].as_array().cloned().unwrap_or_default() {
                            ui.label(l.as_str().unwrap_or(""));
                        }
                        if let Some(link) = s["link"].as_str()
                            && ui.button("Open Microsoft's download page").clicked() {
                                let _ = Command::new("xdg-open").arg(link).spawn();
                            }
                    }
                }
                Page::File => {
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new(self.w("file/heading")).strong().size(17.0));
                    self.lines(ui, "file/lines");
                    ui.add_space(6.0);
                    if self.files.is_empty() {
                        ui.label(self.w("file/none"));
                    }
                    for f in self.files.clone() {
                        let p = f["path"].as_str().unwrap_or("").to_string();
                        let label = format!("{}  ({:.1} GB)", p, f["size"].as_u64().unwrap_or(0) as f64 / 1e9);
                        ui.add_enabled_ui(!busy, |ui| ui.radio_value(&mut self.file, Some(p.clone()), label));
                    }
                    ui.horizontal(|ui| {
                        if ui.add_enabled(!busy, egui::Button::new(self.w("file/look_again"))).clicked() {
                            self.look_for_files();
                        }
                        if ui.add_enabled(!busy && self.file.is_some(), egui::Button::new(self.w("file/check"))).clicked() {
                            let f = self.file.clone().unwrap_or_default();
                            self.note = None;
                            self.job = Some(spawn(ctx, &core(), vec!["go-back".into(), "check".into(), f, "--want".into(), self.want()]));
                        }
                    });
                    if let Some(j) = &self.job {
                        ui.add(egui::ProgressBar::new(j.progress).show_percentage());
                    }
                    if let Some(n) = &self.note {
                        ui.colored_label(egui::Color32::from_rgb(200, 60, 40), n);
                    }
                }
                Page::Stick => {
                    if !busy && self.sticks_at.map(|t| t.elapsed().as_secs() >= 2).unwrap_or(true) {
                        self.refresh_sticks();
                    }
                    ctx.request_repaint_after(std::time::Duration::from_secs(2));
                    ui.add_space(10.0);
                    if let Some(l) = self.checked.as_ref().and_then(|c| c["line"].as_str()) {
                        ui.label(l);
                    }
                    ui.label(egui::RichText::new(self.w("stick/heading")).strong().size(17.0));
                    self.lines(ui, "stick/lines");
                    ui.add_space(6.0);
                    let offered: Vec<usize> = (0..self.sticks.len()).filter(|i| self.sticks[*i]["offered"] == json!(true)).collect();
                    if offered.is_empty() {
                        ui.label(self.w("stick/none"));
                    }
                    for i in offered {
                        let s = self.sticks[i]["shown_as"].as_str().unwrap_or("").to_string();
                        if ui.add_enabled(!busy, egui::RadioButton::new(self.stick == Some(i), s)).clicked() {
                            self.stick = Some(i);
                            self.typed.clear();
                        }
                    }
                    let refused: Vec<Value> = self.sticks.iter().filter(|s| s["offered"] != json!(true)).cloned().collect();
                    if !refused.is_empty() {
                        egui::CollapsingHeader::new(self.w("stick/refused_heading")).show(ui, |ui| {
                            for r in refused {
                                ui.label(format!("{}: {}", r["shown_as"].as_str().unwrap_or(""),
                                    r["refused_because"].as_array().cloned().unwrap_or_default().iter().filter_map(|x| x.as_str().map(str::to_string)).collect::<Vec<_>>().join("; ")));
                            }
                        });
                    }
                    if let Some(i) = self.stick {
                        let word = self.sticks[i]["confirm_word"].as_str().unwrap_or("").to_string();
                        ui.add_space(8.0);
                        ui.label(format!("{}  {}", self.w("stick/type_prompt"), word));
                        ui.add_enabled(!busy, egui::TextEdit::singleline(&mut self.typed));
                        let ok = !word.is_empty() && self.typed.trim() == word;
                        if ui.add_enabled(ok && !busy, egui::Button::new(self.w("stick/write"))).clicked() {
                            let s = &self.sticks[i];
                            let iso = self.checked.as_ref().and_then(|c| c["path"].as_str()).unwrap_or("").to_string();
                            self.job = Some(spawn(ctx, "pkexec", vec![core(), "go-back".into(), "write".into(), "--iso".into(), iso, "--want".into(), self.want(),
                                "--serial".into(), s["serial"].as_str().unwrap_or("").into(), "--size".into(), s["size_bytes"].to_string(), "--typed".into(), self.typed.trim().to_string()]));
                        }
                        ui.label(egui::RichText::new(self.w("stick/note")).weak());
                    }
                    if let Some(j) = &self.job {
                        ui.label(&j.step);
                        ui.add(egui::ProgressBar::new(j.progress).show_percentage());
                    }
                }
                Page::Done => {
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new(self.w("done/heading")).strong().size(17.0));
                    self.lines(ui, "done/lines");
                }
                Page::Failed(d) => {
                    ui.add_space(10.0);
                    ui.label(format!("{}{}", self.w("failed"), d["why"].as_str().unwrap_or("")));
                    ui.label(if d["stick_changed"] == json!(true) { self.w("failed_changed") } else { self.w("failed_unchanged") });
                }
            }
        });
        ui.add_space(12.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            if ui.add_enabled(!busy, egui::Button::new(self.screen["close"].as_str().unwrap_or("Close"))).clicked() {
                close = true;
            }
            if matches!(self.page, Page::Intro) && !self.want().is_empty() && ui.button(self.screen["next"].as_str().unwrap_or("Next")).clicked() {
                self.page = Page::File;
                self.look_for_files();
            }
        });
        close
    }
}
