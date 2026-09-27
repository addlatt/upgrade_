//! The disks' partition tables, read straight from the disks (GPT, the
//! partition layout UEFI machines use): each partition's type and its own
//! unique id. No udev, no blkid - the kernel's block devices, the same on
//! every distribution. Read-only.

use std::io::{Read, Seek, SeekFrom};

pub const ESP_TYPE: [u8; 16] = guid_bytes("c12a7328-f81f-11d2-ba4b-00a0c93ec93b");

#[derive(Debug, Clone, PartialEq)]
pub struct Partition {
    pub disk: String,
    pub number: u32,
    pub type_guid: [u8; 16],
    /// As stored on disk (mixed-endian), the same bytes a firmware boot
    /// entry's hard-drive node carries, so the two compare directly.
    pub unique: [u8; 16],
}

/// "c12a7328-f81f-11d2-ba4b-00a0c93ec93b" -> the 16 bytes as GPT stores them.
pub const fn guid_bytes(s: &str) -> [u8; 16] {
    let b = s.as_bytes();
    const fn hex(c: u8) -> u8 {
        match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            _ => panic!("not hex"),
        }
    }
    const fn byte(b: &[u8], i: usize) -> u8 {
        hex(b[i]) << 4 | hex(b[i + 1])
    }
    // text order -> the first three groups little-endian, the rest as written
    let pos: [usize; 16] = [6, 4, 2, 0, 11, 9, 16, 14, 19, 21, 24, 26, 28, 30, 32, 34];
    let mut out = [0u8; 16];
    let mut i = 0;
    while i < 16 {
        out[i] = byte(b, pos[i]);
        i += 1;
    }
    out
}

pub fn guid_text(g: &[u8; 16]) -> String {
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        g[3], g[2], g[1], g[0], g[5], g[4], g[7], g[6], g[8], g[9], g[10], g[11], g[12], g[13], g[14], g[15]
    )
}

fn u32le(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn u64le(b: &[u8], o: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[o..o + 8]);
    u64::from_le_bytes(a)
}

/// The partitions of one disk image or device. `sector` is its logical block size.
pub fn read_disk<R: Read + Seek>(disk: &str, r: &mut R, sector: u64) -> Result<Vec<Partition>, String> {
    let mut hdr = vec![0u8; sector as usize];
    r.seek(SeekFrom::Start(sector)).map_err(|e| e.to_string())?;
    r.read_exact(&mut hdr).map_err(|e| format!("{}: {}", disk, e))?;
    if &hdr[0..8] != b"EFI PART" {
        return Ok(Vec::new()); // not GPT: nothing a UEFI boot entry can point into
    }
    let entries_lba = u64le(&hdr, 72);
    let count = u32le(&hdr, 80).min(1024) as usize;
    let size = u32le(&hdr, 84) as usize;
    if !(128..=4096).contains(&size) {
        return Err(format!("{}: partition entry size {} is not plausible", disk, size));
    }
    let mut t = vec![0u8; count * size];
    r.seek(SeekFrom::Start(entries_lba * sector)).map_err(|e| e.to_string())?;
    r.read_exact(&mut t).map_err(|e| format!("{}: {}", disk, e))?;
    let mut out = Vec::new();
    for i in 0..count {
        let e = &t[i * size..i * size + 128];
        let mut ty = [0u8; 16];
        ty.copy_from_slice(&e[0..16]);
        if ty == [0u8; 16] {
            continue;
        }
        let mut un = [0u8; 16];
        un.copy_from_slice(&e[16..32]);
        out.push(Partition { disk: disk.to_string(), number: i as u32 + 1, type_guid: ty, unique: un });
    }
    Ok(out)
}

/// Every whole disk the kernel lists (not partitions, not loop or ram devices).
pub fn all_partitions(root: &str) -> Result<Vec<Partition>, String> {
    let r = root.trim_end_matches('/');
    let mut out = Vec::new();
    let rd = std::fs::read_dir(format!("{}/sys/block", r)).map_err(|e| format!("the disk list could not be read ({})", e))?;
    let mut names: Vec<String> = rd.flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
    names.sort();
    for n in names {
        if n.starts_with("loop") || n.starts_with("ram") || n.starts_with("zram") || n.starts_with("dm-") || n.starts_with("sr") {
            continue;
        }
        let sector = std::fs::read_to_string(format!("{}/sys/block/{}/queue/logical_block_size", r, n))
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .unwrap_or(512);
        let dev = format!("{}/dev/{}", r, n);
        let mut f = std::fs::File::open(&dev).map_err(|e| format!("{} could not be read ({}) - every disk must be readable to know which boot entries are stale", dev, e))?;
        out.extend(read_disk(&n, &mut f, sector)?);
    }
    Ok(out)
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A minimal GPT disk image: header at LBA 1, entries at LBA 2.
    pub fn image(parts: &[([u8; 16], [u8; 16])]) -> Vec<u8> {
        let mut d = vec![0u8; 512 * 40];
        d[512..520].copy_from_slice(b"EFI PART");
        d[512 + 72..512 + 80].copy_from_slice(&2u64.to_le_bytes());
        d[512 + 80..512 + 84].copy_from_slice(&128u32.to_le_bytes());
        d[512 + 84..512 + 88].copy_from_slice(&128u32.to_le_bytes());
        for (i, (t, u)) in parts.iter().enumerate() {
            let o = 1024 + i * 128;
            d[o..o + 16].copy_from_slice(t);
            d[o + 16..o + 32].copy_from_slice(u);
        }
        d
    }

    #[test]
    fn guid_text_round_trips() {
        assert_eq!(guid_text(&ESP_TYPE), "c12a7328-f81f-11d2-ba4b-00a0c93ec93b");
        assert_eq!(ESP_TYPE[0..4], [0x28, 0x73, 0x2a, 0xc1]);
    }

    #[test]
    fn reads_partitions() {
        let u1 = guid_bytes("11111111-2222-3333-4444-555555555555");
        let img = image(&[(ESP_TYPE, u1), (guid_bytes("0fc63daf-8483-4772-8e79-3d69d8477de4"), [7u8; 16])]);
        let p = read_disk("vda", &mut std::io::Cursor::new(img), 512).unwrap();
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].type_guid, ESP_TYPE);
        assert_eq!(p[0].unique, u1);
        assert_eq!(p[1].number, 2);
        assert!(read_disk("x", &mut std::io::Cursor::new(vec![0u8; 4096]), 512).unwrap().is_empty());
    }
}
