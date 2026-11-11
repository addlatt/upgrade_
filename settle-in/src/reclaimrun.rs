//! Reclaim's writing half (RISKS R41, VALIDATION V15): the arm in Linux and
//! the run from the kit stick. `reclaim.rs` holds the plan and the move.
//!
//! Decided (2026-11-02, the owner): everything on Windows goes after a typed
//! sentence, and Linux is moved to the front of the freed space. The commit
//! line is the window's last click ("Restart now"), rule #3.
//!
//! Why from the stick: Linux's `/` cannot be moved while it runs. So the
//! work is split in two, and every refusal comes before the first write to
//! the internal disk:
//!
//!   1. `arm` (Linux, root): the plan again, Windows read read-only, the
//!      sentence checked exactly; then an order onto the kit stick
//!      (`upgrade_/reclaim/order.json`), the marker the stick's GRUB reads
//!      (`upgrade_/boot-reclaim`), and a one-time start from the stick. It
//!      writes nothing on the internal disk. `disarm` takes it all back.
//!   2. `onstick` (the kit stick's installer boot, `upg.mode=reclaim`, a
//!      kickstart with no install in it): finds the disk by its GPT id,
//!      checks that the table is the one the order names, that nothing on
//!      the disk is mounted, that both file systems are ext4 and clean
//!      (`e2fsck -n`), then commits. A refusal here takes the marker and the
//!      one-time entry back and starts Linux again, untouched.
//!
//! After the commit, in order, each step recorded in a journal on the stick
//! (two alternating files, so a cut write leaves the one before it):
//!
//!   - guard: every `EFI/*/grub.cfg` on the ESP is replaced by a stub that
//!     will not start Linux (the originals saved on the stick first), and the
//!     stick goes first in the firmware's order. A half-moved Linux is never
//!     started: a power cut starts the stick again, which carries on;
//!   - move: `/boot` and `/` down by the freed space (`reclaim::move_down`,
//!     resumable at any byte);
//!   - table: the new partition table (Windows, MSR and WinRE gone, `/`
//!     reaching the end), backup first;
//!   - grow: `e2fsck -f -y` and `resize2fs` on the moved `/`, then `e2fsck -n`
//!     on both;
//!   - finish: the ESP's `grub.cfg` files put back; Windows' boot files and
//!     firmware entries removed only where Linux does not start through them
//!     (RISKS R21, R22: on the Acer Aspire, shim sits in Windows' own slot,
//!     and deleting it would leave nothing that starts); GRUB's 'Windows
//!     (kept)' entry removed; the firmware's order as it was before, without
//!     the stick and without Windows; `upgrade_/reclaimed.json` on the ESP.
//!
//! The clock is left as it is (local time, R36; switching it to UTC is a
//! decision owed). The judging halves are pure and tested here; the run is
//! tested end to end on a disk image, cut at every step.

use crate::gpt::guid_text;
use crate::gpttable::{Entry, Table};
use crate::reclaim::{self, Journal, CHUNK};
use serde_json::{json, Value};
use std::io::{Read, Seek, Write};

pub const VERSION: &str = "0.1.0";
/// On the stick.
pub const ORDER_REL: &str = "upgrade_/reclaim/order.json";
pub const MARKER_REL: &str = "upgrade_/boot-reclaim";
pub const JOURNAL_DIR_REL: &str = "upgrade_/reclaim";
pub const SAVED_REL: &str = "upgrade_/reclaim/saved";
pub const RECORD_REL: &str = "upgrade_/reclaim/record.json";
pub const KS_REL: &str = "upgrade_/ks-reclaim.cfg";
/// On the ESP: reclaim has run on this computer.
pub const ESP_DONE_REL: &str = "upgrade_/reclaimed.json";
/// The firmware entry the arm makes for the stick.
pub const ENTRY_LABEL: &str = "upgrade_ reclaim";
/// In the installed Linux, the file the installer wrote (`post.rs`).
pub const KEPT_GRUB_D: &str = "etc/grub.d/25_windows_kept";
/// The first line of the ESP's stub `grub.cfg`, by which it is known again.
pub const STUB_FIRST: &str = "# upgrade_ reclaim in progress (RISKS R41): Linux must not start until the move has finished.";

pub fn stub_grub_cfg() -> String {
    format!(
        "{STUB_FIRST}\n\
         echo \"upgrade_: Windows' space is being given to Linux, and that has not finished.\"\n\
         echo \"Put the upgrade_ USB stick back in. The computer restarts in 30 seconds and carries on from the stick.\"\n\
         sleep 30\n\
         reboot\n"
    )
}

pub fn is_stub(text: &str) -> bool {
    text.lines().next() == Some(STUB_FIRST)
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{:02x}", x)).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

/// A table, whole, as JSON (the order carries the one the arm read).
pub fn table_json(t: &Table) -> Value {
    json!({
        "sector": t.sector, "disk_sectors": t.disk_sectors, "first_usable": t.first_usable, "last_usable": t.last_usable,
        "disk_guid": hex(&t.disk_guid), "entry_count": t.entry_count, "entry_size": t.entry_size,
        "entries": t.entries.iter().map(|e| json!({
            "index": e.index, "type": hex(&e.type_guid), "unique": hex(&e.unique),
            "first_lba": e.first_lba, "last_lba": e.last_lba, "attributes": e.attributes, "name": hex(&e.name),
        })).collect::<Vec<_>>(),
    })
}

pub fn table_from_json(v: &Value) -> Result<Table, String> {
    let n = |k: &str| v[k].as_u64().ok_or_else(|| format!("the order's table has no {}", k));
    let arr = |s: &Value, len: usize| -> Result<Vec<u8>, String> {
        let b = s.as_str().and_then(unhex).ok_or("the order's table is not readable")?;
        if b.len() != len { Err("the order's table is not readable".to_string()) } else { Ok(b) }
    };
    let mut disk_guid = [0u8; 16];
    disk_guid.copy_from_slice(&arr(&v["disk_guid"], 16)?);
    let mut entries = Vec::new();
    for e in v["entries"].as_array().ok_or("the order's table has no partitions")? {
        let mut type_guid = [0u8; 16];
        type_guid.copy_from_slice(&arr(&e["type"], 16)?);
        let mut unique = [0u8; 16];
        unique.copy_from_slice(&arr(&e["unique"], 16)?);
        let mut name = [0u8; 72];
        name.copy_from_slice(&arr(&e["name"], 72)?);
        let g = |k: &str| e[k].as_u64().ok_or_else(|| format!("a partition in the order has no {}", k));
        entries.push(Entry { index: g("index")? as usize, type_guid, unique, first_lba: g("first_lba")?, last_lba: g("last_lba")?, attributes: g("attributes")?, name });
    }
    let t = Table {
        sector: n("sector")?, disk_sectors: n("disk_sectors")?, first_usable: n("first_usable")?, last_usable: n("last_usable")?,
        disk_guid, entry_count: n("entry_count")? as u32, entry_size: n("entry_size")? as u32, entries,
    };
    t.check()?;
    Ok(t)
}

/// The table after the move, from the table before it and the plan's
/// `new_table` (every partition kept keeps its slot, id and name).
pub fn new_table(before: &Table, plan: &Value) -> Result<Table, String> {
    let rows = plan["new_table"].as_array().ok_or("the plan has no new table")?;
    let mut t = before.clone();
    t.entries.retain(|e| rows.iter().any(|r| r["number"].as_u64() == Some(e.number() as u64)));
    for e in t.entries.iter_mut() {
        let r = rows.iter().find(|r| r["number"].as_u64() == Some(e.number() as u64)).ok_or("the plan's table is not whole")?;
        if r["type"].as_str() != Some(guid_text(&e.type_guid).as_str()) {
            return Err(format!("the plan's partition {} is not the same kind as the disk's", e.number()));
        }
        e.first_lba = r["first_lba"].as_u64().ok_or("the plan's table is not whole")?;
        e.last_lba = r["last_lba"].as_u64().ok_or("the plan's table is not whole")?;
    }
    if t.entries.len() != rows.len() {
        return Err("the plan names a partition that is not on the disk".into());
    }
    t.check()?;
    Ok(t)
}

/// Where a partition sits after the move, in bytes: (offset, length).
pub fn placed(plan: &Value, sector: u64, number: u64) -> Option<(u64, u64)> {
    let r = plan["new_table"].as_array()?.iter().find(|r| r["number"].as_u64() == Some(number))?;
    let (f, l) = (r["first_lba"].as_u64()?, r["last_lba"].as_u64()?);
    Some((f * sector, (l - f + 1) * sector))
}

/// Where a partition sits before the move, in bytes.
pub fn placed_before(t: &Table, number: u64) -> Option<(u64, u64)> {
    let e = t.entries.iter().find(|e| e.number() as u64 == number)?;
    Some((e.first_lba * t.sector, e.sectors() * t.sector))
}

/// The order the arm writes onto the stick. Pure.
pub fn order(job_id: &str, typed: &str, plan: &Value, facts: &Value, boot: &Value, at_utc: &str) -> Value {
    let t = &facts["table"];
    json!({
        "schema": "reclaim-order/1",
        "program": format!("settle-in {}", env!("CARGO_PKG_VERSION")),
        "job_id": job_id,
        "at_utc": at_utc,
        "sentence": reclaim::SENTENCE,
        "typed": typed,
        "disk_guid": t["disk_guid"],
        "table_before": t,
        "root_number": facts["root_number"],
        "boot_number": facts["boot_number"],
        "windows": facts["windows"],
        "job": facts["job"],
        "outcome": facts["outcome"],
        "plan": plan,
        "boot": boot,
    })
}

/// The id of the move an order names (the journal is refused for any other).
pub fn order_move_id(o: &Value) -> Result<String, String> {
    let t = table_from_json(&o["table_before"])?;
    let m = &o["plan"]["move"];
    let sector = t.sector;
    let g = |k: &str| m[k].as_u64().ok_or("the order's move is not whole");
    Ok(reclaim::move_id(g("from_lba")? * sector, g("sectors")? * sector, g("shift_sectors")? * sector, &t.disk_guid))
}

/// What the stick's run reads before it commits.
pub struct OnStick {
    /// The internal disk's table now (found by the order's disk id).
    pub table: Result<Table, String>,
    /// Other internal disks with a Windows drive on them, as on Linux.
    pub other_disks_in_use: Vec<String>,
    /// Partitions of the disk that something has mounted, or uses as swap.
    pub busy: Vec<String>,
    /// The ESP, read-only: the rollback's and reclaim's own markers, and the
    /// `EFI/*/grub.cfg` files the guard will stub.
    pub rolled_back: bool,
    pub reclaimed: bool,
    pub grub_cfgs: Vec<String>,
    /// The ext4 magic in each file system's superblock, before the move.
    pub boot_is_ext4: bool,
    pub root_is_ext4: bool,
    /// The firmware's BootOrder could be read, and the entry the arm made
    /// for the stick is still there (the guard puts it first).
    pub boot_order_readable: bool,
    pub stick_entry_present: bool,
    /// A battery and no charger: a move of hours is not started on it.
    pub on_battery: bool,
}

/// Every reason the stick's run refuses before it writes. Pure. The plan is
/// made again from the order's job, outcome and Windows read, with the
/// table and the disks as the stick sees them, and must come out the same.
pub fn precheck(o: &Value, s: &OnStick) -> Vec<String> {
    let mut no = Vec::new();
    if o["schema"] != json!("reclaim-order/1") {
        return vec!["the order on the stick is not readable".into()];
    }
    if o["sentence"].as_str() != Some(reclaim::SENTENCE) || o["typed"].as_str() != Some(reclaim::SENTENCE) {
        no.push("the order does not carry the sentence, typed exactly".into());
    }
    let before = match table_from_json(&o["table_before"]) {
        Ok(t) => t,
        Err(e) => return vec![e],
    };
    let now = match &s.table {
        Ok(t) => t,
        Err(e) => {
            no.push(format!("the disk's partition table is not whole ({})", e));
            return no;
        }
    };
    if now != &before {
        no.push("the disk's partition table has changed since the reclaim was asked for".into());
    }
    if !s.busy.is_empty() {
        no.push(format!("something on the disk is in use ({}): nothing is moved under it", s.busy.join(", ")));
    }
    if s.grub_cfgs.is_empty() {
        no.push("no GRUB start file was found on the EFI system partition, so a half-moved Linux could not be kept from starting".into());
    }
    if !s.boot_order_readable {
        no.push("the firmware's start order cannot be read, so the stick cannot be put first while Linux moves".into());
    } else if !s.stick_entry_present {
        no.push("the firmware entry for the USB stick is gone, so a power cut could not come back to the stick".into());
    }
    if s.on_battery {
        no.push("the computer is running on its battery: plug in the charger, then start again".into());
    }
    if !s.boot_is_ext4 || !s.root_is_ext4 {
        no.push("Linux's /boot and / are not both ext4: only ext4 is grown by this program".into());
    }
    let windows = match o["windows"]["has_system32"].as_bool() {
        Some(h) => Ok(reclaim::WindowsRead { has_system32: h, users: o["windows"]["users"].as_array().into_iter().flatten().filter_map(|u| u.as_str().map(str::to_string)).collect() }),
        None => Err("it was not read when the reclaim was asked for".to_string()),
    };
    let f = reclaim::Facts {
        job: &o["job"],
        outcome: &o["outcome"],
        table: Ok(now.clone()),
        root_number: o["root_number"].as_u64().map(|n| n as u32),
        boot_number: o["boot_number"].as_u64().map(|n| n as u32),
        other_disks_in_use: s.other_disks_in_use.clone(),
        windows,
        rolled_back: s.rolled_back,
        reclaimed: s.reclaimed,
    };
    let p = reclaim::plan(&f);
    if p["result"] != "plan" {
        no.extend(p["why"].as_array().into_iter().flatten().filter_map(|w| w.as_str().map(str::to_string)));
    } else if ["move", "new_table", "delete"].iter().any(|k| p[*k] != o["plan"][*k]) {
        no.push("the plan made again on the stick is not the plan that was agreed to".into());
    }
    no
}

/// Two alternating files in a folder on the stick, each with its sequence
/// number and its own checksum: a write cut half way leaves the one before
/// it whole, and `load` takes the newest whole one.
pub struct FileJournal {
    pub dir: String,
}

fn journal_body(text: &str) -> Option<(u64, Value)> {
    let v: Value = serde_json::from_str(text).ok()?;
    let body = v.get("body")?;
    let want = v["sha256"].as_str()?;
    let got = sha2::Sha256::digest(serde_json::to_string(body).ok()?.as_bytes());
    if hex(&got) != want {
        return None;
    }
    Some((v["seq"].as_u64()?, body.clone()))
}

use sha2::Digest;

impl FileJournal {
    fn slots(&self) -> [String; 2] {
        [format!("{}/journal-a.json", self.dir), format!("{}/journal-b.json", self.dir)]
    }
    fn newest(&self) -> Option<(usize, u64, Value)> {
        let mut best: Option<(usize, u64, Value)> = None;
        for (i, p) in self.slots().iter().enumerate() {
            if let Some((seq, body)) = std::fs::read_to_string(p).ok().as_deref().and_then(journal_body)
                && best.as_ref().map(|b| seq > b.1).unwrap_or(true) {
                    best = Some((i, seq, body));
                }
        }
        best
    }
    /// A slot exists (a stray `.part` alone means the first save was cut).
    pub fn exists(&self) -> bool {
        self.slots().iter().any(|p| std::path::Path::new(p).exists())
    }
}

/// Write a file and make it durable: the data, then the folder's entry.
pub fn write_durable(path: &str, bytes: &[u8]) -> Result<(), String> {
    let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).open(path).map_err(|e| format!("{}: {}", path, e))?;
    f.write_all(bytes).and_then(|_| f.sync_all()).map_err(|e| format!("{}: {}", path, e))?;
    if let Some(d) = std::path::Path::new(path).parent()
        && let Ok(df) = std::fs::File::open(d) {
            let _ = df.sync_all();
        }
    Ok(())
}

impl Journal for FileJournal {
    fn load(&mut self) -> Result<Option<Value>, String> {
        if self.exists() && self.newest().is_none() {
            return Err("the reclaim's journal on the stick is damaged (neither copy is whole)".into());
        }
        Ok(self.newest().map(|b| b.2))
    }
    fn save(&mut self, v: &Value) -> Result<(), String> {
        std::fs::create_dir_all(&self.dir).map_err(|e| format!("{}: {}", self.dir, e))?;
        let (slot, seq) = match self.newest() {
            Some((i, seq, _)) => (1 - i, seq + 1),
            None => (0, 1),
        };
        let body = serde_json::to_string(v).map_err(|e| e.to_string())?;
        let rec = json!({ "seq": seq, "sha256": hex(&sha2::Sha256::digest(body.as_bytes())), "body": v });
        // a cut leaves at most a .part file, never a broken slot
        let part = format!("{}.part", self.slots()[slot]);
        write_durable(&part, (rec.to_string() + "\n").as_bytes())?;
        std::fs::rename(&part, &self.slots()[slot]).map_err(|e| format!("{}: {}", self.slots()[slot], e))?;
        if let Ok(df) = std::fs::File::open(&self.dir) {
            let _ = df.sync_all();
        }
        Ok(())
    }
}

impl FileJournal {
    /// After an abort before anything moved: the run is as if never started.
    pub fn clear(&self) -> Result<(), String> {
        for p in self.slots() {
            for q in [p.clone(), format!("{}.part", p)] {
                if std::path::Path::new(&q).exists() {
                    std::fs::remove_file(&q).map_err(|e| format!("{}: {}", q, e))?;
                }
            }
        }
        Ok(())
    }
}

/// The move's own journal, kept inside the run's state as `move`.
struct Nested<'a> {
    j: &'a mut dyn Journal,
    st: &'a mut Value,
}

impl Journal for Nested<'_> {
    fn load(&mut self) -> Result<Option<Value>, String> {
        Ok(Some(self.st["move"].clone()).filter(|v| !v.is_null()))
    }
    fn save(&mut self, v: &Value) -> Result<(), String> {
        self.st["move"] = v.clone();
        self.j.save(self.st)
    }
}

/// The steps that touch something other than the disk's bytes: the ESP,
/// the firmware, the file systems' own tools. Each must be safe to run
/// again after a cut (the run repeats the step it was in).
pub trait Ops {
    /// Put the stick first (read back), decide once what of Windows'
    /// startup may go, hold Windows' own loader aside, stub the ESP's
    /// grub.cfg files. Returns the decision, which the finish follows.
    fn guard(&mut self, o: &Value) -> Result<Value, String>;
    /// `e2fsck -f -n` on the bytes at `offset`, `len` long.
    fn check(&mut self, offset: u64, len: u64) -> Result<Value, String>;
    /// `e2fsck -f -y`, `resize2fs`, `e2fsck -f -n`.
    fn grow(&mut self, offset: u64, len: u64) -> Result<Value, String>;
    /// Put the ESP and the firmware back, without Windows, following the
    /// guard's decision (never deciding again).
    fn finish(&mut self, o: &Value, guard: &Value) -> Result<Value, String>;
    fn say(&mut self, _line: &str) {}
}

/// The state a run starts from once every check has passed: the commit.
pub fn committed(o: &Value, at_utc: &str) -> Result<Value, String> {
    Ok(json!({ "schema": "reclaim-run/1", "id": order_move_id(o)?, "job_id": o["job_id"], "phase": "committed", "committed_utc": at_utc }))
}

/// Everything after the commit, from wherever the journal says it stopped.
pub fn execute<D: Read + Write + Seek>(
    d: &mut D,
    o: &Value,
    j: &mut dyn Journal,
    ops: &mut dyn Ops,
    chunk: u64,
    sync: &mut dyn FnMut(&mut D) -> Result<(), String>,
) -> Result<Value, String> {
    let mut st = j.load()?.ok_or("there is no journal: the run was never committed")?;
    let id = order_move_id(o)?;
    if st["id"] != json!(id) {
        return Err(format!("the journal on the stick is for another reclaim ({})", st["id"]));
    }
    let before = table_from_json(&o["table_before"])?;
    let plan = &o["plan"];
    let sector = before.sector;
    let m = &plan["move"];
    let g = |k: &str| m[k].as_u64().ok_or("the order's move is not whole");
    let (src, len, shift) = (g("from_lba")? * sector, g("sectors")? * sector, g("shift_sectors")? * sector);
    let root_n = o["root_number"].as_u64().ok_or("the order does not name /")?;
    let boot_n = o["boot_number"].as_u64().ok_or("the order does not name /boot")?;
    let after = new_table(&before, plan)?;
    loop {
        let phase = st["phase"].as_str().unwrap_or("").to_string();
        // the disk must be the one this run left, at every step
        let now = crate::gpttable::read(d, sector, before.disk_sectors * sector);
        table_fits(&phase, &now, &before, &after)?;
        match phase.as_str() {
            "committed" => {
                ops.say("Keeping Linux from starting until this has finished.");
                st["guard"] = ops.guard(o)?;
                st["phase"] = json!("guarded");
            }
            "guarded" => {
                ops.say(&format!("Moving Linux ({} GB). This takes a while; leave the computer on.", len / 1_000_000_000));
                let r = reclaim::move_down(d, &id, src, len, shift, chunk, &mut Nested { j, st: &mut st }, sync)?;
                st["move"] = r;
                st["phase"] = json!("moved");
            }
            "moved" => {
                ops.say("Writing the new partition table.");
                let nt = new_table(&before, plan)?;
                crate::gpttable::write(d, &nt, sync)?;
                st["phase"] = json!("table");
            }
            "table" => {
                ops.say("Growing Linux into the space.");
                let (ro, rl) = placed(plan, sector, root_n).ok_or("the plan does not place /")?;
                let (bo, bl) = placed(plan, sector, boot_n).ok_or("the plan does not place /boot")?;
                st["grow"] = json!({ "root": ops.grow(ro, rl)?, "boot": ops.check(bo, bl)? });
                st["phase"] = json!("grown");
            }
            "grown" => {
                ops.say("Putting the startup back, without Windows.");
                let g = st["guard"].clone();
                if g["cleanup"].is_null() {
                    return Err("the journal does not hold the guard's decision about Windows' startup".into());
                }
                st["finish"] = ops.finish(o, &g)?;
                st["phase"] = json!("finished");
            }
            "finished" => return Ok(st),
            other => return Err(format!("the journal names a step this program does not know ({})", other)),
        }
        j.save(&st)?;
    }
}

/// Is the disk's table what a run at `phase` left? Before the table step it
/// is the table the order names; while it is being written, either table or
/// a torn one (one copy whole); after it, the new one. Pure.
pub fn table_fits(phase: &str, now: &Result<Table, String>, before: &Table, after: &Table) -> Result<(), String> {
    let ok = match phase {
        "committed" | "guarded" => now.as_ref().is_ok_and(|t| t == before),
        "moved" => now.as_ref().map(|t| t == before || t == after).unwrap_or(true),
        "table" | "grown" | "finished" => now.as_ref().is_ok_and(|t| t == after),
        _ => true,
    };
    if ok {
        Ok(())
    } else {
        Err(format!("the disk's partition table is not what this reclaim left at its last step ({}): something else changed it, and nothing more is written", phase))
    }
}

// ---------------------------------------------------------------- what is left of Windows

/// What the ESP holds when the guard looks, before it changes anything:
/// sha256 of Windows' slot and of the kept loader (absent: None), and the
/// slot's sum in the ESP snapshot `%pre` took before the install
/// (`upgrade_/esp-snapshot/SHA256SUMS` on the stick): Windows' own loader.
#[derive(Debug, Clone, Default)]
pub struct EspLook {
    pub slot: Option<String>,
    pub kept: Option<String>,
    pub snapshot_slot: Option<String>,
}

/// What of Windows' startup may go. Pure; the guard decides it once, before
/// any change, and journals it; the finish only follows it.
///
/// RISKS R21/R22: on some firmware (the Acer Aspire) shim sits in Windows'
/// slot and Linux starts only through `EFI/Microsoft/Boot/bootmgfw.efi` and
/// the Windows Boot Manager entry. So the slot counts as Windows' own only
/// when it is, byte for byte, the loader the snapshot recorded before the
/// install, and nothing sits beside it as the kept loader. Anything else
/// keeps the slot, its folder and its entry; only the kept loader goes.
/// Windows' folder and entries go only when the slot is Windows' own and
/// this Linux was started from an entry of its own (`BootCurrent`, read by
/// the arm). While the run moves Linux, Windows' own loader is held aside
/// and its entries are out of the order, so Windows cannot start and
/// repair a disk that is being rewritten.
pub fn esp_cleanup(l: &EspLook, started_from_own_entry: bool) -> Value {
    let windows_own = l.kept.is_none() && l.slot.is_some() && l.slot == l.snapshot_slot;
    if !windows_own {
        return json!({ "windows_own_slot": false, "hold_slot": false,
            "remove_kept_loader": l.kept.is_some() && l.slot.is_some(), "remove_microsoft_folder": false, "remove_windows_entries": false,
            "why": "Windows' slot is not known to hold Windows' own loader (shim may start Linux through it, RISKS R21): the slot and its entry stay" });
    }
    if !started_from_own_entry {
        return json!({ "windows_own_slot": true, "hold_slot": true,
            "remove_kept_loader": false, "remove_microsoft_folder": false, "remove_windows_entries": false,
            "why": "this Linux was not started from a firmware entry of its own: Windows' startup is held aside while Linux moves, then put back" });
    }
    json!({ "windows_own_slot": true, "hold_slot": true,
        "remove_kept_loader": false, "remove_microsoft_folder": true, "remove_windows_entries": true,
        "why": "Windows' own loader, and Linux starts from an entry of its own" })
}

/// Where Windows' own loader is held while Linux moves.
pub const HELD_REL: &str = "EFI/Microsoft/Boot/bootmgfw.efi.upgrade_-held";

/// The firmware's order while Linux moves: the stick first, Windows' entries
/// out when the slot is Windows' own. Pure.
pub fn guard_boot_order(now: &[u16], stick: u16, windows: &[u16], windows_out: bool) -> Vec<u16> {
    let mut v = vec![stick];
    v.extend(now.iter().copied().filter(|n| *n != stick && !(windows_out && windows.contains(n))));
    v
}

/// The firmware's order after the finish: the order as it was before the
/// guard, without the stick's entries and without the removed ones. Pure.
pub fn final_boot_order(saved: &[u16], drop: &[u16]) -> Vec<u16> {
    saved.iter().copied().filter(|n| !drop.contains(n)).collect()
}

/// grub.cfg without the block `grub2-mkconfig` copied from 25_windows_kept.
/// None when the block is not there. Pure.
pub fn without_kept_block(cfg: &str) -> Option<String> {
    let begin = "### BEGIN /etc/grub.d/25_windows_kept ###";
    let end = "### END /etc/grub.d/25_windows_kept ###";
    let b = cfg.find(begin)?;
    let e = cfg[b..].find(end)? + b + end.len();
    let e = if cfg[e..].starts_with('\n') { e + 1 } else { e };
    Some(format!("{}{}", &cfg[..b], &cfg[e..]))
}

/// The screen's words (DRAFT: the owner approves them; ROADMAP, decisions
/// owed). Pure. The plan names the folders that are lost.
pub fn words(plan: &Value) -> Value {
    let gb = |b: &Value| b.as_u64().unwrap_or(0) as f64 / 1e9;
    let folders: Vec<String> = plan["folders_lost"].as_array().into_iter().flatten()
        .map(|f| format!("{}: {} files, {:.1} GB", f["name"].as_str().unwrap_or("?"), f["files"].as_u64().unwrap_or(0), gb(&f["bytes"])))
        .collect();
    json!({
        "title": "Free up Windows' space",
        "intro": {
            "heading": "Delete Windows and give its space to Linux",
            "lines": [
                "Windows is still on this computer, beside Linux. Deleting it gives Linux its space.",
                "Everything on Windows is deleted for good: the programs, the settings, and every file in it. Your files were not copied into Linux.",
                "Copy anything you want to keep onto another drive first. Windows' files can be opened from Linux's file manager.",
                "The next step checks this computer. It asks for your password and changes nothing.",
            ],
        },
        "what": {
            "heading": format!("Linux gets {:.0} GB more room.", gb(&plan["deleted_bytes"])),
            "folders_heading": "These Windows folders, and everything else on Windows, will be deleted:",
            "folders": folders,
        },
        "consent": {
            "heading": "Type this sentence exactly to go on:",
            "sentence": reclaim::SENTENCE,
            "button": "Next",
            "note": "Nothing is deleted yet. The next page has the last button.",
        },
        "stick": "The upgrade_ USB stick this computer was converted with must be plugged in. The work runs from it.",
        "preparing": "Checking this computer...",
        "restarting": "The computer is restarting from the USB stick. If it has not restarted in a minute, you can still take it back.",
        "ready": {
            "heading": "This is the last step.",
            "lines": [
                "When you click Restart now, the computer checks everything again, restarts from the USB stick, checks once more, and then deletes Windows. There is no turning back after the click.",
                "Then it moves Linux into the space. This can take an hour or more. Leave the computer plugged in and the USB stick in. If the power goes, start it again with the stick in: it carries on.",
                "When it has finished, Linux starts by itself, with all the space.",
            ],
            "restart": "Restart now and delete Windows",
            "undo": "Take it back",
            "undone": "Nothing was changed. Windows is still on this computer.",
        },
        "refused": "Windows' space cannot be freed on this computer: ",
        "failed": "It stopped: ",
        "close": "Close",
        "next": "Next",
    })
}

// ---------------------------------------------------------------- the live halves

pub mod live {
    use super::*;
    use crate::reclaim::live::{disk_table, mount_of, other_windows_disks, part_of, read};
    use crate::rollback::live::{find_ci, find_stick, sha};
    use std::path::Path;
    use std::process::Command;

    const PATH_ENV: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

    pub(crate) fn run(prog: &str, args: &[&str]) -> Result<String, String> {
        let o = Command::new(prog).args(args).env("PATH", PATH_ENV).env("LC_ALL", "C").output().map_err(|e| format!("{} could not start ({})", prog, e))?;
        if o.status.success() {
            Ok(String::from_utf8_lossy(&o.stdout).to_string())
        } else {
            Err(format!("{} {} failed (exit {}): {}", prog, args.join(" "), o.status.code().unwrap_or(-1), String::from_utf8_lossy(&o.stderr).trim()))
        }
    }

    fn code(prog: &str, args: &[&str]) -> Result<(i32, String), String> {
        let o = Command::new(prog).args(args).env("PATH", PATH_ENV).env("LC_ALL", "C").output().map_err(|e| format!("{} could not start ({})", prog, e))?;
        Ok((o.status.code().unwrap_or(-1), format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))))
    }

    fn is_root() -> bool {
        // SAFETY: geteuid has no preconditions.
        unsafe { libc::geteuid() == 0 }
    }

    /// The disk a mounted folder lives on.
    fn disk_of_mount(r: &str, dir: &str) -> Option<String> {
        mount_of(r, dir.trim_end_matches('/')).and_then(|(d, _, _)| part_of(r, &d)).map(|(d, _)| d)
    }

    fn on_usb(r: &str, disk: &str) -> bool {
        read(&format!("{}/sys/block/{}/removable", r, disk)).as_deref() == Some("1")
            || std::fs::read_link(format!("{}/sys/block/{}", r, disk)).map(|l| l.to_string_lossy().contains("/usb")).unwrap_or(false)
    }

    /// A battery is present and no charger is online.
    pub(crate) fn on_battery(r: &str) -> bool {
        let Ok(rd) = std::fs::read_dir(format!("{}/sys/class/power_supply", r)) else { return false };
        let (mut battery, mut mains_online) = (false, false);
        for e in rd.flatten() {
            let d = e.path().to_string_lossy().to_string();
            match read(&format!("{}/type", d)).as_deref() {
                Some("Battery") => battery = true,
                Some("Mains") | Some("USB") => mains_online |= read(&format!("{}/online", d)).as_deref() == Some("1"),
                _ => {}
            }
        }
        battery && !mains_online
    }

    /// `sda1` of `sda`, `nvme0n1p2` of `nvme0n1`; not `sdaa1` of `sda`.
    pub(crate) fn is_partition_of(name: &str, disk: &str) -> bool {
        let Some(rest) = name.strip_prefix(disk) else { return false };
        // a disk whose name ends in a digit names its partitions with a 'p'
        let rest = if disk.ends_with(|c: char| c.is_ascii_digit()) { match rest.strip_prefix('p') { Some(r) => r, None => return false } } else { rest };
        !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
    }

    // ------------------------------------------------------------ Linux: check, arm and disarm

    /// The plan, with the Windows partition mounted read-only by us
    /// (ntfs-3g) for as long as it is read, then unmounted.
    fn checked_plan(root: &str) -> (Value, Value) {
        let r = root.trim_end_matches('/');
        let refused = |why: String| (json!({ "result": "refused", "why": [why] }), Value::Null);
        let outcome = crate::read_json(&format!("{}/{}/outcome.json", r, crate::HANDOFF)).unwrap_or(Value::Null);
        let (disk, _, _) = crate::reclaim::live::linux_disk(r);
        let Some(disk) = disk else { return refused("the disk Linux runs from is not known".into()) };
        let table = match disk_table(r, &disk) {
            Ok(t) => t,
            Err(e) => return refused(format!("the disk's partition table is not whole ({})", e)),
        };
        let rel = "/run/upgrade_/reclaim-windows";
        let mp = format!("{}{}", r, rel);
        let Some(dev) = crate::reclaim::live::windows_device(r, &disk, &table, &outcome) else {
            // the plan names why (the recorded partition is not on the disk)
            return crate::reclaim::live::plan_with(root, None);
        };
        let _ = std::fs::create_dir_all(&mp);
        if let Err(e) = run("mount", &["-t", "ntfs-3g", "-o", "ro", &dev, &mp]) {
            return refused(format!("Windows' drive could not be opened read-only ({}). If it is open in the file manager, close it there and try again. Nothing is deleted unseen", e));
        }
        let out = crate::reclaim::live::plan_with(root, Some(rel));
        let _ = run("umount", &[&mp]);
        out
    }

    /// `settle-in reclaim check` (root): the plan with Windows read, read-only,
    /// and the words for it. Writes nothing.
    pub fn check(root: &str) -> Value {
        if root == "/" && !is_root() {
            return json!({ "result": "refused", "why": ["this needs administrator rights (run it through pkexec)"] });
        }
        let (mut p, _) = checked_plan(root);
        if p["result"] == "plan" && on_battery(root.trim_end_matches('/')) {
            p = json!({ "result": "refused", "why": ["the computer is running on its battery: plug in the charger first, and leave it in"] });
        }
        p["words"] = words(&p);
        p
    }

    /// The firmware entry this Linux was started from, and whether it is one
    /// of Linux's own (a path under \EFI\ that is not Microsoft's, on `esp`).
    fn started_from(r: &str, esp: Option<[u8; 16]>) -> Value {
        let cur = crate::efi::read_var(r, "BootCurrent").map(|d| crate::efi::u16_list(&d)).and_then(|l| l.first().copied());
        let entries = crate::efi::boot_entries(r);
        let lo = cur.and_then(|c| entries.iter().find(|(n, _)| *n == c).map(|(_, lo)| lo.clone()));
        let own = lo.as_ref().is_some_and(|lo| esp.is_some() && lo.partition == esp
            && lo.path.as_deref().map(|p| { let p = p.replace('/', "\\").to_lowercase(); p.starts_with("\\efi\\") && !p.contains("\\microsoft\\") }).unwrap_or(false));
        json!({ "boot_current": cur.map(|c| format!("{:04X}", c)), "path": lo.and_then(|l| l.path), "linux_own_entry": own })
    }

    /// `settle-in reclaim arm --typed S` (root, through pkexec), inside the
    /// window's last click. Writes only to the stick and the firmware's
    /// one-time start; the window restarts the computer straight after.
    pub fn arm(root: &str, typed: &str, now_utc: &str) -> Value {
        let r = root.trim_end_matches('/');
        let refuse = |why: Vec<String>| json!({ "result": "refused", "why": why, "computer_changed": false });
        if root == "/" && !is_root() {
            return refuse(vec!["this needs administrator rights (run it through pkexec)".into()]);
        }
        if typed.trim() != reclaim::SENTENCE {
            return refuse(vec!["the sentence was not typed exactly".into()]);
        }
        if on_battery(r) {
            return refuse(vec!["the computer is running on its battery: plug in the charger first, and leave it in".into()]);
        }
        let (plan, facts) = checked_plan(root);
        if plan["result"] != "plan" {
            return refuse(plan["why"].as_array().into_iter().flatten().filter_map(|w| w.as_str().map(str::to_string)).collect());
        }
        let disk = facts["disk"].as_str().unwrap_or("").to_string();
        let job = facts["job"].clone();
        // the kit stick: the one this computer was converted with, on USB, not the disk
        let stick = match find_stick(root) {
            Ok(m) => m,
            Err(e) => return refuse(vec![e]),
        };
        let sd = Path::new(&stick.dir);
        let mut no = Vec::new();
        let stick_disk = disk_of_mount(r, &stick.dir);
        match &stick_disk {
            None => no.push("the USB stick's disk could not be named".into()),
            Some(d) if *d == disk => no.push("the stick named as the kit is the disk Linux runs from".into()),
            Some(d) if !on_usb(r, d) => no.push(format!("the disk labelled as the kit ({}) is not a USB stick", d)),
            _ => {}
        }
        let stick_job = find_ci(sd, "upgrade_/job.json").map(|p| crate::read_json(&p.to_string_lossy()).unwrap_or(Value::Null)).unwrap_or(Value::Null);
        if stick_job["job_id"].is_null() || stick_job["job_id"] != job["job_id"] {
            no.push("the USB stick plugged in is not the one this computer was converted with".into());
        }
        if find_ci(sd, "upgrade_/converted").is_none() {
            no.push("the USB stick does not record that it converted this computer".into());
        }
        if find_ci(sd, KS_REL).is_none() || !find_ci(sd, "EFI/BOOT/grub.cfg").and_then(|p| std::fs::read_to_string(p).ok()).is_some_and(|t| t.contains("boot-reclaim")) {
            no.push("this computer's USB stick was made by an older kit that cannot free Windows' space, and that cannot be fixed from here yet".into());
        }
        if find_ci(sd, "upgrade_/esp-snapshot/SHA256SUMS").is_none() {
            no.push("the USB stick does not hold the record of the startup files taken before the install, so Windows' own loader cannot be told from shim".into());
        }
        if (FileJournal { dir: sd.join(JOURNAL_DIR_REL).to_string_lossy().to_string() }).exists() {
            no.push("the USB stick holds a reclaim that has already started; it carries on only from the stick".into());
        }
        if !no.is_empty() {
            if stick.by_us {
                let _ = run("umount", &[&stick.dir]);
            }
            return refuse(no);
        }
        // which entry this Linux started from (RISKS R21: the finish removes
        // Windows' startup only when Linux has an entry of its own)
        let esp_unique = crate::reclaimrun::table_from_json(&facts["table"]).ok()
            .and_then(|t| t.entries.iter().find(|e| guid_text(&e.type_guid) == reclaim::ESP).map(|e| e.unique));
        let started = started_from(r, esp_unique);
        // the one-time start from the stick (as the walk-away way back does)
        let usb_disks = crate::sticks::collect(root).as_array().map(|a| a.iter().filter(|d| d["usb"] == json!(true)).count()).unwrap_or(0);
        let mut boot = match crate::walkaway::boot_once_as(stick_disk.as_deref().unwrap_or(""), usb_disks, ENTRY_LABEL) {
            Ok(b) => b,
            Err(e) => return json!({ "result": "stopped", "why": format!("the computer could not be told to start from the stick ({})", e), "computer_changed": false }),
        };
        boot["started_from"] = started;
        let o = order(job["job_id"].as_str().unwrap_or(""), typed.trim(), &plan, &facts, &boot, now_utc);
        let wrote = (|| -> Result<(), String> {
            std::fs::create_dir_all(sd.join(JOURNAL_DIR_REL)).map_err(|e| e.to_string())?;
            let _ = std::fs::remove_dir_all(sd.join(SAVED_REL));
            write_durable(&sd.join(ORDER_REL).to_string_lossy(), (serde_json::to_string_pretty(&o).unwrap_or_default() + "\n").as_bytes())?;
            write_durable(&sd.join(MARKER_REL).to_string_lossy(), format!("{}\n", now_utc).as_bytes())
        })();
        if let Err(e) = wrote {
            let _ = disarm_stick(sd);
            let _ = crate::walkaway::undo_as(ENTRY_LABEL);
            return json!({ "result": "stopped", "why": format!("the order could not be written onto the stick ({})", e), "computer_changed": false });
        }
        let _ = run("sync", &[]);
        json!({ "result": "ready", "plan": plan, "boot_once": boot, "stick": stick.dir, "computer_changed": false })
    }

    fn disarm_stick(sd: &Path) -> Result<(), String> {
        for rel in [MARKER_REL, ORDER_REL] {
            let p = sd.join(rel);
            if p.exists() {
                std::fs::remove_file(&p).map_err(|e| format!("{}: {}", p.display(), e))?;
            }
        }
        Ok(())
    }

    /// `settle-in reclaim disarm` (root): the order, the marker and the
    /// one-time start, taken back. Refused once the stick has started it.
    pub fn disarm(root: &str) -> Value {
        if root == "/" && !is_root() {
            return json!({ "result": "stopped", "why": "this needs administrator rights (run it through pkexec)" });
        }
        let fw = crate::walkaway::undo_as(ENTRY_LABEL);
        let stick = match find_stick(root) {
            Ok(m) => m,
            Err(e) => return json!({ "result": if fw.is_ok() { "undone" } else { "stopped" }, "why": e, "firmware": fw.unwrap_or_else(|e| json!(e)) }),
        };
        let sd = Path::new(&stick.dir);
        if (FileJournal { dir: sd.join(JOURNAL_DIR_REL).to_string_lossy().to_string() }).exists() {
            return json!({ "result": "refused", "why": "the reclaim has already started from the stick; it can only be finished" });
        }
        let st = disarm_stick(sd);
        match (st, fw) {
            (Ok(()), Ok(f)) => json!({ "result": "undone", "firmware": f }),
            (a, b) => json!({ "result": "stopped", "why": format!("{} {}", a.err().unwrap_or_default(), b.err().unwrap_or_default()).trim().to_string() }),
        }
    }

    // ------------------------------------------------------------ the stick: the run

    /// The console the person is looking at while the installer's boot runs.
    struct Screen {
        tty: Option<std::fs::File>,
        log: Vec<String>,
    }

    impl Screen {
        fn open() -> Screen {
            let _ = run("chvt", &["9"]);
            let tty = std::fs::OpenOptions::new().write(true).open("/dev/tty9").ok();
            Screen { tty, log: Vec::new() }
        }
        fn say(&mut self, line: &str) {
            println!("{}", line);
            if let Some(t) = &mut self.tty {
                let _ = t.write_all(format!("{}\r\n", line).as_bytes());
            }
            self.log.push(line.to_string());
        }
    }

    fn ext4_magic(d: &mut std::fs::File, offset: u64) -> bool {
        let mut b = [0u8; 2];
        d.seek(std::io::SeekFrom::Start(offset + 1024 + 0x38)).is_ok() && d.read_exact(&mut b).is_ok() && b == [0x53, 0xEF]
    }

    /// The internal disk whose GPT (either copy) carries `guid`, never `skip`.
    fn find_disk(skip: &str, guid: &[u8]) -> Result<String, String> {
        let mut hits = Vec::new();
        for e in std::fs::read_dir("/sys/block").map_err(|e| e.to_string())?.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if n == skip || ["loop", "ram", "zram", "dm-", "sr", "md"].iter().any(|p| n.starts_with(p)) {
                continue;
            }
            let sector: u64 = read(&format!("/sys/block/{}/queue/logical_block_size", n)).and_then(|s| s.parse().ok()).unwrap_or(512);
            let sectors: u64 = read(&format!("/sys/block/{}/size", n)).and_then(|s| s.parse().ok()).unwrap_or(0) * 512 / sector;
            let Ok(mut f) = std::fs::File::open(format!("/dev/{}", n)) else { continue };
            for lba in [1, sectors.saturating_sub(1)] {
                let mut h = vec![0u8; sector as usize];
                if f.seek(std::io::SeekFrom::Start(lba * sector)).is_ok() && f.read_exact(&mut h).is_ok() && &h[0..8] == b"EFI PART" && &h[56..72] == guid {
                    hits.push(n.clone());
                    break;
                }
            }
        }
        match hits.len() {
            1 => Ok(hits.remove(0)),
            0 => Err("the disk the reclaim was asked for is not in this computer".into()),
            _ => Err(format!("more than one disk carries the same id ({}): nothing is touched", hits.join(", "))),
        }
    }

    /// Partitions of `disk` that are mounted, used as swap, or held by
    /// anything (device-mapper, RAID, LVM: `holders`).
    fn busy(disk: &str) -> Vec<String> {
        let mut b: Vec<String> = crate::reclaim::live::mounts("").into_iter()
            .filter_map(|(d, m, _, _)| part_of("", &d).filter(|(dk, _)| dk == disk).map(|(_, n)| format!("partition {} at {}", n, m)))
            .collect();
        for l in read("/proc/swaps").unwrap_or_default().lines().skip(1) {
            if let Some(dev) = l.split_whitespace().next() {
                let name = dev.trim_start_matches("/dev/");
                if name == disk || is_partition_of(name, disk) {
                    b.push(format!("{} as swap", dev));
                }
            }
        }
        for e in std::fs::read_dir(format!("/sys/block/{}", disk)).into_iter().flatten().flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if is_partition_of(&n, disk)
                && std::fs::read_dir(e.path().join("holders")).map(|h| h.count() > 0).unwrap_or(false) {
                    b.push(format!("{} held by another device", n));
                }
        }
        if std::fs::read_dir(format!("/sys/block/{}/holders", disk)).map(|h| h.count() > 0).unwrap_or(false) {
            b.push(format!("{} held by another device", disk));
        }
        b
    }

    struct Loop(String);

    impl Loop {
        fn new(disk: &str, offset: u64, len: u64, ro: bool) -> Result<Loop, String> {
            let (o, l) = (offset.to_string(), len.to_string());
            let mut a = vec!["--find", "--show", "--offset", &o, "--sizelimit", &l];
            if ro {
                a.push("--read-only");
            }
            a.push(disk);
            Ok(Loop(run("losetup", &a)?.trim().to_string()))
        }
    }

    impl Drop for Loop {
        fn drop(&mut self) {
            let _ = run("losetup", &["-d", &self.0]);
        }
    }

    struct Mount(String);

    impl Mount {
        fn new(dev: &str, at: &str, fstype: &str, ro: bool) -> Result<Mount, String> {
            std::fs::create_dir_all(at).map_err(|e| format!("{}: {}", at, e))?;
            run("mount", &["-t", fstype, "-o", if ro { "ro" } else { "rw" }, dev, at])?;
            Ok(Mount(at.to_string()))
        }
    }

    impl Drop for Mount {
        fn drop(&mut self) {
            let _ = run("sync", &[]);
            let _ = run("umount", &[&self.0]);
        }
    }

    struct LiveOps<'a> {
        disk: String,
        stick: String,
        screen: &'a mut Screen,
    }

    const WORK: &str = "/tmp/upg-reclaim";

    fn entry_num(o: &Value, p: &str) -> Option<u16> {
        o.pointer(p).and_then(Value::as_str).and_then(|s| u16::from_str_radix(s.trim_start_matches("Boot"), 16).ok())
    }

    fn write_order(v: &[u16]) -> Result<(), String> {
        crate::efi::write_var("", "BootOrder", &v.iter().flat_map(|n| n.to_le_bytes()).collect::<Vec<u8>>())?;
        let back = crate::efi::read_var("", "BootOrder").map(|d| crate::efi::u16_list(&d));
        if back.as_deref() != Some(v) {
            return Err(format!("the firmware did not keep the start order written ({:04X?} read back as {:04X?})", v, back));
        }
        Ok(())
    }

    impl LiveOps<'_> {
        fn esp(&self, o: &Value, ro: bool) -> Result<(Loop, Mount), String> {
            let t = table_from_json(&o["table_before"])?;
            let e = t.entries.iter().find(|e| guid_text(&e.type_guid) == reclaim::ESP).ok_or("the EFI system partition is not in the order")?;
            let lp = Loop::new(&format!("/dev/{}", self.disk), e.first_lba * t.sector, e.sectors() * t.sector, ro)?;
            let m = Mount::new(&lp.0, &format!("{}/esp", WORK), "vfat", ro)?;
            Ok((lp, m))
        }
        fn saved(&self, rel: &str) -> String {
            format!("{}/{}/{}", self.stick, SAVED_REL, rel)
        }
        /// Write `bytes` to the stick's saved folder once; later calls keep the first.
        fn save_once(&self, rel: &str, bytes: &[u8]) -> Result<(), String> {
            let keep = self.saved(rel);
            if Path::new(&keep).exists() {
                return Ok(());
            }
            std::fs::create_dir_all(Path::new(&keep).parent().unwrap()).map_err(|e| e.to_string())?;
            write_durable(&format!("{}.part", keep), bytes)?;
            std::fs::rename(format!("{}.part", keep), &keep).map_err(|e| e.to_string())
        }
        fn windows_entries(o: &Value) -> Vec<u16> {
            let esp_unique = table_from_json(&o["table_before"]).ok()
                .and_then(|t| t.entries.iter().find(|e| guid_text(&e.type_guid) == reclaim::ESP).map(|e| e.unique));
            crate::rollback::windows_entries(&crate::efi::boot_entries("")).into_iter()
                .filter(|(_, p)| p.is_some() && *p == esp_unique).map(|(n, _)| n).collect()
        }
        fn saved_order(&self) -> Result<Vec<u16>, String> {
            std::fs::read_to_string(self.saved("BootOrder.json")).ok()
                .and_then(|t| serde_json::from_str::<Vec<u16>>(&t).ok()).ok_or_else(|| "the start order saved before the move is not on the stick".to_string())
        }

        /// Undo the guard (an abort before anything moved): the start files,
        /// Windows' held loader, the firmware's order, as they were.
        fn unguard(&mut self, o: &Value) -> Result<(), String> {
            {
                let (_lp, m) = self.esp(o, false)?;
                for rel in grub_cfgs(&m.0) {
                    if let Ok(b) = std::fs::read(self.saved(&rel)) {
                        write_durable(&format!("{}/{}", m.0, rel), &b)?;
                    }
                }
                let base = Path::new(&m.0);
                if let Some(h) = find_ci(base, HELD_REL)
                    && find_ci(base, crate::rollback::SLOT_REL).is_none() {
                        std::fs::rename(&h, h.with_file_name("bootmgfw.efi")).map_err(|e| format!("{}: {}", h.display(), e))?;
                    }
            }
            if let Ok(v) = self.saved_order() {
                write_order(&v)?;
            }
            Ok(())
        }
    }

    /// The `EFI/<folder>/grub.cfg` files on a mounted ESP.
    fn grub_cfgs(esp: &str) -> Vec<String> {
        let Some(efi) = find_ci(Path::new(esp), "EFI") else { return vec![] };
        let mut v: Vec<String> = std::fs::read_dir(efi).into_iter().flatten().flatten()
            .filter_map(|d| std::fs::read_dir(d.path()).ok().map(|rd| (d, rd)))
            .flat_map(|(d, rd)| rd.flatten().filter(|f| f.file_name().to_string_lossy().eq_ignore_ascii_case("grub.cfg"))
                .map(move |f| format!("EFI/{}/{}", d.file_name().to_string_lossy(), f.file_name().to_string_lossy())).collect::<Vec<_>>())
            .collect();
        v.sort();
        v
    }

    impl Ops for LiveOps<'_> {
        fn say(&mut self, line: &str) {
            self.screen.say(line);
        }

        fn guard(&mut self, o: &Value) -> Result<Value, String> {
            let (_lp, m) = self.esp(o, false)?;
            let base = Path::new(&m.0);
            // 1. the decision about Windows' startup, once, before anything changes
            let cleanup = match std::fs::read_to_string(self.saved("cleanup.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) {
                Some(c) => c,
                None => {
                    let snap = std::fs::read_to_string(format!("{}/{}/SHA256SUMS", self.stick, crate::rollback::SNAPSHOT_REL)).map(|t| crate::rollback::sums(&t)).unwrap_or_default();
                    let look = EspLook {
                        slot: find_ci(base, crate::rollback::SLOT_REL).and_then(|p| sha(&p)),
                        kept: find_ci(base, crate::rollback::KEPT_REL).and_then(|p| sha(&p)),
                        snapshot_slot: snap.get(&crate::rollback::SLOT_REL.to_lowercase()).cloned(),
                    };
                    let mut c = esp_cleanup(&look, o.pointer("/boot/started_from/linux_own_entry") == Some(&json!(true)));
                    c["windows_entries"] = json!(Self::windows_entries(o));
                    self.save_once("cleanup.json", c.to_string().as_bytes())?;
                    c
                }
            };
            let windows: Vec<u16> = cleanup["windows_entries"].as_array().into_iter().flatten().filter_map(|n| n.as_u64().map(|n| n as u16)).collect();
            // 2. the stick first in the firmware's order (the order before, saved once), read back
            let order_now = crate::efi::read_var("", "BootOrder").map(|d| crate::efi::u16_list(&d)).ok_or("the firmware's start order is not readable")?;
            self.save_once("BootOrder.json", json!(order_now).to_string().as_bytes())?;
            let first = entry_num(o, "/boot/boot_next").ok_or("the order does not name the stick's firmware entry")?;
            let new = guard_boot_order(&order_now, first, &windows, cleanup["windows_own_slot"] == json!(true));
            write_order(&new)?;
            // 3. Windows' own loader held aside, so Windows cannot start and repair a disk being rewritten
            let mut held = false;
            if cleanup["hold_slot"] == json!(true)
                && let Some(slot) = find_ci(base, crate::rollback::SLOT_REL) {
                    std::fs::rename(&slot, slot.with_file_name("bootmgfw.efi.upgrade_-held")).map_err(|e| format!("{}: {}", slot.display(), e))?;
                    held = true;
                }
            // 4. Linux's start files stubbed (originals saved once)
            let mut stubbed = Vec::new();
            for rel in grub_cfgs(&m.0) {
                let p = format!("{}/{}", m.0, rel);
                let now = std::fs::read(&p).map_err(|e| format!("{}: {}", rel, e))?;
                if !is_stub(&String::from_utf8_lossy(&now)) {
                    self.save_once(&rel, &now)?;
                    write_durable(&p, stub_grub_cfg().as_bytes())?;
                }
                stubbed.push(rel);
            }
            if stubbed.is_empty() {
                return Err("no GRUB start file was found to stub".into());
            }
            Ok(json!({ "cleanup": cleanup, "boot_order_before": order_now, "boot_order": new, "held": held, "stubbed": stubbed }))
        }

        fn check(&mut self, offset: u64, len: u64) -> Result<Value, String> {
            let lp = Loop::new(&format!("/dev/{}", self.disk), offset, len, true)?;
            let (c, out) = code("e2fsck", &["-f", "-n", &lp.0])?;
            if c != 0 {
                return Err(format!("the file system check found problems (e2fsck exit {}): {}", c, out.lines().rev().take(3).collect::<Vec<_>>().join(" / ")));
            }
            Ok(json!({ "e2fsck": 0 }))
        }

        fn grow(&mut self, offset: u64, len: u64) -> Result<Value, String> {
            let lp = Loop::new(&format!("/dev/{}", self.disk), offset, len, false)?;
            let (c, out) = code("e2fsck", &["-f", "-y", &lp.0])?;
            if c > 1 {
                return Err(format!("the file system check before growing could not finish (e2fsck exit {}): {}", c, out.lines().rev().take(3).collect::<Vec<_>>().join(" / ")));
            }
            let rs = run("resize2fs", &[&lp.0])?;
            let (c2, out2) = code("e2fsck", &["-f", "-n", &lp.0])?;
            if c2 != 0 {
                return Err(format!("the file system check after growing found problems (e2fsck exit {}): {}", c2, out2.lines().rev().take(3).collect::<Vec<_>>().join(" / ")));
            }
            Ok(json!({ "e2fsck_before": c, "resize2fs": rs.lines().last().unwrap_or("").trim(), "e2fsck_after": 0 }))
        }

        fn finish(&mut self, o: &Value, guard: &Value) -> Result<Value, String> {
            let c = &guard["cleanup"];
            let mut done = json!({ "cleanup": c });
            let t = table_from_json(&o["table_before"])?;
            let plan = &o["plan"];
            // every disk and stick step first; the firmware last (a cut before
            // it still starts the stick, which finishes)
            {
                let (_lp, m) = self.esp(o, false)?;
                let base = Path::new(&m.0);
                let mut restored = Vec::new();
                for rel in grub_cfgs(&m.0) {
                    if let Ok(b) = std::fs::read(self.saved(&rel)) {
                        write_durable(&format!("{}/{}", m.0, rel), &b)?;
                        restored.push(rel);
                    } else if std::fs::read_to_string(format!("{}/{}", m.0, rel)).map(|t| is_stub(&t)).unwrap_or(false) {
                        return Err(format!("{} is still the stub and its original is not on the stick", rel));
                    }
                }
                done["restored"] = json!(restored);
                if c["remove_kept_loader"] == json!(true)
                    && let Some(p) = find_ci(base, crate::rollback::KEPT_REL) {
                        std::fs::remove_file(&p).map_err(|e| format!("{}: {}", p.display(), e))?;
                    }
                if c["remove_microsoft_folder"] == json!(true) {
                    if let Some(p) = find_ci(base, "EFI/Microsoft") {
                        std::fs::remove_dir_all(&p).map_err(|e| format!("{}: {}", p.display(), e))?;
                    }
                } else if let Some(h) = find_ci(base, HELD_REL)
                    && find_ci(base, crate::rollback::SLOT_REL).is_none() {
                        std::fs::rename(&h, h.with_file_name("bootmgfw.efi")).map_err(|e| format!("{}: {}", h.display(), e))?;
                    }
                let rec = json!({ "schema": "reclaimed/1", "job_id": o["job_id"], "program": format!("settle-in {}", env!("CARGO_PKG_VERSION")),
                    "deleted": plan["delete"], "deleted_bytes": plan["deleted_bytes"] });
                let _ = std::fs::create_dir_all(base.join("upgrade_"));
                write_durable(&base.join(ESP_DONE_REL).to_string_lossy(), (rec.to_string() + "\n").as_bytes())?;
            }
            // GRUB's 'Windows (kept)' entry: from the original saved on the stick, so a cut write is redone
            let (bo, bl) = placed(plan, t.sector, o["boot_number"].as_u64().unwrap_or(0)).ok_or("the plan does not place /boot")?;
            {
                let lp = Loop::new(&format!("/dev/{}", self.disk), bo, bl, false)?;
                let m = Mount::new(&lp.0, &format!("{}/boot", WORK), "ext4", false)?;
                let cfg = format!("{}/grub2/grub.cfg", m.0);
                if let Ok(b) = std::fs::read(&cfg)
                    && !Path::new(&self.saved("boot-grub2-grub.cfg")).exists() {
                        if b.is_empty() {
                            return Err("/boot/grub2/grub.cfg is empty and no copy of it was saved".into());
                        }
                        self.save_once("boot-grub2-grub.cfg", &b)?;
                    }
                let orig = std::fs::read_to_string(self.saved("boot-grub2-grub.cfg")).ok();
                done["grub_cfg"] = match orig.as_deref() {
                    None => json!("no grub.cfg in /boot/grub2"),
                    Some(text) => {
                        let new = without_kept_block(text).unwrap_or_else(|| text.to_string());
                        // the same file written over: it keeps its SELinux label
                        let mut f = std::fs::OpenOptions::new().write(true).truncate(true).open(&cfg).map_err(|e| format!("{}: {}", cfg, e))?;
                        f.write_all(new.as_bytes()).and_then(|_| f.sync_all()).map_err(|e| format!("{}: {}", cfg, e))?;
                        json!(if new.len() != text.len() { "the Windows entry removed" } else { "no Windows entry found in grub.cfg" })
                    }
                };
            }
            let (ro, rl) = placed(plan, t.sector, o["root_number"].as_u64().unwrap_or(0)).ok_or("the plan does not place /")?;
            {
                let lp = Loop::new(&format!("/dev/{}", self.disk), ro, rl, false)?;
                let m = Mount::new(&lp.0, &format!("{}/root", WORK), "ext4", false)?;
                let p = format!("{}/{}", m.0, KEPT_GRUB_D);
                done["grub_d"] = json!(if Path::new(&p).exists() { std::fs::remove_file(&p).map(|_| "removed").map_err(|e| format!("{}: {}", p, e))? } else { "not there" });
            }
            // the firmware, last: the order from before (which never held the
            // entry the arm made), without that entry, and without Windows'
            // when its startup was removed
            let saved_order = self.saved_order()?;
            let mut drop: Vec<u16> = entry_num(o, "/boot/entry").into_iter().collect();
            if c["remove_windows_entries"] == json!(true) {
                drop.extend(c["windows_entries"].as_array().into_iter().flatten().filter_map(|n| n.as_u64().map(|n| n as u16)));
            }
            let fin = final_boot_order(&saved_order, &drop);
            if fin.is_empty() {
                return Err("the firmware's order would be empty; it is left as it is".into());
            }
            write_order(&fin)?;
            let entries = crate::efi::boot_entries("");
            // the stick's generic USB entry is the firmware's own: only ours and Windows' are deleted
            for n in drop.iter().filter(|n| entries.iter().any(|(e, _)| e == *n)) {
                let _ = crate::efi::delete_var("", &format!("Boot{:04X}", n));
            }
            done["boot_order"] = json!(fin);
            // the stick: no reclaim boot again
            let mk = format!("{}/{}", self.stick, MARKER_REL);
            if Path::new(&mk).exists() {
                std::fs::remove_file(&mk).map_err(|e| format!("{}: {}", mk, e))?;
            }
            Ok(done)
        }
    }

    /// Make the writes durable, then drop the kernel's cached copy of the
    /// disk (BLKFLSBUF), so the hash after the move reads what the drive
    /// stored, not what the kernel remembers it was sent.
    fn sync_and_drop(f: &mut std::fs::File) -> Result<(), String> {
        use std::os::fd::AsRawFd;
        f.sync_all().map_err(|e| e.to_string())?;
        const BLKFLSBUF: libc::c_ulong = 0x1261;
        // SAFETY: BLKFLSBUF takes no argument; on a block device it flushes and invalidates its buffers.
        if unsafe { libc::ioctl(f.as_raw_fd(), BLKFLSBUF as _, 0) } != 0 {
            return Err(format!("the disk's cache could not be dropped ({})", std::io::Error::last_os_error()));
        }
        Ok(())
    }

    /// Restart, and never return to the installer: if the restart does not
    /// happen, wait here for ever (an installer a few keys from an install
    /// over this disk is the one place it must not go back to).
    fn reboot_never_return() -> ! {
        let _ = run("sync", &[]);
        let _ = run("systemctl", &["reboot"]);
        std::thread::sleep(std::time::Duration::from_secs(60));
        let _ = run("reboot", &["-f"]);
        loop {
            std::thread::sleep(std::time::Duration::from_secs(3600));
        }
    }

    fn wait_forever(screen: &mut Screen, lines: &[&str]) -> ! {
        for l in lines {
            screen.say(l);
        }
        loop {
            std::thread::sleep(std::time::Duration::from_secs(3600));
        }
    }

    fn save_record(stick: &str, v: &Value) {
        let _ = write_durable(&format!("{}/{}", stick, RECORD_REL), (serde_json::to_string_pretty(v).unwrap_or_default() + "\n").as_bytes());
    }

    /// What the stick reads before it commits, or before it carries on.
    fn gather(o: &Value, disk: &str, stick_disk: &str, screen: &mut Screen, stick: &str) -> (OnStick, Vec<String>) {
        let table = disk_table("", disk);
        let before = table_from_json(&o["table_before"]);
        let mut no = Vec::new();
        let (mut flags, mut cfgs) = ((false, false), Vec::new());
        match (LiveOps { disk: disk.to_string(), stick: stick.to_string(), screen }).esp(o, true) {
            Ok((_l, m)) => {
                flags = (find_ci(Path::new(&m.0), "upgrade_/rolled-back.json").is_some(), find_ci(Path::new(&m.0), ESP_DONE_REL).is_some());
                cfgs = grub_cfgs(&m.0);
            }
            Err(e) => no.push(format!("the EFI system partition could not be read ({})", e)),
        }
        let (mut b4, mut r4) = (false, false);
        let mut opened = std::fs::File::open(format!("/dev/{}", disk));
        if let (Ok(t), Ok(f)) = (&before, &mut opened) {
            if let Some((off, _)) = placed_before(t, o["boot_number"].as_u64().unwrap_or(0)) { b4 = ext4_magic(f, off); }
            if let Some((off, _)) = placed_before(t, o["root_number"].as_u64().unwrap_or(0)) { r4 = ext4_magic(f, off); }
        }
        let order_now = crate::efi::read_var("", "BootOrder");
        let stick_entry = entry_num(o, "/boot/boot_next").is_some_and(|n| crate::efi::boot_entries("").iter().any(|(e, _)| *e == n));
        (OnStick {
            table,
            other_disks_in_use: other_windows_disks("", &[disk.to_string(), stick_disk.to_string()]),
            busy: busy(disk),
            rolled_back: flags.0,
            reclaimed: flags.1,
            grub_cfgs: cfgs,
            boot_is_ext4: b4,
            root_is_ext4: r4,
            boot_order_readable: order_now.is_some(),
            stick_entry_present: stick_entry,
            on_battery: on_battery(""),
        }, no)
    }

    /// The run on the stick (`settle-in reclaim run --stick DIR`), from the
    /// kit's reclaim kickstart. It never returns to the installer.
    pub fn onstick(stick: &str, now_utc: &str) -> i32 {
        // only from the stick's reclaim boot: never against a running Linux
        if !read("/proc/cmdline").unwrap_or_default().split_whitespace().any(|a| a == "upg.mode=reclaim") {
            eprintln!("settle-in: `reclaim run` runs only from the USB stick's reclaim start (upg.mode=reclaim); refused, nothing touched");
            return 1;
        }
        let stick = stick.trim_end_matches('/').to_string();
        let mut screen = Screen::open();
        screen.say(&format!("upgrade_ {}: freeing Windows' space for Linux (settle-in {}).", VERSION, env!("CARGO_PKG_VERSION")));
        let _ = run("mount", &["-o", "remount,rw", &stick]);
        // no sleep, no lid switch, for as long as this runs (best effort; the window's check refused a battery)
        let _awake = crate::awake::hold("moving Linux into Windows' space");
        let mut j = FileJournal { dir: format!("{}/{}", stick, JOURNAL_DIR_REL) };
        let o = crate::read_json(&format!("{}/{}", stick, ORDER_REL)).unwrap_or(Value::Null);
        let stick_disk = disk_of_mount("", &stick).unwrap_or_default();
        // before anything moved: every refusal leaves the computer as it was
        let refuse = |screen: &mut Screen, j: &FileJournal, why: Vec<String>| -> ! {
            for w in &why {
                screen.say(&format!("  - {}", w));
            }
            let _ = j.clear();
            let _ = std::fs::remove_file(format!("{}/{}", stick, MARKER_REL));
            let _ = crate::walkaway::undo_as(ENTRY_LABEL);
            save_record(&stick, &json!({ "schema": "reclaim-record/1", "result": "refused", "why": why, "at_utc": now_utc, "computer_changed": false, "screen": screen.log }));
            screen.say("Nothing was deleted or moved. Linux starts again in 60 seconds.");
            std::thread::sleep(std::time::Duration::from_secs(60));
            reboot_never_return()
        };
        let st = match j.load() {
            Err(e) => wait_forever(&mut screen, &[&format!("The reclaim's journal on the stick cannot be read: {}", e), "Nothing more is done. Do not start Linux without this stick. Ask for help with the record on the stick."]),
            Ok(s) => s,
        };
        if o.is_null() {
            if st.is_none() {
                refuse(&mut screen, &j, vec!["there is no reclaim order on the stick".into()]);
            }
            wait_forever(&mut screen, &["The reclaim's order is missing from the stick, so it cannot carry on. Ask for help with the record on the stick."]);
        }
        let guid = o["disk_guid"].as_str().and_then(unhex).unwrap_or_default();
        let phase = st.as_ref().and_then(|s| s["phase"].as_str()).unwrap_or("").to_string();
        // nothing has moved yet: not started, or committed with the guard cut part way
        let unmoved = st.is_none() || phase == "committed";
        let disk = match find_disk(&stick_disk, &guid) {
            Ok(d) => d,
            Err(e) if st.is_none() => refuse(&mut screen, &j, vec![e]),
            Err(e) => wait_forever(&mut screen, &[&format!("It cannot carry on: {}", e), "Put the computer's disk back as it was and start again with the stick in."]),
        };
        if unmoved {
            screen.say("Checking everything once more before anything is deleted.");
            let (facts, mut no) = gather(&o, &disk, &stick_disk, &mut screen, &stick);
            no.extend(precheck(&o, &facts));
            if no.is_empty() && let Ok(t) = table_from_json(&o["table_before"]) {
                let mut ops = LiveOps { disk: disk.clone(), stick: stick.clone(), screen: &mut screen };
                for (what, n) in [("/boot", o["boot_number"].as_u64().unwrap_or(0)), ("/", o["root_number"].as_u64().unwrap_or(0))] {
                    if let Some((off, len)) = placed_before(&t, n)
                        && let Err(e) = ops.check(off, len) {
                            no.push(format!("Linux's {} is not clean: {}", what, e));
                        }
                }
            }
            if !no.is_empty() {
                if st.is_some() {
                    // the guard may have run part way: put it back first
                    let mut ops = LiveOps { disk: disk.clone(), stick: stick.clone(), screen: &mut screen };
                    if let Err(e) = ops.unguard(&o) {
                        wait_forever(&mut screen, &[&format!("It stopped before anything was moved, but the startup could not be put back: {}", e), "Ask for help with the record on the stick."]);
                    }
                }
                refuse(&mut screen, &j, no);
            }
            if st.is_none() {
                // the commit
                if let Err(e) = committed(&o, now_utc).and_then(|c| j.save(&c)) {
                    refuse(&mut screen, &j, vec![format!("the journal could not be written onto the stick ({})", e)]);
                }
                screen.say("Everything checked. Deleting Windows and moving Linux now.");
            }
        } else {
            let b = busy(&disk);
            if !b.is_empty() {
                wait_forever(&mut screen, &[&format!("It cannot carry on while something uses the disk ({}).", b.join(", ")), "Restart with the stick in: it carries on from where it stopped."]);
            }
            screen.say("Carrying on from where it stopped.");
        }
        let mut dev = match std::fs::OpenOptions::new().read(true).write(true).open(format!("/dev/{}", disk)) {
            Ok(f) => f,
            Err(e) => wait_forever(&mut screen, &[&format!("The disk could not be opened ({}). Restart with the stick in to try again.", e)]),
        };
        let r = {
            let mut ops = LiveOps { disk: disk.clone(), stick: stick.clone(), screen: &mut screen };
            execute(&mut dev, &o, &mut j, &mut ops, CHUNK, &mut sync_and_drop)
        };
        match r {
            Ok(st) => {
                save_record(&stick, &json!({ "schema": "reclaim-record/1", "result": "reclaimed", "at_utc": now_utc, "run": st, "screen": screen.log }));
                screen.say("Finished. Windows is gone and Linux has all the space. Linux starts in 10 seconds; you can take the USB stick out.");
                std::thread::sleep(std::time::Duration::from_secs(10));
                reboot_never_return()
            }
            Err(e) => {
                save_record(&stick, &json!({ "schema": "reclaim-record/1", "result": "stopped", "why": e, "at_utc": now_utc, "screen": screen.log }));
                wait_forever(&mut screen, &[&format!("It stopped: {}", e), "Linux will not start until this finishes. Turn the computer off and on again with the stick in: it carries on from where it stopped. If it stops here again, ask for help with the record on the stick."])
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpttable::tests::{entry, image};
    use crate::reclaim::{ESP, LINUX_DATA, MSDATA, MSR};
    use std::io::{Cursor, SeekFrom};

    struct Mem(Option<Value>, usize);
    impl Journal for Mem {
        fn load(&mut self) -> Result<Option<Value>, String> {
            Ok(self.0.clone())
        }
        fn save(&mut self, v: &Value) -> Result<(), String> {
            if self.1 == 0 {
                return Err("power cut".into());
            }
            self.1 -= 1;
            self.0 = Some(v.clone());
            Ok(())
        }
    }

    #[derive(Default)]
    struct FakeOps {
        calls: Vec<String>,
        fail_once: Option<&'static str>,
    }
    impl Ops for FakeOps {
        fn guard(&mut self, _: &Value) -> Result<Value, String> {
            self.step("guard")?;
            Ok(json!({ "cleanup": { "windows_own_slot": false } }))
        }
        fn check(&mut self, o: u64, _: u64) -> Result<Value, String> {
            self.step(&format!("check@{}", o))
        }
        fn grow(&mut self, o: u64, _: u64) -> Result<Value, String> {
            self.step(&format!("grow@{}", o))
        }
        fn finish(&mut self, _: &Value, g: &Value) -> Result<Value, String> {
            assert!(!g["cleanup"].is_null(), "the finish follows the guard's decision");
            self.step("finish")
        }
    }
    impl FakeOps {
        fn step(&mut self, s: &str) -> Result<Value, String> {
            if self.fail_once.is_some_and(|f| s.starts_with(f)) {
                self.fail_once = None;
                return Err(format!("{} cut", s));
            }
            self.calls.push(s.to_string());
            Ok(json!(s))
        }
    }

    const W: u8 = 0x33;
    const M: u64 = 2048;

    /// A small converter-shaped disk image for the run: ESP, MSR, Windows,
    /// /boot, /. Its shift (1 MiB) is under a real chunk, so the run is held
    /// here with a 4 KiB chunk and a plan written out by hand.
    fn disk() -> (Cursor<Vec<u8>>, Table, Vec<u8>) {
        let (mut img, t) = image(8192, vec![
            entry(0, ESP, 0x11, 2048, 4095),
            entry(1, MSR, 0x22, 4096, 4351),
            entry(2, MSDATA, W, 4352, 6143),
            entry(3, LINUX_DATA, 0x55, 6144, 6655),
            entry(4, LINUX_DATA, 0x66, 6656, 8000),
        ]);
        let data: Vec<u8> = (0..((8000 - 6144 + 1) * 512)).map(|i| (i * 7 % 253) as u8).collect();
        img.seek(SeekFrom::Start(6144 * 512)).unwrap();
        img.write_all(&data).unwrap();
        (img, t, data)
    }

    fn job() -> Value {
        json!({ "schema": "job/1", "job_id": "j1", "intent": { "path": "keep-windows", "account": { "windows_name": "Sam" } },
            "storage": { "physical_disk": { "health_status": "Healthy" } }, "harvest": { "bitlocker": { "status": "off" }, "folders": [] } })
    }

    fn outcome(win_bytes: u64) -> Value {
        json!({ "schema": "outcome/1", "windows": { "kept": true, "partition": { "guid": guid_text(&[W; 16]), "size_bytes": win_bytes } } })
    }

    /// The order the arm would write for `t`, with its plan made by reclaim::plan.
    fn order_for(t: &Table, win_bytes: u64) -> Value {
        let (job, out) = (job(), outcome(win_bytes));
        let f = reclaim::Facts { job: &job, outcome: &out, table: Ok(t.clone()), root_number: Some(5), boot_number: Some(4),
            other_disks_in_use: vec![], windows: Ok(reclaim::WindowsRead { has_system32: true, users: vec!["Sam".into()] }), rolled_back: false, reclaimed: false };
        let p = reclaim::plan(&f);
        let facts = json!({ "table": table_json(t), "root_number": 5, "boot_number": 4, "windows": { "has_system32": true, "users": ["Sam"] }, "job": job, "outcome": out });
        order("j1", reclaim::SENTENCE, &p, &facts, &json!({ "entry": "Boot0007", "boot_next": "0007" }), "2026-11-06T00:00:00Z")
    }

    /// The converter's layout on a 2.9 GB disk: big enough for a real chunk.
    fn big() -> Table {
        crate::gpttable::tests::table(5800 * M, vec![
            entry(0, ESP, 0x11, M, 101 * M - 1),
            entry(1, MSR, 0x22, 101 * M, 117 * M - 1),
            entry(2, MSDATA, W, 117 * M, 1117 * M - 1),
            entry(3, LINUX_DATA, 0x55, 1117 * M, 2141 * M - 1),
            entry(4, LINUX_DATA, 0x66, 2141 * M, 5000 * M - 1),
        ])
    }

    fn big_order() -> Value {
        order_for(&big(), 1000 * M * 512)
    }

    fn onstick(t: &Table) -> OnStick {
        OnStick { table: Ok(t.clone()), other_disks_in_use: vec![], busy: vec![], rolled_back: false, reclaimed: false,
            grub_cfgs: vec!["EFI/fedora/grub.cfg".into()], boot_is_ext4: true, root_is_ext4: true,
            boot_order_readable: true, stick_entry_present: true, on_battery: false }
    }

    #[test]
    fn tables_round_trip_through_the_order() {
        let (_, t, _) = disk();
        assert_eq!(table_from_json(&table_json(&t)).unwrap(), t);
        assert_eq!(table_from_json(&table_json(&big())).unwrap(), big());
    }

    #[test]
    fn the_stick_agrees_with_an_unchanged_disk() {
        let o = big_order();
        assert_eq!(o["plan"]["result"], "plan", "{}", o["plan"]);
        assert_eq!(precheck(&o, &onstick(&big())), Vec::<String>::new());
        // the new table the run writes is the plan's, and keeps every id
        let nt = new_table(&big(), &o["plan"]).unwrap();
        assert_eq!(nt.entries.len(), 3);
        assert!(nt.entries.iter().all(|e| big().entries.iter().any(|b| b.unique == e.unique && b.index == e.index)));
    }

    #[test]
    fn every_stick_side_refusal() {
        let t = big();
        let o = big_order();
        let has = |s: &OnStick, o: &Value, needle: &str| precheck(o, s).iter().any(|w| w.contains(needle));
        let mut s = onstick(&t);
        s.busy = vec!["partition 5 at /mnt".into()];
        assert!(has(&s, &o, "in use"));
        let mut s = onstick(&t);
        s.grub_cfgs.clear();
        assert!(has(&s, &o, "GRUB start file"));
        let mut s = onstick(&t);
        s.root_is_ext4 = false;
        assert!(has(&s, &o, "ext4"));
        let mut s = onstick(&t);
        s.stick_entry_present = false;
        assert!(has(&s, &o, "firmware entry for the USB stick"));
        let mut s = onstick(&t);
        s.boot_order_readable = false;
        assert!(has(&s, &o, "start order cannot be read"));
        let mut s = onstick(&t);
        s.on_battery = true;
        assert!(has(&s, &o, "battery"));
        let mut s = onstick(&t);
        s.rolled_back = true;
        assert!(has(&s, &o, "rollback"));
        let mut s = onstick(&t);
        s.reclaimed = true;
        assert!(has(&s, &o, "already given"));
        let mut s = onstick(&t);
        s.other_disks_in_use = vec!["nvme1n1".into()];
        assert!(has(&s, &o, "one disk only"));
        let mut t2 = t.clone();
        t2.entries[4].last_lba -= 1;
        assert!(has(&onstick(&t2), &o, "has changed since"));
        let s = OnStick { table: Err("the backup table's header fails its check (CRC)".into()), ..onstick(&t) };
        assert!(has(&s, &o, "not whole"));
        let mut o2 = o.clone();
        o2["typed"] = json!("I confirm");
        assert!(has(&onstick(&t), &o2, "typed exactly"));
        // an order whose plan differs from the one made again (edited on the stick)
        let mut o2 = o.clone();
        o2["plan"]["move"]["shift_sectors"] = json!(1);
        assert!(has(&onstick(&t), &o2, "not the plan that was agreed to"));
        // the drive's health, as recorded, still counts on the stick
        let mut o2 = o.clone();
        o2["job"]["storage"]["physical_disk"]["health_status"] = json!("Warning");
        assert!(has(&onstick(&t), &o2, "healthy"));
        assert!(precheck(&json!({ "schema": "x" }), &onstick(&t))[0].contains("not readable"));
        // the small disk: too little room for a real chunk, refused on the stick too
        let (_, ts, _) = disk();
        assert!(has(&onstick(&ts), &order_for(&ts, 1792 * 512), "move chunk"));
    }

    /// The plan for the small disk, written out.
    fn test_plan(t: &Table) -> Value {
        let shift = 2048u64; // /boot moves down to 4096, right after the ESP
        let last = t.last_usable;
        json!({
            "result": "plan",
            "delete": [ { "number": 2, "type": MSR, "bytes": 256 * 512 }, { "number": 3, "type": MSDATA, "bytes": 1792 * 512 } ],
            "deleted_bytes": (256 + 1792) * 512,
            "move": { "from_lba": 6144, "to_lba": 4096, "sectors": 8000 - 6144 + 1, "shift_sectors": shift, "sector": 512, "chunk_bytes": 4096 },
            "new_table": [
                { "number": 1, "type": ESP, "first_lba": 2048, "last_lba": 4095 },
                { "number": 4, "type": LINUX_DATA, "first_lba": 6144 - shift, "last_lba": 6655 - shift },
                { "number": 5, "type": LINUX_DATA, "first_lba": 6656 - shift, "last_lba": last },
            ],
        })
    }

    fn small_order(t: &Table) -> Value {
        let mut o = order_for(t, 1792 * 512);
        o["plan"] = test_plan(t);
        o
    }

    fn run_to_end(img: &mut Cursor<Vec<u8>>, o: &Value, j: &mut Mem, ops: &mut FakeOps) -> Result<Value, String> {
        execute(img, o, j, ops, 4096, &mut |_| Ok(()))
    }

    #[test]
    fn the_run_moves_writes_the_table_grows_and_finishes() {
        let (mut img, t, data) = disk();
        let o = small_order(&t);
        let mut j = Mem(Some(committed(&o, "t").unwrap()), usize::MAX);
        let mut ops = FakeOps::default();
        let st = run_to_end(&mut img, &o, &mut j, &mut ops).unwrap();
        assert_eq!(st["phase"], "finished");
        assert_eq!(ops.calls, vec!["guard".to_string(), format!("grow@{}", (6656 - 2048) * 512), format!("check@{}", 4096 * 512), "finish".into()]);
        let back = crate::gpttable::read(&mut img, 512, 8192 * 512).unwrap();
        assert_eq!(back, new_table(&t, &o["plan"]).unwrap());
        assert_eq!(back.entries.len(), 3);
        let mut got = vec![0u8; data.len()];
        img.seek(SeekFrom::Start(4096 * 512)).unwrap();
        img.read_exact(&mut got).unwrap();
        assert_eq!(got, data);
    }

    #[test]
    fn a_power_cut_at_every_journal_write_and_in_every_step_resumes() {
        let (img0, t, data) = disk();
        let o = small_order(&t);
        let want = {
            let mut img = img0.clone();
            let mut j = Mem(Some(committed(&o, "t").unwrap()), usize::MAX);
            run_to_end(&mut img, &o, &mut j, &mut FakeOps::default()).unwrap();
            img.into_inner()
        };
        // the journal dies after n saves: every cut between steps and between chunks
        let total = {
            let mut img = img0.clone();
            let mut j = Mem(Some(committed(&o, "t").unwrap()), 100_000);
            run_to_end(&mut img, &o, &mut j, &mut FakeOps::default()).unwrap();
            100_000 - j.1
        };
        assert!(total > 200, "{} saves", total);
        for n in 0..=total {
            let mut img = img0.clone();
            let mut j = Mem(Some(committed(&o, "t").unwrap()), n);
            let first = run_to_end(&mut img, &o, &mut j, &mut FakeOps::default());
            j.1 = usize::MAX;
            let st = match first {
                Ok(s) => s,
                Err(_) => run_to_end(&mut img, &o, &mut j, &mut FakeOps::default()).unwrap(),
            };
            assert_eq!(st["phase"], "finished", "cut after {} saves", n);
            assert_eq!(img.into_inner(), want, "cut after {} saves", n);
        }
        // a step that fails (a tool, the ESP) is run again on the next start
        for step in ["guard", "grow", "check", "finish"] {
            let mut img = img0.clone();
            let mut j = Mem(Some(committed(&o, "t").unwrap()), usize::MAX);
            let mut ops = FakeOps { fail_once: Some(step), ..Default::default() };
            assert!(run_to_end(&mut img, &o, &mut j, &mut ops).is_err());
            let st = run_to_end(&mut img, &o, &mut j, &mut ops).unwrap();
            assert_eq!(st["phase"], "finished");
            let mut got = vec![0u8; data.len()];
            img.seek(SeekFrom::Start(4096 * 512)).unwrap();
            img.read_exact(&mut got).unwrap();
            assert_eq!(got, data, "after {} failed once", step);
        }
    }

    #[test]
    fn a_run_refuses_a_journal_for_another_reclaim_and_no_journal() {
        let (mut img, t, _) = disk();
        let o = small_order(&t);
        let mut j = Mem(None, usize::MAX);
        assert!(run_to_end(&mut img, &o, &mut j, &mut FakeOps::default()).unwrap_err().contains("never committed"));
        let mut j = Mem(Some(json!({ "id": "other", "phase": "committed" })), usize::MAX);
        assert!(run_to_end(&mut img, &o, &mut j, &mut FakeOps::default()).unwrap_err().contains("another reclaim"));
    }

    #[test]
    fn the_file_journal_survives_a_torn_write() {
        let dir = std::env::temp_dir().join(format!("upg-reclaim-journal-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut j = FileJournal { dir: dir.to_string_lossy().to_string() };
        assert_eq!(j.load().unwrap(), None);
        j.save(&json!({ "phase": "committed" })).unwrap();
        j.save(&json!({ "phase": "guarded" })).unwrap();
        assert_eq!(j.load().unwrap().unwrap()["phase"], "guarded");
        // the newer copy is cut half way: the older one is taken
        let newer = dir.join("journal-b.json");
        let t = std::fs::read_to_string(&newer).unwrap();
        std::fs::write(&newer, &t[..t.len() / 2]).unwrap();
        assert_eq!(j.load().unwrap().unwrap()["phase"], "committed");
        // and the next save goes over the broken one, not the whole one
        j.save(&json!({ "phase": "guarded" })).unwrap();
        assert_eq!(j.load().unwrap().unwrap()["phase"], "guarded");
        // both broken: refused, never taken as "not started"
        std::fs::write(dir.join("journal-a.json"), "{").unwrap();
        std::fs::write(dir.join("journal-b.json"), "{").unwrap();
        assert!(j.load().unwrap_err().contains("damaged"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn windows_startup_is_removed_only_where_linux_does_not_start_through_it() {
        let shim = Some("aa".repeat(32));
        let ms = Some("bb".repeat(32));
        let old_shim = Some("cc".repeat(32));
        let c = |slot: &Option<String>, kept: &Option<String>, own: bool| esp_cleanup(&EspLook { slot: slot.clone(), kept: kept.clone(), snapshot_slot: ms.clone() }, own);
        // the Aspire: shim in Windows' slot, Windows' loader kept beside it
        let x = c(&shim, &ms, true);
        assert_eq!((x["remove_microsoft_folder"].clone(), x["remove_windows_entries"].clone(), x["remove_kept_loader"].clone(), x["hold_slot"].clone()), (json!(false), json!(false), json!(true), json!(false)));
        // the second pass of a finish (the kept loader already gone), or an old
        // shim no longer equal to today's: still not Windows' own, nothing more goes
        for slot in [&shim, &old_shim] {
            let x = c(slot, &None, true);
            assert_eq!(x["remove_microsoft_folder"], false);
            assert_eq!(x["remove_windows_entries"], false);
            assert_eq!(x["remove_kept_loader"], false);
            assert_eq!(x["hold_slot"], false);
        }
        // Windows' own loader (the snapshot's bytes), Linux started from its own entry
        let x = c(&ms, &None, true);
        assert_eq!((x["remove_microsoft_folder"].clone(), x["remove_windows_entries"].clone(), x["hold_slot"].clone()), (json!(true), json!(true), json!(true)));
        // Windows' own loader, but this Linux did not start from its own entry: held, then put back
        let x = c(&ms, &None, false);
        assert_eq!((x["remove_microsoft_folder"].clone(), x["remove_windows_entries"].clone(), x["hold_slot"].clone()), (json!(false), json!(false), json!(true)));
        // no snapshot to compare with: never Windows' own
        let x = esp_cleanup(&EspLook { slot: ms.clone(), kept: None, snapshot_slot: None }, true);
        assert_eq!(x["remove_microsoft_folder"], false);
    }

    #[test]
    fn while_linux_moves_the_stick_is_first_and_windows_out() {
        assert_eq!(guard_boot_order(&[0x0001, 0x0000, 0x0003], 0x0007, &[0x0000], true), vec![0x0007, 0x0001, 0x0003]);
        // shim in Windows' slot: Windows' entry is Linux's way in, it stays
        assert_eq!(guard_boot_order(&[0x0000, 0x0007], 0x0007, &[0x0000], false), vec![0x0007, 0x0000]);
    }

    #[test]
    fn partitions_are_told_from_other_disks() {
        assert!(live::is_partition_of("sda1", "sda"));
        assert!(!live::is_partition_of("sdaa1", "sda"));
        assert!(!live::is_partition_of("sda", "sda"));
        assert!(live::is_partition_of("nvme0n1p2", "nvme0n1"));
        assert!(!live::is_partition_of("nvme0n12", "nvme0n1"));
    }

    #[test]
    fn a_resume_refuses_a_table_it_did_not_leave() {
        let (mut img, t, _) = disk();
        let o = small_order(&t);
        let after = new_table(&t, &o["plan"]).unwrap();
        // committed, but the table was changed since (Linux started in between, say)
        let mut changed = t.clone();
        changed.entries[4].last_lba -= 1;
        crate::gpttable::write(&mut img, &changed, &mut |_| Ok(())).unwrap();
        let mut j = Mem(Some(committed(&o, "t").unwrap()), usize::MAX);
        let mut ops = FakeOps::default();
        assert!(run_to_end(&mut img, &o, &mut j, &mut ops).unwrap_err().contains("not what this reclaim left"));
        assert!(ops.calls.is_empty(), "nothing ran");
        // the rules, phase by phase
        assert!(table_fits("guarded", &Ok(t.clone()), &t, &after).is_ok());
        assert!(table_fits("guarded", &Ok(after.clone()), &t, &after).is_err());
        assert!(table_fits("moved", &Err("torn".into()), &t, &after).is_ok());
        assert!(table_fits("moved", &Ok(changed.clone()), &t, &after).is_err());
        assert!(table_fits("grown", &Ok(after.clone()), &t, &after).is_ok());
        assert!(table_fits("grown", &Ok(t.clone()), &t, &after).is_err());
    }

    /// The real thing at its real size: a plan made by reclaim::plan, a
    /// 64 MiB chunk, a 140 MiB sparse disk file. Windows of 70 MiB, /boot 8,
    /// / 48. The stick's checks pass, the run moves, writes the table, and
    /// every byte of Linux is where the new table says.
    #[test]
    fn a_real_plan_at_the_real_chunk_size() {
        let path = std::env::temp_dir().join(format!("upg-reclaim-real-{}.img", std::process::id()));
        let sectors = 140 * M;
        let entries = vec![
            entry(0, ESP, 0x11, M, 3 * M - 1),
            entry(1, MSR, 0x22, 3 * M, 4 * M - 1),
            entry(2, MSDATA, W, 4 * M, 74 * M - 1),
            entry(3, LINUX_DATA, 0x55, 74 * M, 82 * M - 1),
            entry(4, LINUX_DATA, 0x66, 82 * M, 130 * M - 1),
        ];
        let (_, mut t) = crate::gpttable::tests::blank(sectors);
        t.entries = entries;
        let mut f = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(true).open(&path).unwrap();
        f.set_len(sectors * 512).unwrap();
        crate::gpttable::write(&mut f, &t, &mut |_| Ok(())).unwrap();
        let len = (130 - 74) * M * 512;
        let data: Vec<u8> = (0..len).map(|i| ((i / 4096) as u8) ^ (i as u8).wrapping_mul(31)).collect();
        f.seek(SeekFrom::Start(74 * M * 512)).unwrap();
        f.write_all(&data).unwrap();
        let o = order_for(&t, 70 * M * 512);
        assert_eq!(o["plan"]["result"], "plan", "{}", o["plan"]);
        assert_eq!(precheck(&o, &onstick(&t)), Vec::<String>::new());
        let mut j = Mem(Some(committed(&o, "t").unwrap()), usize::MAX);
        let st = execute(&mut f, &o, &mut j, &mut FakeOps::default(), CHUNK, &mut |f: &mut std::fs::File| f.sync_all().map_err(|e| e.to_string())).unwrap();
        assert_eq!(st["phase"], "finished");
        let back = crate::gpttable::read(&mut f, 512, sectors * 512).unwrap();
        assert_eq!(back, new_table(&t, &o["plan"]).unwrap());
        let boot = back.entries.iter().find(|e| e.index == 3).unwrap();
        assert_eq!(boot.first_lba, 3 * M);
        let root = back.entries.iter().find(|e| e.index == 4).unwrap();
        assert!(root.last_lba > 130 * M - 1, "/ reaches into the end of the disk");
        let mut got = vec![0u8; len as usize];
        f.seek(SeekFrom::Start(3 * M * 512)).unwrap();
        f.read_exact(&mut got).unwrap();
        assert!(got == data, "every byte of /boot and / moved");
        drop(f);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn the_boot_order_is_the_old_one_without_the_stick_and_windows() {
        assert_eq!(final_boot_order(&[0x0001, 0x0003, 0x0007, 0x0000], &[0x0007, 0x0000]), vec![0x0001, 0x0003]);
        assert_eq!(final_boot_order(&[2, 1], &[]), vec![2, 1]);
    }

    #[test]
    fn grub_cfg_loses_only_the_kept_entry() {
        let cfg = format!("### BEGIN /etc/grub.d/10_linux ###\nblscfg\n### END /etc/grub.d/10_linux ###\n\n### BEGIN /etc/grub.d/25_windows_kept ###\n{}### END /etc/grub.d/25_windows_kept ###\n\n### BEGIN /etc/grub.d/30_os-prober ###\n### END /etc/grub.d/30_os-prober ###\n",
            "menuentry 'Windows (kept)' --class windows --class os {\n    chainloader /EFI/Microsoft/Boot/bootmgfw.efi\n}\n");
        let out = without_kept_block(&cfg).unwrap();
        assert!(!out.contains("Windows (kept)"));
        assert!(out.contains("blscfg") && out.contains("30_os-prober"));
        assert_eq!(without_kept_block(&out), None);
    }

    #[test]
    fn the_stub_is_known_again_and_is_not_a_linux_start() {
        let s = stub_grub_cfg();
        assert!(is_stub(&s));
        assert!(!s.contains("configfile") && !s.contains("linux") && !s.contains("blscfg"));
        assert!(!is_stub("search --fs-uuid --set=dev abc\nconfigfile $prefix/grub.cfg\n"));
    }

    #[test]
    fn the_words_name_the_folders_and_the_last_click() {
        let p = json!({ "deleted_bytes": 120_000_000_000u64, "folders_lost": [ { "name": "Pictures", "files": 812, "bytes": 3_100_000_000u64 } ] });
        let w = words(&p);
        assert_eq!(w["what"]["folders"][0], "Pictures: 812 files, 3.1 GB");
        assert_eq!(w["consent"]["sentence"], reclaim::SENTENCE);
        assert!(w["ready"]["lines"][0].as_str().unwrap().contains("no turning back"));
        assert_eq!(w["what"]["heading"], "Linux gets 120 GB more room.");
    }
}
