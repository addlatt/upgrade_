//! The text report, line for line as the PowerShell scanner prints it.

use crate::check::Status;
use crate::data::tables;
use crate::facts::Machine;
use crate::ps::{eq_ci, matches, num, s, Stamp};
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

/// How many days ago the distro table was checked, as `[int]` rounds it.
pub fn distro_table_age(now: Stamp) -> i64 {
    let v = &tables().distro_table_verified;
    let verified = Stamp::parse(&format!("{v}T00:00:00")).map_or(0, |t| t.seconds());
    ((now.seconds() - verified) as f64 / 86400.0).round_ties_even() as i64
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

    let rec = &o.recommendation;
    if let Some(kernel) = o.required_kernel {
        add("-- THE ONE NUMBER THAT MATTERS ------------------------------------------------");
        add("");
        add(&format!("  This machine needs Linux kernel {kernel} or newer."));
        add("");
        add("  A distribution older than that will not merely run slower - the specific");
        add("  hardware listed above will not work at all. This is the mistake that sends");
        add("  people back to Windows convinced Linux is broken.");
        add("");
        if !rec.excluded.is_empty() {
            add("  RULED OUT for shipping an older kernel by default:");
            for x in &rec.excluded {
                add(&format!("    x {}  (kernel {})", x.name, x.kernel));
            }
            add("");
            add("  Those are popular, and they are the ones a first-time user is most");
            add("  likely to be pointed at. On this machine they are the wrong answer.");
            add("");
        }
    }

    // a RED verdict never recommends a distribution
    if o.verdict.level != Level::Red {
        add("-- RECOMMENDED ----------------------------------------------------------------");
        add("");
        if rec.distros.is_empty() {
            add("  No distribution in our table ships a new enough kernel by default.");
            add("  Use a rolling-release distribution, or Fedora, and expect to be on");
            add("  recent-hardware territory.");
        } else {
            for d in &rec.distros {
                add(&format!("  * {}  (kernel {})", d.name, d.kernel));
                for w in wrap(&d.note, 68, "      ") {
                    add(&w);
                }
                add("");
            }
        }
        if rec.has_nvidia {
            add("  NVIDIA present - only distributions that install the proprietary driver");
            add("  for you are listed above.");
            add("");
        }
        add("-- BEFORE YOU DO ANYTHING -----------------------------------------------------");
        add("");
        add("   1. Save your BitLocker recovery key somewhere that is not this computer");
        add("      - a phone note, another machine. An encrypted disk touched without it");
        add("      is gone for good.");
        add("   2. Get a USB stick big enough for your files - the converter puts them");
        add("      there. You do NOT need an external hard drive. (Or keep Windows itself");
        add("      as the fallback: see \"Room to keep Windows\" above.)");
        add("   3. Write the ISO to a USB stick and boot it WITHOUT installing.");
        add("      Live mode runs the whole desktop from the USB and changes nothing.");
        add("   4. In that live session, test: Wi-Fi, sound through the SPEAKERS (not");
        add("      just headphones), screen brightness, and suspend/resume.");
        add("   5. Only then decide. Nothing is irreversible until you commit.");
        add("");
    }

    let age = distro_table_age(now);
    if age > 120 {
        add(&format!("  ! The distribution table in this scanner was last verified {age} days ago."));
        add("    Kernel versions move; confirm against the distribution release notes.");
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
