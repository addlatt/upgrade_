//! The writer's judging half, following Write-UpgradeStick.ps1 0.1.1: every
//! reason one disk must not be written, the whole decision over a set of
//! attached disks, the word the person must type, the manifest's lines,
//! the disk line and the evidence row. Pure: it reads no machine.

use serde_json::{json, Value};
use upgrade_scan::ps::{eq_ci, fmt_n, num, re, round1};

/// `$v.K` as a string, nothing when absent or null.
pub fn s(v: &Value, k: &str) -> String {
    match v.get(k) {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(t)) => t.clone(),
        Some(Value::Bool(b)) => if *b { "True".into() } else { "False".into() },
        Some(other) => other.to_string(),
    }
}

pub fn int(v: &Value, k: &str) -> i64 {
    v.get(k).and_then(|x| x.as_i64().or_else(|| x.as_f64().map(|f| f as i64))).unwrap_or(0)
}

pub fn flag(v: &Value, k: &str) -> bool {
    v.get(k).is_some_and(|x| x.as_bool().unwrap_or(false))
}

pub fn items(v: &Value, k: &str) -> Vec<Value> {
    v.get(k).and_then(Value::as_array).cloned().unwrap_or_default()
}

/// Test-UpgStickSize: exact bytes when the caller has them, else within
/// 10 % of the decimal gigabytes printed on the packaging.
pub fn size_matches(size_bytes: i64, expected_bytes: i64, expected_gb: f64) -> bool {
    if expected_bytes > 0 {
        return size_bytes == expected_bytes;
    }
    if expected_gb > 0.0 {
        let exp = expected_gb * 1e9;
        return (size_bytes as f64 - exp).abs() <= exp * 0.10;
    }
    false
}

/// Get-UpgDiskRefusals: every reason ONE disk must not be written; empty
/// means a writable candidate. The identity test is here too.
pub fn refusals(disk: &Value, target: &str, expected_bytes: i64, expected_gb: f64) -> Vec<String> {
    let mut r = Vec::new();
    if !eq_ci(&s(disk, "UniqueId"), target) {
        r.push("not the device pointed at".to_string());
    }
    let bus = s(disk, "BusType");
    if !eq_ci(&bus, "USB") {
        r.push(format!("bus is {bus}, not USB"));
    }
    let media = s(disk, "MediaType");
    if eq_ci(&media, "HDD") || eq_ci(&media, "SSD") {
        r.push(format!("media type {media} - a drive, not a stick"));
    }
    if flag(disk, "IsSystem") {
        r.push("is the system disk".into());
    }
    if flag(disk, "IsBoot") {
        r.push("is the boot disk".into());
    }
    if flag(disk, "HoldsSystemLetter") {
        r.push("holds a volume Windows is running from".into());
    }
    if flag(disk, "IsOffline") {
        r.push("is offline".into());
    }
    if flag(disk, "IsReadOnly") {
        r.push("is read-only".into());
    }
    let size = int(disk, "SizeBytes");
    if !size_matches(size, expected_bytes, expected_gb) {
        let shown = if expected_bytes > 0 {
            format!("{expected_bytes} bytes")
        } else if expected_gb > 0.0 {
            format!("{} GB", num(expected_gb))
        } else {
            "no expected size given".to_string()
        };
        r.push(format!("size {size} bytes does not match {shown}"));
    }
    r
}

/// Resolve-UpgStickTarget: the whole decision for a set of attached disks:
/// `{Decision, Reason, Chosen, Rows: [{Disk, Refusals}]}`.
pub fn resolve(disks: &[Value], target: &str, expected_bytes: i64, expected_gb: f64) -> Value {
    let rows: Vec<Value> = disks.iter().map(|d| json!({"Disk": d, "Refusals": refusals(d, target, expected_bytes, expected_gb)})).collect();
    let matches: Vec<&Value> = rows.iter().filter(|r| eq_ci(&s(&r["Disk"], "UniqueId"), target)).collect();
    let candidates: Vec<&Value> = rows.iter().filter(|r| r["Refusals"].as_array().is_some_and(|l| l.is_empty())).collect();
    let (mut decision, mut reason, mut chosen) = ("refused", String::new(), Value::Null);
    if target.trim().is_empty() {
        reason = "no target given".into();
    } else if matches.is_empty() {
        reason = "the device pointed at is not attached".into();
    } else if matches.len() > 1 {
        reason = format!("ambiguous: {} attached disks carry the id {target}", matches.len());
    } else if candidates.len() == 1 && eq_ci(&s(&candidates[0]["Disk"], "UniqueId"), target) {
        decision = "selected";
        chosen = candidates[0]["Disk"].clone();
    } else {
        let why: Vec<String> = matches[0]["Refusals"].as_array().into_iter().flatten().map(|x| x.as_str().unwrap_or("").to_string()).collect();
        reason = format!("the device pointed at is refused: {}", why.join("; "));
    }
    json!({"Decision": decision, "Reason": reason, "Chosen": chosen, "Rows": rows})
}

/// Get-UpgConfirmWord: the current label if there is one, else the model
/// name. Never a bare "yes".
pub fn confirm_word(disk: &Value) -> String {
    for v in items(disk, "Volumes") {
        let label = s(&v, "Label");
        if !label.is_empty() {
            return label;
        }
    }
    s(disk, "FriendlyName").trim().to_string()
}

/// ConvertFrom-UpgSha256Sums: `sha256sum -c` lines (`<hex>  ./path`) as
/// path (backslashes, no leading `./`) to lower-case hex. Keys compare
/// without regard to case, as a PowerShell hashtable's do.
pub fn sha256sums(lines: &[String]) -> Vec<(String, String)> {
    let mut m: Vec<(String, String)> = Vec::new();
    let rx = re(r"^([0-9a-fA-F]{64})\s[\s*](.+)$", "");
    for l in lines {
        if let Some(found) = rx.find(l) {
            let (Some(h), Some(p)) = (found.group(1), found.group(2)) else { continue };
            let path = l[p].trim().trim_start_matches("./").replace('/', "\\");
            let hex = l[h].to_lowercase();
            match m.iter_mut().find(|(k, _)| eq_ci(k, &path)) {
                Some(entry) => entry.1 = hex,
                None => m.push((path, hex)),
            }
        }
    }
    m
}

/// Format-DiskLine: `  disk N  <name, 28 wide> <GB, 7 wide> GB  <bus> <media> [SYSTEM] <volumes>`.
pub fn disk_line(disk: &Value) -> String {
    let vols: Vec<String> = items(disk, "Volumes")
        .iter()
        .map(|v| {
            let letter = s(v, "DriveLetter");
            let mut t = if letter.is_empty() { "-".to_string() } else { format!("{letter}:") };
            let label = s(v, "Label");
            if !label.is_empty() {
                t.push(' ');
                t.push_str(&label);
            }
            let fs = s(v, "FileSystem");
            if !fs.is_empty() {
                t.push(' ');
                t.push_str(&fs);
            }
            t
        })
        .collect();
    let system = if flag(disk, "IsSystem") || flag(disk, "IsBoot") { "[SYSTEM] " } else { "" };
    format!("  disk {}  {:<28} {:>7} GB  {:<5} {:<11} {}{}", int(disk, "Number"), s(disk, "FriendlyName"), fmt_n(int(disk, "SizeBytes") as f64 / 1e9, 1), s(disk, "BusType"), s(disk, "MediaType"), system, vols.join(", "))
}

/// The per-disk words of the evidence row's notes.
pub fn per_disk_notes(rows: &[Value]) -> String {
    rows.iter()
        .map(|r| {
            let d = &r["Disk"];
            let why: Vec<String> = r["Refusals"].as_array().into_iter().flatten().map(|x| x.as_str().unwrap_or("").to_string()).collect();
            format!("disk{}[{}/{}/{}]={}", int(d, "Number"), s(d, "BusType"), s(d, "MediaType"), int(d, "SizeBytes"), if why.is_empty() { "writable".to_string() } else { why.join(";") })
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

/// `[long]($ExpectedSizeGB * 1e9)`: the bytes a typed size means (a half
/// rounds to the even number, as a .NET cast does).
pub fn expected_bytes(expected_bytes: i64, expected_gb: f64) -> i64 {
    if expected_bytes > 0 {
        expected_bytes
    } else if expected_gb > 0.0 {
        (expected_gb * 1e9).round_ties_even() as i64
    } else {
        0
    }
}

pub const CSV_HEADER: &str = "timestamp,writer,machine,os_build,mode,disks_attached,usb_disks,target,expected_bytes,decision,refusals,written,verified,notes";

/// Write-EvidenceRow's line: every field quoted, quotes doubled.
pub fn evidence_line(fields: &[String]) -> String {
    fields.iter().map(|f| format!("\"{}\"", f.replace('"', "\"\""))).collect::<Vec<_>>().join(",")
}

/// `[math]::Round(bytes / 1e9, 1)` inside a "..." string.
pub fn gb1(bytes: i64) -> String {
    num(round1(bytes as f64 / 1e9))
}
