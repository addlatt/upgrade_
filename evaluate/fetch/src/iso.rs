//! A read-only ISO 9660 reader that works on a stream (roadmap item 24).
//!
//! The window downloads a release's ISO once and keeps none of it but the
//! files the stick needs. So this reader never seeks: it keeps the first
//! part of the stream in memory until it has read the disc's folders, then
//! hands each wanted file's bytes to its writer as they go past. It works
//! because a disc made by xorriso (Fedora's ISOs, measured 2026-11-08) puts
//! every folder before the first file's bytes; a disc whose folders come
//! later than `HEAD_LIMIT` is refused, never guessed at.
//!
//! Names are read from Rock Ridge (`NM`, the names `bsdtar` and
//! `measure-release.py` see) when the disc has them, else the plain ISO 9660
//! name without its `;1`. A wanted path must name exactly one plain file in
//! one extent; anything else refuses: a folder, a file split over several
//! extents (ISO 9660's way past 4 GiB), a name found twice, a name not found.

use std::io::Write;

pub const SECTOR: u64 = 2048;
/// How much of the stream may be held while the folders are read. Fedora 44's
/// folders end at sector 71 (144 KiB); 64 MiB is far past any disc we know.
pub const HEAD_LIMIT: u64 = 64 * 1024 * 1024;

/// One file the stick needs out of the ISO.
pub struct Want {
    /// the path on the disc, `/` separated, as `bsdtar` names it (`LiveOS/squashfs.img`)
    pub path: String,
    pub out: Box<dyn Write>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub path: String,
    pub start: u64,
    pub size: u64,
}

struct Slot {
    want: Want,
    found: Option<Found>,
    written: u64,
}

/// Asked once for every plain file on the disc, with its path and size:
/// a writer for it, or `None` to leave that file out.
pub type Maker = Box<dyn FnMut(&str, u64) -> Result<Option<Box<dyn Write>>, String>>;

pub struct Extractor {
    /// set for a disc taken whole (Ubuntu's, roadmap item 23): the files are
    /// not known until the folders are read
    every: Option<Maker>,
    slots: Vec<Slot>,
    head: Vec<u8>,
    /// stream bytes seen so far
    pos: u64,
    /// set once the folders are read and every wanted file is found
    ready: bool,
}

#[derive(Debug, Clone)]
struct Entry {
    name: String,
    extent: u64,
    size: u64,
    dir: bool,
    multi: bool,
    /// a Rock Ridge symbolic link: no bytes of its own
    link: bool,
}

/// Can a FAT32 stick, written from Windows, carry this path? A disc taken
/// whole is refused if one of its names cannot be kept as it is.
pub fn fat_safe(path: &str) -> bool {
    !path.is_empty()
        && path.split('/').all(|c| {
            let stem = c.split('.').next().unwrap_or("").to_ascii_uppercase();
            let reserved = ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str()) || (stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")) && stem.as_bytes()[3].is_ascii_digit());
            !c.is_empty() && c.len() <= 255 && c != "." && c != ".." && !c.ends_with('.') && !c.ends_with(' ') && !reserved && !c.chars().any(|ch| ch.is_control() || "<>:\"|?*\\".contains(ch))
        })
}

fn le32(b: &[u8]) -> u64 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as u64
}

/// The Rock Ridge name in a directory record's system use area, if any.
fn rock_ridge_name(su: &[u8]) -> Option<String> {
    let mut i = 0;
    let mut name: Vec<u8> = Vec::new();
    let mut found = false;
    while i + 4 <= su.len() {
        let (sig, len) = (&su[i..i + 2], su[i + 2] as usize);
        if len < 4 || i + len > su.len() {
            break;
        }
        if sig == b"NM" && len >= 5 {
            let flags = su[i + 4];
            // CURRENT (2) and PARENT (4) are the dot entries; skipped by the caller anyway
            if flags & 0x06 == 0 {
                name.extend_from_slice(&su[i + 5..i + len]);
                found = true;
            }
        }
        i += len;
    }
    if found { String::from_utf8(name).ok() } else { None }
}

/// Does the record's system use area say it is a symbolic link (Rock Ridge `SL`)?
fn rock_ridge_link(su: &[u8]) -> bool {
    let mut i = 0;
    while i + 4 <= su.len() {
        let len = su[i + 2] as usize;
        if len < 4 || i + len > su.len() {
            break;
        }
        if &su[i..i + 2] == b"SL" {
            return true;
        }
        i += len;
    }
    false
}

/// The records of one folder's extent.
fn records(data: &[u8]) -> Result<Vec<Entry>, String> {
    let mut out = vec![];
    let mut i = 0usize;
    while i < data.len() {
        let len = data[i] as usize;
        if len == 0 {
            // records never cross a sector: the rest of this sector is padding
            i = (i / SECTOR as usize + 1) * SECTOR as usize;
            continue;
        }
        if len < 34 || i + len > data.len() {
            return Err(format!("a folder record at byte {i} is damaged"));
        }
        let r = &data[i..i + len];
        i += len;
        let name_len = r[32] as usize;
        if 33 + name_len > len {
            return Err("a folder record's name runs past the record".into());
        }
        let raw = &r[33..33 + name_len];
        if raw == [0] || raw == [1] {
            continue;
        }
        let pad = if name_len % 2 == 0 { 1 } else { 0 };
        let su = if 33 + name_len + pad <= len { &r[33 + name_len + pad..] } else { &[][..] };
        let name = match rock_ridge_name(su) {
            Some(n) => n,
            None => {
                let s = String::from_utf8_lossy(raw).to_string();
                let s = s.split(';').next().unwrap_or("").to_string();
                s.strip_suffix('.').map(str::to_string).unwrap_or(s)
            }
        };
        let flags = r[25];
        out.push(Entry { name, extent: le32(&r[2..6]), size: le32(&r[10..14]), dir: flags & 0x02 != 0, multi: flags & 0x80 != 0, link: rock_ridge_link(su) });
    }
    Ok(out)
}

impl Extractor {
    pub fn new(wants: Vec<Want>) -> Extractor {
        Extractor { every: None, slots: wants.into_iter().map(|want| Slot { want, found: None, written: 0 }).collect(), head: Vec::new(), pos: 0, ready: false }
    }

    /// The whole disc: `make` is asked for a writer for every plain file,
    /// once the folders are read. Links are left out (FAT has none). A name
    /// a FAT32 stick cannot keep, two names that differ only in case, or a
    /// file split over several extents refuses the disc.
    pub fn every_file(make: Maker) -> Extractor {
        Extractor { every: Some(make), slots: Vec::new(), head: Vec::new(), pos: 0, ready: false }
    }

    /// The files found on the disc, once the folders are read.
    pub fn found(&self) -> Vec<Found> {
        self.slots.iter().filter_map(|s| s.found.clone()).collect()
    }

    pub fn ready(&self) -> bool {
        self.ready
    }

    /// Try to read the whole folder tree out of `head`. Ok(false): more
    /// bytes are needed. Ok(true): every wanted file is found.
    fn try_tree(&mut self) -> Result<bool, String> {
        let h = &self.head;
        let need = |end: u64| end as usize <= h.len();
        if !need(17 * SECTOR) {
            return Ok(false);
        }
        let pvd = &h[(16 * SECTOR) as usize..(17 * SECTOR) as usize];
        if pvd[0] != 1 || &pvd[1..6] != b"CD001" {
            return Err("this download is not an ISO 9660 disc image".into());
        }
        if le32(&pvd[128..132]) & 0xffff != SECTOR {
            return Err("this disc image does not use 2048-byte blocks".into());
        }
        let root = &pvd[156..190];
        let mut stack = vec![(String::new(), le32(&root[2..6]), le32(&root[10..14]))];
        let mut files: Vec<(String, Entry)> = vec![];
        let mut seen_dirs = 0usize;
        while let Some((prefix, extent, size)) = stack.pop() {
            seen_dirs += 1;
            if seen_dirs > 100_000 {
                return Err("this disc image has too many folders".into());
            }
            let (start, end) = (extent * SECTOR, extent * SECTOR + size);
            if end > HEAD_LIMIT {
                return Err(format!("this disc image keeps a folder past the first {} MiB; refused rather than guessed at", HEAD_LIMIT >> 20));
            }
            if !need(end) {
                return Ok(false);
            }
            for e in records(&h[start as usize..end as usize])? {
                let path = if prefix.is_empty() { e.name.clone() } else { format!("{prefix}/{}", e.name) };
                if e.dir {
                    stack.push((path, e.extent, e.size));
                } else {
                    files.push((path, e));
                }
            }
        }
        if let Some(make) = self.every.as_mut() {
            files.sort_by(|a, b| a.0.cmp(&b.0));
            let mut seen: Vec<String> = Vec::new();
            let mut slots = Vec::new();
            for (p, e) in files.iter().filter(|(_, e)| !e.link) {
                if !fat_safe(p) {
                    return Err(format!("{p}: a FAT32 stick cannot keep this name; refused"));
                }
                let lower = p.to_lowercase();
                if seen.contains(&lower) {
                    return Err(format!("{p} is on this disc image twice, or under two names that differ only in case; refused"));
                }
                seen.push(lower);
                if e.multi {
                    return Err(format!("{p} is split over several parts of the disc image (a file past 4 GiB); this reader refuses it"));
                }
                if let Some(out) = make(p, e.size)? {
                    slots.push(Slot { want: Want { path: p.clone(), out }, found: Some(Found { path: p.clone(), start: e.extent * SECTOR, size: e.size }), written: 0 });
                }
            }
            if slots.is_empty() {
                return Err("this disc image holds no files".into());
            }
            self.slots = slots;
            return Ok(true);
        }
        for slot in &mut self.slots {
            let want = slot.want.path.trim_matches('/');
            let exact: Vec<&(String, Entry)> = files.iter().filter(|(p, _)| p == want).collect();
            let hits = if exact.is_empty() { files.iter().filter(|(p, _)| p.eq_ignore_ascii_case(want)).collect() } else { exact };
            match hits.as_slice() {
                [] => return Err(format!("{want} is not on this disc image")),
                [(p, e)] => {
                    if e.multi {
                        return Err(format!("{p} is split over several parts of the disc image (a file past 4 GiB); this reader refuses it"));
                    }
                    if e.size == 0 {
                        return Err(format!("{p} is empty on this disc image"));
                    }
                    slot.found = Some(Found { path: p.clone(), start: e.extent * SECTOR, size: e.size });
                }
                _ => return Err(format!("{want} is on this disc image more than once")),
            }
        }
        Ok(true)
    }

    /// Hand the bytes in `chunk`, at stream offset `at`, to each file they belong to.
    fn deliver(slots: &mut [Slot], at: u64, chunk: &[u8]) -> Result<(), String> {
        let end = at + chunk.len() as u64;
        for s in slots.iter_mut() {
            let Some(f) = &s.found else { continue };
            let (fs, fe) = (f.start + s.written, f.start + f.size);
            // only the next byte the file needs is accepted, so each byte is written once and in order
            if fs >= end || fs < at || fs >= fe {
                continue;
            }
            let lo = (fs - at) as usize;
            let hi = (fe.min(end) - at) as usize;
            s.want.out.write_all(&chunk[lo..hi]).map_err(|e| format!("{}: could not write ({e})", f.path))?;
            s.written += (hi - lo) as u64;
        }
        Ok(())
    }

    /// The next bytes of the stream.
    pub fn feed(&mut self, chunk: &[u8]) -> Result<(), String> {
        let at = self.pos;
        self.pos += chunk.len() as u64;
        if self.ready {
            return Self::deliver(&mut self.slots, at, chunk);
        }
        self.head.extend_from_slice(chunk);
        if self.try_tree()? {
            self.ready = true;
            // the held bytes are the stream from its start, so a file whose
            // bytes began before the folders were complete is still whole
            let head = std::mem::take(&mut self.head);
            Self::deliver(&mut self.slots, 0, &head)?;
        } else if self.head.len() as u64 > HEAD_LIMIT + SECTOR * 17 {
            return Err(format!("this disc image's folders were not complete in the first {} MiB", HEAD_LIMIT >> 20));
        }
        Ok(())
    }

    /// The stream has ended: every wanted file must be whole.
    pub fn finish(mut self) -> Result<Vec<Found>, String> {
        if !self.ready {
            return Err("the download ended before the disc image's folders were read".into());
        }
        for s in &mut self.slots {
            let f = s.found.as_ref().expect("ready means found");
            if s.written != f.size {
                return Err(format!("the download ended early: {} has {} of {} bytes", f.path, s.written, f.size));
            }
            s.want.out.flush().map_err(|e| format!("{}: could not write ({e})", f.path))?;
        }
        Ok(self.found())
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// A made-up disc: a root folder holding `files` (name, bytes) and one
    /// sub-folder `SUB` holding `sub`. Names carry Rock Ridge NM entries
    /// when `rr`. `late_dirs` puts the sub-folder after the files' bytes.
    pub fn make_iso(files: &[(&str, &[u8])], sub: &[(&str, &[u8])], rr: bool, late_dirs: bool) -> Vec<u8> {
        make_iso_named("Sub", files, sub, rr, late_dirs)
    }

    /// The same disc with its sub-folder called `folder`.
    pub fn make_iso_named(folder: &str, files: &[(&str, &[u8])], sub: &[(&str, &[u8])], rr: bool, late_dirs: bool) -> Vec<u8> {
        fn rec(name: &str, extent: u64, size: u64, dir: bool, rr: bool) -> Vec<u8> {
            let iso_name: Vec<u8> = if name == "\0" || name == "\x01" { name.as_bytes().to_vec() } else if dir { name.to_uppercase().into_bytes() } else { format!("{};1", name.to_uppercase()).into_bytes() };
            let pad = if iso_name.len() % 2 == 0 { 1 } else { 0 };
            let mut su = vec![];
            if rr && name != "\0" && name != "\x01" {
                su.extend_from_slice(b"NM");
                su.push((5 + name.len()) as u8);
                su.push(1);
                su.push(0);
                su.extend_from_slice(name.as_bytes());
            }
            let len = 33 + iso_name.len() + pad + su.len();
            let mut r = vec![0u8; len];
            r[0] = len as u8;
            r[2..6].copy_from_slice(&(extent as u32).to_le_bytes());
            r[10..14].copy_from_slice(&(size as u32).to_le_bytes());
            r[25] = if dir { 2 } else { 0 };
            r[32] = iso_name.len() as u8;
            r[33..33 + iso_name.len()].copy_from_slice(&iso_name);
            r[33 + iso_name.len() + pad..].copy_from_slice(&su);
            r
        }
        let sectors = |n: usize| n.div_ceil(SECTOR as usize).max(1) as u64;
        let root_lba = 18u64;
        let mut next = if late_dirs { 19 } else { 20 };
        let mut placed = vec![];
        for (n, b) in files.iter().chain(sub.iter()) {
            placed.push((n.to_string(), next, b.to_vec()));
            next += sectors(b.len());
        }
        let sub_lba = if late_dirs { next } else { 19 };
        let total = (sub_lba.max(next) + 1) * SECTOR;
        let mut img = vec![0u8; total as usize];
        let pvd = (16 * SECTOR) as usize;
        img[pvd] = 1;
        img[pvd + 1..pvd + 6].copy_from_slice(b"CD001");
        img[pvd + 128..pvd + 130].copy_from_slice(&(SECTOR as u16).to_le_bytes());
        let root_rec = rec("\0", root_lba, SECTOR, true, false);
        img[pvd + 156..pvd + 156 + 34].copy_from_slice(&root_rec[..34]);
        let mut root = rec("\0", root_lba, SECTOR, true, false);
        root.extend(rec("\x01", root_lba, SECTOR, true, false));
        for (i, (n, _)) in files.iter().enumerate() {
            root.extend(rec(n, placed[i].1, placed[i].2.len() as u64, false, rr));
        }
        root.extend(rec(folder, sub_lba, SECTOR, true, rr));
        let r0 = (root_lba * SECTOR) as usize;
        img[r0..r0 + root.len()].copy_from_slice(&root);
        let mut sd = rec("\0", sub_lba, SECTOR, true, false);
        sd.extend(rec("\x01", root_lba, SECTOR, true, false));
        for (i, (n, _)) in sub.iter().enumerate() {
            let p = &placed[files.len() + i];
            sd.extend(rec(n, p.1, p.2.len() as u64, false, rr));
        }
        let s0 = (sub_lba * SECTOR) as usize;
        img[s0..s0 + sd.len()].copy_from_slice(&sd);
        for (_, lba, b) in &placed {
            let o = (*lba * SECTOR) as usize;
            img[o..o + b.len()].copy_from_slice(b);
        }
        img
    }

    #[derive(Clone, Default)]
    pub struct Sink(pub Rc<RefCell<Vec<u8>>>);
    impl Write for Sink {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.borrow_mut().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn run(img: &[u8], wants: &[&str], chunk: usize) -> Result<Vec<Vec<u8>>, String> {
        let sinks: Vec<Sink> = wants.iter().map(|_| Sink::default()).collect();
        let mut x = Extractor::new(wants.iter().zip(&sinks).map(|(p, s)| Want { path: p.to_string(), out: Box::new(s.clone()) }).collect());
        for c in img.chunks(chunk) {
            x.feed(c)?;
        }
        x.finish()?;
        Ok(sinks.iter().map(|s| s.0.borrow().clone()).collect())
    }

    #[test]
    fn files_come_out_whole_in_any_chunk_size() {
        let big: Vec<u8> = (0..10_000u32).map(|i| (i % 251) as u8).collect();
        let img = make_iso(&[("BOOTX64.EFI", b"shim bytes"), ("grubx64.efi", &big)], &[("squashfs.img", b"the image")], true, false);
        for chunk in [1, 7, 2048, 4096, 65536, img.len()] {
            let got = run(&img, &["BOOTX64.EFI", "grubx64.efi", "Sub/squashfs.img"], chunk).unwrap();
            assert_eq!(got[0], b"shim bytes", "chunk {chunk}");
            assert_eq!(got[1], big, "chunk {chunk}");
            assert_eq!(got[2], b"the image", "chunk {chunk}");
        }
    }

    #[test]
    fn rock_ridge_names_are_exact_and_plain_names_match_without_case() {
        let rr = make_iso(&[("grubx64.efi", b"g")], &[], true, false);
        assert_eq!(run(&rr, &["grubx64.efi"], 4096).unwrap()[0], b"g");
        assert_eq!(run(&rr, &["GRUBX64.EFI"], 4096).unwrap()[0], b"g", "case-blind when no exact name");
        let plain = make_iso(&[("vmlinuz", b"k")], &[], false, false);
        assert_eq!(run(&plain, &["vmlinuz"], 4096).unwrap()[0], b"k", "VMLINUZ;1 read as vmlinuz");
    }

    #[test]
    fn a_missing_file_a_short_stream_and_not_an_iso_refuse() {
        let img = make_iso(&[("a.efi", b"aaaa")], &[], true, false);
        assert!(run(&img, &["b.efi"], 4096).unwrap_err().contains("not on this disc image"));
        let short = &img[..(20 * SECTOR + 2) as usize];
        assert!(run(short, &["a.efi"], 4096).unwrap_err().contains("ended early"));
        assert!(run(&img[..10 * SECTOR as usize], &["a.efi"], 4096).unwrap_err().contains("before the disc image's folders"));
        assert!(run(&vec![0u8; 40 * SECTOR as usize], &["a.efi"], 4096).unwrap_err().contains("not an ISO 9660"));
    }

    #[test]
    fn folders_after_the_files_still_read_while_held_in_memory() {
        let img = make_iso(&[("a.efi", b"aaaa")], &[("b.img", b"bbbb")], true, true);
        let got = run(&img, &["a.efi", "Sub/b.img"], 2048).unwrap();
        assert_eq!((got[0].as_slice(), got[1].as_slice()), (&b"aaaa"[..], &b"bbbb"[..]));
    }

    #[test]
    fn a_split_file_and_a_folder_refuse() {
        let mut img = make_iso(&[("big.img", b"x")], &[], true, false);
        // set the multi-extent flag on the file's record
        let r0 = (18 * SECTOR) as usize;
        let mut i = r0;
        loop {
            let len = img[i] as usize;
            let name_len = img[i + 32] as usize;
            if &img[i + 33..i + 33 + name_len] == b"BIG.IMG;1" {
                img[i + 25] |= 0x80;
                break;
            }
            i += len;
        }
        assert!(run(&img, &["big.img"], 4096).unwrap_err().contains("split over several parts"));
        let img = make_iso(&[("a", b"a")], &[], true, false);
        assert!(run(&img, &["Sub"], 4096).unwrap_err().contains("not on this disc image"), "a folder is not a file");
    }

    #[test]
    fn a_whole_disc_gives_every_file_once_and_leaves_out_what_the_maker_declines() {
        let big: Vec<u8> = (0..9_000u32).map(|i| (i % 253) as u8).collect();
        let img = make_iso(&[("vmlinuz", &big), ("grub.cfg", b"theirs"), ("empty", b"")], &[("minimal.squashfs", b"layer")], true, false);
        for chunk in [1, 2048, 5000, img.len()] {
            let got: Rc<RefCell<Vec<(String, u64, Sink)>>> = Rc::default();
            let g = got.clone();
            let mut x = Extractor::every_file(Box::new(move |p, size| {
                if p == "grub.cfg" {
                    return Ok(None);
                }
                let s = Sink::default();
                g.borrow_mut().push((p.to_string(), size, s.clone()));
                Ok(Some(Box::new(s) as Box<dyn Write>))
            }));
            for c in img.chunks(chunk) {
                x.feed(c).unwrap();
            }
            assert_eq!(x.finish().unwrap().len(), 3);
            let got = got.borrow();
            let names: Vec<&str> = got.iter().map(|g| g.0.as_str()).collect();
            assert_eq!(names, ["Sub/minimal.squashfs", "empty", "vmlinuz"], "chunk {chunk}");
            assert_eq!(*got[0].2 .0.borrow(), b"layer");
            assert!(got[1].2 .0.borrow().is_empty() && got[1].1 == 0);
            assert_eq!(*got[2].2 .0.borrow(), big);
        }
    }

    #[test]
    fn a_whole_disc_with_a_name_the_stick_cannot_keep_refuses() {
        let take = |img: &[u8]| {
            let mut x = Extractor::every_file(Box::new(|_, _| Ok(Some(Box::new(Sink::default()) as Box<dyn Write>))));
            x.feed(img).and_then(|_| x.finish().map(|_| ()))
        };
        assert!(take(&make_iso(&[("a:b", b"x")], &[], true, false)).unwrap_err().contains("cannot keep this name"));
        assert!(take(&make_iso(&[("Readme", b"x"), ("README", b"y")], &[], true, false)).unwrap_err().contains("differ only in case"));
        for (name, ok) in [("casper/minimal.squashfs", true), (".disk/info", true), ("pool/main/a/b_1.0~rc1+2_amd64.deb", true), ("a/CON", false), ("a/con.txt", false), ("a/b.", false), ("a/b ", false), ("a//b", false), ("a/b?c", false), ("a/../b", false), ("COM1", false), ("COMA", true)] {
            assert_eq!(fat_safe(name), ok, "{name}");
        }
    }

    /// The real Ubuntu ISO, when it is on this machine (UPG_UBUNTU_ISO): taken whole as a
    /// download is, every layer comes out with the sha256 in data/measured/, links left out.
    #[test]
    fn ubuntu_from_its_iso_if_present() {
        use sha2::{Digest, Sha256};
        let Ok(iso) = std::env::var("UPG_UBUNTU_ISO") else {
            eprintln!("skipped: UPG_UBUNTU_ISO is not set");
            return;
        };
        let table: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/releases.json")).unwrap()).unwrap();
        let measured = table["Releases"].as_array().unwrap().iter().find(|r| r["Id"] == "ubuntu-26.04.1").unwrap().clone();
        #[derive(Clone, Default)]
        struct H(Rc<RefCell<(Sha256, u64)>>);
        impl Write for H {
            fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                let mut s = self.0.borrow_mut();
                s.0.update(b);
                s.1 += b.len() as u64;
                Ok(b.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let got: Rc<RefCell<Vec<(String, u64, H)>>> = Rc::default();
        let g = got.clone();
        let mut x = Extractor::every_file(Box::new(move |p, size| {
            let h = H::default();
            g.borrow_mut().push((p.to_string(), size, h.clone()));
            Ok(Some(Box::new(h) as Box<dyn Write>))
        }));
        let mut r = std::io::BufReader::with_capacity(1 << 20, std::fs::File::open(&iso).unwrap());
        let mut buf = vec![0u8; 1 << 20];
        loop {
            let n = std::io::Read::read(&mut r, &mut buf).unwrap();
            if n == 0 {
                break;
            }
            x.feed(&buf[..n]).unwrap();
        }
        x.finish().unwrap();
        let got = got.borrow();
        eprintln!("{iso}: {} files", got.len());
        // the plan's side of it: the kit has a place for every file but the one it leaves out,
        // and every file the plan checks is on the disc
        let plan = crate::plan::plan(&crate::plan::tests::ubuntu_table(), "ubuntu-26.04.1", "gnome").unwrap();
        let whole = plan.parts[0].whole.as_ref().unwrap();
        for (p, _, _) in got.iter().filter(|(p, _, _)| !whole.skip.contains(p)) {
            let dest = crate::plan::dest_of(p);
            assert!(crate::stick::safe_rel(&dest) && crate::plan::release_owned(&dest), "{p} has no place on the stick");
        }
        for (p, want) in &whole.checks {
            let (_, _, h) = got.iter().find(|(q, _, _)| q == p).unwrap_or_else(|| panic!("{p} is not on the disc"));
            assert_eq!(&format!("{:x}", h.0.borrow().0.clone().finalize()), want, "{p}");
        }
        assert!(whole.skip.iter().all(|s| got.iter().any(|(p, _, _)| p == s)), "the file the kit leaves out is on the disc");
        let layers = measured["Layers"].as_array().unwrap();
        assert!(layers.len() > 10);
        for l in layers {
            let path = l["Path"].as_str().unwrap();
            let (_, size, h) = got.iter().find(|g| g.0 == path).unwrap_or_else(|| panic!("{path} did not come out"));
            assert_eq!(*size, l["Size"].as_u64().unwrap(), "{path}");
            assert_eq!(h.0.borrow().1, *size, "{path}");
            assert_eq!(format!("{:x}", h.0.borrow().0.clone().finalize()), l["Sha256"].as_str().unwrap(), "{path}");
        }
        for must in ["casper/vmlinuz", "casper/initrd", "EFI/boot/bootx64.efi", "EFI/boot/grubx64.efi", "boot/grub/grub.cfg", ".disk/info", "casper/install-sources.yaml"] {
            assert!(got.iter().any(|g| g.0 == must), "{must} did not come out");
        }
    }

    /// The real Fedora 44 ISOs, when the rig has fetched them (rig/vm/fetch-release.sh;
    /// a worktree points at the main checkout's with UPG_RELEASE_ISOS): every file comes
    /// out with the sha256 measured into data/releases.json, streamed as a download is.
    #[test]
    fn fedora_44_from_the_rigs_isos_if_present() {
        use sha2::{Digest, Sha256};
        let dir = std::env::var("UPG_RELEASE_ISOS").map(std::path::PathBuf::from).unwrap_or_else(|_| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rig/vm/artifacts/releases/fedora-44/iso"));
        #[derive(Clone, Default)]
        struct H(Rc<RefCell<(Sha256, u64)>>);
        impl Write for H {
            fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                let mut s = self.0.borrow_mut();
                s.0.update(b);
                s.1 += b.len() as u64;
                Ok(b.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let cases: [(&str, &[(&str, Option<&str>)]); 2] = [
            ("Fedora-Server-netinst-x86_64-44-1.7.iso", &[
                ("EFI/BOOT/BOOTX64.EFI", Some("571ea56b855dcf73bec6acb63c5ded44c2a191138bca0d8cfa5aa93f60f46fff")),
                ("EFI/BOOT/grubx64.efi", Some("51d290821cd0df32de5c2462cd63252276b63ef8dfc53fda79855fe51c9d1e45")),
                ("images/pxeboot/vmlinuz", None),
                ("images/pxeboot/initrd.img", None),
                ("images/install.img", None),
            ]),
            ("Fedora-KDE-Desktop-Live-44-1.7.x86_64.iso", &[("LiveOS/squashfs.img", Some("2b6c1d5727467be858481f16a9131f16fbcaf0cf77bf57139987446825a71330"))]),
        ];
        for (iso, names) in cases {
            let Ok(f) = std::fs::File::open(dir.join(iso)) else {
                eprintln!("skipped: {} is not here", dir.join(iso).display());
                continue;
            };
            let hs: Vec<H> = names.iter().map(|_| H::default()).collect();
            let mut x = Extractor::new(names.iter().zip(&hs).map(|((n, _), h)| Want { path: n.to_string(), out: Box::new(h.clone()) }).collect());
            let mut r = std::io::BufReader::with_capacity(1 << 20, f);
            let mut buf = vec![0u8; 1 << 20];
            loop {
                let n = std::io::Read::read(&mut r, &mut buf).unwrap();
                if n == 0 {
                    break;
                }
                x.feed(&buf[..n]).unwrap();
            }
            let found = x.finish().unwrap();
            for (((n, want), h), f) in names.iter().zip(&hs).zip(&found) {
                let got = format!("{:x}", h.0.borrow().0.clone().finalize());
                eprintln!("{iso}: {} ({} bytes) {got}", f.path, h.0.borrow().1);
                assert_eq!(h.0.borrow().1, f.size, "{n}");
                if let Some(w) = want {
                    assert_eq!(&got, w, "{n}");
                }
            }
        }
    }
}
