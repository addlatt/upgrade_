//! The firmware's own words: its SBAT revocation level, the signing
//! authorities it trusts, and the boot files on the kit beside the scanner
//! (Get-UpgSbatFacts, Get-UpgDbAuthorities).

use super::registry::{self, Hive};
use super::win::{enable_privilege, firmware_variable};
use crate::parse::{db_authorities, pe_section};
use serde_json::{json, Value};
use windows::Win32::Security::SE_SYSTEM_ENVIRONMENT_NAME;

const SBAT_KEY: &str = r"SYSTEM\CurrentControlSet\Control\SecureBoot\SBAT";
const SHIM_GUID: &str = "{605dab50-e046-4300-abb6-3dd810dd8b23}";
const SECURITY_DB_GUID: &str = "{d719b2cb-3d3a-4596-a3bc-dad00e67656f}";

/// Get-UpgSbatFacts. `root`: where the kit sits (the stick's root), or
/// nothing. `{ Levels: [{Source, Text}], Files: [{Name, Sbat, Error}] }`.
pub fn sbat_facts(root: Option<&std::path::Path>, is_admin: bool) -> Value {
    let (mut levels, mut files) = (Vec::new(), Vec::new());
    if let Some((_, bytes)) = registry::value(&Hive::LocalMachine, SBAT_KEY, "SbatLevel") {
        let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
        let level = String::from_utf8_lossy(&bytes[..end]).to_string();
        if !level.is_empty() {
            levels.push(json!({"Source": "Windows (registry)", "Text": level}));
        }
    }
    if is_admin && enable_privilege(SE_SYSTEM_ENVIRONMENT_NAME) {
        if let Some(bytes) = firmware_variable("SbatLevelRT", SHIM_GUID) {
            let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
            let level = String::from_utf8_lossy(&bytes[..end]).to_string();
            levels.push(json!({"Source": "firmware (SbatLevelRT)", "Text": level}));
        }
    }
    if let Some(root) = root.filter(|r| r.join("EFI").join("BOOT").join("BOOTX64.EFI").is_file()) {
        for name in ["BOOTX64.EFI", "grubx64.efi"] {
            let path = root.join("EFI").join("BOOT").join(name);
            let mut file = json!({"Name": format!("stick {name}"), "Sbat": null, "Error": null});
            match std::fs::read(&path) {
                Ok(bytes) => {
                    file["Sbat"] = json!(pe_section(&bytes, ".sbat"));
                    if name == "BOOTX64.EFI" {
                        // shim applies its own built-in level too; count the newest it carries
                        if let Some(level) = pe_section(&bytes, ".sbatlevel") {
                            let re = regress::Regex::new(r"sbat,1,\d{10}[^\x00]*").expect("a fixed pattern");
                            let found: Vec<String> = re.find_iter(&level).map(|m| level[m.range()].to_string()).collect();
                            levels.push(json!({"Source": "the stick's shim (built in)", "Text": found.join("\n")}));
                        }
                    }
                }
                Err(e) => file["Error"] = json!(format!("not readable: {e}")),
            }
            files.push(file);
        }
        let chain = root.join("upgrade_").join("boot-chain");
        if chain.is_dir() {
            let mut entries: Vec<_> = std::fs::read_dir(&chain).into_iter().flatten().flatten().map(|e| e.path()).collect();
            entries.sort();
            for p in entries.iter().filter(|p| p.extension().is_some_and(|x| x == "sbat")) {
                let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                files.push(json!({"Name": format!("installed {stem}"), "Sbat": std::fs::read_to_string(p).ok(), "Error": null}));
            }
            for p in entries.iter().filter(|p| p.extension().is_some_and(|x| x == "sbatlevel")) {
                let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                levels.push(json!({"Source": format!("installed {stem} (built in)"), "Text": std::fs::read_to_string(p).ok()}));
            }
        }
    }
    json!({"Levels": levels, "Files": files})
}

/// Get-UpgDbAuthorities: the names in the firmware's `db`. Nothing when it
/// cannot be read (not elevated, or no Secure Boot variables).
pub fn trusted_authorities() -> Option<Vec<String>> {
    if !enable_privilege(SE_SYSTEM_ENVIRONMENT_NAME) {
        return None;
    }
    firmware_variable("db", SECURITY_DB_GUID).map(|bytes| db_authorities(&bytes))
}
