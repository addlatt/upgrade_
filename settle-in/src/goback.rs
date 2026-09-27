//! "Go back to Windows", stage 1: the facts and the download check
//! (architecture.md, "The way back to Windows"; RISKS R30; VALIDATION V11).
//!
//! Nothing here writes. It reads what Windows said about itself before it
//! was erased (harvest.windows_license, copied into the public summary at
//! first start), what this computer shows Linux today, and checks an
//! installer file the person downloaded from Microsoft against the SHA-256
//! table Microsoft prints on its own download page (data/windows-media.json,
//! refreshed by tools/refresh-windows-media.py). A file that is not in that
//! table is refused: we never guess that a file is Windows.
//!
//! The words the person sees are drafts until the owner approves them.

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::Read;

pub const MEDIA: &str = include_str!("../data/windows-media.json");
pub const PAGE_10: &str = "https://www.microsoft.com/software-download/windows10ISO";
pub const PAGE_11: &str = "https://www.microsoft.com/software-download/windows11";

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

/// The media table, parsed.
pub fn media() -> Vec<Value> {
    serde_json::from_str::<Value>(MEDIA).ok().and_then(|d| d["media"].as_array().cloned()).unwrap_or_default()
}

pub fn media_read_utc() -> String {
    serde_json::from_str::<Value>(MEDIA).ok().and_then(|d| d["read_utc"].as_str().map(str::to_string)).unwrap_or_default()
}

/// Judge a file by its hash. Pure.
pub fn judge(sha256: &str, size: u64, table: &[Value], want: Option<&str>) -> Value {
    let sha = sha256.to_ascii_lowercase();
    let hit = table.iter().find(|m| m["sha256"].as_str() == Some(sha.as_str()));
    let Some(m) = hit else {
        return json!({ "result": "refused", "sha256": sha, "size": size,
            "why": "this file is not one of the Windows installers Microsoft lists on its download page, so it is not used" });
    };
    if m["arch"] != json!("x64") {
        return json!({ "result": "refused", "sha256": sha, "size": size, "media": m,
            "why": "this is the 32-bit Windows installer; this computer needs the 64-bit one" });
    }
    if let Some(w) = want {
        if m["windows"].as_str() != Some(w) {
            return json!({ "result": "refused", "sha256": sha, "size": size, "media": m,
                "why": format!("this is the Windows {} installer, but Windows {} is the one for this computer", m["windows"].as_str().unwrap_or("?"), w) });
        }
    }
    json!({ "result": "verified", "sha256": sha, "size": size, "media": m })
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

/// ISO files in the person's Downloads folder, newest first.
pub fn find_downloads(home: &str) -> Vec<Value> {
    let mut v: Vec<(std::time::SystemTime, Value)> = Vec::new();
    for dir in ["Downloads", ""] {
        let d = format!("{}/{}", home.trim_end_matches('/'), dir);
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()).map(|x| x.eq_ignore_ascii_case("iso")) != Some(true) {
                continue;
            }
            if let Ok(m) = e.metadata() {
                v.push((m.modified().unwrap_or(std::time::UNIX_EPOCH), json!({ "path": p.to_string_lossy(), "size": m.len() })));
            }
        }
    }
    v.sort_by(|a, b| b.0.cmp(&a.0));
    v.into_iter().map(|(_, j)| j).collect()
}

/// "Windows 10 Home" from the harvest, if it said. Pure.
pub fn edition_name(before: &Value, windows: Option<&str>) -> Option<String> {
    let e = before.get("edition_id").and_then(Value::as_str)?;
    let e = match e { "Core" => "Home", "CoreN" => "Home N", "CoreSingleLanguage" => "Home Single Language", "Professional" => "Pro", "ProfessionalN" => "Pro N", "Education" => "Education", other => other };
    Some(format!("Windows {} {}", windows.or(before.get("windows_version").and_then(Value::as_str)).unwrap_or(""), e).replace("  ", " "))
}

fn gb(b: u64) -> String {
    format!("{:.1} GB", b as f64 / 1e9)
}

/// The first screen: the cost first, then the facts, then what to do.
/// DRAFT words, awaiting the owner's approval.
pub fn screen(before: &Value, pc: &Value, off: &Value) -> Value {
    let mut out = Vec::new();
    out.push(json!({ "heading": "What going back means", "lines": [
        "Going back puts a new, empty Windows on this computer.",
        "Linux and everything on it will be deleted: your files, your settings, your apps. Copy what you want to keep onto a USB drive or into online storage first.",
        "This program does not delete anything. It makes a Windows installer stick; you start the installer yourself, and you can stop before it deletes anything.",
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
    if w.is_some() {
        let page = if w == Some("10") { PAGE_10 } else { PAGE_11 };
        out.push(json!({ "heading": "Step 1: download Windows from Microsoft", "lines": [
            "Open Microsoft's download page, choose the edition and your language, and download the file (about 6 to 9 GB).",
            format!("Microsoft's page: {}", page),
            "We never hand out Windows ourselves. This program checks the file against the numbers Microsoft publishes on that page before using it.",
        ], "link": page }));
    }
    json!({ "title": "Go back to Windows", "sections": out, "close": "Close", "next": "I have downloaded it", "wizard": wizard(before, off["windows"].as_str()) })
}

/// The words for the later screens (the window draws them; it has none of
/// its own). DRAFT, awaiting the owner's approval.
pub fn wizard(before: &Value, windows: Option<&str>) -> Value {
    let edition = edition_name(before, windows);
    json!({
        "file": {
            "heading": "Step 2: check the file",
            "lines": ["Choose the file you downloaded. This program checks it against Microsoft's numbers; that takes a minute or two."],
            "none": "No Windows file found in your Downloads folder yet. When the download has finished, press Look again.",
            "look_again": "Look again",
            "check": "Check this file",
        },
        "stick": {
            "heading": "Step 3: the USB stick",
            "lines": [
                "Plug in a USB stick of 16 GB or more (Windows 11 does not fit on 8 GB). Everything on it will be deleted.",
                "Only a USB stick can be chosen. This computer's own drives, USB hard drives and the upgrade_ stick are never offered.",
            ],
            "none": "No USB stick that can be used is plugged in.",
            "refused_heading": "Not offered:",
            "type_prompt": "To confirm, type the stick's name exactly as shown:",
            "write": "Delete everything on this stick and make the Windows installer",
            "note": "You will be asked for your password. It takes 10 to 30 minutes; the stick is checked at the end.",
        },
        "done": {
            "heading": "The Windows installer stick is ready",
            "lines": [
                "Nothing on this computer has changed yet. When you are ready:",
                "1. Copy everything you want to keep off this computer. Going back deletes it.",
                "2. Leave the stick in and restart. As the computer starts, press the key for its startup menu (often F12, F9, Esc or F2; the maker's logo screen usually says which) and choose the USB stick.",
                "3. Windows Setup starts. If it asks for a product key, choose \"I don't have a product key\" (Setup's own advice when reinstalling).",
                match &edition {
                    Some(e) => format!("4. When it asks which edition, choose {}: the one this computer had.", e),
                    None => "4. When it asks which edition, choose the one this computer had (most home computers had Home).".to_string(),
                },
                "5. Choose \"Custom: Install Windows only\". Delete every partition on this computer's drives: this is the step that deletes Linux and your files. Never delete the one called WINSETUP: that is this stick. Then choose the empty space and click Next.",
                "6. When Windows has started, you are done. The stick can go back to ordinary use.",
                "Until step 5, you can stop: take the stick out and restart, and Linux starts as before.",
            ],
        },
        "failed": "The stick was not made: ",
        "failed_changed": "What was on the stick has been deleted, but it does not hold a working installer. Nothing on this computer changed.",
        "failed_unchanged": "Nothing was written: the stick and this computer are as they were.",
    })
}

pub fn check_report(path: &str, want: Option<&str>, table: &[Value]) -> Value {
    let (sha, size) = match sha256_file(path, |d, t| {
        if t > 0 && d % (256 << 20) < (4 << 20) {
            eprintln!("{}", json!({ "progress": d, "total": t }));
        }
    }) {
        Ok(x) => x,
        Err(e) => return json!({ "result": "refused", "why": format!("the file could not be read ({})", e) }),
    };
    let mut j = judge(&sha, size, table, want);
    j["path"] = json!(path);
    j["table_read_utc"] = json!(media_read_utc());
    if j["result"] == "verified" {
        j["line"] = json!(format!("Checked: this is Microsoft's Windows {} installer ({}, {}).",
            j["media"]["windows"].as_str().unwrap_or("?"), j["media"]["language"].as_str().unwrap_or("?"), gb(size)));
    }
    j
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pc(tpm: Option<u64>, uefi: bool) -> Value {
        json!({ "arch": "x86_64", "uefi": uefi, "tpm_version": tpm, "firmware_key_present": true })
    }

    #[test]
    fn the_table_is_real_and_has_both() {
        let t = media();
        assert!(t.len() >= 60, "{}", t.len());
        assert!(t.iter().any(|m| m["windows"] == "10" && m["arch"] == "x64"));
        assert!(t.iter().any(|m| m["windows"] == "11" && m["arch"] == "x64"));
        for m in &t {
            assert_eq!(m["sha256"].as_str().unwrap().len(), 64);
        }
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
    fn a_file_not_in_microsofts_table_is_refused() {
        let j = judge(&"ab".repeat(32), 5, &media(), None);
        assert_eq!(j["result"], "refused");
    }

    #[test]
    fn microsofts_file_is_verified_and_the_wrong_one_refused() {
        let t = media();
        let w11 = t.iter().find(|m| m["windows"] == "11" && m["language"] == "English").unwrap();
        let sha = w11["sha256"].as_str().unwrap().to_ascii_uppercase();
        assert_eq!(judge(&sha, 7, &t, Some("11"))["result"], "verified");
        assert_eq!(judge(&sha, 7, &t, None)["result"], "verified");
        let j = judge(&sha, 7, &t, Some("10"));
        assert_eq!(j["result"], "refused");
        assert!(j["why"].as_str().unwrap().contains("Windows 11 installer"));
        let w10_32 = t.iter().find(|m| m["windows"] == "10" && m["arch"] == "32-bit").unwrap();
        let j = judge(w10_32["sha256"].as_str().unwrap(), 7, &t, None);
        assert_eq!(j["result"], "refused");
        assert!(j["why"].as_str().unwrap().contains("32-bit"));
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
        assert_eq!(find_downloads(dir.to_str().unwrap()).len(), 1);
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
        assert_eq!(secs[3]["link"], PAGE_10);
    }

    #[test]
    fn the_done_steps_name_the_edition_and_spare_the_stick() {
        let before = json!({ "result": "read", "windows_version": "11", "edition_id": "Core" });
        let w = wizard(&before, Some("11")).to_string();
        assert!(w.contains("choose Windows 11 Home: the one this computer had"));
        assert!(w.contains("Never delete the one called WINSETUP"));
        // an edition from Windows 10 on a machine now offered 11 names 11
        let before = json!({ "result": "read", "windows_version": "10", "edition_id": "Professional" });
        assert_eq!(edition_name(&before, Some("11")).unwrap(), "Windows 11 Pro");
        assert!(wizard(&json!({ "result": "unreadable" }), Some("11")).to_string().contains("most home computers had Home"));
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
