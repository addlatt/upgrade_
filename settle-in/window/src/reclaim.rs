//! "Free up Windows' space" (RISKS R41): the same window program, opened
//! from the app menu with `--reclaim`. Like the other pages it decides
//! nothing and has no words of its own: they come from the core.
//!
//! The pages: what reclaim does (`settle-in reclaim words`, no root); what
//! this computer would lose, or every reason it is refused (`pkexec
//! settle-in reclaim check`, which reads the disk and Windows read-only);
//! the typed sentence; then the last page, whose one button is the commit
//! line (rule #3, amended 2026-11-02): "Restart now" runs `pkexec settle-in
//! reclaim arm`, which checks everything again, writes the order onto the
//! kit stick and sets a one-time start from it, and the computer restarts at
//! once. The arm is part of that click on purpose: an armed computer that
//! the person had not yet clicked for could delete Windows at any later
//! restart. Nothing on the internal disk changes before the click.

use eframe::egui;
use serde_json::{json, Value};
use std::process::Command;
use std::sync::mpsc;

fn core() -> String {
    crate::core()
}

enum Page {
    Intro,
    Checking,
    What,
    Consent,
    Arming,
    Ready,
    Restarting,
    Undone,
    Failed(Value),
}

pub struct Reclaim {
    words: Value,
    plan: Value,
    page: Page,
    typed: String,
    rx: Option<mpsc::Receiver<Value>>,
}

/// One root step through pkexec, in a thread; its JSON answer.
fn spawn(ctx: &egui::Context, args: Vec<String>) -> mpsc::Receiver<Value> {
    let (tx, rx) = mpsc::channel();
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let v = match Command::new("pkexec").arg(core()).args(&args).output() {
            Ok(o) => serde_json::from_slice::<Value>(&o.stdout).unwrap_or_else(|_| {
                json!({ "result": "stopped", "why": if o.status.code() == Some(126) { "the password was not given" } else { "the program gave no answer" } })
            }),
            Err(e) => json!({ "result": "stopped", "why": e.to_string() }),
        };
        let _ = tx.send(v);
        ctx.request_repaint();
    });
    rx
}

fn why_text(d: &Value) -> String {
    match &d["why"] {
        Value::Array(a) => a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" "),
        v => v.as_str().unwrap_or("").to_string(),
    }
}

impl Reclaim {
    pub fn new(words: Value) -> Self {
        Reclaim { words, plan: json!({}), page: Page::Intro, typed: String::new(), rx: None }
    }

    fn w(&self, p: &str) -> String {
        self.words.pointer(p).and_then(Value::as_str).unwrap_or("").to_string()
    }

    fn lines(&self, ui: &mut egui::Ui, from: &Value, p: &str) {
        for l in from.pointer(p).and_then(Value::as_array).cloned().unwrap_or_default() {
            ui.label(l.as_str().unwrap_or(""));
        }
    }

    pub fn ui(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) -> bool {
        if let Some(rx) = &self.rx
            && let Ok(d) = rx.try_recv() {
                self.rx = None;
                self.page = match (&self.page, d["result"].as_str()) {
                    (Page::Checking, Some("plan")) => {
                        self.plan = d;
                        Page::What
                    }
                    (Page::Arming, Some("ready")) => {
                        // the restart must really start; if it does not, the arm is taken back
                        match Command::new("systemctl").arg("reboot").status() {
                            Ok(st) if st.success() => Page::Restarting,
                            _ => {
                                self.rx = Some(spawn(ctx, vec!["reclaim".into(), "disarm".into()]));
                                Page::Failed(json!({ "result": "stopped", "why": "the computer did not restart, so nothing will happen: the reclaim was taken back" }))
                            }
                        }
                    }
                    (Page::Arming, _) => {
                        // an arm whose answer is lost may still have armed: take it back, it is harmless if not
                        let r = Command::new("pkexec").arg(core()).args(["reclaim", "disarm"]).output();
                        let _ = r;
                        Page::Failed(d)
                    }
                    (Page::Restarting, Some("undone")) => Page::Undone,
                    _ => Page::Failed(d),
                };
            }
        let busy = self.rx.is_some();
        let mut close = false;
        ui.heading(self.w("/title"));
        ui.separator();
        let words = self.words.clone();
        let plan_words = self.plan["words"].clone();
        egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
            ui.add_space(10.0);
            match &self.page {
                Page::Intro => {
                    ui.label(egui::RichText::new(self.w("/intro/heading")).strong().size(17.0));
                    self.lines(ui, &words, "/intro/lines");
                    ui.add_space(6.0);
                    ui.label(self.w("/stick"));
                }
                Page::Checking | Page::Arming => {
                    ui.label(self.w("/preparing"));
                    ui.add(egui::ProgressBar::new(0.0).animate(true));
                    ctx.request_repaint_after(std::time::Duration::from_millis(500));
                }
                Page::What => {
                    ui.label(egui::RichText::new(plan_words.pointer("/what/heading").and_then(Value::as_str).unwrap_or("")).strong().size(17.0));
                    ui.label(plan_words.pointer("/what/folders_heading").and_then(Value::as_str).unwrap_or(""));
                    for f in plan_words.pointer("/what/folders").and_then(Value::as_array).cloned().unwrap_or_default() {
                        ui.label(egui::RichText::new(format!("    {}", f.as_str().unwrap_or(""))).strong());
                    }
                }
                Page::Consent => {
                    ui.label(egui::RichText::new(self.w("/consent/heading")).strong().size(17.0));
                    ui.label(egui::RichText::new(self.w("/consent/sentence")).strong());
                    ui.add(egui::TextEdit::multiline(&mut self.typed).desired_rows(2));
                    ui.label(egui::RichText::new(self.w("/consent/note")).weak());
                }
                Page::Ready => {
                    ui.label(egui::RichText::new(self.w("/ready/heading")).strong().size(17.0));
                    self.lines(ui, &words, "/ready/lines");
                }
                Page::Restarting => {
                    ui.label(self.w("/restarting"));
                }
                Page::Undone => {
                    ui.label(self.w("/ready/undone"));
                }
                Page::Failed(d) => {
                    let lead = if d["result"] == "refused" { self.w("/refused") } else { self.w("/failed") };
                    ui.label(format!("{}{}", lead, why_text(d)));
                }
            }
        });
        ui.add_space(12.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            // while the restart is under way, closing would leave it armed: the button is Take it back
            if ui.add_enabled(!busy && !matches!(self.page, Page::Restarting), egui::Button::new(self.w("/close"))).clicked() {
                close = true;
            }
            let next = self.w("/next");
            match self.page {
                Page::Intro => {
                    if ui.button(&next).clicked() {
                        self.rx = Some(spawn(ctx, vec!["reclaim".into(), "check".into()]));
                        self.page = Page::Checking;
                    }
                }
                Page::What => {
                    if ui.button(&next).clicked() {
                        self.typed.clear();
                        self.page = Page::Consent;
                    }
                }
                Page::Consent => {
                    let exact = self.typed.trim() == self.w("/consent/sentence");
                    if ui.add_enabled(exact, egui::Button::new(self.w("/consent/button"))).clicked() {
                        self.page = Page::Ready;
                    }
                }
                Page::Ready => {
                    // the commit line: the last click (rule #3). It arms, then restarts.
                    if ui.add_enabled(!busy, egui::Button::new(egui::RichText::new(self.w("/ready/restart")).strong())).clicked() {
                        self.rx = Some(spawn(ctx, vec!["reclaim".into(), "arm".into(), "--typed".into(), self.typed.trim().to_string()]));
                        self.page = Page::Arming;
                    }
                }
                Page::Restarting => {
                    // only if the restart did not happen: take the arm back
                    if ui.add_enabled(!busy, egui::Button::new(self.w("/ready/undo"))).clicked() {
                        self.rx = Some(spawn(ctx, vec!["reclaim".into(), "disarm".into()]));
                    }
                }
                _ => {}
            }
        });
        close
    }
}
