//! "Go back to Windows": the facts the way back starts from
//! (architecture.md, "The way back to Windows"; RISKS R30, R33; VALIDATION V11).
//!
//! Nothing here writes. It reads what Windows said about itself before it
//! was erased (harvest.windows_license, copied into the public summary at
//! first start) and what this computer shows Linux today, and says which
//! Windows to offer. The guided stick (stage 1: a downloaded ISO checked
//! against Microsoft's SHA-256 table, then installed by hand) left the
//! product with roadmap item 20 (decided 2026-10-12, the owner); the
//! walk-away (walkaway.rs) is the way back after an erase.
//!
//! The words the person sees are drafts until the owner approves them.

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::Read;

/// What Windows said about itself, from the public summary.
pub fn windows_before(summary: Option<&Value>) -> Value {
    match summary.and_then(|s| s.get("windows_before")) {
        Some(w) if w.get("result") == Some(&json!("read")) => w.clone(),
        Some(w) => json!({ "result": "unreadable", "reason": w.get("reason").cloned().unwrap_or(json!("Windows could not say")) }),
        None => json!({ "result": "unreadable", "reason": "this computer's setup record does not say which Windows it had" }),
    }
}

/// What this computer shows Linux, read-only. Every field may be null.
pub fn this_computer(root: &str) -> Value {
    let r = root.trim_end_matches('/');
    let read = |p: &str| std::fs::read_to_string(format!("{}/{}", r, p)).ok().map(|s| s.trim().to_string());
    let tpm = read("sys/class/tpm/tpm0/tpm_version_major").and_then(|s| s.parse::<u64>().ok());
    let uefi = std::path::Path::new(&format!("{}/sys/firmware/efi", r)).exists();
    // read_var drops the 4 attribute bytes; the one data byte is 1 when on
    let secure_boot = crate::efi::read_var(root, "SecureBoot").and_then(|d| d.first().map(|b| *b == 1));
    let mem_kib = read("proc/meminfo").and_then(|m| {
        m.lines().find_map(|l| l.strip_prefix("MemTotal:")).and_then(|v| v.trim().trim_end_matches("kB").trim().parse::<u64>().ok())
    });
    let cpu = read("proc/cpuinfo").and_then(|c| c.lines().find_map(|l| l.strip_prefix("model name").map(|v| v.trim_start_matches([' ', '\t', ':']).to_string())));
    // the maker's Windows key: presence only, never read (R13)
    let firmware_key = std::path::Path::new(&format!("{}/sys/firmware/acpi/tables/MSDM", r)).exists();
    json!({
        "arch": std::env::consts::ARCH,
        "uefi": uefi,
        "secure_boot": secure_boot,
        "tpm_version": tpm,
        "memory_gb": mem_kib.map(|k| ((k as f64) / 1024.0 / 1024.0 * 10.0).round() / 10.0),
        "processor": cpu,
        "firmware_key_present": firmware_key,
    })
}

/// Which Windows to offer, from the facts alone. Pure.
///
/// Windows 11 needs TPM 2.0, UEFI and a processor on Microsoft's list. We
/// check the first two; the list is Microsoft's, and its installer checks
/// it. So "11" is offered only when Windows 11 ran here before, or the two
/// facts Linux can see allow it (then the installer has the last word).
pub fn offer(before: &Value, pc: &Value) -> Value {
    let was = before.get("windows_version").and_then(Value::as_str);
    let tpm2 = pc.get("tpm_version").and_then(Value::as_u64) == Some(2);
    let uefi = pc.get("uefi") == Some(&json!(true));
    let x64 = pc.get("arch").and_then(Value::as_str) == Some("x86_64");
    if !x64 {
        return json!({ "windows": null, "why": "this computer is not a 64-bit PC processor; Microsoft's installers here are for those only" });
    }
    if was == Some("11") {
        return json!({ "windows": "11", "why": "Windows 11 ran on this computer before" });
    }
    if tpm2 && uefi {
        return json!({ "windows": "11", "fallback": "10",
            "why": "this computer has the security chip (TPM 2.0) and startup firmware (UEFI) Windows 11 needs; Microsoft's installer checks the processor, and if it says no, Windows 10 is the way back" });
    }
    let missing: Vec<&str> = [(!tpm2, "the security chip Windows 11 needs (TPM 2.0)"), (!uefi, "UEFI startup firmware")]
        .iter().filter(|(m, _)| *m).map(|(_, s)| *s).collect();
    json!({ "windows": "10", "why": format!("Linux cannot see {} on this computer", missing.join(" or ")) })
}

/// SHA-256 of a file, reporting progress as it goes.
pub fn sha256_file(path: &str, mut progress: impl FnMut(u64, u64)) -> Result<(String, u64), String> {
    let mut f = std::fs::File::open(path).map_err(|e| format!("{}: {}", path, e))?;
    let total = f.metadata().map(|m| m.len()).unwrap_or(0);
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 4 << 20];
    let mut done = 0u64;
    loop {
        let n = f.read(&mut buf).map_err(|e| format!("{}: {}", path, e))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        done += n as u64;
        progress(done, total);
    }
    let hex: String = h.finalize().iter().map(|b| format!("{:02x}", b)).collect();
    Ok((hex, done))
}

/// "Windows 10 Home" from the harvest, if it said. Pure.
pub fn edition_name(before: &Value, windows: Option<&str>) -> Option<String> {
    let e = before.get("edition_id").and_then(Value::as_str)?;
    let e = match e { "Core" => "Home", "CoreN" => "Home N", "CoreSingleLanguage" => "Home Single Language", "Professional" => "Pro", "ProfessionalN" => "Pro N", "Education" => "Education", other => other };
    Some(format!("Windows {} {}", windows.or(before.get("windows_version").and_then(Value::as_str)).unwrap_or(""), e).replace("  ", " "))
}

/// The first screen: the cost first, then the facts, then what to do.
/// DRAFT words, awaiting the owner's approval.
pub fn screen(before: &Value, pc: &Value, off: &Value) -> Value {
    let mut out = Vec::new();
    out.push(json!({ "heading": "What going back means", "lines": [
        "Going back puts a new, empty Windows on this computer.",
        "Linux and everything on it will be deleted: your files, your settings, your apps. Copy what you want to keep onto a USB drive or into online storage first.",
    ]}));
    let mut wl = Vec::new();
    if before["result"] == "read" {
        let name = match (before["windows_version"].as_str(), before["edition_id"].as_str()) {
            (Some(v), Some(_)) => edition_name(before, Some(v)).unwrap_or_else(|| format!("Windows {}", v)),
            (Some(v), None) => format!("Windows {}", v),
            _ => before["product_name"].as_str().unwrap_or("Windows").to_string(),
        };
        wl.push(format!("Before Linux, this computer had {}.", name));
        wl.push(match before["activated"].as_bool() {
            Some(true) => "It was activated (Microsoft's check that the copy is licensed).".to_string(),
            Some(false) => "It was not activated. A new Windows will ask for a product key, or run with reminders until you enter one.".to_string(),
            None => "Whether it was activated is not known.".to_string(),
        });
        if before["firmware_key_present"] == json!(true) {
            wl.push("Its licence key is stored in the computer itself. Windows' installer reads it from there, so you will probably not be asked for one.".to_string());
        }
    } else {
        wl.push(format!("Which Windows this computer had is not known: {}.", before["reason"].as_str().unwrap_or("it was not recorded")));
        if pc["firmware_key_present"] == json!(true) {
            wl.push("The computer itself holds a Windows licence key. Windows' installer reads it from there.".to_string());
        }
    }
    out.push(json!({ "heading": "Your Windows before", "lines": wl }));
    let w = off["windows"].as_str();
    let mut ol = Vec::new();
    match w {
        Some("11") => {
            ol.push("Windows 11.".to_string());
            ol.push(format!("Why: {}.", off["why"].as_str().unwrap_or("")));
        }
        Some("10") => {
            ol.push("Windows 10.".to_string());
            ol.push(format!("Why not Windows 11: {}.", off["why"].as_str().unwrap_or("")));
            ol.push("Windows 10 stopped getting free security updates in October 2025. Microsoft's paid extension for home users ends in October 2026. After that, it gets no security fixes at all.".to_string());
            ol.push("Going back is your choice. We want you to make it knowing this.".to_string());
        }
        _ => ol.push(format!("None that this program can make: {}.", off["why"].as_str().unwrap_or(""))),
    }
    out.push(json!({ "heading": "Which Windows", "lines": ol }));
    json!({ "title": "Go back to Windows", "sections": out, "close": "Close", "next_plain": "Next", "wizard": { "stick": stick_words() } })
}

/// The screen when Windows was kept (decided 2026-10-12, the owner): the way
/// back is the rollback, Windows first again, nothing deleted. The words are
/// `rollback::words()` (DRAFT, awaiting the owner's approval).
pub fn kept_screen(before: &Value) -> Value {
    let w = crate::rollback::words();
    let mut sections = vec![json!({ "heading": w["intro"]["heading"], "lines": w["intro"]["lines"] })];
    if before["result"] == "read" {
        if let Some(v) = before["windows_version"].as_str() {
            let name = edition_name(before, Some(v)).unwrap_or_else(|| format!("Windows {}", v));
            sections.push(json!({ "heading": "Your Windows", "lines": [format!("Still on this computer: {}, as it was when you converted.", name)] }));
        }
    }
    json!({ "title": "Go back to Windows", "kept": true, "sections": sections, "close": "Close", "next_plain": "Next", "words": w })
}

/// The words for the walk-away's stick page (the window draws them; it has
/// none of its own). DRAFT, awaiting the owner's approval.
pub fn stick_words() -> Value {
    json!({
        "heading": "The USB stick",
        "lines": [
            "Plug in a USB stick of 16 GB or more (Windows 11 does not fit on 8 GB). Everything on it will be deleted.",
            "Only a USB stick can be chosen. This computer's own drives, USB hard drives and the upgrade_ stick are never offered.",
        ],
        "none": "No USB stick that can be used is plugged in.",
        "refused_heading": "Not offered:",
        "type_prompt": "To confirm, type the stick's name exactly as shown:",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pc(tpm: Option<u64>, uefi: bool) -> Value {
        json!({ "arch": "x86_64", "uefi": uefi, "tpm_version": tpm, "firmware_key_present": true })
    }

    #[test]
    fn reads_a_fake_computer() {
        let r = std::env::temp_dir().join(format!("goback-pc-{}", std::process::id()));
        let w = |p: &str, b: &[u8]| { let f = r.join(p); std::fs::create_dir_all(f.parent().unwrap()).unwrap(); std::fs::write(f, b).unwrap(); };
        w("sys/firmware/efi/efivars/SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c", &[6, 0, 0, 0, 1]);
        w("sys/class/tpm/tpm0/tpm_version_major", b"2\n");
        w("sys/firmware/acpi/tables/MSDM", b"x");
        w("proc/meminfo", b"MemTotal:       12058624 kB\n");
        w("proc/cpuinfo", b"processor\t: 0\nmodel name\t: Intel(R) Core(TM) i7-8550U CPU @ 1.80GHz\n");
        let pc = this_computer(r.to_str().unwrap());
        assert_eq!(pc["secure_boot"], true);
        assert_eq!(pc["uefi"], true);
        assert_eq!(pc["tpm_version"], 2);
        assert_eq!(pc["firmware_key_present"], true);
        assert_eq!(pc["memory_gb"], 11.5);
        assert_eq!(pc["processor"], "Intel(R) Core(TM) i7-8550U CPU @ 1.80GHz");
        std::fs::remove_dir_all(&r).unwrap();
    }

    #[test]
    fn windows_11_before_means_11() {
        let o = offer(&json!({ "result": "read", "windows_version": "11" }), &pc(None, true));
        assert_eq!(o["windows"], "11");
    }

    #[test]
    fn no_tpm2_means_10_and_says_why() {
        let o = offer(&json!({ "result": "read", "windows_version": "10" }), &pc(Some(1), true));
        assert_eq!(o["windows"], "10");
        assert!(o["why"].as_str().unwrap().contains("TPM 2.0"));
        let o = offer(&json!({ "result": "unreadable" }), &pc(None, false));
        assert_eq!(o["windows"], "10");
        assert!(o["why"].as_str().unwrap().contains("(TPM 2.0) or UEFI"));
    }

    #[test]
    fn tpm2_and_uefi_offer_11_with_10_behind_it() {
        let o = offer(&json!({ "result": "unreadable" }), &pc(Some(2), true));
        assert_eq!(o["windows"], "11");
        assert_eq!(o["fallback"], "10");
    }

    #[test]
    fn not_a_pc_processor_gets_nothing() {
        let mut p = pc(Some(2), true);
        p["arch"] = json!("aarch64");
        assert!(offer(&json!({}), &p)["windows"].is_null());
    }

    #[test]
    fn hashing_matches_a_known_value() {
        let dir = std::env::temp_dir().join(format!("goback-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("abc.iso");
        std::fs::write(&p, b"abc").unwrap();
        let (h, n) = sha256_file(p.to_str().unwrap(), |_, _| {}).unwrap();
        assert_eq!(h, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(n, 3);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_screen_says_the_cost_first_and_the_windows_10_warning() {
        let before = json!({ "result": "read", "windows_version": "10", "edition_id": "Core", "activated": true, "firmware_key_present": true });
        let s = screen(&before, &pc(Some(1), true), &offer(&before, &pc(Some(1), true)));
        let secs = s["sections"].as_array().unwrap();
        assert_eq!(secs[0]["heading"], "What going back means");
        assert!(secs[0]["lines"][1].as_str().unwrap().contains("will be deleted"));
        assert_eq!(secs[1]["lines"][0], "Before Linux, this computer had Windows 10 Home.");
        let which = secs[2]["lines"].to_string();
        assert!(which.contains("October 2025") && which.contains("October 2026"));
        // the guided stick's download step is gone (roadmap item 20)
        assert_eq!(secs.len(), 3);
        assert!(!s.to_string().contains("microsoft.com/software-download"));
    }

    #[test]
    fn an_edition_from_windows_10_on_a_machine_offered_11_names_11() {
        let before = json!({ "result": "read", "windows_version": "10", "edition_id": "Professional" });
        assert_eq!(edition_name(&before, Some("11")).unwrap(), "Windows 11 Pro");
    }

    #[test]
    fn a_key_never_appears_on_the_screen() {
        // the schema already refuses key shapes; the screen only prints fixed words and names
        let before = json!({ "result": "read", "windows_version": "11", "edition_id": "Professional", "activated": true, "firmware_key_present": true, "channel": "OEM:DM" });
        let s = screen(&before, &pc(Some(2), true), &offer(&before, &pc(Some(2), true))).to_string();
        assert!(s.contains("Windows 11 Pro"));
        assert!(!regex_like_key(&s));
    }

    fn regex_like_key(s: &str) -> bool {
        s.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-')).any(|w| {
            let p: Vec<&str> = w.split('-').collect();
            p.len() == 5 && p.iter().all(|x| x.len() == 5)
        })
    }
}
