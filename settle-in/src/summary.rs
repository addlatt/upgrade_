//! What the person is shown (the window, or the text at a console sign-in).
//! The words are the owner's, approved 2026-09-27, and live here only: the
//! window and the console both print what `settle-in summary` gives them.
//!
//! The public summary (/var/lib/upgrade_-settle-in/summary.json, readable
//! by everyone) carries no secret: network names, the clock's result, and
//! whether the old-boot-entry button applies. The window runs as the person,
//! who cannot read the root-only handoff folder.

use serde_json::{json, Value};

pub const PUBLIC_DIR: &str = "var/lib/upgrade_-settle-in";

pub fn pretty_name(root: &str) -> String {
    let t = std::fs::read_to_string(format!("{}/etc/os-release", root.trim_end_matches('/'))).unwrap_or_default();
    t.lines()
        .find_map(|l| l.strip_prefix("PRETTY_NAME="))
        .map(|v| v.trim_matches('"').to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "Linux".to_string())
}

/// The public summary, from the root-only report and the job.
pub fn build(root: &str, report: &Value, job: &Value) -> Value {
    let profiles = job.pointer("/harvest/wifi/profiles").and_then(Value::as_array).cloned().unwrap_or_default();
    let auto = |ssid: &Value| profiles.iter().find(|p| p.get("ssid") == Some(ssid)).and_then(|p| p.get("autoconnect")).and_then(Value::as_bool).unwrap_or(true);
    let nets = report.pointer("/wifi/networks").and_then(Value::as_array).cloned().unwrap_or_default();
    let names = |f: &dyn Fn(&Value) -> bool| nets.iter().filter(|n| f(n)).map(|n| n["ssid"].clone()).collect::<Vec<_>>();
    json!({
        "schema": "settle-in-summary/1",
        "name": pretty_name(root),
        "clock": { "result": report.pointer("/clock/result"), "why": report.pointer("/clock/why") },
        "wifi": {
            "result": report.pointer("/wifi/result"),
            "why": report.pointer("/wifi/why"),
            "automatic": names(&|n| matches!(n["result"].as_str(), Some("created" | "already-there")) && auto(&n["ssid"])),
            "manual": names(&|n| matches!(n["result"].as_str(), Some("created" | "already-there")) && !auto(&n["ssid"])),
            "not_set_up": nets.iter().filter(|n| n["result"] == "not-set-up").map(|n| json!({ "ssid": n["ssid"], "why": n["why"] })).collect::<Vec<_>>(),
            "passwords_deleted": report.pointer("/wifi/passwords_deleted"),
        },
        "ssh": report.get("ssh").cloned().unwrap_or(json!({ "result": "not-carried" })),
        "old_boot_entry": report.get("old_boot_entry").cloned().unwrap_or(json!({ "offered": false })),
        // for "Go back to Windows": which Windows it was, never a key (the
        // schema refuses key shapes in every field; R13, R30)
        "windows_before": windows_before(job),
        // for "Go back to Windows": a kept Windows means "Windows first again"
        // (the rollback), not a fresh install (decided 2026-10-12, the owner)
        "windows_kept": windows_kept(root, job),
        // "you had these; here is the Linux version" (decided 2026-11-07, the
        // owner): the person's own program names on their own machine, no secret
        "programs": crate::programs::list(job),
    })
}

fn windows_kept(root: &str, job: &Value) -> bool {
    let intent = job.pointer("/intent/path").and_then(Value::as_str) == Some("keep-windows");
    let outcome = std::fs::read_to_string(format!("{}/var/lib/upgrade_/outcome.json", root.trim_end_matches('/'))).ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok()).unwrap_or(Value::Null);
    intent && outcome.pointer("/path_taken").and_then(Value::as_str) == Some("keep-windows") && outcome.pointer("/windows/kept") == Some(&json!(true))
}

fn windows_before(job: &Value) -> Value {
    let l = job.pointer("/harvest/windows_license").cloned().unwrap_or(Value::Null);
    if l.is_null() {
        return json!({ "result": "unreadable", "reason": "the job predates the licence harvest" });
    }
    let mut o = serde_json::Map::new();
    for k in ["result", "reason", "windows_version", "edition_id", "product_name", "display_version", "build", "activated", "channel", "firmware_key_present"] {
        o.insert(k.to_string(), l.get(k).cloned().unwrap_or(Value::Null));
    }
    Value::Object(o)
}

fn join(v: &Value) -> String {
    v.as_array().map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")).unwrap_or_default()
}

/// The screen as sections of plain text: [{ heading, lines, button? }].
pub fn sections(s: &Value) -> Value {
    let mut out = Vec::new();
    // --- the clock (approved 2026-09-27)
    let clock = match s.pointer("/clock/result").and_then(Value::as_str) {
        Some("corrected") => vec!["Set. Windows kept the computer's clock in local time; it now keeps the standard time Linux uses. Nothing to do.".to_string()],
        _ => vec![format!(
            "Not changed: {}. It will set itself once you are online.",
            s.pointer("/clock/why").and_then(Value::as_str).unwrap_or("settle-in did not run")
        )],
    };
    out.push(json!({ "heading": "The clock", "lines": clock }));
    // --- Wi-Fi (approved 2026-09-27, all lines)
    let w = &s["wifi"];
    let mut wl = Vec::new();
    let auto = join(&w["automatic"]);
    let manual = join(&w["manual"]);
    let not: Vec<String> = w["not_set_up"].as_array().cloned().unwrap_or_default().iter()
        .map(|n| format!("{} - {}", n["ssid"].as_str().unwrap_or("?"), n["why"].as_str().unwrap_or("")))
        .collect();
    match w["result"].as_str() {
        Some("nothing-to-do") | None => wl.push("Windows had no saved Wi-Fi networks.".to_string()),
        Some("no-networkmanager") | Some("failed") | Some("not-in-job") => {
            wl.push(format!("Not set up: {}.", w["why"].as_str().unwrap_or("settle-in could not set them up")))
        }
        _ => {
            if !auto.is_empty() {
                wl.push("These networks connect by themselves:".to_string());
                wl.push(format!("  {}", auto));
            }
            if !manual.is_empty() {
                wl.push("These networks are set up; connect to them from the network menu:".to_string());
                wl.push(format!("  {}", manual));
            }
            if !not.is_empty() {
                wl.push("Not set up (join it from the network menu if you need it):".to_string());
                for n in not {
                    wl.push(format!("  {}", n));
                }
            }
            if w["passwords_deleted"] == json!(true) {
                wl.push("Their passwords are no longer on the USB stick or in this setup program.".to_string());
            }
        }
    }
    out.push(json!({ "heading": "Wi-Fi", "lines": wl }));
    // --- remote access (2026-10-04; words are a draft for the owner): only when Windows had it on
    let h = &s["ssh"];
    match h["result"].as_str() {
        Some("carried") => {
            let n = h["keys"].as_u64().unwrap_or(0);
            out.push(json!({ "heading": "Remote access (SSH)", "lines": [format!(
                "Turned on, as it was in Windows. Only the {} key{} Windows allowed can sign in this way; passwords cannot.",
                n, if n == 1 { "" } else { "s" })] }));
        }
        Some("failed") | Some("no-ssh-server") => {
            out.push(json!({ "heading": "Remote access (SSH)", "lines": [format!("Windows had it on, but it was not turned on here: {}.", h["why"].as_str().unwrap_or("the reason was not recorded"))] }));
        }
        _ => {}
    }
    // --- the files (2026-11-07; words are a draft for the owner): only with Windows kept
    if s["windows_kept"] == json!(true) {
        out.push(files_section(&s["files"]));
    }
    // --- the programs (2026-11-07; words are a draft for the owner): not for a job from before the inventory
    if s.pointer("/programs/result").and_then(Value::as_str).is_some_and(|r| r != "not-in-job") {
        out.push(json!({ "heading": "Your programs", "lines": crate::programs::lines(&s["programs"]) }));
    }
    // --- the old boot entry (approved 2026-09-27): only when offered
    let b = &s["old_boot_entry"];
    if b["offered"] == json!(true) {
        let desc = b["entries"].get(0).and_then(|e| e["description"].as_str()).unwrap_or("Windows Boot Manager");
        out.push(json!({
            "heading": "The old Windows startup entry",
            "lines": [format!("Windows is gone, but the computer's startup menu still lists \"{}\". Choosing it would do nothing.", desc)],
            "button": "Remove the old Windows startup entry",
            "button_note": "You will be asked for your password.",
        }));
    } else if b.get("removed").is_some() {
        // after the button worked (approved 2026-09-27)
        out.push(json!({ "heading": "The old Windows startup entry", "lines": ["Removed."] }));
    }
    json!({ "title": format!("{} is ready", s["name"].as_str().unwrap_or("Linux")), "sections": out, "close": "Close" })
}

fn gb(b: &Value) -> String {
    crate::pull::gb(b.as_u64().unwrap_or(0))
}

fn names(folders: &Value) -> String {
    folders.as_array().map(|a| a.iter().filter(|f| f["copied"].as_u64().unwrap_or(0) > 0).filter_map(|f| f["name"].as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default()
}

fn key_line(k: &Value) -> Option<String> {
    match k["stick"].as_str() {
        Some("not-plugged-in") | Some("still-there") => Some("The USB stick still holds Windows' BitLocker recovery key. Plug the stick in and press the button again to delete it there too.".into()),
        _ => None,
    }
}

/// "Your files", on the first screen (draft words, 2026-11-07).
fn files_section(f: &Value) -> Value {
    let sum = |k: &str| f["folders"].as_array().map(|a| a.iter().map(|x| x[k].as_u64().unwrap_or(0)).sum::<u64>()).unwrap_or(0);
    let mut l: Vec<String> = Vec::new();
    let mut button = Some("Bring my files over");
    match f["result"].as_str() {
        None => {
            l.push("Your files are still in Windows, untouched. This can copy them into your folders here (Documents, Pictures and the rest). Windows keeps its own copies.".into());
        }
        Some("done") | Some("done-with-failures") => {
            l.push(format!("Brought over: {} files ({}) into {}. Windows still has its own copies.", sum("copied"), gb(&f["bytes"]), names(&f["folders"])));
            if sum("renamed") > 0 {
                l.push(format!("{} had the name of a file already here, so \"(from Windows)\" was added to their names.", sum("renamed")));
            }
            if sum("in_onedrive") > 0 {
                l.push(format!("{} files live only in OneDrive and were not copied. Sign in at onedrive.live.com to reach them.", sum("in_onedrive")));
            }
            if f["result"] == "done-with-failures" {
                l.push(format!("{} files could not be read from Windows. They are still there; pressing the button tries them again.", sum("failed")));
                button = Some("Try the rest again");
            } else {
                button = None;
            }
            if let Some(k) = key_line(&f["key"]) {
                l.push(k);
                button = button.or(Some("Delete the key from the stick"));
            }
        }
        Some("stopped") => {
            l.push(format!("Stopped: {}. Nothing already copied is lost, and the copy carries on from where it stopped.", f["why"].as_str().unwrap_or("the reason was not recorded")));
            button = Some("Carry on");
        }
        _ => {}
    }
    let mut s = json!({ "heading": "Your files", "lines": l });
    if let Some(b) = button {
        s["button"] = json!(b);
        s["button_note"] = json!("You will be asked for your password.");
        s["action"] = json!("pull");
    }
    s
}

/// What `settle-in pull check` shows before the copy (draft words, 2026-11-07).
pub fn pull_words(p: &Value) -> Value {
    let mut l: Vec<String> = Vec::new();
    match p["result"].as_str() {
        Some("plan") => {
            let n: u64 = p["folders"].as_array().into_iter().flatten().map(|f| f["files"].as_u64().unwrap_or(0)).sum();
            l.push(format!("This copies {} files ({}) from Windows into this computer's own folders:", n, gb(&p["bytes"])));
            for f in p["folders"].as_array().cloned().unwrap_or_default() {
                l.push(format!("  {}: {} files, {}, into {}", f["name"].as_str().unwrap_or(""), f["files"], gb(&f["bytes"]), f["to"].as_str().unwrap_or("")));
            }
            let skipped: usize = p["folders"].as_array().into_iter().flatten().map(|f| f["not_copied"].as_array().map_or(0, Vec::len)).sum();
            let cloud: u64 = p["folders"].as_array().into_iter().flatten().map(|f| f["in_onedrive"].as_u64().unwrap_or(0)).sum();
            if cloud > 0 {
                l.push(format!("{} files live only in OneDrive, not on this computer; they stay there (onedrive.live.com).", cloud));
            }
            if skipped > 0 {
                l.push(format!("{} things are not copied: links to other places, Windows' own system files and folder settings. The list is below.", skipped));
            }
            l.push("Windows is only read: nothing on it changes, and it keeps its own copies. If the copy is stopped, it carries on next time.".into());
            json!({ "lines": l, "button": "Copy them", "button_note": "Keep the computer plugged in. It can take a while." })
        }
        Some("nothing-to-do") => json!({ "lines": p["why"].as_array().cloned().unwrap_or_default() }),
        _ => {
            l.push("Your files cannot be brought over yet:".into());
            for w in p["why"].as_array().cloned().unwrap_or_default() {
                l.push(format!("  - {}.", w.as_str().unwrap_or("").trim_end_matches('.')));
            }
            l.push("Nothing was copied and nothing changed. Your files are safe in Windows.".into());
            let mut w = json!({ "lines": l });
            if p["needs_key"] == json!(true) {
                w["key_field"] = json!("BitLocker recovery key (eight groups of six digits)");
            }
            w
        }
    }
}

/// What the window shows after `settle-in pull run` (draft words, 2026-11-07).
pub fn pull_done_words(o: &Value) -> Value {
    let mut sec = files_section(&json!({
        "result": o["result"], "why": o["why"], "bytes": o["bytes"], "key": o["key"],
        "folders": o["folders"].as_array().cloned().unwrap_or_default().iter().map(|f| json!({
            "name": f["name"], "copied": f["copied"].as_u64().unwrap_or(0) + f["already_copied"].as_u64().unwrap_or(0),
            "failed": f["failed"].as_array().map_or(0, Vec::len), "renamed": f["renamed"].as_array().map_or(0, Vec::len), "in_onedrive": f["in_onedrive"] })).collect::<Vec<_>>(),
    }));
    if o["result"] == "refused" || o["result"] == "nothing-to-do" {
        sec = json!({ "heading": "Your files", "lines": pull_words(o)["lines"].clone() });
    }
    let failed: Vec<String> = o["folders"].as_array().into_iter().flatten()
        .flat_map(|f| f["failed"].as_array().cloned().unwrap_or_default().into_iter().map(move |x| format!("{}/{}: {}", f["name"].as_str().unwrap_or(""), x["path"].as_str().unwrap_or(""), x["why"].as_str().unwrap_or(""))))
        .collect();
    sec["failed_list"] = json!(failed);
    sec
}

/// The same screen as text, for a console.
pub fn text(sec: &Value) -> String {
    let mut t = format!("{}\n{}\n", sec["title"].as_str().unwrap_or(""), "-".repeat(48));
    for s in sec["sections"].as_array().cloned().unwrap_or_default() {
        t.push_str(&format!("\n{}\n", s["heading"].as_str().unwrap_or("")));
        for l in s["lines"].as_array().cloned().unwrap_or_default() {
            t.push_str(&format!("  {}\n", l.as_str().unwrap_or("")));
        }
        // at a console the button is the question that follows (rig run 4: the bracket line was redundant)
        if s["button"].as_str().is_some() {
            t.push_str(&format!("  {}\n", s["button_note"].as_str().unwrap_or("")));
        }
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary() -> Value {
        let job = json!({ "harvest": { "wifi": { "profiles": [
            { "ssid": "Home Net", "autoconnect": true }, { "ssid": "Cafe Guest", "autoconnect": true }, { "ssid": "Work", "autoconnect": true } ] } } });
        let report = json!({
            "clock": { "result": "corrected" },
            "wifi": { "result": "set-up", "created": 2, "passwords_deleted": true, "networks": [
                { "ssid": "Home Net", "result": "created" }, { "ssid": "Cafe Guest", "result": "created" },
                { "ssid": "Work", "result": "not-set-up", "why": "an enterprise network (a company or school sign-in)" } ] },
            "old_boot_entry": { "offered": true, "entries": [{ "entry": "Boot0000", "description": "Windows Boot Manager" }] } });
        let mut s = build("/nonexistent", &report, &job);
        s["name"] = json!("Fedora Linux 42 (KDE Plasma)");
        s
    }

    #[test]
    fn the_approved_screen() {
        let t = text(&sections(&summary()));
        let want = "Fedora Linux 42 (KDE Plasma) is ready
------------------------------------------------

The clock
  Set. Windows kept the computer's clock in local time; it now keeps the standard time Linux uses. Nothing to do.

Wi-Fi
  These networks connect by themselves:
    Home Net, Cafe Guest
  Not set up (join it from the network menu if you need it):
    Work - an enterprise network (a company or school sign-in)
  Their passwords are no longer on the USB stick or in this setup program.

The old Windows startup entry
  Windows is gone, but the computer's startup menu still lists \"Windows Boot Manager\". Choosing it would do nothing.
  You will be asked for your password.
";
        assert_eq!(t, want);
    }

    #[test]
    fn ssh_only_when_carried() {
        let mut s = summary();
        assert!(!text(&sections(&s)).contains("Remote access"));
        s["ssh"] = json!({ "result": "carried", "keys": 1 });
        let t = text(&sections(&s));
        assert!(t.contains("Remote access (SSH)") && t.contains("Turned on, as it was in Windows. Only the 1 key Windows allowed can sign in this way; passwords cannot."), "{}", t);
        s["ssh"] = json!({ "result": "no-ssh-server", "why": "this system has no SSH server installed; the keys were not set up" });
        assert!(text(&sections(&s)).contains("was not turned on here: this system has no SSH server installed"));
    }

    #[test]
    fn the_files_section() {
        let mut s = summary();
        assert!(!text(&sections(&s)).contains("Your files"), "not when Windows is gone");
        s["windows_kept"] = json!(true);
        let sec = sections(&s);
        let t = text(&sec);
        assert!(t.contains("Your files\n  Your files are still in Windows, untouched."), "{t}");
        assert!(sec["sections"].as_array().unwrap().iter().any(|x| x["action"] == "pull" && x["button"] == "Bring my files over"));
        s["files"] = json!({ "result": "done", "bytes": 2_500_000_000u64, "key": { "deleted_here": true, "stick": "not-plugged-in" },
            "folders": [{ "name": "Documents", "copied": 10, "failed": 0, "renamed": 1, "in_onedrive": 3 }, { "name": "Music", "copied": 0, "failed": 0, "renamed": 0, "in_onedrive": 0 }] });
        let t = text(&sections(&s));
        assert!(t.contains("Brought over: 10 files (2.5 GB) into Documents. Windows still has its own copies."), "{t}");
        assert!(t.contains("1 had the name of a file already here"), "{t}");
        assert!(t.contains("3 files live only in OneDrive"), "{t}");
        assert!(t.contains("still holds Windows' BitLocker recovery key"), "{t}");
        s["files"]["key"]["stick"] = json!("deleted");
        let sec = sections(&s);
        assert!(sec["sections"].as_array().unwrap().iter().all(|x| x["action"] != "pull"), "no button once all is done");
        s["files"]["result"] = json!("stopped");
        s["files"]["why"] = json!("this Linux's drive is full");
        assert!(text(&sections(&s)).contains("Stopped: this Linux's drive is full. Nothing already copied is lost"));
    }

    #[test]
    fn the_pull_words() {
        let p = json!({ "result": "refused", "why": ["Windows' drive is locked with BitLocker, and its recovery key did not come across."], "needs_key": true });
        let w = pull_words(&p);
        assert_eq!(w["lines"][1], "  - Windows' drive is locked with BitLocker, and its recovery key did not come across.");
        assert!(w["key_field"].is_string());
        let p = json!({ "result": "plan", "bytes": 1_000_000_000u64, "folders": [{ "name": "Pictures", "files": 4, "bytes": 1_000_000_000u64, "to": "/home/ann/Bilder", "not_copied": [{ "path": "desktop.ini", "why": "x" }], "in_onedrive": 0 }] });
        let w = pull_words(&p);
        assert_eq!(w["lines"][0], "This copies 4 files (1.0 GB) from Windows into this computer's own folders:");
        assert_eq!(w["lines"][1], "  Pictures: 4 files, 1.0 GB, into /home/ann/Bilder");
        assert_eq!(w["button"], "Copy them");
    }

    #[test]
    fn clock_left_alone_and_no_wifi() {
        let mut s = summary();
        s["clock"] = json!({ "result": "left-alone", "why": "the installer could not read the hardware clock" });
        s["wifi"]["result"] = json!("nothing-to-do");
        s["old_boot_entry"] = json!({ "offered": false });
        let t = text(&sections(&s));
        assert!(t.contains("Not changed: the installer could not read the hardware clock. It will set itself once you are online."));
        assert!(t.contains("Windows had no saved Wi-Fi networks."));
        assert!(!t.contains("startup entry"));
    }
}
