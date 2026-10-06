//! The pure readers: text or bytes that Windows' own tools gave, turned into
//! a fact or into "could not tell". None of them guesses. Text in another
//! language than English reads as unknown, never as fine.

use crate::facts::{ChkdskLog, DiskEvent, DiskEvents, VolumeHealth};
use crate::ps::{capture, matches, re, round1};
use std::collections::BTreeMap;

/// `PCI\VEN_8086&DEV_9A0B&...` -> `8086:9a0b`.
pub fn pci_id(device_id: &str) -> Option<String> {
    let m = re(r"PCI\\VEN_([0-9A-Fa-f]{4})&DEV_([0-9A-Fa-f]{4})", "").find(device_id)?;
    let (v, d) = (m.group(1)?, m.group(2)?);
    Some(format!("{}:{}", device_id[v].to_lowercase(), device_id[d].to_lowercase()))
}

/// diskpart's `shrink querymax` -> the GB that can be freed. The real line
/// (the rig, 2026-09-08): `The maximum number of reclaimable bytes is:   17 GB (17417 MB)`.
pub fn diskpart_query_max(lines: &[String]) -> Option<f64> {
    let text = lines.join("\n");
    let mb = capture(r"reclaimable bytes is:\s*[\d.,]+\s*[KMGT]?B\s*\(\s*([\d,]+)\s*MB\s*\)", "m", &text)
        .or_else(|| capture(r"reclaimable bytes is:\s*([\d,]+)\s*MB\b", "m", &text))?;
    let digits = mb.replace(',', "");
    // PowerShell reads an empty text as 0
    let value: f64 = if digits.is_empty() { 0.0 } else { digits.parse().ok()? };
    Some(round1(value / 1024.0))
}

/// `fsutil dirty query C:` -> `clean`, `dirty` or `unknown`.
pub fn fsutil_dirty(lines: &[String]) -> &'static str {
    let text = lines.join("\n");
    if matches(r"\bis\s+NOT\s+Dirty\b", &text) {
        "clean"
    } else if matches(r"\bis\s+Dirty\b", &text) {
        "dirty"
    } else {
        "unknown"
    }
}

/// The text of a Chkdsk event -> what it concluded.
pub fn chkdsk_event(message: &str) -> ChkdskLog {
    let mut r = ChkdskLog { verdict: "unknown".into(), records: 0, queued: 0 };
    if let Some(count) = capture(r"Examining\s+(\d+)\s+corruption records", "", message) {
        r.records = count.parse().unwrap_or(0);
    }
    r.queued = re("queued for offline repair", "").find_iter(message).count() as i64;
    if matches("found problems", message) {
        r.verdict = "found-problems".into();
    } else if matches("found no problems", message) {
        r.verdict = "no-problems".into();
    } else if r.queued > 0 {
        r.verdict = "found-problems".into();
    }
    r
}

/// The System log's disk events, counted for the one disk that holds C:.
/// The backslash after the number keeps Harddisk1 from matching Harddisk10.
pub fn disk_events(events: &[DiskEvent], disk_number: i64) -> DiskEvents {
    let mut r = DiskEvents::default();
    let needle = format!("\\device\\harddisk{disk_number}\\");
    for e in events {
        if !e.message.as_deref().unwrap_or("").to_lowercase().contains(&needle) {
            continue;
        }
        match e.id {
            7 => r.bad_block += 1,
            51 => r.paging += 1,
            153 => r.reset += 1,
            _ => continue,
        }
        if r.first.is_none_or(|f| e.time_created < f) {
            r.first = Some(e.time_created);
        }
        if r.last.is_none_or(|l| e.time_created > l) {
            r.last = Some(e.time_created);
        }
    }
    r
}

/// The 512-byte ATA SMART block -> attribute id -> the low 16 bits of its
/// raw value. Entries are 12 bytes from offset 2; the first of an id wins.
pub fn smart_attributes(bytes: &[u8]) -> BTreeMap<u8, i64> {
    let mut h = BTreeMap::new();
    let mut i = 2;
    while i + 12 <= bytes.len() {
        let id = bytes[i];
        if id != 0 {
            h.entry(id).or_insert(bytes[i + 5] as i64 + 256 * bytes[i + 6] as i64);
        }
        i += 12;
    }
    h
}

/// Defrag event 259 -> the last file Windows could not move.
pub fn defrag_259(message: &str) -> Option<String> {
    let file = capture(r"last unmovable file appears to be:\s*(\S.*?)\s*$", "m", message)?;
    let cut = re(r"::\$DATA$", "").find(file).map(|m| m.range().start).unwrap_or(file.len());
    Some(file[..cut].to_string())
}

/// The three files the converter turns off before it measures again, and
/// nothing else (RISKS R18, 2026-09-20).
pub fn shrink_mitigable(last_unmovable: &str) -> bool {
    matches(r"^\\?(hiberfil|pagefile|swapfile)\.sys$", last_unmovable)
}

/// NTFS asked for a full check (event 98) and no boot-time check has
/// completed since.
pub fn ntfs98_fresh(health: Option<&VolumeHealth>) -> bool {
    let Some(h) = health else { return false };
    match (h.ntfs_full_chkdsk, h.last_check) {
        (None, _) => false,
        (Some(_), None) => true,
        (Some(asked), Some(done)) => asked > done,
    }
}

/// Windows' own word that C: has an offline repair queued.
pub fn repair_queued(health: Option<&VolumeHealth>) -> bool {
    let Some(h) = health else { return false };
    matches("repair", h.volume_status.as_deref().unwrap_or("")) || ntfs98_fresh(health)
}

/// One named section of a PE file (`.sbat`, `.sbatlevel`) as text, or
/// nothing. A name longer than eight characters lives in the COFF string
/// table, which the section header points into (`/123`).
pub fn pe_section(bytes: &[u8], name: &str) -> Option<String> {
    let u16_at = |at: usize| bytes.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize);
    let u32_at = |at: usize| bytes.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize);
    let pe = u32_at(0x3c)?;
    if bytes.get(pe..pe + 4)? != b"PE\0\0" {
        return None;
    }
    let count = u16_at(pe + 6)?;
    let table = pe + 24 + u16_at(pe + 20)?;
    let strings = u32_at(pe + 12)? + 18 * u32_at(pe + 16)?;
    for i in 0..count {
        let s = table + 40 * i;
        let raw = bytes.get(s..s + 8)?;
        let mut n = String::from_utf8_lossy(raw).trim_end_matches('\0').to_string();
        if let Some(offset) = n.strip_prefix('/').and_then(|d| d.parse::<usize>().ok()) {
            let at = strings + offset;
            let end = bytes.get(at..)?.iter().position(|b| *b == 0)? + at;
            n = String::from_utf8_lossy(&bytes[at..end]).to_string();
        }
        if n == name {
            let (size, at) = (u32_at(s + 16)?, u32_at(s + 20)?);
            return Some(String::from_utf8_lossy(bytes.get(at..at + size)?).to_string());
        }
    }
    None
}

/// The common names (CN) of the X.509 certificates in a UEFI signature
/// database (`db`): the authorities whose signatures the firmware accepts.
pub fn db_authorities(bytes: &[u8]) -> Vec<String> {
    const X509: [u8; 16] = [0xa1, 0x59, 0xc0, 0xa5, 0xe4, 0x94, 0xa7, 0x4a, 0x87, 0xb5, 0xab, 0x15, 0x5c, 0x2b, 0xf0, 0x72];
    let u32_at = |at: usize| bytes.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize);
    let mut names = Vec::new();
    let mut pos = 0;
    while pos + 28 <= bytes.len() {
        let (Some(list_size), Some(header), Some(sig_size)) = (u32_at(pos + 16), u32_at(pos + 20), u32_at(pos + 24)) else { break };
        if list_size < 28 {
            break;
        }
        if bytes[pos..pos + 16] == X509 && sig_size > 16 {
            let mut at = pos + 28 + header;
            while at + sig_size <= pos + list_size && at + sig_size <= bytes.len() {
                if let Some(cn) = certificate_common_name(&bytes[at + 16..at + sig_size]) {
                    names.push(cn);
                }
                at += sig_size;
            }
        }
        pos += list_size;
    }
    names
}

/// DER: the length after a tag, and where the content starts.
fn der_len(b: &[u8], at: usize) -> Option<(usize, usize)> {
    let first = *b.get(at)? as usize;
    if first < 0x80 {
        return Some((first, at + 1));
    }
    let n = first & 0x7f;
    if n == 0 || n > 4 {
        return None;
    }
    let mut len = 0usize;
    for i in 0..n {
        len = (len << 8) | *b.get(at + 1 + i)? as usize;
    }
    Some((len, at + 1 + n))
}

/// The subject's common name of one DER certificate, as .NET's
/// `GetNameInfo(SimpleName)` gives it: the CN, or the whole subject's first
/// value when there is none.
fn certificate_common_name(der: &[u8]) -> Option<String> {
    // Certificate ::= SEQUENCE { tbsCertificate SEQUENCE { [0] version?, serial, signature, issuer, validity, subject, ... } }
    let (_, tbs_at) = der_len(der, 1)?;
    if der.first() != Some(&0x30) || der.get(tbs_at) != Some(&0x30) {
        return None;
    }
    let (_, mut at) = der_len(der, tbs_at + 1)?;
    let skip = |at: &mut usize| -> Option<()> {
        let (len, start) = der_len(der, *at + 1)?;
        *at = start + len;
        Some(())
    };
    if der.get(at) == Some(&0xa0) {
        skip(&mut at)?; // version
    }
    skip(&mut at)?; // serial
    skip(&mut at)?; // signature algorithm
    skip(&mut at)?; // issuer
    skip(&mut at)?; // validity
    // subject: SEQUENCE of SET of SEQUENCE { OID, value }
    if der.get(at) != Some(&0x30) {
        return None;
    }
    let (subject_len, mut rdn) = der_len(der, at + 1)?;
    let subject_end = rdn + subject_len;
    let mut first_value = None;
    while rdn < subject_end {
        if der.get(rdn) != Some(&0x31) {
            return first_value;
        }
        let (set_len, mut attr) = der_len(der, rdn + 1)?;
        let set_end = attr + set_len;
        while attr < set_end {
            let (seq_len, oid_tag) = der_len(der, attr + 1)?;
            let (oid_len, oid_at) = der_len(der, oid_tag + 1)?;
            let oid = &der[oid_at..oid_at + oid_len];
            let value_tag = oid_at + oid_len;
            let (value_len, value_at) = der_len(der, value_tag + 1)?;
            let raw = &der[value_at..value_at + value_len];
            let text = match der[value_tag] {
                0x1e => String::from_utf16_lossy(&raw.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect::<Vec<_>>()),
                _ => String::from_utf8_lossy(raw).to_string(),
            };
            if first_value.is_none() {
                first_value = Some(text.clone());
            }
            if oid == [0x55, 0x04, 0x03] {
                return Some(text);
            }
            attr = oid_tag + seq_len;
        }
        rdn = set_end;
    }
    first_value
}
