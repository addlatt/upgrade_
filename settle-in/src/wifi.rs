//! Wi-Fi (decided 2026-09-26). The job lists the networks; the passwords
//! are files in the handoff folder, the profiles Windows exported. For every
//! network Linux can join (WPA/WPA2/WPA3 personal and open) settle-in writes
//! one NetworkManager connection file. That file format is the same on every
//! distribution that uses NetworkManager, and NetworkManager reads the files
//! when it starts, which is after settle-in: no command is needed. Networks
//! it cannot join are listed with the reason, never guessed. Then settle-in
//! deletes its copy of the passwords. Without NetworkManager it sets up
//! nothing and keeps the root-only copy, and says so (RISKS R28).

use serde_json::{json, Value};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;

pub fn has_networkmanager(root: &str) -> bool {
    let r = root.trim_end_matches('/');
    std::path::Path::new(&format!("{}/etc/NetworkManager", r)).is_dir()
        && ["usr/sbin/NetworkManager", "usr/bin/NetworkManager"].iter().any(|p| std::path::Path::new(&format!("{}/{}", r, p)).exists())
}

fn hex_bytes(h: &str) -> Option<Vec<u8>> {
    if !h.len().is_multiple_of(2) || h.is_empty() {
        return None;
    }
    (0..h.len()).step_by(2).map(|i| u8::from_str_radix(&h[i..i + 2], 16).ok()).collect()
}

/// The password in a Windows profile, checked against the network the job names.
pub fn password_from_windows_xml(xml: &str, ssid_hex: &str) -> Result<String, String> {
    let doc = roxmltree::Document::parse(xml).map_err(|e| format!("the saved profile is not readable ({})", e))?;
    let find = |name: &str| doc.descendants().find(|n| n.tag_name().name() == name);
    let hex = find("hex").and_then(|n| n.text()).unwrap_or("").trim().to_ascii_uppercase();
    if hex != ssid_hex.to_ascii_uppercase() {
        return Err("the saved profile is for a different network than the job names".into());
    }
    if find("protected").and_then(|n| n.text()).map(str::trim) != Some("false") {
        return Err("the saved profile holds its password encrypted".into());
    }
    let key = find("keyMaterial").and_then(|n| n.text()).ok_or("the saved profile holds no password")?;
    Ok(key.to_string())
}

pub fn psk_ok(key_mgmt: &str, psk: &str) -> Result<(), String> {
    match key_mgmt {
        "wpa-psk" => {
            let hex64 = psk.len() == 64 && psk.bytes().all(|b| b.is_ascii_hexdigit());
            let phrase = (8..=63).contains(&psk.len()) && psk.bytes().all(|b| (0x20..=0x7e).contains(&b));
            if hex64 || phrase { Ok(()) } else { Err("the password is not a valid WPA password (8 to 63 characters)".into()) }
        }
        "sae" => if psk.is_empty() { Err("the password is empty".into()) } else { Ok(()) },
        _ => Err(format!("'{}' has no password", key_mgmt)),
    }
}

/// A value in a GKeyFile (the format NetworkManager's files use).
fn esc(s: &str) -> String {
    let mut o = String::new();
    for (i, c) in s.chars().enumerate() {
        match c {
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            ' ' if i == 0 => o.push_str("\\s"),
            c => o.push(c),
        }
    }
    o
}

/// The SSID as NetworkManager's file format writes it: plain text when that
/// is unambiguous, otherwise the exact bytes as a list of numbers.
fn ssid_value(bytes: &[u8]) -> String {
    let plain = std::str::from_utf8(bytes).ok().filter(|s| {
        !s.is_empty()
            && !s.starts_with(' ')
            && !s.ends_with(' ')
            && s.chars().all(|c| !c.is_control() && c != ';' && c != '\\')
            && !s.bytes().all(|b| b.is_ascii_digit())
    });
    match plain {
        Some(s) => s.to_string(),
        None => bytes.iter().map(|b| format!("{};", b)).collect(),
    }
}

pub fn keyfile(p: &Value, uuid: &str, psk: Option<&str>) -> Result<String, String> {
    let s = |k: &str| p.get(k).and_then(Value::as_str).unwrap_or("");
    let bytes = hex_bytes(s("ssid_hex")).ok_or("the network's name bytes are missing")?;
    let km = s("key_mgmt");
    let mut t = String::new();
    t.push_str("[connection]\n");
    t.push_str(&format!("id={}\n", esc(&String::from_utf8_lossy(&bytes))));
    t.push_str(&format!("uuid={}\ntype=wifi\n", uuid));
    if p.get("autoconnect") == Some(&json!(false)) {
        t.push_str("autoconnect=false\n");
    }
    t.push_str("\n[wifi]\nmode=infrastructure\n");
    t.push_str(&format!("ssid={}\n", ssid_value(&bytes)));
    if p.get("hidden") == Some(&json!(true)) {
        t.push_str("hidden=true\n");
    }
    match km {
        "none" => {}
        "wpa-psk" | "sae" => {
            let k = psk.ok_or("no password")?;
            psk_ok(km, k)?;
            t.push_str(&format!("\n[wifi-security]\nkey-mgmt={}\npsk={}\n", km, esc(k)));
        }
        other => return Err(format!("'{}' is not a kind of network this sets up", other)),
    }
    t.push_str("\n[ipv4]\nmethod=auto\n\n[ipv6]\naddr-gen-mode=default\nmethod=auto\n");
    Ok(t)
}

fn new_uuid() -> String {
    std::fs::read_to_string("/proc/sys/kernel/random/uuid").map(|s| s.trim().to_string()).unwrap_or_default()
}

/// SSIDs already set up in NetworkManager (as bytes), so none is doubled.
fn existing_ssids(dir: &str) -> Vec<Vec<u8>> {
    let mut v = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            if let Ok(t) = std::fs::read_to_string(e.path()) {
                for l in t.lines() {
                    if let Some(x) = l.strip_prefix("ssid=") {
                        let nums: Option<Vec<u8>> = x.trim_end_matches(';').split(';').map(|n| n.parse().ok()).collect();
                        v.push(match nums { Some(b) if x.ends_with(';') => b, _ => x.as_bytes().to_vec() });
                    }
                }
            }
        }
    }
    v
}

fn write_private(path: &str, text: &str) -> Result<(), String> {
    let tmp = format!("{}.part", path);
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp).map_err(|e| format!("{}: {}", tmp, e))?;
    f.write_all(text.as_bytes()).and_then(|_| f.sync_all()).map_err(|e| format!("{}: {}", tmp, e))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {}", path, e))
}

pub fn run(root: &str, handoff: &str, job: &Value) -> Value {
    let r = root.trim_end_matches('/');
    let w = match job.pointer("/harvest/wifi") {
        Some(w) => w,
        None => return json!({ "result": "not-in-job", "why": "the job has no Wi-Fi block (job writer before 0.15.0)" }),
    };
    let profiles = w.get("profiles").and_then(Value::as_array).cloned().unwrap_or_default();
    let secrets_dir = w.get("secrets_dir").and_then(Value::as_str).map(|d| format!("{}/{}", handoff, d));
    if profiles.is_empty() {
        return json!({ "result": "nothing-to-do", "why": format!("Windows had no saved networks ({})", w.get("result").and_then(Value::as_str).unwrap_or("?")) });
    }
    let listed: Vec<Value> = profiles.iter().map(|p| json!({ "ssid": p.get("ssid"), "why_not": p.get("why_not") })).collect();
    if !has_networkmanager(root) {
        return json!({ "result": "no-networkmanager",
            "why": "this system does not use NetworkManager, so no network was set up; the passwords stay in the root-only handoff folder",
            "networks": listed, "passwords_deleted": false });
    }
    let dir = format!("{}/etc/NetworkManager/system-connections", r);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return json!({ "result": "failed", "why": format!("{}: {}", dir, e), "networks": listed, "passwords_deleted": false });
    }
    let have = existing_ssids(&dir);
    let mut out = Vec::new();
    let mut made = 0;
    for (i, p) in profiles.iter().enumerate() {
        let ssid = p.get("ssid").cloned().unwrap_or(Value::Null);
        if p.get("supported") != Some(&json!(true)) {
            out.push(json!({ "ssid": ssid, "result": "not-set-up", "why": p.get("why_not") }));
            continue;
        }
        let hex = p.get("ssid_hex").and_then(Value::as_str).unwrap_or("");
        if hex_bytes(hex).map(|b| have.contains(&b)).unwrap_or(false) {
            out.push(json!({ "ssid": ssid, "result": "already-there" }));
            continue;
        }
        let psk = match p.get("secrets_file").and_then(Value::as_str) {
            None => Ok(None),
            Some(rel) if rel.split('/').any(|c| c == "..") || rel.starts_with('/') => Err("the password file's path is not inside the handoff folder".to_string()),
            Some(rel) => std::fs::read_to_string(format!("{}/{}", handoff, rel))
                .map_err(|e| format!("the password file could not be read ({})", e))
                .and_then(|x| password_from_windows_xml(&x, hex))
                .map(Some),
        };
        let res = psk.and_then(|k| keyfile(p, &new_uuid(), k.as_deref())).and_then(|text| {
            write_private(&format!("{}/upgrade_-{:02}.nmconnection", dir, i + 1), &text)
        });
        match res {
            Ok(()) => {
                made += 1;
                out.push(json!({ "ssid": ssid, "result": "created" }))
            }
            Err(why) => out.push(json!({ "ssid": ssid, "result": "not-set-up", "why": why })),
        }
    }
    // settle-in's copy goes once the networks are set up (the owner's sentence)
    let deleted = match &secrets_dir {
        Some(d) if std::path::Path::new(d).exists() => std::fs::remove_dir_all(d).is_ok(),
        _ => true,
    };
    json!({ "result": if made > 0 { "set-up" } else { "none-set-up" }, "created": made, "networks": out, "passwords_deleted": deleted })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn xml(hex: &str, protected: &str, key: &str) -> String {
        format!(r#"<?xml version="1.0"?><WLANProfile xmlns="http://www.microsoft.com/networking/WLAN/profile/v1"><name>Home Net</name><SSIDConfig><SSID><hex>{}</hex><name>Home Net</name></SSID></SSIDConfig><connectionType>ESS</connectionType><connectionMode>auto</connectionMode><MSM><security><authEncryption><authentication>WPA3SAE</authentication><encryption>AES</encryption><useOneX>false</useOneX><transitionMode xmlns="http://www.microsoft.com/networking/WLAN/profile/v4">true</transitionMode></authEncryption><sharedKey><keyType>passPhrase</keyType><protected>{}</protected><keyMaterial>{}</keyMaterial></sharedKey></security></MSM></WLANProfile>"#, hex, protected, key)
    }

    #[test]
    fn reads_the_password_and_checks_the_network() {
        assert_eq!(password_from_windows_xml(&xml("486F6D65204E6574", "false", "correct horse"), "486f6d65204e6574").unwrap(), "correct horse");
        assert!(password_from_windows_xml(&xml("486F6D65204E6574", "false", "x"), "41").is_err());
        assert!(password_from_windows_xml(&xml("486F6D65204E6574", "true", "01000000D08C"), "486F6D65204E6574").is_err());
    }

    #[test]
    fn keyfile_for_a_wpa_network() {
        let p = json!({ "ssid": "Home Net", "ssid_hex": "486F6D65204E6574", "hidden": false, "key_mgmt": "wpa-psk", "supported": true, "autoconnect": true });
        let k = keyfile(&p, "u-1", Some("correct horse")).unwrap();
        assert!(k.contains("id=Home Net\n") && k.contains("uuid=u-1\n") && k.contains("ssid=Home Net\n"));
        assert!(k.contains("key-mgmt=wpa-psk\npsk=correct horse\n"));
        assert!(!k.contains("autoconnect=false") && !k.contains("hidden="));
    }

    #[test]
    fn keyfile_open_hidden_manual_and_odd_names() {
        let p = json!({ "ssid": "a;b", "ssid_hex": "613B62", "hidden": true, "key_mgmt": "none", "supported": true, "autoconnect": false });
        let k = keyfile(&p, "u", None).unwrap();
        assert!(k.contains("ssid=97;59;98;\n") && k.contains("hidden=true") && k.contains("autoconnect=false") && !k.contains("wifi-security"));
        assert_eq!(ssid_value(b"1234"), "49;50;51;52;");
        assert_eq!(ssid_value(&[0xff, 0x41]), "255;65;");
        assert_eq!(esc(" x\\y"), "\\sx\\\\y");
    }

    #[test]
    fn bad_passwords_are_not_set_up() {
        assert!(psk_ok("wpa-psk", "short").is_err());
        assert!(psk_ok("wpa-psk", &"a".repeat(64)).is_ok());
        assert!(psk_ok("wpa-psk", &"z".repeat(64)).is_err());
        assert!(psk_ok("sae", "").is_err());
    }

    #[test]
    fn run_writes_private_files_and_deletes_the_passwords() {
        let t = std::env::temp_dir().join(format!("settle-in-wifi-{}", std::process::id()));
        let root = t.to_str().unwrap().to_string();
        let handoff = format!("{}/var/lib/upgrade_", root);
        std::fs::create_dir_all(format!("{}/artifacts/credentials/wifi", handoff)).unwrap();
        std::fs::create_dir_all(format!("{}/etc/NetworkManager/system-connections", root)).unwrap();
        std::fs::create_dir_all(format!("{}/usr/sbin", root)).unwrap();
        std::fs::write(format!("{}/usr/sbin/NetworkManager", root), "").unwrap();
        std::fs::write(format!("{}/artifacts/credentials/wifi/01.xml", handoff), xml("486F6D65204E6574", "false", "correct horse")).unwrap();
        let job = json!({ "harvest": { "wifi": { "result": "exported", "secrets_dir": "artifacts/credentials/wifi", "profiles": [
            { "ssid": "Home Net", "ssid_hex": "486F6D65204E6574", "hidden": false, "key_mgmt": "wpa-psk", "supported": true, "autoconnect": true, "why_not": null, "secrets_file": "artifacts/credentials/wifi/01.xml" },
            { "ssid": "Cafe", "ssid_hex": "43616665", "hidden": false, "key_mgmt": "none", "supported": true, "autoconnect": false, "why_not": null, "secrets_file": null },
            { "ssid": "Work", "ssid_hex": "576F726B", "hidden": false, "key_mgmt": "UNSUPPORTED", "supported": false, "autoconnect": true, "why_not": "an enterprise network", "secrets_file": null } ] } } });
        let r = run(&root, &handoff, &job);
        assert_eq!(r["result"], "set-up");
        assert_eq!(r["created"], 2);
        assert_eq!(r["networks"][2]["result"], "not-set-up");
        assert_eq!(r["passwords_deleted"], true);
        assert!(!std::path::Path::new(&format!("{}/artifacts/credentials/wifi", handoff)).exists());
        let f = format!("{}/etc/NetworkManager/system-connections/upgrade_-01.nmconnection", root);
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&f).unwrap().permissions().mode() & 0o777, 0o600);
        assert!(std::fs::read_to_string(&f).unwrap().contains("psk=correct horse"));
        // a second run doubles nothing
        let r2 = run(&root, &handoff, &job);
        assert_eq!(r2["networks"][0]["result"], "already-there");
        std::fs::remove_dir_all(&t).unwrap();
    }

    #[test]
    fn without_networkmanager_nothing_is_set_up_and_the_passwords_stay() {
        let t = std::env::temp_dir().join(format!("settle-in-nonm-{}", std::process::id()));
        let root = t.to_str().unwrap().to_string();
        let handoff = format!("{}/var/lib/upgrade_", root);
        std::fs::create_dir_all(format!("{}/artifacts/credentials/wifi", handoff)).unwrap();
        let job = json!({ "harvest": { "wifi": { "result": "exported", "secrets_dir": "artifacts/credentials/wifi", "profiles": [
            { "ssid": "Home Net", "ssid_hex": "486F6D65204E6574", "hidden": false, "key_mgmt": "wpa-psk", "supported": true, "autoconnect": true, "why_not": null, "secrets_file": "artifacts/credentials/wifi/01.xml" } ] } } });
        let r = run(&root, &handoff, &job);
        assert_eq!(r["result"], "no-networkmanager");
        assert!(std::path::Path::new(&format!("{}/artifacts/credentials/wifi", handoff)).exists());
        std::fs::remove_dir_all(&t).unwrap();
    }
}
