//! `copy`: Copy-Kit.ps1, the host-side refresh of a stick that already
//! carries a FAT32 volume. Never partitions or formats: it clears the
//! runtime files a previous run left under `upgrade_\` (never the evidence
//! CSV), deletes any file on the stick whose bytes no longer match the
//! kit's SHA256SUMS (a copy interrupted by a bus drop leaves full-length
//! files with wrong bytes), copies what is missing, leaves alone what
//! already matches (a loosely seated stick drops off the bus under a
//! multi-gigabyte write; verified in place is the same evidence with
//! kilobytes written), then verifies every file against SHA256SUMS. The
//! file work here runs on any OS over two directories; the volume check
//! and the eject are Windows (`live`).
//!
//! Evidence before discarding (2026-10-27, the owner): the runtime files are
//! one conversion's records, and `esp-snapshot`, `rollback` and `job.json`
//! are what `settle-in rollback` reads to undo that conversion. Before any
//! of them is removed, each is copied to a folder on the host, read back
//! against its own bytes and listed in that folder's `SHA256SUMS`; if the
//! saving fails, nothing is removed. Copy-Kit.ps1 deleted them unsaved (the
//! Aspire's stick, 2026-10-27, held the only copy of its ESP snapshot).

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use upgrade_scan::ps::re;

/// What a previous run leaves and the next must not find.
pub const STALE: [&str; 16] = [
    "upgrade_\\job.json",
    "upgrade_\\ks.cfg",
    "upgrade_\\boot-verify",
    "upgrade_\\boot-install",
    "upgrade_\\converted",
    "upgrade_\\prologue.json",
    "upgrade_\\prologue-return.json",
    "upgrade_\\outcome.json",
    "upgrade_\\rollback.json",
    "upgrade_\\report",
    "upgrade_\\reports",
    "upgrade_\\artifacts",
    "upgrade_\\esp-snapshot",
    "upgrade_\\staging",
    "upgrade_\\rollback",
    "upgrade_\\boots.log",
];

/// Copy-Kit's reading of SHA256SUMS: `<hex>  ./path` lines only, the path
/// with backslashes.
pub fn sums(text: &str) -> Vec<(String, String)> {
    let rx = re(r"^([0-9a-f]{64})\s+\*?\./(.+)$", "");
    text.lines()
        .map(|l| l.trim_end_matches('\r'))
        .filter_map(|l| {
            let m = rx.find(l)?;
            Some((l[m.group(2)?].replace('/', "\\"), l[m.group(1)?].to_string()))
        })
        .collect()
}

pub fn sha256_file(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    Some(format!("{:x}", Sha256::digest(&bytes)))
}

/// A stick-relative path (backslashes) under a root on this OS.
pub fn under(root: &Path, rel: &str) -> PathBuf {
    let mut p = root.to_path_buf();
    for part in rel.split('\\').filter(|x| !x.is_empty()) {
        p.push(part);
    }
    p
}

#[derive(Debug, Default)]
pub struct Summary {
    pub saved: usize,
    pub removed_stale: Vec<String>,
    pub deleted_mismatching: Vec<String>,
    pub already_correct: usize,
    pub copied: usize,
    pub verified: usize,
    pub missing: Vec<String>,
    pub mismatched: Vec<String>,
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        entries.sort();
        for p in entries {
            if p.is_dir() {
                walk(&p, out);
            } else {
                out.push(p);
            }
        }
    }
}

/// Every runtime file a previous run left, copied under `save_to` with the
/// same relative paths, each read back, and listed in `save_to/SHA256SUMS`.
/// Returns how many files were saved (0 writes nothing, not even the folder).
pub fn save_stale(root: &Path, save_to: &Path) -> Result<usize, String> {
    let mut files = Vec::new();
    for rel in STALE {
        let p = under(root, rel);
        if p.is_dir() {
            walk(&p, &mut files);
        } else if p.is_file() {
            files.push(p);
        }
    }
    if files.is_empty() {
        return Ok(0);
    }
    if save_to.exists() {
        return Err(format!("{} already exists; refusing to mix two sticks' records", save_to.display()));
    }
    let mut lines = String::new();
    for f in &files {
        let rel = f.strip_prefix(root).map_err(|e| e.to_string())?;
        let dest = save_to.join(rel);
        if let Some(d) = dest.parent() {
            std::fs::create_dir_all(d).map_err(|e| format!("saving: {}: {e}", d.display()))?;
        }
        std::fs::copy(f, &dest).map_err(|e| format!("saving: {} -> {}: {e}", f.display(), dest.display()))?;
        let (want, got) = (sha256_file(f), sha256_file(&dest));
        if want.is_none() || want != got {
            return Err(format!("saving: {} did not read back the same; nothing removed", dest.display()));
        }
        lines.push_str(&format!("{}  ./{}\n", want.unwrap_or_default(), rel.to_string_lossy().replace('\\', "/")));
    }
    std::fs::write(save_to.join("SHA256SUMS"), lines).map_err(|e| format!("saving: SHA256SUMS: {e}"))?;
    Ok(files.len())
}

/// The refresh: the previous run's records saved to `save_to`, then stale
/// files out, mismatches out, the kit in, every file verified. `log` gets
/// the lines the script printed.
pub fn refresh(kit: &Path, root: &Path, save_to: &Path, log: &mut dyn FnMut(String)) -> Result<Summary, String> {
    let sums_path = kit.join("SHA256SUMS");
    if !sums_path.exists() {
        return Err(format!("no SHA256SUMS in {} - run ./make-kit.sh first", kit.display()));
    }
    let mut out = Summary::default();
    out.saved = save_stale(root, save_to)?;
    if out.saved > 0 {
        log(format!("  saved {} file(s) of the previous run to {} (read back, SHA256SUMS there)", out.saved, save_to.display()));
    }
    for rel in STALE {
        let p = under(root, rel);
        if p.exists() {
            let r = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
            r.map_err(|e| format!("{}: {e}", p.display()))?;
            log(format!("  removed stale {rel}"));
            out.removed_stale.push(rel.to_string());
        }
    }
    let list = sums(&std::fs::read_to_string(&sums_path).map_err(|e| e.to_string())?);
    log("  pre-checking files already on the stick (mismatches are deleted and re-copied)...".into());
    let mut same: Vec<String> = Vec::new();
    for (rel, want) in &list {
        let p = under(root, rel);
        if !p.exists() {
            continue;
        }
        if sha256_file(&p).as_deref() != Some(want.as_str()) {
            std::fs::remove_file(&p).map_err(|e| format!("{}: {e}", p.display()))?;
            out.deleted_mismatching.push(rel.clone());
            log(format!("  deleted mismatching {rel}"));
        } else {
            same.push(rel.to_lowercase());
        }
    }
    out.already_correct = same.len();
    log(format!("  {} file(s) removed for re-copy; {} already correct (not rewritten)", out.deleted_mismatching.len(), same.len()));
    log(format!("  copying {} -> {} ...", kit.display(), root.display()));
    let mut files = Vec::new();
    walk(kit, &mut files);
    for f in files {
        let rel = f.strip_prefix(kit).map_err(|e| e.to_string())?.to_string_lossy().replace('/', "\\");
        if same.contains(&rel.to_lowercase()) {
            continue;
        }
        let dest = under(root, &rel);
        if let Some(d) = dest.parent() {
            std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
        }
        std::fs::copy(&f, &dest).map_err(|e| format!("{} -> {}: {e} (a stick dropping off the bus looks like this - re-seat it and re-run)", f.display(), dest.display()))?;
        out.copied += 1;
    }
    log("  verifying every file on the stick against SHA256SUMS...".into());
    for (rel, want) in &list {
        let p = under(root, rel);
        out.verified += 1;
        if !p.exists() {
            log(format!("  MISSING {rel}"));
            out.missing.push(rel.clone());
            continue;
        }
        if same.contains(&rel.to_lowercase()) {
            continue;
        }
        if sha256_file(&p).as_deref() != Some(want.as_str()) {
            log(format!("  MISMATCH {rel}"));
            out.mismatched.push(rel.clone());
        }
    }
    let bad = out.missing.len() + out.mismatched.len();
    if bad > 0 {
        return Err(format!("{bad} of {} files failed verification on the stick", out.verified));
    }
    log(format!("  {} files verified on {}", out.verified, root.display()));
    if let Ok(t) = std::fs::read_to_string(under(root, "KIT-MANIFEST.txt")) {
        for l in t.lines().take(3) {
            log(format!("  {l}"));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kit(dir: &Path) -> Vec<(String, String)> {
        std::fs::create_dir_all(dir.join("EFI/BOOT")).unwrap();
        std::fs::write(dir.join("EFI/BOOT/BOOTX64.EFI"), b"boot").unwrap();
        std::fs::write(dir.join("README-STICK.txt"), b"readme").unwrap();
        std::fs::write(dir.join("KIT-MANIFEST.txt"), "line 1\nline 2\nline 3\nline 4\n").unwrap();
        let entries: Vec<(String, String)> = [("EFI/BOOT/BOOTX64.EFI", "boot"), ("README-STICK.txt", "readme"), ("KIT-MANIFEST.txt", "line 1\nline 2\nline 3\nline 4\n")].iter().map(|(p, c)| (p.to_string(), format!("{:x}", Sha256::digest(c.as_bytes())))).collect();
        let text: String = entries.iter().map(|(p, h)| format!("{h}  ./{p}\n")).collect();
        std::fs::write(dir.join("SHA256SUMS"), text).unwrap();
        entries
    }

    #[test]
    fn a_refresh_removes_stale_files_replaces_wrong_bytes_and_keeps_right_ones() {
        let tmp = std::env::temp_dir().join(format!("upgrade-stick-copy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let (k, r) = (tmp.join("kit"), tmp.join("stick"));
        kit(&k);
        std::fs::create_dir_all(r.join("upgrade_/report")).unwrap();
        std::fs::write(r.join("upgrade_/job.json"), "{}").unwrap();
        std::fs::write(r.join("upgrade_/report/tools.jsonl"), "x").unwrap();
        std::fs::write(r.join("upgrade_/r16-stick-writer.csv"), "kept").unwrap();
        std::fs::write(r.join("README-STICK.txt"), b"readme").unwrap(); // right already
        std::fs::create_dir_all(r.join("EFI/BOOT")).unwrap();
        std::fs::write(r.join("EFI/BOOT/BOOTX64.EFI"), b"bootXXXX").unwrap(); // wrong bytes, same length class
        let mut lines = Vec::new();
        let s = refresh(&k, &r, &tmp.join("saved"), &mut |l| lines.push(l)).unwrap();
        assert_eq!(s.saved, 2, "job.json and report/tools.jsonl saved before removal");
        assert_eq!(std::fs::read_to_string(tmp.join("saved/upgrade_/job.json")).unwrap(), "{}");
        assert_eq!(s.removed_stale, vec!["upgrade_\\job.json", "upgrade_\\report"]);
        assert_eq!(s.deleted_mismatching, vec!["EFI\\BOOT\\BOOTX64.EFI"]);
        assert_eq!(s.already_correct, 1);
        assert_eq!(s.copied, 3, "the sums file, the manifest and the boot file; the readme stays");
        assert_eq!(s.verified, 3);
        assert!(r.join("upgrade_/r16-stick-writer.csv").exists(), "the evidence CSV is never removed");
        assert_eq!(std::fs::read(r.join("EFI/BOOT/BOOTX64.EFI")).unwrap(), b"boot");
        assert!(lines.iter().any(|l| l == "  removed stale upgrade_\\job.json"));
        assert!(lines.iter().any(|l| l == "  1 file(s) removed for re-copy; 1 already correct (not rewritten)"));
        assert!(lines.iter().any(|l| l.ends_with("line 3")) && !lines.iter().any(|l| l.ends_with("line 4")));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn the_rollback_input_is_saved_and_read_back_before_it_is_removed() {
        let tmp = std::env::temp_dir().join(format!("upgrade-stick-copy-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let (k, r) = (tmp.join("kit"), tmp.join("stick"));
        kit(&k);
        std::fs::create_dir_all(r.join("upgrade_/esp-snapshot/EFI/Microsoft/Boot")).unwrap();
        std::fs::write(r.join("upgrade_/esp-snapshot/EFI/Microsoft/Boot/bootmgfw.efi"), b"windows loader").unwrap();
        std::fs::create_dir_all(r.join("upgrade_/rollback")).unwrap();
        std::fs::write(r.join("upgrade_/rollback/bootx64.efi.before"), b"before").unwrap();
        std::fs::write(r.join("upgrade_/job.json"), r#"{"job_id":"j1"}"#).unwrap();
        let save = tmp.join("saved");
        let mut lines = Vec::new();
        let s = refresh(&k, &r, &save, &mut |l| lines.push(l)).unwrap();
        assert_eq!(s.saved, 3);
        assert!(!r.join("upgrade_/esp-snapshot").exists() && !r.join("upgrade_/rollback").exists(), "removed from the stick");
        assert_eq!(std::fs::read(save.join("upgrade_/esp-snapshot/EFI/Microsoft/Boot/bootmgfw.efi")).unwrap(), b"windows loader");
        let sums_text = std::fs::read_to_string(save.join("SHA256SUMS")).unwrap();
        assert_eq!(sums(&sums_text).len(), 3, "every saved file listed, in the kit's own format");
        assert!(lines.iter().any(|l| l.starts_with("  saved 3 file(s) of the previous run to ")));
        // a second refresh into the same folder refuses before removing anything
        std::fs::write(r.join("upgrade_/job.json"), "{}").unwrap();
        let e = refresh(&k, &r, &save, &mut |_| {}).unwrap_err();
        assert!(e.contains("already exists"), "{e}");
        assert!(r.join("upgrade_/job.json").exists(), "nothing removed when saving refused");
        // a stick with no records saves nothing and makes no folder
        let empty = tmp.join("saved-none");
        let s = refresh(&k, &tmp.join("kit2-stick"), &empty, &mut |_| {});
        assert!(s.is_ok() && !empty.exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn a_missing_sums_file_refuses_and_sums_lines_parse_like_the_script() {
        let tmp = std::env::temp_dir().join(format!("upgrade-stick-copy-none-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);
        let e = refresh(&tmp, &tmp, &tmp.join("saved"), &mut |_| {}).unwrap_err();
        assert!(e.starts_with("no SHA256SUMS in"));
        let _ = std::fs::remove_dir_all(&tmp);
        let h = "a".repeat(64);
        let got = sums(&format!("{h}  ./EFI/BOOT/x.efi\r\n{h} *./a/b\nnot a line\n{h}  EFI/no-dot\n"));
        assert_eq!(got, vec![("EFI\\BOOT\\x.efi".to_string(), h.clone()), ("a\\b".to_string(), h.clone())]);
    }
}
