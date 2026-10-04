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
    })
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
