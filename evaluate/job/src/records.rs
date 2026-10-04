//! Facts that are carried into the job as records: the clock, the Windows
//! licence, remote access, the installed programs. None of them is a key or
//! a password.

use crate::val::{at, eq_ci, int, items, s, truthy};
use regress::Regex;
use serde_json::{json, Value};
use std::cell::Cell;
use upgrade_scan::ps::matches;

/// ConvertTo-JobClock. Windows keeps the hardware clock in local time unless
/// told otherwise (the Aspire's installer clock was hours off for this
/// reason). `real_time_is_universal`: absent or 0 = local time, 1 = UTC.
/// Any other value is not guessed at: it is a refusal.
#[allow(clippy::too_many_arguments)]
pub fn clock(windows_zone: &str, iana: &str, real_time_is_universal: &Value, dynamic_dst_disabled: &Value, offset_minutes: i64, base_offset_minutes: i64, dst_active: bool, now_utc: &str) -> Result<Value, String> {
    let shown = s(real_time_is_universal);
    let (rtu, local) = match (real_time_is_universal.is_null(), shown.as_str()) {
        (true, _) => (Value::Null, true),
        (_, "0") => (json!(0), true),
        (_, "1") => (json!(1), false),
        _ => return Err(format!("Windows' RealTimeIsUniversal setting is '{shown}', neither 0 nor 1 - whether the hardware clock holds local time or UTC is not known, and it is not guessed")),
    };
    Ok(json!({
        "windows_zone": windows_zone, "iana": iana,
        "rtc_is_local": local, "real_time_is_universal": rtu,
        "dst_auto_adjust": s(dynamic_dst_disabled) != "1",
        "utc_offset_minutes": offset_minutes, "base_utc_offset_minutes": base_offset_minutes, "dst_active": dst_active,
        "observed_utc": now_utc,
    }))
}

/// ConvertTo-JobLicense (RISKS R30): Windows' licence facts, for the way
/// back to Windows. Facts, never a key: any value shaped like a product key
/// is left out, and the record says so. A failed read is `unreadable` with
/// its reason. It costs a less informed way back, never data, so it is not
/// a refusal.
pub fn license(os: &Value, products: &Value, firmware: &Value, read_error: &str, now_utc: &str) -> Value {
    let dropped = Cell::new(false);
    let clean = |text: String| -> Value {
        if text.is_empty() {
            return Value::Null;
        }
        if matches("[A-Za-z0-9]{5}-[A-Za-z0-9]{5}-[A-Za-z0-9]{5}-[A-Za-z0-9]{5}-[A-Za-z0-9]{5}", &text) {
            dropped.set(true);
            return Value::Null;
        }
        json!(text)
    };
    let build_text = s(at(os, "Build"));
    let build: Option<i64> = if !build_text.is_empty() && build_text.bytes().all(|b| b.is_ascii_digit()) { build_text.parse().ok() } else { None };
    let version = build.filter(|b| *b != 0).map(|b| if b >= 22000 { "11" } else { "10" });
    let mut result = "read";
    let mut reason = Value::Null;
    let (mut activated, mut status, mut channel, mut fw_present, mut fw_description) = (Value::Null, Value::Null, Value::Null, Value::Null, Value::Null);
    if !read_error.is_empty() {
        result = "unreadable";
        reason = clean(format!("Windows' licensing service could not be read ({read_error})"));
        if reason.is_null() {
            reason = json!("Windows' licensing service could not be read");
        }
    } else {
        // the Windows licence itself, not an add-on; a licensed one first
        let main: Vec<&Value> = items(products).into_iter().filter(|p| !p.is_null() && !truthy(at(p, "Addon"))).collect();
        let licensed = |p: &&&Value| int(at(p, "LicenseStatus")) == 1;
        match main.iter().find(licensed).or_else(|| main.iter().find(|p| !licensed(p))) {
            Some(p) => {
                let st = int(at(p, "LicenseStatus"));
                if (0..=6).contains(&st) {
                    status = json!(st);
                }
                activated = json!(st == 1);
                channel = clean(s(at(p, "Channel")));
            }
            None => {
                result = "unreadable";
                reason = json!("Windows reported no installed Windows licence");
            }
        }
        if !firmware.is_null() {
            fw_present = json!(truthy(at(firmware, "Present")));
            fw_description = clean(s(at(firmware, "Description")));
        }
    }
    let (edition, product, display) = (clean(s(at(os, "EditionId"))), clean(s(at(os, "ProductName"))), clean(s(at(os, "DisplayVersion"))));
    if dropped.get() {
        let before = if truthy(&reason) { format!("{}; ", s(&reason)) } else { String::new() };
        reason = json!(format!("{before}a value shaped like a product key was left out"));
    }
    json!({
        "result": result, "reason": reason, "windows_version": version,
        "edition_id": edition, "product_name": product, "display_version": display, "build": build,
        "activated": activated, "license_status": status, "channel": channel,
        "firmware_key_present": fw_present, "firmware_key_description": fw_description,
        "observed_utc": now_utc,
    })
}

/// ConvertTo-JobSsh (decided 2026-10-04, the owner): remote access is
/// carried only if Windows already had it on, and only as PUBLIC keys.
/// `start_type`: the sshd service's start type (nothing = not installed).
/// `key_files`: `[{Path, Lines, Error}]`. One unreadable file carries
/// nothing at all, never half.
pub fn ssh(start_type: &Value, key_files: &Value, read_error: &str) -> Value {
    let out = |result: &str, keys: Vec<String>, sources: Vec<String>, why: Value| json!({"result": result, "keys": keys, "sources": sources, "why": why});
    if !read_error.is_empty() {
        return out("unreadable", vec![], vec![], json!(read_error));
    }
    let start = s(start_type);
    if start.is_empty() {
        return out("not-installed", vec![], vec![], json!("no OpenSSH server in Windows"));
    }
    if !eq_ci(&start, "Automatic") {
        return out("off", vec![], vec![], json!(format!("Windows' OpenSSH server is set to {start}, not to start by itself")));
    }
    // case matters here, as it does to OpenSSH. A line with options in front
    // (from=..., command=...) is a restriction Windows enforced: not carried,
    // rather than carried without it.
    let key_line = Regex::new(r"^(ssh-ed25519|ssh-rsa|ecdsa-sha2-nistp256|ecdsa-sha2-nistp384|ecdsa-sha2-nistp521|sk-ssh-ed25519@openssh\.com|sk-ecdsa-sha2-nistp256@openssh\.com) [A-Za-z0-9+/]+={0,3}( [^\r\n]*)?$").expect("a fixed pattern");
    let (mut keys, mut sources): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    for f in items(key_files) {
        if !truthy(f) {
            continue;
        }
        if truthy(at(f, "Error")) {
            return out("unreadable", vec![], vec![], json!(format!("{}: {}", s(at(f, "Path")), s(at(f, "Error")))));
        }
        let mut took = false;
        for line in items(at(f, "Lines")) {
            let t = s(line).trim().to_string();
            if key_line.find(&t).is_some() && !keys.contains(&t) {
                keys.push(t);
                took = true;
            }
        }
        if took {
            sources.push(s(at(f, "Path")));
        }
    }
    if keys.is_empty() {
        return out("no-keys", vec![], vec![], json!("the OpenSSH server allowed no key (password sign-in is not carried)"));
    }
    out("carried", keys, sources, Value::Null)
}

/// ConvertTo-JobSoftware: the registry's Apps & features entries and the
/// person's Store packages as names. Drops what Apps & features hides,
/// Windows updates and hotfixes, nameless entries, Store frameworks and
/// system packages; one entry per name; sorted; capped, and says so.
///
/// The order differs from PowerShell's on purpose: PowerShell sorts by the
/// machine's language rules, this sorts by the lower-cased name, the same
/// on every machine (docs/RUST-PORT.md, "Differences kept on purpose").
pub fn software(desktop: &Value, store: &Value, cap: usize) -> Value {
    let text_or_null = |v: &Value| if truthy(v) { json!(s(v)) } else { Value::Null };
    let mut d: Vec<(String, Value)> = Vec::new();
    for e in items(desktop) {
        let name = s(at(e, "DisplayName")).trim().to_string();
        if name.is_empty() || s(at(e, "SystemComponent")) == "1" {
            continue;
        }
        if matches(r"^(Security Update|Update|Hotfix|Service Pack)\b.* for ", &name) || matches(r"^KB\d{6,}", &name) {
            continue;
        }
        if !d.iter().any(|(n, _)| eq_ci(n, &name)) {
            let entry = json!({"name": name, "version": text_or_null(at(e, "DisplayVersion")), "publisher": text_or_null(at(e, "Publisher"))});
            d.push((name, entry));
        }
    }
    let mut st: Vec<(String, Value)> = Vec::new();
    for e in items(store) {
        if truthy(at(e, "IsFramework")) || eq_ci(&s(at(e, "SignatureKind")), "System") || truthy(at(e, "NonRemovable")) {
            continue;
        }
        let mut name = s(at(e, "DisplayName")).trim().to_string();
        if name.is_empty() || matches("^ms-resource:", &name) {
            name = s(at(e, "Name")).trim().to_string();
        }
        if name.is_empty() {
            continue;
        }
        if !st.iter().any(|(n, _)| eq_ci(n, &name)) {
            let publisher = if truthy(at(e, "PublisherDisplayName")) { json!(s(at(e, "PublisherDisplayName"))) } else { text_or_null(at(e, "Publisher")) };
            let entry = json!({"name": name, "package": text_or_null(at(e, "Name")), "version": text_or_null(at(e, "Version")), "publisher": publisher});
            st.push((name, entry));
        }
    }
    let sorted = |mut l: Vec<(String, Value)>| {
        l.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()).then_with(|| a.0.cmp(&b.0)));
        l
    };
    let (d, st) = (sorted(d), sorted(st));
    let truncated = d.len() > cap || st.len() > cap;
    let take = |l: Vec<(String, Value)>| l.into_iter().take(cap).map(|(_, v)| v).collect::<Vec<_>>();
    json!({"desktop": take(d), "store": take(st), "truncated": truncated})
}
