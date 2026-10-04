//! One exported Wi-Fi profile (the XML `netsh wlan export profile` writes)
//! read into what the new system needs to know about the network.

#[derive(Debug, Clone, PartialEq)]
pub struct WlanProfile {
    pub ssid: Option<String>,
    pub authentication: String,
    /// NetworkManager's key-mgmt: `none`, `wpa-psk`, `sae`, or `UNSUPPORTED`
    pub nm_key_mgmt: &'static str,
    pub supported: bool,
    pub auto_connect: bool,
    /// a password is there and Windows did not encrypt it for itself
    pub has_secret: bool,
    /// the password, only when asked for
    pub secret: Option<String>,
}

fn key_mgmt(auth: &str) -> &'static str {
    const MAP: [(&str, &str); 7] = [("open", "none"), ("WPAPSK", "wpa-psk"), ("WPA2PSK", "wpa-psk"), ("WPA3SAE", "sae"), ("WPA3ENT", "UNSUPPORTED"), ("WPA2", "UNSUPPORTED"), ("WPA", "UNSUPPORTED")];
    // anything not in the table is unsupported: listed, never guessed
    MAP.iter().find(|(win, _)| win.eq_ignore_ascii_case(auth)).map_or("UNSUPPORTED", |(_, nm)| nm)
}

/// The file's bytes as text. `netsh` writes UTF-8; a byte order mark of
/// either kind is honoured, as PowerShell's XML loader honours it.
fn decode(bytes: &[u8]) -> Result<String, String> {
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let units: Vec<u16> = rest.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        return String::from_utf16(&units).map_err(|e| format!("the profile is not readable text ({e})"));
    }
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    String::from_utf8(bytes.to_vec()).map_err(|e| format!("the profile is not readable text ({e})"))
}

/// ConvertFrom-HarvestWlanProfileXml. An error means this file is not a
/// profile this reader understands (not XML, or no authentication named).
/// The caller must list such a network as not carried, not drop it.
pub fn parse_profile(xml: &[u8], include_secrets: bool) -> Result<WlanProfile, String> {
    let text = decode(xml)?;
    // an encoding="UTF-16" declaration would stop the parser; the text is decoded already
    let doc = roxmltree::Document::parse(&text).map_err(|e| format!("the profile is not XML ({e})"))?;
    let root = doc.root_element();
    if !root.tag_name().name().eq_ignore_ascii_case("WLANProfile") {
        return Err("the file is not a WLAN profile".to_string());
    }
    // PowerShell reads elements by name alone: no namespace, any case
    fn child<'a>(node: Option<roxmltree::Node<'a, 'a>>, name: &str) -> Option<roxmltree::Node<'a, 'a>> {
        node?.children().find(|c| c.is_element() && c.tag_name().name().eq_ignore_ascii_case(name))
    }
    let value = |node: Option<roxmltree::Node>| node.map(|n| n.text().unwrap_or("").to_string());
    let p = Some(root);
    let security = child(child(p, "MSM"), "security");
    let auth = value(child(child(security, "authEncryption"), "authentication")).ok_or("the profile names no authentication")?;
    let shared = child(security, "sharedKey");
    let key = value(child(shared, "keyMaterial"));
    let protected = value(child(shared, "protected"));
    let nm = key_mgmt(&auth);
    Ok(WlanProfile {
        ssid: value(child(p, "name")),
        nm_key_mgmt: nm,
        supported: nm != "UNSUPPORTED",
        auto_connect: value(child(p, "connectionMode")).is_some_and(|m| m.eq_ignore_ascii_case("auto")),
        has_secret: key.as_deref().is_some_and(|k| !k.is_empty()) && !protected.as_deref().is_some_and(|x| x.eq_ignore_ascii_case("true")),
        secret: if include_secrets { key } else { None },
        authentication: auth,
    })
}
