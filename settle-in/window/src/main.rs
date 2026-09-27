//! settle-in's window (decided 2026-09-27, the owner: egui). It opens once,
//! at the person's first sign-in (/etc/xdg/autostart), and shows what the
//! first startup did. It decides nothing and holds no words of its own: it
//! draws the sections `settle-in summary` gives it (the owner's approved
//! text lives there), and its one button runs
//! `pkexec settle-in remove-old-boot-entry`, which asks for the person's
//! password and re-checks everything as root before it touches the firmware.
//!
//! This is a separate program from the static core because a static
//! program cannot load the system's display libraries; it links them the
//! ordinary way (architecture.md, "It runs on any Linux").

use eframe::egui;
use serde_json::Value;
use std::process::Command;
use std::sync::mpsc;

fn core() -> String {
    std::env::var("SETTLE_IN_BIN").unwrap_or_else(|_| "/usr/local/libexec/upgrade_/settle-in".to_string())
}

fn marker() -> std::path::PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join(".local/state")))
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
    base.join("upgrade_/settle-in-shown") // one mark for the window and the console: shown once per person
}

fn sections() -> Option<Value> {
    let out = Command::new(core()).arg("summary").output().ok()?;
    if !out.status.success() {
        return None;
    }
    serde_json::from_slice(&out.stdout).ok()
}

struct App {
    screen: Value,
    running: Option<mpsc::Receiver<Value>>,
    last: Option<String>,
}

impl App {
    fn press(&mut self, ctx: &egui::Context) {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let r = Command::new("pkexec").arg(core()).arg("remove-old-boot-entry").output();
            let v = match r {
                Ok(o) => serde_json::from_slice::<Value>(&o.stdout)
                    .unwrap_or_else(|_| serde_json::json!({ "result": "failed", "why": if o.status.code() == Some(126) { "the password was not given".to_string() } else { String::from_utf8_lossy(&o.stderr).trim().to_string() } })),
                Err(e) => serde_json::json!({ "result": "failed", "why": e.to_string() }),
            };
            let _ = tx.send(v);
            ctx.request_repaint();
        });
        self.running = Some(rx);
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if let Some(rx) = &self.running
            && let Ok(v) = rx.try_recv() {
                self.running = None;
                // a draft awaiting approval: the line shown when the button did not work
                self.last = match v["result"].as_str() {
                    Some("removed") | Some("nothing-to-remove") => None,
                    _ => Some(format!("Not removed: {}", v["why"].as_str().unwrap_or("it did not work"))),
                };
                if let Some(s) = sections() {
                    self.screen = s;
                }
            }
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading(self.screen["title"].as_str().unwrap_or(""));
            ui.separator();
            egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
                for s in self.screen["sections"].as_array().cloned().unwrap_or_default() {
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new(s["heading"].as_str().unwrap_or("")).strong().size(17.0));
                    for l in s["lines"].as_array().cloned().unwrap_or_default() {
                        ui.label(l.as_str().unwrap_or(""));
                    }
                    if let Some(b) = s["button"].as_str() {
                        ui.add_space(4.0);
                        let busy = self.running.is_some();
                        if ui.add_enabled(!busy, egui::Button::new(b)).clicked() {
                            self.press(&ctx);
                        }
                        if busy {
                            ui.spinner();
                        }
                        ui.label(egui::RichText::new(s["button_note"].as_str().unwrap_or("")).weak());
                        if let Some(l) = &self.last {
                            ui.label(l);
                        }
                    }
                }
            });
            ui.add_space(12.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                if ui.button(self.screen["close"].as_str().unwrap_or("Close")).clicked() {
                    // marked shown only now, when the person closes it: a session that
                    // never drew the window must not lose it (rig run 2, 2026-09-27)
                    let m = marker();
                    if let Some(d) = m.parent() {
                        let _ = std::fs::create_dir_all(d);
                    }
                    let _ = std::fs::write(&m, "shown\n");
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
        });
    }
}

fn main() {
    // once per person: after it has been shown, a later sign-in does not open it again
    let m = marker();
    if m.exists() {
        return;
    }
    let Some(screen) = sections() else { return }; // nothing to show (not an upgrade_ install, or settle-in has not run)
    let title = screen["title"].as_str().unwrap_or("settle-in").to_string();
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title(&title).with_inner_size([560.0, 620.0]),
        ..Default::default()
    };
    let _ = eframe::run_native(&title, opts, Box::new(|_cc| Ok(Box::new(App { screen, running: None, last: None }))));
}
