//! Our own read-only NTFS reader, used only inside OneDrive folders (RISKS
//! R43; decided 2026-11-07, the owner).
//!
//! ntfs-3g shows a folder carrying a Cloud Files reparse tag (every folder
//! OneDrive keeps) as a broken link, and nothing inside it opens, not even a
//! file that is fully on the disk (v3-pull.csv line 3). This reader goes past
//! the tag: it reads the volume itself (the `ntfs` crate, read-only by
//! construction: it is only ever handed a reader), walks folder indexes, and
//! reads a file's bytes straight from its `$DATA` attribute.
//!
//! It tells the pull the same facts ntfs-3g would: the file attributes as
//! Windows stored them (raw, so the recall-on-data-access bit survives), the
//! reparse tag, the length and how many of its bytes are really on the disk
//! (a sparse run is a hole: a OneDrive file partly downloaded has some). Two
//! things it does not do, and says so instead of guessing: unpack a file
//! Windows compressed or encrypted (its bytes on disk are not the file's), and
//! count the on-disk bytes of a file whose data is spread over an attribute
//! list (rare: very fragmented files).

use ntfs::indexes::NtfsFileNameIndex;
use ntfs::structured_values::NtfsFileNamespace;
use ntfs::{Ntfs, NtfsAttributeFlags, NtfsAttributeType, NtfsFile, NtfsReadSeek};
use std::cell::RefCell;
use std::io::{BufReader, Read, Seek};

/// 100 ns ticks from 1601 to 1970.
const NT_EPOCH: u64 = 116_444_736_000_000_000;

/// One entry, as the volume records it.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub name: String,
    pub dir: bool,
    pub len: u64,
    /// Bytes of the data that are on the disk (not holes); `None` when it
    /// could not be counted.
    pub on_disk: Option<u64>,
    /// `$STANDARD_INFORMATION`'s file attributes, every bit kept, plus the
    /// directory bit for a folder (as ntfs-3g's `system.ntfs_attrib` shows).
    pub attrib: u32,
    pub reparse_tag: Option<u32>,
    /// The data is compressed or encrypted by Windows: its bytes on disk are not the file's.
    pub packed: bool,
    /// Last write and last access, seconds and nanoseconds since 1970.
    pub mtime: (i64, i64),
    pub atime: (i64, i64),
}

pub trait Device: Read + Seek {}
impl<T: Read + Seek> Device for T {}

/// A volume, opened for reading only.
pub struct Volume {
    fs: RefCell<BufReader<Box<dyn Device>>>,
    ntfs: Ntfs,
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn unix(t: u64) -> (i64, i64) {
    let d = t.saturating_sub(NT_EPOCH);
    ((d / 10_000_000) as i64, ((d % 10_000_000) * 100) as i64)
}

impl Volume {
    /// Open a device or an image file. Nothing is ever written: the file is opened read-only.
    pub fn open(path: &std::path::Path) -> Result<Volume, String> {
        let f = std::fs::File::open(path).map_err(|e| format!("{}: {}", path.display(), e))?;
        Volume::from_reader(Box::new(f))
    }

    pub fn from_reader(r: Box<dyn Device>) -> Result<Volume, String> {
        let mut fs = BufReader::with_capacity(64 * 1024, r);
        let mut ntfs = Ntfs::new(&mut fs).map_err(err)?;
        ntfs.read_upcase_table(&mut fs).map_err(err)?;
        Ok(Volume { fs: RefCell::new(fs), ntfs })
    }

    /// The file at a volume-relative path (`Users/Ann/Documents`), found
    /// case-insensitively as Windows does. The empty path is the root.
    fn file(&self, rel: &str) -> Result<NtfsFile<'_>, String> {
        let mut fs = self.fs.borrow_mut();
        let mut f = self.ntfs.root_directory(&mut *fs).map_err(err)?;
        for part in rel.split('/').filter(|p| !p.is_empty()) {
            let next = {
                let index = f.directory_index(&mut *fs).map_err(|e| format!("{}: {}", rel, e))?;
                let mut finder = index.finder();
                let entry = NtfsFileNameIndex::find(&mut finder, &self.ntfs, &mut *fs, part)
                    .ok_or_else(|| format!("{}: {} is not there", rel, part))?
                    .map_err(err)?;
                entry.to_file(&self.ntfs, &mut *fs).map_err(err)?
            };
            f = next;
        }
        Ok(f)
    }

    fn describe(&self, f: &NtfsFile<'_>, name: String) -> Result<Entry, String> {
        let mut fs = self.fs.borrow_mut();
        let dir = f.is_directory();
        let mut si: Option<Vec<u8>> = None;
        let mut tag: Option<u32> = None;
        let mut attrs = f.attributes();
        while let Some(item) = attrs.next(&mut *fs) {
            let item = item.map_err(err)?;
            let a = item.to_attribute().map_err(err)?;
            match a.ty() {
                Ok(NtfsAttributeType::StandardInformation) => {
                    let mut v = a.value(&mut *fs).map_err(err)?;
                    let mut b = vec![0u8; v.len().min(72) as usize];
                    read_full(&mut v, &mut *fs, &mut b)?;
                    si = Some(b);
                }
                Ok(NtfsAttributeType::ReparsePoint) => {
                    let mut v = a.value(&mut *fs).map_err(err)?;
                    let mut b = [0u8; 4];
                    if v.len() >= 4 {
                        read_full(&mut v, &mut *fs, &mut b)?;
                        tag = Some(u32::from_le_bytes(b));
                    }
                }
                _ => {}
            }
        }
        let si = si.filter(|b| b.len() >= 0x24).ok_or_else(|| format!("{}: no standard information", name))?;
        let u64at = |o: usize| u64::from_le_bytes(si[o..o + 8].try_into().unwrap_or([0; 8]));
        let mut attrib = u32::from_le_bytes(si[0x20..0x24].try_into().unwrap_or([0; 4]));
        if dir {
            attrib |= 0x10;
        }
        let (mut len, mut on_disk, mut packed) = (0u64, Some(0u64), false);
        if !dir {
            if let Some(item) = f.data(&mut *fs, "") {
                let item = item.map_err(err)?;
                let a = item.to_attribute().map_err(err)?;
                packed = a.flags().intersects(NtfsAttributeFlags::COMPRESSED | NtfsAttributeFlags::ENCRYPTED);
                let v = a.value(&mut *fs).map_err(err)?;
                len = v.len();
                on_disk = match &v {
                    ntfs::attribute_value::NtfsAttributeValue::Resident(_) => Some(len),
                    ntfs::attribute_value::NtfsAttributeValue::NonResident(nr) => {
                        let mut n = 0u64;
                        for run in nr.data_runs() {
                            let run = run.map_err(err)?;
                            if run.data_position().value().is_some() {
                                n += run.allocated_size();
                            }
                        }
                        Some(n.min(len))
                    }
                    ntfs::attribute_value::NtfsAttributeValue::AttributeListNonResident(_) => None,
                };
            }
        }
        packed |= !dir && attrib & (0x800 | 0x4000) != 0;
        Ok(Entry { name, dir, len, on_disk, attrib, reparse_tag: tag, packed, mtime: unix(u64at(0x08)), atime: unix(u64at(0x18)) })
    }

    /// One entry, by its volume-relative path.
    pub fn stat(&self, rel: &str) -> Result<Entry, String> {
        let f = self.file(rel)?;
        let name = rel.rsplit('/').next().unwrap_or("").to_string();
        self.describe(&f, name)
    }

    /// A folder's entries, in the index's order, each long name once (an
    /// MS-DOS short name is a second name for the same file, and Windows does
    /// not list it; a hard link is listed under each of its names, as Windows
    /// lists it).
    pub fn list(&self, rel: &str) -> Result<Vec<Entry>, String> {
        let dirf = self.file(rel)?;
        let mut found: Vec<(String, NtfsFile<'_>)> = Vec::new();
        {
            let mut fs = self.fs.borrow_mut();
            let index = dirf.directory_index(&mut *fs).map_err(|e| format!("{}: {}", rel, e))?;
            let mut it = index.entries();
            while let Some(e) = it.next(&mut *fs) {
                let e = e.map_err(err)?;
                let Some(key) = e.key() else { continue };
                let key = key.map_err(err)?;
                if key.namespace() == NtfsFileNamespace::Dos {
                    continue;
                }
                let name = key.name().to_string_lossy();
                let f = e.to_file(&self.ntfs, &mut *fs).map_err(err)?;
                found.push((name, f));
            }
        }
        found.into_iter().map(|(n, f)| self.describe(&f, n)).collect()
    }

    /// Stream a file's bytes into `each`. Refuses a compressed or encrypted
    /// file (its bytes on disk are not the file's).
    pub fn read(&self, rel: &str, each: &mut dyn FnMut(&[u8]) -> std::io::Result<()>) -> Result<u64, String> {
        let f = self.file(rel)?;
        let mut fs = self.fs.borrow_mut();
        let item = f.data(&mut *fs, "").ok_or("it has no data")?.map_err(err)?;
        let a = item.to_attribute().map_err(err)?;
        if a.flags().intersects(NtfsAttributeFlags::COMPRESSED | NtfsAttributeFlags::ENCRYPTED) {
            return Err("Windows compressed or encrypted it, and this reader does not unpack that".into());
        }
        let mut v = a.value(&mut *fs).map_err(err)?;
        let mut buf = vec![0u8; 1 << 20];
        let mut total = 0u64;
        loop {
            let n = v.read(&mut *fs, &mut buf).map_err(err)?;
            if n == 0 {
                break;
            }
            each(&buf[..n]).map_err(err)?;
            total += n as u64;
        }
        Ok(total)
    }
}

fn read_full<V: NtfsReadSeek, T: Read + Seek>(v: &mut V, fs: &mut T, b: &mut [u8]) -> Result<(), String> {
    let mut got = 0;
    while got < b.len() {
        let n = v.read(fs, &mut b[got..]).map_err(err)?;
        if n == 0 {
            return Err("an attribute ended early".into());
        }
        got += n;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pull::{walk, Listing, Seen, Tree};
    use serde_json::Value;
    use sha2::{Digest, Sha256};

    const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/ntfs-fixture");

    /// The test disk Windows made (rig/hyperv/guest/ntfs-fixture.ps1), stored as its non-zero blocks.
    fn fixture() -> (Volume, Value) {
        let b = std::fs::read(format!("{}/volume.blocks", DIR)).expect("the reader's test disk (rig/hyperv/ntfs-fixture.py)");
        assert_eq!(&b[..8], b"UPGNTFS1");
        let mut vol = vec![0u8; u64::from_le_bytes(b[8..16].try_into().unwrap()) as usize];
        for rec in b[16..].chunks(4100) {
            let n = u32::from_le_bytes(rec[..4].try_into().unwrap()) as usize * 4096;
            vol[n..n + 4096].copy_from_slice(&rec[4..]);
        }
        let j: Value = serde_json::from_str(std::fs::read_to_string(format!("{}/fixture.json", DIR)).unwrap().trim_start_matches('\u{feff}')).unwrap();
        (Volume::from_reader(Box::new(std::io::Cursor::new(vol))).unwrap(), j)
    }

    /// Every folder read by our reader, as if all of it were a OneDrive folder.
    struct AllRaw<'a>(&'a Volume);
    impl Tree for AllRaw<'_> {
        fn list(&self, rel: &str) -> Result<Listing, String> {
            Ok(self.0.list(rel)?.into_iter().map(|e| (e.name.clone(), Ok((Seen::of_raw(&e), true)))).collect())
        }
    }

    fn hex(d: &[u8]) -> String {
        d.iter().map(|b| format!("{:02x}", b)).collect()
    }

    #[test]
    fn reads_what_windows_wrote() {
        let (v, j) = fixture();
        let docs = j["documents"].as_str().unwrap();
        let entries = j["entries"].as_array().unwrap();
        for e in entries {
            let rel = e["rel"].as_str().unwrap();
            let got = v.stat(&format!("{}/{}", docs, rel)).unwrap_or_else(|x| panic!("{}: {}", rel, x));
            let want = u32::from_str_radix(e["attributes"].as_str().unwrap().trim_start_matches("0x"), 16).unwrap();
            // FILE_ATTRIBUTE_NORMAL (0x80) is what Windows says when nothing else is set
            assert_eq!(got.attrib & !0x80, want & !0x80, "{} attributes", rel);
            assert_eq!(got.dir, e["dir"].as_bool().unwrap(), "{} kind", rel);
            if let Some(size) = e["size"].as_u64() {
                assert_eq!(got.len, size, "{} size", rel);
            }
        }
        // each name once, as Windows lists it: no MS-DOS short names, the hard link under both names
        let mut seen: Vec<String> = Vec::new();
        let mut stack = vec![docs.to_string()];
        while let Some(d) = stack.pop() {
            for e in v.list(&d).unwrap() {
                let p = format!("{}/{}", d, e.name);
                if e.dir && !e.reparse_tag.is_some_and(|t| t == 0xA000_0003 || t == 0xA000_000C) {
                    stack.push(p.clone());
                }
                seen.push(p[docs.len() + 1..].to_string());
            }
        }
        seen.sort();
        let mut want: Vec<String> = entries.iter().map(|e| e["rel"].as_str().unwrap().to_string()).collect();
        want.sort();
        assert_eq!(seen, want);
    }

    #[test]
    fn only_whole_files_come_over_and_byte_for_byte() {
        let (v, j) = fixture();
        let docs = j["documents"].as_str().unwrap();
        let w = walk(&AllRaw(&v), docs);
        let entries = j["entries"].as_array().unwrap();
        // the harvest's count: every entry that is not a folder (a junction is one), whatever its kind
        let files = entries.iter().filter(|e| !e["dir"].as_bool().unwrap()).count() as u64;
        assert_eq!(w.files(), files, "counted as the harvest counts");
        assert!(w.unreadable.is_empty() && w.unsure.is_empty(), "{:?} {:?}", w.unreadable, w.unsure);
        let copied: Vec<&str> = w.copy.iter().map(|(r, _)| r.as_str()).collect();
        for must in ["small.txt", "Taxes/return.pdf", "hard-link.pdf", "OneDrive/downloaded.pdf", "OneDrive/Sub/downloaded-too.txt"] {
            assert!(copied.contains(&must), "{} is copied ({:?})", must, copied);
        }
        for never in ["OneDrive/only-in-cloud.docx", "OneDrive/Sub/also-in-cloud.xlsx", "OneDrive/partly.bin", "compressed.txt", "OneDrive/compressed-in-onedrive.txt"] {
            assert!(!copied.contains(&never), "{} is not copied", never);
        }
        assert!(w.placeholders >= 2, "the two cloud-only files are placeholders");
        // every byte as Windows wrote it
        for (rel, size) in &w.copy {
            let e = entries.iter().find(|e| e["rel"] == rel.as_str()).unwrap();
            let mut h = Sha256::new();
            let n = v.read(&format!("{}/{}", docs, rel), &mut |b| {
                h.update(b);
                Ok(())
            }).unwrap();
            assert_eq!(n, *size, "{}", rel);
            if let Some(want) = e["sha256"].as_str() {
                assert_eq!(hex(&h.finalize()), want, "{} reads back as Windows wrote it", rel);
            }
        }
        // a compressed file is refused, never read as its packed bytes
        assert!(v.read(&format!("{}/compressed.txt", docs), &mut |_| Ok(())).is_err());
    }
}
