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
