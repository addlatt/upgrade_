//! Reclaim: delete the kept Windows and move Linux into the space (RISKS
//! R40, VALIDATION V15).
//!
//! Decided (2026-11-02, the owner): everything on Windows goes, the person's
//! files with it, after a typed sentence that names what is lost; Linux's
//! `/boot` and `/` are moved to the front of the freed space so `/` grows
//! into all of it. The commit line is the window's last click.
//!
//! This file holds the two halves that can be proven without a machine:
//!
//!   - `plan`: pure. From the job, the outcome, the disk's partition table
//!     and what was read on the Windows partition, either the exact plan or
//!     every reason to refuse. It refuses on anything it cannot read, on
//!     anything on the disk it does not recognise, and on any sign that the
//!     drive is failing (R23's sentence consented to the conversion, not to
//!     this).
//!   - `move_down`: the copy that moves a range of the disk towards its
//!     start, chunk by chunk, with its progress in a journal kept off the
//!     disk. Safe to resume after a power cut at any byte, because the shift
//!     is never less than a chunk (proven below by cutting it at every
//!     chunk and in the middle of one).
//!
//! Not here yet (owed, R40): running it from the kit stick, the file
//! system's own grow (`e2fsck` and `resize2fs`, offline), removing Windows'
//! firmware entry, the 'Windows (kept)' GRUB entry and the R22 guard, and
//! the clock (a decision owed, R36). No command writes to a disk.

use crate::gpt::guid_text;
use crate::gpttable::{Entry, Table};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom, Write};

pub const ESP: &str = "c12a7328-f81f-11d2-ba4b-00a0c93ec93b";
pub const MSR: &str = "e3c9e316-0b5c-4db8-817d-f92df00215ae";
pub const MSDATA: &str = "ebd0a0a2-b9e5-4433-87c0-68b6b72699c7";
pub const WINRE: &str = "de94bba4-06d1-4d40-a16a-bfd50179d6ac";
pub const LINUX_DATA: &str = "0fc63daf-8483-4772-8e79-3d69d8477de4";

/// The move's chunk: 64 MiB. The shift must be at least this (see `move_down`).
pub const CHUNK: u64 = 64 << 20;
const MIB: u64 = 1 << 20;

/// A draft (ROADMAP, decisions owed: the owner approves the words). Typed
/// verbatim; no shorter form is accepted (the same rule as R23's).
pub const SENTENCE: &str = "I confirm that Windows and every file in it will be deleted for good, including the folders listed here";

/// What the live half read on the Windows partition, mounted read-only.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowsRead {
    pub has_system32: bool,
    /// The folder names under `Users/`, as found.
    pub users: Vec<String>,
}

pub struct Facts<'a> {
    pub job: &'a Value,
    pub outcome: &'a Value,
    /// The partition table of the disk that holds Linux's `/`.
    pub table: Result<Table, String>,
    /// Partition numbers on that disk mounted at `/` and `/boot`.
    pub root_number: Option<u32>,
    pub boot_number: Option<u32>,
    /// Another disk with a partition on it that is mounted, or any other
    /// disk holding a Windows partition: reclaim touches one disk only.
    pub other_disks_in_use: Vec<String>,
    pub windows: Result<WindowsRead, String>,
    /// `upgrade_/rolled-back.json` on the ESP: Windows was made first again.
    pub rolled_back: bool,
}

fn s<'a>(v: &'a Value, p: &str) -> &'a str {
    v.pointer(p).and_then(Value::as_str).unwrap_or("")
}

fn ty(e: &Entry) -> String {
    guid_text(&e.type_guid)
}

/// Profile folders Windows makes for itself, never a person's.
fn system_profile(name: &str) -> bool {
    ["public", "default", "default user", "all users", "defaultapppool", "desktop.ini", "wdagutilityaccount"].contains(&name.to_lowercase().as_str())
}

/// The plan, or every reason to refuse. Pure: no disk, no clock.
pub fn plan(f: &Facts) -> Value {
    let mut no: Vec<String> = Vec::new();
    let job = f.job;
    let out = f.outcome;

    // --- what kind of machine this is
    if s(job, "/schema") != "job/1" || s(out, "/schema") != "outcome/1" {
        no.push("the job or the outcome is not readable".into());
    }
    if !s(job, "/intent/path").eq_ignore_ascii_case("keep-windows") {
        no.push("this computer was not converted with Windows kept: there is nothing to reclaim".into());
    }
    if out.pointer("/windows/kept").and_then(Value::as_bool) != Some(true) {
        no.push("the install's record does not say Windows was kept".into());
    }
    if f.rolled_back {
        no.push("Windows was made to start first again (the rollback): reclaim is for a person who stays in Linux".into());
    }
    // --- the drive: a move reads and writes every block of Linux
    let health = s(job, "/storage/physical_disk/health_status");
    if health != "Healthy" {
        no.push(format!("the drive did not report itself healthy when it was scanned ({}): moving every block of Linux on it could lose Linux too", if health.is_empty() { "not recorded" } else { health }));
    }
    if job.get("risk_acknowledgement").is_some_and(|v| !v.is_null()) {
        no.push("this conversion went ahead on a drive the scanner found failing (the data-loss sentence); that sentence does not cover reclaim".into());
    }
    if s(job, "/harvest/bitlocker/status") != "off" {
        no.push("Windows' drive is locked with BitLocker (or its state is not recorded): what is on it cannot be read, so it cannot be shown before it is deleted".into());
    }
    if !f.other_disks_in_use.is_empty() {
        no.push(format!("another internal disk holds a Windows drive ({}): Windows' files may live there too, and reclaim works on one disk only", f.other_disks_in_use.join(", ")));
    }
    let off_c: Vec<&str> = job.pointer("/harvest/folders").and_then(Value::as_array).into_iter().flatten()
        .filter(|x| x.get("exists") == Some(&json!(true)))
        .filter(|x| !x["path"].as_str().is_some_and(|p| p.to_ascii_lowercase().starts_with("c:\\")))
        .filter_map(|x| x["name"].as_str())
        .collect();
    if !off_c.is_empty() {
        no.push(format!("the person's {} folder is not on Windows' own drive (C:): what would be lost cannot be said truly", off_c.join(", ")));
    }
    // --- what is on the Windows partition
    match &f.windows {
        Err(e) => no.push(format!("the Windows partition could not be read ({}): nothing is deleted unseen", e)),
        Ok(w) => {
            if !w.has_system32 {
                no.push("the partition named as Windows holds no Windows folder: it may not be Windows".into());
            }
            let me = s(job, "/intent/account/windows_name").to_lowercase();
            let others: Vec<&String> = w.users.iter().filter(|u| !system_profile(u) && u.to_lowercase() != me).collect();
            if me.is_empty() {
                no.push("the job does not name the person's Windows account".into());
            } else if !others.is_empty() {
                no.push(format!("other people's accounts are on Windows ({}): their files would be deleted without them (RISKS R5)", others.iter().map(|x| x.as_str()).collect::<Vec<_>>().join(", ")));
            }
        }
    }

    let t = match &f.table {
        Ok(t) => t,
        Err(e) => {
            no.push(format!("the disk's partition table is not whole ({})", e));
            return json!({ "result": "refused", "why": no });
        }
    };
    let sector = t.sector;
    let by_num = |n: Option<u32>| n.and_then(|n| t.entries.iter().find(|e| e.number() == n));
    let win_guid = s(out, "/windows/partition/guid").to_lowercase();
    let win = t.entries.iter().find(|e| !win_guid.is_empty() && guid_text(&e.unique) == win_guid);
    let (root, boot) = (by_num(f.root_number), by_num(f.boot_number));
    let mut delete: Vec<&Entry> = Vec::new();
    match win {
        None => no.push("the partition the install recorded as Windows is not on the disk Linux runs from".into()),
        Some(w) => {
            if ty(w) != MSDATA {
                no.push("the partition recorded as Windows is not a Windows data partition".into());
            }
            if out.pointer("/windows/partition/size_bytes").and_then(Value::as_u64) != Some(w.sectors() * sector) {
                no.push("the Windows partition is not the size the install recorded: it has changed since".into());
            }
        }
    }
    let (Some(root), Some(boot)) = (root, boot) else {
        no.push("Linux's / and /boot are not both on this disk as separate partitions".into());
        return json!({ "result": "refused", "why": no });
    };
    // Every partition on the disk must be one we can name. Anything else is
    // someone's, and stays; a layout with it in the way is refused.
    for e in &t.entries {
        let k = ty(e);
        let named = if win.is_some_and(|w| w.index == e.index) {
            delete.push(e);
            true
        } else if k == MSR || k == WINRE {
            delete.push(e);
            true
        } else {
            e.index == root.index || e.index == boot.index || k == ESP
        };
        if !named {
            no.push(format!("partition {} ({}) is neither Windows' nor this Linux's: it may hold someone's files (another Windows drive, another Linux, RISKS R39)", e.number(), if k == MSDATA { "a Windows data partition".to_string() } else { k }));
        }
    }
    if t.entries.iter().filter(|e| ty(e) == ESP).count() != 1 {
        no.push("the disk does not hold exactly one EFI system partition".into());
    }
    if !(boot.last_lba < root.first_lba) {
        no.push("/boot does not sit before / on the disk".into());
    }
    if let Some(w) = win
        && w.first_lba > boot.first_lba
    {
        no.push("Windows sits after Linux on the disk, not before it: this layout was not made by the converter".into());
    }
    // nothing kept may sit between the new start and /boot, or between / and its new end
    let kept_before: Vec<&Entry> = t.entries.iter().filter(|e| e.last_lba < boot.first_lba && !delete.iter().any(|d| d.index == e.index)).collect();
    let after: Vec<&Entry> = t.entries.iter().filter(|e| e.first_lba > root.last_lba && !delete.iter().any(|d| d.index == e.index)).collect();
    if t.entries.iter().any(|e| e.first_lba > boot.last_lba && e.last_lba < root.first_lba) {
        no.push("a partition sits between /boot and /".into());
    }
    let align = MIB / sector;
    let floor = kept_before.iter().map(|e| e.last_lba + 1).max().unwrap_or(t.first_usable);
    let new_start = floor.div_ceil(align) * align;
    let ceiling = after.iter().map(|e| e.first_lba - 1).min().unwrap_or(t.last_usable);
    let new_root_last = ((ceiling + 1) / align) * align - 1;
    let shift = boot.first_lba.saturating_sub(new_start);
    if shift * sector < CHUNK {
        no.push(format!("the space freed in front of Linux ({} MiB) is less than one move chunk ({} MiB): too little to move into safely", shift * sector / MIB, CHUNK / MIB));
    }
    if shift % align != 0 {
        no.push("Linux's partitions do not start on a 1 MiB boundary; the move would leave them misaligned".into());
    }
    if !no.is_empty() {
        return json!({ "result": "refused", "why": no });
    }

    // --- the plan
    let mut new = t.clone();
    new.entries.retain(|e| !delete.iter().any(|d| d.index == e.index));
    for e in new.entries.iter_mut() {
        if e.index == boot.index || e.index == root.index {
            e.first_lba -= shift;
            e.last_lba -= shift;
        }
        if e.index == root.index {
            e.last_lba = new_root_last;
        }
    }
    if let Err(e) = new.check() {
        return json!({ "result": "refused", "why": [format!("the new table would not be whole ({})", e)] });
    }
    let folders: Vec<Value> = job.pointer("/harvest/folders").and_then(Value::as_array).into_iter().flatten()
        .filter(|x| x.get("exists") == Some(&json!(true)))
        .map(|x| json!({ "name": x["name"], "files": x["files"], "bytes": x["bytes"] }))
        .collect();
    let deleted_bytes: u64 = delete.iter().map(|e| e.sectors() * sector).sum();
    json!({
        "result": "plan",
        "sentence": SENTENCE,
        "folders_lost": folders,
        "delete": delete.iter().map(|e| json!({ "number": e.number(), "type": ty(e), "bytes": e.sectors() * sector })).collect::<Vec<_>>(),
        "deleted_bytes": deleted_bytes,
        "move": {
            "from_lba": boot.first_lba,
            "to_lba": new_start,
            "sectors": root.last_lba - boot.first_lba + 1,
            "shift_sectors": shift,
            "sector": sector,
            "chunk_bytes": CHUNK,
        },
        "root_grows_to_bytes": (new_root_last - (root.first_lba - shift) + 1) * sector,
        "new_table": new.entries.iter().map(|e| json!({ "number": e.number(), "type": ty(e), "first_lba": e.first_lba, "last_lba": e.last_lba })).collect::<Vec<_>>(),
    })
}

/// Where the move's progress lives: off the disk being moved (the kit stick).
/// `save` must be durable before it returns.
pub trait Journal {
    fn load(&mut self) -> Result<Option<Value>, String>;
    fn save(&mut self, v: &Value) -> Result<(), String>;
}

/// One move, as the journal names it. A journal for another move is refused.
pub fn move_id(src: u64, len: u64, shift: u64, disk_guid: &[u8; 16]) -> String {
    format!("{}:{}:{}:{}", guid_text(disk_guid), src, len, shift)
}

fn hash_range<D: Read + Seek>(d: &mut D, at: u64, len: u64, chunk: u64) -> Result<String, String> {
    let mut h = Sha256::new();
    let mut buf = vec![0u8; chunk as usize];
    let mut pos = 0;
    d.seek(SeekFrom::Start(at)).map_err(|e| e.to_string())?;
    while pos < len {
        let n = chunk.min(len - pos) as usize;
        d.read_exact(&mut buf[..n]).map_err(|e| format!("read at byte {}: {}", at + pos, e))?;
        h.update(&buf[..n]);
        pos += n as u64;
    }
    Ok(format!("{:x}", h.finalize()))
}

/// Move `len` bytes at `src` down by `shift` bytes. Copies in ascending
/// order, `chunk` at a time, flushing the disk and then the journal after
/// each chunk.
///
/// Why a cut at any byte can be resumed: the chunk being copied when the
/// power went is copied again from the start. Its source is
/// `[src+p, src+p+chunk)`; every write so far reached at most
/// `src+p+chunk-shift`, which is at or below `src+p` because
/// `shift >= chunk`. So the source of the chunk being redone is untouched.
///
/// Before the first chunk the whole range is hashed into the journal; after
/// the last, the moved range is hashed again and must match.
pub fn move_down<D: Read + Write + Seek>(
    d: &mut D,
    id: &str,
    src: u64,
    len: u64,
    shift: u64,
    chunk: u64,
    j: &mut dyn Journal,
    sync: &mut dyn FnMut(&mut D) -> Result<(), String>,
) -> Result<Value, String> {
    if shift < chunk || chunk == 0 || shift > src {
        return Err(format!("a shift of {} bytes is less than a chunk ({}): not resumable, refused", shift, chunk));
    }
    let mut state = match j.load()? {
        Some(v) => {
            if v["id"] != json!(id) {
                return Err(format!("the journal is for another move ({}), refused", v["id"]));
            }
            v
        }
        None => {
            let before = hash_range(d, src, len, chunk)?;
            let v = json!({ "schema": "reclaim-move/1", "id": id, "sha256_before": before, "done": 0, "finished": false });
            j.save(&v)?;
            v
        }
    };
    if state["finished"] == json!(true) {
        return Ok(state);
    }
    let mut pos = state["done"].as_u64().ok_or("the journal's progress is not a number")?;
    if pos > len {
        return Err("the journal says more was moved than the move holds".into());
    }
    let mut buf = vec![0u8; chunk as usize];
    while pos < len {
        let n = chunk.min(len - pos) as usize;
        d.seek(SeekFrom::Start(src + pos)).map_err(|e| e.to_string())?;
        d.read_exact(&mut buf[..n]).map_err(|e| format!("read at byte {}: {}", src + pos, e))?;
        d.seek(SeekFrom::Start(src - shift + pos)).map_err(|e| e.to_string())?;
        d.write_all(&buf[..n]).map_err(|e| format!("write at byte {}: {}", src - shift + pos, e))?;
        sync(d)?;
        pos += n as u64;
        state["done"] = json!(pos);
        j.save(&state)?;
    }
    let after = hash_range(d, src - shift, len, chunk)?;
    if json!(after) != state["sha256_before"] {
        return Err("the moved data does not match what was there before the move: the drive returned something else".into());
    }
    state["sha256_after"] = json!(after);
    state["finished"] = json!(true);
    j.save(&state)?;
    Ok(state)
}

/// The live half: reads only. sysfs, the kernel's mount table, the disk's
/// own table, and the Windows partition where the caller mounted it
/// read-only. Every fact it cannot read becomes a refusal in `plan`.
pub mod live {
    use super::*;

    fn read(p: &str) -> Option<String> {
        std::fs::read_to_string(p).ok().map(|s| s.trim().to_string())
    }

    /// (major:minor, mount point, options) for every mount.
    fn mounts(r: &str) -> Vec<(String, String, String)> {
        read(&format!("{}/proc/self/mountinfo", r)).unwrap_or_default().lines().filter_map(|l| {
            let f: Vec<&str> = l.split(' ').collect();
            Some((f.get(2)?.to_string(), f.get(4)?.replace("\\040", " "), f.get(5)?.to_string()))
        }).collect()
    }

    /// major:minor -> (disk, partition number), from /sys/dev/block.
    fn part_of(r: &str, devnum: &str) -> Option<(String, u32)> {
        let link = std::fs::read_link(format!("{}/sys/dev/block/{}", r, devnum)).ok()?;
        let n: u32 = read(&format!("{}/sys/dev/block/{}/partition", r, devnum))?.parse().ok()?;
        let disk = link.parent()?.file_name()?.to_string_lossy().to_string();
        Some((disk, n))
    }

    fn devnum_of_partition(r: &str, disk: &str, number: u32) -> Option<String> {
        for e in std::fs::read_dir(format!("{}/sys/block/{}", r, disk)).ok()?.flatten() {
            let p = e.path();
            if read(&p.join("partition").to_string_lossy()).as_deref() == Some(&number.to_string()) {
                return read(&p.join("dev").to_string_lossy());
            }
        }
        None
    }

    fn mount_of(r: &str, at: &str) -> Option<(String, String)> {
        mounts(r).into_iter().rev().find(|(_, m, _)| m == at).map(|(d, _, o)| (d, o))
    }

    pub fn run(root: &str, windows_mount: Option<&str>) -> Value {
        let r = root.trim_end_matches('/');
        let handoff = format!("{}/{}", r, crate::HANDOFF);
        let job = crate::read_json(&format!("{}/job.json", handoff)).unwrap_or(Value::Null);
        let outcome = crate::read_json(&format!("{}/outcome.json", handoff)).unwrap_or(Value::Null);
        let rootm = mount_of(r, "/").and_then(|(d, _)| part_of(r, &d));
        let bootm = mount_of(r, "/boot").and_then(|(d, _)| part_of(r, &d));
        let disk = rootm.as_ref().map(|(d, _)| d.clone());
        let table = match &disk {
            None => Err("the disk Linux runs from is not known".to_string()),
            Some(d) => (|| {
                let sector: u64 = read(&format!("{}/sys/block/{}/queue/logical_block_size", r, d)).and_then(|s| s.parse().ok()).ok_or("its sector size is not readable")?;
                let size: u64 = read(&format!("{}/sys/block/{}/size", r, d)).and_then(|s| s.parse().ok()).ok_or("its size is not readable")?;
                let mut f = std::fs::File::open(format!("{}/dev/{}", r, d)).map_err(|e| format!("/dev/{}: {}", d, e))?;
                crate::gpttable::read(&mut f, sector, size * 512)
            })(),
        };
        // other internal disks that carry a Windows data partition
        let mut other = Vec::new();
        if let Ok(rd) = std::fs::read_dir(format!("{}/sys/block", r)) {
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().to_string();
                if Some(&n) == disk.as_ref() || ["loop", "ram", "zram", "dm-", "sr"].iter().any(|p| n.starts_with(p)) {
                    continue;
                }
                if read(&format!("{}/sys/block/{}/removable", r, n)).as_deref() == Some("1") {
                    continue;
                }
                let sector = read(&format!("{}/sys/block/{}/queue/logical_block_size", r, n)).and_then(|s| s.parse().ok()).unwrap_or(512);
                match std::fs::File::open(format!("{}/dev/{}", r, n)).map_err(|e| e.to_string()).and_then(|mut f| crate::gpt::read_disk(&n, &mut f, sector)) {
                    Ok(parts) if parts.iter().any(|p| guid_text(&p.type_guid) == MSDATA) => other.push(n),
                    Ok(_) => {}
                    Err(e) => other.push(format!("{} (unreadable: {})", n, e)),
                }
            }
        }
        // the Windows partition, where the caller mounted it, and only if it is that partition
        let win_guid = s(&outcome, "/windows/partition/guid").to_lowercase();
        let windows = match (windows_mount, &disk, &table) {
            (None, _, _) => Err("it is not mounted (mount it read-only and name it with --windows-mount)".to_string()),
            (Some(m), Some(d), Ok(t)) => {
                let want = t.entries.iter().find(|e| guid_text(&e.unique) == win_guid).and_then(|e| devnum_of_partition(r, d, e.number()));
                match mount_of(r, m.trim_end_matches('/')) {
                    None => Err(format!("{} is not a mount point", m)),
                    Some((dev, _)) if Some(&dev) != want.as_ref() => Err(format!("{} is not the Windows partition the install recorded", m)),
                    Some((_, opts)) if !opts.split(',').any(|o| o == "ro") => Err(format!("{} is mounted read-write; reclaim reads Windows only read-only", m)),
                    Some(_) => {
                        let base = format!("{}{}", r, m.trim_end_matches('/'));
                        let users = std::fs::read_dir(format!("{}/Users", base)).map(|rd| rd.flatten().filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().to_string()).collect::<Vec<_>>());
                        match users {
                            Err(e) => Err(format!("its Users folder: {}", e)),
                            Ok(mut users) => {
                                users.sort();
                                Ok(WindowsRead { has_system32: std::path::Path::new(&format!("{}/Windows/System32", base)).is_dir(), users })
                            }
                        }
                    }
                }
            }
            _ => Err("the disk is not readable".to_string()),
        };
        let rolled_back = std::path::Path::new(&format!("{}/boot/efi/upgrade_/rolled-back.json", r)).exists();
        let f = Facts {
            job: &job,
            outcome: &outcome,
            table,
            root_number: rootm.as_ref().filter(|(d, _)| Some(d) == disk.as_ref()).map(|(_, n)| *n),
            boot_number: bootm.as_ref().filter(|(d, _)| Some(d) == disk.as_ref()).map(|(_, n)| *n),
            other_disks_in_use: other,
            windows,
            rolled_back,
        };
        let mut p = plan(&f);
        p["writes"] = json!("none: this is the plan only (RISKS R40; the move is not wired to any command)");
        p
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpttable::tests::{entry, image, table};
    use std::io::Cursor;

    struct Mem(Option<Value>);
    impl Journal for Mem {
        fn load(&mut self) -> Result<Option<Value>, String> {
            Ok(self.0.clone())
        }
        fn save(&mut self, v: &Value) -> Result<(), String> {
            self.0 = Some(v.clone());
            Ok(())
        }
    }

    /// A disk that loses power after `left` more bytes are written: the
    /// write that crosses the line lands only in part.
    struct Cut {
        inner: Cursor<Vec<u8>>,
        left: Option<u64>,
    }
    impl Read for Cut {
        fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
            self.inner.read(b)
        }
    }
    impl Seek for Cut {
        fn seek(&mut self, p: SeekFrom) -> std::io::Result<u64> {
            self.inner.seek(p)
        }
    }
    impl Write for Cut {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            match self.left {
                Some(0) => Err(std::io::Error::other("power cut")),
                Some(l) if (b.len() as u64) > l => {
                    let n = l as usize;
                    self.inner.write_all(&b[..n])?;
                    self.left = Some(0);
                    Ok(n)
                }
                Some(l) => {
                    self.left = Some(l - b.len() as u64);
                    self.inner.write(b)
                }
                None => self.inner.write(b),
            }
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn pattern(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i * 31 % 251) as u8 ^ (i >> 10) as u8).collect()
    }

    #[test]
    fn a_move_cut_anywhere_resumes_to_the_same_bytes() {
        let chunk = 4096u64;
        let (src, len, shift) = (5 * chunk, 7 * chunk + 100, chunk); // the tightest shift allowed
        let mut disk = vec![0xEEu8; (src + len + chunk) as usize];
        let data = pattern(len as usize);
        disk[src as usize..(src + len) as usize].copy_from_slice(&data);
        let mut expect = disk.clone();
        expect[(src - shift) as usize..(src - shift + len) as usize].copy_from_slice(&data);
        // every cut point: each chunk boundary and the middle of each chunk
        let mut cuts: Vec<u64> = (0..=len / chunk + 1).flat_map(|k| [k * chunk, k * chunk + chunk / 2]).collect();
        cuts.push(1);
        for cut in cuts {
            let mut d = Cut { inner: Cursor::new(disk.clone()), left: Some(cut) };
            let mut j = Mem(None);
            let first = move_down(&mut d, "m", src, len, shift, chunk, &mut j, &mut |_| Ok(()));
            d.left = None; // power back
            let r = match first {
                Ok(v) => v,
                Err(_) => move_down(&mut d, "m", src, len, shift, chunk, &mut j, &mut |_| Ok(())).unwrap(),
            };
            assert_eq!(r["finished"], json!(true), "cut at {}", cut);
            let got = d.inner.into_inner();
            assert_eq!(&got[(src - shift) as usize..(src - shift + len) as usize], &data[..], "cut at {}", cut);
            assert_eq!(&got[..(src - shift) as usize], &expect[..(src - shift) as usize], "nothing below the target touched, cut at {}", cut);
        }
    }

    #[test]
    fn refuses_a_shift_under_a_chunk_and_a_strange_journal() {
        let mut d = Cursor::new(vec![0u8; 1 << 16]);
        assert!(move_down(&mut d, "m", 8192, 4096, 2048, 4096, &mut Mem(None), &mut |_| Ok(())).unwrap_err().contains("not resumable"));
        let mut j = Mem(Some(json!({ "id": "another", "done": 0 })));
        assert!(move_down(&mut d, "m", 8192, 4096, 4096, 4096, &mut j, &mut |_| Ok(())).unwrap_err().contains("another move"));
    }

    #[test]
    fn a_drive_that_returns_other_bytes_is_caught() {
        // a disk whose writes to one sector are silently lost
        struct Liar(Cursor<Vec<u8>>);
        impl Read for Liar {
            fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> { self.0.read(b) }
        }
        impl Seek for Liar {
            fn seek(&mut self, p: SeekFrom) -> std::io::Result<u64> { self.0.seek(p) }
        }
        impl Write for Liar {
            fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                if self.0.position() == 4096 { self.0.seek(SeekFrom::Current(b.len() as i64))?; return Ok(b.len()); }
                self.0.write(b)
            }
            fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
        }
        let mut v = vec![0u8; 1 << 16];
        v[8192..16384].copy_from_slice(&pattern(8192));
        let mut d = Liar(Cursor::new(v));
        let e = move_down(&mut d, "m", 8192, 8192, 4096, 4096, &mut Mem(None), &mut |_| Ok(())).unwrap_err();
        assert!(e.contains("does not match"), "{}", e);
    }

    // --- the plan, on made-up tables (sectors of 512 bytes; 1 MiB = 2048)

    const M: u64 = 2048;
    const WIN_GUID: u8 = 0x33;

    /// ESP, MSR, Windows (shrunk), /boot, /, WinRE: the converter's layout.
    fn layout() -> Vec<Entry> {
        vec![
            entry(0, ESP, 0x11, M, 101 * M - 1),
            entry(1, MSR, 0x22, 101 * M, 117 * M - 1),
            entry(2, MSDATA, WIN_GUID, 117 * M, 1117 * M - 1),
            entry(4, LINUX_DATA, 0x55, 1117 * M, 2141 * M - 1),
            entry(5, LINUX_DATA, 0x66, 2141 * M, 5000 * M - 1),
            entry(3, WINRE, 0x44, 5000 * M, 5700 * M - 1),
        ]
    }

    fn job() -> Value {
        json!({
            "schema": "job/1",
            "intent": { "path": "keep-windows", "account": { "windows_name": "Sam" } },
            "storage": { "physical_disk": { "health_status": "Healthy" } },
            "harvest": {
                "bitlocker": { "status": "off" },
                "folders": [
                    { "name": "Pictures", "path": "C:\\Users\\Sam\\Pictures", "exists": true, "files": 812, "bytes": 3_100_000_000u64 },
                    { "name": "Music", "path": null, "exists": false, "files": 0, "bytes": 0 },
                ],
            },
        })
    }

    fn outcome() -> Value {
        json!({ "schema": "outcome/1", "windows": { "kept": true, "partition": { "guid": guid_text(&[WIN_GUID; 16]), "size_bytes": 1000 * M * 512 } } })
    }

    fn facts<'a>(job: &'a Value, out: &'a Value, entries: Vec<Entry>) -> Facts<'a> {
        let t = table(5800 * M, entries);
        Facts {
            job,
            outcome: out,
            table: Ok(t),
            root_number: Some(6),
            boot_number: Some(5),
            other_disks_in_use: vec![],
            windows: Ok(WindowsRead { has_system32: true, users: vec!["Sam".into(), "Public".into(), "Default".into()] }),
            rolled_back: false,
        }
    }

    #[test]
    fn plans_the_converters_layout() {
        let (j, o) = (job(), outcome());
        let p = plan(&facts(&j, &o, layout()));
        assert_eq!(p["result"], "plan", "{}", p);
        assert_eq!(p["move"]["to_lba"], json!(101 * M)); // right after the ESP
        assert_eq!(p["move"]["shift_sectors"], json!(1016 * M));
        let dels: Vec<u64> = p["delete"].as_array().unwrap().iter().map(|d| d["number"].as_u64().unwrap()).collect();
        assert_eq!(dels, vec![2, 3, 4]); // MSR, Windows, WinRE
        let nt = p["new_table"].as_array().unwrap();
        assert_eq!(nt.len(), 3);
        let root = nt.iter().find(|e| e["number"] == 6).unwrap();
        // / now reaches the last whole MiB before the backup table
        assert!(root["last_lba"].as_u64().unwrap() > 5700 * M);
        assert_eq!(p["folders_lost"].as_array().unwrap().len(), 1);
        assert_eq!(p["sentence"], SENTENCE);
    }

    fn refused_for(p: &Value, needle: &str) -> bool {
        p["result"] == "refused" && p["why"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains(needle))
    }

    #[test]
    fn every_planted_fault_is_refused() {
        let (j, o) = (job(), outcome());
        // the recorded GUID is on no partition here
        let mut o2 = o.clone();
        o2["windows"]["partition"]["guid"] = json!(guid_text(&[0x99; 16]));
        assert!(refused_for(&plan(&facts(&j, &o2, layout())), "not on the disk Linux runs from"));
        // a second Windows data partition (a D: drive)
        let mut l = layout();
        l[5] = entry(3, MSDATA, 0x44, 5000 * M, 5700 * M - 1);
        assert!(refused_for(&plan(&facts(&j, &o, l)), "neither Windows' nor this Linux's"));
        // another Linux already on the disk (R39)
        let mut l = layout();
        l.push(entry(6, LINUX_DATA, 0x77, 5700 * M, 5750 * M - 1));
        assert!(refused_for(&plan(&facts(&j, &o, l)), "R39"));
        // other people's accounts
        let mut f = facts(&j, &o, layout());
        f.windows = Ok(WindowsRead { has_system32: true, users: vec!["Sam".into(), "Alex".into(), "Public".into()] });
        assert!(refused_for(&plan(&f), "Alex"));
        // not Windows at all
        let mut f = facts(&j, &o, layout());
        f.windows = Ok(WindowsRead { has_system32: false, users: vec!["Sam".into()] });
        assert!(refused_for(&plan(&f), "may not be Windows"));
        // unreadable
        let mut f = facts(&j, &o, layout());
        f.windows = Err("mount failed".into());
        assert!(refused_for(&plan(&f), "nothing is deleted unseen"));
        // BitLocker
        let mut j2 = j.clone();
        j2["harvest"]["bitlocker"]["status"] = json!("on");
        assert!(refused_for(&plan(&facts(&j2, &o, layout())), "BitLocker"));
        // the drive: not healthy, or the data-loss sentence was used
        let mut j2 = j.clone();
        j2["storage"]["physical_disk"]["health_status"] = json!("Warning");
        assert!(refused_for(&plan(&facts(&j2, &o, layout())), "healthy"));
        let mut j2 = j.clone();
        j2["risk_acknowledgement"] = json!({ "sentence": "I confirm that I understand the risks and could lose data" });
        assert!(refused_for(&plan(&facts(&j2, &o, layout())), "does not cover reclaim"));
        // Windows' size changed since the install
        let mut o2 = o.clone();
        o2["windows"]["partition"]["size_bytes"] = json!(5);
        assert!(refused_for(&plan(&facts(&j, &o2, layout())), "changed since"));
        // a second disk in use
        let mut f = facts(&j, &o, layout());
        f.other_disks_in_use = vec!["sdb".into()];
        assert!(refused_for(&plan(&f), "one disk only"));
        // a folder moved to another drive
        let mut j2 = j.clone();
        j2["harvest"]["folders"][0]["path"] = json!("D:\\Photos");
        assert!(refused_for(&plan(&facts(&j2, &o, layout())), "not on Windows' own drive"));
        // the rollback was taken
        let mut f = facts(&j, &o, layout());
        f.rolled_back = true;
        assert!(refused_for(&plan(&f), "rollback"));
        // an erase-path job
        let mut j2 = j.clone();
        j2["intent"]["path"] = json!("clean-slate");
        assert!(refused_for(&plan(&facts(&j2, &o, layout())), "nothing to reclaim"));
        // a damaged table
        let mut f = facts(&j, &o, layout());
        f.table = Err("the backup table's header fails its check (CRC)".into());
        assert!(refused_for(&plan(&f), "not whole"));
        // too little room to move safely: Windows of 32 MiB
        let l = vec![
            entry(0, ESP, 0x11, M, 101 * M - 1),
            entry(2, MSDATA, WIN_GUID, 101 * M, 133 * M - 1),
            entry(4, LINUX_DATA, 0x55, 133 * M, 1157 * M - 1),
            entry(5, LINUX_DATA, 0x66, 1157 * M, 5000 * M - 1),
        ];
        let mut o2 = o.clone();
        o2["windows"]["partition"]["size_bytes"] = json!(32 * M * 512);
        assert!(refused_for(&plan(&facts(&j, &o2, l)), "less than one move chunk"));
    }

    /// The move and the new table together on a small image: Windows gone,
    /// Linux's two partitions moved down, / grown to the end, and both hold
    /// exactly what they held, under their own ids.
    #[test]
    fn move_then_table_on_an_image() {
        let sec = 512u64;
        let (mut img, t) = image(4096, vec![
            entry(0, ESP, 0x11, 64, 127),
            entry(2, MSDATA, WIN_GUID, 128, 1151),
            entry(4, LINUX_DATA, 0x55, 1152, 1663),
            entry(5, LINUX_DATA, 0x66, 1664, 3999),
        ]);
        let data = pattern(((3999 - 1152 + 1) * sec) as usize);
        img.seek(SeekFrom::Start(1152 * sec)).unwrap();
        img.write_all(&data).unwrap();
        let shift = (1152 - 128) * sec;
        let id = move_id(1152 * sec, data.len() as u64, shift, &t.disk_guid);
        let r = move_down(&mut img, &id, 1152 * sec, data.len() as u64, shift, 64 * 1024, &mut Mem(None), &mut |_| Ok(())).unwrap();
        assert_eq!(r["finished"], json!(true));
        let mut nt = t.clone();
        nt.entries.retain(|e| e.index != 2);
        for e in nt.entries.iter_mut().filter(|e| e.index >= 4) {
            e.first_lba -= 1024;
            e.last_lba -= 1024;
        }
        nt.entries.iter_mut().find(|e| e.index == 5).unwrap().last_lba = nt.last_usable;
        crate::gpttable::write(&mut img, &nt, &mut |_| Ok(())).unwrap();
        let back = crate::gpttable::read(&mut img, sec, 4096 * sec).unwrap();
        let boot = back.entries.iter().find(|e| e.index == 4).unwrap();
        assert_eq!(boot.first_lba, 128);
        assert_eq!(boot.unique, [0x55; 16]); // the partition keeps its own id
        let mut got = vec![0u8; data.len()];
        img.seek(SeekFrom::Start(128 * sec)).unwrap();
        img.read_exact(&mut got).unwrap();
        assert_eq!(got, data);
    }
}
