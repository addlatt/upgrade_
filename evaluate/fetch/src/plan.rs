//! What to download for one release and one desktop, and where each file
//! goes on the stick, from `data/releases.json` as compiled into the
//! scanner (the same bytes the scanner judges). Nothing here is typed per
//! release: a release whose layout the kit does not know is refused.

use serde_json::Value;

/// One file out of a download, and its place on the stick.
#[derive(Debug, Clone, PartialEq)]
pub struct Extract {
    /// on the disc, as bsdtar names it
    pub iso_path: String,
    /// on the stick, backslash separated, as SHA256SUMS names it
    pub dest: String,
    /// the measured sha256 of this file, where the table has one
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub url: String,
    pub sha256: String,
    pub extracts: Vec<Extract>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub id: String,
    pub name: String,
    pub desktop: String,
    pub desktop_label: String,
    pub family: String,
    pub measured: String,
    pub parts: Vec<Part>,
    /// release.json for the stick (make-kit.sh's own shape)
    pub release_json: String,
    /// upgrade_\boot-chain\<name> files: the installed system's boot files' SBAT facts
    pub boot_chain: Vec<(String, String)>,
}

fn s(v: &Value) -> String {
    v.as_str().unwrap_or("").to_string()
}

fn hex64(h: &str) -> bool {
    h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Every release and desktop the table offers, as (id, name, desktop id, desktop label).
pub fn offered(table: &str) -> Result<Vec<(String, String, String, String)>, String> {
    let t: Value = serde_json::from_str(table).map_err(|e| format!("data/releases.json: {e}"))?;
    let mut out = vec![];
    for r in t["Releases"].as_array().cloned().unwrap_or_default() {
        for d in r["Desktops"].as_array().cloned().unwrap_or_default() {
            out.push((s(&r["Id"]), s(&r["Name"]), s(&d["Id"]), s(&d["Label"])));
        }
    }
    Ok(out)
}

/// The plan for `id` with `desktop`, or why there is none.
pub fn plan(table: &str, id: &str, desktop: &str) -> Result<Plan, String> {
    let t: Value = serde_json::from_str(table).map_err(|e| format!("data/releases.json: {e}"))?;
    let releases = t["Releases"].as_array().cloned().unwrap_or_default();
    let Some(r) = releases.iter().find(|r| s(&r["Id"]) == id) else {
        return Err(format!("there is no release '{id}' in this kit's table"));
    };
    let (family, installer) = (s(&r["Family"]), s(&r["Installer"]));
    // the one layout the kit knows today (architecture.md, "Linux releases beyond
    // Fedora"): Anaconda from the netinst ISO, the desktop's live image beside it
    if family != "fedora" || installer != "kickstart" {
        return Err(format!("{} is a '{family}' release with a '{installer}' installer; this kit can only carry Fedora's layout so far", s(&r["Name"])));
    }
    let Some(d) = r["Desktops"].as_array().and_then(|a| a.iter().find(|d| s(&d["Id"]) == desktop)).cloned() else {
        return Err(format!("{} has no desktop '{desktop}' in this kit's table", s(&r["Name"])));
    };
    if !desktop.bytes().all(|b| b.is_ascii_lowercase()) || desktop.is_empty() {
        return Err(format!("the desktop id '{desktop}' is not a plain name"));
    }
    let boot = r["Boot"].as_array().cloned().unwrap_or_default();
    let stick_sha = |file: &str| boot.iter().find(|b| s(&b["Role"]) == "stick" && s(&b["File"]) == file).map(|b| s(&b["Sha256"]));
    let (shim, grub) = (stick_sha("BOOTX64.EFI"), stick_sha("grubx64.efi"));
    let (Some(shim), Some(grub)) = (shim, grub) else {
        return Err(format!("{}'s stick boot files are not measured in the table", s(&r["Name"])));
    };
    let net = &r["Netinst"];
    let parts = vec![
        Part {
            url: s(&net["Url"]),
            sha256: s(&net["Sha256"]),
            extracts: vec![
                Extract { iso_path: "EFI/BOOT/BOOTX64.EFI".into(), dest: "EFI\\BOOT\\BOOTX64.EFI".into(), sha256: Some(shim) },
                Extract { iso_path: "EFI/BOOT/grubx64.efi".into(), dest: "EFI\\BOOT\\grubx64.efi".into(), sha256: Some(grub) },
                Extract { iso_path: "images/pxeboot/vmlinuz".into(), dest: "images\\pxeboot\\vmlinuz".into(), sha256: None },
                Extract { iso_path: "images/pxeboot/initrd.img".into(), dest: "images\\pxeboot\\initrd.img".into(), sha256: None },
                Extract { iso_path: "images/install.img".into(), dest: "images\\install.img".into(), sha256: None },
            ],
        },
        Part {
            url: s(&d["Url"]),
            sha256: s(&d["IsoSha256"]),
            extracts: vec![Extract { iso_path: s(&d["ImagePath"]), dest: format!("upgrade_\\LiveOS\\{desktop}.squashfs"), sha256: Some(s(&d["ImageSha256"])) }],
        },
    ];
    for p in &parts {
        if !p.url.starts_with("https://") {
            return Err(format!("{}: the table's address is not https ({})", s(&r["Name"]), p.url));
        }
        if !hex64(&p.sha256) || p.extracts.iter().any(|x| x.sha256.as_deref().is_some_and(|h| !hex64(h))) || p.extracts.iter().any(|x| x.iso_path.is_empty()) {
            return Err(format!("{}: a sha256 or a path in the table is missing or malformed", s(&r["Name"])));
        }
    }
    // release.json: the same shape make-kit.sh writes, so the job writer reads it as before
    let rel = serde_json::json!({
        "id": s(&r["Id"]), "name": s(&r["Name"]), "family": family,
        "release": s(&r["Id"]).rsplit('-').next().unwrap_or(""), "measured": s(&r["Measured"]), "desktop": desktop,
    });
    let release_json = serde_json::to_string_pretty(&rel).map_err(|e| e.to_string())? + "\n";
    let mut boot_chain = vec![];
    for b in &boot {
        let role = s(&b["Role"]);
        let Some(which) = role.strip_prefix("installed ") else { continue };
        let stem = s(&b["File"]);
        let stem = stem.rsplit_once('.').map(|(a, _)| a.to_string()).unwrap_or(stem);
        let name = format!("{which}-{stem}");
        boot_chain.push((format!("upgrade_\\boot-chain\\{name}.sbat"), s(&b["Sbat"]) + "\n"));
        let levels: Vec<String> = match &b["SbatLevel"] {
            Value::Array(a) => a.iter().map(s).collect(),
            Value::String(x) if !x.is_empty() => vec![x.clone()],
            _ => vec![],
        };
        if !levels.is_empty() {
            boot_chain.push((format!("upgrade_\\boot-chain\\{name}.sbatlevel"), levels.join("\n") + "\n"));
        }
    }
    Ok(Plan { id: s(&r["Id"]), name: s(&r["Name"]), desktop: desktop.into(), desktop_label: s(&d["Label"]), family, measured: s(&r["Measured"]), parts, release_json, boot_chain })
}

/// Is this stick path one the release owns (replaced whole when another release is fetched)?
pub fn release_owned(rel: &str) -> bool {
    let r = rel.trim_start_matches(".\\").to_ascii_lowercase();
    ["efi\\boot\\bootx64.efi", "efi\\boot\\grubx64.efi", "images\\pxeboot\\vmlinuz", "images\\pxeboot\\initrd.img", "images\\install.img", "release.json"].contains(&r.as_str())
        || (r.starts_with("upgrade_\\liveos\\") && r.ends_with(".squashfs"))
        || r.starts_with("upgrade_\\boot-chain\\")
}

#[cfg(test)]
mod tests {
    use super::*;
    use upgrade_scan::data::RELEASES_JSON;

    #[test]
    fn fedora_44_kde_from_the_compiled_table() {
        let p = plan(RELEASES_JSON, "fedora-44", "kde").unwrap();
        assert_eq!(p.parts.len(), 2);
        assert!(p.parts[0].url.ends_with("Fedora-Server-netinst-x86_64-44-1.7.iso"));
        assert_eq!(p.parts[0].sha256, "ae20c06bea746913cadea7d80463e13f4bf55bee4df2918111c921c674b70283");
        assert_eq!(p.parts[0].extracts[0].sha256.as_deref(), Some("571ea56b855dcf73bec6acb63c5ded44c2a191138bca0d8cfa5aa93f60f46fff"));
        assert_eq!(p.parts[1].extracts[0].dest, "upgrade_\\LiveOS\\kde.squashfs");
        assert_eq!(p.parts[1].extracts[0].sha256.as_deref(), Some("2b6c1d5727467be858481f16a9131f16fbcaf0cf77bf57139987446825a71330"));
        let rel: Value = serde_json::from_str(&p.release_json).unwrap();
        assert_eq!((rel["id"].as_str(), rel["release"].as_str(), rel["desktop"].as_str()), (Some("fedora-44"), Some("44"), Some("kde")));
        assert!(p.boot_chain.iter().any(|(n, _)| n == "upgrade_\\boot-chain\\kde-shimx64.sbat"));
        assert!(p.boot_chain.iter().any(|(n, _)| n == "upgrade_\\boot-chain\\kde-shimx64.sbatlevel"));
    }

    #[test]
    fn the_table_offers_fedora_44_alone_with_two_desktops() {
        let o = offered(RELEASES_JSON).unwrap();
        assert_eq!(o.iter().map(|x| format!("{}/{}", x.0, x.2)).collect::<Vec<_>>(), ["fedora-44/kde", "fedora-44/gnome"]);
    }

    #[test]
    fn unknown_releases_desktops_and_families_refuse() {
        assert!(plan(RELEASES_JSON, "fedora-99", "kde").unwrap_err().contains("no release"));
        assert!(plan(RELEASES_JSON, "fedora-44", "xfce").unwrap_err().contains("no desktop"));
        let other = RELEASES_JSON.replace("\"Family\": \"fedora\"", "\"Family\": \"bazzite\"");
        assert!(plan(&other, "fedora-44", "kde").unwrap_err().contains("only carry Fedora's layout"));
        let http = RELEASES_JSON.replace("https://dl.fedoraproject.org", "http://dl.fedoraproject.org");
        assert!(plan(&http, "fedora-44", "kde").unwrap_err().contains("not https"));
        let bad = RELEASES_JSON.replace("ae20c06bea746913cadea7d80463e13f4bf55bee4df2918111c921c674b70283", "ae20");
        assert!(plan(&bad, "fedora-44", "kde").unwrap_err().contains("malformed"));
    }

    #[test]
    fn the_release_owns_its_files_and_nothing_else() {
        for p in ["EFI\\BOOT\\BOOTX64.EFI", "images\\install.img", "upgrade_\\LiveOS\\gnome.squashfs", "release.json", "upgrade_\\boot-chain\\kde-shimx64.sbat", ".\\EFI\\BOOT\\grubx64.efi"] {
            assert!(release_owned(p), "{p}");
        }
        for p in ["UPGRADE.exe", "EFI\\BOOT\\grub.cfg", "EFI\\BOOT\\grubenv", "upgrade_\\upgrade-installer", "upgrade_\\settle-in\\settle-in", "SHA256SUMS"] {
            assert!(!release_owned(p), "{p}");
        }
    }
}
