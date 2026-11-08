//! "Bring my files from Windows" (decided 2026-11-07, the owner): the same
//! window program, opened with `--pull` (from the first screen's button or
//! the app menu). Like the other pages it decides nothing and has no words
//! of its own: they come from the core.
//!
//! The pages: the plan, or every reason it is refused (`pkexec settle-in
//! pull check`, which opens Windows read-only and closes it again); a
//! field for the BitLocker recovery key when the core says one is needed
//! (it goes to the core on stdin, never on a command line); the copy
//! (`pkexec settle-in pull run`), with its progress; the result. Nothing on
//! Windows changes at any point, so there is no commit line here.

use eframe::egui;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;

enum Msg {
    Progress(Value),
    Done(Value),
}

enum Page {
    Checking,
    Plan,
    Copying,
    Result,
}

pub struct Pull {
    page: Page,
    plan: Value,
    result: Value,
    progress: Value,
    key: String,
    rx: Option<mpsc::Receiver<Msg>>,
}

/// One root step through pkexec, in a thread: `progress` lines as they
/// come, then the JSON answer (everything else it printed).
fn spawn(ctx: &egui::Context, args: Vec<String>, key: Option<String>) -> mpsc::Receiver<Msg> {
    let (tx, rx) = mpsc::channel();
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let mut c = Command::new("pkexec");
        c.arg(crate::core()).args(&args);
        if key.is_some() {
            c.arg("--key-stdin");
        }
        let child = c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn();
        let mut child = match child {
            Ok(ch) => ch,
            Err(e) => {
                let _ = tx.send(Msg::Done(json!({ "result": "stopped", "why": [e.to_string()] })));
                ctx.request_repaint();
                return;
            }
        };
        if let Some(mut i) = child.stdin.take() {
            if let Some(k) = &key {
                let _ = i.write_all(format!("{}\n", k).as_bytes());
            }
        }
        let mut rest = String::new();
        if let Some(out) = child.stdout.take() {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                match line.strip_prefix("progress ").and_then(|p| serde_json::from_str::<Value>(p).ok()) {
                    Some(p) => {
                        let _ = tx.send(Msg::Progress(p));
                        ctx.request_repaint();
                    }
                    None => {
                        rest.push_str(&line);
                        rest.push('\n');
                    }
                }
            }
        }
        let st = child.wait().ok().and_then(|s| s.code());
        let v = serde_json::from_str::<Value>(&rest).unwrap_or_else(|_| {
            json!({ "result": "stopped", "why": [if st == Some(126) { "the password was not given" } else { "the program gave no answer" }] })
        });
        let _ = tx.send(Msg::Done(v));
        ctx.request_repaint();
    });
    rx
}

impl Pull {
    pub fn new(ctx: &egui::Context) -> Self {
        Pull { page: Page::Checking, plan: json!({}), result: json!({}), progress: json!({}), key: String::new(), rx: Some(spawn(ctx, vec!["pull".into(), "check".into()], None)) }
    }

    fn typed_key(&self) -> Option<String> {
        let k = self.key.trim();
        (!k.is_empty()).then(|| k.to_string())
    }

    fn lines(ui: &mut egui::Ui, v: &Value) {
        for l in v.as_array().cloned().unwrap_or_default() {
            ui.label(l.as_str().unwrap_or(""));
        }
    }

    pub fn ui(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) -> bool {
        while let Some(m) = self.rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            match m {
                Msg::Progress(p) => self.progress = p,
                Msg::Done(d) => {
                    self.rx = None;
                    match self.page {
                        Page::Checking => {
                            self.plan = d;
                            self.page = Page::Plan;
                        }
                        _ => {
                            self.result = d;
                            self.page = Page::Result;
                        }
                    }
                }
            }
        }
        let busy = self.rx.is_some();
        let mut close = false;
        ui.heading("Bring my files from Windows");
        ui.separator();
        egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
            ui.add_space(10.0);
            match self.page {
                Page::Checking => {
                    ui.label("Looking at Windows' drive (only reading it)...");
                    ui.add(egui::ProgressBar::new(0.0).animate(true));
                }
                Page::Plan => {
                    let w = self.plan["words"].clone();
                    Self::lines(ui, &w["lines"]);
                    if let Some(field) = w["key_field"].as_str() {
                        ui.add_space(6.0);
                        ui.label(egui::RichText::new(field).strong());
                        ui.add(egui::TextEdit::singleline(&mut self.key).desired_width(460.0));
                    }
                    let skipped: Vec<String> = self.plan["folders"].as_array().into_iter().flatten()
                        .flat_map(|f| f["not_copied"].as_array().cloned().unwrap_or_default().into_iter().map(move |n| format!("{}/{}: {}", f["name"].as_str().unwrap_or(""), n["path"].as_str().unwrap_or(""), n["why"].as_str().unwrap_or(""))))
                        .collect();
                    if !skipped.is_empty() {
                        egui::CollapsingHeader::new(format!("Not copied ({})", skipped.len())).show(ui, |ui| {
                            for s in skipped {
                                ui.label(egui::RichText::new(s).weak());
                            }
                        });
                    }
                }
                Page::Copying => {
                    let (fd, f) = (self.progress["files_done"].as_u64().unwrap_or(0), self.progress["files"].as_u64().unwrap_or(0));
                    let (bd, b) = (self.progress["bytes_done"].as_u64().unwrap_or(0), self.progress["bytes"].as_u64().unwrap_or(0));
                    ui.label(format!("Copying {} ({} of {} files)...", self.progress["folder"].as_str().unwrap_or("your files"), fd, f));
                    ui.add(egui::ProgressBar::new(if b > 0 { bd as f32 / b as f32 } else { 0.0 }).show_percentage());
                    ui.label(egui::RichText::new("Keep the computer plugged in. If it stops, pressing the button again carries on from where it stopped.").weak());
                }
                Page::Result => {
                    let w = self.result["words"].clone();
                    Self::lines(ui, &w["lines"]);
                    let failed = w["failed_list"].as_array().cloned().unwrap_or_default();
                    if !failed.is_empty() {
                        egui::CollapsingHeader::new(format!("Could not be copied ({})", failed.len())).show(ui, |ui| {
                            for s in failed {
                                ui.label(egui::RichText::new(s.as_str().unwrap_or("")).weak());
                            }
                        });
                    }
                }
            }
            if busy {
                ctx.request_repaint_after(std::time::Duration::from_millis(500));
            }
        });
        ui.add_space(12.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            // closing during the copy would leave it running unseen: it carries on, and the button waits
            if ui.add_enabled(!busy, egui::Button::new("Close")).clicked() {
                close = true;
            }
            match self.page {
                Page::Plan => {
                    let w = self.plan["words"].clone();
                    if let Some(b) = w["button"].as_str() {
                        if ui.add_enabled(!busy, egui::Button::new(egui::RichText::new(b).strong())).clicked() {
                            self.progress = json!({});
                            self.rx = Some(spawn(ctx, vec!["pull".into(), "run".into()], self.typed_key()));
                            self.page = Page::Copying;
                        }
                    } else if w["key_field"].is_string() || self.plan["result"] == "refused" {
                        if ui.add_enabled(!busy, egui::Button::new("Check again")).clicked() {
                            self.rx = Some(spawn(ctx, vec!["pull".into(), "check".into()], self.typed_key()));
                            self.page = Page::Checking;
                        }
                    }
                }
                Page::Result if self.result["result"] == "stopped" || self.result["result"] == "done-with-failures" => {
                    if ui.add_enabled(!busy, egui::Button::new("Carry on")).clicked() {
                        self.rx = Some(spawn(ctx, vec!["pull".into(), "run".into()], self.typed_key()));
                        self.page = Page::Copying;
                    }
                }
                _ => {}
            }
        });
        close
    }
}
