//! The BitLocker recovery key, for `settle-in`'s pull (decided 2026-11-07,
//! the owner; RISKS R13, R19). Read from Windows' own BitLocker provider,
//! never typed, and written only to the stick's credentials folder
//! (`artifacts/credentials/bitlocker-C.txt`), which the installer copies
//! into Linux's root-only handoff folder and `settle-in` deletes after the
//! pull. Never in a report, never in `job.json`.
//!
//! A recovery password is eight groups of six digits. Each group is a
//! 16-bit number times 11, so each is divisible by 11 and under 720,896:
//! a check that catches a mistyped or garbled key before it is relied on.
//!
//! The file, one fact per line, `#` lines are words for a person:
//!
//! ```text
//! # upgrade_: the BitLocker recovery key for C: (RISKS R13) ...
//! conversion-status: 1
//! 123456-...
//! ```
//!
//! `conversion-status` is `Win32_EncryptableVolume.ConversionStatus`: 0
//! decrypted, 1 encrypted, 2 to 5 encrypting or decrypting (part of the
//! volume each way, which settle-in refuses: R19's residue).

/// The file's name under the job's folder.
pub const KEY_FILE: &str = "artifacts/credentials/bitlocker-C.txt";

/// Eight groups of six digits, each a multiple of 11 below 720,896.
pub fn valid(key: &str) -> bool {
    let groups: Vec<&str> = key.split('-').collect();
    groups.len() == 8
        && groups.iter().all(|g| g.len() == 6 && g.bytes().all(|b| b.is_ascii_digit()) && g.parse::<u32>().is_ok_and(|n| n % 11 == 0 && n < 720_896))
}

/// Every valid recovery password in a text (manage-bde's output, in any
/// language: the digits are the same), in order, once each.
pub fn find(text: &str) -> Vec<String> {
    let b = text.as_bytes();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i + 55 <= b.len() {
        let cand = &text[i..i + 55];
        let edge_before = i == 0 || !b[i - 1].is_ascii_digit();
        let edge_after = i + 55 == b.len() || !b[i + 55].is_ascii_digit();
        if edge_before && edge_after && cand.is_ascii() && valid(cand) {
            if !out.iter().any(|k| k == cand) {
                out.push(cand.to_string());
            }
            i += 55;
        } else {
            i += 1;
        }
        while i < b.len() && !text.is_char_boundary(i) {
            i += 1;
        }
    }
    out
}

/// The file's text. `why` says why there is no key, when there is none.
pub fn file_text(keys: &[String], conversion_status: Option<i64>, why: Option<&str>, writer: &str) -> String {
    let mut t = String::new();
    t.push_str(&format!("# upgrade_: the BitLocker recovery key for C: (RISKS R13), read by upgrade-job {writer}.\n"));
    t.push_str("# It unlocks this computer's Windows drive. Linux copies it, uses it once to bring your files over, then deletes it.\n");
    t.push_str("# Keep your own copy somewhere that is not this computer (aka.ms/myrecoverykey).\n");
    match conversion_status {
        Some(c) => t.push_str(&format!("conversion-status: {c}\n")),
        None => t.push_str("conversion-status: unknown\n"),
    }
    for k in keys.iter().filter(|k| valid(k)) {
        t.push_str(k);
        t.push('\n');
    }
    if !keys.iter().any(|k| valid(k)) {
        t.push_str(&format!("# none: {}\n", why.unwrap_or("Windows gave no recovery password for C:")));
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    // a made-up key that passes the check (every group a multiple of 11)
    const K: &str = "000011-000022-000033-000044-000055-000066-000077-000088";

    #[test]
    fn the_check() {
        assert!(valid(K));
        assert!(valid("715000-001111-123453-000000-654324-111111-222222-333333"));
        assert!(!valid("000012-000022-000033-000044-000055-000066-000077-000088"), "12 is not a multiple of 11");
        assert!(!valid("720896-000022-000033-000044-000055-000066-000077-000088"), "past 65535 times 11");
        assert!(!valid("000011-000022-000033-000044-000055-000066-000077"));
        assert!(!valid("00011-000022-000033-000044-000055-000066-000077-0000880"));
    }

    #[test]
    fn found_in_manage_bde_in_any_language() {
        let en = format!("BitLocker Drive Encryption: Configuration Tool\n\nVolume C: [OS]\nAll Key Protectors\n\n    Numerical Password:\n      ID: {{1A2B}}\n      Password:\n        {K}\n\n    TPM:\n      ID: {{3C4D}}\n");
        assert_eq!(find(&en), [K]);
        let de = format!("Volume C: [Betriebssystem]\n    Numerisches Kennwort:\n      Kennwort:\n        {K}\n        {K}\n");
        assert_eq!(find(&de), [K], "once each");
        // a longer run of digits around it is not a key
        assert!(find(&format!("9{K}")).is_empty());
        assert!(find("ID: {000011-000022-000033-000044-000055-000066-000077-000089}").is_empty());
        assert!(find("äö ü").is_empty());
    }

    #[test]
    fn the_file() {
        let t = file_text(&[K.to_string(), "bogus".into()], Some(1), None, "0.30.0");
        assert!(t.contains("conversion-status: 1\n"));
        assert_eq!(t.lines().filter(|l| !l.starts_with('#') && !l.starts_with("conversion")).collect::<Vec<_>>(), [K]);
        let none = file_text(&[], None, Some("no numerical password protector"), "0.30.0");
        assert!(none.contains("conversion-status: unknown\n") && none.contains("# none: no numerical password protector\n"));
    }
}
