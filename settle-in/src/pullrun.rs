//! Bring the files home: the copy (decided 2026-11-07, the owner; the
//! keep-Windows path only; architecture.md, Module 3, step 2).
//!
//! The one writer in the pull, and it writes only into the person's own
//! home. Windows is opened read-only twice over (`cryptsetup --readonly`,
//! then ntfs-3g `-o ro`), so whatever goes wrong here, Windows and every
//! file on it stay as they were, one boot-menu pick away (rule #4: a
//! writer is held to more than a reader). The rules, each for a way a copy
//! loses or mangles a file:
//!
//! - The source is opened `O_NOFOLLOW | O_NONBLOCK` (a pipe cannot hang
//!   it) and checked again after opening: a regular file of the size the
//!   plan saw, or it is not copied and is listed.
//! - Its bytes are hashed while they are read; the copy is synced, read
//!   back and hashed again. Two different hashes: the copy is deleted,
//!   tried once more, then listed as failed.
//! - A copy is put in place with `link()`, which never replaces a file: a
//!   name already there keeps its file, and the copy becomes
//!   "name (from Windows).ext".
//! - A journal, synced after every file, lets a power cut resume without a
//!   file copied twice. The part file's name comes from the file's own
//!   path, so a copy put in place just before the cut is found again by
//!   its inode.
//! - Out of space stops the copy; one file that cannot be read does not.

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::ffi::CString;
use std::io::{Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

pub const JOURNAL_REL: &str = "var/lib/upgrade_/settle-in/pull-journal.tsv";
pub const REPORT_REL: &str = "var/lib/upgrade_/settle-in/pull.json";
pub const KEY_REL: &str = "var/lib/upgrade_/artifacts/credentials/bitlocker-C.txt";
pub const SUFFIX: &str = " (from Windows)";

/// Who the copies belong to.
#[derive(Clone, Copy)]
pub struct Owner {
    pub uid: u32,
    pub gid: u32,
}

#[derive(Debug, PartialEq)]
pub enum CopyErr {
    /// Not copied, and listed: the source changed or could not be read.
    Skip(String),
    /// The home's drive is full: the copy stops.
    NoSpace,
    /// The journal could not be written: the copy stops.
    Journal(String),
}

fn hex(d: &[u8]) -> String {
    d.iter().map(|b| format!("{:02x}", b)).collect()
}

/// `photo.jpg` -> `photo (from Windows).jpg`, then `photo (from Windows 2).jpg`.
pub fn clash_name(name: &str, n: u32) -> String {
    let tag = if n <= 1 { SUFFIX.to_string() } else { format!(" (from Windows {})", n) };
    match name.rfind('.') {
        Some(i) if i > 0 => format!("{}{}{}", &name[..i], tag, &name[i..]),
        _ => format!("{}{}", name, tag),
    }
}

/// The part file of one source path: the same name every run, so a resume finds it.
pub fn part_name(key: &str) -> String {
    format!(".upgrade_-part-{}", &hex(&Sha256::digest(key.as_bytes()))[..16])
}

fn io_err(e: &std::io::Error) -> CopyErr {
    if e.raw_os_error() == Some(libc::ENOSPC) || e.raw_os_error() == Some(libc::EDQUOT) {
        CopyErr::NoSpace
    } else {
        CopyErr::Skip(e.to_string())
    }
}

fn hash_file(p: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(p)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex(&h.finalize()))
}

fn sync_dir(d: &Path) {
    if let Ok(f) = std::fs::File::open(d) {
        let _ = f.sync_all();
    }
}

/// A file in `dir` that is the same file (inode) as `part`: a copy put in
/// place before a power cut, under its own name or a clash name.
fn landed(dir: &Path, name: &str, part: &Path) -> Option<PathBuf> {
    let pm = std::fs::symlink_metadata(part).ok()?;
    (0..64u32)
        .map(|n| dir.join(if n == 0 { name.to_string() } else { clash_name(name, n) }))
        .find(|p| std::fs::symlink_metadata(p).is_ok_and(|m| m.ino() == pm.ino() && m.dev() == pm.dev()))
}

/// Copy one file into `dir` as `name` (or a clash name), checked by its
/// hash both ways. `commit` (the journal) is told where it landed before
/// the part file goes, so a cut at any point resumes to one copy. Returns
/// where it landed and its sha256.
pub fn copy_one(src: &Path, size: u64, dir: &Path, name: &str, key: &str, owner: Option<Owner>, commit: &mut dyn FnMut(&Path, &str) -> std::io::Result<()>) -> Result<(PathBuf, String), CopyErr> {
    let part = dir.join(part_name(key));
    let finish = |to: PathBuf, sha: String, commit: &mut dyn FnMut(&Path, &str) -> std::io::Result<()>| -> Result<(PathBuf, String), CopyErr> {
        commit(&to, &sha).map_err(|e| CopyErr::Journal(e.to_string()))?;
        let _ = std::fs::remove_file(&part);
        sync_dir(dir);
        Ok((to, sha))
    };
    // a resume: the copy was put in place, the journal line not yet written
    if let Some(p) = landed(dir, name, &part) {
        let sha = hash_file(&p).map_err(|e| io_err(&e))?;
        return finish(p, sha, commit);
    }
    let _ = std::fs::remove_file(&part);
    let mut last = String::new();
    for _ in 0..2 {
        match copy_attempt(src, size, &part, owner) {
            Ok(sha) => {
                // put in place, never over a file that is there
                for n in 0..64u32 {
                    let to = dir.join(if n == 0 { name.to_string() } else { clash_name(name, n) });
                    match std::fs::hard_link(&part, &to) {
                        Ok(()) => {
                            sync_dir(dir);
                            return finish(to, sha, commit);
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                        Err(e) => {
                            let _ = std::fs::remove_file(&part);
                            return Err(io_err(&e));
                        }
                    }
                }
                let _ = std::fs::remove_file(&part);
                return Err(CopyErr::Skip("64 files of that name are already there".into()));
            }
            Err(e @ (CopyErr::NoSpace | CopyErr::Journal(_))) => {
                let _ = std::fs::remove_file(&part);
                return Err(e);
            }
            Err(CopyErr::Skip(why)) => {
                let _ = std::fs::remove_file(&part);
                last = why;
            }
        }
    }
    Err(CopyErr::Skip(last))
}

fn copy_attempt(src: &Path, size: u64, part: &Path, owner: Option<Owner>) -> Result<String, CopyErr> {
    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(src)
        .map_err(|e| CopyErr::Skip(format!("it could not be opened ({})", e)))?;
    let md = f.metadata().map_err(|e| CopyErr::Skip(e.to_string()))?;
    if !md.file_type().is_file() {
        return Err(CopyErr::Skip("it is no longer an ordinary file".into()));
    }
    if md.len() != size {
        return Err(CopyErr::Skip(format!("it changed since the plan ({} bytes then, {} now)", size, md.len())));
    }
    let mut out = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(part).map_err(|e| io_err(&e))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut total = 0u64;
    loop {
        let n = match f.read(&mut buf) {
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(CopyErr::Skip(format!("Windows' drive could not be read here ({})", e))),
        };
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        out.write_all(&buf[..n]).map_err(|e| io_err(&e))?;
        total += n as u64;
    }
    if total != size {
        return Err(CopyErr::Skip(format!("{} of its {} bytes could be read", total, size)));
    }
    out.sync_all().map_err(|e| io_err(&e))?;
    let want = hex(&h.finalize());
    // read back what is on the disk, not what was meant to be
    let got = hash_file(part).map_err(|e| io_err(&e))?;
    if got != want {
        return Err(CopyErr::Skip("the copy did not read back the same as the original".into()));
    }
    // the person's file, with the time Windows gave it
    let times = [libc::timespec { tv_sec: md.atime(), tv_nsec: md.atime_nsec() }, libc::timespec { tv_sec: md.mtime(), tv_nsec: md.mtime_nsec() }];
    if let Ok(cp) = CString::new(part.as_os_str().as_bytes()) {
        // SAFETY: cp is NUL-terminated and times holds two timespecs.
        unsafe {
            libc::utimensat(libc::AT_FDCWD, cp.as_ptr(), times.as_ptr(), 0);
            libc::chmod(cp.as_ptr(), 0o644);
            if let Some(o) = owner {
                libc::chown(cp.as_ptr(), o.uid, o.gid);
            }
        }
    }
    Ok(want)
}

/// Make a folder and its parents under `base`, each owned by the person.
pub fn make_dirs(base: &Path, rel: &Path, owner: Option<Owner>) -> std::io::Result<PathBuf> {
    let mut d = base.to_path_buf();
    if !d.exists() {
        std::fs::create_dir_all(&d)?;
        chown(&d, owner);
    }
    for c in rel.components() {
        d.push(c);
        match std::fs::create_dir(&d) {
            Ok(()) => chown(&d, owner),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if !std::fs::symlink_metadata(&d)?.is_dir() {
                    return Err(std::io::Error::other(format!("{} is there and is not a folder", d.display())));
                }
            }
            Err(e) => return Err(e),
        }
    }
    Ok(d)
}

fn chown(p: &Path, owner: Option<Owner>) {
    if let (Some(o), Ok(cp)) = (owner, CString::new(p.as_os_str().as_bytes())) {
        // SAFETY: cp is NUL-terminated.
        unsafe {
            libc::chown(cp.as_ptr(), o.uid, o.gid);
        }
    }
}

/// The journal: one line per file put in place, `folder TAB rel TAB dest TAB sha256`.
pub struct Journal {
    path: PathBuf,
    pub done: HashMap<(String, String), (String, String)>,
}

impl Journal {
    pub fn open(path: &Path) -> Journal {
        let mut done = HashMap::new();
        for l in std::fs::read_to_string(path).unwrap_or_default().lines() {
            // a torn last line (a power cut mid-write) has fewer than four fields and is ignored
            let f: Vec<&str> = l.split('\t').collect();
            if f.len() == 4 && f[3].len() == 64 {
                done.insert((f[0].to_string(), f[1].to_string()), (f[2].to_string(), f[3].to_string()));
            }
        }
        Journal { path: path.to_path_buf(), done }
    }

    pub fn add(&mut self, folder: &str, rel: &str, dest: &str, sha: &str) -> std::io::Result<()> {
        if [folder, rel, dest].iter().any(|s| s.contains('\t') || s.contains('\n')) {
            // a name with a tab or a line break cannot be journalled; it is still copied and checked
            return Ok(());
        }
        if let Some(d) = self.path.parent() {
            std::fs::create_dir_all(d)?;
        }
        let mut f = std::fs::OpenOptions::new().create(true).append(true).mode(0o600).open(&self.path)?;
        f.write_all(format!("{}\t{}\t{}\t{}\n", folder, rel, dest, sha).as_bytes())?;
        f.sync_all()?;
        self.done.insert((folder.to_string(), rel.to_string()), (dest.to_string(), sha.to_string()));
        Ok(())
    }
}

/// The copy of every folder in a plan the caller has just made.
/// `progress` is told after every file.
pub fn copy_all(plan: &Value, win: &Path, journal: &mut Journal, owner: Option<Owner>, probe: &dyn crate::pull::Probe, progress: &mut dyn FnMut(&Value)) -> Value {
    if plan["result"] != "plan" {
        return json!({ "result": "refused", "why": plan["why"].clone() });
    }
    let total_files: u64 = plan["folders"].as_array().into_iter().flatten().map(|f| f["files"].as_u64().unwrap_or(0)).sum();
    let total_bytes = plan["bytes"].as_u64().unwrap_or(0);
    let (mut files_done, mut bytes_done) = (0u64, 0u64);
    let mut folders = Vec::new();
    let mut stopped: Option<String> = None;
    for f in plan["folders"].as_array().cloned().unwrap_or_default() {
        let name = f["name"].as_str().unwrap_or("").to_string();
        let to = PathBuf::from(f["to"].as_str().unwrap_or(""));
        let from = crate::pull::on_volume(f["from"].as_str().unwrap_or("")).unwrap_or_default();
        let (mut copied, mut resumed, mut bytes) = (0u64, 0u64, 0u64);
        let mut failed: Vec<Value> = Vec::new();
        let mut renamed: Vec<Value> = Vec::new();
        if stopped.is_none() {
            let w = crate::pull::walk(&win.join(&from), probe);
            for (rel, size) in &w.copy {
                let relp = Path::new(rel);
                let file_name = relp.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                let key = format!("{}/{}", name, rel);
                if let Some((dest, _)) = journal.done.get(&(name.clone(), rel.clone())) {
                    if std::fs::metadata(dest).is_ok_and(|m| m.len() == *size) {
                        resumed += 1;
                        files_done += 1;
                        bytes_done += size;
                        bytes += size;
                        continue;
                    }
                }
                let dir = match make_dirs(&to, relp.parent().unwrap_or(Path::new("")), owner) {
                    Ok(d) => d,
                    Err(e) => {
                        if e.raw_os_error() == Some(libc::ENOSPC) {
                            stopped = Some("this Linux's drive is full".into());
                            break;
                        }
                        failed.push(json!({ "path": rel, "why": format!("its folder could not be made ({})", e) }));
                        continue;
                    }
                };
                let mut commit = |to: &Path, sha: &str| journal.add(&name, rel, &to.to_string_lossy(), sha);
                match copy_one(&win.join(&from).join(relp), *size, &dir, &file_name, &key, owner, &mut commit) {
                    Ok((dest, _)) => {
                        if dest.file_name().map(|n| n.to_string_lossy().to_string()) != Some(file_name.clone()) {
                            renamed.push(json!({ "path": rel, "as": dest.file_name().map(|n| n.to_string_lossy().to_string()) }));
                        }
                        copied += 1;
                        bytes += size;
                    }
                    Err(CopyErr::NoSpace) => {
                        stopped = Some("this Linux's drive is full".into());
                        break;
                    }
                    Err(CopyErr::Journal(e)) => {
                        stopped = Some(format!("the copy's journal could not be written ({})", e));
                        break;
                    }
                    Err(CopyErr::Skip(why)) => failed.push(json!({ "path": rel, "why": why })),
                }
                files_done += 1;
                bytes_done += size;
                progress(&json!({ "folder": name, "files_done": files_done, "files": total_files, "bytes_done": bytes_done, "bytes": total_bytes }));
            }
        }
        folders.push(json!({
            "name": name, "to": to.to_string_lossy(), "copied": copied, "already_copied": resumed, "bytes": bytes,
            "failed": failed, "renamed": renamed, "not_copied": f["not_copied"].clone(), "in_onedrive": f["in_onedrive"].clone(),
        }));
    }
    let any_failed = folders.iter().any(|f| f["failed"].as_array().is_some_and(|a| !a.is_empty()));
    json!({
        "result": if stopped.is_some() { "stopped" } else if any_failed { "done-with-failures" } else { "done" },
        "why": stopped,
        "folders": folders,
        "files": files_done,
        "bytes": bytes_done,
    })
}

/// The recovery key file's lines: its conversion status and the keys that
/// pass the check (`evaluate/job/src/recovery.rs`'s format).
pub fn read_key_file(text: &str) -> (Option<i64>, Vec<String>) {
    let mut status = None;
    let mut keys = Vec::new();
    for l in text.lines().map(str::trim) {
        if let Some(s) = l.strip_prefix("conversion-status:") {
            status = s.trim().parse().ok();
        } else if !l.starts_with('#') && valid_key(l) {
            keys.push(l.to_string());
        }
    }
    (status, keys)
}

/// Eight groups of six digits, each a multiple of 11 below 720,896.
pub fn valid_key(k: &str) -> bool {
    let g: Vec<&str> = k.split('-').collect();
    g.len() == 8 && g.iter().all(|g| g.len() == 6 && g.bytes().all(|b| b.is_ascii_digit()) && g.parse::<u32>().is_ok_and(|n| n % 11 == 0 && n < 720_896))
}

pub mod live {
    use super::*;
    use crate::reclaimrun::live::run;
    use std::process::{Command, Stdio};

    const MAPPER: &str = "upgrade_-pull-windows";
    const MOUNT_REL: &str = "run/upgrade_/pull-windows";
    const PATH_ENV: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

    /// Windows, opened read-only for as long as this lives.
    pub struct Opened {
        pub dir: PathBuf,
        mapped: bool,
    }
    impl Drop for Opened {
        fn drop(&mut self) {
            let _ = run("umount", &[&self.dir.to_string_lossy()]);
            if self.mapped {
                let _ = run("cryptsetup", &["close", MAPPER]);
            }
        }
    }

    fn is_root() -> bool {
        // SAFETY: geteuid has no preconditions.
        unsafe { libc::geteuid() == 0 }
    }

    /// BitLocker's signature, `-FVE-FS-` at byte 3 of the partition.
    fn is_bitlocker(dev: &str) -> Result<bool, String> {
        let mut f = std::fs::File::open(dev).map_err(|e| format!("{}: {}", dev, e))?;
        let mut b = [0u8; 11];
        f.read_exact(&mut b).map_err(|e| format!("{}: {}", dev, e))?;
        Ok(&b[3..11] == b"-FVE-FS-")
    }

    /// The person: who started pkexec, their uid, gid and home.
    pub fn person(r: &str) -> Option<(String, Owner, PathBuf)> {
        let name = crate::walkaway::person(if r.is_empty() { "/" } else { r })?;
        let passwd = std::fs::read_to_string(format!("{}/etc/passwd", r)).ok()?;
        let f: Vec<String> = passwd.lines().map(|l| l.split(':').map(str::to_string).collect::<Vec<_>>()).find(|f| f.len() >= 7 && f[0] == name)?;
        Some((name, Owner { uid: f[2].parse().ok()?, gid: f[3].parse().ok()? }, PathBuf::from(format!("{}{}", r, f[5]))))
    }

    /// Find, unlock (when it is BitLocker) and mount Windows read-only.
    pub fn open_windows(r: &str, typed_key: Option<&str>) -> Result<Opened, String> {
        let outcome = crate::read_json(&format!("{}/{}/outcome.json", r, crate::HANDOFF)).unwrap_or(Value::Null);
        let (disk, _, _) = crate::reclaim::live::linux_disk(r);
        let disk = disk.ok_or("the disk Linux runs from is not known")?;
        let table = crate::reclaim::live::disk_table(r, &disk).map_err(|e| format!("the disk's partition table is not whole ({})", e))?;
        let dev = crate::reclaim::live::windows_device(r, &disk, &table, &outcome).ok_or("the Windows partition the install recorded is not on this disk")?;
        let dev = format!("{}{}", r, dev);
        let mp = format!("{}/{}", r, MOUNT_REL);
        let _ = run("umount", &[&mp]);
        let _ = run("cryptsetup", &["close", MAPPER]);
        std::fs::create_dir_all(&mp).map_err(|e| format!("{}: {}", mp, e))?;
        let mut source = dev.clone();
        let mut mapped = false;
        if is_bitlocker(&dev)? {
            let text = std::fs::read_to_string(format!("{}/{}", r, KEY_REL)).unwrap_or_default();
            let (status, keys) = read_key_file(&text);
            match status {
                Some(1) => {}
                Some(2..=5) => return Err("Windows was still encrypting or decrypting its drive when it was checked, and a drive caught halfway is not read here. Start Windows, let it finish (Settings, Privacy and security, Device encryption), then try again".into()),
                _ => return Err("this Linux does not know whether Windows' drive is wholly encrypted (the record from Windows is missing or from an older kit), so it is not read".into()),
            }
            let key = match typed_key {
                Some(k) if valid_key(k.trim()) => k.trim().to_string(),
                Some(_) => return Err("that is not a BitLocker recovery key: it is eight groups of six digits, like 123456-123456-...".into()),
                None => keys.first().cloned().ok_or("Windows' drive is locked with BitLocker, and its recovery key did not come across. Find it at aka.ms/myrecoverykey (sign in with the Microsoft account you use on Windows) and type it in")?,
            };
            // the key on stdin, as the rig read it (rig/hyperv/guest/v3-read.sh), never on a command line
            let mut c = Command::new("cryptsetup")
                .args(["open", "--type", "bitlk", "--readonly", &dev, MAPPER])
                .env("PATH", PATH_ENV)
                .env("LC_ALL", "C")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| format!("cryptsetup could not start ({})", e))?;
            if let Some(mut i) = c.stdin.take() {
                let _ = i.write_all(format!("{}\n", key).as_bytes());
            }
            let o = c.wait_with_output().map_err(|e| e.to_string())?;
            if !o.status.success() {
                return Err(format!("Windows' drive could not be unlocked with its recovery key ({})", String::from_utf8_lossy(&o.stderr).trim()));
            }
            mapped = true;
            source = format!("/dev/mapper/{}", MAPPER);
        }
        if let Err(e) = run("mount", &["-t", "ntfs-3g", "-o", "ro,nosuid,nodev,noexec", &source, &mp]) {
            if mapped {
                let _ = run("cryptsetup", &["close", MAPPER]);
            }
            return Err(format!("Windows' drive could not be opened read-only ({}). If it is open in the file manager, close it there and try again", e));
        }
        Ok(Opened { dir: PathBuf::from(mp), mapped })
    }

    /// The stick's copy of the key, deleted when the kit stick is plugged in.
    fn scrub_stick(root: &str) -> &'static str {
        match crate::rollback::live::find_stick(root) {
            Ok(m) => {
                let p = format!("{}/upgrade_/{}", m.dir, KEY_REL.trim_start_matches("var/lib/upgrade_/"));
                let r = if !Path::new(&p).exists() || std::fs::remove_file(&p).is_ok() { "deleted" } else { "still-there" };
                let _ = run("sync", &[]);
                if m.by_us {
                    let _ = run("umount", &[&m.dir]);
                }
                r
            }
            Err(_) => "not-plugged-in",
        }
    }

    fn facts<'a>(r: &str, job: &'a Value, outcome: &'a Value, win: Result<PathBuf, String>, home: &'a Path) -> crate::pull::Facts<'a> {
        crate::pull::Facts {
            job,
            outcome,
            windows: win,
            home,
            free: crate::pull::live::free_bytes(home),
            rolled_back: Path::new(&format!("{}/boot/efi/upgrade_/rolled-back.json", r)).exists(),
            reclaimed: Path::new(&format!("{}/boot/efi/{}", r, crate::reclaimrun::ESP_DONE_REL)).exists(),
        }
    }

    /// `settle-in pull check` (root): the plan, with Windows opened by us
    /// read-only and closed again. Writes nothing.
    pub fn check(root: &str, typed_key: Option<&str>) -> Value {
        let r = root.trim_end_matches('/');
        if r.is_empty() && !is_root() {
            return json!({ "result": "refused", "why": ["this needs administrator rights (run it through pkexec)"] });
        }
        let job = crate::read_json(&format!("{}/{}/job.json", r, crate::HANDOFF)).unwrap_or(Value::Null);
        let outcome = crate::read_json(&format!("{}/{}/outcome.json", r, crate::HANDOFF)).unwrap_or(Value::Null);
        let Some((_, _, home)) = person(r) else {
            return json!({ "result": "refused", "why": ["whose files these are could not be told (start this from your own account)"] });
        };
        // nothing to open when there is nothing to pull
        let pre = crate::pull::plan(&facts(r, &job, &outcome, Err("not opened".into()), &home), &crate::pull::live::Ntfs3g);
        if pre["result"] == "nothing-to-do" {
            return pre;
        }
        let opened = open_windows(r, typed_key);
        let win = opened.as_ref().map(|o| o.dir.clone()).map_err(|e| e.clone());
        let mut p = crate::pull::plan(&facts(r, &job, &outcome, win, &home), &crate::pull::live::Ntfs3g);
        if let Err(e) = &opened {
            p["needs_key"] = json!(e.contains("aka.ms/myrecoverykey") || e.contains("not a BitLocker recovery key") || e.contains("could not be unlocked"));
        }
        drop(opened);
        p
    }

    /// `settle-in pull run` (root): the plan made again, then the copy,
    /// then the key deleted. Prints `progress {...}` lines, then the result.
    pub fn run_pull(root: &str, typed_key: Option<&str>) -> Value {
        let r = root.trim_end_matches('/');
        if r.is_empty() && !is_root() {
            return json!({ "result": "refused", "why": ["this needs administrator rights (run it through pkexec)"] });
        }
        let job = crate::read_json(&format!("{}/{}/job.json", r, crate::HANDOFF)).unwrap_or(Value::Null);
        let outcome = crate::read_json(&format!("{}/{}/outcome.json", r, crate::HANDOFF)).unwrap_or(Value::Null);
        let Some((who, owner, home)) = person(r) else {
            return json!({ "result": "refused", "why": ["whose files these are could not be told (start this from your own account)"] });
        };
        // once every file is home, a second run copies nothing: it only tries the stick's key again
        let before = crate::read_json(&format!("{}/{}", r, REPORT_REL)).unwrap_or(Value::Null);
        if before["result"] == "done" {
            let mut out = before.clone();
            out["result"] = json!("nothing-to-do");
            out["why"] = json!(["your files were already brought over"]);
            out["key"]["stick"] = json!(scrub_stick(root));
            let _ = crate::save(&format!("{}/{}", r, REPORT_REL), &{ let mut b = before; b["key"]["stick"] = out["key"]["stick"].clone(); b });
            let _ = crate::record_pull(r, &{ let mut o = out.clone(); o["result"] = json!("done"); o });
            return out;
        }
        // no sleep while it copies (the walk-away's lesson: KDE slept 45 minutes in, 2026-10-28)
        let mut inhibit = Command::new("systemd-inhibit")
            .args(["--what=sleep:idle:handle-lid-switch", "--who=upgrade_", "--why=bringing your files over from Windows", "sleep", "infinity"])
            .env("PATH", PATH_ENV)
            .spawn()
            .ok();
        let opened = match open_windows(r, typed_key) {
            Ok(o) => o,
            Err(e) => {
                if let Some(c) = inhibit.as_mut() {
                    let _ = c.kill();
                }
                return json!({ "result": "refused", "why": [e] });
            }
        };
        let plan = crate::pull::plan(&facts(r, &job, &outcome, Ok(opened.dir.clone()), &home), &crate::pull::live::Ntfs3g);
        let mut journal = Journal::open(Path::new(&format!("{}/{}", r, JOURNAL_REL)));
        let mut out = copy_all(&plan, &opened.dir, &mut journal, Some(owner), &crate::pull::live::Ntfs3g, &mut |p| {
            println!("progress {}", p);
            let _ = std::io::stdout().flush();
        });
        // the bookmarks, into the person's Documents (R20: bookmarks only, as files to import)
        if out["result"] != "stopped" && out["result"] != "refused" {
            if let Some(u) = crate::bookmarks::user_dir(&job) {
                let found = crate::bookmarks::find(&opened.dir, &u);
                let docs = plan["folders"].as_array().into_iter().flatten().find(|f| f["name"] == "Documents").and_then(|f| f["to"].as_str()).map(PathBuf::from)
                    .unwrap_or_else(|| crate::pull::destinations(&home).into_iter().find(|(n, _)| n == "Documents").map(|(_, p)| p).unwrap_or_else(|| home.join("Documents")));
                out["bookmarks"] = match make_dirs(&docs, Path::new(""), Some(owner)) {
                    Ok(d) => crate::bookmarks::write(&found, &d, Some(owner)),
                    Err(e) => json!([{ "browser": "all", "result": "failed", "why": e.to_string() }]),
                };
            }
        }
        drop(opened);
        if let Some(c) = inhibit.as_mut() {
            let _ = c.kill();
        }
        // the key is spent once every file is home (R13; scrub_after: settle-in-pull)
        if out["result"] == "done" {
            let here = std::fs::remove_file(format!("{}/{}", r, KEY_REL)).is_ok() || !Path::new(&format!("{}/{}", r, KEY_REL)).exists();
            out["key"] = json!({ "deleted_here": here, "stick": scrub_stick(root) });
        }
        out["person"] = json!(who);
        let _ = crate::save(&format!("{}/{}", r, REPORT_REL), &out);
        let _ = crate::record_pull(r, &out);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Plain;
    impl crate::pull::Probe for Plain {
        fn ntfs(&self, _: &Path) -> crate::pull::Ntfs {
            crate::pull::Ntfs::default()
        }
    }

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("settle-in-pullrun-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn names_on_a_clash() {
        assert_eq!(clash_name("photo.jpg", 1), "photo (from Windows).jpg");
        assert_eq!(clash_name("photo.jpg", 2), "photo (from Windows 2).jpg");
        assert_eq!(clash_name("README", 1), "README (from Windows)");
        assert_eq!(clash_name(".bashrc", 1), ".bashrc (from Windows)");
        assert_eq!(part_name("Documents/a.txt"), part_name("Documents/a.txt"));
        assert_ne!(part_name("Documents/a.txt"), part_name("Documents/b.txt"));
    }

    #[test]
    fn copies_checks_and_never_replaces() {
        let d = dir("one");
        let src = d.join("src.txt");
        std::fs::write(&src, "the person's words").unwrap();
        let to = d.join("to");
        std::fs::create_dir_all(&to).unwrap();
        let (p, sha) = copy_one(&src, 18, &to, "src.txt", "k1", None, &mut |_, _| Ok(())).unwrap();
        assert_eq!(p, to.join("src.txt"));
        assert_eq!(std::fs::read(&p).unwrap(), b"the person's words");
        assert_eq!(sha, hex(&Sha256::digest(b"the person's words")));
        assert_eq!(std::fs::metadata(&p).unwrap().mtime(), std::fs::metadata(&src).unwrap().mtime());
        // a file already there keeps its bytes
        std::fs::write(to.join("mine.txt"), "already here").unwrap();
        let (p2, _) = copy_one(&src, 18, &to, "mine.txt", "k2", None, &mut |_, _| Ok(())).unwrap();
        assert_eq!(p2, to.join("mine (from Windows).txt"));
        assert_eq!(std::fs::read_to_string(to.join("mine.txt")).unwrap(), "already here");
        // no part file left behind
        assert!(std::fs::read_dir(&to).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().starts_with(".upgrade_-part")));
    }

    #[test]
    fn a_changed_or_strange_source_is_not_copied() {
        let d = dir("odd");
        let src = d.join("grew.txt");
        std::fs::write(&src, "longer now").unwrap();
        let to = d.join("to");
        std::fs::create_dir_all(&to).unwrap();
        assert!(matches!(copy_one(&src, 3, &to, "grew.txt", "k", None, &mut |_, _| Ok(())), Err(CopyErr::Skip(w)) if w.contains("changed since the plan")));
        // a pipe: opened without waiting, refused
        let fifo = d.join("pipe");
        let c = CString::new(fifo.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o644) }, 0);
        assert!(matches!(copy_one(&fifo, 0, &to, "pipe", "k", None, &mut |_, _| Ok(())), Err(CopyErr::Skip(w)) if w.contains("no longer an ordinary file")));
        // a link: not followed
        std::os::unix::fs::symlink(&src, d.join("link")).unwrap();
        assert!(matches!(copy_one(&d.join("link"), 10, &to, "link", "k", None, &mut |_, _| Ok(())), Err(CopyErr::Skip(w)) if w.contains("could not be opened")));
        assert_eq!(std::fs::read_dir(&to).unwrap().count(), 0, "nothing landed");
    }

    #[test]
    fn a_cut_after_landing_resumes_without_a_second_copy() {
        let d = dir("cut");
        let src = d.join("a.txt");
        std::fs::write(&src, "abc").unwrap();
        let to = d.join("to");
        std::fs::create_dir_all(&to).unwrap();
        // the state a power cut leaves: the part file and its link in place, no journal line
        let part = to.join(part_name("Documents/a.txt"));
        std::fs::write(&part, "abc").unwrap();
        std::fs::hard_link(&part, to.join("a.txt")).unwrap();
        let (p, _) = copy_one(&src, 3, &to, "a.txt", "Documents/a.txt", None, &mut |_, _| Ok(())).unwrap();
        assert_eq!(p, to.join("a.txt"));
        assert!(!to.join("a (from Windows).txt").exists(), "not copied twice");
        assert!(!part.exists());
    }

    #[test]
    fn a_journal_that_fails_stops_and_resumes_to_one_copy() {
        let d = dir("jfail");
        let src = d.join("a.txt");
        std::fs::write(&src, "abc").unwrap();
        let to = d.join("to");
        std::fs::create_dir_all(&to).unwrap();
        let r = copy_one(&src, 3, &to, "a.txt", "Documents/a.txt", None, &mut |_, _| Err(std::io::Error::other("disk gone")));
        assert!(matches!(r, Err(CopyErr::Journal(_))));
        assert!(to.join("a.txt").exists() && to.join(part_name("Documents/a.txt")).exists(), "the part stays, so the resume can find the copy");
        let mut told = None;
        let (p, _) = copy_one(&src, 3, &to, "a.txt", "Documents/a.txt", None, &mut |to, _| { told = Some(to.to_path_buf()); Ok(()) }).unwrap();
        assert_eq!(p, to.join("a.txt"));
        assert_eq!(told, Some(to.join("a.txt")), "the journal is told on the resume");
        assert_eq!(std::fs::read_dir(&to).unwrap().count(), 1, "one copy, no part");
    }

    #[test]
    fn the_journal_survives_a_torn_line() {
        let d = dir("journal");
        let j = d.join("j.tsv");
        let mut a = Journal::open(&j);
        a.add("Documents", "x.txt", "/home/ann/Documents/x.txt", &"0".repeat(64)).unwrap();
        std::fs::OpenOptions::new().append(true).open(&j).unwrap().write_all(b"Documents\ty.txt\t/home/a").unwrap();
        let b = Journal::open(&j);
        assert_eq!(b.done.len(), 1);
        assert!(b.done.contains_key(&("Documents".to_string(), "x.txt".to_string())));
    }

    #[test]
    fn the_whole_copy_and_a_resume() {
        let d = dir("all");
        let docs = d.join("win/Users/Ann/Documents");
        std::fs::create_dir_all(docs.join("Taxes")).unwrap();
        std::fs::write(docs.join("a.txt"), "one").unwrap();
        std::fs::write(docs.join("Taxes/b.pdf"), vec![9u8; 4000]).unwrap();
        let to = d.join("home/Documents");
        let plan = json!({ "result": "plan", "bytes": 4003, "folders": [{ "name": "Documents", "from": "C:\\Users\\Ann\\Documents", "to": to.to_string_lossy(), "files": 2, "not_copied": [], "in_onedrive": 0 }] });
        let mut j = Journal::open(&d.join("j.tsv"));
        let mut ticks = 0;
        let out = copy_all(&plan, &d.join("win"), &mut j, None, &Plain, &mut |_| ticks += 1);
        assert_eq!(out["result"], "done", "{out:#}");
        assert_eq!(out["folders"][0]["copied"], 2);
        assert_eq!(ticks, 2);
        assert_eq!(std::fs::read(to.join("Taxes/b.pdf")).unwrap(), vec![9u8; 4000]);
        // run again from the journal: nothing copied twice
        let mut j = Journal::open(&d.join("j.tsv"));
        let again = copy_all(&plan, &d.join("win"), &mut j, None, &Plain, &mut |_| {});
        assert_eq!(again["folders"][0]["copied"], 0);
        assert_eq!(again["folders"][0]["already_copied"], 2);
        assert_eq!(std::fs::read_dir(&to).unwrap().count(), 2, "a.txt and Taxes, no copies");
        // a refused plan copies nothing
        assert_eq!(copy_all(&json!({ "result": "refused", "why": ["x"] }), &d, &mut j, None, &Plain, &mut |_| {})["result"], "refused");
    }

    #[test]
    fn the_key_file() {
        let t = "# words\nconversion-status: 1\n000011-000022-000033-000044-000055-000066-000077-000088\nbogus\n";
        assert_eq!(read_key_file(t), (Some(1), vec!["000011-000022-000033-000044-000055-000066-000077-000088".to_string()]));
        assert_eq!(read_key_file("conversion-status: unknown\n# none: x\n"), (None, vec![]));
        assert!(!valid_key("000012-000022-000033-000044-000055-000066-000077-000088"));
    }
}
