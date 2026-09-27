//! "Go back to Windows": write Microsoft's installer to the one USB stick the
//! person named (RISKS R16, R30). The only part of this program that writes
//! to a disk, so rule #4: it re-checks everything itself, and any doubt stops
//! it before the first byte.
//!
//! Before anything is written:
//!   1. the file is hashed again and must be in Microsoft's table, and be the
//!      Windows this computer was offered;
//!   2. the stick is found again from a FRESH list by serial number, its size
//!      must match to the byte, the typed word must be its model name, and it
//!      must break none of R16's rules (sticks::choose);
//!   3. only the desktop's own removable-media mounts are undone, and the
//!      disk is opened exclusively (the kernel refuses if anything holds it).
//! Then: a DOS partition table with one FAT32 partition (what Microsoft's
//! own instructions use; UEFI starts from it), every file copied, the one
//! file over 4 GB (sources/install.wim) split into install.swm parts as
//! Microsoft's instructions say, and every file read back from the stick
//! against the original.
//!
//! `--image PATH` writes a new file instead of a stick (tests and the rig):
//! the path must not exist yet, so no disk and no existing file can be hit.

use crate::{goback, sticks};
use serde_json::{json, Value};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::process::Command;

const FAT32_MAX: u64 = 4 * 1024 * 1024 * 1024 - 1;
const SPLIT_MB: &str = "3800"; // Microsoft's own example size
const LABEL: &str = "WINSETUP";
const PATH_ENV: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

pub struct Request {
    pub iso: String,
    pub want: Option<String>,
    pub serial: String,
    pub size_bytes: u64,
    pub typed: String,
    pub image: Option<String>,
    pub wimlib: String,
    pub root: String,
}

/// What happened, step by step. Every stop names its step and reason.
struct Log {
    steps: Vec<Value>,
}

impl Log {
    fn ok(&mut self, step: &str, detail: Value) {
        eprintln!("{}", json!({ "step": step, "ok": true }));
        self.steps.push(json!({ "step": step, "ok": true, "detail": detail }));
    }
    fn stop(&mut self, step: &str, why: impl Into<String>, wrote: bool) -> Value {
        let why = why.into();
        eprintln!("{}", json!({ "step": step, "ok": false, "why": why }));
        self.steps.push(json!({ "step": step, "ok": false, "why": why }));
        json!({
            "result": "stopped",
            "stopped_at": step,
            "why": why,
            // the plain truth about the stick after a stop
            "stick_changed": wrote,
            "steps": self.steps,
        })
    }
}

fn run(prog: &str, args: &[&str]) -> Result<String, String> {
    let o = Command::new(prog).args(args).env("PATH", PATH_ENV).env("LC_ALL", "C").output().map_err(|e| format!("{} could not start ({})", prog, e))?;
    if o.status.success() {
        Ok(String::from_utf8_lossy(&o.stdout).to_string())
    } else {
        Err(format!("{} {} failed: {}", prog, args.join(" "), String::from_utf8_lossy(&o.stderr).trim()))
    }
}

/// The partition's device name: sdc -> sdc1, loop0 / nvme0n1 / mmcblk0 -> ...p1.
pub fn part1(disk: &str) -> String {
    if disk.chars().last().map(|c| c.is_ascii_digit()).unwrap_or(false) { format!("{}p1", disk) } else { format!("{}1", disk) }
}

/// A DOS partition table: one bootable FAT32 (LBA) partition from 1 MiB to
/// the end. Pure, so the bytes are tested.
pub fn mbr(disk_sectors: u64, disk_id: u32) -> [u8; 512] {
    let mut m = [0u8; 512];
    m[440..444].copy_from_slice(&disk_id.to_le_bytes());
    let start: u64 = 2048;
    let count = disk_sectors.saturating_sub(start).min(u32::MAX as u64);
    let e = &mut m[446..462];
    e[0] = 0x80; // active: what Microsoft's steps ask for
    e[1..4].copy_from_slice(&[0xFE, 0xFF, 0xFF]); // CHS: use LBA
    e[4] = 0x0C; // FAT32, LBA
    e[5..8].copy_from_slice(&[0xFE, 0xFF, 0xFF]);
    e[8..12].copy_from_slice(&(start as u32).to_le_bytes());
    e[12..16].copy_from_slice(&(count as u32).to_le_bytes());
    m[510] = 0x55;
    m[511] = 0xAA;
    m
}

/// Every file under a directory, relative paths, sorted.
fn walk(base: &str) -> Result<Vec<(String, u64)>, String> {
    let mut out = Vec::new();
    let mut stack = vec![String::new()];
    while let Some(rel) = stack.pop() {
        let dir = if rel.is_empty() { base.to_string() } else { format!("{}/{}", base, rel) };
        for e in std::fs::read_dir(&dir).map_err(|e| format!("{}: {}", dir, e))?.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let r = if rel.is_empty() { name } else { format!("{}/{}", rel, name) };
            let md = e.metadata().map_err(|e| format!("{}: {}", r, e))?;
            if md.is_dir() { stack.push(r) } else { out.push((r, md.len())) }
        }
    }
    out.sort();
    Ok(out)
}

/// Which files cannot go on FAT32 as they are. Only install.wim may be
/// split; anything else that big stops the write. Pure.
pub fn plan_files(files: &[(String, u64)]) -> Result<(Vec<(String, u64)>, Option<String>), String> {
    let mut copy = Vec::new();
    let mut split = None;
    for (rel, size) in files {
        if *size > FAT32_MAX {
            if rel.eq_ignore_ascii_case("sources/install.wim") {
                split = Some(rel.clone());
            } else {
                return Err(format!("{} is larger than a FAT32 stick can hold, and only sources/install.wim can be split", rel));
            }
        } else {
            copy.push((rel.clone(), *size));
        }
    }
    Ok((copy, split))
}

fn sha_of(path: &str) -> Result<String, String> {
    goback::sha256_file(path, |_, _| {}).map(|(h, _)| h)
}

fn copy_file(src: &str, dst: &str) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    if let Some(p) = std::path::Path::new(dst).parent() {
        std::fs::create_dir_all(p).map_err(|e| format!("{}: {}", p.display(), e))?;
    }
    let mut i = std::fs::File::open(src).map_err(|e| format!("{}: {}", src, e))?;
    let mut o = std::fs::OpenOptions::new().write(true).create_new(true).open(dst).map_err(|e| format!("{}: {}", dst, e))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 4 << 20];
    loop {
        let n = i.read(&mut buf).map_err(|e| format!("{}: {}", src, e))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        o.write_all(&buf[..n]).map_err(|e| format!("{}: {}", dst, e))?;
    }
    o.sync_all().map_err(|e| format!("{}: {}", dst, e))?;
    Ok(h.finalize().iter().map(|b| format!("{:02x}", b)).collect())
}

struct Mounts {
    points: Vec<String>,
    loops: Vec<String>,
}

impl Drop for Mounts {
    fn drop(&mut self) {
        for p in self.points.iter().rev() {
            let _ = run("umount", &[p]);
            let _ = std::fs::remove_dir(p);
        }
        for l in &self.loops {
            let _ = run("losetup", &["-d", l]);
        }
    }
}

fn wait_for(path: &str) -> bool {
    for _ in 0..50 {
        if std::path::Path::new(path).exists() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    false
}

pub fn write(req: &Request) -> Value {
    let mut log = Log { steps: Vec::new() };
    // SAFETY: geteuid has no preconditions.
    if unsafe { libc::geteuid() } != 0 {
        return log.stop("rights", "writing a stick needs administrator rights (run it through pkexec)", false);
    }
    if !std::path::Path::new(&req.wimlib).exists() {
        return log.stop("tools", format!("the tool that splits Windows' largest file is missing ({})", req.wimlib), false);
    }
    for t in ["mkfs.vfat", "mount", "umount", "losetup"] {
        if run("sh", &["-c", &format!("command -v {}", t)]).is_err() {
            return log.stop("tools", format!("this system lacks {}", t), false);
        }
    }

    // 1. the file, again
    let chk = goback::check_report(&req.iso, req.want.as_deref(), &goback::media());
    if chk["result"] != "verified" {
        return log.stop("check-file", chk["why"].as_str().unwrap_or("the file is not Microsoft's installer").to_string(), false);
    }
    let iso_size = chk["size"].as_u64().unwrap_or(0);
    log.ok("check-file", json!({ "sha256": chk["sha256"], "media": chk["media"] }));
    let min = iso_size + 256 * 1024 * 1024;

    let mut mounts = Mounts { points: Vec::new(), loops: Vec::new() };
    // 2. the stick, found again
    let (dev, disk_name, sectors) = if let Some(img) = &req.image {
        if std::path::Path::new(img).exists() {
            return log.stop("find-stick", format!("{} already exists; a test image must be a new file", img), false);
        }
        let f = match std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(img) {
            Ok(f) => f,
            Err(e) => return log.stop("find-stick", format!("{}: {}", img, e), false),
        };
        if let Err(e) = f.set_len(req.size_bytes) {
            return log.stop("find-stick", format!("{}: {}", img, e), false);
        }
        drop(f);
        if req.size_bytes < min {
            let _ = std::fs::remove_file(img);
            return log.stop("find-stick", format!("the image is too small for this installer ({} bytes, {} needed)", req.size_bytes, min), false);
        }
        let l = match run("losetup", &["--find", "--show", "--partscan", img]) {
            Ok(l) => l.trim().to_string(),
            Err(e) => return log.stop("find-stick", e, false),
        };
        mounts.loops.push(l.clone());
        let name = l.trim_start_matches("/dev/").to_string();
        log.ok("find-stick", json!({ "image": img, "device": l }));
        (l, name, req.size_bytes / 512)
    } else {
        let fresh = sticks::collect(&req.root);
        match sticks::choose(&fresh, &req.serial, req.size_bytes, &req.typed, min) {
            Err(why) => return log.stop("find-stick", why.join("; "), false),
            Ok(d) => {
                let name = d["name"].as_str().unwrap_or("").to_string();
                // 3. undo only the desktop's own mounts of it
                for p in d["partitions"].as_array().cloned().unwrap_or_default() {
                    for m in p["mounted_at"].as_array().cloned().unwrap_or_default() {
                        let m = m.as_str().unwrap_or("");
                        if let Err(e) = run("umount", &[m]) {
                            return log.stop("unmount", format!("the stick is open at {} and could not be closed ({})", m, e), false);
                        }
                    }
                }
                log.ok("find-stick", json!({ "name": name, "serial": d["serial"], "size_bytes": d["size_bytes"], "model": d["model"] }));
                (format!("/dev/{}", name), name, d["size_bytes"].as_u64().unwrap_or(0) / 512)
            }
        }
    };

    // the last moment nothing has been written
    let mut disk = match std::fs::OpenOptions::new().read(true).write(true).custom_flags(libc::O_EXCL).open(&dev) {
        Ok(f) => f,
        Err(e) => return log.stop("open-stick", format!("the stick is in use by something else ({})", e), false),
    };
    // ---- from here on the stick's old contents are gone
    let zero = vec![0u8; 1 << 20];
    let end = sectors * 512;
    let id = (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(1) as u32) | 1;
    let wrote = (|| -> std::io::Result<()> {
        disk.seek(SeekFrom::Start(0))?;
        disk.write_all(&zero)?;
        // an old GPT keeps a copy at the end; clear it so nothing reads it
        disk.seek(SeekFrom::Start(end.saturating_sub(1 << 20)))?;
        disk.write_all(&zero)?;
        disk.seek(SeekFrom::Start(0))?;
        disk.write_all(&mbr(sectors, id))?;
        disk.sync_all()
    })();
    if let Err(e) = wrote {
        return log.stop("partition", format!("writing the partition table failed ({})", e), true);
    }
    // BLKRRPART: tell the kernel to read the new table
    // SAFETY: BLKRRPART takes no argument.
    let rc = unsafe { libc::ioctl(std::os::fd::AsRawFd::as_raw_fd(&disk), 0x125F as _) };
    drop(disk);
    if rc != 0 {
        return log.stop("partition", format!("the system did not accept the new partition table ({})", std::io::Error::last_os_error()), true);
    }
    let part = format!("/dev/{}", part1(&disk_name));
    let _ = run("udevadm", &["settle"]);
    if !wait_for(&part) {
        return log.stop("partition", format!("{} did not appear", part), true);
    }
    log.ok("partition", json!({ "table": "dos", "partition": part, "type": "0x0C FAT32 LBA, active" }));
    if let Err(e) = run("mkfs.vfat", &["-F", "32", "-n", LABEL, &part]) {
        return log.stop("format", e, true);
    }
    log.ok("format", json!({ "fs": "FAT32", "label": LABEL }));

    // mount both, in a private folder
    let base = format!("/run/upgrade_-go-back.{}", std::process::id());
    let (isodir, stickdir) = (format!("{}/iso", base), format!("{}/stick", base));
    for d in [&isodir, &stickdir] {
        if let Err(e) = std::fs::create_dir_all(d) {
            return log.stop("mount", format!("{}: {}", d, e), true);
        }
    }
    let _ = std::fs::set_permissions(&base, std::os::unix::fs::PermissionsExt::from_mode(0o700));
    if run("mount", &["-o", "ro,loop", "-t", "udf", &req.iso, &isodir]).is_err() {
        if let Err(e) = run("mount", &["-o", "ro,loop", "-t", "iso9660", &req.iso, &isodir]) {
            return log.stop("mount", e, true);
        }
    }
    mounts.points.push(isodir.clone());
    if let Err(e) = run("mount", &["-t", "vfat", "-o", "flush", &part, &stickdir]) {
        return log.stop("mount", e, true);
    }
    mounts.points.push(stickdir.clone());

    // the files
    let files = match walk(&isodir) {
        Ok(f) => f,
        Err(e) => return log.stop("read-installer", e, true),
    };
    let (copy, split) = match plan_files(&files) {
        Ok(p) => p,
        Err(e) => return log.stop("read-installer", e, true),
    };
    let total: u64 = files.iter().map(|f| f.1).sum();
    if total + 64 * 1024 * 1024 > end {
        return log.stop("read-installer", format!("the installer needs {:.1} GB; the stick holds {:.1} GB", total as f64 / 1e9, end as f64 / 1e9), true);
    }
    let mut sums = Vec::new();
    let mut done = 0u64;
    for (rel, size) in &copy {
        match copy_file(&format!("{}/{}", isodir, rel), &format!("{}/{}", stickdir, rel)) {
            Ok(h) => sums.push((rel.clone(), h)),
            Err(e) => return log.stop("copy", e, true),
        }
        done += size;
        eprintln!("{}", json!({ "progress": done, "total": total }));
    }
    log.ok("copy", json!({ "files": copy.len(), "bytes": done }));
    if let Some(wim) = &split {
        let src = format!("{}/{}", isodir, wim);
        let dst = format!("{}/sources/install.swm", stickdir);
        if let Err(e) = run(&req.wimlib, &["split", &src, &dst, SPLIT_MB]) {
            return log.stop("split", e, true);
        }
        log.ok("split", json!({ "from": wim, "to": "sources/install*.swm", "part_mb": SPLIT_MB }));
    }

    // read back: unmount, drop the cache, mount read-only, compare
    let _ = run("umount", &[&stickdir]);
    mounts.points.retain(|p| p != &stickdir);
    let _ = run("blockdev", &["--flushbufs", &part]);
    let _ = std::fs::write("/proc/sys/vm/drop_caches", "3");
    if let Err(e) = run("mount", &["-t", "vfat", "-o", "ro", &part, &stickdir]) {
        return log.stop("read-back", e, true);
    }
    mounts.points.push(stickdir.clone());
    let mut bad = Vec::new();
    for (rel, want) in &sums {
        match sha_of(&format!("{}/{}", stickdir, rel)) {
            Ok(h) if &h == want => {}
            Ok(_) => bad.push(format!("{} differs", rel)),
            Err(e) => bad.push(e),
        }
    }
    if !bad.is_empty() {
        return log.stop("read-back", format!("{} file(s) did not read back the same: {}", bad.len(), bad.iter().take(3).cloned().collect::<Vec<_>>().join("; ")), true);
    }
    let mut detail = json!({ "files_read_back": sums.len() });
    if split.is_some() {
        let swm = format!("{}/sources/install.swm", stickdir);
        let refs = format!("--ref={}/sources/install*.swm", stickdir);
        if let Err(e) = run(&req.wimlib, &["verify", &swm, &refs]) {
            return log.stop("read-back", format!("the split Windows image did not verify ({})", e), true);
        }
        // the same images, in the same order, as the original
        let names = |p: &str, extra: Option<&str>| -> Result<Vec<String>, String> {
            let mut a = vec!["info", p];
            if let Some(x) = extra { a.push(x) }
            Ok(run(&req.wimlib, &a)?.lines().filter_map(|l| l.strip_prefix("Name:").map(|v| v.trim().to_string())).collect())
        };
        let (a, b) = (names(&format!("{}/sources/install.wim", isodir), None), names(&swm, Some(&refs)));
        match (a, b) {
            (Ok(a), Ok(b)) if a == b && !a.is_empty() => detail["editions"] = json!(a),
            (Ok(a), Ok(b)) => return log.stop("read-back", format!("the split image lists different editions ({:?} vs {:?})", a, b), true),
            (Err(e), _) | (_, Err(e)) => return log.stop("read-back", e, true),
        }
    }
    log.ok("read-back", detail);
    drop(mounts);
    let _ = run("sync", &[]);
    json!({ "result": "written", "stick_changed": true, "windows": chk["media"]["windows"], "language": chk["media"]["language"], "steps": log.steps })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partition_names() {
        assert_eq!(part1("sdc"), "sdc1");
        assert_eq!(part1("loop7"), "loop7p1");
        assert_eq!(part1("nvme0n1"), "nvme0n1p1");
        assert_eq!(part1("mmcblk0"), "mmcblk0p1");
    }

    #[test]
    fn the_partition_table_bytes() {
        let m = mbr(31_266_816, 0x1234_5678); // a 16 GB stick
        assert_eq!(&m[510..], &[0x55, 0xAA]);
        assert_eq!(m[446], 0x80);
        assert_eq!(m[450], 0x0C);
        assert_eq!(u32::from_le_bytes(m[454..458].try_into().unwrap()), 2048);
        assert_eq!(u32::from_le_bytes(m[458..462].try_into().unwrap()), 31_266_816 - 2048);
        assert_eq!(u32::from_le_bytes(m[440..444].try_into().unwrap()), 0x1234_5678);
        // one partition only; no boot code
        assert!(m[462..510].iter().all(|b| *b == 0));
        assert!(m[..440].iter().all(|b| *b == 0));
    }

    #[test]
    fn only_install_wim_may_be_split() {
        let big = 5_000_000_000u64;
        let (copy, split) = plan_files(&[("setup.exe".into(), 100), ("sources/install.wim".into(), big)]).unwrap();
        assert_eq!(copy.len(), 1);
        assert_eq!(split.as_deref(), Some("sources/install.wim"));
        assert!(plan_files(&[("sources/install.esd".into(), big)]).is_err());
        let (_, split) = plan_files(&[("sources/install.wim".into(), 3_000_000_000)]).unwrap();
        assert!(split.is_none());
        // exactly 4 GiB - 1 still fits
        assert!(plan_files(&[("big.bin".into(), FAT32_MAX)]).unwrap().1.is_none());
        assert!(plan_files(&[("big.bin".into(), FAT32_MAX + 1)]).is_err());
    }

    #[test]
    fn without_root_nothing_happens() {
        // SAFETY: geteuid has no preconditions.
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let r = write(&Request { iso: "/nonexistent".into(), want: None, serial: "X".into(), size_bytes: 1, typed: "X".into(), image: None, wimlib: "/nonexistent".into(), root: "/".into() });
        assert_eq!(r["result"], "stopped");
        assert_eq!(r["stopped_at"], "rights");
        assert_eq!(r["stick_changed"], false);
    }
}
