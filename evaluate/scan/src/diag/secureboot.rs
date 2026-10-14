//! The Secure Boot revocation diagnostic (`Diag-SecureBoot.ps1`, added
//! 2026-10-03 after the Aspire's run 10, RISKS R34): the revocation level
//! the firmware holds (`SbatLevel`, `SbatLevelRT` under shim's vendor GUID),
//! the size and sha256 of `db` and `dbx`, Windows' Secure Boot servicing
//! keys in the registry, its Secure Boot update events of the last 90 days,
//! and the `.sbat` section of the stick's two boot files.

use super::{ascii, collapse, hexbytes, sha256_hex};
use crate::parse::pe_section;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const SHIM_GUID: &str = "{605dab50-e046-4300-abb6-3dd810dd8b23}";
/// EFI_IMAGE_SECURITY_DATABASE_GUID: where `db` and `dbx` live.
pub const SECURITY_DB_GUID: &str = "{d719b2cb-3d3a-4596-a3bc-dad00e67656f}";
/// EFI_GLOBAL_VARIABLE_GUID: where `SecureBoot` lives.
pub const GLOBAL_GUID: &str = "{8be4df61-93ca-11d2-aa0d-00e098032b8c}";
pub const REGISTRY_KEY: &str = r"SYSTEM\CurrentControlSet\Control\SecureBoot";
pub const VARIABLES: [&str; 2] = ["SbatLevel", "SbatLevelRT"];
pub const DATABASES: [&str; 2] = ["db", "dbx"];
pub const PROVIDERS: [&str; 2] = ["Microsoft-Windows-TPM-WMI", "Microsoft-Windows-Kernel-Boot"];
pub const FILES: [&str; 2] = [r"EFI\BOOT\grubx64.efi", r"EFI\BOOT\BOOTX64.EFI"];
pub const DAYS: i64 = 90;

/// The words the script used for a variable it could not read; 203 is
/// ERROR_ENVVAR_NOT_FOUND, the usual answer for a variable that is not there.
pub fn variable_error(code: u32) -> String {
    format!("error {code} (203 = not present or not readable from Windows)")
}

pub fn variable_ok(attributes: u32) -> String {
    format!("attributes 0x{attributes:x}")
}

/// `Get-SecureBootUEFI` and `Confirm-SecureBootUEFI`, run without
/// elevation, say this.
pub const NOT_ELEVATED: &str = "Unable to set proper privileges. Access was denied.";

/// A Secure Boot update event is one of these ids (the script's own filter).
pub fn event_wanted(id: i64) -> bool {
    (1030..=1050).contains(&id) || (1795..=1810).contains(&id)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Variable {
    pub name: String,
    /// `attributes 0x7`, or `error 203 (...)`
    pub status: String,
    #[serde(with = "hexbytes")]
    pub bytes: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Database {
    pub name: String,
    #[serde(with = "hexbytes")]
    pub bytes: Option<Vec<u8>>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct RegValue {
    pub name: String,
    /// What `.GetValue()` gave: a number, a string, or a list of either
    /// (a binary value's bytes, a multi-string's strings).
    pub data: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Key {
    /// `HKLM\SYSTEM\...`
    pub key: String,
    pub values: Vec<RegValue>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Event {
    /// UTC, as `.ToUniversalTime().ToString('o')` prints it
    pub time_utc: String,
    pub provider: String,
    pub id: i64,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct File {
    pub name: String,
    /// nothing = not on this stick
    #[serde(with = "hexbytes")]
    pub bytes: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Facts {
    /// UTC, `2026-10-14T16:01:02.1234567Z`
    pub now: String,
    pub computer: String,
    /// `ok`, or which call failed and its error number
    pub privilege: String,
    pub variables: Vec<Variable>,
    pub databases: Vec<Database>,
    /// `True`, `False`, or the error
    pub confirm_secure_boot: String,
    /// the key and its subkeys, depth first, each with its values in order
    pub registry: Vec<Key>,
    /// per provider in `PROVIDERS` order, newest first within a provider
    pub events: Vec<Event>,
    pub files: Vec<File>,
}

/// `$k.GetValue($p) -join ' '`.
pub fn reg_text(data: &Value) -> String {
    let one = |v: &Value| -> String {
        match v {
            Value::String(s) => s.clone(),
            Value::Null => String::new(),
            Value::Bool(b) => if *b { "True".into() } else { "False".into() },
            other => other.to_string(),
        }
    };
    match data {
        Value::Array(items) => items.iter().map(one).collect::<Vec<_>>().join(" "),
        other => one(other),
    }
}

/// The entries of the text file, in the script's order and words.
pub fn render(f: &Facts) -> Vec<String> {
    let mut o: Vec<String> = Vec::new();
    o.push(format!("== upgrade_ Secure Boot diagnostic {} {}", f.now, f.computer));
    o.push(format!("privilege: {}", f.privilege));
    for v in &f.variables {
        o.push(String::new());
        o.push(format!("== {} {SHIM_GUID}  {}", v.name, v.status));
        if let Some(b) = v.bytes.as_deref().filter(|b| !b.is_empty()) {
            o.push(ascii(b));
        }
    }
    for d in &f.databases {
        o.push(String::new());
        match &d.bytes {
            Some(b) => o.push(format!("== {}  {} bytes  sha256 {}", d.name, b.len(), sha256_hex(b))),
            None => o.push(format!("== {}  not read: {}", d.name, d.error.as_deref().unwrap_or(""))),
        }
    }
    o.push(String::new());
    o.push(format!("== Confirm-SecureBootUEFI: {}", f.confirm_secure_boot));
    o.push(String::new());
    o.push(format!("== registry HKLM\\{REGISTRY_KEY}"));
    for k in &f.registry {
        o.push(format!("  [{}]", k.key));
        for v in &k.values {
            o.push(format!("    {} = {}", v.name, reg_text(&v.data)));
        }
    }
    o.push(String::new());
    o.push("== Secure Boot update events, last 90 days (TPM-WMI, Kernel-Boot)".into());
    for e in &f.events {
        o.push(format!("  {}  {} {}  {}", e.time_utc, e.provider, e.id, collapse(&e.message).trim()));
    }
    for file in &f.files {
        o.push(String::new());
        match &file.bytes {
            None => o.push(format!("== {}  not on this stick", file.name)),
            Some(b) => {
                o.push(format!("== {}  sha256 {}", file.name, sha256_hex(b)));
                if let Some(text) = pe_section(b, ".sbat") {
                    o.push(text.trim_end_matches('\0').to_string());
                }
            }
        }
    }
    o
}

/// Where the script wrote its file: `<root>\upgrade_\report\secureboot-diag-<yyyyMMddTHHmmssZ>.txt`.
pub fn default_out(root: &std::path::Path, now: &str) -> std::path::PathBuf {
    let digits: String = now.chars().take(19).filter(|c| c.is_ascii_digit() || *c == 'T').collect();
    root.join("upgrade_").join("report").join(format!("secureboot-diag-{digits}Z.txt"))
}
