//! Bring the files home: the plan (architecture.md, Module 3, step 2;
//! decided 2026-11-07, the owner: the keep-Windows path only).
//!
//! This half reads and never writes. It walks the folder map from
//! `job.json` on the kept Windows partition (mounted read-only through
//! ntfs-3g, RISKS R19), sorts every entry into what will be copied and what
//! will not, and refuses when the folders are not what `evaluate` counted.
//! The rules it follows, each from a rig finding or a decision:
//!
//! - `lstat` first, and only regular files are opened: Windows' empty
//!   system files show up as pipes under ntfs-3g, and opening one waits
//!   forever (log 2026-09-01, V3).
//! - A OneDrive placeholder is never copied (decided 2026-09-26, R8). It is
//!   recognised by the harvest's own rule, its attributes, read through
//!   ntfs-3g's `system.ntfs_attrib` (and the reparse tag, through
//!   `system.ntfs_reparse_data`). How a
//!   placeholder actually looks through ntfs-3g has never been seen (V8's
//!   residue), so a regular file with bytes but no space on disk is not
//!   copied either, and the count of placeholders must match the job's.
//! - Links and junctions are not followed, as the harvest did not follow
//!   them (log 2026-09-26): a link to another tree is not the person's folder.
//! - Every file not copied is listed with its reason. Nothing is left out
//!   silently (R20's worst kind of failure).
//!
//! The live half (unlock, mount, the person's home, free space) is in `live`.

use serde_json::{json, Value};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// The folders the harvest maps, and the XDG name each lands in.
pub const FOLDERS: [(&str, &str); 6] =
    [("Desktop", "DESKTOP"), ("Documents", "DOCUMENTS"), ("Pictures", "PICTURES"), ("Music", "MUSIC"), ("Videos", "VIDEOS"), ("Downloads", "DOWNLOAD")];

/// Room left on the home drive after the copy.
pub const MARGIN: u64 = 2 << 30;

// NTFS file attributes (Microsoft's FILE_ATTRIBUTE_* values)
const ATTR_DIRECTORY: u32 = 0x10;
const ATTR_OFFLINE: u32 = 0x1000;
const ATTR_RECALL_ON_DATA_ACCESS: u32 = 0x400000;
// the two folder links the harvest does not walk (evaluate/harvest/src/folders.rs)
const TAG_MOUNT_POINT: u32 = 0xA000_0003;
const TAG_SYMLINK: u32 = 0xA000_000C;

/// A reparse tag of the cloud-files family (IO_REPARSE_TAG_CLOUD to _CLOUD_F,
/// 0x9000001A with any value in bits 12 to 15).
pub fn is_cloud_tag(tag: u32) -> bool {
    tag & 0xFFFF_0FFF == 0x9000_001A
}

/// What ntfs-3g says about an entry beyond `lstat`.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Ntfs {
    pub reparse_tag: Option<u32>,
    pub attrib: Option<u32>,
}

pub trait Probe {
    fn ntfs(&self, p: &Path) -> Ntfs;
}

/// How one entry is sorted.
#[derive(Debug, PartialEq)]
pub enum Sort {
    Copy(u64),
    Placeholder,
    /// Not copied: why, and whether the harvest counted it as a file.
    Skip(&'static str, bool),
    /// A file that may be only in OneDrive and cannot be told: the plan refuses.
    Unsure(&'static str),
    Walk,
}

/// The rule for one entry, from what `lstat` and ntfs-3g say. It counts
/// and recognises exactly as the harvest did on Windows, so the counts can
/// be held against each other: every entry that is not a folder is a file;
/// every folder is walked except a junction or a folder symlink; a
/// placeholder is a file with the recall-on-data-access or offline bit
/// (`evaluate/harvest/src/cloud.rs`). A OneDrive file already downloaded
/// keeps its cloud reparse tag but loses those bits (log 2026-09-08,
/// `0x80420`), and is copied like any other.
pub fn sort(name: &str, md: &std::fs::Metadata, n: Ntfs) -> Sort {
    let ft = md.file_type();
    let folder_link = n.reparse_tag.is_some_and(|t| t == TAG_MOUNT_POINT || t == TAG_SYMLINK);
    let cloud_tag = n.reparse_tag.is_some_and(is_cloud_tag);
    // a cloud file ntfs-3g shows as something other than a file or a folder is not guessed at (review, 2026-11-07)
    if cloud_tag && !ft.is_file() && !ft.is_dir() {
        return Sort::Unsure("a OneDrive file this Linux cannot read the kind of");
    }
    if ft.is_symlink() {
        // ntfs-3g shows junctions and symlinks as links; the folder bit says which kind the harvest saw
        let is_folder = n.attrib.is_some_and(|a| a & ATTR_DIRECTORY != 0);
        return Sort::Skip(if is_folder { "a link to another folder (not followed)" } else { "a link to somewhere else (not followed)" }, !is_folder);
    }
    if ft.is_dir() {
        return if folder_link { Sort::Skip("a link to another folder (not followed)", false) } else { Sort::Walk };
    }
    if !ft.is_file() {
        return Sort::Skip("a Windows system file, not a document", true);
    }
    match n.attrib {
        Some(a) if a & (ATTR_OFFLINE | ATTR_RECALL_ON_DATA_ACCESS) != 0 => return Sort::Placeholder,
        // a cloud file whose attributes cannot be read cannot be told apart from a placeholder
        None if cloud_tag => return Sort::Unsure("it could not be told whether it is only in OneDrive"),
        _ => {}
    }
    if n.reparse_tag.is_some_and(|t| !is_cloud_tag(t)) {
        return Sort::Skip("a special Windows file (a reparse point)", true);
    }
    // a OneDrive file partly downloaded has less on the disk than its length: the rest would read as zeros
    if cloud_tag && md.len() > 1024 && md.blocks().saturating_mul(512) < md.len() {
        return Sort::Skip("only part of it is on this computer; the rest is in OneDrive", true);
    }
    // a file with bytes and no space on disk is not trusted to read back its bytes
    if md.len() > 1024 && md.blocks() == 0 {
        return Sort::Skip("it has no data on the disk (perhaps only in the cloud)", true);
    }
    if name.eq_ignore_ascii_case("desktop.ini") || name.eq_ignore_ascii_case("thumbs.db") {
        return Sort::Skip("Windows' own folder settings", true);
    }
    Sort::Copy(md.len())
}

#[derive(Debug, Default)]
pub struct Walked {
    /// Relative paths and sizes to copy.
    pub copy: Vec<(String, u64)>,
    pub placeholders: u64,
    /// Relative path, why, whether the harvest counted it as a file.
    pub skipped: Vec<(String, &'static str, bool)>,
    /// Folders that could not be read: a refusal.
    pub unreadable: Vec<String>,
    /// Files that may be only in OneDrive and cannot be told: a refusal.
    pub unsure: Vec<(String, &'static str)>,
}

impl Walked {
    pub fn files(&self) -> u64 {
        // what the harvest counted: every file, whatever its kind (not the folders or links)
        self.copy.len() as u64 + self.placeholders + self.unsure.len() as u64 + self.skipped.iter().filter(|s| s.2).count() as u64
    }
    pub fn bytes(&self) -> u64 {
        self.copy.iter().map(|(_, b)| b).sum()
    }
}

/// Walk one folder, depth first, in name order, never following a link.
pub fn walk(top: &Path, probe: &dyn Probe) -> Walked {
    let mut w = Walked::default();
    let mut stack = vec![PathBuf::new()];
    while let Some(rel) = stack.pop() {
        let dir = top.join(&rel);
        let mut names = match std::fs::read_dir(&dir) {
            Ok(rd) => match rd.map(|e| e.map(|e| e.file_name())).collect::<Result<Vec<_>, _>>() {
                Ok(n) => n,
                Err(e) => {
                    w.unreadable.push(format!("{} ({})", shown(&rel), e));
                    continue;
                }
            },
            Err(e) => {
                w.unreadable.push(format!("{} ({})", shown(&rel), e));
                continue;
            }
        };
        names.sort();
        let mut subdirs = Vec::new();
        for name in names {
            let r = rel.join(&name);
            let p = top.join(&r);
            let md = match std::fs::symlink_metadata(&p) {
                Ok(m) => m,
                Err(e) => {
                    w.unreadable.push(format!("{} ({})", shown(&r), e));
                    continue;
                }
            };
            match sort(&name.to_string_lossy(), &md, probe.ntfs(&p)) {
                Sort::Copy(n) => w.copy.push((shown(&r), n)),
                Sort::Placeholder => w.placeholders += 1,
                Sort::Skip(why, counted) => w.skipped.push((shown(&r), why, counted)),
                Sort::Unsure(why) => w.unsure.push((shown(&r), why)),
                Sort::Walk => subdirs.push(r),
            }
        }
        // pushed in reverse so the walk goes in name order
        stack.extend(subdirs.into_iter().rev());
    }
    w
}

fn shown(rel: &Path) -> String {
    let s = rel.to_string_lossy().to_string();
    if s.is_empty() { ".".into() } else { s }
}

/// `C:\Users\Ann\Documents` -> `Users/Ann/Documents`, only on C: and never
/// with `..` in it.
pub fn on_volume(win: &str) -> Option<String> {
    let rest = win.strip_prefix("C:\\").or_else(|| win.strip_prefix("c:\\"))?;
    let parts: Vec<&str> = rest.split('\\').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() || parts.iter().any(|p| *p == "." || *p == ".." || p.contains('/')) {
        return None;
    }
    Some(parts.join("/"))
}

/// Where the folders land: the person's own XDG folders
/// (`~/.config/user-dirs.dirs`, named in their language), else `~/<Name>`.
pub fn destinations(home: &Path) -> Vec<(String, PathBuf)> {
    let t = std::fs::read_to_string(home.join(".config/user-dirs.dirs")).unwrap_or_default();
    FOLDERS
        .iter()
        .map(|(name, xdg)| {
            let key = format!("XDG_{}_DIR=", xdg);
            let from_file = t.lines().map(str::trim).find_map(|l| l.strip_prefix(&key)).map(|v| v.trim_matches('"').to_string())
                .and_then(|v| v.strip_prefix("$HOME/").map(str::to_string))
                // only a folder inside the home, named plainly (no `..`, review 2026-11-07)
                .filter(|r| Path::new(r).components().all(|c| matches!(c, std::path::Component::Normal(_))))
                .map(|r| home.join(r))
                // a folder set to the home itself (XDG's "turned off") is not a destination
                .filter(|p| p != home);
            (name.to_string(), from_file.unwrap_or_else(|| home.join(name)))
        })
        .collect()
}

pub struct Facts<'a> {
    pub job: &'a Value,
    pub outcome: &'a Value,
    /// Where the Windows partition is mounted read-only, or why it is not.
    pub windows: Result<PathBuf, String>,
    pub home: &'a Path,
    /// Free bytes on the drive that holds the home.
    pub free: Option<u64>,
    pub rolled_back: bool,
    pub reclaimed: bool,
    /// Bytes an earlier, cut-off run already put home (its journal): not needed again.
    pub already_home: u64,
}

fn s<'a>(v: &'a Value, p: &str) -> &'a str {
    v.pointer(p).and_then(Value::as_str).unwrap_or("")
}

/// The plan: what is copied where, what is not and why, or every reason to
/// refuse. Writes nothing.
pub fn plan(f: &Facts, probe: &dyn Probe) -> Value {
    let mut no: Vec<String> = Vec::new();
    if f.job.get("schema") != Some(&json!("job/1")) {
        return json!({ "result": "refused", "why": ["the job is not readable, so where your files are is not known"] });
    }
    if s(f.job, "/intent/path") != "keep-windows" || s(f.outcome, "/path_taken") != "keep-windows" || f.outcome.pointer("/windows/kept") != Some(&json!(true)) {
        return json!({ "result": "nothing-to-do", "why": ["Windows was not kept on this computer, so there are no files on it to bring over"] });
    }
    if f.reclaimed {
        return json!({ "result": "nothing-to-do", "why": ["Windows was deleted from this computer (reclaim), so its files are gone"] });
    }
    if f.rolled_back {
        no.push("Windows was made to start first again (the rollback): finish going back, or undo that, before copying".into());
    }
    let expect_placeholders = match s(f.job, "/harvest/cloud_files/result") {
        "left-in-cloud" => true,
        "none-found" | "materialized" => false,
        other => {
            no.push(format!("the job does not say how OneDrive's files were left ({:?})", other));
            false
        }
    };
    let folders: Vec<&Value> = f.job.pointer("/harvest/folders").and_then(Value::as_array).into_iter().flatten().filter(|d| d.get("exists") == Some(&json!(true))).collect();
    if folders.is_empty() {
        return json!({ "result": "nothing-to-do", "why": ["Windows had none of the usual folders (Desktop, Documents, Pictures, Music, Videos, Downloads) with anything in them"] });
    }
    // where each one is on the volume, and none inside another (it would be copied twice)
    let mut mapped: Vec<(&Value, String)> = Vec::new();
    for d in &folders {
        match on_volume(s(d, "/path")) {
            Some(v) => mapped.push((d, v)),
            None => no.push(format!("{} is at {:?}, not on Windows' own drive C:, and only that drive is read", s(d, "/name"), s(d, "/path"))),
        }
    }
    for (a, va) in &mapped {
        for (b, vb) in &mapped {
            if !std::ptr::eq(*a, *b) && vb.eq_ignore_ascii_case(va) && s(a, "/name") < s(b, "/name") {
                no.push(format!("{} and {} are the same folder, so its files would be copied twice", s(a, "/name"), s(b, "/name")));
            }
            if !std::ptr::eq(*a, *b) && vb.to_lowercase().starts_with(&format!("{}/", va.to_lowercase())) {
                no.push(format!("{} is inside {}, so its files would be copied twice", s(b, "/name"), s(a, "/name")));
            }
        }
    }
    let win = match &f.windows {
        Ok(w) => w.clone(),
        Err(e) => {
            no.push(format!("Windows' drive could not be read: {}", e));
            return json!({ "result": "refused", "why": no });
        }
    };
    // a Windows that hibernated (or shut down with Fast Startup) may hold newer files in its
    // memory image than on the drive: read now, they could come over stale (review, 2026-11-07)
    if let Ok(mut h) = std::fs::File::open(win.join("hiberfil.sys")) {
        let mut b = [0u8; 4];
        if std::io::Read::read_exact(&mut h, &mut b).is_ok() && (b.eq_ignore_ascii_case(b"hibr") || b.eq_ignore_ascii_case(b"wake")) {
            no.push("Windows was not fully shut down (it hibernated, or Fast Startup kept it half-asleep), so its newest changes may not be on the drive. Start Windows, then choose Restart (not Shut down), let it reach the sign-in screen, and start Linux from there".into());
        }
    }
    let dest = destinations(f.home);
    let mut out = Vec::new();
    let mut total = 0u64;
    for (d, v) in &mapped {
        let name = s(d, "/name");
        let to = dest.iter().find(|(n, _)| n == name).map(|(_, p)| p.clone()).unwrap_or_else(|| f.home.join(name));
        let w = walk(&win.join(v), probe);
        let job_files = d.get("files").and_then(Value::as_u64).unwrap_or(0);
        let job_ph = if expect_placeholders { d.get("cloud_only_files").and_then(Value::as_u64).unwrap_or(0) } else { 0 };
        for u in &w.unreadable {
            no.push(format!("in {}, {} could not be read", name, u));
        }
        if !w.unsure.is_empty() {
            no.push(format!("in {}, {} files cannot be told apart from files that live only in OneDrive (the first: {}), so nothing is copied", name, w.unsure.len(), w.unsure[0].0));
        }
        // the harvest's count is the check that nothing hides from this walk (V8's residue)
        if w.files() != job_files {
            no.push(format!("{} holds {} files now, but {} were counted before the conversion: it is not the folder that was checked, so nothing is copied", name, w.files(), job_files));
        }
        if w.placeholders != job_ph {
            no.push(format!("{} has {} files that live only in OneDrive, but {} were counted before the conversion: they could not all be recognised, so nothing is copied", name, w.placeholders, job_ph));
        }
        total += w.bytes();
        out.push(json!({
            "name": name,
            "from": s(d, "/path"),
            "to": to.to_string_lossy(),
            "files": w.copy.len(),
            "bytes": w.bytes(),
            "in_onedrive": w.placeholders,
            "not_copied": w.skipped.iter().map(|(p, why, _)| json!({ "path": p, "why": why })).collect::<Vec<_>>(),
            "counted_before": { "files": job_files, "in_onedrive": job_ph },
        }));
    }
    match f.free {
        Some(free) if free < total.saturating_sub(f.already_home).saturating_add(MARGIN) => {
            let need = total.saturating_sub(f.already_home);
            no.push(format!("this Linux has {} free, and the files need {} plus {} to spare: {} more space is needed",
                gb(free), gb(need), gb(MARGIN), gb(need + MARGIN - free)))
        }
        None => no.push("the free space on this Linux could not be read".into()),
        _ => {}
    }
    // the browsers' bookmarks, read only (R20, decided 2026-11-07): never a reason to refuse
    let bookmarks = crate::bookmarks::user_dir(f.job).map(|u| crate::bookmarks::summary(&crate::bookmarks::find(&win, &u))).unwrap_or(json!([]));
    if !no.is_empty() {
        return json!({ "result": "refused", "why": no, "folders": out, "bookmarks": bookmarks });
    }
    json!({ "result": "plan", "folders": out, "bytes": total, "free": f.free, "bookmarks": bookmarks })
}

pub fn gb(b: u64) -> String {
    format!("{:.1} GB", b as f64 / 1e9)
}

pub mod live {
    use super::*;
    use std::ffi::CString;

    /// ntfs-3g's extended attributes (its `system.ntfs_*` names).
    pub struct Ntfs3g;
    impl Probe for Ntfs3g {
        fn ntfs(&self, p: &Path) -> Ntfs {
            let le = |b: &[u8]| (b.len() >= 4).then(|| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
            Ntfs { reparse_tag: xattr(p, "system.ntfs_reparse_data").and_then(|b| le(&b)), attrib: xattr(p, "system.ntfs_attrib").and_then(|b| le(&b)) }
        }
    }

    fn xattr(p: &Path, name: &str) -> Option<Vec<u8>> {
        let cp = CString::new(p.as_os_str().as_encoded_bytes()).ok()?;
        let cn = CString::new(name).ok()?;
        let mut buf = vec![0u8; 16 * 1024];
        // SAFETY: both strings are NUL-terminated and buf is valid for its length.
        let n = unsafe { libc::lgetxattr(cp.as_ptr(), cn.as_ptr(), buf.as_mut_ptr().cast(), buf.len()) };
        if n < 0 {
            return None;
        }
        buf.truncate(n as usize);
        Some(buf)
    }

    pub fn free_bytes(p: &Path) -> Option<u64> {
        let cp = CString::new(p.as_os_str().as_encoded_bytes()).ok()?;
        // SAFETY: statvfs fills the struct it is given.
        let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
        (unsafe { libc::statvfs(cp.as_ptr(), &mut st) } == 0).then(|| st.f_bavail as u64 * st.f_frsize as u64)
    }

    /// `settle-in pull plan --windows-mount DIR --home DIR [--root DIR]`:
    /// the caller mounted Windows read-only; nothing is written.
    pub fn plan_at(root: &str, windows_mount: &str, home: &str) -> Value {
        let r = root.trim_end_matches('/');
        let handoff = format!("{}/{}", r, crate::HANDOFF);
        let job = crate::read_json(&format!("{}/job.json", handoff)).unwrap_or(Value::Null);
        let outcome = crate::read_json(&format!("{}/outcome.json", handoff)).unwrap_or(Value::Null);
        let m = format!("{}{}", r, windows_mount.trim_end_matches('/'));
        let windows = match crate::reclaim::live::mount_of(r, windows_mount.trim_end_matches('/')) {
            None if r.is_empty() => Err(format!("{} is not a mount point", windows_mount)),
            Some((_, opts, _)) if !opts.split(',').any(|o| o == "ro") => Err(format!("{} is mounted read-write; Windows is only ever read here read-only", windows_mount)),
            _ => Ok(PathBuf::from(&m)),
        };
        let home = PathBuf::from(format!("{}{}", r, home));
        let f = Facts {
            job: &job,
            outcome: &outcome,
            windows,
            home: &home,
            free: free_bytes(&home),
            rolled_back: Path::new(&format!("{}/boot/efi/upgrade_/rolled-back.json", r)).exists(),
            reclaimed: Path::new(&format!("{}/boot/efi/{}", r, crate::reclaimrun::ESP_DONE_REL)).exists(),
            already_home: 0,
        };
        plan(&f, &Ntfs3g)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Made-up ntfs-3g answers, by path relative to the tree.
    struct Fake(PathBuf, HashMap<&'static str, Ntfs>);
    impl Probe for Fake {
        fn ntfs(&self, p: &Path) -> Ntfs {
            let rel = p.strip_prefix(&self.0).unwrap().to_string_lossy().to_string();
            self.1.get(rel.as_str()).copied().unwrap_or_default()
        }
    }

    fn tree(tag: &str) -> PathBuf {
        let t = std::env::temp_dir().join(format!("settle-in-pull-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        let docs = t.join("win/Users/Ann/Documents");
        std::fs::create_dir_all(docs.join("Taxes/2025")).unwrap();
        std::fs::create_dir_all(t.join("win/Users/Ann/Pictures")).unwrap();
        std::fs::create_dir_all(t.join("home/.config")).unwrap();
        std::fs::write(docs.join("letter.txt"), "dear").unwrap();
        std::fs::write(docs.join("desktop.ini"), "[.ShellClassInfo]").unwrap();
        std::fs::write(docs.join("Taxes/2025/return.pdf"), vec![7u8; 5000]).unwrap();
        std::fs::write(docs.join("cloud.docx"), "").unwrap();
        std::os::unix::fs::symlink("/etc", docs.join("My Music")).unwrap();
        std::fs::write(t.join("win/Users/Ann/Pictures/cat.jpg"), vec![1u8; 3000]).unwrap();
        // a pipe, as ntfs-3g shows Windows' empty system files
        let fifo = std::ffi::CString::new(docs.join("pipe").to_string_lossy().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o644) }, 0);
        std::fs::write(t.join("home/.config/user-dirs.dirs"), "XDG_DOCUMENTS_DIR=\"$HOME/Dokumente\"\nXDG_PICTURES_DIR=\"$HOME/Bilder\"\n").unwrap();
        t
    }

    fn job(docs_files: u64, cloud: u64) -> Value {
        json!({ "schema": "job/1", "intent": { "path": "keep-windows" }, "harvest": {
            "cloud_files": { "result": if cloud > 0 { "left-in-cloud" } else { "none-found" } },
            "folders": [
                { "name": "Documents", "path": "C:\\Users\\Ann\\Documents", "exists": true, "files": docs_files, "cloud_only_files": cloud },
                { "name": "Pictures", "path": "C:\\Users\\Ann\\Pictures", "exists": true, "files": 1, "cloud_only_files": 0 },
                { "name": "Music", "path": "C:\\Users\\Ann\\Music", "exists": false, "files": 0, "cloud_only_files": 0 } ] } })
    }

    fn outcome() -> Value {
        json!({ "path_taken": "keep-windows", "windows": { "kept": true } })
    }

    fn run(t: &Path, job: &Value, fake: HashMap<&'static str, Ntfs>, free: u64) -> Value {
        let o = outcome();
        let home = t.join("home");
        let f = Facts { job, outcome: &o, windows: Ok(t.join("win")), home: &home, free: Some(free), rolled_back: false, reclaimed: false, already_home: 0 };
        plan(&f, &Fake(t.join("win"), fake))
    }

    fn cloud_doc() -> HashMap<&'static str, Ntfs> {
        HashMap::from([("Users/Ann/Documents/cloud.docx", Ntfs { reparse_tag: Some(0x9000_601A), attrib: Some(0x0040_0020) })])
    }

    #[test]
    fn plans_and_sorts_every_entry() {
        let t = tree("plan");
        // letter, desktop.ini, return.pdf, cloud.docx, the pipe, and the link (a file link to the
        // harvest: ntfs-3g gives it no folder bit here): six files
        let p = run(&t, &job(6, 1), cloud_doc(), 100 << 30);
        assert_eq!(p["result"], "plan", "{p:#}");
        let d = &p["folders"][0];
        assert_eq!(d["to"], t.join("home/Dokumente").to_string_lossy().to_string());
        assert_eq!(d["files"], 2);
        assert_eq!(d["bytes"], 5004);
        assert_eq!(d["in_onedrive"], 1);
        let why: Vec<(&str, &str)> = d["not_copied"].as_array().unwrap().iter().map(|n| (n["path"].as_str().unwrap(), n["why"].as_str().unwrap())).collect();
        assert_eq!(why, [("My Music", "a link to somewhere else (not followed)"), ("desktop.ini", "Windows' own folder settings"), ("pipe", "a Windows system file, not a document")]);
        assert_eq!(p["folders"][1]["to"], t.join("home/Bilder").to_string_lossy().to_string());
        assert_eq!(p["folders"].as_array().unwrap().len(), 2, "a folder that did not exist is not walked");
        assert_eq!(p["bytes"], 8004);
    }

    #[test]
    fn a_placeholder_not_recognised_refuses() {
        let t = tree("blind");
        // the job counted one OneDrive-only file; ntfs-3g shows nothing special about it
        let p = run(&t, &job(6, 1), HashMap::new(), 100 << 30);
        assert_eq!(p["result"], "refused");
        assert!(p["why"].to_string().contains("Documents has 0 files that live only in OneDrive, but 1 were counted"), "{p:#}");
    }

    #[test]
    fn a_folder_not_as_counted_refuses() {
        let t = tree("count");
        let p = run(&t, &job(7, 1), cloud_doc(), 100 << 30);
        assert!(p["why"].to_string().contains("Documents holds 6 files now, but 7 were counted"), "{p:#}");
        // a folder link is not a file to the harvest
        let fake = HashMap::from([("Users/Ann/Documents/cloud.docx", Ntfs { reparse_tag: Some(0x9000_601A), attrib: Some(0x0040_0020) }), ("Users/Ann/Documents/My Music", Ntfs { reparse_tag: Some(TAG_MOUNT_POINT), attrib: Some(0x410) })]);
        assert_eq!(run(&t, &job(5, 1), fake, 100 << 30)["result"], "plan");
    }

    #[test]
    fn too_little_room_names_the_gap() {
        let t = tree("room");
        let p = run(&t, &job(6, 1), cloud_doc(), 1 << 30);
        assert_eq!(p["result"], "refused");
        assert!(p["why"].to_string().contains("more space is needed"), "{p:#}");
    }

    #[test]
    fn the_sorting_rules() {
        let t = tree("sort");
        let f = t.join("win/Users/Ann/Documents/letter.txt");
        let md = std::fs::symlink_metadata(&f).unwrap();
        assert_eq!(sort("letter.txt", &md, Ntfs::default()), Sort::Copy(4));
        // the harvest's rule: the recall-on-data-access or offline bit, with or without a tag
        for tag in [Some(0x9000_001Au32), Some(0x9000_101A), Some(0x9000_F01A), None] {
            assert_eq!(sort("x", &md, Ntfs { reparse_tag: tag, attrib: Some(ATTR_RECALL_ON_DATA_ACCESS | 0x20) }), Sort::Placeholder);
            assert_eq!(sort("x", &md, Ntfs { reparse_tag: tag, attrib: Some(ATTR_OFFLINE) }), Sort::Placeholder);
        }
        // downloaded: the cloud tag stays, the recall bit goes (the rig's 0x80420); it is copied
        assert_eq!(sort("x", &md, Ntfs { reparse_tag: Some(0x9000_601A), attrib: Some(0x80420) }), Sort::Copy(4));
        // a cloud tag with unreadable attributes cannot be told apart, so it is not copied
        assert_eq!(sort("x", &md, Ntfs { reparse_tag: Some(0x9000_601A), attrib: None }), Sort::Unsure("it could not be told whether it is only in OneDrive"));
        // an app alias (IO_REPARSE_TAG_APPEXECLINK) is not a document
        assert_eq!(sort("x", &md, Ntfs { reparse_tag: Some(0x8000_001B), attrib: Some(0x420) }), Sort::Skip("a special Windows file (a reparse point)", true));
        // a junction is not walked and not a file; any other reparse folder (OneDrive's) is walked
        let dmd = std::fs::symlink_metadata(t.join("win/Users/Ann/Documents/Taxes")).unwrap();
        assert_eq!(sort("Taxes", &dmd, Ntfs { reparse_tag: Some(TAG_MOUNT_POINT), attrib: None }), Sort::Skip("a link to another folder (not followed)", false));
        assert_eq!(sort("Taxes", &dmd, Ntfs { reparse_tag: Some(0x9000_701A), attrib: None }), Sort::Walk);
        // bytes with no space on the disk
        let sparse = t.join("sparse");
        std::fs::File::create(&sparse).unwrap().set_len(1 << 20).unwrap();
        assert_eq!(sort("sparse", &std::fs::symlink_metadata(&sparse).unwrap(), Ntfs::default()), Sort::Skip("it has no data on the disk (perhaps only in the cloud)", true));
    }

    #[test]
    fn the_review_rules() {
        let t = tree("review");
        let docs = t.join("win/Users/Ann/Documents");
        // a cloud file with only part of it on the disk (a sparse file stands in)
        let part = docs.join("half.docx");
        std::fs::File::create(&part).unwrap().set_len(1 << 20).unwrap();
        let md = std::fs::symlink_metadata(&part).unwrap();
        assert_eq!(sort("half.docx", &md, Ntfs { reparse_tag: Some(0x9000_601A), attrib: Some(0x80420) }), Sort::Skip("only part of it is on this computer; the rest is in OneDrive", true));
        std::fs::remove_file(&part).unwrap();
        // a cloud tag on something shown as a link: unsure, and the plan refuses
        let mut fake = cloud_doc();
        fake.insert("Users/Ann/Documents/My Music", Ntfs { reparse_tag: Some(0x9000_301A), attrib: Some(0x400) });
        let p = run(&t, &job(6, 1), fake, 100 << 30);
        assert_eq!(p["result"], "refused");
        assert!(p["why"].to_string().contains("cannot be told apart from files that live only in OneDrive"), "{p:#}");
        // a hibernated Windows is not read
        std::fs::write(t.join("win/hiberfil.sys"), b"HIBR\0\0\0\0").unwrap();
        assert!(run(&t, &job(6, 1), cloud_doc(), 100 << 30)["why"].to_string().contains("was not fully shut down"));
        std::fs::write(t.join("win/hiberfil.sys"), b"\0\0\0\0").unwrap();
        assert_eq!(run(&t, &job(6, 1), cloud_doc(), 100 << 30)["result"], "plan", "a hiberfil.sys Windows wiped is fine");
        // two folders mapped to one path
        let mut j = job(6, 1);
        j["harvest"]["folders"][1]["path"] = json!("C:\\Users\\Ann\\documents");
        assert!(run(&t, &j, cloud_doc(), 100 << 30)["why"].to_string().contains("are the same folder"));
        // a resume needs room only for what is not home yet
        let o = outcome();
        let home = t.join("home");
        let jb = job(6, 1);
        let mut f = Facts { job: &jb, outcome: &o, windows: Ok(t.join("win")), home: &home, free: Some((2 << 30) + 100), rolled_back: false, reclaimed: false, already_home: 0 };
        assert_eq!(plan(&f, &Fake(t.join("win"), cloud_doc()))["result"], "refused", "8004 bytes plus 2 GiB spare do not fit");
        f.already_home = 8004;
        assert_eq!(plan(&f, &Fake(t.join("win"), cloud_doc()))["result"], "plan", "they do once the journal says they are home");
    }

    #[test]
    fn paths_and_refusals_before_the_walk() {
        assert_eq!(on_volume("C:\\Users\\Ann\\OneDrive\\Documents").as_deref(), Some("Users/Ann/OneDrive/Documents"));
        assert_eq!(on_volume("D:\\Photos"), None);
        assert_eq!(on_volume("C:\\Users\\..\\Windows"), None);
        let t = tree("pre");
        let mut j = job(5, 1);
        j["harvest"]["folders"][1]["path"] = json!("C:\\Users\\Ann\\Documents\\Taxes");
        j["harvest"]["folders"][0]["path"] = json!("D:\\Docs");
        let p = run(&t, &j, cloud_doc(), 100 << 30);
        assert!(p["why"].to_string().contains("not on Windows' own drive C:"), "{p:#}");
        let mut j = job(5, 1);
        j["harvest"]["folders"][1]["path"] = json!("C:\\Users\\Ann\\Documents\\Taxes");
        assert!(run(&t, &j, cloud_doc(), 100 << 30)["why"].to_string().contains("Pictures is inside Documents"));
        let mut j = job(5, 1);
        j["intent"]["path"] = json!("clean-slate");
        assert_eq!(run(&t, &j, cloud_doc(), 100 << 30)["result"], "nothing-to-do");
    }
}
