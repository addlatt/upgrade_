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
//!
//! The plan is a tree of tick boxes (2026-11-23, the owner): every folder
//! and file the core found, all ticked to start with. The person unticks a
//! folder to leave it, or opens it and ticks only some. What is ticked goes
//! to the core as the choice (on stdin, after the key); the core copies
//! only that, and only ever from what its own walk found.

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

/// One folder or file in the plan's tree, and whether it is ticked.
#[derive(Default, Debug)]
pub struct Node {
    name: String,
    /// the path inside its top folder (`Taxes/2025/a.pdf`); empty for the top folder
    rel: String,
    /// a file's size; None for a folder
    size: Option<u64>,
    on: bool,
    kids: Vec<Node>,
    files: u64,
    bytes: u64,
    on_files: u64,
    on_bytes: u64,
}

impl Node {
    /// A top folder from the plan's `entries` (`[[path, size], ...]`), all ticked.
    pub fn top(name: &str, entries: &Value) -> Node {
        let mut root = Node { name: name.to_string(), ..Node::default() };
        for e in entries.as_array().into_iter().flatten() {
            let (Some(rel), Some(size)) = (e[0].as_str(), e[1].as_u64()) else { continue };
            let mut at = &mut root;
            let parts: Vec<&str> = rel.split('/').collect();
            for (i, part) in parts.iter().enumerate() {
                let last = i + 1 == parts.len();
                let found = at.kids.iter().position(|k| k.size.is_none() && k.name == *part).filter(|_| !last);
                let idx = match found {
                    Some(x) => x,
                    None => {
                        at.kids.push(Node { name: part.to_string(), rel: parts[..=i].join("/"), size: last.then_some(size), on: true, ..Node::default() });
                        at.kids.len() - 1
                    }
                };
                at = &mut at.kids[idx];
            }
        }
        root.sort();
        root.tally();
        root
    }

    /// Folders first, then files, each by name.
    fn sort(&mut self) {
        self.kids.sort_by(|a, b| (a.size.is_some(), a.name.to_lowercase()).cmp(&(b.size.is_some(), b.name.to_lowercase())));
        self.kids.iter_mut().for_each(Node::sort);
    }

    /// Count again what is here and what is ticked.
    pub fn tally(&mut self) {
        if let Some(b) = self.size {
            (self.files, self.bytes) = (1, b);
            (self.on_files, self.on_bytes) = if self.on { (1, b) } else { (0, 0) };
            return;
        }
        (self.files, self.bytes, self.on_files, self.on_bytes) = (0, 0, 0, 0);
        for k in &mut self.kids {
            k.tally();
            self.files += k.files;
            self.bytes += k.bytes;
            self.on_files += k.on_files;
            self.on_bytes += k.on_bytes;
        }
    }

    /// Tick or untick this and everything under it.
    pub fn set(&mut self, on: bool) {
        self.on = on;
        self.kids.iter_mut().for_each(|k| k.set(on));
    }

    /// Tick exactly what a choice names (the core's shape: `""`, `Taxes/`, `Taxes/a.pdf`).
    pub fn apply(&mut self, paths: &[&str]) {
        if self.size.is_some() {
            self.on = paths.iter().any(|p| p.is_empty() || *p == self.rel || (p.ends_with('/') && self.rel.starts_with(p)));
        }
        self.kids.iter_mut().for_each(|k| k.apply(paths));
    }

    /// What is ticked, as the shortest list that says it: a folder ticked
    /// whole is one line, not one per file.
    pub fn choice(&self, out: &mut Vec<String>) {
        if self.size.is_some() {
            if self.on {
                out.push(self.rel.clone());
            }
        } else if self.on_files == self.files && self.files > 0 {
            out.push(if self.rel.is_empty() { String::new() } else { format!("{}/", self.rel) });
        } else if self.on_files > 0 {
            self.kids.iter().for_each(|k| k.choice(out));
        }
    }
}

/// The choice for the core: None when everything is ticked (the copy as it
/// always was), else each top folder's list.
pub fn choice_of(tops: &[Node]) -> Option<Value> {
    if tops.iter().all(|t| t.on_files == t.files) {
        return None;
    }
    let mut o = serde_json::Map::new();
    for t in tops {
        let mut l = Vec::new();
        t.choice(&mut l);
        if !l.is_empty() {
            o.insert(t.name.clone(), json!(l));
        }
    }
    Some(Value::Object(o))
}

/// A size as a person reads it.
pub fn size(b: u64) -> String {
    match b {
        0..=999 => format!("{} bytes", b),
        1_000..=999_999 => format!("{:.0} KB", b as f64 / 1e3),
        1_000_000..=999_999_999 => format!("{:.1} MB", b as f64 / 1e6),
        _ => format!("{:.1} GB", b as f64 / 1e9),
    }
}

/// One row of the tree and, when it is open, the rows under it. True when a tick changed.
fn tree(ui: &mut egui::Ui, n: &mut Node, id: egui::Id) -> bool {
    if let Some(b) = n.size {
        return ui.checkbox(&mut n.on, format!("{}  ({})", n.name, size(b))).changed();
    }
    let (mut all, some) = (n.on_files == n.files, n.on_files > 0 && n.on_files < n.files);
    let files = if n.files == 1 { "file" } else { "files" };
    let label = if some {
        format!("{}  ({} of {} {}, {} of {})", n.name, n.on_files, n.files, files, size(n.on_bytes), size(n.bytes))
    } else {
        format!("{}  ({} {}, {})", n.name, n.files, files, size(n.bytes))
    };
    let (mut flip, mut changed) = (None, false);
    egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false)
        .show_header(ui, |ui| {
            if ui.add(egui::Checkbox::new(&mut all, label).indeterminate(some)).changed() {
                flip = Some(all);
            }
        })
        .body(|ui| {
            for (i, k) in n.kids.iter_mut().enumerate() {
                changed |= tree(ui, k, id.with(i));
            }
        });
    if let Some(on) = flip {
        n.set(on);
        changed = true;
    }
    changed
}

pub struct Pull {
    /// the plan as tick boxes, one tree per top folder
    tops: Vec<Node>,
    /// the choice the running copy was started with (Carry on sends it again)
    only: Option<String>,
    page: Page,
    plan: Value,
    result: Value,
    progress: Value,
    key: String,
    rx: Option<mpsc::Receiver<Msg>>,
}

/// One root step through pkexec, in a thread: `progress` lines as they
/// come, then the JSON answer (everything else it printed).
fn spawn(ctx: &egui::Context, args: Vec<String>, key: Option<String>, only: Option<String>) -> mpsc::Receiver<Msg> {
    let (tx, rx) = mpsc::channel();
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let mut c = Command::new("pkexec");
        c.arg(crate::core()).args(&args);
        if key.is_some() {
            c.arg("--key-stdin");
        }
        if only.is_some() {
            c.arg("--only-stdin");
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
            if let Some(o) = &only {
                let _ = i.write_all(format!("{}\n", o).as_bytes());
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
        Pull { tops: Vec::new(), only: None, page: Page::Checking, plan: json!({}), result: json!({}), progress: json!({}), key: String::new(), rx: Some(spawn(ctx, vec!["pull".into(), "check".into()], None, None)) }
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

    /// What the core will not copy from any folder, and why, folded away.
    fn not_copied(&self, ui: &mut egui::Ui) {
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

    /// Ticked files, all files, ticked bytes, all bytes.
    fn totals(&self) -> (u64, u64, u64, u64) {
        self.tops.iter().fold((0, 0, 0, 0), |a, t| (a.0 + t.on_files, a.1 + t.files, a.2 + t.on_bytes, a.3 + t.bytes))
    }

    /// Whether what is ticked fits on this Linux with the core's room to
    /// spare. The core decides again when it copies; this only saves a refusal.
    fn fits(&self) -> bool {
        let (n, all_n, b, _) = self.totals();
        let Some(free) = self.plan["free"].as_u64() else { return true };
        // what an earlier run already brought counts only when everything is ticked, as in the core
        let need = if n == all_n { b.saturating_sub(self.plan["already_home"].as_u64().unwrap_or(0)) } else { b };
        need.saturating_add(self.plan["margin"].as_u64().unwrap_or(0)) <= free
    }

    pub fn ui(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) -> bool {
        while let Some(m) = self.rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            match m {
                Msg::Progress(p) => self.progress = p,
                Msg::Done(d) => {
                    self.rx = None;
                    match self.page {
                        Page::Checking => {
                            self.tops = d["folders"].as_array().into_iter().flatten().filter(|f| f["entries"].as_array().is_some_and(|e| !e.is_empty()))
                                .map(|f| Node::top(f["name"].as_str().unwrap_or(""), &f["entries"])).collect();
                            // a copy that stopped is offered again as it was chosen
                            if let Some(last) = d["last_only"].as_object() {
                                for t in &mut self.tops {
                                    let paths: Vec<&str> = last.get(&t.name).and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
                                    t.apply(&paths);
                                    t.tally();
                                }
                            }
                            if d["result"] != "plan" {
                                self.tops.clear();
                            }
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
                Page::Plan if !self.tops.is_empty() => {
                    let w = self.plan["words"].clone();
                    Self::lines(ui, &w["intro"]);
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        for (word, on) in [("all", true), ("none", false)] {
                            if ui.small_button(w[word].as_str().unwrap_or("")).clicked() {
                                self.tops.iter_mut().for_each(|t| {
                                    t.set(on);
                                    t.tally();
                                });
                            }
                        }
                    });
                    for (i, t) in self.tops.iter_mut().enumerate() {
                        if tree(ui, t, egui::Id::new(("pull-tree", i))) {
                            t.tally();
                        }
                    }
                    ui.add_space(6.0);
                    let (n, all_n, b, all_b) = self.totals();
                    let line = w["chosen"].as_str().unwrap_or("").replace("{files}", &n.to_string()).replace("{all_files}", &all_n.to_string()).replace("{size}", &size(b)).replace("{all_size}", &size(all_b));
                    ui.label(egui::RichText::new(line).strong());
                    if !self.fits() {
                        let free = size(self.plan["free"].as_u64().unwrap_or(0));
                        ui.label(egui::RichText::new(w["too_big"].as_str().unwrap_or("").replace("{free}", &free)).color(egui::Color32::from_rgb(200, 60, 40)));
                    }
                    ui.add_space(6.0);
                    for l in w["notes"].as_array().cloned().unwrap_or_default() {
                        ui.label(egui::RichText::new(l.as_str().unwrap_or("")).weak());
                    }
                    self.not_copied(ui);
                }
                Page::Plan => {
                    let w = self.plan["words"].clone();
                    Self::lines(ui, &w["lines"]);
                    if let Some(field) = w["key_field"].as_str() {
                        ui.add_space(6.0);
                        ui.label(egui::RichText::new(field).strong());
                        ui.add(egui::TextEdit::singleline(&mut self.key).desired_width(460.0));
                    }
                    self.not_copied(ui);
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
                        let can = self.tops.is_empty() || (self.totals().0 > 0 && self.fits());
                        if ui.add_enabled(!busy && can, egui::Button::new(egui::RichText::new(b).strong())).clicked() {
                            self.progress = json!({});
                            self.only = choice_of(&self.tops).filter(|_| !self.tops.is_empty()).map(|c| c.to_string());
                            self.rx = Some(spawn(ctx, vec!["pull".into(), "run".into()], self.typed_key(), self.only.clone()));
                            self.page = Page::Copying;
                        }
                    } else if w["key_field"].is_string() || self.plan["result"] == "refused" {
                        if ui.add_enabled(!busy, egui::Button::new("Check again")).clicked() {
                            self.rx = Some(spawn(ctx, vec!["pull".into(), "check".into()], self.typed_key(), None));
                            self.page = Page::Checking;
                        }
                    }
                }
                Page::Result if self.result["result"] == "stopped" || self.result["result"] == "done-with-failures" => {
                    if ui.add_enabled(!busy, egui::Button::new("Carry on")).clicked() {
                        self.rx = Some(spawn(ctx, vec!["pull".into(), "run".into()], self.typed_key(), self.only.clone()));
                        self.page = Page::Copying;
                    }
                }
                _ => {}
            }
        });
        close
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn docs() -> Node {
        Node::top("Documents", &json!([["letter.txt", 4], ["Taxes/2025/return.pdf", 5000], ["Taxes/2025/notes.txt", 10], ["Taxes/old.pdf", 300]]))
    }

    fn pick(t: &mut Node, paths: &[&str]) -> Option<Value> {
        t.apply(paths);
        t.tally();
        choice_of(std::slice::from_ref(t))
    }

    #[test]
    fn the_tree_counts_and_says_the_choice_in_the_cores_shape() {
        let mut t = docs();
        assert_eq!((t.files, t.bytes, t.on_files), (4, 5314, 4));
        assert_eq!(t.kids.iter().map(|k| k.name.as_str()).collect::<Vec<_>>(), ["Taxes", "letter.txt"], "folders first");
        // everything ticked is no choice at all: the copy as it always was
        assert_eq!(choice_of(std::slice::from_ref(&t)), None);
        // a folder ticked whole is one line; a folder half ticked is its files
        assert_eq!(pick(&mut t, &["Taxes/"]), Some(json!({ "Documents": ["Taxes/"] })));
        assert_eq!((t.on_files, t.on_bytes), (3, 5310));
        assert_eq!(pick(&mut t, &["Taxes/2025/", "letter.txt"]), Some(json!({ "Documents": ["Taxes/2025/", "letter.txt"] })));
        assert_eq!(pick(&mut t, &["Taxes/2025/notes.txt"]), Some(json!({ "Documents": ["Taxes/2025/notes.txt"] })));
        // nothing ticked: the folder is not named at all
        t.set(false);
        t.tally();
        assert_eq!(choice_of(std::slice::from_ref(&t)), Some(json!({})));
        // unticking one whole top folder of two names only the other
        let (mut a, b) = (docs(), Node::top("Pictures", &json!([["cat.jpg", 3000]])));
        a.set(false);
        a.tally();
        assert_eq!(choice_of(&[a, b]), Some(json!({ "Pictures": [""] })));
    }

    #[test]
    fn sizes_as_a_person_reads_them() {
        assert_eq!([size(4), size(5_000), size(2_500_000), size(11_400_000_000)], ["4 bytes", "5 KB", "2.5 MB", "11.4 GB"]);
    }
}
