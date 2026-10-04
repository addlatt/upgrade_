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

mod goback;

use eframe::egui;
use serde_json::Value;
use std::process::Command;
use std::sync::mpsc;

pub fn core() -> String {
    std::env::var("SETTLE_IN_BIN").unwrap_or_else(|_| "/usr/local/libexec/upgrade_/settle-in".to_string())
}

fn marker() -> std::path::PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join(".local/state")))
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
    base.join("upgrade_/settle-in-shown") // one mark for the window and the console: shown once per person
}

/// A login screen's own session, not a person's: the greeter's session class,
/// or a system account (below UID_MIN, 1000 on Fedora). Fedora 44's KDE login
/// screen runs /etc/xdg/autostart too, and the window opened over it before
/// anyone signed in (rig, 2026-10-03).
fn is_login_screen(session_class: Option<&str>, uid: u32) -> bool {
    session_class == Some("greeter") || uid < 1000
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
    drawn: bool,
    shown_at: Option<std::time::Instant>,
    focus_logged: bool,
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
        // the evidence that the window reached the screen (not just that it was started):
        // one line in the journal on the first frame drawn (the rig's verdict reads it)
        if !self.drawn {
            self.drawn = true;
            self.shown_at = Some(std::time::Instant::now());
            eprintln!("settle-in-window: showing the summary");
            // ask to come first (the owner, 2026-09-27): the welcome apps open at the same
            // sign-in; a desktop may refuse, so the attention request is the fallback
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(egui::UserAttentionType::Informational));
        }
        // whether the desktop honoured it, as evidence (the rig's capture reads it)
        if !self.focus_logged && self.shown_at.map(|t| t.elapsed().as_secs() >= 3).unwrap_or(false) {
            self.focus_logged = true;
            let focused = ctx.input(|i| i.viewport().focused).unwrap_or(false);
            eprintln!("settle-in-window: {}", if focused { "in front (the desktop gave it focus)" } else { "not given focus by the desktop" });
        }
        if !self.focus_logged {
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }
        if let Some(rx) = &self.running
            && let Ok(v) = rx.try_recv() {
                self.running = None;
                // the line shown when the button did not work (approved 2026-09-27)
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
                    eprintln!("settle-in-window: closed by the person");
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
        });
    }
}

/// "Go back to Windows", from the app menu: its own window, any number of times.
struct GoBackApp(goback::GoBack);

impl eframe::App for GoBackApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        egui::CentralPanel::default().show(ui, |ui| {
            if self.0.ui(&ctx, ui) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
    }
}

fn go_back() {
    let screen = match Command::new(core()).args(["go-back", "screen"]).output() {
        Ok(o) if o.status.success() => serde_json::from_slice::<Value>(&o.stdout).ok(),
        _ => None,
    };
    let Some(screen) = screen else {
        eprintln!("settle-in-window: the go-back screen could not be read from {}", core());
        std::process::exit(1);
    };
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("Go back to Windows").with_inner_size([620.0, 680.0]),
        ..Default::default()
    };
    if let Err(e) = eframe::run_native("Go back to Windows", opts, Box::new(|_cc| Ok(Box::new(GoBackApp(goback::GoBack::new(screen)))))) {
        eprintln!("settle-in-window: could not open the window: {}", e);
        std::process::exit(1);
    }
}

fn main() {
    if std::env::args().any(|a| a == "--go-back") {
        return go_back();
    }
    let uid = std::fs::metadata("/proc/self").map(|m| std::os::unix::fs::MetadataExt::uid(&m)).unwrap_or(0);
    let class = std::env::var("XDG_SESSION_CLASS").ok();
    if is_login_screen(class.as_deref(), uid) {
        eprintln!("settle-in-window: a login screen's session (class {:?}, uid {}); not opening", class, uid);
        return;
    }
    // once per person: after it has been shown, a later sign-in does not open it again
    let m = marker();
    if m.exists() {
        eprintln!("settle-in-window: already shown to this person; not opening");
        return;
    }
    let Some(screen) = sections() else {
        eprintln!("settle-in-window: no summary to show (not an upgrade_ install, or settle-in has not run)");
        return;
    };
    // a few seconds after the sign-in, so the desktop's own welcome app opens first
    // and this window, opening after it, is the newest (the owner, 2026-09-27)
    std::thread::sleep(std::time::Duration::from_secs(5));
    let title = screen["title"].as_str().unwrap_or("settle-in").to_string();
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title(&title).with_inner_size([560.0, 620.0]).with_active(true),
        ..Default::default()
    };
    if let Err(e) = eframe::run_native(&title, opts, Box::new(|_cc| Ok(Box::new(App { screen, drawn: false, shown_at: None, focus_logged: false, running: None, last: None })))) {
        eprintln!("settle-in-window: could not open the window: {}", e);
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::is_login_screen;
    #[test]
    fn a_greeter_session_never_opens_the_window() {
        assert!(is_login_screen(Some("greeter"), 1000));
        assert!(is_login_screen(None, 977)); // a login manager's system account
        assert!(!is_login_screen(Some("user"), 1000));
        assert!(!is_login_screen(None, 1001));
    }
}
