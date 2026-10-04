//! Saved Wi-Fi networks: one profile (the XML Windows' Native Wifi API
//! returns) into a `harvest.wifi.profiles` row, and all of them into
//! `harvest.wifi` plus the password files to write, or a refusal.
//!
//! A row never carries the password, only whether there is one. What Linux
//! can join is WPA/WPA2/WPA3 personal and open networks (decided
//! 2026-09-26). The rest is listed with the reason, never guessed.

use crate::val::{at, eq_ci, items, one_of, s, truthy};
use serde_json::{json, Value};

type Node<'a> = roxmltree::Node<'a, 'a>;

// PowerShell reads elements by name alone: no namespace, any case
fn child<'a>(node: Option<Node<'a>>, name: &str) -> Option<Node<'a>> {
    node?.children().find(|c| c.is_element() && c.tag_name().name().eq_ignore_ascii_case(name))
}

fn text(node: Option<Node>) -> String {
    node.and_then(|n| n.text()).unwrap_or("").to_string()
}

// an element with nothing inside reads as an empty text, which is "not there"
fn there(node: Option<Node>) -> bool {
    node.is_some_and(|n| n.has_children() || n.attributes().len() > 0)
}

/// ConvertFrom-JobWlanProfile. Nothing for a text that is not a network
/// profile. The row still carries `HasKey`, which `harvest_wifi` uses and
/// then removes.
pub fn profile_row(xml: &str) -> Option<Value> {
    let doc = roxmltree::Document::parse(xml).ok()?;
    let p = Some(doc.root_element()).filter(|r| r.tag_name().name().eq_ignore_ascii_case("WLANProfile"));
    let ssid_config = child(p, "SSIDConfig");
    if !there(p) || !there(ssid_config) {
        return None;
    }
    let ssid_node = child(ssid_config, "SSID");
    let ssid = text(child(ssid_node, "name"));
    let mut hex = text(child(ssid_node, "hex")).to_uppercase();
    if hex.is_empty() {
        hex = ssid.bytes().map(|b| format!("{b:02X}")).collect();
    }
    let sec = child(child(p, "MSM"), "security");
    let auth_enc = child(sec, "authEncryption");
    let auth = text(child(auth_enc, "authentication"));
    let enc = text(child(auth_enc, "encryption"));
    let onex = eq_ci(&text(child(auth_enc, "useOneX")), "true");
    // WPA3 in transition mode: the router also takes WPA2 (9 of 9 WPA3
    // profiles on the G16, 2026-09-27), so it is joined as WPA2-personal
    let transition = auth_enc.is_some_and(|a| a.children().any(|n| n.is_element() && n.tag_name().name().eq_ignore_ascii_case("transitionMode") && eq_ci(n.text().unwrap_or(""), "true")));
    let shared = child(sec, "sharedKey");
    let has_key = there(shared) && eq_ci(&text(child(shared, "protected")), "false") && !text(child(shared, "keyMaterial")).is_empty();

    let (mut km, mut why): (&str, Option<String>) = ("UNSUPPORTED", None);
    if !eq_ci(&text(child(p, "connectionType")), "ESS") {
        why = Some("an ad-hoc (computer-to-computer) network".into());
    } else if onex || one_of(&auth, &["WPA", "WPA2", "WPA3", "WPA3ENT", "WPA3ENT192"]) {
        why = Some("an enterprise network (a company or school sign-in)".into());
    } else if eq_ci(&auth, "open") && eq_ci(&enc, "none") {
        km = "none";
    } else if eq_ci(&enc, "WEP") {
        why = Some("WEP, an old and broken kind of Wi-Fi security".into());
    } else if one_of(&auth, &["WPAPSK", "WPA2PSK"]) || (eq_ci(&auth, "WPA3SAE") && transition) {
        km = "wpa-psk";
    } else if eq_ci(&auth, "WPA3SAE") {
        km = "sae";
    } else {
        why = Some(format!("a kind of Wi-Fi security this version does not set up ({auth}/{enc})"));
    }
    if (km == "wpa-psk" || km == "sae") && !has_key {
        why = Some("its password could not be read from Windows".into());
        km = "UNSUPPORTED";
    }
    Some(json!({
        "name": text(child(p, "name")), "ssid": ssid, "ssid_hex": hex,
        "hidden": eq_ci(&text(child(ssid_config, "nonBroadcast")), "true"),
        "windows_auth": format!("{auth}/{enc}"), "key_mgmt": km, "supported": km != "UNSUPPORTED",
        "autoconnect": eq_ci(&text(child(p, "connectionMode")), "auto"), "why_not": why, "secrets_file": null,
        "HasKey": has_key,
    }))
}

/// One password file to write: where (relative to the job folder), and the
/// profile XML that holds the password.
#[derive(Debug, Clone, PartialEq)]
pub struct SecretFile {
    pub rel: String,
    pub xml: String,
}

pub const WIFI_DIR: &str = "artifacts/credentials/wifi";

/// ConvertTo-JobWifi. `api`: what the Native Wifi API returned
/// (`{Present, Error, Profiles: [{Xml}]}`). `stored_count`: how many
/// profiles Windows keeps on disk, counted on its own. If the two disagree
/// a network would be silently missing after the move, so it refuses
/// (netsh's export lost one of 14 on the G16, 2026-09-27).
pub fn harvest_wifi(api: &Value, stored_count: i64, dir: &str) -> Result<(Value, Vec<SecretFile>), String> {
    if truthy(at(api, "Error")) {
        return Err(format!("the saved Wi-Fi networks could not be read ({})", s(at(api, "Error"))));
    }
    if !truthy(at(api, "Present")) {
        if stored_count > 0 {
            return Err(format!("Windows has {stored_count} saved Wi-Fi network(s), but its Wi-Fi service is not running, so they cannot be read"));
        }
        return Ok((json!({"result": "no-wireless", "secrets_dir": null, "profiles": []}), Vec::new()));
    }
    let rows: Vec<(Value, String)> = items(at(api, "Profiles")).into_iter().filter_map(|pr| { let xml = s(at(pr, "Xml")); profile_row(&xml).map(|r| (r, xml)) }).collect();
    if rows.len() as i64 != stored_count {
        return Err(format!("Windows has {stored_count} saved Wi-Fi network(s) on disk but {} could be read - one would be missing after the move", rows.len()));
    }
    let (mut out, mut files, mut seen): (Vec<Value>, Vec<SecretFile>, Vec<String>) = (Vec::new(), Vec::new(), Vec::new());
    for (mut row, xml) in rows {
        // the same network saved on two Wi-Fi adapters is set up once
        let key = format!("{}|{}", s(&row["name"]), s(&row["ssid_hex"])).to_lowercase();
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        if truthy(&row["supported"]) && truthy(&row["HasKey"]) {
            let rel = format!("{dir}/{:02}.xml", files.len() + 1);
            row["secrets_file"] = json!(rel);
            files.push(SecretFile { rel, xml });
        }
        if let Some(map) = row.as_object_mut() {
            map.shift_remove("HasKey");
        }
        out.push(row);
    }
    let result = if out.is_empty() { "none-saved" } else { "exported" };
    let secrets_dir = if files.is_empty() { Value::Null } else { json!(dir) };
    Ok((json!({"result": result, "secrets_dir": secrets_dir, "profiles": out}), files))
}
