//! A whole GPT partition table, read and written (RISKS R41, reclaim). The
//! reader in `gpt.rs` only needs each partition's type and id; reclaim needs
//! where each partition starts and ends, and has to write a new table. This
//! is the only code in settle-in that writes a partition table, so it is
//! held to the writers' bar (rule #4):
//!
//!   - a table is trusted only when both copies (the primary at the front of
//!     the disk, the backup at the end) pass their CRC checks and agree;
//!     anything else is a refusal, never a repair;
//!   - a new table keeps the disk's id, the entry count and size, and every
//!     partition's own id, so nothing that names a partition by its id
//!     (firmware entries, fstab by PARTUUID) loses it;
//!   - writing goes backup first, then primary, then reads both back.

use std::io::{Read, Seek, SeekFrom, Write};

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub index: usize, // 0-based slot in the entry array; partition number = index + 1
    pub type_guid: [u8; 16],
    pub unique: [u8; 16],
    pub first_lba: u64,
    pub last_lba: u64,
    pub attributes: u64,
    pub name: [u8; 72], // UTF-16LE, as stored
}

impl Entry {
    pub fn number(&self) -> u32 {
        self.index as u32 + 1
    }
    pub fn sectors(&self) -> u64 {
        self.last_lba - self.first_lba + 1
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub sector: u64,
    pub disk_sectors: u64,
    pub first_usable: u64,
    pub last_usable: u64,
    pub disk_guid: [u8; 16],
    pub entry_count: u32,
    pub entry_size: u32,
    pub entries: Vec<Entry>, // the used slots only, in slot order
}

/// CRC-32 as GPT uses it (IEEE 802.3, reflected, init and xorout 0xFFFFFFFF).
pub fn crc32(data: &[u8]) -> u32 {
    let mut c: u32 = 0xFFFF_FFFF;
    for &b in data {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 };
        }
    }
    !c
}

fn u32le(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn u64le(b: &[u8], o: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[o..o + 8]);
    u64::from_le_bytes(a)
}

fn read_at<R: Read + Seek>(r: &mut R, at: u64, n: usize) -> Result<Vec<u8>, String> {
    let mut v = vec![0u8; n];
    r.seek(SeekFrom::Start(at)).map_err(|e| e.to_string())?;
    r.read_exact(&mut v).map_err(|e| format!("read at byte {}: {}", at, e))?;
    Ok(v)
}

fn entry_sectors(count: u32, size: u32, sector: u64) -> u64 {
    (count as u64 * size as u64).div_ceil(sector)
}

/// One header and its entry array, checked. `lba` is where the header is.
fn read_copy<R: Read + Seek>(r: &mut R, sector: u64, lba: u64, which: &str) -> Result<(Vec<u8>, Vec<u8>), String> {
    let hdr = read_at(r, lba * sector, sector as usize)?;
    if &hdr[0..8] != b"EFI PART" {
        return Err(format!("the {} partition table is missing", which));
    }
    let hsize = u32le(&hdr, 12) as usize;
    if !(92..=sector as usize).contains(&hsize) {
        return Err(format!("the {} table's header size {} is not plausible", which, hsize));
    }
    let mut h = hdr[..hsize].to_vec();
    let want = u32le(&h, 16);
    h[16..20].copy_from_slice(&[0, 0, 0, 0]);
    if crc32(&h) != want {
        return Err(format!("the {} table's header fails its check (CRC)", which));
    }
    if u64le(&hdr, 24) != lba {
        return Err(format!("the {} table says it is somewhere else on the disk", which));
    }
    let count = u32le(&hdr, 80);
    let size = u32le(&hdr, 84);
    if !(1..=1024).contains(&count) || !(128..=4096).contains(&size) || size % 8 != 0 {
        return Err(format!("the {} table's entry count {} or size {} is not plausible", which, count, size));
    }
    let ents = read_at(r, u64le(&hdr, 72) * sector, (count as u64 * size as u64) as usize)?;
    if crc32(&ents) != u32le(&hdr, 88) {
        return Err(format!("the {} table's partition list fails its check (CRC)", which));
    }
    Ok((hdr, ents))
}

/// The table of one disk. `disk_bytes` is the disk's size. Refuses unless
/// both copies are whole and say the same thing.
pub fn read<R: Read + Seek>(r: &mut R, sector: u64, disk_bytes: u64) -> Result<Table, String> {
    if sector < 512 || disk_bytes % sector != 0 || disk_bytes / sector < 68 {
        return Err("the disk's size or sector size is not plausible".into());
    }
    let disk_sectors = disk_bytes / sector;
    let (ph, pe) = read_copy(r, sector, 1, "primary")?;
    let alt = u64le(&ph, 32);
    if alt != disk_sectors - 1 {
        return Err(format!("the primary table puts its backup at sector {}, not at the disk's end ({}): the disk may have changed size", alt, disk_sectors - 1));
    }
    let (bh, be) = read_copy(r, sector, alt, "backup")?;
    if pe != be {
        return Err("the primary and backup partition lists disagree".into());
    }
    for (o, n, what) in [(40usize, 8usize, "first usable sector"), (48, 8, "last usable sector"), (56, 16, "disk id"), (80, 4, "entry count"), (84, 4, "entry size")] {
        if ph[o..o + n] != bh[o..o + n] {
            return Err(format!("the primary and backup tables disagree on the {}", what));
        }
    }
    let count = u32le(&ph, 80);
    let size = u32le(&ph, 84);
    let (first_usable, last_usable) = (u64le(&ph, 40), u64le(&ph, 48));
    let mut disk_guid = [0u8; 16];
    disk_guid.copy_from_slice(&ph[56..72]);
    let mut entries = Vec::new();
    for i in 0..count as usize {
        let e = &pe[i * size as usize..i * size as usize + 128];
        let mut type_guid = [0u8; 16];
        type_guid.copy_from_slice(&e[0..16]);
        if type_guid == [0u8; 16] {
            continue;
        }
        let mut unique = [0u8; 16];
        unique.copy_from_slice(&e[16..32]);
        let mut name = [0u8; 72];
        name.copy_from_slice(&e[56..128]);
        let (first_lba, last_lba) = (u64le(e, 32), u64le(e, 40));
        if first_lba < first_usable || last_lba > last_usable || last_lba < first_lba {
            return Err(format!("partition {} lies outside the disk's usable space", i + 1));
        }
        entries.push(Entry { index: i, type_guid, unique, first_lba, last_lba, attributes: u64le(e, 48), name });
    }
    let t = Table { sector, disk_sectors, first_usable, last_usable, disk_guid, entry_count: count, entry_size: size, entries };
    t.check()?;
    Ok(t)
}

impl Table {
    /// No two partitions overlap, every slot is used once.
    pub fn check(&self) -> Result<(), String> {
        let mut v: Vec<&Entry> = self.entries.iter().collect();
        v.sort_by_key(|e| e.first_lba);
        for w in v.windows(2) {
            if w[1].first_lba <= w[0].last_lba {
                return Err(format!("partitions {} and {} overlap", w[0].number(), w[1].number()));
            }
        }
        let mut idx: Vec<usize> = self.entries.iter().map(|e| e.index).collect();
        idx.sort();
        idx.dedup();
        if idx.len() != self.entries.len() || idx.iter().any(|&i| i >= self.entry_count as usize) {
            return Err("a partition slot is used twice or lies past the list".into());
        }
        for e in &self.entries {
            if e.first_lba < self.first_usable || e.last_lba > self.last_usable || e.last_lba < e.first_lba {
                return Err(format!("partition {} lies outside the disk's usable space", e.number()));
            }
        }
        Ok(())
    }

    fn entry_array(&self) -> Vec<u8> {
        let size = self.entry_size as usize;
        let mut a = vec![0u8; self.entry_count as usize * size];
        for e in &self.entries {
            let o = e.index * size;
            a[o..o + 16].copy_from_slice(&e.type_guid);
            a[o + 16..o + 32].copy_from_slice(&e.unique);
            a[o + 32..o + 40].copy_from_slice(&e.first_lba.to_le_bytes());
            a[o + 40..o + 48].copy_from_slice(&e.last_lba.to_le_bytes());
            a[o + 48..o + 56].copy_from_slice(&e.attributes.to_le_bytes());
            a[o + 56..o + 128].copy_from_slice(&e.name);
        }
        a
    }

    fn header(&self, my: u64, alt: u64, entries_lba: u64, ents_crc: u32) -> Vec<u8> {
        let mut h = vec![0u8; self.sector as usize];
        h[0..8].copy_from_slice(b"EFI PART");
        h[8..12].copy_from_slice(&0x0001_0000u32.to_le_bytes());
        h[12..16].copy_from_slice(&92u32.to_le_bytes());
        h[24..32].copy_from_slice(&my.to_le_bytes());
        h[32..40].copy_from_slice(&alt.to_le_bytes());
        h[40..48].copy_from_slice(&self.first_usable.to_le_bytes());
        h[48..56].copy_from_slice(&self.last_usable.to_le_bytes());
        h[56..72].copy_from_slice(&self.disk_guid);
        h[72..80].copy_from_slice(&entries_lba.to_le_bytes());
        h[80..84].copy_from_slice(&self.entry_count.to_le_bytes());
        h[84..88].copy_from_slice(&self.entry_size.to_le_bytes());
        h[88..92].copy_from_slice(&ents_crc.to_le_bytes());
        let c = crc32(&h[..92]);
        h[16..20].copy_from_slice(&c.to_le_bytes());
        h
    }
}

/// Write `t` as the disk's table: backup copy first, then the primary, each
/// flushed; then both read back and compared. `sync` makes a write durable
/// (`File::sync_all` on a device; nothing on a test image).
pub fn write<D: Read + Write + Seek>(d: &mut D, t: &Table, sync: &mut dyn FnMut(&mut D) -> Result<(), String>) -> Result<(), String> {
    t.check()?;
    let ents = t.entry_array();
    let ec = crc32(&ents);
    let es = entry_sectors(t.entry_count, t.entry_size, t.sector);
    let last = t.disk_sectors - 1;
    let backup_entries = last - es;
    if t.last_usable >= backup_entries || t.first_usable < 2 + es {
        return Err("the table's usable space overlaps its own lists".into());
    }
    let put = |d: &mut D, lba: u64, bytes: &[u8]| -> Result<(), String> {
        d.seek(SeekFrom::Start(lba * t.sector)).map_err(|e| e.to_string())?;
        d.write_all(bytes).map_err(|e| format!("write at sector {}: {}", lba, e))
    };
    put(d, backup_entries, &ents)?;
    put(d, last, &t.header(last, 1, backup_entries, ec))?;
    sync(d)?;
    put(d, 2, &ents)?;
    put(d, 1, &t.header(1, last, 2, ec))?;
    sync(d)?;
    let back = read(d, t.sector, t.disk_sectors * t.sector)?;
    if &back != t {
        return Err("the table read back is not the table written".into());
    }
    Ok(())
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::gpt::guid_bytes;

    pub const MSDATA: &str = "ebd0a0a2-b9e5-4433-87c0-68b6b72699c7";

    /// A table alone, with no image behind it (for plans on large disks).
    pub fn table(sectors: u64, entries: Vec<Entry>) -> Table {
        let es = entry_sectors(128, 128, 512);
        let t = Table {
            sector: 512,
            disk_sectors: sectors,
            first_usable: 2 + es,
            last_usable: sectors - 2 - es,
            disk_guid: guid_bytes("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"),
            entry_count: 128,
            entry_size: 128,
            entries,
        };
        t.check().unwrap();
        t
    }

    /// A blank disk image with an empty table: 512-byte sectors, 128 entries.
    pub fn blank(sectors: u64) -> (Vec<u8>, Table) {
        let es = entry_sectors(128, 128, 512);
        let t = Table {
            sector: 512,
            disk_sectors: sectors,
            first_usable: 2 + es,
            last_usable: sectors - 2 - es,
            disk_guid: guid_bytes("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"),
            entry_count: 128,
            entry_size: 128,
            entries: vec![],
        };
        (vec![0u8; (sectors * 512) as usize], t)
    }

    pub fn entry(index: usize, ty: &str, uniq: u8, first: u64, last: u64) -> Entry {
        Entry { index, type_guid: guid_bytes(ty), unique: [uniq; 16], first_lba: first, last_lba: last, attributes: 0, name: [0u8; 72] }
    }

    pub fn image(sectors: u64, entries: Vec<Entry>) -> (std::io::Cursor<Vec<u8>>, Table) {
        let (img, mut t) = blank(sectors);
        t.entries = entries;
        let mut c = std::io::Cursor::new(img);
        write(&mut c, &t, &mut |_| Ok(())).unwrap();
        (c, t)
    }

    #[test]
    fn crc_known_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn writes_and_reads_back() {
        let (mut c, t) = image(4096, vec![entry(0, MSDATA, 1, 2048, 3000), entry(2, MSDATA, 3, 3001, 3500)]);
        let back = read(&mut c, 512, 4096 * 512).unwrap();
        assert_eq!(back, t);
        assert_eq!(back.entries[1].number(), 3);
    }

    #[test]
    fn refuses_a_damaged_or_disagreeing_table() {
        let (c, _) = image(4096, vec![entry(0, MSDATA, 1, 2048, 3000)]);
        let mut img = c.into_inner();
        // the primary list damaged
        let mut bad = img.clone();
        bad[1024 + 40] ^= 1;
        assert!(read(&mut std::io::Cursor::new(bad), 512, 4096 * 512).unwrap_err().contains("primary"));
        // the backup header damaged
        let mut bad = img.clone();
        bad[4095 * 512 + 40] ^= 1;
        assert!(read(&mut std::io::Cursor::new(bad), 512, 4096 * 512).unwrap_err().contains("backup"));
        // a disk that grew: the backup is no longer at the end
        img.extend(vec![0u8; 512 * 8]);
        assert!(read(&mut std::io::Cursor::new(img), 512, 4104 * 512).unwrap_err().contains("changed size"));
    }

    #[test]
    fn refuses_to_write_overlaps() {
        let (img, mut t) = blank(4096);
        t.entries = vec![entry(0, MSDATA, 1, 2048, 3000), entry(1, MSDATA, 2, 3000, 3500)];
        assert!(write(&mut std::io::Cursor::new(img), &t, &mut |_| Ok(())).unwrap_err().contains("overlap"));
    }

    /// The table this writes is one sgdisk (gdisk's scriptable form) accepts
    /// as whole, when sgdisk is installed. An outside check of our reading
    /// of the format, not of any machine (rule #5).
    #[test]
    fn sgdisk_agrees_when_present() {
        if std::process::Command::new("sgdisk").arg("--version").output().is_err() {
            return;
        }
        let (c, _) = image(8192, vec![entry(0, MSDATA, 1, 2048, 4095), entry(1, "0fc63daf-8483-4772-8e79-3d69d8477de4", 2, 4096, 8000)]);
        let dir = std::env::temp_dir().join(format!("settle-in-gpt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("disk.img");
        std::fs::write(&f, c.into_inner()).unwrap();
        let o = std::process::Command::new("sgdisk").arg("-v").arg(&f).output().unwrap();
        let text = String::from_utf8_lossy(&o.stdout).to_string();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(text.contains("No problems found"), "sgdisk -v: {}", text);
    }
}
