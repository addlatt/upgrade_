//! "Go back to Windows", 100% managed (decided 2026-09-29, the owner; RISKS
//! R33; architecture.md, "The way back to Windows", stage 2).
//!
//! Everything here happens in Linux, before the restart, and changes
//! nothing on this computer's drives. In order:
//!
//!   catalog   Microsoft's own catalog (the one its Media Creation Tool
//!             reads), over HTTPS: which file, its size and its SHA-1;
//!   download  that file (Microsoft's host refuses HTTPS, 2026-09-29), kept
//!             only if its size and SHA-1 match the catalog;
//!   tree      the stick's files built from it with wimlib: Setup's files,
//!             boot.wim with the gate inside and started first, and the one
//!             edition this computer had as install.wim;
//!   drives    the drives to erase, by serial, world-wide name and exact
//!             size: the one holding `/`, and the one holding `/home`;
//!   job       upgrade_/go-back.json for the gate, which checks it all again.
//!
//! The stick is written by stickwrite (R16's rules). The commit line is the
//! gate's countdown, on the stick, after the restart (settle-in/gate).

use serde_json::{json, Value};
use std::process::Command;

pub const CATALOG_11: &str = "https://go.microsoft.com/fwlink/?linkid=2156292";
pub const CATALOG_10: &str = "https://go.microsoft.com/fwlink/?LinkId=841361";
pub const SENTENCE: &str = "I confirm that Linux and everything on this computer will be deleted and nothing will be kept";
const PATH_ENV: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

fn run(prog: &str, args: &[&str]) -> Result<String, String> {
    let o = Command::new(prog).args(args).env("PATH", PATH_ENV).env("LC_ALL", "C").output().map_err(|e| format!("{} could not start ({})", prog, e))?;
    if o.status.success() {
        Ok(String::from_utf8_lossy(&o.stdout).to_string())
    } else {
        Err(format!("{} failed: {}", prog, String::from_utf8_lossy(&o.stderr).trim()))
    }
}

// ---------------------------------------------------------------- the catalog

/// One Windows file from the catalog. Pure.
pub fn pick(xml: &str, language: &str, edition: &str) -> Result<Value, String> {
    let doc = roxmltree::Document::parse(xml).map_err(|e| format!("the catalog could not be read ({})", e))?;
    let text = |n: &roxmltree::Node, k: &str| n.children().find(|c| c.has_tag_name(k)).and_then(|c| c.text()).unwrap_or("").trim().to_string();
    for f in doc.descendants().filter(|n| n.has_tag_name("File")) {
        if text(&f, "Architecture") == "x64" && text(&f, "LanguageCode").eq_ignore_ascii_case(language) && text(&f, "Edition") == edition {
            let url = text(&f, "FilePath");
            let size: u64 = text(&f, "Size").parse().unwrap_or(0);
            let sha1 = text(&f, "Sha1").to_ascii_lowercase();
            if !url.starts_with("http://dl.delivery.mp.microsoft.com/") && !url.starts_with("https://dl.delivery.mp.microsoft.com/") {
                return Err(format!("the catalog points outside Microsoft's download host ({})", url));
            }
            if size == 0 || sha1.len() != 40 || !sha1.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err("the catalog's entry has no size or checksum".into());
            }
            return Ok(json!({ "file": text(&f, "FileName"), "url": url, "size": size, "sha1": sha1, "edition": edition, "language": text(&f, "LanguageCode") }));
        }
    }
    Err(format!("Microsoft's catalog has no {} file for {}", edition, language))
}

/// The catalog's edition name for what this computer had. Pure.
/// Home and Pro are in the consumer file; anything else falls back to Home.
pub fn catalog_edition(before: &Value) -> &'static str {
    match before["edition_id"].as_str().unwrap_or("") {
        "Professional" => "Professional",
        "ProfessionalN" => "ProfessionalN",
        "CoreN" => "CoreN",
        "CoreSingleLanguage" => "CoreSingleLanguage",
        "Education" => "Education",
        _ => "Core",
    }
}

/// Fetch Microsoft's catalog over HTTPS and unpack it with cabextract
/// (static, carried with settle-in: the cabinet is LZX-compressed, 2026-09-29).
pub fn fetch_catalog(windows: &str, dir: &str, cabextract: &str) -> Result<String, String> {
    let url = if windows == "10" { CATALOG_10 } else { CATALOG_11 };
    let cab = format!("{}/products-win{}.cab", dir, windows);
    // HTTPS only, redirects allowed only to HTTPS (go.microsoft.com -> download.microsoft.com)
    run("curl", &["--fail", "--silent", "--show-error", "--location", "--proto", "=https", "--proto-redir", "=https", "--max-time", "120", "-o", &cab, url])?;
    unpack_catalog(&cab, dir, cabextract)
}

/// products.xml out of the cabinet; cabextract checks the cabinet's own checksums.
pub fn unpack_catalog(cab: &str, dir: &str, cabextract: &str) -> Result<String, String> {
    let out = format!("{}/catalog", dir);
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {}", out, e))?;
    run(cabextract, &["-q", "-d", &out, "-F", "products.xml", cab])?;
    std::fs::read_to_string(format!("{}/products.xml", out)).map_err(|_| "the catalog did not contain products.xml".to_string())
}

/// SHA-1 of a file (coreutils' sha1sum: every Linux has it).
pub fn sha1_file(path: &str) -> Result<String, String> {
    Ok(run("sha1sum", &[path])?.split_whitespace().next().unwrap_or("").to_ascii_lowercase())
}

/// Download (resuming a partial file) and keep it only if it is the catalog's.
pub fn download(entry: &Value, dir: &str) -> Result<String, String> {
    let name = entry["file"].as_str().unwrap_or("windows.esd");
    if name.contains('/') || name.starts_with('.') {
        return Err("the catalog's file name is not a plain name".into());
    }
    let (dst, part) = (format!("{}/{}", dir, name), format!("{}/{}.part", dir, name));
    let size = entry["size"].as_u64().unwrap_or(0);
    let ok = |p: &str| std::fs::metadata(p).map(|m| m.len() == size).unwrap_or(false) && sha1_file(p).ok().as_deref() == entry["sha1"].as_str();
    if ok(&dst) {
        return Ok(dst);
    }
    let url = entry["url"].as_str().unwrap_or("");
    // curl in the background; its progress is the file's size, said every 2 s
    let mut child = Command::new("curl")
        .args(["--fail", "--silent", "--show-error", "--location", "--proto", "=http,https", "--retry", "5", "-C", "-", "-o", &part, url])
        .env("PATH", PATH_ENV).stderr(std::process::Stdio::piped()).spawn().map_err(|e| format!("curl could not start ({})", e))?;
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) => {
                let got = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
                eprintln!("{}", json!({ "step": "download", "progress": got, "total": size }));
                std::thread::sleep(std::time::Duration::from_secs(2));
            }
            Err(e) => return Err(format!("the download stopped ({})", e)),
        }
    };
    if !status.success() {
        let mut msg = String::new();
        if let Some(mut e) = child.stderr.take() {
            let _ = std::io::Read::read_to_string(&mut e, &mut msg);
        }
        return Err(format!("the download from Microsoft stopped ({}); running it again carries on where it stopped", msg.trim()));
    }
    if std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0) != size {
        return Err("the download is not the size Microsoft's catalog says".into());
    }
    if sha1_file(&part)?.as_str() != entry["sha1"].as_str().unwrap_or("") {
        let _ = std::fs::remove_file(&part);
        return Err("the download is not the file Microsoft's catalog names (its checksum differs); it was deleted".into());
    }
    std::fs::rename(&part, &dst).map_err(|e| format!("{}: {}", dst, e))?;
    Ok(dst)
}

// ---------------------------------------------------------------- the stick's files

/// The image index of an edition inside the file, from `wimlib-imagex info`. Pure.
pub fn edition_index(info: &str, edition: &str) -> Option<u32> {
    let mut idx: Option<u32> = None;
    for l in info.lines() {
        if let Some(v) = l.strip_prefix("Index:") {
            idx = v.trim().parse().ok();
        } else if let Some(v) = l.strip_prefix("Edition ID:") {
            if v.trim() == edition {
                return idx;
            }
        }
    }
    None
}

pub const WINPESHL: &str = "[LaunchApps]\r\n%SYSTEMROOT%\\System32\\wpeinit.exe\r\n%SYSTEMROOT%\\System32\\upgrade-gate.exe\r\n";

/// Build the stick's files in `tree` (a new folder) from the .esd.
pub fn build_tree(esd: &str, edition: &str, account: &str, tree: &str, gate: &str, wimlib: &str) -> Result<Value, String> {
    if std::path::Path::new(tree).exists() {
        return Err(format!("{} already exists", tree));
    }
    if !std::path::Path::new(gate).exists() {
        return Err(format!("the gate program is missing ({})", gate));
    }
    let info = run(wimlib, &["info", esd])?;
    let index = edition_index(&info, edition).ok_or_else(|| format!("the Windows file has no {} edition", edition))?;
    if edition_index(&info, "WindowsPE").is_none() || !info.contains("Index:                  3") {
        return Err("the Windows file is not laid out as Microsoft's setup file (Setup, WinPE, Setup's WinPE, editions)".into());
    }
    std::fs::create_dir_all(tree).map_err(|e| format!("{}: {}", tree, e))?;
    run(wimlib, &["apply", esd, "1", tree])?;
    let boot = format!("{}/sources/boot.wim", tree);
    run(wimlib, &["export", esd, "2", &boot, "--compress=LZX"])?;
    run(wimlib, &["export", esd, "3", &boot, "--boot", "--compress=LZX"])?;
    let ini = format!("{}.winpeshl.ini", tree);
    std::fs::write(&ini, WINPESHL).map_err(|e| e.to_string())?;
    run(wimlib, &["update", &boot, "2", &format!("--command=add {} /Windows/System32/upgrade-gate.exe", gate)])?;
    run(wimlib, &["update", &boot, "2", &format!("--command=add {} /Windows/System32/winpeshl.ini", ini)])?;
    let _ = std::fs::remove_file(&ini);
    run(wimlib, &["export", esd, &index.to_string(), &format!("{}/sources/install.wim", tree), "--compress=LZX"])?;
    // no SetupComplete.cmd: Windows skips it on computers with a maker's key
    // (the Aspire, 2026-10-02); the gate's answer file runs first-sign-in
    // commands instead, which run everywhere (R33)
    let _ = account;
    std::fs::create_dir_all(format!("{}/upgrade_", tree)).map_err(|e| e.to_string())?;
    Ok(json!({ "edition": edition, "esd_index": index }))
}

/// A world-wide name only if it is one: "0x" / "naa." / "eui." and 16+ hex
/// digits. Linux falls back to "t10.ATA <model> <serial>" text for SATA drives
/// without one (the Aspire's SSD, 2026-09-29): that is not an identity. Pure.
pub fn clean_wwn(s: &str) -> String {
    let t = s.trim().to_ascii_lowercase();
    let body = t.strip_prefix("0x").or_else(|| t.strip_prefix("naa.")).or_else(|| t.strip_prefix("eui."));
    match body {
        Some(b) if b.len() >= 16 && b.chars().all(|c| c.is_ascii_hexdigit()) => t,
        _ => String::new(),
    }
}

/// Free bytes where the files are prepared.
pub fn free_bytes(dir: &str) -> u64 {
    let c = std::ffi::CString::new(dir).unwrap_or_default();
    // SAFETY: statvfs fills the struct it is given; c is a valid C string.
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return 0;
    }
    st.f_bavail as u64 * st.f_frsize as u64
}

/// The same rule the gate applies (settle-in/gate, logic::account_ok). Pure.
pub fn account_ok(name: &str) -> bool {
    let reserved = ["administrator", "guest", "defaultaccount", "wdagutilityaccount", "system", "none"];
    !name.is_empty()
        && name.len() <= 20
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
        && !name.ends_with('.')
        && !reserved.contains(&name.to_ascii_lowercase().as_str())
}

// ---------------------------------------------------------------- Wi-Fi
// Decided 2026-10-02 (the owner): every OS switch carries the saved networks
// and their passwords (architecture.md, "What migrates"). Here: Linux's
// NetworkManager keyfiles -> Windows Wi-Fi profiles on the stick, added at the
// first sign-in by the stick's own script, then deleted from the stick.

/// A NetworkManager keyfile as sections of key=value. Pure.
fn ini(text: &str) -> std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>> {
    let mut m: std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>> = Default::default();
    let mut sec = String::new();
    for l in text.lines() {
        let l = l.trim();
        if l.starts_with('#') || l.is_empty() {
            continue;
        }
        if l.starts_with('[') && l.ends_with(']') {
            sec = l[1..l.len() - 1].to_string();
        } else if let Some((k, v)) = l.split_once('=') {
            m.entry(sec.clone()).or_default().insert(k.trim().to_string(), v.to_string());
        }
    }
    m
}

fn xml_esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

/// One saved network: the Windows profile to carry, or why it is not
/// carried. Pure. The password is only in the profile, never in the answer.
pub fn wifi_profile(keyfile: &str) -> Option<(Value, Option<String>)> {
    let k = ini(keyfile);
    let conn = k.get("connection")?;
    let kind = conn.get("type").map(String::as_str).unwrap_or("");
    if kind != "wifi" && kind != "802-11-wireless" {
        return None;
    }
    let wifi = k.get("wifi").or_else(|| k.get("802-11-wireless"))?;
    let ssid = wifi.get("ssid")?.clone();
    let name = conn.get("id").cloned().unwrap_or_else(|| ssid.clone());
    let info = |carried: bool, why: &str| json!({ "name": ssid, "connection": name, "carried": carried, "why": why });
    if wifi.get("mode").map(|m| m != "infrastructure").unwrap_or(false) {
        return Some((info(false, "not an ordinary network (hotspot or ad-hoc)"), None));
    }
    let sec = k.get("wifi-security").or_else(|| k.get("802-11-wireless-security"));
    let mgmt = sec.and_then(|s| s.get("key-mgmt")).map(String::as_str).unwrap_or("open");
    let (auth, enc, psk) = match mgmt {
        "open" => ("open", "none", None),
        "wpa-psk" | "sae" => {
            let Some(pw) = sec.and_then(|s| s.get("psk")) else {
                return Some((info(false, "Linux keeps its password in a keyring, not in a file this program can read"), None));
            };
            (if mgmt == "sae" { "WPA3SAE" } else { "WPA2PSK" }, "AES", Some(pw.clone()))
        }
        "none" => return Some((info(false, "an old WEP network, which Windows no longer sets up this way"), None)),
        _ => return Some((info(false, "an enterprise network (a company or school sign-in)"), None)),
    };
    let hex: String = ssid.bytes().map(|b| format!("{:02X}", b)).collect();
    let hidden = wifi.get("hidden").map(|h| h == "true").unwrap_or(false);
    let auto = conn.get("autoconnect").map(|a| a != "false").unwrap_or(true);
    let key = psk.map(|pw| format!("<sharedKey><keyType>passPhrase</keyType><protected>false</protected><keyMaterial>{}</keyMaterial></sharedKey>", xml_esc(&pw))).unwrap_or_default();
    let xml = format!(
        "<?xml version=\"1.0\"?>\r\n<WLANProfile xmlns=\"http://www.microsoft.com/networking/WLAN/profile/v1\"><name>{n}</name><SSIDConfig><SSID><hex>{hex}</hex><name>{n}</name></SSID><nonBroadcast>{hidden}</nonBroadcast></SSIDConfig><connectionType>ESS</connectionType><connectionMode>{mode}</connectionMode><MSM><security><authEncryption><authentication>{auth}</authentication><encryption>{enc}</encryption><useOneX>false</useOneX></authEncryption>{key}</security></MSM></WLANProfile>\r\n",
        n = xml_esc(&ssid), hex = hex, hidden = hidden, mode = if auto { "auto" } else { "manual" }, auth = auth, enc = enc, key = key);
    Some((info(true, if auto { "connects by itself" } else { "set up; connect from the network menu" }), Some(xml)))
}

/// The stick's first-sign-in script (run by the gate's answer file, with the
/// stick's drive as %1): add every profile for all users, log what Windows
/// said (names only), delete the profiles from the stick. Pure.
pub const FIRST_LOGON_CMD: &str = "@echo off\r\nrem written by settle-in: Wi-Fi from Linux (decided 2026-10-02); passwords leave this stick here\r\nset S=%1\r\nif exist %S%\\upgrade_\\wifi (\r\n  for %%f in (%S%\\upgrade_\\wifi\\*.xml) do netsh wlan add profile filename=\"%%f\" user=all >> %S%\\upgrade_\\go-back-wifi.log 2>&1\r\n  rmdir /s /q %S%\\upgrade_\\wifi\r\n)\r\nif exist %S%\\upgrade_\\wifi (echo wifi folder NOT removed >> %S%\\upgrade_\\go-back-wifi.log) else (echo wifi folder removed >> %S%\\upgrade_\\go-back-wifi.log)\r\n";

/// Read Linux's saved networks (root) and write the profiles into the
/// stick's files. Returns the list for the job: names and reasons, never a password.
pub fn carry_wifi(root: &str, tree: &str) -> Result<Vec<Value>, String> {
    let dir = format!("{}/etc/NetworkManager/system-connections", root.trim_end_matches('/'));
    let mut names: Vec<String> = std::fs::read_dir(&dir).map(|d| d.flatten().map(|e| e.path().to_string_lossy().to_string()).collect()).unwrap_or_default();
    names.sort();
    let out = format!("{}/upgrade_/wifi", tree);
    let mut list = Vec::new();
    let mut n = 0;
    for f in names {
        let Ok(text) = std::fs::read_to_string(&f) else { continue };
        let Some((info, xml)) = wifi_profile(&text) else { continue };
        if let Some(x) = xml {
            std::fs::create_dir_all(&out).map_err(|e| format!("{}: {}", out, e))?;
            n += 1;
            std::fs::write(format!("{}/{:02}.xml", out, n), x).map_err(|e| e.to_string())?;
        }
        list.push(info);
    }
    std::fs::write(format!("{}/upgrade_/go-back-first-logon.cmd", tree), FIRST_LOGON_CMD).map_err(|e| e.to_string())?;
    Ok(list)
}

// ---------------------------------------------------------------- the drives

fn read(p: &str) -> Option<String> {
    std::fs::read_to_string(p).ok().map(|s| s.trim().to_string())
}

/// The whole disks under a block device (major:minor), through partitions,
/// device-mapper and RAID layers. Pure over a sysfs tree at `root`.
pub fn disks_under(root: &str, devnum: &str) -> Vec<String> {
    let r = root.trim_end_matches('/');
    let Ok(p) = std::fs::canonicalize(format!("{}/sys/dev/block/{}", r, devnum)) else { return vec![] };
    let mut out = Vec::new();
    let mut stack = vec![p];
    while let Some(p) = stack.pop() {
        let slaves: Vec<_> = std::fs::read_dir(p.join("slaves")).map(|d| d.flatten().map(|e| e.path()).collect()).unwrap_or_default();
        if !slaves.is_empty() {
            for s in slaves {
                if let Ok(c) = std::fs::canonicalize(&s) {
                    stack.push(c);
                }
            }
        } else if p.join("partition").exists() {
            if let Some(parent) = p.parent().and_then(|x| x.file_name()) {
                out.push(parent.to_string_lossy().to_string());
            }
        } else if let Some(n) = p.file_name() {
            out.push(n.to_string_lossy().to_string());
        }
    }
    out.sort();
    out.dedup();
    out
}

/// The drives to erase: the one holding `/`, then the one holding `/home`
/// if it is another. Refuses layouts it cannot name exactly. Pure over `root`.
pub fn drives(root: &str, disks: &Value) -> Result<Vec<Value>, String> {
    let r = root.trim_end_matches('/');
    let mi = read(&format!("{}/proc/self/mountinfo", r)).unwrap_or_default();
    let devnum_of = |mp: &str| mi.lines().find_map(|l| {
        let f: Vec<&str> = l.split(' ').collect();
        (f.get(4) == Some(&mp)).then(|| f.get(2).map(|s| s.to_string())).flatten()
    });
    let root_dev = devnum_of("/").ok_or("what holds / could not be found")?;
    let sys = disks_under(r, &root_dev);
    let home = devnum_of("/home").map(|d| disks_under(r, &d)).unwrap_or_default();
    if sys.len() != 1 {
        return Err(format!("Linux here is spread over {} drives; this program only erases a system on one drive", sys.len()));
    }
    let mut names = vec![("system", sys[0].clone())];
    match home.as_slice() {
        [] => {}
        [h] if *h == sys[0] => {}
        [h] => names.push(("second", h.clone())),
        _ => return Err("/home is spread over several drives; this program does not erase that".into()),
    }
    let mut out = Vec::new();
    for (role, n) in names {
        let d = disks.as_array().into_iter().flatten().find(|d| d["name"] == json!(n)).ok_or_else(|| format!("{} is not in the disk list", n))?;
        if d["usb"] == json!(true) || d["removable"] == json!(true) {
            return Err(format!("the {} drive ({}) is a USB or removable drive; Linux is not installed on this computer's own drive", role, n));
        }
        let serial = d["serial"].as_str().unwrap_or("").to_string();
        let wwn = clean_wwn(d["wwn"].as_str().unwrap_or(""));
        if serial.trim().is_empty() && wwn.trim().is_empty() {
            return Err(format!("the {} drive ({}) reports no serial number or world-wide name, so it could not be found again safely", role, n));
        }
        out.push(json!({ "role": role, "name": n, "serial": serial, "wwn": wwn, "size_bytes": d["size_bytes"], "model": d["model"] }));
    }
    Ok(out)
}

/// The gate's job. Pure.
pub fn job(job_id: &str, now: &str, typed: &str, windows: &str, edition: &str, edition_name: &str, language: &str, account: &str, drives: &[Value], entry: &Value, wifi: &[Value]) -> Value {
    json!({
        "schema": "go-back-job/1",
        "job_id": job_id,
        "created_utc": now,
        "settle_in_version": crate::VERSION,
        "consent": { "sentence": typed, "typed_utc": now },
        "windows": { "version": windows, "edition": edition, "edition_name": edition_name, "image_index": 1, "language": language,
                     "source": { "file": entry["file"], "sha1": entry["sha1"], "size": entry["size"] } },
        "account": { "name": account },
        "wifi": wifi,
        "drives": drives.iter().map(|d| json!({ "role": d["role"], "serial": d["serial"], "wwn": d["wwn"], "size_bytes": d["size_bytes"], "model": d["model"] })).collect::<Vec<_>>(),
    })
}

/// Which entry to start once. Pure, over `efibootmgr`'s own listing.
///
/// The firmware's own generic USB entry when it has one (a "USB" entry whose
/// path is not a partition, e.g. Insyde's "Boot2001* EFI USB Device  RC"),
/// and the stick is the only USB disk plugged in: on the Aspire (2026-10-02)
/// that entry started the stick and our own partition entry was passed over.
/// Otherwise our own entry (the rig, whose firmware has no generic USB entry).
pub fn pick_boot_next(listing: &str, ours: &str, usb_disks: usize) -> (String, &'static str) {
    let order: Vec<String> = listing.lines().find_map(|l| l.strip_prefix("BootOrder:"))
        .map(|o| o.trim().split(',').map(|x| x.trim().to_ascii_uppercase()).collect()).unwrap_or_default();
    let mut generic: Vec<String> = listing.lines().filter_map(|l| {
        let rest = l.strip_prefix("Boot")?;
        let num = rest.get(0..4)?.to_ascii_uppercase();
        if !num.chars().all(|c| c.is_ascii_hexdigit()) || num == ours.to_ascii_uppercase() {
            return None;
        }
        let body = rest.get(4..)?.trim_start_matches('*').trim();
        let (label, path) = body.split_once('\t').unwrap_or((body, ""));
        let path = path.trim();
        (label.to_ascii_lowercase().contains("usb") && !path.starts_with("HD(") && !path.contains("File(")).then_some(num)
    }).collect();
    generic.sort_by_key(|n| order.iter().position(|o| o == n).unwrap_or(usize::MAX));
    match generic.first() {
        Some(g) if usb_disks == 1 => (g.clone(), "the firmware's own USB entry"),
        _ => (ours.to_ascii_uppercase(), "our own entry for the stick"),
    }
}

/// A one-time boot of the stick: our own firmware entry (efibootmgr -C: not
/// added to the boot order), then BootNext at the entry pick_boot_next
/// chooses. Linux starts again after it.
pub fn boot_once(stick_disk: &str, usb_disks: usize) -> Result<Value, String> {
    let dev = format!("/dev/{}", stick_disk);
    let out = run("efibootmgr", &["-C", "-d", &dev, "-p", "1", "-L", "upgrade_ go back to Windows", "-l", "\\EFI\\BOOT\\BOOTX64.EFI"])?;
    let ours = out.lines().filter_map(|l| l.strip_prefix("Boot")).filter(|l| l.contains("upgrade_ go back to Windows"))
        .filter_map(|l| l.get(0..4)).last().ok_or("the new firmware entry could not be found")?.to_string();
    let (num, why) = pick_boot_next(&run("efibootmgr", &[])?, &ours, usb_disks);
    run("efibootmgr", &["-n", &num])?;
    Ok(json!({ "entry": format!("Boot{}", ours), "boot_next": num, "chosen": why, "usb_disks": usb_disks }))
}

/// Undo the one-time start: clear BootNext if it points at our entry, and
/// delete our entries. Nothing else on the computer was changed before it.
pub fn undo() -> Result<Value, String> {
    let out = run("efibootmgr", &[])?;
    let ours: Vec<String> = out.lines().filter_map(|l| l.strip_prefix("Boot")).filter(|l| l.contains("upgrade_ go back to Windows"))
        .filter_map(|l| l.get(0..4)).map(str::to_string).collect();
    let next = out.lines().find_map(|l| l.strip_prefix("BootNext:")).map(|v| v.trim().to_string());
    // BootNext is ours to clear when it points at our entry, or at the
    // firmware's generic USB entry that boot_once may have chosen instead
    let generic = pick_boot_next(&out, "----", 1);
    if let Some(n) = &next {
        if ours.contains(n) || (generic.1 == "the firmware's own USB entry" && generic.0 == n.to_ascii_uppercase()) {
            run("efibootmgr", &["-N"])?;
        }
    }
    for n in &ours {
        run("efibootmgr", &["-b", n, "-B"])?;
    }
    // the Wi-Fi passwords leave the stick at every stop (if the stick is in)
    let wifi = wipe_stick_wifi();
    Ok(json!({ "result": "undone", "removed": ours, "boot_next_was": next, "stick_wifi": wifi }))
}

/// Delete upgrade_/wifi from the WINSETUP stick, mounting it privately if needed.
fn wipe_stick_wifi() -> Value {
    let dev = "/dev/disk/by-label/WINSETUP";
    if !std::path::Path::new(dev).exists() {
        return json!("the stick is not plugged in");
    }
    let mp = format!("/run/upgrade_-go-back-undo.{}", std::process::id());
    if std::fs::create_dir_all(&mp).is_err() || run("mount", &["-t", "vfat", dev, &mp]).is_err() {
        let _ = std::fs::remove_dir(&mp);
        return json!("the stick could not be opened");
    }
    let gone = std::fs::remove_dir_all(format!("{}/upgrade_/wifi", mp)).is_ok() || !std::path::Path::new(&format!("{}/upgrade_/wifi", mp)).exists();
    let _ = run("umount", &[&mp]);
    let _ = std::fs::remove_dir(&mp);
    json!(if gone { "removed" } else { "NOT removed" })
}

/// The walk-away pages' words (the window has none of its own). DRAFT,
/// awaiting the owner's approval. Pure.
pub fn words(plan: &Value) -> Value {
    let drives = plan["drives"].as_array().cloned().unwrap_or_default();
    let gb = |d: &Value| d["size_bytes"].as_u64().unwrap_or(0) as f64 / 1e9;
    let mut named: Vec<String> = drives.iter().map(|d| format!("{} ({:.0} GB, {})", d["model"].as_str().unwrap_or("?"),
        gb(d), if d["role"] == "system" { "Linux is on it: Windows goes here" } else { "your files in Linux are on it: it is left empty" })).collect();
    if let Some(r) = plan["drives"]["refused"].as_str() {
        named = vec![format!("None: {}.", r)];
    }
    json!({
        "what": {
            "heading": "What happens",
            "lines": [
                "This program downloads Windows from Microsoft (about 5 GB), makes a USB stick from it, and restarts this computer from the stick.",
                "After the restart a 2-minute countdown appears. Press any key during it to stop: nothing is deleted and Linux starts again.",
                "If you leave it, these drives are erased and Windows is installed, with nobody at the keyboard:",
            ],
            "drives": named,
            "after": "At the end, Windows starts. Your account is there without a password: Windows asks you to choose one the first time you sign in.",
        },
        "stick": {
            "heading": "The USB stick",
            "lines": [
                "Plug in a USB stick of 16 GB or more. Everything on it will be deleted.",
                "Use a name-brand stick (for example SanDisk, Kingston or Samsung). Some no-name sticks cannot start a computer.",
                "Only a USB stick can be chosen. This computer's own drives, USB hard drives and the upgrade_ stick are never offered.",
            ],
        },
        "consent": {
            "heading": "Your decision",
            "lines": [
                "Your saved Wi-Fi networks and their passwords are copied onto the USB stick, so Windows can connect to them on its own. They are removed from the stick once Windows has them, or if you stop.",
                "To go back to Windows, type this sentence exactly:",
            ],
            "sentence": SENTENCE,
            "button": "Prepare the way back",
            "note": "You will be asked for your password. Preparing takes 20 to 60 minutes; nothing on this computer is deleted yet.",
        },
        "preparing": {
            "heading": "Preparing",
            "steps": { "consent": "Checking your decision", "drives": "Naming the drives", "room": "Checking space", "catalog": "Asking Microsoft which file",
                       "download": "Downloading Windows from Microsoft", "build": "Preparing Windows' files", "stick": "Writing the USB stick", "boot-once": "Setting the computer to start from the stick once" },
        },
        "ready": {
            "heading": "Ready",
            "lines": [
                "Everything is prepared. Nothing on this computer has been deleted.",
                "Leave the USB stick in. When you restart, the countdown appears. Press any key during it to stop.",
            ],
            "restart": "Restart now",
            "undo": "Do not go back after all",
            "undone": "Done. This computer starts Linux as usual. The USB stick can be used for something else.",
        },
        "failed": "The way back was not prepared: ",
        "failed_unchanged": "Nothing on this computer changed.",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<MCT><Catalogs><Catalog><PublishedMedia><Files>
      <File id="1"><FileName>26100.x_en-us.esd</FileName><LanguageCode>en-us</LanguageCode><Language>English (United States)</Language><Edition>Core</Edition><Architecture>x64</Architecture><Size>4680320173</Size><Sha1>8ceab2838f8e90180ac7490e8752157fb05588cb</Sha1><FilePath>http://dl.delivery.mp.microsoft.com/filestreamingservice/files/009d9a0d/26100.x_en-us.esd</FilePath></File>
      <File id="2"><FileName>26100.x_fr-fr.esd</FileName><LanguageCode>fr-fr</LanguageCode><Edition>Core</Edition><Architecture>x64</Architecture><Size>1</Size><Sha1>aa</Sha1><FilePath>http://dl.delivery.mp.microsoft.com/x</FilePath></File>
      <File id="3"><FileName>evil.esd</FileName><LanguageCode>de-de</LanguageCode><Edition>Core</Edition><Architecture>x64</Architecture><Size>1</Size><Sha1>8ceab2838f8e90180ac7490e8752157fb05588cb</Sha1><FilePath>http://example.com/evil.esd</FilePath></File>
    </Files></PublishedMedia></Catalog></Catalogs></MCT>"#;

    #[test]
    fn reads_microsofts_real_catalogs_when_present() {
        // the two catalogs read 2026-09-29 (rig artifacts, gitignored) through the
        // static cabextract settle-in carries (tools/build-cabextract.sh)
        let base = env!("CARGO_MANIFEST_DIR");
        let cx = format!("{}/target/cabextract/cabextract", base);
        if !std::path::Path::new(&cx).exists() {
            return;
        }
        for (f, n) in [("products-win11.cab", "26100"), ("products-win10.cab", "19045")] {
            let cab = format!("{}/../rig/vm/artifacts/windows/{}", base, f);
            if !std::path::Path::new(&cab).exists() {
                continue;
            }
            let dir = std::env::temp_dir().join(format!("walkaway-cat-{}-{}", n, std::process::id()));
            let x = unpack_catalog(&cab, dir.to_str().unwrap(), &cx).unwrap();
            let e = pick(&x, "en-us", "Core").unwrap();
            assert!(e["file"].as_str().unwrap().starts_with(n), "{}", e);
            assert_eq!(e["sha1"].as_str().unwrap().len(), 40);
            std::fs::remove_dir_all(&dir).unwrap();
        }
    }

    #[test]
    fn picks_the_language_and_refuses_bad_entries() {
        let e = pick(XML, "EN-US", "Core").unwrap();
        assert_eq!(e["size"], 4680320173u64);
        assert_eq!(e["sha1"], "8ceab2838f8e90180ac7490e8752157fb05588cb");
        assert!(pick(XML, "fr-fr", "Core").unwrap_err().contains("no size or checksum"));
        assert!(pick(XML, "de-de", "Core").unwrap_err().contains("outside Microsoft's download host"));
        assert!(pick(XML, "en-us", "Professional").is_err());
    }

    #[test]
    fn editions_and_their_index() {
        assert_eq!(catalog_edition(&json!({ "edition_id": "Professional" })), "Professional");
        assert_eq!(catalog_edition(&json!({ "edition_id": "Enterprise" })), "Core");
        assert_eq!(catalog_edition(&json!({})), "Core");
        let info = "Index:                  3\nName: Setup\nEdition ID:             WindowsPE\nIndex:                  4\nEdition ID:             Core\nIndex:                  9\nEdition ID:             Professional\n";
        assert_eq!(edition_index(info, "Core"), Some(4));
        assert_eq!(edition_index(info, "Professional"), Some(9));
        assert_eq!(edition_index(info, "Education"), None);
    }

    fn fake_sys(tag: &str) -> std::path::PathBuf {
        // sda (system: sda2 is /), sdb (home: its partition under an LVM volume is /home), sdc (USB stick)
        let r = std::env::temp_dir().join(format!("walkaway-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&r);
        let mk = |p: &str| std::fs::create_dir_all(r.join(p)).unwrap();
        let ln = |target: &str, link: &str| std::os::unix::fs::symlink(r.join(target), r.join(link)).unwrap();
        mk("sys/devices/pci/sda/sda2");
        std::fs::write(r.join("sys/devices/pci/sda/sda2/partition"), "2").unwrap();
        mk("sys/devices/pci/sdb/sdb1");
        std::fs::write(r.join("sys/devices/pci/sdb/sdb1/partition"), "1").unwrap();
        mk("sys/devices/virtual/block/dm-0/slaves");
        ln("sys/devices/pci/sdb/sdb1", "sys/devices/virtual/block/dm-0/slaves/sdb1");
        mk("sys/dev/block");
        ln("sys/devices/pci/sda/sda2", "sys/dev/block/8:2");
        ln("sys/devices/virtual/block/dm-0", "sys/dev/block/253:0");
        mk("proc/self");
        std::fs::write(r.join("proc/self/mountinfo"), "1 0 8:2 / / rw - btrfs /dev/sda2 rw\n2 1 253:0 / /home rw - ext4 /dev/mapper/v-home rw\n").unwrap();
        r
    }

    fn disks() -> Value {
        json!([
            { "name": "sda", "size_bytes": 256060514304u64, "usb": false, "removable": false, "serial": "EI8AN00951150A71I", "wwn": "", "model": "HFS256G39TND-N210A" },
            { "name": "sdb", "size_bytes": 1000204886016u64, "usb": false, "removable": false, "serial": "WD-WXV1A88H0TFV", "wwn": "0x50014ee2b5a8e5a1", "model": "WDC WD10SPZX-21Z10T0" },
            { "name": "sdc", "size_bytes": 15664676864u64, "usb": true, "removable": true, "serial": "", "wwn": "", "model": "General UDisk" }
        ])
    }

    #[test]
    fn names_the_system_drive_and_the_home_drive_through_lvm() {
        let r = fake_sys("two");
        assert_eq!(disks_under(r.to_str().unwrap(), "253:0"), vec!["sdb".to_string()]);
        let d = drives(r.to_str().unwrap(), &disks()).unwrap();
        assert_eq!((d[0]["role"].as_str(), d[0]["name"].as_str()), (Some("system"), Some("sda")));
        assert_eq!((d[1]["role"].as_str(), d[1]["name"].as_str()), (Some("second"), Some("sdb")));
        assert_eq!(d[1]["wwn"], "0x50014ee2b5a8e5a1");
        std::fs::remove_dir_all(&r).unwrap();
    }

    #[test]
    fn refuses_a_system_on_usb_and_a_drive_with_no_identity() {
        let r = fake_sys("usb");
        let mut ds = disks();
        ds[0]["usb"] = json!(true);
        assert!(drives(r.to_str().unwrap(), &ds).unwrap_err().contains("USB or removable"));
        let mut ds = disks();
        ds[0]["serial"] = json!("");
        assert!(drives(r.to_str().unwrap(), &ds).unwrap_err().contains("no serial number or world-wide name"));
        std::fs::remove_dir_all(&r).unwrap();
    }

    #[test]
    fn the_words_name_both_drives_and_the_way_to_stop() {
        let plan = json!({ "drives": [
            { "role": "system", "model": "HFS256G39TND-N21", "size_bytes": 256060514304u64 },
            { "role": "second", "model": "WDC WD10SPZX-21Z", "size_bytes": 1000204886016u64 } ] });
        let w = words(&plan);
        assert_eq!(w["what"]["drives"][0], "HFS256G39TND-N21 (256 GB, Linux is on it: Windows goes here)");
        assert!(w["what"]["drives"][1].as_str().unwrap().contains("1000 GB"));
        assert!(w["what"]["lines"].to_string().contains("Press any key during it to stop"));
        assert_eq!(w["consent"]["sentence"], SENTENCE);
        let w = words(&json!({ "drives": { "refused": "Linux here is spread over 2 drives" } }));
        assert!(w["what"]["drives"][0].as_str().unwrap().starts_with("None: Linux here is spread"));
    }

    #[test]
    fn only_real_world_wide_names_count() {
        assert_eq!(clean_wwn("0x50014EE6B3A6DB94"), "0x50014ee6b3a6db94");
        assert_eq!(clean_wwn("eui.000000000000000100a07524480c575b"), "eui.000000000000000100a07524480c575b");
        assert_eq!(clean_wwn("t10.ATA     HFS256G39TND-N210A                      EI8AN00951150A71I"), "");
        assert_eq!(clean_wwn("0x1234"), "");
    }

    #[test]
    fn the_aspire_starts_through_its_own_usb_entry() {
        // the Aspire's firmware listing, 2026-10-02 (Insyde H2O); Boot0003 is ours
        let aspire = "BootCurrent: 0004\nTimeout: 0 seconds\nBootOrder: 0000,2001,2002,2003,0004\n\
            Boot0000* Unknown Device: \tHD(1,GPT,530bed19-60b9-48af-9ce5-c0c6942310ef,0x800,0x12c000)/\\EFI\\fedora\\shim.efiRC\n\
            Boot0003* upgrade_ go back to Windows\tHD(1,MBR,0x4003abbf,0x800,0x1d4b800)/\\EFI\\BOOT\\BOOTX64.EFI\n\
            Boot0004* Fedora\tHD(1,GPT,530bed19-60b9-48af-9ce5-c0c6942310ef,0x800,0x12c000)/\\EFI\\fedora\\shimx64.efi\n\
            Boot2001* EFI USB Device\tRC\nBoot2002* EFI DVD/CDROM\tRC\nBoot2003* EFI Network\tRC\n";
        assert_eq!(pick_boot_next(aspire, "0003", 1), ("2001".to_string(), "the firmware's own USB entry"));
        // two USB disks: "any USB" could start the wrong one, so our own entry
        assert_eq!(pick_boot_next(aspire, "0003", 2).0, "0003");
        // the rig (Hyper-V): no generic USB entry at all
        let rig = "BootOrder: 0001,0002\nBoot0001* Windows Boot Manager\tHD(1,GPT,x,0x800,0x32000)/\\EFI\\Microsoft\\Boot\\bootmgfw.efi\nBoot0002* EFI SCSI Device\tAcpiEx(VMBus,0,0)/VenHw(9b17)\nBoot0005* upgrade_ go back to Windows\tHD(1,MBR,0x1,0x800,0x1)/\\EFI\\BOOT\\BOOTX64.EFI\n";
        assert_eq!(pick_boot_next(rig, "0005", 1).0, "0005");
    }

    #[test]
    fn wifi_profiles_for_windows_and_what_is_not_carried() {
        let home = "[connection]\nid=Home\ntype=wifi\n\n[wifi]\nmode=infrastructure\nssid=Home & Co\n\n[wifi-security]\nkey-mgmt=wpa-psk\npsk=p<ss\"w0rd\n";
        let (i, x) = wifi_profile(home).unwrap();
        let x = x.unwrap();
        assert_eq!(i["carried"], true);
        assert!(x.contains("<name>Home &amp; Co</name>") && x.contains("<hex>486F6D65202620436F</hex>"));
        assert!(x.contains("<authentication>WPA2PSK</authentication>") && x.contains("<keyMaterial>p&lt;ss&quot;w0rd</keyMaterial>"));
        assert!(!i.to_string().contains("w0rd"), "a password never reaches the record");
        let wpa3 = home.replace("wpa-psk", "sae");
        assert!(wifi_profile(&wpa3).unwrap().1.unwrap().contains("WPA3SAE"));
        let hidden_manual = "[connection]\nid=x\ntype=wifi\nautoconnect=false\n[wifi]\nssid=Lab\nhidden=true\n";
        let x = wifi_profile(hidden_manual).unwrap().1.unwrap();
        assert!(x.contains("<nonBroadcast>true</nonBroadcast>") && x.contains("<connectionMode>manual</connectionMode>") && x.contains("<authentication>open</authentication>"));
        let eap = "[connection]\nid=Work\ntype=wifi\n[wifi]\nssid=Work\n[wifi-security]\nkey-mgmt=wpa-eap\n";
        let (i, x) = wifi_profile(eap).unwrap();
        assert!(x.is_none() && i["why"].as_str().unwrap().contains("enterprise"));
        let keyring = "[connection]\nid=K\ntype=wifi\n[wifi]\nssid=K\n[wifi-security]\nkey-mgmt=wpa-psk\npsk-flags=1\n";
        assert!(wifi_profile(keyring).unwrap().1.is_none());
        assert!(wifi_profile("[connection]\nid=eth\ntype=ethernet\n").is_none());
    }

    #[test]
    fn carry_wifi_writes_profiles_and_the_first_logon_script() {
        let r = std::env::temp_dir().join(format!("walkaway-wifi-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&r);
        let nm = r.join("root/etc/NetworkManager/system-connections");
        std::fs::create_dir_all(&nm).unwrap();
        std::fs::write(nm.join("Home.nmconnection"), "[connection]\nid=Home\ntype=wifi\n[wifi]\nssid=Home\n[wifi-security]\nkey-mgmt=wpa-psk\npsk=secret123\n").unwrap();
        std::fs::write(nm.join("Work.nmconnection"), "[connection]\nid=Work\ntype=wifi\n[wifi]\nssid=Work\n[wifi-security]\nkey-mgmt=wpa-eap\n").unwrap();
        let tree = r.join("tree");
        std::fs::create_dir_all(tree.join("upgrade_")).unwrap();
        let list = carry_wifi(r.join("root").to_str().unwrap(), tree.to_str().unwrap()).unwrap();
        assert_eq!(list.len(), 2);
        assert!(std::fs::read_to_string(tree.join("upgrade_/wifi/01.xml")).unwrap().contains("secret123"));
        assert!(!tree.join("upgrade_/wifi/02.xml").exists());
        let cmd = std::fs::read_to_string(tree.join("upgrade_/go-back-first-logon.cmd")).unwrap();
        assert!(cmd.contains("netsh wlan add profile filename=") && cmd.contains("user=all") && cmd.contains("rmdir /s /q %S%\\upgrade_\\wifi"));
        assert!(!serde_json::to_string(&list).unwrap().contains("secret123"));
        std::fs::remove_dir_all(&r).unwrap();
    }

    #[test]
    fn the_job_is_what_the_gate_reads() {
        let d = vec![json!({ "role": "system", "name": "sda", "serial": "S", "wwn": "", "size_bytes": 1u64, "model": "M" })];
        let j = job("id", "2026-09-29T00:00:00Z", SENTENCE, "11", "Core", "Windows 11 Home", "en-US", "rig", &d, &json!({ "file": "f.esd", "sha1": "ab", "size": 2 }), &[]);
        assert_eq!(j["schema"], "go-back-job/1");
        assert_eq!(j["consent"]["sentence"], SENTENCE);
        assert_eq!(j["windows"]["image_index"], 1);
        assert!(j["drives"][0].get("name").is_none(), "Linux's own disk names mean nothing to the gate");
    }
}
