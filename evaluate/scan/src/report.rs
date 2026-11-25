//! The text report, line for line as the PowerShell scanner prints it.

use crate::check::Status;
use crate::facts::Machine;
use crate::ps::{eq_ci, matches, num, s, Stamp, Version};
use crate::run::Outcome;
use crate::verdict::Level;

const RULE: &str = "===============================================================================";

fn mark(status: Status) -> &'static str {
    match status {
        Status::Ok => "[ OK ]",
        Status::Warn => "[WARN]",
        Status::Fail => "[FAIL]",
        Status::Unknown => "[ ?? ]",
        Status::Info => "[ -- ]",
    }
}

fn len(text: &str) -> usize {
    text.encode_utf16().count()
}

/// Format-UpgWrap: words wrapped at `width`, each line after `indent`.
/// Lines that start with two or more spaces are kept as they are.
pub fn wrap(text: &str, width: usize, indent: &str) -> Vec<String> {
    let mut out = Vec::new();
    if text.trim().is_empty() {
        return out;
    }
    for para in text.split('\n').map(|p| p.strip_suffix('\r').unwrap_or(p)) {
        if para.trim().is_empty() {
            out.push(String::new());
            continue;
        }
        if matches(r"^\s{2,}\S", para) {
            out.push(format!("{indent}{para}"));
            continue;
        }
        let mut line = String::new();
        for word in para.split_whitespace() {
            if len(&line) + len(word) + 1 > width {
                out.push(format!("{indent}{line}"));
                line = word.to_string();
            } else if line.is_empty() {
                line = word.to_string();
            } else {
                line = format!("{line} {word}");
            }
        }
        if !line.is_empty() {
            out.push(format!("{indent}{line}"));
        }
    }
    out
}

/// The unmatched devices as the report lists them: sorted, each once.
pub fn unmatched_sorted(unmatched: &[String]) -> Vec<String> {
    let mut list: Vec<String> = Vec::new();
    for u in unmatched {
        if !list.iter().any(|x| eq_ci(x, u)) {
            list.push(u.clone());
        }
    }
    list.sort_by_key(|x| x.to_lowercase());
    list
}

/// Write-UpgReport. `version` is the scanner version the header names.
pub fn lines(m: &Machine, o: &Outcome, now: Stamp, version: &str) -> Vec<String> {
    let mut l: Vec<String> = Vec::new();
    let mut add = |text: &str| l.push(text.to_string());
    let sys = &m.sys;
    add(RULE);
    add(&format!("  upgrade_  preflight  v{version}"));
    add(&format!("  {} {}", s(&sys.vendor), s(&sys.model)));
    add(&format!("  {:04}-{:02}-{:02} {:02}:{:02}", now.year, now.month, now.day, now.hour, now.minute));
    add(RULE);
    add("");
    add(&format!("  {}", s(&sys.cpu_name)));
    add(&format!("  {} GB RAM   |   BIOS {}   |   {}", sys.ram_gb.map(num).unwrap_or_default(), s(&sys.bios_version), s(&sys.os_caption)));
    if !m.is_admin {
        add("");
        add("  ! Running without Administrator rights. Encryption status could not be");
        add("    checked - see the BitLocker line below.");
    }
    add("");

    for section in ["Fundamentals", "Storage", "Hardware", "Software", "Context"] {
        let items: Vec<_> = o.scan.checks.iter().filter(|c| eq_ci(&c.section, section)).collect();
        if items.is_empty() {
            continue;
        }
        add("");
        add(&format!("-- {} {}", section.to_uppercase(), "-".repeat(75 - section.len())));
        add("");
        for c in items {
            add(&format!("  {} {:<24} {}", mark(c.status), c.title, c.detail));
            if !c.min_kernel.is_empty() {
                add(&format!("         needs kernel {} or newer", c.min_kernel));
            }
            for w in wrap(&c.note, 68, "         ") {
                add(&w);
            }
            if !c.remedy.is_empty() {
                add("");
                add("         WHAT TO DO:");
                for w in wrap(&c.remedy, 68, "           ") {
                    add(&w);
                }
            }
            add("");
        }
    }

    add("");
    add(RULE);
    add(&format!("  VERDICT: {}", o.verdict.level.as_str()));
    add(RULE);
    add("");
    for w in wrap(o.verdict.summary, 68, "  ") {
        add(&w);
    }
    add("");
    for g in &o.verdict.groups {
        add(&format!("  {}", g.label));
        for item in &g.items {
            add(&format!("    - {item}"));
        }
        add("");
    }

    // the kit's own Linuxes, and nothing else: what this stick can install here
    // under RED nothing is put forward: only the releases that are refused, and why
    let red = o.verdict.level == Level::Red;
    let shown: Vec<_> = o.scan.releases.iter().filter(|r| !red || r.starts == "no").collect();
    if !shown.is_empty() {
        add("-- THE LINUX THIS KIT INSTALLS ------------------------------------------------");
        add("");
        if let Some(kernel) = o.required_kernel {
            add(&format!("  This computer's hardware needs Linux kernel {kernel} or newer."));
            add("  An older Linux would install, and the hardware listed above would not work.");
            add("");
        }
        for r in shown {
            let kernel = Version::parse_release(&r.kernel).map_or(String::new(), |k| format!("  (kernel {k})"));
            match r.starts.as_str() {
                "yes" => {
                    add(&format!("  * {}{kernel}", r.name));
                    add(if o.required_kernel.is_some() { "      Starts on this computer, and its kernel is new enough." } else { "      Starts on this computer." });
                }
                "no" => {
                    add(&format!("  x {}{kernel}", r.name));
                    add("      Not offered on this computer:");
                    for why in &r.why {
                        for w in wrap(why, 66, "        ") {
                            add(&w);
                        }
                    }
                }
                _ => {
                    add(&format!("  ? {}{kernel}", r.name));
                    add("      Whether it starts here could not be checked. Run the scanner as");
                    add("      administrator.");
                }
            }
            add("");
        }
    }

    if o.verdict.level != Level::Red {
        add("-- BEFORE YOU DO ANYTHING -----------------------------------------------------");
        add("");
        add("   1. Save your BitLocker recovery key somewhere that is not this computer");
        add("      - a phone note, another machine. An encrypted disk touched without it");
        add("      is gone for good.");
        add("   2. Copy anything you cannot lose to another place. Keeping Windows leaves");
        add("      your files where they are. Erasing everything carries none of them.");
        add("   3. If you are unsure, keep Windows. You can go back to it from Linux,");
        add("      and remove it later, once everything works.");
        add("   4. Nothing is changed until your last click in the window.");
        add("");
    }

    if !o.scan.unmatched.is_empty() {
        add("-- HELP THE PROJECT -----------------------------------------------------------");
        add("");
        add("  These devices are not individually catalogued yet, so the advice above");
        add("  fell back to a general rule for the vendor. Reporting them - and what");
        add("  actually happened when you installed - makes the next report exact:");
        add("");
        for u in unmatched_sorted(&o.scan.unmatched) {
            add(&format!("    {u}"));
        }
        add("");
    }

    add(RULE);
    add("  This scanner made no changes to this computer and sent nothing anywhere.");
    add(RULE);
    l
}
