//! `upgrade-scan diag`: the three read-only diagnostics that were
//! `Diag-SecureBoot.ps1`, `Diag-Smart.ps1` and the inline PowerShell of
//! `DIAG-VOLUME.cmd` on the stick (phase 2 of docs/RUST-PORT.md, 2026-10-14). Each has a facts half (what was read; the
//! collectors in `collect::diag` fill it on Windows) and a render half (the
//! lines of the text file, held to the script's word for word). The render
//! halves are tested in `tests/diag.rs` against what the scripts answered
//! for made-up facts (`tests/diag-golden.json`, recorded from the scripts
//! through mocked cmdlets before they were deleted).
//!
//! Nothing here judges. A diagnostic prints what the machine says, for a
//! person to read; the scanner's checks make the refusals.

pub mod secureboot;
pub mod smart;
pub mod volume;

use crate::ps::Stamp;
use serde::{Deserialize, Deserializer, Serializer};

/// Bytes as lowercase hex, two digits each.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Hex back to bytes; anything that is not a hex pair is skipped.
pub fn unhex(text: &str) -> Vec<u8> {
    let digits: Vec<u8> = text.bytes().filter(u8::is_ascii_hexdigit).collect();
    digits.chunks_exact(2).map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap_or("0"), 16).unwrap_or(0)).collect()
}

/// `[Security.Cryptography.SHA256]::Create().ComputeHash(bytes)` as the
/// scripts printed it: lowercase hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex(&Sha256::digest(bytes))
}

/// `$text -replace '\s+', ' '`: every run of whitespace becomes one space.
pub fn collapse(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_space = false;
    for c in text.chars() {
        if c.is_whitespace() {
            if !in_space {
                out.push(' ');
                in_space = true;
            }
        } else {
            out.push(c);
            in_space = false;
        }
    }
    out
}

/// `[Text.Encoding]::ASCII.GetString(bytes).TrimEnd([char]0)`: a byte past
/// ASCII reads as `?`, trailing NULs go.
pub fn ascii(bytes: &[u8]) -> String {
    let s: String = bytes.iter().map(|b| if b.is_ascii() { *b as char } else { '?' }).collect();
    s.trim_end_matches('\0').to_string()
}

/// A date and time as `"{0}" -f $date` prints it on an English (United
/// States) Windows: `10/3/2026 4:01:02 PM`. The scripts printed the
/// machine's own culture; the port prints this one (docs/RUST-PORT.md,
/// "Differences kept on purpose").
pub fn en_us(t: &Stamp) -> String {
    let (hour, half) = match t.hour {
        0 => (12, "AM"),
        1..=11 => (t.hour, "AM"),
        12 => (12, "PM"),
        h => (h - 12, "PM"),
    };
    format!("{}/{}/{} {}:{:02}:{:02} {half}", t.month, t.day, t.year, hour, t.minute, t.second)
}

/// `2026-10-03T09:01:02.1234567` (a local time with its ticks) -> the stamp
/// to the second, for printing. The whole text orders the events.
pub fn stamp_of(text: &str) -> Option<Stamp> {
    Stamp::parse(text.get(..19)?)
}

/// serde for the byte fields: hex in JSON (the cases, a capture), bytes here.
pub mod hexbytes {
    use super::*;
    pub fn serialize<S: Serializer>(v: &Option<Vec<u8>>, s: S) -> Result<S::Ok, S::Error> {
        match v {
            Some(b) => s.serialize_str(&hex(b)),
            None => s.serialize_none(),
        }
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<u8>>, D::Error> {
        Ok(Option::<String>::deserialize(d)?.map(|t| unhex(&t)))
    }
}

/// `Set-Content -Encoding UTF8` on Windows PowerShell 5.1: a byte order
/// mark, then every entry followed by CRLF.
pub fn text_with_bom(lines: &[String]) -> String {
    let mut text = String::from("\u{feff}");
    for l in lines {
        text.push_str(l);
        text.push_str("\r\n");
    }
    text
}

/// `[IO.File]::WriteAllLines(path, lines, UTF8Encoding($false))`: no byte
/// order mark, every entry followed by CRLF.
pub fn text_without_bom(lines: &[String]) -> String {
    let mut text = String::new();
    for l in lines {
        text.push_str(l);
        text.push_str("\r\n");
    }
    text
}
